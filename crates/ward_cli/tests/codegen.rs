//! Snapshots of the Python generated for the examples and e2e programs, so changes to
//! the output are reviewed deliberately.

mod common;

#[test]
fn generated_python() {
    let mut programs: Vec<(String, String)> = common::wardscript_files("examples")
        .into_iter()
        .chain(common::wardscript_files("tests/e2e"))
        .map(|p| {
            let rel = p
                .strip_prefix(common::repo_root())
                .expect("under repo root");
            let name = p
                .file_stem()
                .and_then(|s| s.to_str())
                .expect("name")
                .to_owned();
            (name, rel.to_str().expect("utf-8 path").to_owned())
        })
        .collect();
    programs.push(("modules".into(), "tests/e2e/modules/main.ward".into()));
    programs.push((
        "support_async".into(),
        "examples/support.ward --async".into(),
    ));
    for (name, src) in programs {
        let mut words = src.split_whitespace();
        let file = words.next().unwrap_or_default();
        let flags: Vec<&str> = words.collect();
        let out = common::build_with(&format!("codegen_{name}"), file, &flags);
        let mut snapshot = String::new();
        for file in common::files_under(&out) {
            let text = std::fs::read_to_string(out.join(&file)).expect("read output");
            snapshot.push_str(&format!("=== {} ===\n{text}\n", file.display()));
        }
        insta::with_settings!({ description => src.as_str(), omit_expression => true }, {
            insta::assert_snapshot!(name, snapshot);
        });
    }
}

/// The same for the TypeScript backend.
#[test]
fn generated_typescript() {
    let mut programs: Vec<(String, String)> = common::wardscript_files("examples")
        .into_iter()
        .chain(common::wardscript_files("tests/e2e"))
        .map(|p| {
            let rel = p
                .strip_prefix(common::repo_root())
                .expect("under repo root");
            let name = p
                .file_stem()
                .and_then(|s| s.to_str())
                .expect("name")
                .to_owned();
            (name, rel.to_str().expect("utf-8 path").to_owned())
        })
        .collect();
    programs.push(("modules".into(), "tests/e2e/modules/main.ward".into()));
    for (name, src) in programs {
        let out = common::build_with(
            &format!("codegen_ts_{name}"),
            &src,
            &["--target", "typescript"],
        );
        let mut snapshot = String::new();
        for file in common::files_under(&out) {
            let text = std::fs::read_to_string(out.join(&file)).expect("read output");
            snapshot.push_str(&format!("=== {} ===\n{text}\n", file.display()));
        }
        insta::with_settings!({ description => src.as_str(), omit_expression => true }, {
            insta::assert_snapshot!(format!("ts_{name}"), snapshot);
        });
    }
}
