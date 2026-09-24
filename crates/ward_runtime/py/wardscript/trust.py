"""Trust labels at the host boundary.

The compiler proves that untrusted data can't reach a sink inside a program. A value
the host passes in is untrusted by default, so a parameter that reaches a sink only
accepts a value the host vouches for by wrapping it: `handle(email, Trusted(to))`.
"""

from __future__ import annotations

import dataclasses
from typing import Any, Generic, TypeVar

from .errors import TrustError

T = TypeVar("T")


@dataclasses.dataclass(frozen=True)
class Trusted(Generic[T]):
    value: T


def vouched(value: Any, param: str, function: str) -> Any:
    if isinstance(value, Trusted):
        return value.value
    raise TrustError(
        f"`{function}` sends its parameter `{param}` to a sensitive action, so it only "
        f"accepts a value the caller vouches for: pass `wardscript.Trusted(value)`"
    )
