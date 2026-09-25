# WEP 011: Unknown model cost fails closed

**Status:** Final. Accepted 2026-09-24.

## Decision

- **`Completion.cost` is `float | None`.** `None` means unknown: a provider built
  without `prices`, or a model that returns plain text. Providers return `None`
  instead of `0.0` when they have no prices. The mock model keeps `0.0`.
- **A `cost` budget refuses what it can't count.** Before a request, if a `cost`
  budget is active and the model has `prices=None`, the runtime raises
  `BudgetUnenforceable` without calling it. After an answer whose cost is unknown,
  it raises the same error. Without a `cost` budget, nothing changes.
- **`configure(unpriced="warn")`** downgrades the error to one `RuntimeWarning` per
  function; such calls count as free.
- **Both cores decide the same way**: `Budget.charge_usage(tokens, cost)` accepts an
  unknown cost (counted as 0) and `Budget.unenforceable(cost)` says whether that
  breaks a `cost` limit.
- **The trace says so**: `ai_call.cost` is `null` when unknown, and a
  `budget_unenforceable` record (`function`, `when`) precedes the error.
  `ward trace show` prints unknown costs as `$?`; OTLP spans leave out `ward.cost`.

## Why

A budget that silently never runs out is worse than no budget: the program claims
a limit it doesn't enforce. Failing closed matches the rest of Wardscript, and
the error names the fix (give the provider `prices`).

## Not done

- A built-in price table per model. Prices change too often to ship in the runtime.
