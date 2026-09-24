# Wardscript language specification

This is the normative description of Wardscript. It is kept in sync with the
implementation: every language feature lands together with its section here.

Status: **M10 (tests)**.

## Contents

| Section | Status |
|---|---|
| [Lexical structure and syntax](syntax.md) | implemented (M1) |
| [Diagnostics](diagnostics.md) | syntax (M1), names and types (M2), trust (M4), effects (M5), tools (M7) |
| [Names, modules and imports](names.md) | implemented (M2) |
| [Types](types.md) | implemented (M2), refinements and checks (M9) |
| [Trust labels](trust.md) (`Trusted` / `Untrusted`, sources, sinks, `validate` / `approve` / `declassify`) | implemented (M4) |
| [Effects, budgets, Rule of Two](effects.md) | implemented (M5) |
| [Python backend and runtime](runtime.md) | implemented (M3), budgets (M5), audit trace, providers, sink checks, async and streaming (M6), model policies (M8) |
| [Tools and MCP imports](tools.md) (`ward.lock`, typed tool calls, sink parameters) | implemented (M7) |
| [Tests](testing.md) (`test` blocks, `assert`, `ward test`, recordings) | implemented (M10) |

## Diagnostics

Every diagnostic has a stable code `W0xxx`. A code's meaning never changes once
it is assigned; retired codes are not reused.

| Range | Area |
|---|---|
| W00xx | syntax ([list](diagnostics.md)) |
| W010x | names and modules, and W0107: untrusted data reaches a sensitive action |
| W011x-W012x | types |
| W02xx | effects, budgets, Rule of Two |
| W03xx | tools |

## `ward` exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | the program has errors (diagnostics were reported) |
| 2 | usage error or internal failure |
| 3 | `ward run`: the program threw, or failed at runtime |
| 4 | `ward test`: a test failed |
