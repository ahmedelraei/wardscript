"""How Wardscript values are represented in Python, and the operations generated code
needs beyond plain Python.

`Option<T>` is `None` or the value itself, so hosts see ordinary optionals. The one
value that can't be represented that way, `Some(x)` where `x` is itself `None` or a
`Some` (as in `Option<Option<Int>>`), is wrapped in `Some`. `some()` and `unwrap()`
apply that rule, so every `Option` has exactly one representation and `==` works.
"""

from __future__ import annotations

import dataclasses
import enum
import json
import math
import re
from typing import Any

from .errors import PanicError


class Some:
    __slots__ = ("value",)

    def __init__(self, value: Any) -> None:
        self.value = value

    def __eq__(self, other: object) -> bool:
        return isinstance(other, Some) and self.value == other.value

    def __hash__(self) -> int:
        return hash(("Some", self.value))

    def __repr__(self) -> str:
        return f"Some({self.value!r})"


def some(value: Any) -> Any:
    return Some(value) if value is None or isinstance(value, Some) else value


def unwrap(option: Any) -> Any:
    """The `x` in `Some(x)`; `option` must not be `None`."""
    return option.value if isinstance(option, Some) else option


def unwrap_or(option: Any, default: Any) -> Any:
    return default if option is None else unwrap(option)


def idiv(a: int, b: int) -> int:
    """Integer division, rounding towards zero."""
    if b == 0:
        raise PanicError("division by zero")
    q = abs(a) // abs(b)
    return q if (a < 0) == (b < 0) else -q


def irem(a: int, b: int) -> int:
    """Remainder with the sign of `a`, so that `a == (a / b) * b + a % b`."""
    return a - b * idiv(a, b)


def fdiv(a: float, b: float) -> float:
    """IEEE 754 division: dividing by zero gives an infinity or NaN, not an error."""
    if b == 0:
        if a == 0 or math.isnan(a):
            return math.nan
        return math.copysign(math.inf, a) * math.copysign(1.0, b)
    return a / b


def frem(a: float, b: float) -> float:
    """Float remainder with the sign of `a`, like `irem`."""
    if b == 0:
        return math.nan
    return math.fmod(a, b)


def round_half_away(x: float) -> int:
    if math.isnan(x) or math.isinf(x):
        raise PanicError(f"cannot round {x} to an integer")
    return math.floor(x + 0.5) if x >= 0 else -math.floor(-x + 0.5)


_INT = re.compile(r"[+-]?[0-9]+")
_FLOAT = re.compile(r"[+-]?[0-9]+(\.[0-9]+)?")


def parse_int(s: str) -> Any:
    t = s.strip()
    if not _INT.fullmatch(t):
        return None
    n = int(t)
    return some(n) if -(2**63) <= n < 2**63 else None


def parse_float(s: str) -> Any:
    t = s.strip()
    return some(float(t)) if _FLOAT.fullmatch(t) else None


def _check_index(xs: list, i: int) -> None:
    if not 0 <= i < len(xs):
        raise PanicError(f"index {i} is out of bounds for a list of length {len(xs)}")


def index(xs: list, i: int) -> Any:
    _check_index(xs, i)
    return xs[i]


def list_get(xs: list, i: int) -> Any:
    return some(xs[i]) if 0 <= i < len(xs) else None


def first(xs: list) -> Any:
    return some(xs[0]) if xs else None


def last(xs: list) -> Any:
    return some(xs[-1]) if xs else None


def list_set(xs: list, i: int, value: Any) -> list:
    _check_index(xs, i)
    return [*xs[:i], value, *xs[i + 1 :]]


def map_get(m: dict, key: Any) -> Any:
    return some(m[key]) if key in m else None


def map_index(m: dict, key: Any) -> Any:
    if key not in m:
        raise PanicError(f"key {to_str(key)} is not in the map")
    return m[key]


def field(obj: Any, name: str) -> Any:
    """A field of a tool result: a mapping key or an attribute."""
    if isinstance(obj, dict):
        return obj[name]
    return getattr(obj, name)


def with_field(obj: Any, name: str, value: Any) -> Any:
    if isinstance(obj, dict):
        return {**obj, name: value}
    return dataclasses.replace(obj, **{name: value})


def to_str(value: Any) -> str:
    """Text for string templates and `to_string()`."""
    if isinstance(value, str):
        return value
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return str(value)
    if isinstance(value, Some):
        return to_str(value.value)
    if value is None:
        return "None"
    if isinstance(value, enum.Enum):
        return value.name
    from .schema import encode

    return json.dumps(encode(value), ensure_ascii=False)
