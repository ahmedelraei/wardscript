//! The checker and code generators compiled to WebAssembly, for the website's playground.
//!
//! Every function takes the program as a JSON object of files (`{"main.ward": "..."}`) and
//! the entry file's name, and returns JSON text, so the JavaScript side needs no bindings
//! beyond strings.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use ariadne::{Config, IndexType, Label, Report, ReportKind, Source};
use serde_json::{Value, json};
use ward_resolve::{FileSystem, Program, ProgramDiagnostic};
use ward_syntax::{LineIndex, Severity, Span};
use wasm_bindgen::prelude::wasm_bindgen;

/// Source files held in memory, keyed by path.
struct MemoryFs(HashMap<PathBuf, String>);

impl FileSystem for MemoryFs {
    fn read(&self, path: &Path) -> io::Result<String> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such file"))
    }
}

fn memory_fs(files: &str) -> Result<MemoryFs, String> {
    let files: HashMap<String, String> =
        serde_json::from_str(files).map_err(|e| format!("invalid files: {e}"))?;
    Ok(MemoryFs(
        files
            .into_iter()
            .map(|(p, s)| (PathBuf::from(p), s))
            .collect(),
    ))
}

fn analyze(files: &str, entry: &str) -> Result<ward_check::Analysis, String> {
    let fs = memory_fs(files)?;
    ward_check::analyze(Path::new(entry), &fs)
        .map_err(|e| format!("cannot read `{}`: {}", e.path.display(), e.error))
}

/// The diagnostics as `ward check` prints them, without color.
fn human(program: &Program, diags: &[ProgramDiagnostic]) -> String {
    let config = Config::default()
        .with_color(false)
        .with_index_type(IndexType::Byte);
    let mut out = Vec::new();
    for pd in diags {
        let module = program.module(pd.module);
        let (path, src, d) = (module.path.as_str(), module.src.as_str(), &pd.diagnostic);
        let kind = match d.severity {
            Severity::Error => ReportKind::Error,
            Severity::Warning => ReportKind::Warning,
        };
        let mut report = Report::build(kind, (path, d.span().range()))
            .with_config(config)
            .with_code(d.code.0)
            .with_message(&d.message);
        for label in &d.labels {
            let l = Label::new((path, label.span.range()));
            report = report.with_label(match &label.message {
                Some(m) => l.with_message(m),
                None => l,
            });
        }
        if let Some(help) = &d.help {
            report = report.with_help(help);
        }
        for note in &d.notes {
            report = report.with_note(note);
        }
        let _ = report.finish().write((path, Source::from(src)), &mut out);
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The diagnostics in the `ward check --format json` shape.
fn diagnostics(program: &Program, diags: &[ProgramDiagnostic]) -> Vec<Value> {
    diags
        .iter()
        .map(|pd| {
            let module = program.module(pd.module);
            let index = LineIndex::new(&module.src);
            let span = |s: Span| {
                let (start, end) = (index.line_col(s.start), index.line_col(s.end));
                json!({
                    "start": { "offset": s.start, "line": start.line, "column": start.column },
                    "end": { "offset": s.end, "line": end.line, "column": end.column },
                })
            };
            let d = &pd.diagnostic;
            let labels: Vec<Value> = d
                .labels
                .iter()
                .enumerate()
                .map(|(i, l)| json!({ "primary": i == 0, "span": span(l.span), "message": l.message }))
                .collect();
            json!({
                "file": module.path,
                "code": d.code.0,
                "severity": d.severity.as_str(),
                "message": d.message,
                "span": span(d.span()),
                "labels": labels,
                "help": d.help,
                "notes": d.notes,
            })
        })
        .collect()
}

/// Checks the program and, if it has no errors, lowers it; `report` always has the diagnostics.
fn compile(files: &str, entry: &str) -> (Value, Option<ward_ir::Program>) {
    let analysis = match analyze(files, entry) {
        Ok(a) => a,
        Err(error) => {
            return (
                json!({ "ok": false, "error": error, "diagnostics": [], "text": "" }),
                None,
            );
        }
    };
    let diags = analysis.diagnostics();
    let errors = diags
        .iter()
        .filter(|d| d.diagnostic.severity == Severity::Error)
        .count();
    let program = if errors == 0 {
        ward_ir::lower(&analysis).ok()
    } else {
        None
    };
    let functions: Vec<Value> = program
        .as_ref()
        .and_then(|p| p.modules.first())
        .map(|m| {
            m.fns
                .iter()
                .filter(|f| f.method.is_none() && f.generics.is_empty())
                .map(|f| {
                    let params: Vec<&str> = f
                        .params
                        .iter()
                        .map(|&p| f.locals[p].name.as_str())
                        .collect();
                    json!({ "name": f.name, "pub": f.is_pub, "params": params })
                })
                .collect()
        })
        .unwrap_or_default();
    let report = json!({
        "ok": errors == 0 && program.is_some(),
        "errors": errors,
        "warnings": diags.len() - errors,
        "diagnostics": diagnostics(&analysis.program, diags),
        "text": human(&analysis.program, diags),
        "functions": functions,
    });
    (report, program)
}

fn files_json<'a>(files: impl IntoIterator<Item = (&'a Path, &'a str)>) -> Value {
    files
        .into_iter()
        .map(|(p, c)| json!({ "path": p.to_string_lossy(), "contents": c }))
        .collect()
}

/// `ward check`: `{ok, errors, warnings, diagnostics, text, functions}`.
#[wasm_bindgen]
pub fn check(files: &str, entry: &str) -> String {
    compile(files, entry).0.to_string()
}

/// `ward build`: the check report, plus `files` (`[{path, contents}]`) when it has no errors.
/// `target` is `python` or `typescript`.
#[wasm_bindgen]
pub fn build(files: &str, entry: &str, target: &str, asyncio: bool) -> String {
    let (mut report, program) = compile(files, entry);
    if let Some(program) = program {
        report["files"] = match target {
            "typescript" => {
                let out = ward_codegen_ts::generate(&program);
                files_json(out.iter().map(|f| (f.path.as_path(), f.contents.as_str())))
            }
            _ => {
                let out = ward_codegen_py::generate_with(
                    &program,
                    ward_codegen_py::Options {
                        asyncio,
                        tests: false,
                    },
                );
                files_json(out.iter().map(|f| (f.path.as_path(), f.contents.as_str())))
            }
        };
    }
    report.to_string()
}

/// What `ward run` would execute: the check report, plus the generated Python `files`,
/// the runner `script` and the function's `arity`. The runner reads its arguments from
/// `sys.argv[1:]` (JSON) and its mock answers from the file named by `WARD_MOCK`.
#[wasm_bindgen]
pub fn run_plan(files: &str, entry: &str, function: &str) -> String {
    let (mut report, program) = compile(files, entry);
    if let Some(program) = program {
        match ward_codegen_py::runner(&program, function) {
            Some(runner) => {
                let out = ward_codegen_py::generate(&program);
                report["files"] =
                    files_json(out.iter().map(|f| (f.path.as_path(), f.contents.as_str())));
                report["script"] = runner.script.into();
                report["arity"] = runner.arity.into();
            }
            None => report["error"] = format!("no function `{function}` in `{entry}`").into(),
        }
    }
    report.to_string()
}

/// The `wardscript` Python runtime package, as `[{path, contents}]`.
#[wasm_bindgen]
pub fn python_runtime() -> String {
    files_json(
        ward_runtime::PYTHON_PACKAGE
            .iter()
            .map(|(p, c)| (Path::new(*p), *c)),
    )
    .to_string()
}

/// `ward trace show` for a trace's JSON lines.
#[wasm_bindgen]
pub fn trace_show(jsonl: &str) -> String {
    let records: Result<Vec<ward_runtime::trace::Record>, _> = jsonl
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(serde_json::from_str)
        .collect();
    match records {
        Ok(r) => ward_runtime::show::render(&r),
        Err(e) => format!("error: cannot read the trace: {e}"),
    }
}

/// `ward fmt` for one file; returns the source unchanged if it doesn't parse.
#[wasm_bindgen]
pub fn format(src: &str) -> String {
    ward_syntax::printer::format(src).unwrap_or_else(|_| src.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(src: &str) -> String {
        json!({ "main.ward": src }).to_string()
    }

    #[test]
    fn reports_untrusted_data_reaching_a_tool() {
        let src = "import mcp \"gmail\" as mail\n\nai fn reply(e: Untrusted<String>) -> String {\n    \"Reply to {e}\"\n}\n\npub fn go(e: Untrusted<String>) uses {llm, mail} {\n    mail.send(reply(e))\n}\n";
        let report: Value =
            serde_json::from_str(&check(&files(src), "main.ward")).unwrap_or_default();
        assert_eq!(report["ok"], false);
        assert_eq!(report["diagnostics"][0]["code"], "W0107");
        assert!(
            report["text"]
                .as_str()
                .unwrap_or_default()
                .contains("W0107")
        );
    }

    #[test]
    fn builds_and_plans_a_run() {
        let src = "pub fn add(a: Int, b: Int) -> Int {\n    a + b\n}\n";
        let built: Value =
            serde_json::from_str(&build(&files(src), "main.ward", "typescript", false))
                .unwrap_or_default();
        assert_eq!(built["ok"], true);
        assert!(
            built["files"][0]["contents"]
                .as_str()
                .unwrap_or_default()
                .contains("add")
        );
        let plan: Value =
            serde_json::from_str(&run_plan(&files(src), "main.ward", "add")).unwrap_or_default();
        assert_eq!(plan["arity"], 2);
        assert_eq!(plan["functions"][0]["params"][1], "b");
        assert!(!python_runtime().is_empty());
    }
}
