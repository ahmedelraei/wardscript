# WEP 017: The TypeScript backend

**Status:** Final. Accepted 2026-09-25.

## Decision

- **`ward build --target typescript`** generates TypeScript from the same WIR as
  the Python backend. `ward_codegen_ts` follows `ward_codegen_py`'s structure:
  expression-oriented code becomes statements with destinations, and operands are
  saved to temporaries so evaluation stays left to right.
- **Plain data.** Records are interfaces over plain objects, enums are unions of
  string literals or of `{tag, _0, ...}` objects, lists and maps are readonly
  arrays and `Map`s. Hosts can build and read values without the runtime; the
  runtime descriptions (`Ticket$`) exist only to decode model answers.
- **Everything is async**, since model calls, approvals and tools are.
- **Every variable is declared at the top** of its function (`let x!: T;`), so
  hoisted expressions can assign them from any branch, and the output type-checks
  under `strict`.
- **The runtime is written in TypeScript**, not a napi-rs binding of the Rust core
  as the plan said. A native module would need a build per platform for a few
  hundred lines of counters and trace writing; the TypeScript core writes the same
  trace format, which `ward trace show` reads, and is tested against it.
- **Tested end to end**: the Python e2e programs have TypeScript drivers
  (`tests/e2e/ts`), run with Node's test runner after the output is type-checked
  with `tsc --strict`. CI runs them with Node 22.

## Not done

- Streaming and partial values, OTLP export.
- `ward run` / `ward test` for TypeScript.
- `Int` beyond 2^53 (it's a `number`).
