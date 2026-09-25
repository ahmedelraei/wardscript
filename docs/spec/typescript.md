# The TypeScript backend

Status: implemented in M11 (`ward_codegen_ts`, the `wardscript` npm package in
`crates/ward_runtime/ts`). Design notes are in
[decision 017](../decisions/017-typescript-backend.md).

```bash
ward build examples/triage.ward --target typescript -o src/generated
```

```ts
import { configure, MockModel, Trusted } from "wardscript";
import { route } from "./generated/triage.ts";

configure({ model: new MockModel({ triage: { customer: "Ada", /* ... */ } }) });
console.log(await route("My checkout crashed"));
```

It generates one `.ts` module per Wardscript module. The checker, and so every
guarantee about trust, effects and budgets, is the same as for Python; only the
output differs.

## Values

| Wardscript | TypeScript |
|---|---|
| `Int`, `Float` | `number` (so `Int` is exact up to 2^53) |
| `String`, `Bool` | `string`, `boolean` |
| `()` | `null` (a function returning `()` returns `Promise<void>`) |
| `List<T>` | `ReadonlyArray<T>`; updates make copies |
| `Map<K, V>` | `ReadonlyMap<K, V>` (a `Map`); updates make copies |
| `Option<T>` | `_rt.Opt<T>`: `null`, or the value; `Some(None)` is `new Some(null)` |
| record `Ticket` | `interface Ticket` with readonly fields, used as plain objects |
| class `Agent` | `class Agent`, created with `await Agent$new(...)`, which awaits its `init` (`_init$Agent`) |
| enum with no fields | a union of string literals, `"Low" \| "Urgent"` |
| enum with fields | a union of `{ tag: "Bug", _0: string }` objects |

Each record and enum also has a runtime description, `Ticket$`, used to decode model
answers. `==` is structural (`_rt.eq`) except on numbers, strings, booleans and
enums without fields. `.len()` counts characters, as in Python, not UTF-16 units.

## Functions

Every function is `async` and returns a `Promise`. Public functions are exported.
A parameter that must be trusted takes `T | Trusted<T>`, and a plain `T` is refused
with `TrustError`, as in Python. Each outermost call is a run in the audit trace,
in the same format as Python's, so `ward trace show` reads it.

## The runtime

`import { ... } from "wardscript"`:

- `configure({model, models, approver, tools, retries, modelRetries, backoff,
  traceDir, checkSinks, unpriced})` and `reset()`, as in
  [Python](runtime.md#the-runtime);
- `MockModel`, `Raw`, `Seq`, `Usage`; `Anthropic` and `OpenAI` (using `fetch`, no
  SDK; API keys from `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`);
- `McpServer` and `loadMcpConfig("mcp.json")` for `tools`;
- the errors (`Thrown`, `AiOutputError`, `ModelError`, `BudgetExceeded`, ...),
  `Trusted`, `decode`, `encode`, `jsonSchema`, `lastRun()`.

Generated code imports `wardscript/rt`. Model policies, refinements, checks,
budgets (with unknown costs failing closed), runtime sink checks and typed tool
calls work as in Python.

## Using it

The runtime needs Node 22 or later. Build it with `npm run build` in
`crates/ward_runtime/ts` (or `npm install` that directory). Generated modules import
each other with `.ts` extensions, so compile them with TypeScript 5.7+ and
`allowImportingTsExtensions` with `rewriteRelativeImportExtensions`, or run them
with `node --experimental-strip-types`.

## Not yet

- Streaming (`on_stream`, partial values) and OTLP export.
- `ward run` and `ward test` run the Python output; they don't take `--target`.
- The Rust runtime core through napi-rs: the TypeScript core is written in
  TypeScript and writes the same trace format.
