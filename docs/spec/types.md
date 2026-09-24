# Types

Status: implemented in M2 (`ws_check`). Trust labels (M4) and effects (M5) are
tracked separately from these types.

## The types

| Type | Values |
|---|---|
| `Int`, `Float` | 64-bit integer and float. No implicit conversion between them. |
| `String`, `Bool` | |
| `List<T>` | immutable list |
| `Map<K, V>` | immutable map |
| `Option<T>` | `Some(x)` or `None` |
| `Result<T, E>` | `Ok(x)` or `Err(e)` |
| records | `type Ticket { title: String }`, generic: `type Page<T> { items: List<T> }` |
| enums | `enum Shape { Point, Circle(Float) }` |
| aliases | `type Outcome = Result<String, String>` (expanded; may be generic, not recursive) |
| `Untrusted<T>`, `Trusted<T>` | for now, identical to `T`; M4 gives them meaning as labels |

A function without `-> T` returns `()`. `()` can't be written as a type.

## Checking

Checking is bidirectional with unification. Signatures must be fully annotated;
inside a body, `let` types are inferred (`let xs = [];` gets its element type from
later use; if it never does, that's W0121).

- A block's type is its final expression's type, `()` if it has none, or *never*
  if it always `return`s. *never* fits any expected type.
- `if` without `else` has type `()`. With `else`, both branches must agree.
- `match` arms must agree, and the arms must be **exhaustive** (W0117). The check
  understands nested patterns over `Bool`, `Option`, `Result` and enums. Numbers
  and strings need a catch-all arm. Arms that can never match are warned about (W0127).
- `e?` on `Result<T, E>` gives `T` and returns `Err(e)` early; the function must
  return `Result<_, E>` with the same `E`. On `Option<T>`, the function must return
  an `Option`.
- Operators: `+ - * / %` on `Int` or `Float` (both sides the same type); `+` also
  concatenates `String`s and `List`s. `< <= > >=` on numbers and strings. `== !=`
  on any two values of the same type. `&& || !` on `Bool`.
- `xs[i]` indexes a `List` with an `Int` or a `Map` with its key type.
  `for x in xs` loops over a `List`'s elements or a `Map`'s keys.
- Generic functions are instantiated at each call: `fn first<T>(xs: List<T>) -> Option<T>`.

## AI functions

An `ai fn`'s return type must have a JSON schema (W0120), because the
model's answer is validated against it: `Int`, `Float`, `String`, `Bool`, `List`,
`Option`, `Map<String, _>`, and records and enums made of those.

## Trust built-ins (typing only)

| Built-in | Type |
|---|---|
| `validate(x, rule)` | `Result<T, String>` where `x: T` and `rule` names a function `fn(T) -> Bool` (W0123) |
| `approve(x)` | `T` |
| `declassify(x, reason)` | `T`, with `reason: String` |

What these do to trust labels is specified in M4.

## Built-in methods

Lists and maps are immutable: `push` and `insert` return a new collection.

| Receiver | Methods |
|---|---|
| `String` | `len() -> Int`, `is_empty() -> Bool`, `contains/starts_with/ends_with(String) -> Bool`, `trim/lower/upper() -> String`, `split(String) -> List<String>`, `lines() -> List<String>`, `replace(String, String) -> String` |
| `List<T>` | `len() -> Int`, `is_empty() -> Bool`, `contains(T) -> Bool`, `get(Int) -> Option<T>`, `first/last() -> Option<T>`, `push(T) -> List<T>` |
| `Map<K, V>` | `len() -> Int`, `is_empty() -> Bool`, `get(K) -> Option<V>`, `contains_key(K) -> Bool`, `keys() -> List<K>`, `values() -> List<V>`, `insert(K, V) -> Map<K, V>` |
| `Option<T>` | `is_some/is_none() -> Bool`, `unwrap_or(T) -> T` |
| `Result<T, E>` | `is_ok/is_err() -> Bool`, `unwrap_or(T) -> T` |
| `Int` | `to_string() -> String`, `to_float() -> Float` |
| `Float` | `to_string() -> String`, `round() -> Int` |
| `Bool` | `to_string() -> String` |
