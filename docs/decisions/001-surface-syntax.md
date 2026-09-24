# 001: Surface syntax choices (M1)

**Status:** accepted, 2026-09-24

## Decisions

1. **Rust-like block and statement rules.** A block's last expression without `;`
   is its value. `if`, `match` and bare blocks at the start of a statement end it.
   This is familiar to most readers and keeps the grammar LL(1)-ish.
2. **No record literals in `if`/`while`/`match`/`for` heads** (Rust's restriction),
   so `if x { ... }` is never misparsed as a record literal `x { ... }`.
3. **String interpolation is `{expr}`, and literal braces are doubled (`{{`, `}}`)**,
   as in Python f-strings and Rust's `format!`. Python is the first backend, so its
   users already know the rule. `\{` is rejected with a hint to double the brace.
   JSON-heavy prompts will be better served by a raw/multi-line string form later.
4. **Interpolations are re-lexed from the file text**, so their spans point at the
   real source location and diagnostics inside templates are exact.
5. **`by llm` requires a string-literal prompt and a return type at parse time.**
   A prompt built at runtime would hide what data reaches the model, and the
   return type is what the output gets validated against.
6. **Type arguments use `<...>` and there is no `>>` token**, so nested generics
   (`Result<List<T>>`) need no special-casing.
7. **Keywords `llm`, `uses`, `budget` and `by` are reserved.** `llm` is still
   accepted as an effect name inside `uses {...}`.
8. **Comparisons are non-associative.** `a < b < c` is an error with a suggested fix
   instead of silently comparing a boolean.

## Consequences

- Items don't end with `;`; a stray `;` at the top level is reported as W0011.
- The pretty-printer drops comments. Keeping them (for a `ward fmt`) needs trivia
  in the token stream; deferred until there's a formatter.
