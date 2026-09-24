"""Exceptions raised by Wardscript programs and by the runtime."""

from __future__ import annotations

from typing import Any


class Thrown(Exception):
    """A Wardscript `throw` that reached the host. The thrown value is `value`."""

    def __init__(self, value: Any) -> None:
        super().__init__(value)
        self.value = value

    def __str__(self) -> str:
        from .values import to_str

        return to_str(self.value)


class WardError(Exception):
    """A runtime failure: something the program's types can't express, like a model
    that keeps answering with invalid output. Never caught by Wardscript `try`."""


class NoModelError(WardError):
    pass


class AiOutputError(WardError):
    """The model's answers didn't match the return type, on every attempt."""

    def __init__(self, function: str, errors: list[str]) -> None:
        attempts = "\n".join(f"  attempt {i + 1}: {e}" for i, e in enumerate(errors))
        super().__init__(f"`{function}` got no valid answer from the model:\n{attempts}")
        self.function = function
        self.errors = errors


class ModelError(WardError):
    """A model provider failed to answer: a rate limit, a timeout, an outage, or a
    request it refused. `retryable` errors are retried with backoff (see `model {...}`
    and `configure(model_retries=..., backoff=...)`); after that, or right away for
    the others, the call falls back to the next model."""

    retryable = False

    def __init__(self, message: str, *, status: int | None = None) -> None:
        super().__init__(message)
        self.status = status


class RateLimited(ModelError):
    """The provider asked to slow down (HTTP 429)."""

    retryable = True


class ModelUnavailable(ModelError):
    """A timeout, a lost connection, or a server error (HTTP 5xx, 529 overloaded)."""

    retryable = True


class BudgetExceeded(WardError):
    """A function used more of a resource than its `budget` allows; the run stops."""

    def __init__(self, function: str, resource: str, limit: float, used: float) -> None:
        unit = {"cost": "$", "time": "s"}.get(resource, "")
        shown = (lambda v: f"${v:g}") if unit == "$" else (lambda v: f"{v:g}{unit}")
        super().__init__(
            f"`{function}` went over its {resource} budget: used {shown(used)} of {shown(limit)}"
        )
        self.function = function
        self.resource = resource
        self.limit = limit
        self.used = used


class BudgetUnenforceable(WardError):
    """A function has a `cost` budget, but the model's cost is unknown (a provider
    without `prices`), so the budget can't be enforced. `when` is `"before"` when the
    request was refused unsent, `"after"` when an answer came back without a cost.
    `configure(unpriced="warn")` turns this into a warning."""

    def __init__(self, function: str, when: str) -> None:
        why = (
            "the model has no prices; give the provider `prices=(input, output)`"
            if when == "before"
            else "the model's answer didn't say what it cost"
        )
        super().__init__(f"`{function}` has a cost budget, but {why}")
        self.function = function
        self.when = when


class ApprovalDenied(WardError):
    pass


class ToolError(WardError):
    pass


class DecodeError(WardError):
    """A JSON value doesn't match a Wardscript type. `path` locates it, e.g. `$.items[2]`."""

    def __init__(self, path: str, message: str) -> None:
        super().__init__(f"{path}: {message}")
        self.path = path
        self.message = message


class PanicError(WardError):
    """An operation with no defined result: an index out of bounds, division by zero."""


class TrustError(WardError):
    """A host passed a value to a parameter that must be trusted without vouching for it."""
