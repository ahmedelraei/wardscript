//! Every `tests/attacks/*.ward` (and `tests/attacks/<case>/main.ward`) is an injection
//! attempt and MUST be rejected by `ward check` with W0107, and with nothing else.

#![allow(clippy::expect_used)]

mod common;

use std::path::PathBuf;

fn cases() -> Vec<PathBuf> {
    let dir = common::repo_root().join("tests/attacks");
    let mut cases = common::wardscript_files("tests/attacks");
    for entry in std::fs::read_dir(&dir).expect("read tests/attacks") {
        let main = entry.expect("dir entry").path().join("main.ward");
        if main.is_file() {
            cases.push(main);
        }
    }
    cases.sort();
    cases
}

#[test]
fn attacks_are_rejected() {
    let cases = cases();
    assert!(cases.len() >= 30, "only {} attack cases", cases.len());
    let accepted: Vec<_> = cases
        .into_iter()
        .filter_map(|file| {
            let path = file.to_str().expect("utf-8 path");
            let out = common::ward(&["check", "--format", "json", path]);
            let json: serde_json::Value =
                serde_json::from_slice(&out.stdout).unwrap_or(serde_json::Value::Null);
            let codes: Vec<&str> = json["diagnostics"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|d| d["code"].as_str())
                .collect();
            // Exit code 2 (internal error) doesn't count, and neither does a rejection for
            // another reason: the attack must be caught by the trust checker.
            let rejected = out.status.code() == Some(common::EXIT_DIAGNOSTICS)
                && !codes.is_empty()
                && codes.iter().all(|&c| c == "W0107");
            (!rejected).then(|| format!("{}\n{}", file.display(), common::render(&out)))
        })
        .collect();
    assert!(
        accepted.is_empty(),
        "{} attack programs not rejected with W0107:\n\n{}",
        accepted.len(),
        accepted.join("\n\n")
    );
}
