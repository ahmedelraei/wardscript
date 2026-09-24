# Python backend and runtime

Status: implemented in M3 (`ward_ir`, `ward_codegen_py`, `crates/ward_runtime/py`).
Trust labels at the host boundary (M4), budgets (M5) and the audit trace (M6) are
not enforced by the runtime yet. The design is recorded in
[decision 006](../decisions/006-python-backend.md).

## Building

```bash
ward build app.wardscript -o build      # build/app.py, build/app.pyi
ward run app.wardscript route '"an email"' --mock answers.json
```

`ward build --target python` writes one `.py` module and one `.pyi` stub per
Wardscript module; `import support.tickets` becomes `build/support/tickets.py`.
Generated code needs Python 3.10+ and the `wardscript` runtime package
(`crates/ward_runtime/py`) on the import path.

`ward run FILE FUNCTION ARGS...` builds to a temporary directory and calls a
function of the entry module. Arguments are JSON values, decoded by the
parameters' types; the result is printed as JSON. `--mock FILE` answers `ai fn`
calls from a JSON object keyed by function name. `ward run` embeds the runtime, so
it doesn't need the package installed. It uses `python3`, or `WARD_PYTHON`.

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
| `ApprovalDenied` | `approve` was refused, or no approver is configured |
| `ToolError` | a tool or tool function isn't configured |
| `PanicError` | integer division by zero, an index out of bounds, a missing map key |
| `DecodeError` | a JSON value doesn't match a type (`ward run` arguments, `wardscript.decode`) |

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

- **`model`**: anything with `complete(request: AiRequest) -> str`, returning JSON
  text. `AiRequest` has the `function` name, the `prompt` with arguments filled in,
  the JSON `schema` of the return type, the `attempt` number and the `errors` of
  earlier attempts; `request.instructions()` combines them into one prompt.
- **`approver`**: called by `approve(x)` with an `ApprovalRequest(value, site)`;
  `site` is where `approve` was written, e.g. `support.wardscript:60:24`. Returning
  `False` raises `ApprovalDenied`.
- **`tools`**: implementations for `import mcp "source" as x`, keyed by `source`.
  Each is a mapping of functions or an object with a method per tool function;
  `x.send(a, b)` calls `tools["source"].send(a, b)`.
- **`retries`**: extra attempts after an invalid model answer (default 2).

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
as JSON; records and enums work), `Raw(text)` for literal text, `Seq(a, b, ...)`
for one answer per call, or a function of the `AiRequest`. Every request is
recorded in `model.calls`. `MockModel.from_json(text)` reads the `--mock` format.
