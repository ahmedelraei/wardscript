//! JSON-RPC messages framed with `Content-Length` headers, as LSP sends them.

use std::io::{self, BufRead, Write};

use serde_json::Value;

pub struct Message {
    pub id: Option<Value>,
    pub method: Option<String>,
    pub params: Option<Value>,
}

/// The next message, or `None` at the end of the input.
pub fn read_message(input: &mut impl BufRead) -> io::Result<Option<Message>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            if length.is_some() {
                break;
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse::<usize>().ok();
            }
        }
    }
    let mut body = vec![0; length.unwrap_or(0)];
    input.read_exact(&mut body)?;
    let value: Value =
        serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some(Message {
        id: value.get("id").cloned(),
        method: value
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_owned),
        params: value.get("params").cloned(),
    }))
}

pub fn write_message(output: &mut impl Write, value: &Value) -> io::Result<()> {
    let body = value.to_string();
    write!(output, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    output.flush()
}
