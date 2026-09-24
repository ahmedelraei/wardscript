//! `file://` URIs and paths.

use std::path::{Path, PathBuf};

pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // `file:///C:/x` on Windows; `file:///home/x` elsewhere.
    let rest = match rest.strip_prefix('/') {
        Some(r) if r.get(1..2) == Some(":") => r,
        _ => rest,
    };
    let mut bytes = Vec::with_capacity(rest.len());
    let raw = rest.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' {
            let hex = std::str::from_utf8(raw.get(i + 1..i + 3)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            bytes.push(raw[i]);
            i += 1;
        }
    }
    Some(PathBuf::from(String::from_utf8(bytes).ok()?))
}

pub fn path_to_uri(path: &Path) -> String {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    let text = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    if !text.starts_with('/') {
        out.push('/');
    }
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' | b':' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let uri = "file:///home/ada/my%20project/main.ward";
        let path = uri_to_path(uri);
        assert_eq!(
            path.as_deref(),
            Some(Path::new("/home/ada/my project/main.ward"))
        );
        assert_eq!(path.map(|p| path_to_uri(&p)).as_deref(), Some(uri));
        assert_eq!(uri_to_path("http://x"), None);
    }
}
