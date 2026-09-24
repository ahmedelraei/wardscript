//! Wardscript runtime core: budget counters and the audit trace, used by the Python
//! binding (`ward_runtime_py`) and by `ward trace`. The rest of the runtime is the
//! Python package in `py/`, which falls back to a pure-Python core without the binding.

pub mod budget;
pub mod otlp;
pub mod show;
pub mod trace;

/// The `wardscript` Python package's files, as `(path, contents)`, so `ward run` can
/// work without the package being installed.
pub const PYTHON_PACKAGE: &[(&str, &str)] = &[
    (
        "wardscript/__init__.py",
        include_str!("../py/wardscript/__init__.py"),
    ),
    (
        "wardscript/_core_py.py",
        include_str!("../py/wardscript/_core_py.py"),
    ),
    ("wardscript/_rt.py", include_str!("../py/wardscript/_rt.py")),
    (
        "wardscript/audit.py",
        include_str!("../py/wardscript/audit.py"),
    ),
    (
        "wardscript/budget.py",
        include_str!("../py/wardscript/budget.py"),
    ),
    (
        "wardscript/core.py",
        include_str!("../py/wardscript/core.py"),
    ),
    (
        "wardscript/errors.py",
        include_str!("../py/wardscript/errors.py"),
    ),
    (
        "wardscript/mock.py",
        include_str!("../py/wardscript/mock.py"),
    ),
    (
        "wardscript/model.py",
        include_str!("../py/wardscript/model.py"),
    ),
    (
        "wardscript/providers/__init__.py",
        include_str!("../py/wardscript/providers/__init__.py"),
    ),
    (
        "wardscript/providers/anthropic.py",
        include_str!("../py/wardscript/providers/anthropic.py"),
    ),
    (
        "wardscript/providers/openai.py",
        include_str!("../py/wardscript/providers/openai.py"),
    ),
    (
        "wardscript/py.typed",
        include_str!("../py/wardscript/py.typed"),
    ),
    (
        "wardscript/runtime.py",
        include_str!("../py/wardscript/runtime.py"),
    ),
    (
        "wardscript/schema.py",
        include_str!("../py/wardscript/schema.py"),
    ),
    (
        "wardscript/trust.py",
        include_str!("../py/wardscript/trust.py"),
    ),
    (
        "wardscript/values.py",
        include_str!("../py/wardscript/values.py"),
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_every_package_file() {
        fn walk(dir: &std::path::Path, prefix: &str, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let path = entry.path();
                if path.is_dir() {
                    if name != "__pycache__" {
                        walk(&path, &format!("{prefix}{name}/"), out);
                    }
                } else if !name.ends_with(".so") {
                    out.push(format!("{prefix}{name}"));
                }
            }
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("py/wardscript");
        let mut on_disk = Vec::new();
        walk(&dir, "wardscript/", &mut on_disk);
        on_disk.sort();
        let embedded: Vec<&str> = PYTHON_PACKAGE.iter().map(|(p, _)| *p).collect();
        assert_eq!(on_disk, embedded);
    }
}
