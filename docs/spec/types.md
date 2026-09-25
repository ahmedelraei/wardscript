# Types

Status: implemented in M2 (`ward_check`). Trust labels ([trust](trust.md), M4) and effects ([effects](effects.md), M5) are
tracked separately from these types.

## The types

| Type | Values |
|---|---|
| `Int`, `Float` | 64-bit integer and float. No implicit conversion between them. |
| `String`, `Bool` | |
| `List<T>` | immutable list |
| `Map<K, V>` | immutable map |
| `Option<T>` | `Some(x)` or `None` |
| records | `type Ticket { title: String }`, generic: `type Page<T> { items: List<T> }` |
| enums | `enum Shape { Point, Circle(Float) }` |
| aliases | `type Outcome = Result<String, String>` (expanded; may be generic, not recursive) |
| `Untrusted<T>`, `Trusted<T>` | the type `T`, with a trust label ([trust](trust.md)) |

A function without `-> T` returns `()`. `()` can't be written as a type.

## Checking

Checking is bidirectional with unification. Signatures must be fully annotated;
inside a body, `let` types are inferred (`let xs = [];` gets its element type from
later use; if it never does, that's W0121).

- A block's type is its final expression's type, `()` if it has none, or *never*
  if it always `return`s. *never* fits any expected type. Where `()` is expected,
  the final expression's value is discarded, whatever its type.
- Errors are exceptions; see [Exceptions](#exceptions).
- `if` without `else` has type `()`. With `else`, both branches must agree.
- `match` arms must agree, and the arms must be **exhaustive** (W0117). The check
  understands nested patterns over `Bool`, `Option` and enums. Numbers
  and strings need a catch-all arm. Arms that can never match are warned about (W0127).
- Operators: `+ - * / %` on `Int` or `Float` (both sides the same type); `+` also
  concatenates `String`s and `List`s. `< <= > >=` on numbers and strings. `== !=`
  on any two values of the same type. `&& || !` on `Bool`.
- `xs[i]` indexes a `List` with an `Int` or a `Map` with its key type.
  `for x in xs` loops over a `List`'s elements or a `Map`'s keys.
- Generic functions are instantiated at each call: `fn first<T>(xs: List<T>) -> Option<T>`.

## Exceptions

A function declares what it may throw after its return type:

```wardscript
fn parse_level(s: String) -> Int throws ParseError {
    if s.is_empty() {
        throw ParseError.Empty
    }
    ...
}
```

- `throw e` raises `e`. Any type can be thrown (a `String`, a record, an enum);
  a function throws one type.
- A call to a function that `throws` must be marked with `?` (W0128), so every
  place a function can exit early is visible: `let n = parse_level(s)?`. `?` on a
  call that can't throw is an error (W0118).
- A thrown error must be handled: either an enclosing `try` catches it, or the
  function declares `throws` with the same type (W0129, W0110).
- `try { ... } catch err { ... }` is an expression. Its value is the `try`
  block's, or the handler's if something was thrown; `err` has the thrown type
  (`catch _` ignores it). One `try` catches one error type. A `try` whose block
  can't throw is warned about (W0131).
- `ai fn` can't declare `throws` (W0130): a failed model call is a runtime error.

There is no `Result` type: exceptions are the only error mechanism.

## AI functions

An `ai fn`'s return type must have a JSON schema (W0120), because the
model's answer is validated against it: `Int`, `Float`, `String`, `Bool`, `List`,
`Option`, `Map<String, _>`, and records and enums made of those.

## Refinements

A **refinement** narrows a type with a condition on the value, called `it`:

```ward
type Subject = String where it.len() <= 80 && !it.contains("\n")

pub type Review {
    stars: Int where it >= 1 && it <= 5,
    tags: List<String> where it.len() <= 3,
}

ai fn tagline(product: String) -> String where it.len() < 60 { "..." }
```

- The condition must be a `Bool` using only `it`, literals, operators, fields,
  built-in methods and enum variants: no function calls, `if`, `match` or blocks
  (W0132). It can't see anything but `it`.
- Refinements describe decoded data, so they go on record fields, variant
  payloads, type aliases and `ai fn` return types (W0133 elsewhere). They can't be
  written inside type arguments, where `>` would be ambiguous; name the refined
  type with an alias instead (`List<Subject>`).
- For type checking, `T where ...` is `T`.
- They are checked when a value is decoded: every model answer (a failed
  refinement is retried with the reason, like any invalid answer), tool results
  and `ward run` arguments. Values the program builds itself aren't re-checked.
- Where the condition's shape allows, it becomes JSON Schema, which structured
  output can enforce: comparisons of `it.len()` with a number (`minLength`,
  `maxLength`, `minItems`, `maxItems`) and of a number `it` (`minimum`, `maximum`,
  `exclusiveMinimum`, `exclusiveMaximum`, `const`), joined with `&&`. Every
  refinement's text also goes in the schema's `description`.

## Checks on answers

An `ai fn` can check its answer, `it`, with a `check` clause:

```ward
ai fn reply(name: String, email: Untrusted<String>) -> Reply
    check {
        it.body.contains(name) => "greet the customer by name",
        !it.body.contains("http"),
        is_polite(it.body) => "be polite",
    }
{
    "..."
}
```

Each condition is a `Bool` (W0110) that may use `it`, the parameters and any
function, including another `ai fn` as a judge. After an answer decodes, the
conditions run in order; the first that fails rejects the answer, and the reason
(the string after `=>`, else the condition's text) is sent back to the model with
the retry. Reasons are plain strings, without interpolation. When the retries run
out, the call fails with `AiOutputError` (or falls back to the next model, see
[model policies](runtime.md#model-policies)). Only an `ai fn` has a `check` clause
(W0134). What the checks call counts as the `ai fn`'s effects and budget, and they
are analyzed for trust like other code.

**Checks and refinements don't change trust.** An answer that passes them is still
`Untrusted`; only `validate`, `approve` and `declassify` make data trusted.

## Trust built-ins (typing only)

| Built-in | Type |
|---|---|
| `validate(x, rule)` | `T` where `x: T`, `rule` names a function `fn(T) -> Bool` (W0123); throws a `String` naming the failed rule, so calls are written `validate(x, rule)?` |
| `approve(x)` | `T` |
| `declassify(x, reason)` | `T`, with `reason: String` |

What these do to trust labels is specified in [trust](trust.md).

## Built-in methods

Lists and maps are immutable: `push` and `insert` return a new collection.

| Receiver | Methods |
|---|---|
| `String` | `len() -> Int`, `is_empty() -> Bool`, `contains/starts_with/ends_with(String) -> Bool`, `trim/lower/upper() -> String`, `split(String) -> List<String>`, `lines() -> List<String>`, `replace(String, String) -> String` |
| `List<T>` | `len() -> Int`, `is_empty() -> Bool`, `contains(T) -> Bool`, `get(Int) -> Option<T>`, `first/last() -> Option<T>`, `push(T) -> List<T>` |
| `Map<K, V>` | `len() -> Int`, `is_empty() -> Bool`, `get(K) -> Option<V>`, `contains_key(K) -> Bool`, `keys() -> List<K>`, `values() -> List<V>`, `insert(K, V) -> Map<K, V>` |
| `Option<T>` | `is_some/is_none() -> Bool`, `unwrap_or(T) -> T` |
| `Int` | `to_string() -> String`, `to_float() -> Float` |
| `Float` | `to_string() -> String`, `round() -> Int` |
| `Bool` | `to_string() -> String` |
