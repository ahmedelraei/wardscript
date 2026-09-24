# 007: Trust labels

**Status:** accepted, 2026-09-24.

## Decision

- **One label per value**, `Trusted` or `Untrusted`, not per component: a list
  holding one untrusted string is untrusted. Record and enum fields declared
  `Untrusted<T>` are the exception, so a record can carry an untrusted body next to
  a trusted sender.
- **Plain parameters are label-polymorphic.** Each function gets a summary: which
  parameters reach its result, which reach what it throws, and which reach a sink.
  Callers apply the summary to their arguments. Summaries are computed to a fixpoint,
  so recursion and mutual recursion work without annotations.
- **Every tool argument is a sink** until M7 reads tool schemas and label config.
  Every tool result is untrusted.
- **Implicit flows use a pc label**, but only for data: assignments to variables
  from outside an untrusted branch, the value of an `if`/`match`, returns, and what
  a `try` produces. Calling a sink under an untrusted condition with trusted
  arguments is allowed.
- **`let` bindings inside a branch aren't tainted by the branch**, since they can't
  outlive it; only writes to outer variables are.
- **W0107 shows one path**, numbered, from the source to the sink, following
  helpers into other functions and modules.
- **The host vouches** for parameters that reach a sink by passing
  `wardscript.Trusted(x)`; anything else raises `TrustError`. `ward run` vouches for
  its command-line arguments.

## Why

Per-component labels (a `List<Untrusted<String>>` whose spine is trusted) would
be more precise but make every signature and error harder to read. Coarse labels
plus `Untrusted` fields cover the common shape: a trusted envelope around an
untrusted payload.

Summaries keep annotations in signatures only, as PLAN.md asks, without making every
helper's parameters `Trusted` or `Untrusted`. The price is that a helper's
requirements come from its body, so the path in W0107 has to show them; hence steps
inside callees.

Tracking whether a sink runs, not just what it receives, would reject the flagship
support agent: it picks between human review and automatic validation based on
the model's triage, and both paths are safe. CaMeL-style strict control-flow
tracking can be added later as an opt-in.

The host boundary is where static labels end. Treating host input as untrusted by
default matches the language's rule for data from outside, and a wrapper type is
visible in Python code and stubs without changing how other parameters are passed.

## Not yet

- A runtime check at the sink itself (M6 carries labels into the Rust runtime and
  the audit trace).
- Which tool parameters are sensitive, and which tool results are trusted (M7).
- Warnings for a `declassify` or `approve` of data that's already trusted.
