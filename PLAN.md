# Wardscript: build plan

Work through one milestone at a time. Each milestone lists its tasks, the checks it must pass before moving on ("Done when") Read `AGENTS.md` and `docs/spec/` first.

Status: M0–M7 done, including M6 additions (runtime sink checks, OTLP collectors, async code, text streaming; decision 010), unknown model cost failing closed (decision 011) MCP imports with `ward.lock` (decision 012) model fallbacks and retry policies (decision 013), refinements and output checks (decision 014), and tests with recorded responses (decision 015). M11 in progress: language server and VS Code extension (016), partial streaming, `ward fmt`, the TypeScript backend (017) are done; the AgentDojo port, `salsa` and the WASM backend remain.

---

## M0: Project skeleton (½ day)
**Tasks**
- Cargo workspace with the crates listed in AGENTS.md (empty stubs).
- `ward` CLI built with `clap`: `check`, `build`, `run` subcommands (stubs).
- CI (GitHub Actions): fmt, clippy, test.
- Set up `insta`, `tests/ui`, `tests/attacks`, `docs/spec/README.md`.

**Done when:** `cargo test` passes on CI and `ward --help` works.

---

## M1: Lexer and parser (2–4 days)
**Tasks**
- `logos` lexer: identifiers, literals (int, float, string, string templates `"{x}"`), keywords (`ai fn pub let type enum match if else for while return import uses budget`), operators, comments.
- Recursive-descent parser with Pratt-style expression parsing:
  - Items: `fn`, `ai fn` (body is the prompt string), `type` records, `enum`, `import`.
  - Function headers: parameters, return type, `uses {...}`, `budget {...}`.
  - Statements and expressions: `let`, assignment, `if/else`, `match`, `for`, `while`, `return`, calls, field access, binary and unary operators, list and record literals, `?`.
  - Types: names, generics `List<T>`, `Option<T>`, `Result<T,E>`, `Untrusted<T>`, `Trusted<T>`.
- Error recovery: report several syntax errors per file.
- Pretty-printer (AST → source) for round-trip tests.

**Done when:** `examples/support.ward` parses; round-trip tests pass; there are 20+ ui snapshots of syntax errors with good messages.

---

## M2: Name resolution and base types (3–5 days)
**Tasks**
- `ward_resolve`: scopes, modules (one file = one module), `pub`, imports. Unknown-name errors with "did you mean" suggestions.
- `ward_check` (types only for now): bidirectional checking; primitives, records, enums, generics (List, Map, Option, Result), exhaustive `match`, `?`.
- Check that `ai fn` functions have a return type that can be turned into a JSON schema.
- `ward check` with human output and `--format json` output.

**Done when:** type-error ui tests pass (mismatch, non-exhaustive match, unknown field…); JSON diagnostics have stable codes.

---

## M3: WIR + Python backend + minimal runtime (4–6 days) ← first end-to-end
**Tasks**
- `ward_ir`: lower the checked AST into WIR (typed, explicit, no syntax sugar).
- `ward_codegen_py`: WIR → readable Python module + `.pyi` stubs. Records become dataclasses; enums become `Enum`.
- Runtime (Python side first, a thin package `wardscript`):
  - `runtime.configure(model=..., approver=...)`
  - LLM call for `ai fn`: build the prompt, derive a JSON schema from the return type, validate the output, retry N times, then raise a typed error.
  - A mock model provider for tests.
- `ward build --target python`, `ward run file.ward fn args`.

**Done when:** e2e test: `examples/triage.ward` builds, Python imports it, and the mock LLM returns a correctly typed `Ticket`.

---

## M4: Trust labels (the core, 1–2 weeks)
**Tasks**
- Label lattice `Trusted < Untrusted` in `ward_check`, with automatic label inference inside function bodies (developers annotate signatures only).
- Explicit flows: operators, calls, fields, collections combine labels.
- Implicit flows: branches or loops depending on untrusted data raise the label of everything written inside them (pc-label).
- Sources: `ai fn` outputs, imported tools, and host-language inputs are `Untrusted` by default.
- Sinks: tool parameters marked `sink` require `Trusted`.
- `validate(x, rule)`, `approve(x)`, `declassify(x, reason)` as built-ins; provenance is written into WIR.
- Diagnostic W0107 "untrusted data reaches sensitive action", showing the whole path the data took.
- Runtime: carry labels at the host-language boundary and check them again at sinks, as defense in depth.

**Done when:** 30+ `tests/attacks/` cases fail to compile (direct, via string concatenation, via collections, via branches, via helper functions); the fixed versions compile; no false positives on examples.

---

## M5: Effects, budgets, Rule of Two (1 week)
**Tasks**
- `uses {...}` effect sets; callers must declare everything their callees use (W0200-series).
- Effects inferred inside bodies, compared against the declared set; unused declared effects produce warnings.
- `budget {tokens, cost, time, calls}`: checked statically where possible, and enforced by the runtime counters otherwise.
- Rule of Two check, plus the `@allow(...)` override with a required reason.

**Done when:** ui tests for missing effects, budget violations and Rule of Two pass; the runtime stops a run that goes over budget (e2e).

---

## M6: Runtime completion + audit trace (1 week)
**Tasks**
- Move the runtime core to Rust (`ward_runtime`) with a PyO3 binding (built with maturin); keep the Python API the same.
- Approval hooks (sync and async), budget counters, typed errors.
- Audit trace per run: calls, labels, validations, approvals, declassifications → JSON Lines + OpenTelemetry export.
- `ward trace show <run_id>`.
- Real model providers: `wardscript.providers.anthropic` (then OpenAI), reporting real token usage and cost to budgets and using native structured output where the provider has it. `ward run --model anthropic` alongside `--mock`.
- Live tests against real models, opt-in with `WARD_LIVE=1` and skipped in CI.

**Done when:** `pip install` of a locally built wheel works; the trace for the support example shows the full provenance path; with `WARD_LIVE=1`, the triage and support examples run end to end against a real model and their budgets count its reported usage.

---

## M6.1: Unknown model cost fails closed (½–1 day)
A provider without `prices` reports `cost=0.0`, so a `cost` budget silently never runs out.

**Tasks**
- `Completion.cost` becomes `float | None`; `None` means unknown (provider without prices, or a model returning plain text). The mock model keeps `0.0`.
- Providers return `None` instead of `0.0` when they have no prices.
- Before each request: if a `cost` budget is active and the model has no prices, raise `BudgetUnenforceable` without calling.
- After each answer: if the cost is `None` and a `cost` budget is active, raise `BudgetUnenforceable`.
- `runtime.configure(unpriced="warn")` downgrades the error to a one-time warning.
- The Rust core's budget counter (`budget.rs`) and the pure-Python core accept an unknown cost and make the same decisions.
- Record the change in the audit trace; update `docs/spec/runtime.md` and `docs/spec/effects.md`; add a decision record.

**Done when:** tests cover a priced model (unchanged), an unpriced model under a `cost` budget (error before any request), an unpriced model with no `cost` budget (runs), and `unpriced="warn"` (one warning); both cores produce the same results.

---

## M7: MCP imports (1 week)
**Tasks**
- `import mcp "<server>" as x`: read the server's tool schemas (from a local config or a lockfile) and generate typed tool signatures.
- Label config: which parameters are sinks, which results are untrusted (conservative defaults: all results untrusted, all write-type tools are sinks).
- `ward.lock` pins the tool schemas so builds are reproducible.

**Done when:** the Gmail-like mock MCP server example type-checks and the injection case is rejected.

---

## M8: Model fallbacks and retry policies (1 week) ← to match BAML
BAML defines retry policies, fallbacks and round robin statically; without them, Wardscript loses users before they see the safety features.

**Tasks**
- Design the syntax first (a decision record), in the clause style of `budget {...}` and `uses {...}`, e.g. `model {primary: fast, fallback: smart}`. Model aliases map to providers in `runtime.configure`.
- Retry policy: attempts and backoff for provider errors (rate limits, timeouts), separate from the existing `retries` for invalid answers.
- Fallback: try the next model when one fails or its answer stays invalid after retries.
- Every attempt counts against budgets (`calls`, `tokens`, `cost` at the model that answered) and is recorded in the audit trace.

**Done when:** e2e tests with the mock model show a fallback after a failing primary, a retry after a rate-limit error, and budgets charged for every attempt; the trace shows each attempt.

---

## M9: Output checks and refinements (1–2 weeks) ← to match BAML
BAML has `@assert` and `@check` on outputs.

**Tasks**
- Design the syntax first (a decision record): refinements on types (e.g. `String where len < 200`) and checks on an `ai fn`'s answer.
- Refinements go into the JSON schema where possible and are checked on every answer; a failed check triggers a retry with the reason added to the prompt, then `AiOutputError`.
- Semantic checks such as `grounded_in`: decide how they are evaluated (rules, a second model call, both) before implementing.
- Checks don't change trust labels: an answer that passes its checks is still `Untrusted`.

**Done when:** ui tests for invalid refinements and checks pass; e2e tests show a failed check causing a retry and then success or a typed error; an attack test shows a checked answer still can't reach a sink.

---

## M10: `test` blocks (1 week) ← to match BAML
BAML defines tests in its files and runs them with `baml-cli test`.

**Tasks**
- `test` blocks in `.ward` files; `ward test` runs them (filters, exit codes).
- Record real model responses once, replay them deterministically in CI; assertions on typed results.
- Budgets and trust apply inside tests as in normal runs.

**Done when:** tests for the examples pass offline from recorded responses; `ward test` fails CI when an assertion fails.

---

## M11: Proof and polish (ongoing)
- [x] Typed streaming of partial decoded values (`configure(on_partial=...)`, `wardscript.partial`).
- [x] TypeScript backend (`ward_codegen_ts`, `ward build --target typescript`) with a runtime written in TypeScript (`crates/ward_runtime/ts`) instead of a napi-rs binding; decision 017.
- AgentDojo port: publish how many attacks are rejected at compile time and how many normal tasks still succeed (utility).
- [x] LSP (`ward lsp`: diagnostics, hover, go to definition) + VS Code extension (`editors/vscode`); decision 016.
- [x] `ward fmt` (and LSP formatting): the printer keeps comments and blank lines between statements.
- `salsa` for incremental checking.
- WASM backend via `wasm-encoder` (sandboxed execution).

---

## Working method
- Start each session by reading AGENTS.md, PLAN.md and `docs/spec/`, and naming the current milestone.
- Write tests first for checker and trust features: the failing ui and attack cases, then the implementation.
- Keep PRs small: one feature at a time, each with its tests and spec update.
- After each milestone, update `docs/spec/`, record decisions in `docs/decisions/NNN-*.md`, and list what changed so Notion can be synced.
