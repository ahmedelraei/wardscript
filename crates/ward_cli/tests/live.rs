//! Runs `tests/live/test_live.py` against a real model, only when `WARD_LIVE=1`: it costs
//! money and needs an API key, so CI skips it.

#![allow(clippy::expect_used, clippy::panic)]

mod common;

#[test]
fn live_model() {
    if std::env::var_os("WARD_LIVE").is_none_or(|v| v != "1") {
        eprintln!("skipped: set WARD_LIVE=1 (and an API key) to run against a real model");
        return;
    }
    let triage = common::build("live_triage", "examples/triage.ward");
    let support = common::build("live_support", "examples/support.ward");
    let mut paths = vec![triage, support.clone()];
    if std::env::var_os("WARD_RUNTIME_INSTALLED").is_none() {
        paths.push(common::runtime_py());
    }
    // Keep the caller's entries, e.g. where the provider SDK is installed.
    if let Some(existing) = std::env::var_os("PYTHONPATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    let run = common::python()
        .arg(common::repo_root().join("tests/live/test_live.py"))
        .arg("-v")
        .env(
            "PYTHONPATH",
            std::env::join_paths(paths).expect("PYTHONPATH"),
        )
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .current_dir(&support)
        .output()
        .expect("run python");
    assert!(run.status.success(), "{}", common::render(&run));
}
