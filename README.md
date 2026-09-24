# Wardscript

A small, typed language for trustworthy AI functions and agents. The compiler
proves that untrusted data (LLM output, tool results, network input) can't reach
sensitive actions without an explicit `validate`, `approve` or `declassify` step.

```wardscript
pub fn handle(email: Untrusted<String>, to: String) -> Outcome
    uses {llm, mail.send}
{
    let reply = draft_reply(triage(email));
    let body = validate(reply.body, no_links)?;   // required before `mail.send`
    mail.send(to, reply.subject, body);
    Ok("sent")
}
```

## Status

Early development. The parser, name resolution and type checker work
(`ward check`); trust labels, the Python backend and the runtime are in progress.
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
