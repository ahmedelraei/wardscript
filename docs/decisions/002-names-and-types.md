# 002: Names and base types (M2)

**Status:** accepted, 2026-09-24

## Decisions

1. **One namespace for types and values at module level.** A record and a function
   can't share a name. It keeps `Priority.High` and `t.Ticket` unambiguous and the
   resolver simple; separate namespaces can be added later without breaking programs.
2. **Enum variants are qualified (`Priority.High`)**, except the prelude's `Some`,
   `None`, `Ok`, `Err`. A lone name in a pattern is therefore always a binding
   (apart from `None`), which removes Rust's "typo becomes a catch-all" trap.
3. **Bidirectional checking over unification.** Signatures are fully annotated, so
   inference stays local to one function; `check` pushes expected types down so
   errors land on the innermost wrong expression.
4. **`validate(x, rule)` takes a named rule function `fn(T) -> Bool`.** Rules are
   ordinary, testable code, and the audit trace (M6) can record the rule's name.
   Built-in rule libraries can come later as prelude functions.
5. **`Untrusted<T>` is transparent to the type checker.** Labels are a separate
   lattice checked in M4; mixing them into types would make every function generic
   over trust.
6. **MCP tool calls are dynamic until M7.** The alternative, rejecting them, would
   make the flagship examples fail to check for five milestones.
7. **`ward check` stops after syntax errors.** Resolution errors in unparsed code
   are almost always noise.
8. **Record literals accept a module path** (`t.Ticket { ... }`), found by lookahead
   for `Ident (. Ident)* {` outside `if`/`while`/`match`/`for` heads.

## Consequences

- Lists and maps are immutable values (`push` returns a new list). This keeps label
  tracking in M4 simple: there's no aliasing to follow.
- `Int` and `Float` never mix implicitly; `to_float()` and `round()` convert.
