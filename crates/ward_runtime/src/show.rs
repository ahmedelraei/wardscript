//! `ward trace show`: a run's events in order, and for each value that reached a tool,
//! where it came from.

use std::fmt::Write;

use serde_json::Value;

use crate::trace::{Event, Leaf, Record, Status};

const MAX_VALUE: usize = 60;

fn short(v: &Value) -> String {
    let text = serde_json::to_string(v).unwrap_or_default();
    if text.chars().count() <= MAX_VALUE {
        text
    } else {
        let cut: String = text.chars().take(MAX_VALUE - 1).collect();
        format!("{cut}…")
    }
}

fn find_leaf<'a>(leaves: &'a [Leaf], digest: &str) -> Option<&'a Leaf> {
    leaves.iter().find(|l| l.digest == digest)
}

/// `$.subject of ` for a part, nothing for the whole value.
fn part(leaf: &Leaf) -> String {
    match leaf.path.as_str() {
        "$" => String::new(),
        p => format!("{p} of "),
    }
}

/// Where a value with this digest came from, looking back from event `before`.
fn source(records: &[Record], before: usize, digest: &str) -> String {
    for r in records[..before].iter().rev() {
        let n = r.seq;
        match &r.event {
            Event::AiCall {
                function, leaves, ..
            } => {
                if let Some(l) = find_leaf(leaves, digest) {
                    return format!(
                        "{}the output of `ai fn {function}` (#{n}), untrusted",
                        part(l)
                    );
                }
            }
            Event::ToolCall {
                tool,
                function,
                leaves,
                ..
            } => {
                if let Some(l) = find_leaf(leaves, digest) {
                    return format!(
                        "{}the result of `{tool}.{function}` (#{n}), untrusted",
                        part(l)
                    );
                }
            }
            Event::RunStart { args, .. } => {
                for a in args {
                    if let Some(l) = find_leaf(&a.leaves, digest) {
                        let path = l.path.trim_start_matches('$');
                        let how = if a.vouched {
                            "vouched for by the host"
                        } else {
                            "untrusted"
                        };
                        return format!("argument `{}`{path} from the host, {how}", a.name);
                    }
                }
            }
            _ => {}
        }
    }
    "no model, tool or host value: a literal, or computed from several".to_owned()
}

/// The provenance of a value that reached a tool.
fn provenance(records: &[Record], before: usize, digest: &str) -> String {
    for (i, r) in records[..before].iter().enumerate().rev() {
        let n = r.seq;
        let (what, leaves) = match &r.event {
            Event::Validate {
                rule,
                site,
                passed: true,
                leaves,
            } => (format!("validated by `{rule}` (#{n}, {site})"), leaves),
            Event::Approve {
                site,
                approved: true,
                leaves,
            } => (format!("approved by a human (#{n}, {site})"), leaves),
            Event::Declassify {
                site,
                reason,
                leaves,
            } => (format!("declassified: \"{reason}\" (#{n}, {site})"), leaves),
            _ => continue,
        };
        let Some(leaf) = find_leaf(leaves, digest) else {
            continue;
        };
        let part = match leaf.path.as_str() {
            "$" => String::new(),
            p => format!("{p} of a value "),
        };
        // The checked value as a whole came from somewhere; follow it.
        let whole = leaves.first().map_or(digest, |l| l.digest.as_str());
        let from = source(records, i, whole);
        let from = if leaf.path == "$" {
            from
        } else {
            format!("that value: {from}")
        };
        return format!("{part}{what} ← {from}");
    }
    source(records, before, digest)
}

pub fn render(records: &[Record]) -> String {
    let mut out = String::new();
    let run = records.first().map_or("?", |r| r.run.as_str());
    let start = records.first().map_or(0, |r| r.time);
    for (i, r) in records.iter().enumerate() {
        let n = r.seq;
        match &r.event {
            Event::RunStart { function, args } => {
                let _ = writeln!(out, "run {run}: `{function}`");
                for a in args {
                    let how = if a.vouched { "vouched" } else { "untrusted" };
                    let _ = writeln!(out, "  {} = {} ({how})", a.name, short(&a.value));
                }
            }
            Event::AiCall {
                function,
                attempt,
                model,
                tokens,
                cost,
                error,
                ..
            } => {
                let outcome = match error {
                    Some(e) => format!("rejected: {e}"),
                    None => "ok".to_owned(),
                };
                let cost = cost.map_or("$?".to_owned(), |c| format!("${c:.4}"));
                let asked = model
                    .as_ref()
                    .map_or_else(String::new, |m| format!(" ({m})"));
                let _ = writeln!(
                    out,
                    "#{n:<3} model  `ai fn {function}` attempt {}{asked}, {tokens} tokens, {cost}: {outcome}",
                    attempt + 1
                );
            }
            Event::ToolCall {
                tool,
                function,
                site,
                args,
                digests,
                error,
                ..
            } => {
                let _ = writeln!(out, "#{n:<3} tool   `{tool}.{function}` at {site}");
                for (j, (v, d)) in args.iter().zip(digests).enumerate() {
                    let _ = writeln!(
                        out,
                        "         arg {}: {} ← {}",
                        j + 1,
                        short(v),
                        provenance(records, i, d)
                    );
                }
                if let Some(e) = error {
                    let _ = writeln!(out, "         failed: {e}");
                }
            }
            Event::Validate {
                rule, site, passed, ..
            } => {
                let r = if *passed { "passed" } else { "rejected" };
                let _ = writeln!(out, "#{n:<3} check  `{rule}` {r} at {site}");
            }
            Event::Approve { site, approved, .. } => {
                let r = if *approved { "approved" } else { "denied" };
                let _ = writeln!(out, "#{n:<3} human  {r} at {site}");
            }
            Event::Declassify { site, reason, .. } => {
                let _ = writeln!(out, "#{n:<3} trust  declassified at {site}: \"{reason}\"");
            }
            Event::BudgetExceeded {
                function,
                resource,
                limit,
                used,
            } => {
                let _ = writeln!(
                    out,
                    "#{n:<3} budget `{function}` went over {resource}: {used} of {limit}"
                );
            }
            Event::BudgetUnenforceable { function, when } => {
                let why = if when == "before" {
                    "the model has no prices"
                } else {
                    "the model's answer has no cost"
                };
                let _ = writeln!(
                    out,
                    "#{n:<3} budget `{function}` has a cost limit, but {why}"
                );
            }
            Event::RunEnd {
                status,
                error,
                tokens,
                calls,
                cost,
            } => {
                let secs = r.time.saturating_sub(start) as f64 / 1e9;
                let status = match status {
                    Status::Ok => "ok".to_owned(),
                    Status::Threw => format!("threw {}", error.as_deref().unwrap_or("")),
                    Status::Error => format!("failed: {}", error.as_deref().unwrap_or("")),
                };
                let _ = writeln!(
                    out,
                    "end: {status} ({calls} model calls, {tokens} tokens, ${cost:.4}, {secs:.2}s)"
                );
            }
        }
    }
    out
}
