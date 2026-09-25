# Getting started

Wardscript is a typed language for AI functions and agents. Its checker proves that
untrusted data (model output, tool results, network input) can't reach a sensitive
action without an explicit `validate`, `approve` or `declassify`. Programs compile to
Python or TypeScript.

## Install

Wardscript is in **beta**: the language and its diagnostics may still change between
releases.

On macOS or Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/ahmedelraei/wardscript/main/install/install.sh | sh
```

On Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/ahmedelraei/wardscript/main/install/install.ps1 | iex
```

The installer downloads the newest release for your platform (Linux x86_64 and arm64,
macOS Apple Silicon and Intel, Windows x86_64), checks it against its SHA-256, puts
`ward` in `~/.ward/bin` (`%USERPROFILE%\.ward\bin` on Windows) and adds that to your
PATH. `WARD_VERSION=v0.1.0-beta.1` picks a release, `WARD_INSTALL_DIR` another folder,
and `WARD_NO_MODIFY_PATH=1` leaves your PATH alone. You can also download an archive
from the [releases](https://github.com/ahmedelraei/wardscript/releases) and check it
against `SHA256SUMS`. To uninstall, delete `~/.ward` and the PATH line the installer
added (it's marked "Added by the Wardscript installer").

`ward run` and `ward test` bring their own copy of the Python runtime. To import
`ward build` output from your own application, install the `wardscript` runtime from
source for now:

```bash
pip install maturin && (cd crates/ward_runtime/py && maturin build --release -o dist)
pip install crates/ward_runtime/py/dist/*.whl
```

From source, `cargo install --path crates/ward_cli` builds `ward` itself. For real
models, install a provider's SDK (`pip install anthropic` or `openai`) and set its API
key (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`).

## A first program

```bash
ward init hello && cd hello
ward check main.ward
ward test main.ward
ward run main.ward reply '"Where is my order?"' --model anthropic
```

`main.ward` has an `ai fn` whose answer is a typed `Reply`, refined (`where`) and
checked (`check {...}`) before your code sees it, and a `test` that replays a
recorded answer. [The spec](spec/README.md) describes the language.

## The pieces, in order

1. **Types and AI functions.** An `ai fn`'s body is its prompt; its return type is
   the schema the answer must match, with retries when it doesn't
   ([types](spec/types.md), [syntax](spec/syntax.md)).
2. **Trust.** Parameters and results are `Untrusted` or trusted; tool arguments are
   sinks. `ward check` reports W0107 with the path untrusted data took
   ([trust](spec/trust.md)).
3. **Effects and budgets.** `uses {llm, mail.send}` and `budget {calls: 3, cost:
   0.10}`; the Rule of Two ([effects](spec/effects.md)).
4. **Tools.** `import mcp "gmail" as mail`, with schemas pinned by `ward lock`
   from your `mcp.json` ([tools](spec/tools.md); see `examples/inbox`).
5. **Models.** `model {primary: fast, fallback: smart, retries: 2}` and
   `ward run --model fast=anthropic:<model>` ([runtime](spec/runtime.md#model-policies)).
6. **Tests.** `test "..." { assert ... }`, `ward test`, `ward test --record`
   ([tests](spec/testing.md)).
7. **Running it.** `ward build` writes a Python module (and `.pyi` stubs) to import
   from your application; `wardscript.runtime.configure(...)` sets the model,
   approver and tools; every run leaves an audit trace (`ward trace show`)
   ([runtime](spec/runtime.md)).

## Editors

`ward lsp` is a language server (diagnostics, hover, go to definition, formatting);
[editors/vscode](../editors/vscode) is a VS Code extension that uses it.

## Commands

| Command | Does |
|---|---|
| `ward init [DIR]` | start a project |
| `ward check FILE` | check a program (`--format json` for tools) |
| `ward build FILE -o DIR` | compile to Python (`--async` for asyncio hosts), or `--target typescript` ([TypeScript](spec/typescript.md)) |
| `ward run FILE FN ARGS...` | build and call a function (`--mock`, `--model`) |
| `ward test FILE [FILTER...]` | run tests from recordings (`--record` to record) |
| `ward fmt FILES...` | format in place, keeping comments (`--check` in CI) |
| `ward lock [mcp.json]` | pin MCP tool schemas in `ward.lock` (`--check` in CI) |
| `ward trace show [RUN]` | show a run's audit trace; `ward trace export` for OTLP |
| `ward lsp` | the language server |
