//! The TypeScript backend, end to end: builds each `tests/e2e/ts/*.test.ts` driver's
//! program (named on its `// ward: <file>` line) with `--target typescript`, type-checks
//! it with the runtime's `tsc` when it's installed (`npm ci` in
//! `crates/ward_runtime/ts`), and runs the driver with Node's test runner.
//!
//! Needs Node 22.6+ (for type stripping); without it the test is skipped, unless
//! `WARD_REQUIRE_NODE` is set.

#![allow(clippy::expect_used, clippy::panic)]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

fn node() -> Command {
    Command::new(std::env::var_os("WARD_NODE").unwrap_or_else(|| "node".into()))
}

/// Whether `node` can run TypeScript directly.
fn node_ok() -> bool {
    let Ok(out) = node().arg("--version").output() else {
        return false;
    };
    let v = String::from_utf8_lossy(&out.stdout);
    let mut parts = v.trim().trim_start_matches('v').split('.');
    let major: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let minor: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    major > 22 || (major == 22 && minor >= 6)
}

fn runtime_ts() -> PathBuf {
    common::repo_root().join("crates/ward_runtime/ts")
}

fn drivers() -> Vec<(PathBuf, String)> {
    let dir = common::repo_root().join("tests/e2e/ts");
    let mut out: Vec<(PathBuf, String)> = std::fs::read_dir(&dir)
        .expect("read tests/e2e/ts")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.to_string_lossy().ends_with(".test.ts"))
        .map(|p| {
            let text = std::fs::read_to_string(&p).expect("read driver");
            let src = text
                .lines()
                .next()
                .and_then(|l| l.strip_prefix("// ward: "))
                .unwrap_or_else(|| panic!("{} has no `// ward: <file>` line", p.display()))
                .trim()
                .to_owned();
            (p, src)
        })
        .collect();
    out.sort();
    out
}

fn link_runtime(dir: &Path) {
    let modules = dir.join("node_modules");
    std::fs::create_dir_all(&modules).expect("mkdir node_modules");
    let link = modules.join("wardscript");
    let _ = std::fs::remove_file(&link);
    #[cfg(unix)]
    std::os::unix::fs::symlink(runtime_ts(), &link).expect("link the runtime");
    #[cfg(not(unix))]
    panic!("the TypeScript e2e tests link the runtime with a symlink");
}

#[test]
fn e2e_typescript() {
    if !node_ok() {
        assert!(
            std::env::var_os("WARD_REQUIRE_NODE").is_none(),
            "WARD_REQUIRE_NODE is set, but `node` isn't Node 22.6+"
        );
        eprintln!("skipping: needs Node 22.6+ (set WARD_NODE)");
        return;
    }
    // The runtime's own tests first.
    let unit = node()
        .args(["--experimental-strip-types", "--no-warnings", "--test"])
        .arg(runtime_ts().join("test/runtime.test.ts"))
        .env(
            "WARD_PYTHON",
            std::env::var_os("WARD_PYTHON").unwrap_or_else(|| "python3".into()),
        )
        .output()
        .expect("run node");
    assert!(
        unit.status.success(),
        "runtime tests failed\n{}",
        common::render(&unit)
    );

    let tsc = runtime_ts().join("node_modules/.bin/tsc");
    for (driver, src) in drivers() {
        let name = driver
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.trim_end_matches(".test.ts").to_owned())
            .expect("driver name");
        let out = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("ts")
            .join(&name);
        let _ = std::fs::remove_dir_all(&out);
        let o = out.to_str().expect("utf-8 path");
        let built = common::ward(&["build", &src, "--target", "typescript", "-o", o]);
        assert_eq!(built.status.code(), Some(0), "{}", common::render(&built));
        link_runtime(&out);
        std::fs::write(out.join("package.json"), "{\"type\": \"module\"}\n").expect("package.json");
        let test_file = out.join(driver.file_name().expect("file name"));
        std::fs::copy(&driver, &test_file).expect("copy driver");

        if tsc.is_file() {
            let tsconfig = serde_json::json!({
                "compilerOptions": {
                    "target": "ES2022",
                    "module": "NodeNext",
                    "moduleResolution": "NodeNext",
                    "customConditions": ["ward-source"],
                    "strict": true,
                    "noEmit": true,
                    "allowImportingTsExtensions": true,
                    "verbatimModuleSyntax": true,
                    "skipLibCheck": true,
                    "types": ["node"],
                    "typeRoots": [runtime_ts().join("node_modules/@types")],
                },
                "include": ["*.ts", "**/*.ts"],
                "exclude": ["node_modules"],
            });
            std::fs::write(out.join("tsconfig.json"), tsconfig.to_string()).expect("tsconfig");
            let checked = node()
                .arg(&tsc)
                .args(["-p", "."])
                .current_dir(&out)
                .output()
                .expect("run tsc");
            assert!(
                checked.status.success(),
                "{name}: the generated TypeScript doesn't type-check\n{}",
                common::render(&checked)
            );
        }

        let run = node()
            .args([
                "--experimental-strip-types",
                "--no-warnings",
                "--conditions=ward-source",
                "--test",
            ])
            .arg(&test_file)
            .current_dir(&out)
            .output()
            .expect("run node");
        assert!(
            run.status.success(),
            "{name} failed\n{}",
            common::render(&run)
        );
    }
}
