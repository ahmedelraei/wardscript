# Wardscript: build plan

Work through one milestone at a time. Each milestone lists its tasks, the checks it must pass before moving on ("Done when") and a prompt you can give a coding agent. Read `AGENTS.md` first.

---

## M0: Project skeleton (½ day)
**Tasks**
- Cargo workspace with the crates listed in AGENTS.md (empty stubs).
- `ward` CLI built with `clap`: `check`, `build`, `run` subcommands (stubs).
- CI (GitHub Actions): fmt, clippy, test.
- Set up `insta`, `tests/ui`, `tests/attacks`, `docs/spec/README.md`.

**Done when:** `cargo test` passes on CI and `ward --help` works.

**Prompt**
> Read AGENTS.md. Set up milestone M0 from PLAN.md: the Cargo workspace, crate stubs, the clap-based `ward` CLI, GitHub Actions CI and the test folders. Don't implement any language features yet.

---

## M1: Lexer and parser (2–4 days)
**Tasks**
- `logos` lexer: identifiers, literals (int, float, string, string templates `"{x}"`), keywords (`fn pub let type enum match if else for while return import uses budget by llm`), operators, comments.
- Recursive-descent parser with Pratt-style expression parsing:
  - Items: `fn` (block body **or** `by llm "prompt"`), `type` records, `enum`, `import`.
  - Function headers: parameters, return type, `uses {...}`, `budget {...}`.
  - Statements and expressions: `let`, assignment, `if/else`, `match`, `for`, `while`, `return`, calls, field access, binary and unary operators, list and record literals, `?`.
  - Types: names, generics `List<T>`, `Option<T>`, `Result<T,E>`, `Untrusted<T>`, `Trusted<T>`.
- Error recovery: report several syntax errors per file.
- Pretty-printer (AST → source) for round-trip tests.

**Done when:** `examples/support.wardscript` parses; round-trip tests pass; there are 20+ ui snapshots of syntax errors with good messages.

**Prompt**
> Implement M1 from PLAN.md in `ws_syntax`. Hand-written recursive-descent parser, Pratt parsing for expressions, arena-allocated AST, error recovery. Add a pretty-printer and round-trip tests, plus ui snapshot tests for syntax errors.

---

## M2: Name resolution and base types (3–5 days)
**Tasks**
- `ws_resolve`: scopes, modules (one file = one module), `pub`, imports. Unknown-name errors with "did you mean" suggestions.
- `ws_check` (types only for now): bidirectional checking; primitives, records, enums, generics (List, Map, Option, Result), exhaustive `match`, `?`.
- Check that `by llm` functions have a return type that can be turned into a JSON schema.
- `ward check` with human output and `--format json` output.

**Done when:** type-error ui tests pass (mismatch, non-exhaustive match, unknown field…); JSON diagnostics have stable codes.

**Prompt**
> Implement M2: name resolution in `ws_resolve` and base type checking in `ws_check` (bidirectional, generics, exhaustive match). Wire up `ward check` with ariadne output and `--format json` using stable W-codes. Update docs/spec.

---

## M3: WIR + Python backend + minimal runtime (4–6 days) ← first end-to-end
**Tasks**
- `ws_ir`: lower the checked AST into WIR (typed, explicit, no syntax sugar).
- `ws_codegen_py`: WIR → readable Python module + `.pyi` stubs. Records become dataclasses; enums become `Enum`.
- Runtime (Python side first, a thin package `wardscript`):
  - `runtime.configure(model=..., approver=...)`
  - LLM call for `by llm`: build the prompt, derive a JSON schema from the return type, validate the output, retry N times, then raise a typed error.
  - A mock model provider for tests.
- `ward build --target python`, `ward run file.wardscript fn args`.

**Done when:** e2e test: `examples/triage.wardscript` builds, Python imports it, and the mock LLM returns a correctly typed `Ticket`.

**Prompt**
> Implement M3: lower to WIR, generate Python + .pyi, and a minimal Python runtime package with configure(), schema-validated LLM calls with retries, and a mock provider. Add e2e tests under tests/e2e.

---

## M4: Trust labels (the core, 1–2 weeks)
**Tasks**
- Label lattice `Trusted < Untrusted` in `ws_check`, with automatic label inference inside function bodies (developers annotate signatures only).
- Explicit flows: operators, calls, fields, collections combine labels.
- Implicit flows: branches or loops depending on untrusted data raise the label of everything written inside them (pc-label).
- Sources: `by llm` outputs, imported tools, and host-language inputs are `Untrusted` by default.
- Sinks: tool parameters marked `sink` require `Trusted`.
- `validate(x, rule)`, `approve(x)`, `declassify(x, reason)` as built-ins; provenance is written into WIR.
- Diagnostic W0107 "untrusted data reaches sensitive action", showing the whole path the data took.
- Runtime: carry labels at the host-language boundary and check them again at sinks, as defense in depth.

**Done when:** 30+ `tests/attacks/` cases fail to compile (direct, via string concatenation, via collections, via branches, via helper functions); the fixed versions compile; no false positives on examples.

**Prompt**
> Implement M4: trust labels in ws_check. Signatures carry labels and bodies are inferred. Track explicit flows plus implicit flows through a pc-label. Add sources and sinks, plus the validate/approve/declassify built-ins. W0107 must show the full flow path. Write the attack test suite first, then make it pass.

---

## M5: Effects, budgets, Rule of Two (1 week)
**Tasks**
- `uses {...}` effect sets; callers must declare everything their callees use (W0200-series).
- Effects inferred inside bodies, compared against the declared set; unused declared effects produce warnings.
- `budget {tokens, cost, time, calls}`: checked statically where possible, and enforced by the runtime counters otherwise.
- Rule of Two check, plus the `#[allow(...)]` override with a required reason.

**Done when:** ui tests for missing effects, budget violations and Rule of Two pass; the runtime stops a run that goes over budget (e2e).

---

## M6: Runtime completion + audit trace (1 week)
**Tasks**
- Move the runtime core to Rust (`ws_runtime`) with a PyO3 binding (built with maturin); keep the Python API the same.
- Approval hooks (sync and async), budget counters, typed errors.
- Audit trace per run: calls, labels, validations, approvals, declassifications → JSON Lines + OpenTelemetry export.
- `ward trace show <run_id>`.

**Done when:** `pip install` of a locally built wheel works; the trace for the support example shows the full provenance path.

---

## M7: MCP imports (1 week)
**Tasks**
- `import mcp "<server>" as x`: read the server's tool schemas (from a local config or a lockfile) and generate typed tool signatures.
- Label config: which parameters are sinks, which results are untrusted (conservative defaults: all results untrusted, all write-type tools are sinks).
- `ward.lock` pins the tool schemas so builds are reproducible.

**Done when:** the Gmail-like mock MCP server example type-checks and the injection case is rejected.

---

## M8: Proof and polish (ongoing)
- TypeScript backend (`ws_codegen_ts`) + napi-rs runtime binding.
- AgentDojo port: publish how many attacks are rejected at compile time and how many normal tasks still succeed (utility).
- LSP (`tower-lsp`) + VS Code extension (syntax highlighting for `.wardscript`).
- `salsa` for incremental checking.
- WASM backend via `wasm-encoder` (sandboxed execution).

---

## Working method
- Start each session with: "Read AGENTS.md and PLAN.md. We are on milestone Mx."
- Ask for tests first on M4 and M5: "write the attack cases, confirm they fail, then implement".
- Keep PRs small: one feature at a time, each with its tests and spec update.
- After each milestone, update `docs/spec/` and record decisions in `docs/decisions/NNN-*.md`.
