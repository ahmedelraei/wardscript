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

Early development. Done: the parser, name resolution and type checker
(`ward check`), the Python backend with its runtime (`ward build`, `ward run`), and
trust labels, effects, budgets and the Rule of Two, and the runtime's Rust core,
audit trace, model providers, runtime sink checks, OTLP export, async code and
streaming (`ward trace`, `ward run --model`, `ward build --async`), and typed MCP
tool imports pinned in `ward.lock` (`ward lock`; see [examples/inbox](examples/inbox)).
Next: model fallbacks and retry policies (M8). See [PLAN.md](PLAN.md) for the milestones and
[docs/spec](docs/spec/README.md) for the language specification.

```bash
cargo run -p ward_cli -- check examples/support.wardscript
cargo run -p ward_cli -- build examples/triage.wardscript -o build
cargo run -p ward_cli -- run examples/triage.wardscript route '"My order never came"' --mock answers.json
cargo run -p ward_cli -- lock examples/inbox/mcp.json       # pin the MCP servers' tool schemas
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
