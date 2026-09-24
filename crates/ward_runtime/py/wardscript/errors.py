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
