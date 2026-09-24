//! Runs the `wardscript` Python package's unit tests.

#[test]
#[allow(clippy::expect_used)]
fn python_unit_tests() {
    let py = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("py");
    let python = std::env::var_os("WARD_PYTHON").unwrap_or_else(|| "python3".into());
    let out = std::process::Command::new(python)
        .args(["-m", "unittest", "discover", "-s", "tests"])
        .current_dir(&py)
        .env("PYTHONPATH", &py)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("run python (set WARD_PYTHON to a Python 3.10+ interpreter)");
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
