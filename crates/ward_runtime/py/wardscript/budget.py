"""Budget counters. A function declared with `budget {...}` runs inside `budget()`;
every model call is charged to all the budgets active around it, and the run stops
with `BudgetExceeded` as soon as one is used up. The counting is the runtime core's."""

from __future__ import annotations

import contextlib
import contextvars
from typing import Any, Iterator

from . import audit, core
from .errors import BudgetExceeded

_active: contextvars.ContextVar[tuple[Any, ...]] = contextvars.ContextVar(
    "wardscript_budgets", default=()
)


@contextlib.contextmanager
def budget(function: str, **limits: float) -> Iterator[Any]:
    b = core.Budget(function, **limits)
    token = _active.set((*_active.get(), b))
    try:
        yield b
    finally:
        _active.reset(token)


def active() -> tuple[Any, ...]:
    return _active.get()


def _raise(b: Any, over: tuple[str, float, float] | None) -> None:
    if over is not None:
        resource, limit, used = over
        audit.record(
            "budget_exceeded", function=b.function, resource=resource, limit=limit, used=used
        )
        raise BudgetExceeded(b.function, resource, limit, used)


def check_time() -> None:
    for b in _active.get():
        _raise(b, b.check_time())


def before_model_call() -> None:
    """Charges one call, refusing it if that goes over a `calls` budget."""
    for b in _active.get():
        _raise(b, b.charge_call())


def after_model_call(tokens: int, cost: float) -> None:
    overs = [(b, b.charge_usage(tokens, cost)) for b in _active.get()]
    for b, over in overs:
        _raise(b, over)
