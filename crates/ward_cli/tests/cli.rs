mod common;

#[test]
fn help() {
    let out = common::ward(&["--help"]);
    assert!(out.status.success());
    insta::assert_snapshot!(String::from_utf8_lossy(&out.stdout));
}

#[test]
fn unimplemented_subcommand_is_an_internal_error() {
    let out = common::ward(&["build", "examples/support.wardscript"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn unreadable_file_is_an_internal_error() {
    let out = common::ward(&["check", "examples/missing.wardscript"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn examples_check_ok() {
    for file in common::wardscript_files("examples") {
        let out = common::ward(&["check", file.to_str().expect("utf-8 path")]);
        assert_eq!(out.status.code(), Some(0), "{}", common::render(&out));
    }
}

#[test]
fn diagnostic_codes_are_unique_and_documented() {
    use ward_syntax::diag::codes::ALL;
    let spec = std::fs::read_to_string(common::repo_root().join("docs/spec/diagnostics.md"))
        .expect("read diagnostics spec");
    let mut seen = std::collections::HashSet::new();
    for code in ALL {
        assert!(seen.insert(code.0), "{} is assigned twice", code.0);
        assert!(
            spec.contains(&format!("| {} |", code.0)),
            "{} is missing from docs/spec/diagnostics.md",
            code.0
        );
    }
    let declared = include_str!("../../ward_syntax/src/diag.rs")
        .matches("Code(\"W")
        .count();
    assert_eq!(
        declared,
        ALL.len(),
        "every code must be listed in `codes::ALL`"
    );
}

#[test]
fn readme_examples_check_ok() {
    let readme =
        std::fs::read_to_string(common::repo_root().join("README.md")).expect("read README");
    let blocks: Vec<&str> = readme
        .split("```wardscript\n")
        .skip(1)
        .filter_map(|rest| rest.split("```").next())
        .collect();
    assert!(!blocks.is_empty(), "README has no wardscript examples");
    for (i, block) in blocks.iter().enumerate() {
        let file = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("readme_{i}.wardscript"));
        std::fs::write(&file, block).expect("write example");
        let out = common::ward(&["check", file.to_str().expect("utf-8 path")]);
        assert_eq!(
            out.status.code(),
            Some(0),
            "README example {i}:\n{}",
            common::render(&out)
        );
    }
}
