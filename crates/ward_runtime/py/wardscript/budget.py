"""Budget counters. A function declared with `budget {...}` runs inside `budget()`;
every model call is charged to all the budgets active around it, and the run stops
with `BudgetExceeded` as soon as one is used up."""

from __future__ import annotations

import contextlib
import contextvars
import time
from dataclasses import dataclass, field
from typing import Iterator

from .errors import BudgetExceeded


@dataclass
class Budget:
    function: str
    #: `tokens`, `calls`, `cost` (dollars) and `time` (seconds); missing means unlimited.
    limits: dict[str, float]
    used: dict[str, float] = field(default_factory=lambda: {"tokens": 0, "calls": 0, "cost": 0.0})
    started: float = field(default_factory=time.monotonic)

    def elapsed(self) -> float:
        return time.monotonic() - self.started

    def check(self) -> None:
        for resource in ("tokens", "calls", "cost"):
            limit = self.limits.get(resource)
            if limit is not None and self.used[resource] > limit:
                raise BudgetExceeded(self.function, resource, limit, self.used[resource])
        limit = self.limits.get("time")
        if limit is not None and self.elapsed() > limit:
            raise BudgetExceeded(self.function, "time", limit, self.elapsed())


_active: contextvars.ContextVar[tuple[Budget, ...]] = contextvars.ContextVar(
    "wardscript_budgets", default=()
)


@contextlib.contextmanager
def budget(function: str, **limits: float) -> Iterator[Budget]:
    b = Budget(function, dict(limits))
    token = _active.set((*_active.get(), b))
    try:
        yield b
    finally:
        _active.reset(token)


def active() -> tuple[Budget, ...]:
    return _active.get()


def check_time() -> None:
    for b in _active.get():
        if "time" in b.limits:
            b.check()


def before_model_call() -> None:
    """Charges one call, refusing it if that goes over a `calls` budget."""
    for b in _active.get():
        b.used["calls"] += 1
        b.check()


def after_model_call(tokens: int, cost: float) -> None:
    for b in _active.get():
        b.used["tokens"] += tokens
        b.used["cost"] += cost
    for b in _active.get():
        b.check()
