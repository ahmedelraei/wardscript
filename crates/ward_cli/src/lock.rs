//! `ward lock`: asks each MCP server in `mcp.json` for its tools (`tools/list`) and pins
//! their schemas in `ward.lock`, which `ward check` reads. `--check` fails instead of
//! writing when the lock is out of date.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::Duration;

use serde_json::json;
use ward_resolve::json::Json;
use ward_resolve::tools::{LOCK_FILE, LOCK_VERSION};

const PROTOCOL_VERSION: &str = "2025-06-18";

#[derive(Debug)]
pub struct LockError(pub String);

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn err<T>(message: impl Into<String>) -> Result<T, LockError> {
    Err(LockError(message.into()))
}

/// A stdio server from `mcp.json`.
pub struct ServerSpec {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// The stdio servers of an `mcp.json`, sorted by name. Servers reached over HTTP are
/// skipped with a note on stderr.
pub fn read_config(path: &Path) -> Result<Vec<ServerSpec>, LockError> {
    let text = std::fs::read_to_string(path)
        .or_else(|e| err(format!("cannot read `{}`: {e}", path.display())))?;
    let config: serde_json::Value = serde_json::from_str(&text)
        .or_else(|e| err(format!("`{}` is not valid JSON: {e}", path.display())))?;
    let Some(servers) = config.get("mcpServers").and_then(|s| s.as_object()) else {
        return err(format!("`{}` has no `mcpServers` object", path.display()));
    };
    let mut out = Vec::new();
    for (name, spec) in servers {
        let Some(command) = spec.get("command").and_then(|c| c.as_str()) else {
            eprintln!(
                "note: skipping `{name}`: only servers with a `command` (stdio) are supported"
            );
            continue;
        };
        let strings = |v: Option<&serde_json::Value>| -> Vec<String> {
            v.and_then(|a| a.as_array())
                .into_iter()
                .flatten()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect()
        };
        let env = spec
            .get("env")
            .and_then(|e| e.as_object())
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned())))
            .collect();
        out.push(ServerSpec {
            name: name.clone(),
            command: command.to_owned(),
            args: strings(spec.get("args")),
            env,
        });
    }
    Ok(out)
}

struct Client {
    name: String,
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Option<String>>,
    next_id: u64,
    timeout: Duration,
}

impl Client {
    fn start(spec: &ServerSpec, dir: &Path, timeout: Duration) -> Result<Client, LockError> {
        let mut child = Command::new(&spec.command)
            .args(&spec.args)
            .envs(spec.env.iter().map(|(k, v)| (k, v)))
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .or_else(|e| err(format!("server `{}` didn't start: {e}", spec.name)))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return err(format!("server `{}` has no stdio", spec.name));
        };
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(Some(line)).is_err() {
                    return;
                }
            }
            let _ = tx.send(None);
        });
        let mut c = Client {
            name: spec.name.clone(),
            child,
            stdin,
            lines,
            next_id: 0,
            timeout,
        };
        c.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "ward", "version": env!("CARGO_PKG_VERSION")},
            }),
        )?;
        c.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))?;
        Ok(c)
    }

    fn send(&mut self, message: &serde_json::Value) -> Result<(), LockError> {
        writeln!(self.stdin, "{message}")
            .and_then(|()| self.stdin.flush())
            .or_else(|e| err(format!("server `{}` stopped: {e}", self.name)))
    }

    fn request(&mut self, method: &str, params: serde_json::Value) -> Result<Json, LockError> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        loop {
            let line = match self.lines.recv_timeout(self.timeout) {
                Ok(Some(line)) => line,
                Ok(None) | Err(RecvTimeoutError::Disconnected) => {
                    return err(format!("server `{}` exited during `{method}`", self.name));
                }
                Err(RecvTimeoutError::Timeout) => {
                    return err(format!(
                        "server `{}` didn't answer `{method}` within {}s",
                        self.name,
                        self.timeout.as_secs()
                    ));
                }
            };
            let Ok(message) = Json::parse(&line) else {
                continue;
            };
            if let Some(m) = message.get("method").and_then(Json::as_str) {
                if let Some(req) = message.get("id") {
                    let reply = if m == "ping" {
                        json!({"jsonrpc": "2.0", "id": req, "result": {}})
                    } else {
                        json!({"jsonrpc": "2.0", "id": req, "error": {"code": -32601, "message": "not supported"}})
                    };
                    self.send(&reply)?;
                }
                continue;
            }
            match message.get("id") {
                Some(Json::Number(n)) if n.as_u64() == Some(id) => {}
                _ => continue,
            }
            if let Some(e) = message.get("error") {
                let text = e.get("message").and_then(Json::as_str).map_or_else(
                    || serde_json::to_string(e).unwrap_or_default(),
                    str::to_owned,
                );
                return err(format!("server `{}`: `{method}` failed: {text}", self.name));
            }
            return Ok(message.get("result").cloned().unwrap_or(Json::Null));
        }
    }

    fn list_tools(&mut self) -> Result<Vec<Json>, LockError> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let params = match &cursor {
                Some(c) => json!({"cursor": c}),
                None => json!({}),
            };
            let result = self.request("tools/list", params)?;
            tools.extend(
                result
                    .get("tools")
                    .and_then(Json::as_array)
                    .unwrap_or_default()
                    .iter()
                    .cloned(),
            );
            cursor = result
                .get("nextCursor")
                .and_then(Json::as_str)
                .map(str::to_owned);
            if cursor.is_none() {
                return Ok(tools);
            }
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Only what `ward check` and reviewers need; descriptions stay, as documentation.
fn pinned(tool: &Json) -> Json {
    let keep = [
        "name",
        "title",
        "description",
        "inputSchema",
        "outputSchema",
        "annotations",
    ];
    match tool {
        Json::Object(fields) => Json::Object(
            keep.iter()
                .filter_map(|k| fields.iter().find(|(f, _)| f == k).cloned())
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The lock's text for these servers.
pub fn lock_text(servers: &[(String, Vec<Json>)]) -> String {
    let mut sorted: Vec<&(String, Vec<Json>)> = servers.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let servers = Json::Object(
        sorted
            .into_iter()
            .map(|(name, tools)| {
                let mut tools: Vec<Json> = tools.iter().map(pinned).collect();
                tools.sort_by(|a, b| {
                    let n = |t: &Json| {
                        t.get("name")
                            .and_then(Json::as_str)
                            .unwrap_or("")
                            .to_owned()
                    };
                    n(a).cmp(&n(b))
                });
                (
                    name.clone(),
                    Json::Object(vec![("tools".to_owned(), Json::Array(tools))]),
                )
            })
            .collect(),
    );
    let lock = Json::Object(vec![
        (
            "version".to_owned(),
            Json::Number(serde_json::Number::from(LOCK_VERSION)),
        ),
        ("servers".to_owned(), servers),
    ]);
    format!("{}\n", lock.to_pretty())
}

pub struct Outcome {
    pub path: PathBuf,
    /// Whether the lock on disk was already up to date.
    pub unchanged: bool,
    pub servers: Vec<(String, usize)>,
}

/// Locks the servers of `config`, writing `ward.lock` next to it (or only comparing,
/// with `check`).
pub fn lock(config: &Path, check: bool, timeout: Duration) -> Result<Outcome, LockError> {
    let specs = read_config(config)?;
    let dir = config
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut servers = Vec::new();
    for spec in &specs {
        let mut client = Client::start(spec, dir, timeout)?;
        servers.push((spec.name.clone(), client.list_tools()?));
    }
    let text = lock_text(&servers);
    let path = dir.join(LOCK_FILE);
    let unchanged = std::fs::read_to_string(&path).is_ok_and(|old| old == text);
    if !check && !unchanged {
        std::fs::write(&path, &text)
            .or_else(|e| err(format!("cannot write `{}`: {e}", path.display())))?;
    }
    Ok(Outcome {
        path,
        unchanged,
        servers: servers.iter().map(|(n, t)| (n.clone(), t.len())).collect(),
    })
}
