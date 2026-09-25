# WEP 000: Wardscript Enhancement Proposals

**Status:** Active. Accepted 2026-09-25.

A WEP proposes a change to the language and records why it was made, including the
options that were turned down. [`docs/spec/`](../spec/README.md) says what the
language is; WEPs say why. WEPs 001–019 were written as design decisions after the
fact and were brought into this process unchanged.

## When a WEP is needed

Write one before changing:

- syntax, or the meaning of existing syntax;
- the type system, trust labels, effects or budgets;
- what a backend generates, or what the runtime does at a sink;
- the meaning of a diagnostic code;
- this process.

No WEP is needed for bug fixes, new diagnostics, performance work, tooling that
doesn't change what programs mean, or docs. When in doubt, open an issue first.

## Lifecycle

```
Draft ──> Accepted ──> Final
  │          │
  │          └──> Deferred
  ├──> Rejected
  └──> Withdrawn
Final ──> Superseded (by a later WEP)
```

- **Draft**: opened as a PR adding `docs/weps/NNN-short-name.md` from the
  [template](template.md). Take the next free number.
- **Accepted**: the design is agreed. The PR merges and implementation can start.
- **Final**: implemented, with its tests, and `docs/spec/` updated. Only a Final
  WEP describes the language as shipped.
- **Rejected / Withdrawn / Deferred**: the file stays, with the reason in its
  status line, so the same proposal isn't argued again from scratch.
- **Superseded**: a later WEP replaced it. Both link to each other.

A Final WEP isn't edited except to fix typos or add a "Superseded by" note; a change
of mind is a new WEP.

## Who decides

The maintainer (@ahmedelraei) accepts or rejects WEPs. The PR is the place for
discussion; the decision and its reason go in the status line.

## What a WEP must argue

Wardscript's promise is that untrusted data can't reach a sensitive action without
`validate`, `approve` or `declassify`. Every Standards WEP has a **Trust and
security** section that shows the change keeps that promise: which new paths data
can take, and why the checker still sees them. A change that weakens it needs a
`tests/attacks/` case showing where the new limit is.
