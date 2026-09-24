# 004: Line breaks end statements; `return x` wraps in `Ok`

**Status:** accepted, 2026-09-24.

## Decisions

1. **No required semicolons.** A line break ends a statement; `;` remains legal as a
   separator on one line. Line breaks are ignored inside `( )`, `[ ]` and record
   literals, after a trailing binary operator, and before a leading `.`. This is
   the Kotlin/Swift rule: simple to state, and the common multi-line forms (long
   calls, operator chains, method chains) work without markers.
2. **An unused block value may have any type.** Without semicolons every block's
   last line looks like its value; a function returning nothing must not be
   rejected because its last call returns something.
3. **`return x` in a `Result<T, E>` function means `return Ok(x)`** when `x` is a
   `T`. Errors stay explicit (`return Err(e)`, `?`). A final expression is not
   wrapped, so the only implicit `Ok` is on the keyword that says "leave with
   this value".

Kept deliberately: braces, `let`, `->`, `pub`, and generic types (`List<T>`,
`Untrusted<T>`).

## Consequences

- The lexer records whether a line break precedes each token; the parser decides
  where that matters.
- W0015 now means "statement doesn't end" (two statements on one line), the same
  role as "missing `;`" before.
- The checker records auto-wrapped returns (`ModuleTypes::auto_ok`) for code
  generation.
