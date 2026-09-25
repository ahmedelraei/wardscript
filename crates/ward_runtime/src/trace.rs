//! The audit trace of a run: one JSON object per line, in order. A run is one call from the
//! host into Wardscript; it records the model and tool calls, and every `validate`,
//! `approve` and `declassify`, with digests that let `ward trace show` link a value that
//! reached a tool back to where it came from.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Values are digested down to this many nodes, so a huge tool result stays cheap.
const MAX_LEAVES: usize = 512;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub run: String,
    pub seq: u64,
    /// Unix time in nanoseconds.
    pub time: u64,
    #[serde(flatten)]
    pub event: Event,
}

/// A node of a value and its digest: `$`, `$.subject`, `$.items[2]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Leaf {
    pub path: String,
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Arg {
    pub name: String,
    pub value: Value,
    /// The host passed it wrapped in `Trusted`.
    pub vouched: bool,
    pub leaves: Vec<Leaf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    /// A Wardscript `throw` reached the host.
    Threw,
    /// A runtime error: a denied approval, an exceeded budget, bad model output.
    Error,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    RunStart {
        function: String,
        args: Vec<Arg>,
    },
    RunEnd {
        status: Status,
        error: Option<String>,
        tokens: f64,
        calls: f64,
        cost: f64,
    },
    AiCall {
        /// Unix time in nanoseconds when the request was sent; `time` is when it ended.
        started: u64,
        function: String,
        /// Counts every request of the call: retries and fallbacks too.
        attempt: u32,
        /// The alias of the model asked (`model {primary: fast}`); `None` for the
        /// default model.
        #[serde(default)]
        model: Option<String>,
        prompt: String,
        answer: Option<String>,
        tokens: f64,
        /// `None` when unknown: a model without prices, or one returning plain text.
        cost: Option<f64>,
        /// Why the answer was rejected, or why the call failed.
        error: Option<String>,
        /// Of the decoded output, when the answer was accepted.
        leaves: Vec<Leaf>,
    },
    ToolCall {
        started: u64,
        tool: String,
        function: String,
        site: String,
        args: Vec<Value>,
        digests: Vec<String>,
        error: Option<String>,
        leaves: Vec<Leaf>,
    },
    /// `leaves` are the checked value's, so a field of it links back to the check.
    Validate {
        rule: String,
        site: String,
        passed: bool,
        leaves: Vec<Leaf>,
    },
    Approve {
        site: String,
        approved: bool,
        leaves: Vec<Leaf>,
    },
    Declassify {
        site: String,
        reason: String,
        leaves: Vec<Leaf>,
    },
    BudgetExceeded {
        function: String,
        resource: String,
        limit: f64,
        used: f64,
    },
    /// A `cost` budget can't be enforced because the model's cost is unknown; `when` is
    /// `before` (a model without prices) or `after` (an answer without a cost).
    BudgetUnenforceable {
        function: String,
        when: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum TraceError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}:{line}: not a trace record: {source}")]
    Parse {
        path: PathBuf,
        line: usize,
        source: serde_json::Error,
    },
    #[error("no trace `{0}` in {1}")]
    NotFound(String, PathBuf),
    #[error("`{0}` matches more than one trace in {1}; give more of the run id")]
    Ambiguous(String, PathBuf),
}

fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

/// Sorts by start time: milliseconds, then bits that differ between processes.
pub fn new_run_id() -> String {
    let nanos = now_nanos();
    let mix = (nanos ^ u64::from(std::process::id()).wrapping_mul(2_654_435_761)) & 0xffff;
    format!("{:011x}{mix:04x}", nanos / 1_000_000)
}

/// FNV-1a 64 of the value's canonical JSON (compact, keys sorted).
pub fn digest(value: &Value) -> String {
    let text = serde_json::to_string(value).unwrap_or_default();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Digests of the value and every part of it, parents first.
pub fn leaves(value: &Value) -> Vec<Leaf> {
    fn walk(v: &Value, path: String, out: &mut Vec<Leaf>) {
        if out.len() >= MAX_LEAVES {
            return;
        }
        out.push(Leaf {
            path: path.clone(),
            digest: digest(v),
        });
        match v {
            Value::Array(items) => {
                for (i, x) in items.iter().enumerate() {
                    walk(x, format!("{path}[{i}]"), out);
                }
            }
            Value::Object(fields) => {
                for (k, x) in fields {
                    walk(x, format!("{path}.{k}"), out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(value, "$".to_owned(), &mut out);
    out
}

/// Records one run's events, and writes them to `<dir>/<run>.jsonl` as they happen.
pub struct Recorder {
    pub run: String,
    seq: u64,
    file: Option<(PathBuf, BufWriter<File>)>,
    pub records: Vec<Record>,
}

impl Recorder {
    pub fn new(dir: Option<&Path>) -> Result<Self, TraceError> {
        let run = new_run_id();
        let file = match dir {
            Some(dir) => {
                let io = |source| TraceError::Io {
                    path: dir.to_owned(),
                    source,
                };
                std::fs::create_dir_all(dir).map_err(io)?;
                let path = dir.join(format!("{run}.jsonl"));
                let f = File::create(&path).map_err(|source| TraceError::Io {
                    path: path.clone(),
                    source,
                })?;
                Some((path, BufWriter::new(f)))
            }
            None => None,
        };
        Ok(Recorder {
            run,
            seq: 0,
            file,
            records: Vec::new(),
        })
    }

    pub fn path(&self) -> Option<&Path> {
        self.file.as_ref().map(|(p, _)| p.as_path())
    }

    pub fn record(&mut self, event: Event) -> Result<Record, TraceError> {
        let record = Record {
            run: self.run.clone(),
            seq: self.seq,
            time: now_nanos(),
            event,
        };
        self.seq += 1;
        if let Some((path, w)) = &mut self.file {
            let line = serde_json::to_string(&record).unwrap_or_default();
            // Flushed per line, so a crash still leaves the trace up to that point.
            writeln!(w, "{line}")
                .and_then(|()| w.flush())
                .map_err(|source| TraceError::Io {
                    path: path.clone(),
                    source,
                })?;
        }
        self.records.push(record.clone());
        Ok(record)
    }
}

pub fn read(path: &Path) -> Result<Vec<Record>, TraceError> {
    let f = File::open(path).map_err(|source| TraceError::Io {
        path: path.to_owned(),
        source,
    })?;
    let mut out = Vec::new();
    for (i, line) in BufReader::new(f).lines().enumerate() {
        let line = line.map_err(|source| TraceError::Io {
            path: path.to_owned(),
            source,
        })?;
        if line.trim().is_empty() {
            continue;
        }
        out.push(
            serde_json::from_str(&line).map_err(|source| TraceError::Parse {
                path: path.to_owned(),
                line: i + 1,
                source,
            })?,
        );
    }
    Ok(out)
}

/// The trace file for `run` (a run id or a unique prefix of one) in `dir`.
pub fn find(dir: &Path, run: &str) -> Result<PathBuf, TraceError> {
    let io = |source| TraceError::Io {
        path: dir.to_owned(),
        source,
    };
    let mut matches: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(io)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "jsonl")
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.starts_with(run))
        })
        .collect();
    matches.sort();
    match matches.len() {
        0 => Err(TraceError::NotFound(run.to_owned(), dir.to_owned())),
        1 => Ok(matches.remove(0)),
        _ if run.is_empty() => matches
            .pop()
            .ok_or_else(|| TraceError::NotFound(run.to_owned(), dir.to_owned())),
        _ => Err(TraceError::Ambiguous(run.to_owned(), dir.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn digests_ignore_key_order_and_cover_parts() {
        let a: Value = serde_json::from_str(r#"{"b": 1, "a": "x"}"#).unwrap_or_default();
        let b: Value = serde_json::from_str(r#"{"a": "x", "b": 1}"#).unwrap_or_default();
        assert_eq!(digest(&a), digest(&b));
        let ls = leaves(&json!({"subject": "hi", "tags": ["a"]}));
        let paths: Vec<&str> = ls.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths, ["$", "$.subject", "$.tags", "$.tags[0]"]);
        assert_eq!(ls[1].digest, digest(&json!("hi")));
        // Pinned: the Python fallback computes the same digests.
        assert_eq!(digest(&json!("hi")), "94e12d83d4ec08a8");
    }

    #[test]
    fn records_round_trip_through_a_file() -> Result<(), TraceError> {
        let dir = std::env::temp_dir().join(format!("ward-trace-test-{}", new_run_id()));
        let mut r = Recorder::new(Some(&dir))?;
        r.record(Event::Validate {
            rule: "short".into(),
            site: "a.ward:1:1".into(),
            passed: true,
            leaves: leaves(&json!("x")),
        })?;
        let back = read(&find(&dir, &r.run[..6])?)?;
        assert_eq!(back, r.records);
        let _ = std::fs::remove_dir_all(dir);
        Ok(())
    }
}
