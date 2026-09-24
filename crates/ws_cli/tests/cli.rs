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
