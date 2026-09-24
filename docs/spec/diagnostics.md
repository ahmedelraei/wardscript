# Diagnostics

Every diagnostic has a stable code. A code's meaning never changes once assigned,
and retired codes are not reused. `ward check --format json` reports each
diagnostic with its `code`, `severity`, `message`, primary `span`, all `labels`
(each with a `span` and optional `message`; the first is `primary`), and an
optional `help`. Spans carry the byte `offset` plus a 1-based `line` and `column`
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

## Error recovery

The parser reports every independent syntax error in a file rather than stopping
at the first one. After an error it resynchronises at the next `,` or closing
delimiter inside a list, the next statement inside a block, or the next item
keyword at the top level. Follow-on errors are suppressed: at most one error per
token, none right after a character the lexer rejected, and no "unclosed" error
when the input ends inside an unterminated string.
