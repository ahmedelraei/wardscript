#![allow(clippy::expect_used)]

mod common;

#[test]
fn help() {
    let out = common::ward(&["--help"]);
    assert!(out.status.success());
    insta::assert_snapshot!(String::from_utf8_lossy(&out.stdout));
}

#[test]
fn build_with_errors_fails() {
    let out = common::ward(&[
        "build",
        "tests/ui/types_mismatch_let.wardscript",
        "-o",
        "unused",
    ]);
    assert_eq!(out.status.code(), Some(common::EXIT_DIAGNOSTICS));
    assert!(String::from_utf8_lossy(&out.stderr).contains("could not build"));
    assert!(!common::repo_root().join("unused").exists());
}

fn mock(name: &str, json: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, json).expect("write mock");
    path.to_str().expect("utf-8 path").to_owned()
}

#[test]
fn run_calls_a_function() {
    let answers = mock(
        "triage.json",
        r#"{"triage": {"customer": "Ada", "summary": "Crash", "priority": "Urgent",
            "category": "Billing", "tags": [], "order_id": null}}"#,
    );
    let traces = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("run_traces");
    let _ = std::fs::remove_dir_all(&traces);
    let dir = traces.to_str().expect("utf-8 path");
    let out = common::ward(&[
        "run",
        "examples/triage.wardscript",
        "route",
        "\"help\"",
        "--mock",
        &answers,
        "--trace-dir",
        dir,
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", common::render(&out));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "\"[urgent] billing: Crash\"\n"
    );
    // The run's trace, named on stderr, is what `ward trace show` reads.
    let stderr = String::from_utf8_lossy(&out.stderr);
    let run = stderr
        .lines()
        .find_map(|l| l.strip_prefix("trace: "))
        .and_then(|l| l.split(' ').next())
        .expect("trace line");
    let shown = common::ward(&["trace", "show", run, "--dir", dir]);
    assert_eq!(shown.status.code(), Some(0), "{}", common::render(&shown));
    assert!(String::from_utf8_lossy(&shown.stdout).contains("`ai fn triage` attempt 1"));
    assert_eq!(
        common::ward(&["trace", "show", "nope", "--dir", dir])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn run_reports_failures() {
    let run = |args: &[&str]| {
        let mut all = vec!["run", "--no-trace", "tests/e2e/semantics.wardscript"];
        all.extend_from_slice(args);
        common::ward(&all)
    };
    // Thrown out of the function, and a runtime panic.
    let out = run(&["thrown", "\"nope\""]);
    assert_eq!(out.status.code(), Some(3), "{}", common::render(&out));
    assert!(String::from_utf8_lossy(&out.stderr).contains(r#"threw {"Bad": ["nope"]}"#));
    assert_eq!(run(&["int_div", "1", "0"]).status.code(), Some(3));
    // Usage errors.
    assert_eq!(run(&["missing"]).status.code(), Some(2));
    assert_eq!(run(&["int_div", "1"]).status.code(), Some(2));
    assert_eq!(run(&["int_div", "1", "\"x\""]).status.code(), Some(2));
    // Negative numbers are arguments, not flags.
    let out = run(&["int_div", "-7", "2"]);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "-3\n",
        "{}",
        common::render(&out)
    );
}

#[test]
fn unreadable_file_is_an_internal_error() {
    let out = common::ward(&["check", "examples/missing.wardscript"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn examples_check_ok() {
    let mut files = common::wardscript_files("examples");
    for entry in std::fs::read_dir(common::repo_root().join("examples")).expect("read examples") {
        let main = entry.expect("dir entry").path().join("main.ward");
        if main.is_file() {
            files.push(main);
        }
    }
    for file in files {
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
        .split("<!-- wardscript")
        .skip(1)
        // Skip the rest of the marker line and the ```rust fence line.
        .filter_map(|rest| rest.splitn(3, '\n').nth(2))
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

#[test]
fn lock_is_up_to_date() {
    let out = common::ward(&["lock", "--check", "examples/inbox/mcp.json"]);
    assert_eq!(out.status.code(), Some(0), "{}", common::render(&out));
    assert!(String::from_utf8_lossy(&out.stderr).contains("gmail: 4 tools"));
}

#[test]
fn lock_writes_and_checks() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("lock");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let example = common::repo_root().join("examples/inbox");
    for f in ["mcp.json", "gmail_server.py"] {
        std::fs::copy(example.join(f), dir.join(f)).expect("copy");
    }
    let config = dir.join("mcp.json");
    let config = config.to_str().expect("utf-8 path");

    let out = common::ward(&["lock", "--check", config]);
    assert_eq!(
        out.status.code(),
        Some(common::EXIT_DIAGNOSTICS),
        "{}",
        common::render(&out)
    );
    assert!(!dir.join("ward.lock").exists());

    let out = common::ward(&["lock", config]);
    assert_eq!(out.status.code(), Some(0), "{}", common::render(&out));
    let written = std::fs::read_to_string(dir.join("ward.lock")).expect("lock written");
    let expected = std::fs::read_to_string(example.join("ward.lock")).expect("example lock");
    assert_eq!(written, expected);

    let out = common::ward(&["lock", "--check", config]);
    assert_eq!(out.status.code(), Some(0), "{}", common::render(&out));
}

#[test]
fn lock_reports_a_broken_server() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("lock_broken");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(
        dir.join("mcp.json"),
        r#"{"mcpServers": {"x": {"command": "/no/such/server"}}}"#,
    )
    .expect("write config");
    let out = common::ward(&["lock", dir.join("mcp.json").to_str().expect("utf-8")]);
    assert_eq!(out.status.code(), Some(2), "{}", common::render(&out));
    assert!(String::from_utf8_lossy(&out.stderr).contains("server `x` didn't start"));
}

#[test]
fn run_connects_to_mcp_servers() {
    let answers = mock("inbox.json", r#"{"classify": "Other"}"#);
    let out = common::ward(&[
        "run",
        "examples/inbox/main.ward",
        "triage_inbox",
        "\"owner@example.com\"",
        "--mock",
        &answers,
        "--no-trace",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", common::render(&out));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "3");
}
