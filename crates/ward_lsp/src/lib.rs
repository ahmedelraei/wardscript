//! The Wardscript language server (`ward lsp`): the Language Server Protocol over stdio,
//! with diagnostics as you type, the type of an expression on hover, and go to
//! definition. Each open file is checked as the entry of its own program; its imports
//! are read from open editors first, then from disk.

mod protocol;
mod uri;

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use ward_check::Analysis;
use ward_resolve::{FileSystem, ModuleId, TypeRes, ValueRes};
use ward_syntax::ast::Item;
use ward_syntax::{Severity, Span};

pub use crate::protocol::Message;
use crate::protocol::{read_message, write_message};
pub use crate::uri::{path_to_uri, uri_to_path};

/// Open documents, by path; everything else comes from disk.
struct Overlay<'a>(&'a HashMap<PathBuf, String>);

impl FileSystem for Overlay<'_> {
    fn read(&self, path: &Path) -> io::Result<String> {
        match self.0.get(path) {
            Some(text) => Ok(text.clone()),
            None => std::fs::read_to_string(path),
        }
    }
}

pub struct Server {
    docs: HashMap<PathBuf, String>,
    shutdown: bool,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs the server until the client sends `exit`. Returns whether it shut down
/// cleanly (a `shutdown` request came first).
pub fn serve(input: impl BufRead, mut output: impl Write) -> io::Result<bool> {
    let mut server = Server::new();
    let mut input = input;
    while let Some(message) = read_message(&mut input)? {
        if message.method.as_deref() == Some("exit") {
            return Ok(server.shutdown);
        }
        for reply in server.handle(message) {
            write_message(&mut output, &reply)?;
        }
    }
    Ok(false)
}

impl Server {
    pub fn new() -> Self {
        Server {
            docs: HashMap::new(),
            shutdown: false,
        }
    }

    /// Replies and notifications to send for a message from the client.
    pub fn handle(&mut self, message: Message) -> Vec<Value> {
        let params = message.params.unwrap_or(Value::Null);
        let Some(method) = message.method else {
            return Vec::new(); // A response to something we never ask.
        };
        let reply = |result: Value| match &message.id {
            Some(id) => vec![json!({"jsonrpc": "2.0", "id": id, "result": result})],
            None => Vec::new(),
        };
        match method.as_str() {
            "initialize" => reply(json!({
                "capabilities": {
                    "textDocumentSync": {"openClose": true, "change": 1, "save": true},
                    "hoverProvider": true,
                    "definitionProvider": true,
                    "documentFormattingProvider": true,
                    "documentFormattingProvider": true,
                },
                "serverInfo": {"name": "ward", "version": env!("CARGO_PKG_VERSION")},
            })),
            "shutdown" => {
                self.shutdown = true;
                reply(Value::Null)
            }
            "textDocument/didOpen" => {
                let doc = &params["textDocument"];
                self.open(doc["uri"].as_str(), doc["text"].as_str())
            }
            "textDocument/didChange" => {
                // Full sync: the last change is the whole text.
                let text = params["contentChanges"]
                    .as_array()
                    .and_then(|c| c.last())
                    .and_then(|c| c["text"].as_str());
                self.open(params["textDocument"]["uri"].as_str(), text)
            }
            "textDocument/didSave" => {
                let uri = params["textDocument"]["uri"].as_str();
                let text = uri
                    .and_then(uri_to_path)
                    .and_then(|p| self.docs.get(&p).cloned());
                self.open(uri, text.as_deref())
            }
            "textDocument/didClose" => {
                let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
                if let Some(path) = uri_to_path(uri) {
                    self.docs.remove(&path);
                }
                vec![publish(uri, Vec::new())]
            }
            "textDocument/hover" => reply(self.at(&params, hover).unwrap_or(Value::Null)),
            "textDocument/definition" => reply(self.at(&params, definition).unwrap_or(Value::Null)),
            "textDocument/formatting" => reply(self.format(&params).unwrap_or(Value::Null)),
            _ => match &message.id {
                Some(id) => vec![json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32601, "message": format!("unsupported method `{method}`")},
                })],
                None => Vec::new(),
            },
        }
    }

    fn open(&mut self, uri: Option<&str>, text: Option<&str>) -> Vec<Value> {
        let (Some(uri), Some(text)) = (uri, text) else {
            return Vec::new();
        };
        let Some(path) = uri_to_path(uri) else {
            return Vec::new();
        };
        self.docs.insert(path.clone(), text.to_owned());
        let diags = match self.analyze(&path) {
            Some(a) => diagnostics(&a),
            None => Vec::new(),
        };
        vec![publish(uri, diags)]
    }

    /// One edit replacing the whole document, or none when it's formatted or has
    /// syntax errors.
    fn format(&self, params: &Value) -> Option<Value> {
        let path = uri_to_path(params["textDocument"]["uri"].as_str()?)?;
        let src = self.docs.get(&path)?;
        let out = ward_syntax::printer::format(src).ok()?;
        if out == *src {
            return Some(json!([]));
        }
        let end = position(src, src.len() as u32);
        Some(json!([{"range": {"start": {"line": 0, "character": 0}, "end": end}, "newText": out}]))
    }

    fn analyze(&self, path: &Path) -> Option<Analysis> {
        ward_check::analyze(path, &Overlay(&self.docs)).ok()
    }

    /// Runs `f` on the analysis of the document at the request's position.
    fn at(&self, params: &Value, f: fn(&Analysis, u32) -> Option<Value>) -> Option<Value> {
        let path = uri_to_path(params["textDocument"]["uri"].as_str()?)?;
        let analysis = self.analyze(&path)?;
        let src = &analysis.program.module(ModuleId(0)).src;
        let pos = &params["position"];
        let offset = offset_of(src, pos["line"].as_u64()?, pos["character"].as_u64()?);
        f(&analysis, offset)
    }
}

fn publish(uri: &str, diagnostics: Vec<Value>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {"uri": uri, "diagnostics": diagnostics},
    })
}

/// LSP positions count UTF-16 code units from the start of the line.
fn position(src: &str, offset: u32) -> Value {
    let offset = (offset as usize).min(src.len());
    let before = src.get(..offset).unwrap_or(src);
    let line = before.matches('\n').count();
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    let character: usize = before
        .get(start..)
        .unwrap_or("")
        .chars()
        .map(char::len_utf16)
        .sum();
    json!({"line": line, "character": character})
}

fn range(src: &str, span: Span) -> Value {
    json!({"start": position(src, span.start), "end": position(src, span.end)})
}

fn offset_of(src: &str, line: u64, character: u64) -> u32 {
    let mut offset = 0;
    for (i, l) in src.split_inclusive('\n').enumerate() {
        if i as u64 == line {
            let mut units = 0u64;
            for (b, c) in l.char_indices() {
                if units >= character || c == '\n' {
                    return (offset + b) as u32;
                }
                units += c.len_utf16() as u64;
            }
            return (offset + l.len()) as u32;
        }
        offset += l.len();
    }
    src.len() as u32
}

/// The entry file's diagnostics, as LSP diagnostics.
fn diagnostics(analysis: &Analysis) -> Vec<Value> {
    let program = &analysis.program;
    let entry = program.module(ModuleId(0));
    let uri = path_to_uri(Path::new(&entry.path));
    analysis
        .diagnostics()
        .iter()
        .filter(|d| d.module == ModuleId(0))
        .map(|d| {
            let d = &d.diagnostic;
            let mut message = d.message.clone();
            if let Some(help) = &d.help {
                message.push_str(&format!("\nhelp: {help}"));
            }
            for note in &d.notes {
                message.push_str(&format!("\nnote: {note}"));
            }
            let related: Vec<Value> = d
                .labels
                .iter()
                .skip(1)
                .filter_map(|l| {
                    Some(json!({
                        "location": {"uri": uri, "range": range(&entry.src, l.span)},
                        "message": l.message.clone()?,
                    }))
                })
                .collect();
            json!({
                "range": range(&entry.src, d.span()),
                "severity": if d.severity == Severity::Error { 1 } else { 2 },
                "code": d.code.0,
                "source": "ward",
                "message": message,
                "relatedInformation": related,
            })
        })
        .collect()
}

/// The innermost expression of the entry module around `offset`.
fn expr_at(analysis: &Analysis, offset: u32) -> Option<ward_syntax::ast::ExprId> {
    let ast = &analysis.program.module(ModuleId(0)).ast;
    ast.exprs
        .iter()
        .filter(|(_, e)| e.span.start <= offset && offset <= e.span.end)
        .min_by_key(|(_, e)| e.span.end - e.span.start)
        .map(|(id, _)| id)
}

fn hover(analysis: &Analysis, offset: u32) -> Option<Value> {
    let program = &analysis.program;
    let module = program.module(ModuleId(0));
    let e = expr_at(analysis, offset)?;
    let res = analysis.resolution.modules.first()?;
    let text = match res.values.get(e) {
        Some(ValueRes::Fn(d)) => signature(analysis, *d)?,
        _ => {
            let ty = analysis.checked.types.first()?.exprs.get(e)?;
            let source = module
                .src
                .get(module.ast.exprs[e].span.range())
                .unwrap_or("");
            let source = if source.len() > 60 { "…" } else { source };
            format!("{source}: {}", ty.display(program, &[]))
        }
    };
    Some(json!({
        "contents": {"kind": "markdown", "value": format!("```ward\n{text}\n```")},
        "range": range(&module.src, module.ast.exprs[e].span),
    }))
}

fn signature(analysis: &Analysis, d: ward_resolve::DefId) -> Option<String> {
    let program = &analysis.program;
    let Item::Fn(f) = program.item(d) else {
        return None;
    };
    let sig = analysis.checked.fns.get(&d)?;
    let generics: Vec<String> = f.generics.iter().map(|g| g.name.clone()).collect();
    // A method's `self` is implicit.
    let skip = usize::from(f.method.is_some());
    let params: Vec<String> = f
        .params
        .iter()
        .zip(&sig.params)
        .skip(skip)
        .map(|(p, t)| format!("{}: {}", p.name.name, t.display(program, &generics)))
        .collect();
    let mut out = format!(
        "{}fn {}({})",
        if f.is_ai { "ai " } else { "" },
        f.name.name,
        params.join(", ")
    );
    if sig.ret != ward_check::Ty::Unit {
        out.push_str(&format!(" -> {}", sig.ret.display(program, &generics)));
    }
    if let Some(t) = &sig.throws {
        out.push_str(&format!(" throws {}", t.display(program, &generics)));
    }
    Some(out)
}

fn item_name_span(item: &Item) -> Option<Span> {
    Some(match item {
        Item::Fn(f) => f.name.span,
        Item::Record(r) => r.name.span,
        Item::Alias(a) => a.name.span,
        Item::Enum(e) => e.name.span,
        Item::Class(c) => c.name.span,
        Item::Import(i) => i.alias.as_ref().map_or(i.span, |a| a.span),
        Item::Test(t) => t.name_span,
    })
}

fn location(analysis: &Analysis, module: ModuleId, span: Span) -> Value {
    let m = analysis.program.module(module);
    json!({"uri": path_to_uri(Path::new(&m.path)), "range": range(&m.src, span)})
}

fn definition(analysis: &Analysis, offset: u32) -> Option<Value> {
    let program = &analysis.program;
    let res = analysis.resolution.modules.first()?;
    let ast = &program.module(ModuleId(0)).ast;
    // A type name under the cursor.
    let ty = ast
        .types
        .iter()
        .filter(|(_, t)| t.span.start <= offset && offset <= t.span.end)
        .min_by_key(|(_, t)| t.span.end - t.span.start)
        .map(|(id, _)| id);
    if let Some(TypeRes::Def(d)) = ty.and_then(|t| res.types.get(t)) {
        let in_refinement = expr_at(analysis, offset).is_some_and(|e| {
            ty.is_some_and(|t| {
                ast.types[t].refinement.is_some_and(|c| {
                    let s = ast.exprs[c].span;
                    s.start <= ast.exprs[e].span.start && ast.exprs[e].span.end <= s.end
                })
            })
        });
        if !in_refinement {
            return Some(location(
                analysis,
                d.module,
                item_name_span(program.item(*d))?,
            ));
        }
    }
    let e = expr_at(analysis, offset)?;
    // The method named in `obj.name(...)`.
    let types = analysis.checked.types.first();
    let method = ast.exprs.iter().find_map(|(call, x)| match &x.kind {
        ward_syntax::ast::ExprKind::Call { callee, .. } if *callee == e => {
            types.and_then(|t| t.methods.get(call)).map(|m| m.target)
        }
        _ => None,
    });
    if let Some(d) = method {
        return Some(location(
            analysis,
            d.module,
            item_name_span(program.item(d))?,
        ));
    }
    let target = match res.values.get(e)? {
        ValueRes::Local(l) => (ModuleId(0), res.locals[*l].span),
        ValueRes::Fn(d)
        | ValueRes::Enum(d)
        | ValueRes::Tool(d)
        | ValueRes::ToolMember(d)
        | ValueRes::Class(d)
        | ValueRes::Super(d) => (d.module, item_name_span(program.item(*d))?),
        ValueRes::Variant(d, i) => match program.item(*d) {
            Item::Enum(en) => (d.module, en.variants.get(*i)?.name.span),
            _ => return None,
        },
        ValueRes::Module(m) => (*m, Span::default()),
        ValueRes::Builtin(_) => return None,
    };
    Some(location(analysis, target.0, target.1))
}
