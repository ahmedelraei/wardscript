# Wardscript language specification

This is the normative description of Wardscript. It is kept in sync with the
implementation: every language feature lands together with its section here.

Status: **M0 (project skeleton)**. No language features are implemented yet.

## Contents

| Section | Status |
|---|---|
| Lexical structure | planned (M1) |
| Syntax | planned (M1) |
| Names, modules and imports | planned (M2) |
| Types | planned (M2) |
| Trust labels (`Trusted` / `Untrusted`, sources, sinks, `validate` / `approve` / `declassify`) | planned (M4) |
| Effects, budgets, Rule of Two | planned (M5) |
| Runtime and audit trace | planned (M3, M6) |
| MCP imports | planned (M7) |

## Diagnostics

Every diagnostic has a stable code `W0xxx`. A code's meaning never changes once
it is assigned; retired codes are not reused.

| Range | Area |
|---|---|
| W00xx | syntax |
| W01xx | names, types and trust labels (W0107: untrusted data reaches a sensitive action) |
| W02xx | effects, budgets, Rule of Two |

## `ward` exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | the program has errors (diagnostics were reported) |
| 2 | usage error or internal failure (including "not implemented yet") |
