# Wardscript: guide for coding agents

Wardscript is a small, typed language for trustworthy AI functions and agents. What sets it apart is that the **compiler proves untrusted data can't reach sensitive actions** without an explicit `validate`, `approve` or `declassify` step.

- Source files use `.ward` (canonical) or `.wardscript`; the compiler treats both identically. Use `.ward` in all docs, examples and tests.
- CLI command: `ward` (`ward init | check | build | run | test | lock | trace | lsp`)
- The compiler and runtime core are written in Rust (a Cargo workspace)
- Backends: Python and TypeScript. Then WASM through `wasm-encoder`. **No LLVM or Cranelift.**

## Guiding principle
The checker is the product; backends are plumbing. When in doubt, spend effort on `ward_check` and on diagnostics.

## Workspace layout
```
crates/
  ward_syntax      # logos lexer, hand-written recursive-descent parser, AST (arena + IDs)
  ward_resolve     # name resolution, modules, imports (incl. MCP tools)
  ward_check       # types + trust labels + effects + budgets
  ward_ir          # WIR: fully typed IR with explicit labels + provenance metadata
  ward_codegen_py  # WIR -> Python (+ .pyi stubs)
  ward_codegen_ts  # WIR -> TypeScript
  ward_runtime     # runtime core (Rust) + py/ (the Python package, PyO3/maturin) + ts/ (the npm package)
  ward_lsp         # language server (`ward lsp`)
  ward_cli         # `ward` binary
  ward_wasm        # checker + codegen for the browser (the website's playground)
tests/
  ui/            # .ward programs + expected diagnostics (insta snapshots)
  attacks/       # injection programs that MUST fail to compile
  e2e/           # build to Python, run with a mocked LLM, check the output
examples/
docs/spec/       # language spec, kept up to date with the implementation
editors/vscode/  # VS Code extension: grammar + `ward lsp` client
```

## Rules
- Do only one milestone at a time (see PLAN.md). Don't start the next until the current milestone's acceptance checks pass.
- Every new language feature ships with: a parser test, a checker test (ui snapshot), and, if it touches trust, a `tests/attacks/` case.
- Diagnostics: use `ariadne` for human output. `--format json` must produce stable codes (`W0xxx`) with line/column spans and a `help` field. Never change an existing code's meaning.
- No `unwrap()` or `panic!` in library crates. Use typed errors (`thiserror`).
- AST and IR nodes live in arenas (`la-arena`) and are referenced by ID. Don't use `Rc<RefCell<...>>` graphs.
- Keep `docs/spec/` in sync with each feature you add.
- Changes to syntax, types, trust, effects, backend output or diagnostic meanings need a WEP first (`docs/weps/000-process.md`). Write it as a Draft before the code, and mark it Final once the spec is updated.
- Tests with real LLMs are opt-in (`WARD_LIVE=1`). CI uses a deterministic mock model.

## Commands
```
cargo build
cargo test                      # all unit + ui + attack tests
cargo insta review              # accept snapshot changes deliberately
cargo run -p ward_cli -- check examples/support.ward
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## Core semantics (short version; see docs/spec for details)
- Labels: `Trusted` < `Untrusted`. Operations produce the combined label of their inputs, taking the least trusted (`Trusted + Untrusted = Untrusted`).
- Branching on untrusted data taints everything written inside that branch (implicit flow).
- Values from LLM output, tools, network, files or the host language are `Untrusted` by default.
- Sensitive actions (tool arguments marked `sink`) require `Trusted` arguments.
- `validate(x, rule)`, `approve(x)` and `declassify(x, reason)` are the only ways to turn `Untrusted` into `Trusted`, and each one is recorded in the audit trace.
- Effects: `uses {llm, net.read, mail.send, ...}`. A caller must declare everything its callees use.
- Rule of Two: a function may not have all three at once: untrusted input, sensitive-data reads and external-state writes. Allowed only with an explicit `@allow(rule_of_two, reason = "...")`.
