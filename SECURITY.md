# Security policy

Wardscript's promise is that untrusted data can't reach a sensitive action (a
`sink`) without `validate`, `approve` or `declassify`. A way around that is a
security bug.

## Reporting

**Don't open a public issue.** Report privately through
[GitHub's private vulnerability reporting](https://github.com/ahmedelraei/wardscript/security/advisories/new).

## What counts

- A program that compiles although untrusted data reaches a `sink` argument
  without `validate`, `approve` or `declassify`.
- An implicit-flow leak: branching on untrusted data that the checker misses.
- A runtime sink check that doesn't fire.
- An effect, budget or Rule of Two violation that `ward check` accepts.
- Generated Python or TypeScript that drops a check the source program has.
- A bug in the runtime that alters or loses audit trace records.

Not in scope: bugs in your own Wardscript program, a model misbehaving inside code
the checker correctly accepted, and `declassify` or `@allow` doing what they say.

## What to include

- A few plain-text sentences describing the problem. No long reports.
- A minimal `.ward` program that shows it, and the exact command: `ward check`,
  or `ward run` with the mock model (not `WARD_LIVE=1`).
- The `ward --version` output and your OS.
- Optionally, a patch.

Don't include a severity or CVSS score; the maintainer assigns it. Send text only:
no PDFs, binaries or notebooks.

## AI-assisted reports

Allowed, but you must have run the proof of concept yourself and checked that every
API, flag and diagnostic code in the report exists. Reports without a working
proof of concept, and batches of generated reports, are closed. See the
[AI policy](docs/ai-policy.md).

## After a fix

The proof of concept becomes a `tests/attacks/` case so the bypass can't come
back. Reporters are credited in the advisory unless they ask not to be.

## Supported versions

Wardscript is in beta. Only the latest release gets security fixes.
