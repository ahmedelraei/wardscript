"""Runtime configuration, and the operations generated code calls that depend on it:
model calls, approvals and tools."""

from __future__ import annotations

import asyncio
import concurrent.futures
import inspect
import json
from dataclasses import dataclass, field
from typing import Any, Awaitable, Callable, Mapping, Union

from . import audit, budget
from .errors import AiOutputError, ApprovalDenied, DecodeError, NoModelError, Thrown, ToolError
from .model import AiRequest, Completion, Model, estimate_tokens
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


def ai(function: str, prompt: str, returns: Type) -> Any:
    """Calls the model for `ai fn function` and decodes its answer as `returns`,
    retrying with the error when the answer doesn't fit."""
    model = _config.model
    if model is None:
        raise NoModelError(
            f"`{function}` needs a model; call wardscript.runtime.configure(model=...) first"
        )
    schema = json_schema(returns)
    errors: list[str] = []
    for attempt in range(_config.retries + 1):
        request = AiRequest(function, prompt, schema, attempt, tuple(errors))
        budget.before_model_call()
        started = audit.now()
        answer = _resolve(model.complete(request))
        if isinstance(answer, Completion):
            text = answer.text
            tokens = answer.tokens
            cost = answer.cost
        else:
            text, tokens, cost = answer, None, 0.0
        if tokens is None:
            tokens = estimate_tokens(request.prompt) + estimate_tokens(str(text))
        run = audit.current()
        if run is not None:
            run.calls += 1
            run.tokens += tokens
            run.cost += cost
        error, value, ok = None, None, False
        try:
            value = decode(returns, json.loads(text))
            ok = True
        except (json.JSONDecodeError, TypeError) as e:
            error = f"the answer is not valid JSON ({e})"
        except DecodeError as e:
            error = str(e)
        audit.record(
            "ai_call",
            started=started,
            function=function,
            attempt=attempt,
            prompt=prompt,
            answer=str(text),
            tokens=float(tokens),
            cost=float(cost),
            error=error,
            leaves=audit.leaves(value) if ok else [],
        )
        budget.after_model_call(tokens, cost)
        if ok:
            return value
        errors.append(error or "")
    raise AiOutputError(function, errors)


def validate(value: Any, rule: Callable[[Any], bool], rule_name: str, site: str = "?") -> Any:
    passed = bool(rule(value))
    audit.record("validate", rule=rule_name, site=site, passed=passed, leaves=audit.leaves(value))
    if passed:
        return value
    raise Thrown(f"validation failed: `{rule_name}` rejected the value")


def approve(value: Any, site: str) -> Any:
    approver = _config.approver
    if approver is None:
        raise ApprovalDenied(
            f"approval needed at {site}, but no approver is configured; "
            "call wardscript.runtime.configure(approver=...)"
        )
    run = audit.current()
    approved = bool(_resolve(approver(ApprovalRequest(value, site, run.id if run else None))))
    audit.record("approve", site=site, approved=approved, leaves=audit.leaves(value))
    if not approved:
        raise ApprovalDenied(f"approval denied at {site}")
    return value


def declassify(value: Any, reason: str, site: str = "?") -> Any:
    audit.record("declassify", site=site, reason=reason, leaves=audit.leaves(value))
    return value


def call_tool(source: str, name: str, site: str, *args: Any) -> Any:
    budget.check_time()
    impl = _config.tools.get(source)
    if impl is None:
        raise ToolError(
            f"tool `{source}` is not configured; "
            f"call wardscript.runtime.configure(tools={{{source!r}: ...}})"
        )
    fn = impl.get(name) if isinstance(impl, Mapping) else getattr(impl, name, None)
    if fn is None:
        raise ToolError(f"tool `{source}` has no function `{name}`")
    started = audit.now()
    error, result = None, None
    try:
        result = fn(*args)
        return result
    except Exception as e:
        error = f"{type(e).__name__}: {e}"
        raise
    finally:
        audit.record(
            "tool_call",
            started=started,
            tool=source,
            function=name,
            site=site,
            args=[audit.to_json(a) for a in args],
            digests=[audit.digest(a) for a in args],
            error=error,
            leaves=audit.leaves(result) if error is None else [],
        )
        budget.check_time()
