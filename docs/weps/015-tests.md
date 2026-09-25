# WEP 015: Tests with recorded responses

**Status:** Final. Accepted 2026-09-24.

## Decision

- **Tests live in the program's files** as `test "name" { ... }` items, with
  `assert cond => "message"` statements. They are type- and trust-checked like
  any code, so a test can't exercise a flow the checker would reject elsewhere.
  `test` and `assert` are contextual keywords, so existing names keep working.
- **Replay by default, record on request.** `ward test` replays each test's
  recorded model answers *and tool results* from `<file>.recordings.json`; CI
  never calls a model or a tool. `ward test --record` runs against a real model
  (or `--mock`) and the tools in `mcp.json`, and rewrites the recordings of the
  tests it ran.
- **Replays are strict.** The recording is matched call by call: function, model
  alias, prompt, tool arguments, and the number of calls. A change in the program
  that changes what it asks fails the test until it is recorded again, instead of
  silently answering a different question with an old answer.
- **Everything runs as usual.** Budgets are charged with the recorded usage, model
  errors replay so retries and fallbacks are tested, and every test is a run in the
  audit trace. Approvals are granted, since a recording has no human.
- **Exit code 4** means a test failed, distinct from errors in the program (1),
  internal errors (2) and runtime failures of `ward run` (3).

## Why

LLM tests are either flaky (live calls) or fake (hand-written mocks that drift from
what models say). Recording real answers once and replaying them exactly gives
deterministic CI with realistic data, and strict matching tells you when a prompt
change needs new recordings.

## Not done

- Denying approvals in a test.
- Recording streamed answers chunk by chunk (answers are recorded whole).
- Tests in imported modules: `ward test FILE` runs FILE's tests.
