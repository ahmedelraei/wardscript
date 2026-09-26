# Roadmap

Wardscript is in beta (`v0.1.0-beta.1`). The language and its diagnostics may still
change between releases. [PLAN.md](PLAN.md) has the detailed milestones.

## Done

- The checker: types, trust labels, effects, budgets and the Rule of Two.
- Python backend and runtime, with audit traces, OTLP export, runtime sink checks,
  async code and streaming.
- TypeScript backend and runtime for Node.
- Model providers, fallbacks and retry policies.
- Typed MCP tool imports, pinned in `ward.lock`.
- Refinement types and `check` clauses.
- `test` blocks with recorded model answers.
- Classes, interfaces and abstract classes.
- Language server, VS Code extension and `ward fmt`.
- AgentDojo port of all four suites.

## In progress

- **AgentDojo with real models.** The port is scored against scripted answers; a
  run with real models (`benchmarks/agentdojo/run.py --model`) is next.
- **WASM backend** (`wasm-encoder`) for sandboxed execution.
- **Number parsing** (`String.parse_int`, `String.parse_float`), WEP 020 draft.

## Where help is welcome

- **Diagnostics:** clearer messages and `help` text. No WEP needed.
- **Attack cases:** new `tests/attacks/` programs that should fail to compile.
  If one compiles, that's a security bug: see [SECURITY.md](SECURITY.md).
- **Examples and docs:** real-world agents written in Wardscript.
- **Editor support:** grammars and `ward lsp` setup for editors other than VS Code.
- **Language server:** more features on top of diagnostics, hover and go to
  definition.

Language changes need a WEP first. See [CONTRIBUTING.md](CONTRIBUTING.md).
