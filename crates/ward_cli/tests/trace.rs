//! Runs the support example with tracing on, then `ward trace show` and `ward trace export`
//! on the trace it wrote. The shown trace must link every value that reached a tool back
//! to where it came from.

#![allow(clippy::expect_used, clippy::panic)]

mod common;

fn redact(text: &str, run: &str) -> String {
    let text = text.replace(run, "<run>");
    // Durations in the summary line.
    let re_secs = text
        .lines()
        .map(|l| match l.rfind(", ") {
            Some(i) if l.starts_with("end: ") && l.ends_with("s)") => {
                format!("{}, <time>)", &l[..i])
            }
            _ => l.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    re_secs + "\n"
}

#[test]
fn support_trace() {
    let out = common::build("trace_support", "examples/support.wardscript");
    let traces = out.join("traces");
    let _ = std::fs::remove_dir_all(&traces);
    let mut paths = vec![out.clone()];
    if std::env::var_os("WARD_RUNTIME_INSTALLED").is_none() {
        paths.push(common::runtime_py());
    }
    let run = common::python()
        .arg(common::repo_root().join("tests/e2e/traces/run_support.py"))
        .arg(&traces)
        .env(
            "PYTHONPATH",
            std::env::join_paths(paths).expect("PYTHONPATH"),
        )
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .current_dir(&out)
        .output()
        .expect("run python");
    assert!(run.status.success(), "{}", common::render(&run));

    let dir = traces.to_str().expect("utf-8 path");
    let shown = common::ward(&["trace", "show", "--dir", dir]);
    assert_eq!(shown.status.code(), Some(0), "{}", common::render(&shown));
    let text = String::from_utf8_lossy(&shown.stdout).into_owned();
    let run_id = text
        .strip_prefix("run ")
        .and_then(|t| t.split(':').next())
        .expect("run id")
        .to_owned();
    insta::assert_snapshot!("support_trace", redact(&text, &run_id));

    let by_prefix = common::ward(&["trace", "show", &run_id[..8], "--dir", dir]);
    assert_eq!(by_prefix.stdout, shown.stdout);

    let otlp = common::ward(&["trace", "export", "--dir", dir]);
    let otlp: serde_json::Value = serde_json::from_slice(&otlp.stdout).expect("OTLP JSON");
    let spans = &otlp["resourceSpans"][0]["scopeSpans"][0]["spans"];
    let names: Vec<&str> = spans
        .as_array()
        .expect("spans")
        .iter()
        .map(|s| s["name"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(
        names,
        [
            "run handle",
            "ai fn triage",
            "ai fn draft_reply",
            "gmail.send"
        ]
    );
    let events: Vec<&str> = spans[0]["events"]
        .as_array()
        .expect("events")
        .iter()
        .map(|e| e["name"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(events, ["approve", "usage"]);
}
