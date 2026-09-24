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
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext == "ward" || ext == "wardscript")
        })
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

/// `tests/ui/*.{ward,wardscript}` files, plus `tests/ui/<case>/main.{ward,wardscript}`
/// multi-module cases.
pub fn ui_cases() -> Vec<(String, PathBuf)> {
    let dir = repo_root().join("tests/ui");
    let mut cases: Vec<(String, PathBuf)> = std::fs::read_dir(&dir)
        .expect("read tests/ui")
        .map(|e| e.expect("dir entry").path())
        .filter_map(|p| {
            let main = ["main.ward", "main.wardscript"]
                .map(|m| p.join(m))
                .into_iter()
                .find(|m| m.is_file())?;
            let name = p.file_name().and_then(|n| n.to_str()).expect("utf-8 name");
            Some((name.to_owned(), main))
        })
        .collect();
    for f in wardscript_files("tests/ui") {
        let name = f.file_stem().and_then(|s| s.to_str()).expect("utf-8 name");
        cases.push((name.to_owned(), f));
    }
    cases.sort();
    cases
}

/// The interpreter e2e tests run: `WARD_PYTHON`, or `python3`.
pub fn python() -> Command {
    Command::new(std::env::var_os("WARD_PYTHON").unwrap_or_else(|| "python3".into()))
}

pub fn runtime_py() -> PathBuf {
    repo_root().join("crates/ward_runtime/py")
}

/// Runs `ward build src -o <tmp>/<name>` and returns the output directory.
pub fn build(name: &str, src: &str) -> PathBuf {
    build_with(name, src, &[])
}

/// `build`, with more `ward build` flags.
pub fn build_with(name: &str, src: &str, flags: &[&str]) -> PathBuf {
    let out = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("build")
        .join(name);
    let _ = std::fs::remove_dir_all(&out);
    let mut args = vec!["build", src, "-o", out.to_str().expect("utf-8 path")];
    args.extend_from_slice(flags);
    let result = ward(&args);
    assert_eq!(result.status.code(), Some(0), "{}", render(&result));
    out
}

/// Every file under `dir`, relative to it, sorted.
pub fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_owned()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).expect("read dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path.strip_prefix(dir).expect("under dir").to_owned());
            }
        }
    }
    out.sort();
    out
}
