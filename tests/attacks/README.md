# Attack tests

Each `*.ward` file here, and each `<case>/main.ward` for attacks that span modules,
is a prompt-injection or data-exfiltration attempt and **must be rejected** by
`ward check` with W0107 (untrusted data reaches a sensitive action), and with no
other diagnostic. An internal error (exit code 2), or a rejection for some other
reason, counts as a failure: the trust checker has to catch it.

The fixed, accepted versions are in `tests/ui/ok_trust_fixed.ward`, so false
positives are caught too.

- Harness: `crates/ward_cli/tests/attacks.rs`
