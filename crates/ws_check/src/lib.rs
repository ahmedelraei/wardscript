//! Type checking for Wardscript: bidirectional inference with unification, generics,
//! exhaustive `match` and `?`. Trust labels, effects and budgets join in M4 and M5.

mod exhaust;
mod infer;
mod lower;
mod methods;
pub mod ty;

use std::collections::HashMap;
use std::path::Path;

use la_arena::ArenaMap;
use ws_resolve::{
    DefId, FileSystem, LoadError, LocalId, ModuleId, Program, ProgramDiagnostic, Resolution,
};
use ws_syntax::ast::{ExprId, Item};

pub use ty::Ty;

pub struct FnSig {
    pub generics: usize,
    pub params: Vec<Ty>,
    pub ret: Ty,
}

/// Types of one module's expressions and variables, fully resolved.
#[derive(Default)]
pub struct ModuleTypes {
    pub exprs: ArenaMap<ExprId, Ty>,
    pub locals: ArenaMap<LocalId, Ty>,
}

pub struct Checked {
    pub diagnostics: Vec<ProgramDiagnostic>,
    pub types: Vec<ModuleTypes>,
    pub fns: HashMap<DefId, FnSig>,
}

/// Everything `ward check` does: load, parse, resolve and type-check a program.
pub struct Analysis {
    pub program: Program,
    pub resolution: Resolution,
    pub checked: Checked,
}

impl Analysis {
    /// All diagnostics, ordered by file and position.
    pub fn diagnostics(&self) -> &[ProgramDiagnostic] {
        &self.checked.diagnostics
    }
}

pub fn analyze(entry: &Path, fs: &dyn FileSystem) -> Result<Analysis, LoadError> {
    let (program, mut diags) = ws_resolve::load(entry, fs)?;
    // Name and type errors in code that didn't parse are mostly echoes of the syntax error.
    if diags
        .iter()
        .any(|d| d.diagnostic.severity == ws_syntax::Severity::Error)
    {
        diags.sort_by_key(|d| (d.module, d.diagnostic.span().start));
        return Ok(Analysis {
            program,
            resolution: Resolution {
                modules: Vec::new(),
            },
            checked: Checked {
                diagnostics: diags,
                types: Vec::new(),
                fns: HashMap::new(),
            },
        });
    }
    let (resolution, resolve_diags) = ws_resolve::resolve(&program);
    diags.extend(resolve_diags);
    let mut checked = check(&program, &resolution);
    diags.append(&mut checked.diagnostics);
    diags.sort_by_key(|d| (d.module, d.diagnostic.span().start));
    checked.diagnostics = diags;
    Ok(Analysis {
        program,
        resolution,
        checked,
    })
}

pub fn check(program: &Program, resolution: &Resolution) -> Checked {
    let mut c = Checker {
        program,
        res: resolution,
        diags: Vec::new(),
        records: HashMap::new(),
        enums: HashMap::new(),
        fns: HashMap::new(),
        aliases: HashMap::new(),
        expanding: Vec::new(),
    };
    c.collect_signatures();
    let types = program
        .module_ids()
        .map(|m| {
            let mut types = ModuleTypes::default();
            for (item, it) in program.module(m).ast.items.iter().enumerate() {
                if let Item::Fn(f) = it {
                    infer::check_fn(&mut c, DefId { module: m, item }, f, &mut types);
                }
            }
            types
        })
        .collect();
    Checked {
        diagnostics: c.diags,
        types,
        fns: c.fns,
    }
}

pub(crate) struct Checker<'p> {
    pub program: &'p Program,
    pub res: &'p Resolution,
    pub diags: Vec<ProgramDiagnostic>,
    /// Field names and types, in terms of the record's own generic parameters.
    pub records: HashMap<DefId, Vec<(String, Ty)>>,
    pub enums: HashMap<DefId, Vec<(String, Vec<Ty>)>>,
    pub fns: HashMap<DefId, FnSig>,
    pub aliases: HashMap<DefId, Ty>,
    /// Aliases being expanded, to catch cycles.
    pub expanding: Vec<DefId>,
}

impl Checker<'_> {
    pub fn error(&mut self, module: ModuleId, diagnostic: ws_syntax::Diagnostic) {
        self.diags.push(ProgramDiagnostic { module, diagnostic });
    }
}
