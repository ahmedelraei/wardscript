//! A trace as OpenTelemetry spans, in the OTLP/JSON encoding collectors accept. The run is
//! the root span; model and tool calls are child spans; checks, approvals and
//! declassifications are events on the root span.

use serde_json::{Value, json};

use crate::trace::{Event, Record, digest};

fn attr(key: &str, value: Value) -> Value {
    let v = match value {
        Value::String(s) => json!({ "stringValue": s }),
        Value::Bool(b) => json!({ "boolValue": b }),
        Value::Number(n) if n.is_i64() || n.is_u64() => json!({ "intValue": n.to_string() }),
        Value::Number(n) => json!({ "doubleValue": n }),
        other => json!({ "stringValue": other.to_string() }),
    };
    json!({ "key": key, "value": v })
}

fn span_id(run: &str, seq: u64) -> String {
    digest(&json!([run, seq]))
}

pub fn export(records: &[Record]) -> Value {
    let run = records.first().map_or("", |r| r.run.as_str());
    let trace_id = format!("{}{}", digest(&json!(run)), digest(&json!([run])));
    let root = span_id(run, 0);
    let mut spans = Vec::new();
    let mut root_events = Vec::new();
    let (mut name, mut start, mut end, mut status) = (String::new(), 0, 0, json!({}));
    for r in records {
        let time = r.time.to_string();
        match &r.event {
            Event::RunStart { function, .. } => {
                name = format!("run {function}");
                start = r.time;
            }
            Event::RunEnd {
                status: s,
                error,
                tokens,
                calls,
                cost,
            } => {
                end = r.time;
                status = match s {
                    crate::trace::Status::Ok => json!({ "code": 1 }),
                    _ => json!({ "code": 2, "message": error.clone().unwrap_or_default() }),
                };
                root_events.push(json!({
                    "timeUnixNano": time,
                    "name": "usage",
                    "attributes": [
                        attr("ward.tokens", json!(tokens)),
                        attr("ward.calls", json!(calls)),
                        attr("ward.cost", json!(cost)),
                    ],
                }));
            }
            Event::AiCall {
                started,
                function,
                attempt,
                model,
                tokens,
                cost,
                error,
                ..
            } => {
                let attributes: Vec<Value> = [
                    Some(attr("ward.attempt", json!(attempt))),
                    Some(attr("gen_ai.usage.total_tokens", json!(tokens))),
                    model
                        .as_ref()
                        .map(|m| attr("gen_ai.request.model", json!(m))),
                    cost.map(|c| attr("ward.cost", json!(c))),
                ]
                .into_iter()
                .flatten()
                .collect();
                spans.push(json!({
                "traceId": trace_id,
                "spanId": span_id(run, r.seq),
                "parentSpanId": root,
                "name": format!("ai fn {function}"),
                "kind": 3,
                "startTimeUnixNano": started.to_string(),
                "endTimeUnixNano": time,
                "attributes": attributes,
                "status": match error {
                    Some(e) => json!({ "code": 2, "message": e }),
                    None => json!({ "code": 1 }),
                },
            }))
            }
            Event::ToolCall {
                started,
                tool,
                function,
                site,
                error,
                ..
            } => spans.push(json!({
                "traceId": trace_id,
                "spanId": span_id(run, r.seq),
                "parentSpanId": root,
                "name": format!("{tool}.{function}"),
                "kind": 3,
                "startTimeUnixNano": started.to_string(),
                "endTimeUnixNano": time,
                "attributes": [attr("ward.site", json!(site))],
                "status": match error {
                    Some(e) => json!({ "code": 2, "message": e }),
                    None => json!({ "code": 1 }),
                },
            })),
            Event::Validate {
                rule, site, passed, ..
            } => root_events.push(json!({
                "timeUnixNano": time,
                "name": "validate",
                "attributes": [
                    attr("ward.rule", json!(rule)),
                    attr("ward.site", json!(site)),
                    attr("ward.passed", json!(passed)),
                ],
            })),
            Event::Approve { site, approved, .. } => root_events.push(json!({
                "timeUnixNano": time,
                "name": "approve",
                "attributes": [attr("ward.site", json!(site)), attr("ward.approved", json!(approved))],
            })),
            Event::Declassify { site, reason, .. } => root_events.push(json!({
                "timeUnixNano": time,
                "name": "declassify",
                "attributes": [attr("ward.site", json!(site)), attr("ward.reason", json!(reason))],
            })),
            Event::BudgetExceeded {
                function,
                resource,
                limit,
                used,
            } => root_events.push(json!({
                "timeUnixNano": time,
                "name": "budget_exceeded",
                "attributes": [
                    attr("ward.function", json!(function)),
                    attr("ward.resource", json!(resource)),
                    attr("ward.limit", json!(limit)),
                    attr("ward.used", json!(used)),
                ],
            })),
            Event::BudgetUnenforceable { function, when } => root_events.push(json!({
                "timeUnixNano": time,
                "name": "budget_unenforceable",
                "attributes": [
                    attr("ward.function", json!(function)),
                    attr("ward.when", json!(when)),
                ],
            })),
        }
    }
    spans.insert(
        0,
        json!({
            "traceId": trace_id,
            "spanId": root,
            "name": name,
            "kind": 1,
            "startTimeUnixNano": start.to_string(),
            "endTimeUnixNano": end.max(start).to_string(),
            "attributes": [attr("ward.run", json!(run))],
            "events": root_events,
            "status": status,
        }),
    );
    json!({
        "resourceSpans": [{
            "resource": { "attributes": [attr("service.name", json!("wardscript"))] },
            "scopeSpans": [{ "scope": { "name": "wardscript" }, "spans": spans }],
        }]
    })
}
