//! Wardscript runtime core. For now the runtime is the pure-Python package in `py/`;
//! it moves to Rust behind the same Python API in M6.

/// The `wardscript` Python package's files, as `(path, contents)`, so `ward run` can
/// work without the package being installed.
pub const PYTHON_PACKAGE: &[(&str, &str)] = &[
    (
        "wardscript/__init__.py",
        include_str!("../py/wardscript/__init__.py"),
    ),
    ("wardscript/_rt.py", include_str!("../py/wardscript/_rt.py")),
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
        "wardscript/values.py",
        include_str!("../py/wardscript/values.py"),
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_every_package_file() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("py/wardscript");
        let mut on_disk: Vec<String> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| format!("wardscript/{}", e.file_name().to_string_lossy()))
            .filter(|p| !p.ends_with("__pycache__"))
            .collect();
        on_disk.sort();
        let embedded: Vec<&str> = PYTHON_PACKAGE.iter().map(|(p, _)| *p).collect();
        assert_eq!(on_disk, embedded);
    }
}
