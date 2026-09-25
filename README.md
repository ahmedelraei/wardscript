# Wardscript

A small, typed language for trustworthy AI functions and agents. The compiler
proves that untrusted data (LLM output, tool results, network input) can't reach
sensitive actions without an explicit `validate`, `approve` or `declassify` step.

<!-- wardscript: checked by `readme_examples_check_ok`; tagged rust only for highlighting -->
```rust
import mcp "gmail" as mail

type Reply {
    subject: String,
    body: String,
}

// The model's output is untrusted: it was shaped by the email.
ai fn draft_reply(email: Untrusted<String>) -> Reply {
    "Write a short, polite reply to this email:\n{email}"
}

fn no_links(text: String) -> Bool {
    !text.contains("http://") && !text.contains("https://")
}

pub fn answer(email: Untrusted<String>, to: String) -> String throws String
    uses {llm, mail.send}
{
    let reply = draft_reply(email)
    // `mail.send` is a sensitive action: anything derived from the email must be validated first.
    let subject = validate(reply.subject, no_links)?
    let body = validate(reply.body, no_links)?
    mail.send(to, subject, body)
    return "sent"
}
```

Remove either `validate` and the program is rejected at compile time: the
model's text would reach `mail.send` unchecked, and `ward check` reports W0107 with
the path it took. The host calling `answer` has to vouch for `to`
(`answer(email, Trusted(to))`), since it also reaches `mail.send`.

## Status

Early development, but usable end to end. Done:

- the checker (`ward check`): types, trust labels, effects, budgets and the Rule of Two;
- the Python backend and runtime (`ward build`, `ward run`, `ward build --async`),
  with a Rust core, an audit trace (`ward trace`), OTLP export, runtime sink checks
  and streaming;
- a TypeScript backend and runtime for Node (`ward build --target typescript`);
- model providers (`ward run --model`), with fallbacks and retry policies
  (`model {primary: fast, fallback: smart}`);
- typed MCP tool imports pinned in `ward.lock` (`ward lock`; see
  [examples/inbox](examples/inbox));
- refinement types (`String where it.len() <= 80`) and `check {...}` clauses on
  model answers, retried with the reason when they fail;
- `test` blocks, run by `ward test` from recorded model answers and tool results
  (`ward test --record` records them).

Next: proof and polish (M11): typed streaming, the TypeScript backend, an LSP. See [PLAN.md](PLAN.md) for the milestones
and [docs/spec](docs/spec/README.md) for the language specification.

[docs/guide.md](docs/guide.md) gets you started: install, `ward init`, and a tour.
[docs/demo.md](docs/demo.md) shows a vulnerable agent that doesn't compile.

```bash
cargo run -p ward_cli -- init hello                         # a first project
cargo run -p ward_cli -- check examples/support.wardscript
cargo run -p ward_cli -- build examples/triage.wardscript -o build
cargo run -p ward_cli -- run examples/triage.wardscript route '"My order never came"' --mock answers.json
cargo run -p ward_cli -- lock examples/inbox/mcp.json       # pin the MCP servers' tool schemas
cargo run -p ward_cli -- test examples/triage.wardscript      # replay the recorded tests
```

## Editor support

`ward lsp` is a language server: diagnostics as you type, types on hover, and go to
definition. [editors/vscode](editors/vscode) is a VS Code extension with
highlighting that starts it; other editors can run `ward lsp` over stdio.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
