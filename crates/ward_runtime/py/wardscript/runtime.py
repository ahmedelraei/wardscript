"""Runtime configuration, and the operations generated code calls that depend on it:
model calls, approvals and tools."""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any, Callable, Mapping

from .errors import AiOutputError, ApprovalDenied, DecodeError, NoModelError, Thrown, ToolError
from .model import AiRequest, Model
from .schema import Type, decode, json_schema


@dataclass(frozen=True)
class ApprovalRequest:
    value: Any
    #: Where `approve` was called, e.g. `support.wardscript:58:24`.
    site: str


#: Returns whether a human approved the value.
Approver = Callable[[ApprovalRequest], bool]


@dataclass
class Config:
    model: Model | None = None
    approver: Approver | None = None
    #: Tool implementations by import source: `import mcp "gmail"` looks up `"gmail"`.
    #: Each is a mapping of functions, or an object with a method per tool function.
    tools: dict[str, Any] = field(default_factory=dict)
    #: How many times a model call is retried after an invalid answer.
    retries: int = 2


_config = Config()
_UNSET: Any = object()


def configure(
    *,
    model: Model | None = _UNSET,
    approver: Approver | None = _UNSET,
    tools: Mapping[str, Any] = _UNSET,
    retries: int = _UNSET,
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
        text = model.complete(request)
        try:
            value = json.loads(text)
        except (json.JSONDecodeError, TypeError) as e:
            errors.append(f"the answer is not valid JSON ({e})")
            continue
        try:
            return decode(returns, value)
        except DecodeError as e:
            errors.append(str(e))
    raise AiOutputError(function, errors)


def validate(value: Any, rule: Callable[[Any], bool], rule_name: str) -> Any:
    if rule(value):
        return value
    raise Thrown(f"validation failed: `{rule_name}` rejected the value")


def approve(value: Any, site: str) -> Any:
    approver = _config.approver
    if approver is None:
        raise ApprovalDenied(
            f"approval needed at {site}, but no approver is configured; "
            "call wardscript.runtime.configure(approver=...)"
        )
    if not approver(ApprovalRequest(value, site)):
        raise ApprovalDenied(f"approval denied at {site}")
    return value


def declassify(value: Any, reason: str) -> Any:
    return value


def call_tool(source: str, name: str, *args: Any) -> Any:
    impl = _config.tools.get(source)
    if impl is None:
        raise ToolError(
            f"tool `{source}` is not configured; "
            f"call wardscript.runtime.configure(tools={{{source!r}: ...}})"
        )
    fn = impl.get(name) if isinstance(impl, Mapping) else getattr(impl, name, None)
    if fn is None:
        raise ToolError(f"tool `{source}` has no function `{name}`")
    return fn(*args)
