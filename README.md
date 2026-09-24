# Wardscript

A small, typed language for trustworthy AI functions and agents. The compiler
proves that untrusted data (LLM output, tool results, network input) can't reach
sensitive actions without an explicit `validate`, `approve` or `declassify` step.

```wardscript
import mcp "gmail" as mail

type Reply {
    subject: String,
    body: String,
}

// The model's output is untrusted: it was shaped by the email.
fn draft_reply(email: Untrusted<String>) -> Reply
    uses {llm}
    by llm "Write a short, polite reply to this email:\n{email}"

fn no_links(text: String) -> Bool {
    !text.contains("http://") && !text.contains("https://")
}

pub fn answer(email: Untrusted<String>, to: String) -> Result<String, String>
    uses {llm, mail.send}
{
    let reply = draft_reply(email);
    // `mail.send` is a sensitive action: anything derived from the email must be validated first.
    let subject = validate(reply.subject, no_links)?;
    let body = validate(reply.body, no_links)?;
    mail.send(to, subject, body);
    Ok("sent")
}
```

Remove either `validate` and the program is rejected at compile time: the
model's text would reach `mail.send` unchecked. (That trust check is milestone M4;
today `ward check` verifies syntax, names and types.)

## Status

Early development. Done: the parser, name resolution and type checker
(`ward check`). Next: the Python backend and runtime (M3), then trust labels (M4).
See [PLAN.md](PLAN.md) for the milestones and [docs/spec](docs/spec/README.md)
for the language specification.

```bash
cargo run -p ws_cli -- check examples/support.wardscript
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
