# Using AI tools

You can use AI tools to contribute to Wardscript. These rules apply to every
issue, pull request, WEP and security report, whether AI was involved or not.

## You are responsible

You own everything you submit. You must be able to explain every change in your
own words and answer review questions yourself, without relaying them to a model.
Review AI output in full before submitting it, including titles and descriptions.

## Say when you used it

Tick the box in the PR template and say briefly what the tool did, for example
"wrote the first draft of the parser tests". A `Co-Authored-By` trailer from an
agent counts.

## Good uses

- Understanding the codebase.
- Drafting code, tests or docs that you then review and understand.
- Writing or polishing English, especially if it isn't your first language.
- Finding bugs and reviewing your own diff before you submit.

## Hard rules

A PR that breaks one of these is closed.

- **Don't weaken tests to make them pass.** Don't edit, skip or delete a test to
  turn it green. That includes accepting `insta` snapshots you haven't read: every
  changed `.snap` file must be explained in the PR.
- **Don't touch attack cases to make them compile.** Programs in `tests/attacks/`
  must fail to compile. Never edit or remove one to make it pass. New trust
  behavior needs a new attack case.
- **Don't change what a diagnostic code means.** Add a new code instead.
- **Don't submit numbers you didn't produce.** Benchmark, performance and
  AgentDojo results must come from actually running the code. Say how you ran it.
- **Don't include secrets.** No API keys, tokens, or unredacted recorded model
  responses.

## WEPs

AI can help with the wording of a WEP. The motivation, the alternatives that were
turned down and the Trust and security argument must be your own reasoning.

## Issues

Reproduce the problem before filing. Include a `.ward` program, the command you ran
and its actual output. Don't file generated reports you haven't checked.

## Security reports

Verify before you report. A report must include a `.ward` program that compiles
but shouldn't, or that lets untrusted data reach a sink at runtime, plus the
`ward` version and the exact command. Check that every API, flag and diagnostic
code you mention actually exists. Reports that only describe a bypass in theory
are closed. See [SECURITY.md](../SECURITY.md).

## Maintainer discretion

Unproductive issues and PRs may be closed without explanation, whether AI was used
or not. Repeatedly opening them may get you blocked from the project.
