#![allow(clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use ward_lsp::{Server, path_to_uri, serve};

fn dir() -> std::path::PathBuf {
    let d = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("lsp");
    std::fs::create_dir_all(&d).expect("mkdir");
    d
}

fn frame(v: &Value) -> String {
    let body = v.to_string();
    format!("Content-Length: {}\r\n\r\n{body}", body.len())
}

fn messages(out: &[u8]) -> Vec<Value> {
    let text = String::from_utf8_lossy(out);
    text.split("Content-Length: ")
        .filter(|s| !s.is_empty())
        .map(|s| {
            let (_, body) = s.split_once("\r\n\r\n").expect("framed");
            serde_json::from_str(body).expect("json")
        })
        .collect()
}

const SRC: &str = "fn double(n: Int) -> Int {\n    n * 2\n}\n\npub fn main() -> String {\n    let x = double(21)\n    x\n}\n";

fn session(src: &str, requests: &[Value]) -> Vec<Value> {
    let path = dir().join("main.ward");
    let uri = path_to_uri(&path);
    let mut input = String::new();
    input.push_str(&frame(
        &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
    ));
    input.push_str(&frame(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {"textDocument": {"uri": uri, "languageId": "ward", "version": 1, "text": src}},
    })));
    for r in requests {
        let mut r = r.clone();
        r["params"]["textDocument"] = json!({"uri": uri});
        input.push_str(&frame(&r));
    }
    input.push_str(&frame(
        &json!({"jsonrpc": "2.0", "id": 99, "method": "shutdown"}),
    ));
    input.push_str(&frame(&json!({"jsonrpc": "2.0", "method": "exit"})));
    let mut out = Vec::new();
    let clean = serve(input.as_bytes(), &mut out).expect("serve");
    assert!(clean);
    messages(&out)
}

#[test]
fn diagnostics_as_you_type() {
    let msgs = session(SRC, &[]);
    assert_eq!(msgs[0]["result"]["capabilities"]["hoverProvider"], true);
    let diags = &msgs[1]["params"]["diagnostics"];
    assert_eq!(diags.as_array().map(Vec::len), Some(1), "{diags}");
    assert_eq!(diags[0]["code"], "W0110");
    assert_eq!(
        diags[0]["range"]["start"],
        json!({"line": 6, "character": 4})
    );
    assert_eq!(diags[0]["severity"], 1);
}

#[test]
fn hover_and_definition() {
    let src = SRC.replace("-> String", "-> Int");
    let msgs = session(
        &src,
        &[
            json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/hover", "params": {"position": {"line": 6, "character": 4}}}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "textDocument/hover", "params": {"position": {"line": 5, "character": 13}}}),
            json!({"jsonrpc": "2.0", "id": 4, "method": "textDocument/definition", "params": {"position": {"line": 5, "character": 13}}}),
            json!({"jsonrpc": "2.0", "id": 5, "method": "textDocument/references", "params": {}}),
            json!({"jsonrpc": "2.0", "id": 6, "method": "textDocument/formatting", "params": {}}),
        ],
    );
    assert_eq!(msgs[1]["params"]["diagnostics"], json!([]));
    let hover = |id: u64| {
        msgs.iter()
            .find(|m| m["id"] == id)
            .map(|m| {
                m["result"]["contents"]["value"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned()
            })
            .unwrap_or_default()
    };
    assert!(hover(2).contains("x: Int"), "{}", hover(2));
    assert!(
        hover(3).contains("fn double(n: Int) -> Int"),
        "{}",
        hover(3)
    );
    let def = msgs.iter().find(|m| m["id"] == 4).expect("definition");
    assert_eq!(
        def["result"]["range"]["start"],
        json!({"line": 0, "character": 3})
    );
    let unsupported = msgs.iter().find(|m| m["id"] == 5).expect("references");
    assert_eq!(unsupported["error"]["code"], -32601);
    // The source is formatted already: no edits.
    let formatting = msgs.iter().find(|m| m["id"] == 6).expect("formatting");
    assert_eq!(formatting["result"], json!([]));
}

#[test]
fn close_clears_diagnostics() {
    let mut s = Server::new();
    let uri = path_to_uri(&dir().join("closed.ward"));
    let out = s.handle(ward_lsp::Message {
        id: None,
        method: Some("textDocument/didClose".into()),
        params: Some(json!({"textDocument": {"uri": uri}})),
    });
    assert_eq!(out[0]["params"]["diagnostics"], json!([]));
}
