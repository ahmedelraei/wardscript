# Syntax

Status: implemented in M1 (`ws_syntax`). This covers the grammar only; what the
constructs *mean* (types, trust labels, effects) is specified in later sections.

## Lexical structure

- Source is UTF-8. Whitespace is insignificant. `//` starts a comment that runs
  to the end of the line.
- **Identifiers**: `[A-Za-z_][A-Za-z0-9_]*`. A lone `_` is the wildcard pattern.
- **Keywords** (reserved; can't be used as names):
  `ai fn pub let type enum match if else for in while return import as uses budget true false`
- **Integers**: `[0-9][0-9_]*`, 64-bit signed. `_` separators are ignored (`1_000`).
- **Floats**: `[0-9][0-9_]*.[0-9][0-9_]*`. No exponent form; a leading digit is required.
- **Strings**: `"..."`, may span lines. Escapes: `\n \r \t \0 \\ \"`.
- **String templates**: `{expr}` inside a string interpolates an expression, e.g.
  `"Hello {user.name}"`. A literal brace is written doubled, `{{` or `}}`, as in Python f-strings. The expression
  can't contain a `"` (so no nested string literals).
- **Operators and punctuation**:
  `( ) { } [ ] , ; : . -> => + - * / % == != < <= > >= && || ! = ?`

## Grammar

```ebnf
module      = item* ;
item        = import | ["pub"] (fn | ai_fn | type | enum) ;

import      = "import" path ["as" IDENT]                 (* module import *)
            | "import" IDENT STRING "as" IDENT ;         (* tool import: import mcp "gmail" as mail *)

fn          = "fn" IDENT signature block ;
ai_fn       = "ai" "fn" IDENT signature "{" STRING "}" ;   (* the body is only the prompt *)
signature   = [generics] "(" [param ("," param)* [","]] ")" ["->" type] clause* ;
param       = IDENT ":" type ;
clause      = "uses" "{" [effect ("," effect)* [","]] "}"
            | "budget" "{" [IDENT ":" expr ("," IDENT ":" expr)* [","]] "}" ;
effect      = IDENT ("." IDENT)* ;                       (* llm, net.read, mail.send *)

type_decl   = "type" IDENT [generics] "{" [field ("," field)* [","]] "}"   (* record *)
            | "type" IDENT [generics] "=" type ;                           (* alias *)
field       = IDENT ":" type ;
enum        = "enum" IDENT [generics] "{" [variant ("," variant)* [","]] "}" ;
variant     = IDENT ["(" type ("," type)* [","] ")"] ;
generics    = "<" IDENT ("," IDENT)* [","] ">" ;

type        = path ["<" type ("," type)* [","] ">"] ;    (* String, List<T>, Untrusted<Ticket> *)
path        = IDENT ("." IDENT)* ;

block       = "{" stmt* [expr] "}" ;                     (* the trailing expr is the block's value *)
stmt        = "let" IDENT [":" type] "=" expr ";"
            | "return" [expr] ";"
            | "for" IDENT "in" expr_ns block
            | "while" expr_ns block
            | expr "=" expr ";"                          (* target: name, field or index *)
            | block_like [";"]
            | expr ";" ;

expr        = binary ;
unary       = ("-" | "!") unary | postfix ;
postfix     = primary ( "." IDENT | "(" args ")" | "[" expr "]" | "?" )* ;
primary     = INT | FLOAT | STRING | "true" | "false"
            | IDENT
            | path "{" [field_init ("," field_init)* [","]] "}"    (* record literal; not in expr_ns *)
            | "(" expr ")"
            | "[" [expr ("," expr)* [","]] "]"
            | block_like ;
field_init  = IDENT [":" expr] ;                         (* `Ticket { title }` is shorthand *)
block_like  = if | match | block ;
if          = "if" expr_ns block ["else" (if | block)] ;
match       = "match" expr_ns "{" arm* "}" ;
arm         = pattern "=>" expr ","? ;                   (* "," required unless the body is block-like or last *)

pattern     = "_" | literal | "-" (INT | FLOAT)
            | path ["(" [pattern ("," pattern)* [","]] ")"] ;
```

`expr_ns` is an expression where a record literal isn't allowed at the top level,
because the `{` would be ambiguous with the block that follows (`if x { ... }`).
Wrap it in parentheses if needed: `if (Point { x: 1 }) == p { ... }`.

### Operator precedence

From loosest to tightest. Binary operators are left-associative, except
comparisons, which can't be chained (`a < b < c` is error W0019).

| Level | Operators |
|---|---|
| 1 | `\|\|` |
| 2 | `&&` |
| 3 | `== != < <= > >=` (non-associative) |
| 4 | `+ -` |
| 5 | `* / %` |
| 6 | unary `-` `!` |
| 7 | postfix: `.field`, call `f(...)`, index `x[i]`, `?` |

### Statements and block-like expressions

As in Rust, an `if`, `match` or `{ ... }` at the start of a statement ends that
statement at its closing `}`, and needs no `;`. So `match x { ... }.len()` in
statement position is two statements; write `(match x { ... }).len()`.

### AI functions

An `ai fn` is answered by a model. Its body is exactly one prompt string (error
W0016 otherwise), which may interpolate parameters. It must declare a return type
(error W0017), because the model's answer is parsed and validated against it.
`ai fn` implies the `llm` effect; other effects and a `budget` go between the
signature and the body, as for any function.

```wardscript
ai fn triage(email: Untrusted<String>) -> Ticket
    budget {tokens: 2000, calls: 3}
{
    "Fill in a ticket for this email:\n{email}"
}
```

## Canonical formatting

`ws_syntax::printer::print` turns an AST back into canonical source (4-space indent,
trailing commas on multi-line lists; `uses` and `budget` clauses on their own
lines, with the body's `{` on the next line). Printing
is a fixed point: parsing the printed output and printing again yields the same
text. Comments are not preserved yet.
