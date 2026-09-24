use std::io::{self, Write};

use ariadne::{Config, IndexType, Label, Report, ReportKind, Source};
use serde_json::{Value, json};
use ws_resolve::{Program, ProgramDiagnostic};
use ws_syntax::{LineIndex, Severity, Span};

pub fn human(
    program: &Program,
    diags: &[ProgramDiagnostic],
    color: bool,
    out: &mut impl Write,
) -> io::Result<()> {
    let config = Config::default()
        .with_color(color)
        .with_index_type(IndexType::Byte);
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
        report
            .finish()
            .write((path, Source::from(src)), &mut *out)?;
    }
    Ok(())
}

/// The `--format json` schema. Field names and meanings are stable; only additions are allowed.
pub fn json(program: &Program, diags: &[ProgramDiagnostic]) -> Value {
    let diagnostics: Vec<Value> = diags
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
            })
        })
        .collect();
    let entry = program.modules.first().map_or("", |m| m.path.as_str());
    json!({ "file": entry, "diagnostics": diagnostics })
}
