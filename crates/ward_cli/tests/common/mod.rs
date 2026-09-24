// Each test binary uses a different subset of these helpers.
#![allow(dead_code)]
#![allow(clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub const EXIT_DIAGNOSTICS: i32 = 1;

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn ward(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ward"))
        .args(args)
        .current_dir(repo_root())
        .output()
        .expect("failed to spawn `ward`")
}

/// Sorted so snapshot order and failure output are deterministic.
pub fn wardscript_files(dir: &str) -> Vec<PathBuf> {
    let dir = repo_root().join(dir);
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|entry| entry.expect("bad dir entry").path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "wardscript"))
        .collect();
    files.sort();
    files
}

pub fn render(output: &Output) -> String {
    format!(
        "exit: {:?}\n--- stdout ---\n{}--- stderr ---\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

/// `tests/ui/*.wardscript` files, plus `tests/ui/<case>/main.wardscript` multi-module cases.
pub fn ui_cases() -> Vec<(String, PathBuf)> {
    let dir = repo_root().join("tests/ui");
    let mut cases: Vec<(String, PathBuf)> = std::fs::read_dir(&dir)
        .expect("read tests/ui")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.join("main.wardscript").is_file())
        .map(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).expect("utf-8 name");
            (name.to_owned(), p.join("main.wardscript"))
        })
        .collect();
    for f in wardscript_files("tests/ui") {
        let name = f.file_stem().and_then(|s| s.to_str()).expect("utf-8 name");
        cases.push((name.to_owned(), f));
    }
    cases.sort();
    cases
}
