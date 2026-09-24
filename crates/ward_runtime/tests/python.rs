//! Runs the `wardscript` Python package's unit tests: against the source in `py/`, or,
//! with `WARD_RUNTIME_INSTALLED=1`, against the package installed from a built wheel.

use std::process::Command;

fn python() -> Command {
    Command::new(std::env::var_os("WARD_PYTHON").unwrap_or_else(|| "python3".into()))
}

fn installed() -> bool {
    std::env::var_os("WARD_RUNTIME_INSTALLED").is_some()
}

#[test]
#[allow(clippy::expect_used)]
fn python_unit_tests() {
    let py = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("py");
    let mut cmd = python();
    // Run from `tests/`, so `import wardscript` finds the source only through PYTHONPATH.
    cmd.args(["-m", "unittest", "discover", "-s", "."])
        .current_dir(py.join("tests"))
        .env("PYTHONDONTWRITEBYTECODE", "1");
    if !installed() {
        cmd.env("PYTHONPATH", &py);
    }
    let out = cmd
        .output()
        .expect("run python (set WARD_PYTHON to a Python 3.10+ interpreter)");
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A wheel has the Rust core; the source tree only the Python fallback.
#[test]
#[allow(clippy::expect_used)]
fn core_implementation() {
    let py = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("py");
    let mut cmd = python();
    cmd.args(["-c", "import wardscript.core as c; print(c.IMPLEMENTATION)"])
        .current_dir(std::env::temp_dir())
        .env("PYTHONDONTWRITEBYTECODE", "1");
    if !installed() {
        cmd.env("PYTHONPATH", &py);
    }
    let out = cmd.output().expect("run python");
    let expected = if installed() { "rust" } else { "python" };
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), expected);
}
