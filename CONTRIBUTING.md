# Contributing to Wardscript

Thanks for helping. Wardscript is a small language whose one promise is that
untrusted data can't reach a sensitive action without `validate`, `approve` or
`declassify`. Most of the rules below exist to protect that promise.

- Found a security problem? Don't open an issue. See [SECURITY.md](SECURITY.md).
- Using AI tools? Read the [AI policy](docs/ai-policy.md). It's short.
- Everyone follows the [Code of Conduct](CODE_OF_CONDUCT.md).
- Not sure where to start? See [ROADMAP.md](ROADMAP.md).

## Before you write code

- **Bug fixes, new diagnostics, docs, performance, tooling:** open a PR directly,
  or an issue first if the fix isn't obvious.
- **Changes to syntax, types, trust labels, effects, budgets, backend output or
  what a diagnostic code means:** these need a WEP (Wardscript Enhancement
  Proposal) first. See [docs/weps/000-process.md](docs/weps/000-process.md). Open
  an issue to talk it through before writing one.

## Setup

You need the Rust toolchain pinned in `rust-toolchain.toml`. For the end-to-end
tests you also need Python 3.10+ and, for the TypeScript backend, Node 22.

```
cargo build
cargo test                       # unit, ui and attack tests
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo run -p ward_cli -- check examples/support.ward
```

CI runs all of these on every PR, plus the Python wheel and TypeScript runtime
tests (see `.github/workflows/ci.yml`). Tests that call real models are opt-in with
`WARD_LIVE=1` and never run in CI; CI uses a deterministic mock model.

## Where things are

| Path | What |
|---|---|
| `crates/ward_syntax` | lexer, parser, AST |
| `crates/ward_resolve` | names, modules, imports |
| `crates/ward_check` | types, trust labels, effects, budgets. **The core.** |
| `crates/ward_ir`, `ward_codegen_py`, `ward_codegen_ts` | IR and backends |
| `crates/ward_runtime` | Rust runtime core, Python package (`py/`), npm package (`ts/`) |
| `crates/ward_lsp`, `ward_cli`, `ward_wasm` | language server, `ward` binary, browser build |
| `tests/ui` | `.ward` programs and their snapshotted diagnostics |
| `tests/attacks` | injection programs that **must fail to compile** |
| `tests/e2e` | build, run against a mock model, check the output |
| `docs/spec` | the language spec |
| `docs/weps` | design decisions and why they were made |

## Rules for changes

- **Tests with every feature:** a parser test, a checker test (a `tests/ui`
  snapshot), and, if it touches trust, a `tests/attacks/` case.
- **Snapshots:** accept changes with `cargo insta review`, one by one, and explain
  every changed `.snap` file in the PR. Never accept them in bulk without reading.
- **Attack tests:** never edit or delete a `tests/attacks/` case to make it pass.
  Add new cases instead. A CI check flags PRs that modify or remove them.
- **Diagnostics:** codes (`W0xxx`) are stable. Never change what an existing code
  means; add a new one. `--format json` output must stay stable.
- **Library crates:** no `unwrap()` or `panic!`. Use typed errors (`thiserror`).
- **Data structures:** AST and IR nodes live in arenas (`la-arena`) and are
  referenced by ID, not `Rc<RefCell<...>>`.
- **Spec:** keep `docs/spec/` in sync with what you change.
- **Docs and examples:** use the `.ward` extension.

## Pull requests

- Keep them small: one change, with its tests and spec update.
- Fill in the PR template.
- You must be able to explain every line of your PR yourself, whether or not you
  used AI tools.
- The maintainer (@ahmedelraei) reviews and merges. Unproductive issues and PRs
  may be closed without explanation.

## License

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed under MIT and Apache-2.0, without any additional terms or conditions.
