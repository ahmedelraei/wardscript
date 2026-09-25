//! Tool schemas from `ward.lock`, which `ward lock` writes from what each MCP server
//! lists (`tools/list`). An `import mcp "gmail" as mail` whose source is in the lock gets
//! typed functions; one that isn't stays dynamic.

use std::collections::BTreeMap;

use crate::json::Json;

/// The lock file's name. It's looked up in the entry file's directory and its parents.
pub const LOCK_FILE: &str = "ward.lock";
pub const LOCK_VERSION: u64 = 1;

/// A tool's parameter or result type, as far as a JSON Schema says.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolTy {
    String,
    Int,
    Float,
    Bool,
    List(Box<ToolTy>),
    Option(Box<ToolTy>),
    /// Objects and anything the schema doesn't pin down: accepted as a dynamic value.
    Any,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolParam {
    /// The name the server knows it by.
    pub name: String,
    pub ty: ToolTy,
    pub required: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolFn {
    /// The name in Wardscript: the server's name with characters that can't be in a
    /// name replaced by `_`.
    pub name: String,
    /// The name the server knows it by.
    pub mcp_name: String,
    pub description: Option<String>,
    /// Required parameters first, in schema order, then optional ones.
    pub params: Vec<ToolParam>,
    /// From `outputSchema`; `String` (the text content) without one.
    pub result: ToolTy,
    /// MCP's `readOnlyHint`: the tool doesn't change anything.
    pub read_only: bool,
    /// MCP's `openWorldHint` (default true): the tool talks to the outside world.
    pub open_world: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolServer {
    pub source: String,
    pub functions: Vec<ToolFn>,
}

impl ToolServer {
    pub fn function(&self, name: &str) -> Option<&ToolFn> {
        self.functions.iter().find(|f| f.name == name)
    }
}

/// The lock that applies to a program.
#[derive(Clone, Debug, Default)]
pub struct ToolLock {
    /// Where it was found, as shown in diagnostics.
    pub path: String,
    pub servers: BTreeMap<String, ToolServer>,
    /// Why it couldn't be read; then `servers` is empty.
    pub error: Option<String>,
}

pub fn parse_lock(text: &str) -> Result<BTreeMap<String, ToolServer>, String> {
    let json = Json::parse(text).map_err(|e| format!("not valid JSON: {e}"))?;
    match json.get("version") {
        Some(Json::Number(n)) if n.as_u64() == Some(LOCK_VERSION) => {}
        Some(v) => {
            return Err(format!(
                "unsupported version {}; this `ward` reads version {LOCK_VERSION}",
                serde_json::to_string(v).unwrap_or_default()
            ));
        }
        None => return Err("missing `version`".into()),
    }
    let servers = json
        .get("servers")
        .and_then(Json::as_object)
        .ok_or("missing `servers` object")?;
    let mut out = BTreeMap::new();
    for (source, server) in servers {
        let tools = server
            .get("tools")
            .and_then(Json::as_array)
            .ok_or_else(|| format!("server `{source}` has no `tools` list"))?;
        let functions = tools
            .iter()
            .map(|t| tool_fn(t).map_err(|e| format!("server `{source}`: {e}")))
            .collect::<Result<Vec<_>, _>>()?;
        out.insert(
            source.clone(),
            ToolServer {
                source: source.clone(),
                functions,
            },
        );
    }
    Ok(out)
}

/// A server's name for a tool as a Wardscript name.
pub fn wardscript_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    let mut diags = Vec::new();
    let toks = ward_syntax::lexer::lex(&out, 0, &mut diags);
    if toks.first().is_some_and(|t| t.kind.is_keyword()) {
        out.push('_');
    }
    out
}

fn tool_fn(t: &Json) -> Result<ToolFn, String> {
    let mcp_name = t
        .get("name")
        .and_then(Json::as_str)
        .ok_or("a tool without a `name`")?
        .to_owned();
    let input = t.get("inputSchema");
    let required: Vec<&str> = input
        .and_then(|s| s.get("required"))
        .and_then(Json::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(Json::as_str)
        .collect();
    let props = input
        .and_then(|s| s.get("properties"))
        .and_then(Json::as_object)
        .unwrap_or_default();
    let mut params: Vec<ToolParam> = props
        .iter()
        .map(|(name, schema)| ToolParam {
            name: name.clone(),
            ty: schema_ty(schema),
            required: required.contains(&name.as_str()),
        })
        .collect();
    // Stable: required parameters keep their order, and so do optional ones.
    params.sort_by_key(|p| !p.required);
    let result = match t.get("outputSchema") {
        Some(s) => schema_ty(s),
        None => ToolTy::String,
    };
    let hint = |key: &str| {
        t.get("annotations")
            .and_then(|a| a.get(key))
            .and_then(Json::as_bool)
    };
    Ok(ToolFn {
        name: wardscript_name(&mcp_name),
        description: t
            .get("description")
            .and_then(Json::as_str)
            .map(str::to_owned),
        mcp_name,
        params,
        result,
        read_only: hint("readOnlyHint").unwrap_or(false),
        open_world: hint("openWorldHint").unwrap_or(true),
    })
}

fn is_null(s: &Json) -> bool {
    s.get("type").and_then(Json::as_str) == Some("null")
}

pub fn schema_ty(s: &Json) -> ToolTy {
    match s.get("type") {
        Some(Json::String(t)) => named_ty(t, s),
        Some(Json::Array(ts)) => {
            let names: Vec<&str> = ts.iter().filter_map(Json::as_str).collect();
            let non_null: Vec<&str> = names.iter().copied().filter(|t| *t != "null").collect();
            match non_null.as_slice() {
                [t] if names.len() == 2 => ToolTy::Option(Box::new(named_ty(t, s))),
                [t] => named_ty(t, s),
                _ => ToolTy::Any,
            }
        }
        _ => {
            if let Some(alts) = s
                .get("anyOf")
                .or_else(|| s.get("oneOf"))
                .and_then(Json::as_array)
            {
                let non_null: Vec<&Json> = alts.iter().filter(|a| !is_null(a)).collect();
                return match non_null.as_slice() {
                    [one] if alts.len() == 2 => ToolTy::Option(Box::new(schema_ty(one))),
                    [one] => schema_ty(one),
                    _ => ToolTy::Any,
                };
            }
            match s.get("enum").and_then(Json::as_array) {
                Some(vs) if !vs.is_empty() && vs.iter().all(|v| v.as_str().is_some()) => {
                    ToolTy::String
                }
                _ => ToolTy::Any,
            }
        }
    }
}

fn named_ty(t: &str, s: &Json) -> ToolTy {
    match t {
        "string" => ToolTy::String,
        "integer" => ToolTy::Int,
        "number" => ToolTy::Float,
        "boolean" => ToolTy::Bool,
        "array" => ToolTy::List(Box::new(s.get("items").map_or(ToolTy::Any, schema_ty))),
        _ => ToolTy::Any,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK: &str = r#"{
      "version": 1,
      "servers": {
        "gmail": {
          "tools": [
            {
              "name": "send-email",
              "inputSchema": {
                "type": "object",
                "properties": {
                  "to": {"type": "string"},
                  "cc": {"type": ["array", "null"], "items": {"type": "string"}},
                  "subject": {"type": "string"},
                  "body": {"type": "string"}
                },
                "required": ["to", "subject", "body"]
              }
            },
            {
              "name": "search",
              "inputSchema": {"type": "object", "properties": {"limit": {"type": "integer"}}},
              "outputSchema": {"type": "object"},
              "annotations": {"readOnlyHint": true, "openWorldHint": false}
            }
          ]
        }
      }
    }"#;

    #[test]
    fn reads_a_lock() {
        let servers = parse_lock(LOCK).unwrap_or_default();
        let gmail = &servers["gmail"];
        let send = &gmail.functions[0];
        assert_eq!(
            (send.name.as_str(), send.mcp_name.as_str()),
            ("send_email", "send-email")
        );
        let names: Vec<&str> = send.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["to", "subject", "body", "cc"]);
        assert_eq!(
            send.params[3].ty,
            ToolTy::Option(Box::new(ToolTy::List(Box::new(ToolTy::String))))
        );
        assert_eq!(send.result, ToolTy::String);
        assert!(!send.read_only && send.open_world);
        let search = &gmail.functions[1];
        assert_eq!(search.result, ToolTy::Any);
        assert!(search.read_only && !search.open_world);
        assert!(!search.params[0].required);
    }

    #[test]
    fn rejects_bad_locks() {
        assert!(parse_lock("{").is_err());
        assert!(parse_lock(r#"{"version": 2, "servers": {}}"#).is_err());
        assert!(parse_lock(r#"{"version": 1}"#).is_err());
        assert!(parse_lock(r#"{"version": 1, "servers": {"x": {}}}"#).is_err());
    }

    #[test]
    fn names() {
        assert_eq!(wardscript_name("send-email"), "send_email");
        assert_eq!(wardscript_name("1st"), "_1st");
        assert_eq!(wardscript_name("match"), "match_");
    }
}
