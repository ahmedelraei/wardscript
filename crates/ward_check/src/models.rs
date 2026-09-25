//! `model {...}` clauses: which models an `ai fn` asks, and how it retries them. The
//! aliases name models configured in the runtime, so only the clause's shape is
//! checked here.

use ward_resolve::{Program, ProgramDiagnostic};
use ward_syntax::Diagnostic;
use ward_syntax::ast::{FnDecl, Item, ModelValue};
use ward_syntax::diag::codes;

pub const KEYS: [&str; 4] = ["primary", "fallback", "retries", "backoff"];

pub fn check(program: &Program) -> Vec<ProgramDiagnostic> {
    let mut out = Vec::new();
    for m in program.module_ids() {
        for item in &program.module(m).ast.items {
            let Item::Fn(f) = item else { continue };
            for d in check_fn(f) {
                out.push(ProgramDiagnostic {
                    module: m,
                    diagnostic: d,
                });
            }
        }
    }
    out
}

fn invalid(span: ward_syntax::Span, message: String, label: &str) -> Diagnostic {
    Diagnostic::error(codes::INVALID_MODEL_CLAUSE, message, span).with_label(label.to_owned())
}

fn check_fn(f: &FnDecl) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if let (Some(checks), false) = (&f.checks, f.is_ai) {
        out.push(
            Diagnostic::error(
                codes::CHECK_NOT_AI,
                format!(
                    "`{}` isn't an `ai fn`, so it has no answer to check",
                    f.name.name
                ),
                checks.span,
            )
            .with_label("a `check` clause only goes on an `ai fn`")
            .with_help("check other values with `validate` or an `if`"),
        );
    }
    out.extend(model_clause(f));
    out
}

fn model_clause(f: &FnDecl) -> Vec<Diagnostic> {
    let Some(clause) = &f.model else {
        return Vec::new();
    };
    if !f.is_ai {
        return vec![
            Diagnostic::error(
                codes::MODEL_CLAUSE_NOT_AI,
                format!(
                    "`{}` isn't an `ai fn`, so it doesn't call a model",
                    f.name.name
                ),
                clause.span,
            )
            .with_label("a `model` clause only goes on an `ai fn`")
            .with_help("move it to the `ai fn`s this function calls"),
        ];
    }
    let mut out = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    let mut primary: Option<&str> = None;
    for e in &clause.entries {
        let key = e.name.name.as_str();
        if seen.contains(&key) {
            out.push(invalid(
                e.name.span,
                format!("`{key}` is set twice"),
                "set again here",
            ));
            continue;
        }
        seen.push(key);
        let d = match (key, &e.value) {
            ("primary", ModelValue::Name(n)) => {
                primary = Some(&n.name);
                None
            }
            ("fallback", ModelValue::Name(n)) => (primary == Some(n.name.as_str())).then(|| {
                invalid(
                    n.span,
                    format!("`{}` is already the primary model", n.name),
                    "falls back to itself",
                )
            }),
            ("fallback", ModelValue::Names(names, span)) => {
                let mut d = None;
                if names.is_empty() {
                    d = Some(invalid(
                        *span,
                        "`fallback` lists no models".to_owned(),
                        "empty list",
                    ));
                }
                for (i, n) in names.iter().enumerate() {
                    if d.is_some() {
                        break;
                    }
                    if names[..i].iter().any(|x| x.name == n.name)
                        || primary == Some(n.name.as_str())
                    {
                        d = Some(invalid(
                            n.span,
                            format!("`{}` is listed twice", n.name),
                            "listed again here",
                        ));
                    }
                }
                d
            }
            ("retries", ModelValue::Number(text, span)) => {
                match text.replace('_', "").parse::<u32>() {
                    Ok(_) => None,
                    Err(_) => Some(invalid(
                        *span,
                        format!("`retries` must be a whole number, not `{text}`"),
                        "expected e.g. `2`",
                    )),
                }
            }
            ("backoff", ModelValue::Number(text, span)) => {
                match text.replace('_', "").parse::<f64>() {
                    Ok(v) if v.is_finite() && v >= 0.0 => None,
                    _ => Some(invalid(
                        *span,
                        format!("`backoff` must be a number of seconds, not `{text}`"),
                        "expected e.g. `0.5`",
                    )),
                }
            }
            ("primary" | "fallback", v) => Some(invalid(
                v.span(),
                format!("`{key}` takes a model alias, like `fast`"),
                "expected a model alias",
            )),
            ("retries" | "backoff", v) => Some(invalid(
                v.span(),
                format!("`{key}` takes a number"),
                "expected a number",
            )),
            _ => {
                let d = invalid(
                    e.name.span,
                    format!("unknown `model` setting `{key}`"),
                    "not a model setting",
                );
                Some(match ward_resolve::did_you_mean(key, KEYS.to_vec()) {
                    Some(s) => d.with_help(format!("did you mean `{s}`?")),
                    None => d.with_help(format!(
                        "settings are {}",
                        KEYS.map(|k| format!("`{k}`")).join(", ")
                    )),
                })
            }
        };
        out.extend(d);
    }
    out
}
