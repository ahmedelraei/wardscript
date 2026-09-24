//! Runs `ward check --format json` on every `tests/ui/*.wardscript` and snapshots the result.

mod common;

#[test]
fn ui() {
    for file in common::wardscript_files("tests/ui") {
        let name = file
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("utf-8 name");
        let rel = file
            .strip_prefix(common::repo_root())
            .expect("under repo root");
        let rel = rel.to_str().expect("utf-8 path");
        let out = common::ward(&["check", "--format", "json", rel]);
        insta::with_settings!({
            snapshot_path => common::repo_root().join("tests/ui/snapshots"),
            prepend_module_to_snapshot => false,
            description => rel,
        }, {
            insta::assert_snapshot!(name, common::render(&out));
        });
    }
}
