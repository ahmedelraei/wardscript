# UI tests

Each `*.wardscript` file here is run through `ward check --format json` and the
exit code, stdout and stderr are snapshotted into `snapshots/` with `insta`.

- Harness: `crates/ward_cli/tests/ui.rs`
- Accept changes deliberately with `cargo insta review`.
