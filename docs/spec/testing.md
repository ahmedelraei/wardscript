# Tests

Status: implemented in M10 (`test` blocks, `ward test`, `wardscript.testing`).
Design notes are in [decision 015](../decisions/015-tests.md).

```ward
test "a double charge goes to billing" {
    let ticket = triage("I was charged twice for my subscription this month.")
    assert queue(ticket) == "billing" => "a double charge is a billing question"
    assert ticket.order_id.is_none()
}
```

## `test` blocks

- `test "name" { ... }` is an item. Its body is checked like a function's that
  returns nothing and may throw anything: `?` works on any throwing call, and a
  thrown error fails the test. Names are unique in a module (W0103).
- `assert cond` and `assert cond => "message"` are statements, only in tests. The
  condition is a `Bool` (W0110). A failed assertion fails the test with the
  message, or the condition's text, and where it is.
- Tests are checked for trust like other code (W0107): a model's answer can't
  reach a sink from a test either. They don't declare effects.
- `test` is a keyword only before a string at the top level, and `assert` only at
  the start of a statement in a test; elsewhere both are ordinary names.
- `ward build` leaves tests out of the generated code.

## `ward test`

```bash
ward test examples/triage.ward            # replay: offline, deterministic
ward test examples/triage.ward billing    # only tests whose names contain "billing"
ward test examples/triage.ward --record --model anthropic
ward test examples/triage.ward --record --mock answers.json
```

`ward test FILE [FILTER...]` runs the file's tests, each as its own run (with an
audit trace in `--trace-dir`, if given), and prints a line per test and a summary.
It exits with 0 when all pass and 4 when one fails.

By default it **replays** `<file>.recordings.json` (next to the program, or
`--recordings PATH`): every model answer and tool result the test got when it was
recorded. Nothing is sent to a model or a tool. A replay is strict: a test fails
if it asks a different `ai fn`, model alias or prompt, calls a different tool or
with different arguments, or makes more or fewer calls than were recorded. Record
it again after changing it. A test without a recording fails.

`--record` runs the tests against a model (`--model`, as for `ward run`, with
`alias=provider:model` for aliases; or `--mock`) and the tools of the nearest
`mcp.json`, and writes what they got. Tests not selected by the filters keep their
recordings. Provider errors are recorded too, so retries and fallbacks replay.

Budgets apply as in a normal run: the recorded tokens and cost are charged again.
`approve` is always granted in tests.

## Recordings

```json
{
  "version": 1,
  "tests": {
    "a double charge goes to billing": [
      {"kind": "model", "function": "triage", "model": null, "prompt": "...",
       "text": "{...}", "tokens": 812, "cost": 0.0031},
      {"kind": "tool", "tool": "gmail", "function": "send_email",
       "arguments": {"to": "...", "subject": "...", "body": "..."}, "result": "sent"}
    ]
  }
}
```

A model event has the answer's `text`, `tokens` and `cost`, or an `error`
(`type`, `message`, `status`). A tool event has named `arguments` (tools with a
schema) or positional `args`, and a `result`, a `thrown` value or an `error`. The
file is meant to be committed, and can be edited by hand.
