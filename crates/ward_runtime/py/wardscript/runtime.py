"""Runtime configuration, and the operations generated code calls that depend on it:
model calls, approvals and tools."""

from __future__ import annotations

import asyncio
import concurrent.futures
import inspect
import json
import time
from dataclasses import dataclass, field
from typing import Any, AsyncIterator, Awaitable, Callable, Generator, Iterable, Mapping, Union

from . import audit, budget
from .errors import (
    AiOutputError,
    ApprovalDenied,
    DecodeError,
    ModelError,
    NoModelError,
    Thrown,
    ToolError,
    TrustError,
)
from .model import AiRequest, Completion, Model, StreamChunk, estimate_tokens
from .schema import Type, decode, encode, json_schema


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
    #: The model an `ai fn` asks when its `model {...}` clause names no `primary`.
    model: Model | None = None
    #: Models by the aliases `model {primary: fast, fallback: smart}` uses.
    models: dict[str, Model] = field(default_factory=dict)
    #: Retries of a request that failed with a retryable `ModelError` (a rate limit,
    #: a timeout), unless the `ai fn`'s `model {retries: ...}` says otherwise.
    model_retries: int = 2
    #: Seconds before the first such retry; each next one waits twice as long.
    backoff: float = 1.0
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
    #: What to do when a `cost` budget meets a model whose cost is unknown: `"error"`
    #: raises `BudgetUnenforceable` (before the request when the model has no prices);
    #: `"warn"` warns once per function and counts the call as free.
    unpriced: str = "error"


_config = Config()
_UNSET: Any = object()


def configure(
    *,
    model: Model | None = _UNSET,
    models: Mapping[str, Model] = _UNSET,
    model_retries: int = _UNSET,
    backoff: float = _UNSET,
    approver: Approver | None = _UNSET,
    tools: Mapping[str, Any] = _UNSET,
    retries: int = _UNSET,
    trace_dir: str | None = _UNSET,
    otlp_endpoint: str | None = _UNSET,
    check_sinks: bool = _UNSET,
    on_stream: StreamObserver | None = _UNSET,
    unpriced: str = _UNSET,
) -> None:
    """Sets the runtime's configuration. Arguments left out keep their current value."""
    if model is not _UNSET:
        _config.model = model
    if models is not _UNSET:
        _config.models = dict(models)
    if model_retries is not _UNSET:
        if model_retries < 0:
            raise ValueError("model_retries must be at least 0")
        _config.model_retries = model_retries
    if backoff is not _UNSET:
        if backoff < 0:
            raise ValueError("backoff must be at least 0")
        _config.backoff = float(backoff)
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
    if unpriced is not _UNSET:
        if unpriced not in ("error", "warn"):
            raise ValueError('unpriced must be "error" or "warn"')
        _config.unpriced = unpriced


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
    budget.reset_warnings()


def config() -> Config:
    return _config


class _Attempt:
    """One model request of an `ai fn` call: what goes before and after asking the model,
    shared by the sync and async paths."""

    def __init__(
        self,
        function: str,
        prompt: str,
        returns: Type,
        errors: list[str],
        alias: str | None = None,
        number: int | None = None,
    ) -> None:
        if alias is None:
            model = _config.model
            if model is None:
                raise NoModelError(
                    f"`{function}` needs a model; call wardscript.runtime.configure(model=...) first"
                )
        else:
            model = _config.models.get(alias)
            if model is None:
                known = ", ".join(f"`{a}`" for a in _config.models) or "none"
                raise NoModelError(
                    f"`{function}` asks for the model `{alias}`, but configure(models=...) "
                    f"doesn't have it (configured: {known})"
                )
        self.model = model
        self.alias = alias
        self.returns = returns
        self.request = AiRequest(function, prompt, json_schema(returns), len(errors), tuple(errors))
        #: This request's place among all of the call's requests, for the trace.
        self.number = len(errors) if number is None else number
        self.text = ""
        strict = _config.unpriced == "error"
        # A model that says it has no prices (`prices=None`) can't be counted.
        budget.before_priced_call(getattr(model, "prices", ()) is not None, strict)
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
            text, tokens, cost = answer, None, None
        if tokens is None:
            tokens = estimate_tokens(self.request.prompt) + estimate_tokens(str(text))
        run = audit.current()
        if run is not None:
            run.calls += 1
            run.tokens += tokens
            run.cost += cost or 0.0
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
            attempt=self.number,
            model=self.alias,
            prompt=self.request.prompt,
            answer=str(text),
            tokens=float(tokens),
            cost=None if cost is None else float(cost),
            error=error,
            leaves=audit.leaves(value) if ok else [],
        )
        budget.after_model_call(tokens, cost, _config.unpriced == "error")
        return ok, value, error


    def failed(self, error: Exception) -> None:
        """Records a request the provider didn't answer; it counts as a call."""
        run = audit.current()
        if run is not None:
            run.calls += 1
        audit.record(
            "ai_call",
            started=self.started,
            function=self.request.function,
            attempt=self.number,
            model=self.alias,
            prompt=self.request.prompt,
            answer=None,
            tokens=0.0,
            cost=0.0,
            error=f"{type(error).__name__}: {error}",
            leaves=[],
        )
        budget.after_model_call(0, 0.0)


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


Models = Union[tuple[Union[str, None], ...], None]


def _steps(
    function: str,
    prompt: str,
    returns: Type,
    models: Models,
    retries: int | None,
    backoff: float | None,
) -> Generator[Union[_Attempt, float], Any, Any]:
    """The plan of an `ai fn` call, shared by the sync and async paths. Yields each
    `_Attempt` to ask (the caller sends back the answer, or throws the `ModelError`)
    and each backoff delay in seconds (the caller sleeps); returns the decoded value.

    Each model in turn: provider errors that are `retryable` are retried up to
    `retries` times with exponential backoff; an answer that doesn't fit the type is
    retried `config.retries` times with the error. Then the next model is tried. When
    every model fails, the last one's error is raised."""
    provider_retries = _config.model_retries if retries is None else retries
    delay = _config.backoff if backoff is None else backoff
    number = 0
    last: Exception | None = None
    for alias in models or (None,):
        errors: list[str] = []
        failure: ModelError | None = None
        for _ in range(_config.retries + 1):
            for n in range(provider_retries + 1):
                attempt = _Attempt(function, prompt, returns, errors, alias, number)
                number += 1
                try:
                    answer = yield attempt
                except ModelError as e:
                    attempt.failed(e)
                    failure = e
                    if e.retryable and n < provider_retries:
                        yield delay * 2**n
                        continue
                    break
                failure = None
                ok, value, error = attempt.finish(answer)
                if ok:
                    return value
                errors.append(error or "")
                break
            if failure is not None:
                break
        last = failure if failure is not None else AiOutputError(function, errors)
    assert last is not None
    raise last


def ai(
    function: str,
    prompt: str,
    returns: Type,
    models: Models = None,
    retries: int | None = None,
    backoff: float | None = None,
) -> Any:
    """Calls the model for `ai fn function` and decodes its answer as `returns`,
    retrying with the error when the answer doesn't fit. `models`, `retries` and
    `backoff` come from the function's `model {...}` clause (see `_steps`)."""
    steps = _steps(function, prompt, returns, models, retries, backoff)
    try:
        step = next(steps)
        while True:
            if isinstance(step, _Attempt):
                try:
                    answer = _ask(step)
                except ModelError as e:
                    step = steps.throw(e)
                    continue
                step = steps.send(answer)
            else:
                budget.check_time()
                time.sleep(step)
                step = next(steps)
    except StopIteration as done:
        return done.value


async def ai_async(
    function: str,
    prompt: str,
    returns: Type,
    models: Models = None,
    retries: int | None = None,
    backoff: float | None = None,
) -> Any:
    steps = _steps(function, prompt, returns, models, retries, backoff)
    try:
        step = next(steps)
        while True:
            if isinstance(step, _Attempt):
                try:
                    answer = await _ask_async(step)
                except ModelError as e:
                    step = steps.throw(e)
                    continue
                step = steps.send(answer)
            else:
                budget.check_time()
                await asyncio.sleep(step)
                step = next(steps)
    except StopIteration as done:
        return done.value


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
    """A tool call. With a schema from `ward.lock`, the generated code passes the
    parameter `names`, which of them are `sinks` and the type the result `returns`; a
    tool object with a `call_tool(name, arguments)` method (an MCP server) then gets
    named arguments."""

    def __init__(
        self,
        source: str,
        name: str,
        site: str,
        args: tuple[Any, ...],
        mcp_name: str | None = None,
        names: tuple[str, ...] | None = None,
        sinks: tuple[bool, ...] | None = None,
        returns: Type | None = None,
    ) -> None:
        budget.check_time()
        self.source, self.name, self.site, self.args = source, name, site, args
        self.returns = returns
        impl = _config.tools.get(source)
        call_named = getattr(impl, "call_tool", None)
        if names is not None and callable(call_named):
            arguments = {n: encode(a) for n, a in zip(names, args) if a is not None}
            tool_name = mcp_name or name
            self.fn: Callable[..., Any] = lambda *_: call_named(tool_name, arguments)
            self.blocking = True
        else:
            self.fn = _tool(source, name)
            self.blocking = False
        self.started = audit.now()
        try:
            for i, arg in enumerate(args):
                if sinks is None or (sinks[i] if i < len(sinks) else True):
                    audit.check_sink(f"{source}.{name}", i, arg)
        except TrustError as e:
            self.done(None, f"TrustError: {e}")
            raise

    def result(self, value: Any) -> Any:
        """The result, checked against the schema's type."""
        if self.returns is None:
            return value
        try:
            return decode(self.returns, value, f"result of `{self.source}.{self.name}`")
        except DecodeError as e:
            raise ToolError(f"`{self.source}.{self.name}` returned something its schema doesn't allow: {e}") from e

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


def call_tool(source: str, name: str, site: str, *args: Any, **schema: Any) -> Any:
    call = _ToolCall(source, name, site, args, **schema)
    try:
        result = call.result(_resolve(call.fn(*args)))
    except Exception as e:
        call.done(None, f"{type(e).__name__}: {e}")
        raise
    call.done(result, None)
    return result


async def call_tool_async(source: str, name: str, site: str, *args: Any, **schema: Any) -> Any:
    call = _ToolCall(source, name, site, args, **schema)
    try:
        if call.blocking:
            # An MCP request blocks; keep the event loop free.
            result = await asyncio.to_thread(call.fn)
        else:
            result = call.fn(*args)
        if inspect.isawaitable(result):
            result = await result
        result = call.result(result)
    except Exception as e:
        call.done(None, f"{type(e).__name__}: {e}")
        raise
    call.done(result, None)
    return result
