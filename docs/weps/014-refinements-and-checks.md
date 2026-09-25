# WEP 014: Refinements and checks on answers

**Status:** Final. Accepted 2026-09-24.

## Decision

- **Refinements are types with a condition**: `T where <condition on it>`, e.g.
  `String where it.len() <= 80`. They go where decoded data is described: record
  fields, variant payloads, type aliases, `ai fn` return types. For type checking
  they are erased (`T where ...` is `T`).
- **Refinement conditions are pure and self-contained**: `it`, literals,
  operators, fields, methods and enum variants. No calls. So they can run anywhere
  a value is decoded, have no effects or trust consequences, and simple ones map
  to JSON Schema (`maxLength`, `minimum`, ...) that providers' structured output
  enforces before the runtime even sees the answer.
- **Checks are a `check {...}` clause on the `ai fn`**, with conditions on `it`
  and the parameters, each with an optional reason: `cond => "why"`. Unlike
  refinements, checks may call functions. That's how semantic checks work: a rule
  written in Wardscript, or an `ai fn` returning `Bool` as a model judge. There is
  no built-in `grounded_in`: which judge to use, and what it costs, stays visible
  in the program, its effects and its budget.
- **A failed refinement or check is an invalid answer**: retried with the reason
  added to the prompt, then `AiOutputError` (or the next model's turn). Each
  attempt's `ai_call` record carries the reason.
- **Checks don't change trust.** An answer that passes is still `Untrusted`. Checks
  constrain shape; they can't tell whether an attacker shaped the content. Only
  `validate`, `approve` and `declassify` make data trusted.

## Why

BAML's `@assert` and `@check` show that constraints on outputs belong next to the
function. Splitting them into pure refinements (schema-friendly, reusable through
aliases) and function-level checks (anything, including a second model) keeps each
simple, and keeps the trust model unchanged: validation of *content* is still an
explicit `validate` at the point of use.

## Not done

- Refinements inside type arguments (`List<String where ...>`); an alias does it.
- Re-checking refinements on values the program builds itself.
- Non-failing checks that only record a score (BAML's `@check`).
- Interpolation in check reasons.
