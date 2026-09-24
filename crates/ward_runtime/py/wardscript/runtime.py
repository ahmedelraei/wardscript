"""Runtime configuration, and the operations generated code calls that depend on it:
model calls, approvals and tools."""

from __future__ import annotations

import asyncio
import concurrent.futures
import inspect
import json
from dataclasses import dataclass, field
from typing import Any, AsyncIterator, Awaitable, Callable, Iterable, Mapping, Union

from . import audit, budget
from .errors import (
    AiOutputError,
    ApprovalDenied,
    DecodeError,
    NoModelError,
    Thrown,
    ToolError,
    TrustError,
)
from .model import AiRequest, Completion, Model, StreamChunk, estimate_tokens
from .schema import Type, decode, json_schema


@dataclass(frozen=True)
class ApprovalRequest:
    value: Any
    #: Where `approve` was called, e.g. `support.wardscript:58:24`.
    site: str
    #: The run it's part of; `ward trace show <run>` shows how the value was made.
    run: str | None = None


#: Returns whether a human approved the value, directly or from a coroutine.
Approver = Callable[[ApprovalRequest], Union[bool, Awaitable[bool]]]

#: Sees a model's answer as it streams in.
StreamObserver = Callable[[StreamChunk], None]


@dataclass
class Config:
    model: Model | None = None
    approver: Approver | None = None
    #: Tool implementations by import source: `import mcp "gmail"` looks up `"gmail"`.
    #: Each is a mapping of functions, or an object with a method per tool function.
    tools: dict[str, Any] = field(default_factory=dict)
    #: How many times a model call is retried after an invalid answer.
    retries: int = 2
    #: Where each run's audit trace is written; else `WARD_TRACE_DIR`, else nowhere
    #: (the last run is still in `wardscript.runtime.last_run()`).
    trace_dir: str | None = None
    #: Also sends each run to this OTLP/HTTP collector (e.g. `http://localhost:4318`);
    #: else `OTEL_EXPORTER_OTLP_ENDPOINT`, else nowhere.
    otlp_endpoint: str | None = None
    #: Refuse a tool call whose argument is exactly an unchecked untrusted value.
    check_sinks: bool = True
    #: Streams answers from models that can, passing each piece here.
    on_stream: StreamObserver | None = None


_config = Config()
_UNSET: Any = object()


def configure(
    *,
    model: Model | None = _UNSET,
    approver: Approver | None = _UNSET,
    tools: Mapping[str, Any] = _UNSET,
    retries: int = _UNSET,
    trace_dir: str | None = _UNSET,
    otlp_endpoint: str | None = _UNSET,
    check_sinks: bool = _UNSET,
    on_stream: StreamObserver | None = _UNSET,
) -> None:
    """Sets the runtime's configuration. Arguments left out keep their current value."""
    if model is not _UNSET:
        _config.model = model
    if approver is not _UNSET:
        _config.approver = approver
    if tools is not _UNSET:
        _config.tools = dict(tools)
    if retries is not _UNSET:
        if retries < 0:
            raise ValueError("retries must be at least 0")
        _config.retries = retries
    if trace_dir is not _UNSET:
        _config.trace_dir = None if trace_dir is None else str(trace_dir)
    if otlp_endpoint is not _UNSET:
        _config.otlp_endpoint = otlp_endpoint
    if check_sinks is not _UNSET:
        _config.check_sinks = bool(check_sinks)
    if on_stream is not _UNSET:
        _config.on_stream = on_stream


def last_run() -> audit.Run | None:
    """The most recent finished run: its `id`, trace `path` and `records`."""
    return audit.last()


def _resolve(value: Any) -> Any:
    """Waits for an async hook's result. Generated code is synchronous, so a coroutine
    runs on its own event loop, in a worker thread when one is already running here."""
    if not inspect.isawaitable(value):
        return value

    async def wait() -> Any:
        return await value

    try:
        asyncio.get_running_loop()
    except RuntimeError:
        return asyncio.run(wait())
    with concurrent.futures.ThreadPoolExecutor(1) as pool:
        return pool.submit(asyncio.run, wait()).result()


def reset() -> None:
    """Restores the default configuration."""
    global _config
    _config = Config()


def config() -> Config:
    return _config


class _Attempt:
    """One model request of an `ai fn` call: what goes before and after asking the model,
    shared by the sync and async paths."""

    def __init__(self, function: str, prompt: str, returns: Type, errors: list[str]) -> None:
        model = _config.model
        if model is None:
            raise NoModelError(
                f"`{function}` needs a model; call wardscript.runtime.configure(model=...) first"
            )
        self.model = model
        self.returns = returns
        self.request = AiRequest(function, prompt, json_schema(returns), len(errors), tuple(errors))
        self.text = ""
        budget.before_model_call()
        self.started = audit.now()

    def streams(self) -> bool:
        """Stream when the model can and something wants the pieces: an observer, or a
        token budget that can stop the answer early."""
        return callable(getattr(self.model, "stream", None)) and (
            _config.on_stream is not None or budget.limits_tokens()
        )

    def feed(self, chunk: str | Completion) -> Completion | None:
        """Takes a piece of a streamed answer; a `Completion` ends it."""
        if isinstance(chunk, Completion):
            return Completion(chunk.text or self.text, chunk.tokens, chunk.cost)
        self.text += chunk
        if _config.on_stream is not None:
            _config.on_stream(StreamChunk(self.request.function, self.request.attempt, chunk, self.text))
        so_far = estimate_tokens(self.request.prompt) + estimate_tokens(self.text)
        if budget.tokens_over(so_far):
            # Stop reading; charging what was produced raises `BudgetExceeded`.
            self.finish(Completion(self.text, so_far))
        return None

    def finish(self, answer: str | Completion) -> tuple[bool, Any, str | None]:
        """Charges the answer and decodes it: `(ok, value, error)`."""
        if isinstance(answer, Completion):
            text, tokens, cost = answer.text, answer.tokens, answer.cost
        else:
            text, tokens, cost = answer, None, 0.0
        if tokens is None:
            tokens = estimate_tokens(self.request.prompt) + estimate_tokens(str(text))
        run = audit.current()
        if run is not None:
            run.calls += 1
            run.tokens += tokens
            run.cost += cost
        error, value, ok = None, None, False
        try:
            value = decode(self.returns, json.loads(text))
            ok = True
        except (json.JSONDecodeError, TypeError) as e:
            error = f"the answer is not valid JSON ({e})"
        except DecodeError as e:
            error = str(e)
        audit.record(
            "ai_call",
            started=self.started,
            function=self.request.function,
            attempt=self.request.attempt,
            prompt=self.request.prompt,
            answer=str(text),
            tokens=float(tokens),
            cost=float(cost),
            error=error,
            leaves=audit.leaves(value) if ok else [],
        )
        budget.after_model_call(tokens, cost)
        return ok, value, error


def _collect(attempt: _Attempt, chunks: Iterable[str | Completion]) -> Completion:
    try:
        for chunk in chunks:
            done = attempt.feed(chunk)
            if done is not None:
                return done
    finally:
        close = getattr(chunks, "close", None)
        if callable(close):
            close()
    return Completion(attempt.text)


async def _collect_async(attempt: _Attempt, chunks: AsyncIterator[str | Completion]) -> Completion:
    try:
        async for chunk in chunks:
            done = attempt.feed(chunk)
            if done is not None:
                return done
    finally:
        close = getattr(chunks, "aclose", None)
        if callable(close):
            await close()
    return Completion(attempt.text)


def _ask(attempt: _Attempt) -> str | Completion:
    if attempt.streams():
        chunks = attempt.model.stream(attempt.request)  # type: ignore[attr-defined]
        if hasattr(chunks, "__aiter__"):
            return _resolve(_collect_async(attempt, chunks))
        return _collect(attempt, chunks)
    return _resolve(attempt.model.complete(attempt.request))


async def _ask_async(attempt: _Attempt) -> str | Completion:
    if attempt.streams():
        chunks = attempt.model.stream(attempt.request)  # type: ignore[attr-defined]
        if hasattr(chunks, "__aiter__"):
            return await _collect_async(attempt, chunks)
        return _collect(attempt, chunks)
    answer = attempt.model.complete(attempt.request)
    return await answer if inspect.isawaitable(answer) else answer


def ai(function: str, prompt: str, returns: Type) -> Any:
    """Calls the model for `ai fn function` and decodes its answer as `returns`,
    retrying with the error when the answer doesn't fit."""
    errors: list[str] = []
    for _ in range(_config.retries + 1):
        attempt = _Attempt(function, prompt, returns, errors)
        ok, value, error = attempt.finish(_ask(attempt))
        if ok:
            return value
        errors.append(error or "")
    raise AiOutputError(function, errors)


async def ai_async(function: str, prompt: str, returns: Type) -> Any:
    errors: list[str] = []
    for _ in range(_config.retries + 1):
        attempt = _Attempt(function, prompt, returns, errors)
        ok, value, error = attempt.finish(await _ask_async(attempt))
        if ok:
            return value
        errors.append(error or "")
    raise AiOutputError(function, errors)


def _validated(value: Any, passed: bool, rule_name: str, site: str) -> Any:
    audit.record("validate", rule=rule_name, site=site, passed=passed, leaves=audit.leaves(value))
    if passed:
        return value
    raise Thrown(f"validation failed: `{rule_name}` rejected the value")


def validate(value: Any, rule: Callable[[Any], bool], rule_name: str, site: str = "?") -> Any:
    return _validated(value, bool(_resolve(rule(value))), rule_name, site)


async def validate_async(
    value: Any, rule: Callable[[Any], Any], rule_name: str, site: str = "?"
) -> Any:
    passed = rule(value)
    if inspect.isawaitable(passed):
        passed = await passed
    return _validated(value, bool(passed), rule_name, site)


def _approver(site: str) -> Approver:
    approver = _config.approver
    if approver is None:
        raise ApprovalDenied(
            f"approval needed at {site}, but no approver is configured; "
            "call wardscript.runtime.configure(approver=...)"
        )
    return approver


def _request(value: Any, site: str) -> ApprovalRequest:
    run = audit.current()
    return ApprovalRequest(value, site, run.id if run else None)


def _approved(value: Any, approved: bool, site: str) -> Any:
    audit.record("approve", site=site, approved=approved, leaves=audit.leaves(value))
    if not approved:
        raise ApprovalDenied(f"approval denied at {site}")
    return value


def approve(value: Any, site: str) -> Any:
    approver = _approver(site)
    return _approved(value, bool(_resolve(approver(_request(value, site)))), site)


async def approve_async(value: Any, site: str) -> Any:
    answer = _approver(site)(_request(value, site))
    if inspect.isawaitable(answer):
        answer = await answer
    return _approved(value, bool(answer), site)


def declassify(value: Any, reason: str, site: str = "?") -> Any:
    audit.record("declassify", site=site, reason=reason, leaves=audit.leaves(value))
    return value


def _tool(source: str, name: str) -> Callable[..., Any]:
    impl = _config.tools.get(source)
    if impl is None:
        raise ToolError(
            f"tool `{source}` is not configured; "
            f"call wardscript.runtime.configure(tools={{{source!r}: ...}})"
        )
    fn = impl.get(name) if isinstance(impl, Mapping) else getattr(impl, name, None)
    if fn is None:
        raise ToolError(f"tool `{source}` has no function `{name}`")
    return fn


class _ToolCall:
    def __init__(self, source: str, name: str, site: str, args: tuple[Any, ...]) -> None:
        budget.check_time()
        self.source, self.name, self.site, self.args = source, name, site, args
        self.fn = _tool(source, name)
        self.started = audit.now()
        try:
            for i, arg in enumerate(args):
                audit.check_sink(f"{source}.{name}", i, arg)
        except TrustError as e:
            self.done(None, f"TrustError: {e}")
            raise

    def done(self, result: Any, error: str | None) -> None:
        audit.record(
            "tool_call",
            started=self.started,
            tool=self.source,
            function=self.name,
            site=self.site,
            args=[audit.to_json(a) for a in self.args],
            digests=[audit.digest(a) for a in self.args],
            error=error,
            leaves=audit.leaves(result) if error is None else [],
        )
        budget.check_time()


def call_tool(source: str, name: str, site: str, *args: Any) -> Any:
    call = _ToolCall(source, name, site, args)
    try:
        result = _resolve(call.fn(*args))
    except Exception as e:
        call.done(None, f"{type(e).__name__}: {e}")
        raise
    call.done(result, None)
    return result


async def call_tool_async(source: str, name: str, site: str, *args: Any) -> Any:
    call = _ToolCall(source, name, site, args)
    try:
        result = call.fn(*args)
        if inspect.isawaitable(result):
            result = await result
    except Exception as e:
        call.done(None, f"{type(e).__name__}: {e}")
        raise
    call.done(result, None)
    return result
