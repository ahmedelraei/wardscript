"""Budget counters. A function declared with `budget {...}` runs inside `budget()`;
every model call is charged to all the budgets active around it, and the run stops
with `BudgetExceeded` as soon as one is used up. The counting is the runtime core's."""

from __future__ import annotations

import contextlib
import contextvars
import warnings
from typing import Any, Iterator

from . import audit, core
from .errors import BudgetExceeded, BudgetUnenforceable

_active: contextvars.ContextVar[tuple[Any, ...]] = contextvars.ContextVar(
    "wardscript_budgets", default=()
)


#: Token limits of the active budgets, for stopping a streamed answer early.
_token_limits: contextvars.ContextVar[tuple[tuple[Any, float], ...]] = contextvars.ContextVar(
    "wardscript_token_limits", default=()
)


@contextlib.contextmanager
def budget(function: str, **limits: float) -> Iterator[Any]:
    b = core.Budget(function, **limits)
    token = _active.set((*_active.get(), b))
    tokens = limits.get("tokens")
    limit_token = _token_limits.set(
        _token_limits.get() + (((b, float(tokens)),) if tokens is not None else ())
    )
    try:
        yield b
    finally:
        _token_limits.reset(limit_token)
        _active.reset(token)


def limits_tokens() -> bool:
    return bool(_token_limits.get())


def tokens_over(tokens: float) -> bool:
    """Whether charging `tokens` more would go over a `tokens` budget."""
    return any(b.used[0] + tokens > limit for b, limit in _token_limits.get())


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


_warned: set[str] = set()


def _unenforceable(function: str, when: str, strict: bool) -> None:
    """Fails closed on a `cost` budget that can't be counted, or warns once per
    function when the host allowed it with `configure(unpriced="warn")`."""
    if strict:
        audit.record("budget_unenforceable", function=function, when=when)
        raise BudgetUnenforceable(function, when)
    if function not in _warned:
        _warned.add(function)
        warnings.warn(str(BudgetUnenforceable(function, when)), RuntimeWarning, stacklevel=4)


def reset_warnings() -> None:
    _warned.clear()


def before_priced_call(priced: bool, strict: bool) -> None:
    """Refuses a request to a model without prices while a `cost` budget is active."""
    if priced:
        return
    for b in _active.get():
        if b.limits_cost:
            _unenforceable(b.function, "before", strict)
            return


def after_model_call(tokens: int, cost: float | None, strict: bool = True) -> None:
    overs = [(b, b.charge_usage(tokens, cost)) for b in _active.get()]
    for b, over in overs:
        _raise(b, over)
    for b, _ in overs:
        if b.unenforceable(cost):
            _unenforceable(b.function, "after", strict)
            return
