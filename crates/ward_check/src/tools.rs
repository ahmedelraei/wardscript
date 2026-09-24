//! Which tool parameters are sinks. With a schema from `ward.lock`: every parameter of a
//! tool that may change something or reach the outside world, and none of a tool that
//! declares itself read-only and closed-world (`readOnlyHint`, `openWorldHint: false`).
//! `@sink(f.p)` and `@not_sink(f.p, reason = "...")` on the import override that.
//! Without a schema, every argument is a sink.

use ward_resolve::tools::ToolFn;
use ward_resolve::{DefId, Program};
use ward_syntax::ast::{Annotation, Item};

/// Sink flags for each parameter of `func` of the tool imported by `def`, or `None`
/// when the tool has no schema (then every argument is a sink).
pub fn tool_sinks(program: &Program, def: DefId, func: &str) -> Option<Vec<bool>> {
    let f = program.tool_schema(def)?.function(func)?;
    let annotations: &[Annotation] = match program.item(def) {
        Item::Import(i) => &i.annotations,
        _ => &[],
    };
    Some(
        f.params
            .iter()
            .map(|p| param_is_sink(f, &p.name, annotations))
            .collect(),
    )
}

pub fn default_sink(f: &ToolFn) -> bool {
    !f.read_only || f.open_world
}

fn param_is_sink(f: &ToolFn, param: &str, annotations: &[Annotation]) -> bool {
    let path = format!("{}.{param}", f.name);
    let mut sink = default_sink(f);
    for a in annotations {
        let named = a
            .args
            .iter()
            .any(|x| x.value.is_none() && x.name.name == path);
        match a.name.name.as_str() {
            "sink" if named => sink = true,
            "not_sink" if named => sink = false,
            _ => {}
        }
    }
    sink
}
