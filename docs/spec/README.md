# Wardscript language specification

This is the normative description of Wardscript. It is kept in sync with the
implementation: every language feature lands together with its section here.

Status: **M4 (trust labels)**.

## Contents

| Section | Status |
|---|---|
| [Lexical structure and syntax](syntax.md) | implemented (M1) |
| [Diagnostics](diagnostics.md) | syntax (M1), names and types (M2), trust (M4) |
| [Names, modules and imports](names.md) | implemented (M2) |
| [Types](types.md) | implemented (M2) |
| [Trust labels](trust.md) (`Trusted` / `Untrusted`, sources, sinks, `validate` / `approve` / `declassify`) | implemented (M4) |
| Effects, budgets, Rule of Two | planned (M5) |
| [Python backend and runtime](runtime.md) | implemented (M3); audit trace planned (M6) |
| MCP imports | planned (M7) |

## Diagnostics

Every diagnostic has a stable code `W0xxx`. A code's meaning never changes once
it is assigned; retired codes are not reused.

| Range | Area |
|---|---|
| W00xx | syntax ([list](diagnostics.md)) |
| W010x | names and modules, and W0107: untrusted data reaches a sensitive action |
| W011x-W012x | types |
| W02xx | effects, budgets, Rule of Two |

## `ward` exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | the program has errors (diagnostics were reported) |
| 2 | usage error or internal failure |
| 3 | `ward run`: the program threw, or failed at runtime |
