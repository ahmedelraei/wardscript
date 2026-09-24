use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use ward_syntax::Diagnostic;
use ward_syntax::ast::{ImportKind, Item};
use ward_syntax::diag::codes;

use crate::{ModuleData, ModuleId, Program, ProgramDiagnostic};

/// Source access, so tests can load programs from memory.
pub trait FileSystem {
    fn read(&self, path: &Path) -> io::Result<String>;
}

pub struct RealFs;

impl FileSystem for RealFs {
    fn read(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }
}

#[derive(Debug)]
pub struct LoadError {
    pub path: PathBuf,
    pub error: io::Error,
}

/// Source file extensions, in lookup order: `.ward` is canonical.
const EXTENSIONS: [&str; 2] = ["ward", "wardscript"];

/// Parses `entry` and, transitively, every module it imports. `import a.b` refers to
/// `a/b.ward` (or, failing that, `a/b.wardscript`) relative to the entry file's directory.
pub fn load(
    entry: &Path,
    fs: &dyn FileSystem,
) -> Result<(Program, Vec<ProgramDiagnostic>), LoadError> {
    let src = fs.read(entry).map_err(|error| LoadError {
        path: entry.to_owned(),
        error,
    })?;
    let root = entry.parent().unwrap_or(Path::new("")).to_owned();

    let mut modules = Vec::new();
    let mut diags = Vec::new();
    let mut by_path: HashMap<PathBuf, ModuleId> = HashMap::new();
    by_path.insert(entry.to_owned(), ModuleId(0));
    let entry_name = entry
        .file_stem()
        .map_or(String::new(), |s| s.to_string_lossy().into_owned());
    add_module(&mut modules, &mut diags, entry, entry_name, src);

    let mut next = 0;
    while next < modules.len() {
        let id = ModuleId(next as u32);
        let wanted: Vec<(usize, Vec<String>, ward_syntax::Span)> = modules[next]
            .ast
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| match item {
                Item::Import(imp) => match &imp.kind {
                    ImportKind::Module(path) => Some((
                        i,
                        path.segments.iter().map(|s| s.name.clone()).collect(),
                        path.span,
                    )),
                    ImportKind::Tool { .. } => None,
                },
                _ => None,
            })
            .collect();

        for (item, segments, span) in wanted {
            let mut base = root.clone();
            base.extend(&segments);
            let candidates = EXTENSIONS.map(|ext| base.with_extension(ext));
            let known = candidates.iter().find_map(|f| by_path.get(f).copied());
            let target = match known {
                Some(target) => target,
                None => match candidates
                    .iter()
                    .find_map(|f| fs.read(f).ok().map(|src| (f, src)))
                {
                    Some((file, src)) => {
                        let target = ModuleId(modules.len() as u32);
                        by_path.insert(file.clone(), target);
                        add_module(&mut modules, &mut diags, file, segments.join("."), src);
                        target
                    }
                    None => {
                        let looked: Vec<String> = candidates
                            .iter()
                            .map(|f| format!("`{}`", f.display()))
                            .collect();
                        diags.push(ProgramDiagnostic {
                            module: id,
                            diagnostic: Diagnostic::error(
                                codes::UNRESOLVED_MODULE,
                                format!("cannot find module `{}`", segments.join(".")),
                                span,
                            )
                            .with_label("no such module")
                            .with_help(format!("looked for {}", looked.join(" and "))),
                        });
                        continue;
                    }
                },
            };
            modules[next].imports.insert(item, target);
        }
        next += 1;
    }
    Ok((Program { modules }, diags))
}

fn add_module(
    modules: &mut Vec<ModuleData>,
    diags: &mut Vec<ProgramDiagnostic>,
    path: &Path,
    name: String,
    src: String,
) {
    let id = ModuleId(modules.len() as u32);
    let parse = ward_syntax::parse(&src);
    diags.extend(
        parse
            .diagnostics
            .into_iter()
            .map(|diagnostic| ProgramDiagnostic {
                module: id,
                diagnostic,
            }),
    );
    modules.push(ModuleData {
        name,
        path: path.display().to_string(),
        src,
        ast: parse.module,
        imports: HashMap::new(),
    });
}
