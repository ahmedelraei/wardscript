# Python backend and runtime

Status: implemented in M3 (`ward_ir`, `ward_codegen_py`, `crates/ward_runtime/py`);
trust at the host boundary in M4, budgets in M5; the Rust core, audit trace,
model providers, sink checks, collectors, async code and streaming in M6. The design
is recorded in decisions [006](../decisions/006-python-backend.md),
[009](../decisions/009-runtime-core-and-trace.md) and
[010](../decisions/010-sink-checks-async-streaming.md).

## Building

```bash
ward build app.ward -o build      # build/app.py, build/app.pyi
ward build app.ward --async       # async def functions, for asyncio hosts
ward run app.ward route '"an email"' --mock answers.json
ward run app.ward route '"an email"' --model anthropic
ward trace show                         # the latest run's trace
```

`ward build --target python` writes one `.py` module and one `.pyi` stub per
Wardscript module; `import support.tickets` becomes `build/support/tickets.py`.
Generated code needs Python 3.10+ and the `wardscript` runtime package. Build and
install it with its Rust core:

```bash
cd crates/ward_runtime/py && maturin build --release -o dist && pip install dist/*.whl
```

The package also works from source (`crates/ward_runtime/py` on the import path),
with a pure-Python core instead of the Rust one; `wardscript.core.IMPLEMENTATION`
says which is in use. Both write the same traces, and tests check they agree.

`ward run FILE FUNCTION ARGS...` builds to a temporary directory and calls a
function of the entry module. Arguments are JSON values, decoded by the
parameters' types; the result is printed as JSON. `--mock FILE` answers `ai fn`
calls from a JSON object keyed by function name; `--model anthropic`,
`anthropic:<model>` or `openai:<model>` uses a real model instead
([providers](#model-providers)). Arguments come from whoever runs the command, so
`ward run` vouches for them (see below). The run's audit trace goes to
`.ward/traces` (`--trace-dir DIR`, or `--no-trace`), and its id is printed on stderr.
`ward run` embeds the runtime (with the pure-Python core), so it doesn't need the
package installed. It uses `python3`, or `WARD_PYTHON`.

## Values

| Wardscript | Python |
|---|---|
| `Int`, `Float`, `String`, `Bool` | `int`, `float`, `str`, `bool` |
| `()` | `None` |
| `List<T>`, `Map<K, V>` | `list`, `dict` (never mutated) |
| `Option<T>` | `None`, or the value itself; `Some(x)` is `wardscript.Some(x)` only when `x` is itself `None` or a `Some` |
| record `Ticket` | frozen dataclass `Ticket` |
| enum without fields `Priority` | `enum.Enum`; `Priority.Low.value == "Low"` |
| enum with fields `Verdict` | class `Verdict`; variants are frozen dataclasses `Verdict.Pass()`, `Verdict.Fail("log")` with fields `_0`, `_1`, ... |
| tool results | whatever the tool returned |

Names that are Python keywords, or the builtins `float`, `isinstance`, `len`,
`list` and `str`, get a trailing `_` (`from_`). A module's `pub` items are its
`__all__`; the stub declares every record and enum and the `pub` functions.

Semantics that differ from Python's and are preserved:

- Assigning to a field or element (`t.x = 1`, `xs[i] = v`) rebinds the variable
  to an updated copy. Values are never mutated in place.
- `/` on `Int` truncates toward zero and `%` takes the sign of the dividend.
  `Int` division by zero and out-of-bounds indexing (`xs[i]`, including negative
  `i`) raise `PanicError`; a missing map key in `m[k]` too. `Float` arithmetic
  follows IEEE 754: `1.0 / 0.0` is infinity.
- `Float.round()` rounds halves away from zero.
- String templates print `Bool`s as `true`/`false`, `None` as `None`, enum
  variants by name, and records, lists and maps as JSON.
- Evaluation is left to right, including when an argument is an `if`, `match`
  or block that compiles to statements.

## Exceptions

`throw e` raises `wardscript.Thrown`, with the thrown value in `.value`; a
Wardscript `try` catches only these. A failed `validate` throws the `String`
``validation failed: `rule` rejected the value``.

Everything else the runtime raises is a `wardscript.WardError`, which no `try`
catches:

| Error | Raised when |
|---|---|
| `NoModelError` | an `ai fn` is called with no model configured |
| `AiOutputError` | the model's answers didn't match the return type on every attempt; `.errors` says why, per attempt |
| `BudgetExceeded` | a function went over its `budget`; `.function`, `.resource`, `.limit`, `.used` |
| `BudgetUnenforceable` | a function has a `cost` budget but the model's cost is unknown; `.function`, `.when` (`before` or `after`); see [unknown cost](#unknown-cost) |
| `ModelError` | a provider failed to answer after its retries and every fallback; `RateLimited` and `ModelUnavailable` (timeouts, 5xx) are retryable, `.status` has the HTTP status |
| `ApprovalDenied` | `approve` was refused, or no approver is configured |
| `ToolError` | a tool or tool function isn't configured |
| `TrustError` | the host passed a parameter that must be trusted without vouching for it |
| `PanicError` | integer division by zero, an index out of bounds, a missing map key |
| `DecodeError` | a JSON value doesn't match a type (`ward run` arguments, `wardscript.decode`) |

## The audit trace

A **run** is one call from the host into Wardscript; calls between Wardscript
functions are part of it. Each run gets an id (sortable by start time) and a trace:
one JSON object per line, in `<trace_dir>/<run>.jsonl`, written as events happen.

Every record has `run`, `seq`, `time` (Unix nanoseconds) and a `kind`:

| `kind` | Fields |
|---|---|
| `run_start` | `function`; `args`, each with `name`, `value`, `vouched` and `leaves` |
| `ai_call` | `started`, `function`, `attempt` (counting every request of the call), `model` (the alias asked, or `null` for the default model), `prompt`, `answer`, `tokens`, `cost` (`null` when unknown), `error` (why it was rejected), `leaves` of the decoded output |
| `tool_call` | `started`, `tool`, `function`, `site`, `args`, `digests` of the args, `error`, `leaves` of the result |
| `validate` | `rule`, `site`, `passed`, `leaves` of the checked value |
| `approve` | `site`, `approved`, `leaves` |
| `declassify` | `site`, `reason`, `leaves` |
| `budget_exceeded` | `function`, `resource`, `limit`, `used` |
| `budget_unenforceable` | `function`, `when` (`before` or `after`) |
| `run_end` | `status` (`ok`, `threw`, `error`), `error`, and the run's `tokens`, `calls`, `cost` |

`leaves` are `{path, digest}` for a value and each of its parts (`$`, `$.subject`,
`$.items[0]`); a digest is FNV-1a 64 of the value's canonical JSON. They let
`ward trace show` link a value that reached a tool back through the check that
cleared it to where it came from, without the trace holding more than the events:

```text
#4   tool   `gmail.send` at support.ward:61:9
         arg 1: "ada@example.com" ← argument `to` from the host, vouched for by the host
         arg 2: "Your refund" ← $.subject of a value approved by a human (#3, support.ward:60:24) ← that value: the output of `ai fn draft_reply` (#2), untrusted
```

A value computed from several sources (a concatenation) has no exact match, and is
shown as such. `ward trace show [RUN]` takes a run id or a unique prefix, and shows
the latest run without one; `ward trace export [RUN] --format otlp` prints the run
as OpenTelemetry spans in OTLP/JSON (the run is the root span, model and tool calls
its children, and checks, approvals and declassifications events on the root),
`--format jsonl` the trace as written. Both read `--dir`, else `WARD_TRACE_DIR`,
else `.ward/traces`.

### Sending runs to a collector

`ward trace export [RUN] --endpoint http://localhost:4318` POSTs the spans to an
OTLP/HTTP collector (at `/v1/traces`, JSON encoding) instead of printing them. A
program can send each run itself as it ends, with
`runtime.configure(otlp_endpoint=...)` or `OTEL_EXPORTER_OTLP_ENDPOINT`. If the
collector can't be reached, the runtime warns and the run carries on; the trace
file still has everything.

## Model providers

`wardscript.providers` has `Model`s for real APIs, which report the tokens used (and,
given prices, the cost) to budgets:

```python
from wardscript.providers.anthropic import Anthropic
from wardscript.providers.openai import OpenAI

runtime.configure(model=Anthropic("claude-sonnet-5", prices=(3.0, 15.0)))
runtime.configure(model=OpenAI("<model>", prices=(..., ...)))
```

- `Anthropic` gives the return type's schema as a tool the model must call; `OpenAI`
  gives it as a JSON-schema response format. Either way the answer is still decoded
  and retried as usual.
- `prices` are dollars per million input and output tokens. Without them the cost
  is unknown (`Completion.cost` is `None`), and a `cost` budget refuses the call
  ([unknown cost](#unknown-cost)).
- They need the SDK (`pip install wardscript[anthropic]` or `[openai]`) and read
  `ANTHROPIC_API_KEY` or `OPENAI_API_KEY`; `client=` takes a configured client.
- `providers.load("anthropic:claude-sonnet-5")` is what `ward run --model` uses.

Tests against real models are opt-in: `WARD_LIVE=1 cargo test -p ward_cli --test live`
runs the triage and support examples (`tests/live`) with `WARD_LIVE_MODEL`, default
`anthropic`.

## Budgets

A function with a `budget` runs inside `wardscript._rt.budget(...)`, which charges
every model call made while it runs, in callees too. Going over raises
`BudgetExceeded`; see [effects](effects.md#budgets) for when each resource is checked.

## Model policies

An `ai fn` can say which models it asks and how it retries them:

```ward
ai fn triage(email: Untrusted<String>) -> Ticket
    model {primary: fast, fallback: [smart, backup], retries: 2, backoff: 0.5}
{
    "..."
}
```

| Setting | Value | Default |
|---|---|---|
| `primary` | the alias of the model asked first | the configured `model` |
| `fallback` | an alias, or a list of them, tried in order | none |
| `retries` | retries of a request that failed with a retryable `ModelError` | `configure(model_retries=...)`, 2 |
| `backoff` | seconds before the first such retry; each next one waits twice as long | `configure(backoff=...)`, 1.0 |

Aliases are bound at run time: `configure(models={"fast": ..., "smart": ...})`
(`ward run --model fast=anthropic:<model>`; aliases left out use the plain
`--model`, and `--mock` answers for all of them). An alias that isn't configured
raises `NoModelError`. Mistakes in the clause itself are W0230 and W0231.

Each model in turn:

1. A request that fails with a retryable `ModelError` (`RateLimited`,
   `ModelUnavailable`) is sent again after the backoff, up to `retries` times.
2. An answer that doesn't fit the return type is retried with the error, as always
   (`configure(retries=...)`).
3. When the model keeps failing, or its answers stay invalid, the next one is
   tried. A `ModelError` that isn't retryable (a rejected request) moves on at
   once.

An answer that doesn't satisfy a [refinement](types.md#refinements) (while it is
decoded) or fails a [check](types.md#checks-on-answers) counts as invalid: it is
retried with the reason, and its `ai_call` record has the reason as its `error`
(`the answer failed a check: ...`). Generated code passes the checks as a
function of the answer that returns the first failed reason, or `None`
(`_rt.ai(..., check=...)`); in `--async` code it is awaited. Refined types are
`_rt.Refined(base, condition, text, schema)` descriptors.

When every model fails, the last one's error is raised: its `ModelError`, or
`AiOutputError`. Other exceptions from a model aren't retried. The providers turn
their SDK's errors into `ModelError`s by HTTP status, and turn off the SDK's own
retries so the policy decides.

Every request counts against budgets: `calls` before it is sent, so a retry over
the limit is never sent; `tokens` and `cost` from the model that answered (a failed
request costs nothing); and the [unknown cost](#unknown-cost) check applies to each
model. Backoff sleeps count against `time`. Each request is an `ai_call` in the
trace, with its `model` and, for a failed one, its error. The mock model raises
exceptions given as answers, e.g. `Seq(RateLimited("429"), answer)`.

### Unknown cost

A `cost` budget fails closed when the cost can't be counted:

- **Before a request**, if a `cost` budget is active and the model has
  `prices=None` (a provider built without prices), the runtime raises
  `BudgetUnenforceable` with `when="before"` and sends nothing.
- **After an answer**, if its cost is unknown (`Completion.cost` is `None`, or the
  model returned plain text) and a `cost` budget is active, it raises
  `BudgetUnenforceable` with `when="after"`.

Either way a `budget_unenforceable` record goes into the trace. Without a `cost`
budget, unknown costs are fine and count as 0. `configure(unpriced="warn")` turns
the error into a `RuntimeWarning`, once per function, and counts those calls as
free. The mock model's answers cost 0 unless a `Usage` says otherwise. Both runtime
cores make the same decision (`Budget.unenforceable(cost)`).

## Trust at the host boundary

The checker proves that data inside the program reaches sinks only after
`validate`, `approve` or `declassify` ([trust](trust.md)). What the host passes in is
untrusted by default. So a function parameter that reaches a sink (a tool
argument, directly or through other functions, or a parameter declared `Trusted<T>`)
only accepts a value the host vouches for, by wrapping it in `wardscript.Trusted`:

```python
from wardscript import Trusted

support.handle(email, Trusted("ada@example.com"))
support.handle(email, "ada@example.com")   # raises TrustError
```

Such parameters are annotated `Trusted[T]` in the generated code and stubs. Calls
between Wardscript functions wrap arguments the checker proved trusted, so the
same function works from both sides. Other parameters take plain values.

### Sink checks

As defense in depth behind the checker, the runtime also checks every tool
argument before calling the tool. It uses the trace's digests. If an argument,
or a part of one, is exactly a value that came from an untrusted source in this
run, and no `validate`, `approve` or `declassify` cleared that value and the host
didn't vouch for it, the tool isn't called. Untrusted sources are model outputs,
tool results and unvouched host arguments. The call raises `TrustError` and is
recorded as a `tool_call` with that error.

This catches what a checker bug or edited generated code would let through
directly. It doesn't catch values built from untrusted ones, such as
`"Re: " + subject`: exact matches are all the trace has. That's the checker's job.
Strings shorter than 8 characters, numbers and booleans are skipped, since they
match by chance. `configure(check_sinks=False)` turns the check off. For a tool
with a schema, only its sink parameters are checked ([tools](tools.md#trust)).

## The runtime

```python
from wardscript import runtime
from wardscript.mock import MockModel, Raw, Seq

runtime.configure(
    model=MockModel({"triage": {"customer": "Ada", ...}}),
    approver=lambda request: ask_a_human(request.value, request.site),
    tools={"gmail": gmail_client},
    retries=2,
)
```

`configure` only changes the settings it's given; `runtime.reset()` restores the
defaults.

- **`model`**: anything with `complete(request: AiRequest) -> str | Completion`
  (and optionally `stream`, [below](#streaming)),
  returning JSON text, or a `Completion(text, tokens, cost)` that also says what the
  answer cost, for [budgets](effects.md#budgets) (`cost=None`, the default, means
  unknown; plain text has an unknown cost too). A model with a `prices` attribute
  set to `None` is known to be unpriced. `AiRequest` has the `function` name, the `prompt` with arguments filled in,
  the JSON `schema` of the return type, the `attempt` number and the `errors` of
  earlier attempts; `request.instructions()` combines them into one prompt.
- **`approver`**: called by `approve(x)` with an `ApprovalRequest(value, site, run)`;
  `site` is where `approve` was written, e.g. `support.ward:60:24`, and `run`
  the id of the run's trace. Returning `False` raises `ApprovalDenied`. It may be
  `async`: the program waits for the coroutine, on its own event loop (in a worker
  thread if one is already running). A model's `complete` may be `async` too.
- **`tools`**: implementations for `import mcp "source" as x`, keyed by `source`.
  Each is a mapping of functions or an object with a method per tool function;
  `x.send(a, b)` calls `tools["source"].send(a, b)`. An object with a
  `call_tool(name, arguments)` method, like an MCP server from
  `wardscript.mcp.load_config("mcp.json")`, gets named arguments instead when the
  tool has a schema ([tools](tools.md#at-run-time)).
- **`retries`**: extra attempts after an invalid model answer (default 2).
- **`trace_dir`**: where each run's audit trace is written; else `WARD_TRACE_DIR`,
  else nowhere. `runtime.last_run()` has the last run's `id`, `path` and `records`
  either way.
- **`otlp_endpoint`**: an OTLP/HTTP collector each run is also sent to
  ([above](#sending-runs-to-a-collector)).
- **`check_sinks`**: the runtime [sink checks](#sink-checks) (default on).
- **`on_stream`**: called with a `StreamChunk(function, attempt, delta, text)` for
  each piece of a streamed answer ([streaming](#streaming)).

### Async code

`ward build --async` generates `async def` functions. They await calls to each
other, the model, the approver, tools and `validate` rules, using the runtime's
`ai_async`, `approve_async`, `call_tool_async` and `validate_async`. Models,
approvers and tools may then be sync or `async`. Each asyncio task that calls
in is its own run, so concurrent runs (`asyncio.gather`) get separate traces and
budgets. The generated code is otherwise the same as the synchronous build, and
its stubs declare `async def`.

### Streaming

A model can also have `stream(request)`, yielding the answer's text in pieces
(sync or async). It may end with a `Completion` that gives the usage, and, if its
text isn't empty, the final answer. The runtime streams instead of calling
`complete` when `on_stream` or `on_partial` is set or a `tokens` budget is active. After each
piece, the tokens so far are estimated. If that goes over a `tokens` budget, the
stream is closed and the call raises `BudgetExceeded`, without waiting for the
rest of the answer. Both providers stream. Their pieces are the raw
`{"value": ...}` JSON the API returns (they say so with `stream_wraps_value`).

**Partial values.** `configure(on_partial=f)` calls `f` with a
`PartialValue(function, attempt, value, done)` each time the answer decodes further:
a record the model is still writing is a `Partial(cls, fields)` with the fields
that have started (read them as attributes); a string grows as it's written;
numbers, booleans and enum variants appear once complete; lists hold their
elements so far. Refinements and checks are only judged on the whole answer: the
last call, with `done=True`, has the complete value after it passed them. A retry
starts again from nothing, with the next `attempt`.

### `ai fn` calls

An `ai fn` builds its prompt from the template, then asks the model. The answer
must be JSON matching the return type's schema (JSON Schema 2020-12): records are
objects with every field required and no others, enums without fields are
strings, and enums with fields are `"Variant"` or `{"Variant": [fields...]}`.
Records and enums are named in `$defs`. An answer that isn't JSON or doesn't
match is retried, with the error passed to the model in `AiRequest.errors`;
after `retries + 1` attempts the call raises `AiOutputError`.

### The mock model

`MockModel(answers)` answers each `ai fn` by name. An answer is a value (encoded
as JSON; records and enums work), `Raw(text)` for literal text,
`Usage(answer, tokens=, cost=)` for an answer with its cost, `Seq(a, b, ...)`
for one answer per call, or a function of the `AiRequest`. Every request is
recorded in `model.calls`. `MockModel.from_json(text)` reads the `--mock` format.
