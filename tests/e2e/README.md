# End-to-end tests

Each `test_*.py` here is a Python `unittest` driver whose first line names the
program it tests: `# ward: examples/triage.wardscript`. The harness
(`crates/ward_cli/tests/e2e.rs`) builds that program with `ward build`, checks the
`.pyi` stubs parse, and runs the driver with the generated modules and the
`wardscript` runtime (`crates/ward_runtime/py`) on `PYTHONPATH`. Drivers use the
deterministic `MockModel`, so no real model is involved.

The generated Python for these programs and for `examples/` is snapshotted by
`crates/ward_cli/tests/codegen.rs`.

Tests need Python 3.10+ (`python3`, or `WARD_PYTHON`). Tests against real LLMs are
opt-in with `WARD_LIVE=1`.
