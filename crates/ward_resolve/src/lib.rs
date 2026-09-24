//! Name resolution, modules and imports for Wardscript.
//!
//! A program is a set of modules, one per file. [`load`] parses the entry file and
//! everything it imports; [`resolve`] maps every name in every module to its definition.

mod loader;
mod resolver;
mod suggest;

use std::collections::HashMap;

use la_arena::{Arena, ArenaMap, Idx};
use ward_syntax::Diagnostic;
use ward_syntax::ast::{ExprId, PatId, StmtId, TypeId};

pub use loader::{FileSystem, LoadError, RealFs, load};
pub use resolver::resolve;
pub use suggest::did_you_mean;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModuleId(pub u32);

/// An item of a module: `module.ast.items[item]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DefId {
    pub module: ModuleId,
    pub item: usize,
}

pub struct Program {
    /// `modules[0]` is the entry module.
    pub modules: Vec<ModuleData>,
}

impl Program {
    pub fn module(&self, id: ModuleId) -> &ModuleData {
        &self.modules[id.0 as usize]
    }

    pub fn item(&self, def: DefId) -> &ward_syntax::ast::Item {
        &self.module(def.module).ast.items[def.item]
    }

    pub fn module_ids(&self) -> impl Iterator<Item = ModuleId> + use<> {
        (0..self.modules.len() as u32).map(ModuleId)
    }
}

pub struct ModuleData {
    /// The dotted name it's imported by (`support.tickets`); the file stem for the entry.
    pub name: String,
    /// The file path as shown in diagnostics.
    pub path: String,
    pub src: String,
    pub ast: ward_syntax::ast::Module,
    /// Target module of each `import a.b` item, by item index.
    pub imports: HashMap<usize, ModuleId>,
}

/// A diagnostic in one of the program's files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramDiagnostic {
    pub module: ModuleId,
    pub diagnostic: Diagnostic,
}

pub type LocalId = Idx<Local>;

/// A variable: a parameter, `let`, `for` variable or pattern binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Local {
    pub name: String,
    pub span: ward_syntax::Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    Some,
    None,
    Ok,
    Err,
    Validate,
    Approve,
    Declassify,
}

impl Builtin {
    pub fn name(self) -> &'static str {
        match self {
            Builtin::Some => "Some",
            Builtin::None => "None",
            Builtin::Ok => "Ok",
            Builtin::Err => "Err",
            Builtin::Validate => "validate",
            Builtin::Approve => "approve",
            Builtin::Declassify => "declassify",
        }
    }
}

/// What a name (or a `a.b` path) in expression or pattern position refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueRes {
    Local(LocalId),
    Fn(DefId),
    /// Variant `index` of the enum `DefId`.
    Variant(DefId, usize),
    Builtin(Builtin),
    /// The enum itself, as the namespace in `Priority.High`.
    Enum(DefId),
    Module(ModuleId),
    /// An `import mcp "..." as x` namespace. Typed dynamically until M7.
    Tool(DefId),
    ToolMember(DefId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prim {
    Int,
    Float,
    String,
    Bool,
    List,
    Map,
    Option,
    Result,
    Untrusted,
    Trusted,
}

impl Prim {
    pub const ALL: [(&'static str, Prim); 10] = [
        ("Int", Prim::Int),
        ("Float", Prim::Float),
        ("String", Prim::String),
        ("Bool", Prim::Bool),
        ("List", Prim::List),
        ("Map", Prim::Map),
        ("Option", Prim::Option),
        ("Result", Prim::Result),
        ("Untrusted", Prim::Untrusted),
        ("Trusted", Prim::Trusted),
    ];

    pub fn arity(self) -> usize {
        match self {
            Prim::Int | Prim::Float | Prim::String | Prim::Bool => 0,
            Prim::List | Prim::Option | Prim::Untrusted | Prim::Trusted => 1,
            Prim::Map | Prim::Result => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeRes {
    Prim(Prim),
    /// A record, alias or enum.
    Def(DefId),
    /// The `n`th generic parameter of the enclosing item.
    Param(usize),
}

/// Resolution results for one module, keyed by AST node IDs.
#[derive(Default)]
pub struct ModuleRes {
    pub locals: Arena<Local>,
    /// `Name` expressions, and `Field` expressions that are paths (`Priority.High`, `mail.send`).
    pub values: ArenaMap<ExprId, ValueRes>,
    pub types: ArenaMap<TypeId, TypeRes>,
    /// Target record of each record literal.
    pub records: ArenaMap<ExprId, DefId>,
    /// The variable read by shorthand field `i` (`Page { next }`) of a record literal.
    pub shorthands: HashMap<(ExprId, usize), ValueRes>,
    pub pat_bindings: ArenaMap<PatId, LocalId>,
    /// `None`, `Some(..)`, `Priority.High`, ... in patterns.
    pub pat_variants: ArenaMap<PatId, ValueRes>,
    /// Parameters of each function, by item index.
    pub params: HashMap<usize, Vec<LocalId>>,
    /// Variables bound by `let` and `for` statements.
    pub stmt_locals: ArenaMap<StmtId, LocalId>,
}

pub struct Resolution {
    pub modules: Vec<ModuleRes>,
}

impl Resolution {
    pub fn module(&self, id: ModuleId) -> &ModuleRes {
        &self.modules[id.0 as usize]
    }
}
