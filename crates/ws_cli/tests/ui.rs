//! Runs `ward check` on every `tests/ui/*.wardscript`, and on `tests/ui/<case>/main.wardscript`
//! for multi-module cases, and snapshots the human and JSON output.

mod common;

#[test]
fn ui() {
    for (name, file) in common::ui_cases() {
        let rel = file
            .strip_prefix(common::repo_root())
            .expect("under repo root");
        let rel = rel.to_str().expect("utf-8 path");
        let human = common::ward(&["check", rel]);
        let json = common::ward(&["check", "--format", "json", rel]);
        assert_eq!(
            human.status.code(),
            json.status.code(),
            "{rel}: exit codes differ"
        );
        let snapshot = format!(
            "exit: {:?}\n--- human ---\n{}--- json ---\n{}",
            human.status.code(),
            String::from_utf8_lossy(&human.stderr),
            String::from_utf8_lossy(&json.stdout),
        );
        insta::with_settings!({
            snapshot_path => common::repo_root().join("tests/ui/snapshots"),
            prepend_module_to_snapshot => false,
            description => rel,
            omit_expression => true,
        }, {
            insta::assert_snapshot!(name, snapshot);
        });
    }
}
