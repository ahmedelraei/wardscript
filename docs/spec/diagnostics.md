# Diagnostics

Every diagnostic has a stable code, listed in `ws_syntax::diag::codes` (a test fails
if a code is missing from this page or assigned twice). A code's meaning never changes once assigned,
and retired codes are not reused. `ward check --format json` reports each
diagnostic with its `code`, `severity`, `message`, primary `span`, all `labels`
(each with a `span` and optional `message`; the first is `primary`), and an
optional `help`, plus the `file` it's in (programs can span several modules). Spans carry the byte `offset` plus a 1-based `line` and `column`
(the column counts characters).

## W00xx: syntax

| Code | Meaning |
|---|---|
| W0001 | unexpected character |
| W0002 | unterminated string |
| W0003 | unknown escape sequence in a string |
| W0004 | malformed interpolation: unclosed `{`, unmatched `}`, empty `{}`, or not a single expression |
| W0005 | integer literal doesn't fit in 64 bits |
| W0010 | expected a specific token (also: keyword used as a name) |
| W0011 | expected an item (`fn`, `type`, `enum`, `import`) |
| W0012 | expected an expression |
| W0013 | expected a type |
| W0014 | expected a pattern |
| W0015 | missing `;` |
| W0016 | `by llm` prompt is not a string literal |
| W0017 | `by llm` function has no return type |
| W0018 | duplicate `uses` or `budget` clause |
| W0019 | chained comparison (`a < b < c`) |
| W0020 | invalid assignment target |
| W0021 | unclosed delimiter |
| W0022 | string interpolation where it isn't allowed (import sources, patterns) |
| W0023 | `pub` on an import |

## W010x: names and modules

| Code | Meaning |
|---|---|
| W0100 | unknown name in value position (with a "did you mean" suggestion) |
| W0101 | unknown type (also: unknown record in a record literal) |
| W0102 | imported module not found |
| W0103 | name defined more than once (items, parameters, fields, variants, pattern bindings) |
| W0104 | item of another module isn't `pub` |
| W0105 | no such member: enum variant or module item |
| W0106 | wrong kind of name: a type used as a value, or a value used as a type |
| W0107 | *reserved*: untrusted data reaches a sensitive action (M4) |

## W011x-W012x: types

| Code | Meaning |
|---|---|
| W0110 | mismatched types |
| W0111 | no such field |
| W0112 | wrong number of type arguments |
| W0113 | record literal is missing fields |
| W0114 | record literal sets a field twice |
| W0115 | wrong number of arguments (calls, variant constructors and variant patterns) |
| W0116 | calling something that isn't a function |
| W0117 | non-exhaustive `match` (names a value that isn't covered) |
| W0118 | invalid `?`: not a `Result`/`Option`, incompatible error type, or the function doesn't return one |
| W0119 | operator or `for`/index applied to a type that doesn't support it |
| W0120 | `by llm` return type has no JSON schema |
| W0121 | type can't be inferred; annotation needed |
| W0122 | no such method |
| W0123 | `validate` rule isn't a function `fn(T) -> Bool` |
| W0124 | function, variant with fields, builtin or namespace used as a value without calling it |
| W0125 | type alias refers to itself |
| W0126 | assignment to something that isn't a variable, field or list element |
| W0127 | *warning*: unreachable `match` arm |

## Error recovery

The parser reports every independent syntax error in a file rather than stopping
at the first one. After an error it resynchronises at the next `,` or closing
delimiter inside a list, the next statement inside a block, or the next item
keyword at the top level. Follow-on errors are suppressed: at most one error per
token, none right after a character the lexer rejected, and no "unclosed" error
when the input ends inside an unterminated string.

If any file has syntax errors, `ward check` stops after parsing: name and type
errors in code that didn't parse are mostly echoes of the syntax error.
