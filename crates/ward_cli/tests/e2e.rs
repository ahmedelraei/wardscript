//! Builds each `tests/e2e/test_*.py` driver's program (named on its `# ward: <file>`
//! first line, optionally followed by `ward build` flags) to Python, then runs the driver against the generated modules and the
//! `wardscript` runtime, with the deterministic mock model.

#![allow(clippy::expect_used, clippy::panic)]

mod common;

use std::path::PathBuf;

fn drivers() -> Vec<(String, PathBuf, String)> {
    let dir = common::repo_root().join("tests/e2e");
    let mut out: Vec<_> = std::fs::read_dir(&dir)
        .expect("read tests/e2e")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "py")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("test_"))
        })
        .map(|p| {
            let text = std::fs::read_to_string(&p).expect("read driver");
            let src = text
                .lines()
                .next()
                .and_then(|l| l.strip_prefix("# ward: "))
                .unwrap_or_else(|| panic!("{} must start with `# ward: <file>`", p.display()))
                .trim()
                .to_owned();
            let name = p
                .file_stem()
                .and_then(|s| s.to_str())
                .expect("name")
                .to_owned();
            (name, p, src)
        })
        .collect();
    out.sort();
    out
}

#[test]
fn e2e() {
    let drivers = drivers();
    assert!(!drivers.is_empty(), "no e2e drivers found");
    for (name, driver, src) in drivers {
        let mut words = src.split_whitespace();
        let file = words.next().unwrap_or_default();
        let flags: Vec<&str> = words.collect();
        let out = common::build_with(&name, file, &flags);

        // Stubs aren't imported by the drivers; make sure they at least parse.
        let stubs: Vec<PathBuf> = common::files_under(&out)
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e == "pyi"))
            .map(|p| out.join(p))
            .collect();
        let parsed = common::python()
            .arg("-c")
            .arg("import ast, sys\nfor p in sys.argv[1:]: ast.parse(open(p).read(), p)")
            .args(&stubs)
            .output()
            .expect("run python (set WARD_PYTHON to a Python 3.10+ interpreter)");
        assert!(
            parsed.status.success(),
            "{name}: stubs don't parse\n{}",
            common::render(&parsed)
        );

        // With WARD_RUNTIME_INSTALLED, the runtime comes from an installed wheel instead.
        let mut paths = vec![out.clone()];
        if std::env::var_os("WARD_RUNTIME_INSTALLED").is_none() {
            paths.push(common::runtime_py());
        }
        let path = std::env::join_paths(paths).expect("PYTHONPATH");
        let run = common::python()
            .arg(&driver)
            .env("PYTHONPATH", path)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .current_dir(&out)
            .output()
            .expect("run python");
        assert!(
            run.status.success(),
            "{name} failed\n{}",
            common::render(&run)
        );
    }
}
