# WEP 006: How Wardscript values map onto Python

**Status:** Final. Accepted 2026-09-24.

## Decision

- Generated modules are **plain, readable Python** that hosts import directly:
  records are frozen dataclasses, enums without fields are `enum.Enum`s, `List` is
  `list`, `Map` is `dict`, `Option<T>` is `T | None`.
- Enums with fields become a base class with one frozen dataclass per variant
  (`Verdict.Fail("x")`, fields `_0`, `_1`, ...), so `isinstance` does the matching.
- **Values are immutable.** `x.a = v` and `xs[i] = v` rebind the variable to an
  updated copy (`dataclasses.replace`, a new list or dict); nothing is mutated, so a
  caller's argument never changes under it.
- **`Some(x)` is `x`**, except when `x` is itself `None` or a `Some` (only possible
  with nested options, or through generics). Then it's wrapped in `wardscript.Some`.
  Every option value has exactly one representation, so `==` works.
- `throw e` raises `wardscript.Thrown(e)`; `try`/`catch` catches only `Thrown`.
  Runtime failures (bad model output, denied approval) are `WardError`s, which no
  Wardscript `try` can catch.
- `ai fn` calls go through `wardscript.runtime`: the JSON schema comes from a type
  descriptor the compiler emits, and the model's answer is decoded against it.
- `match` compiles to `if`/`elif` chains, not Python's `match` statement.

## Why

The output is meant to be read and debugged by Python programmers, and called from
ordinary Python code without wrappers; hence built-in types and dataclasses rather
than runtime value classes. Making `Option` plain `None` keeps host code natural;
the `Some` wrapper covers the one case where that would lose information, instead
of forbidding nested options.

Rebinding on assignment gives value semantics without deep copies: only the path
being assigned is copied.

Separating `Thrown` from `WardError` preserves the checker's guarantee: a `try`
handles exactly the errors the type system says it can, and a failure the program
didn't declare can't be swallowed by one.

`if`/`elif` instead of `match` because the option encoding and literal patterns
don't map onto class patterns cleanly, and the conditions read just as well.

## Consequences

- Generated code needs Python 3.10+ (`X | None` in annotations is only ever a
  string there, via `from __future__ import annotations`).
- Tool results (M7 types them) are passed through as the tool returned them.
- Integer arithmetic isn't bounded to 64 bits in Python; `/` and `%` on `Int` go
  through runtime helpers that truncate toward zero, as the spec requires.
