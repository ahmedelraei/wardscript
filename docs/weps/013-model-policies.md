# WEP 013: Model fallbacks and retry policies

**Status:** Final. Accepted 2026-09-24.

## Decision

- **A `model {...}` clause on the `ai fn`**, in the style of `budget {...}`:
  `model {primary: fast, fallback: [smart, backup], retries: 2, backoff: 0.5}`.
  Its values are model aliases and numbers, not expressions, so aliases never
  collide with variables. `model` is a keyword only where a clause can start.
- **Aliases are bound in the runtime** (`configure(models={...})`,
  `ward run --model alias=provider:model`). The program says what it needs (a fast
  model, a smart one); the deployment says which models those are.
- **Two kinds of retries.** `retries` in the clause is for provider errors that
  may pass (rate limits, timeouts, 5xx), with exponential backoff. Invalid answers
  keep using `configure(retries=...)`, with the error added to the prompt. A
  request the provider rejects outright isn't retried.
- **Fallback moves on** when a model keeps failing or keeps answering invalidly.
  When all fail, the last error is raised (`ModelError` or `AiOutputError`).
- **Typed provider errors**: `ModelError`, with `RateLimited` and
  `ModelUnavailable` as the retryable ones. Providers map SDK exceptions by HTTP
  status and create their clients with `max_retries=0`, so retries follow the
  policy and are visible in the trace and budgets.
- **Every request counts**: a `calls` budget is charged before each one, and a
  retry that would go over is never sent. Tokens and cost come from the model that
  answered. Each request is an `ai_call` record with its `model` alias.

## Why

Production code needs a second model when the first is rate limited or down. With
the policy in the program, it's reviewed with the code, and the checker's budgets
and the trace cover every attempt instead of hiding retries inside an SDK.

## Not done

- Round-robin and weighted strategies (BAML has them).
- A policy on a whole module or a named policy reused by several functions.
- Static checks that every alias is configured: aliases are bound at run time.
