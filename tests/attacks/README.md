# Attack tests

Each `*.wardscript` file here is a prompt-injection or data-exfiltration attempt
and **must be rejected** by `ward check` (exit code 1). An internal error
(exit code 2) counts as a failure: the checker has to actually reject it.

Put the fixed, accepted version of an attack in `tests/ui/` so false positives
are caught too.

- Harness: `crates/ws_cli/tests/attacks.rs`
