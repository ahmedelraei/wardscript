//! WIR: the fully typed Wardscript IR. Every expression carries its type, names are
//! resolved to definitions and locals, and surface sugar is gone: `?` disappears
//! (propagation is implicit once the checker has approved it), record shorthands become
//! locals, methods become [`Method`]s and assignments to fields or elements become
//! [`Place`] paths. Trust labels join in M4; provenance is the [`Site`] of each
//! `validate`, `approve` and `declassify`.

mod lower;

use la_arena::{Arena, Idx};
pub use ward_check::Ty;
pub use ward_resolve::{DefId, ModuleId};
pub use ward_syntax::ast::{BinOp, UnOp};

pub use lower::{LowerError, lower};
mod refine;

pub type ExprId = Idx<Expr>;
pub type StmtId = Idx<Stmt>;
pub type PatId = Idx<Pat>;
pub type LocalId = Idx<Local>;

pub struct Program {
    /// Indexed by `ModuleId`; `modules[0]` is the entry module.
    pub modules: Vec<Module>,
}

impl Program {
    pub fn module(&self, id: ModuleId) -> &Module {
        &self.modules[id.0 as usize]
    }

    pub fn record(&self, def: DefId) -> Option<&Record> {
        self.module(def.module)
            .records
            .iter()
            .find(|r| r.def == def)
    }

    pub fn enum_(&self, def: DefId) -> Option<&Enum> {
        self.module(def.module).enums.iter().find(|e| e.def == def)
    }

    pub fn func(&self, def: DefId) -> Option<&Fn> {
        self.module(def.module).fns.iter().find(|f| f.def == def)
    }

    pub fn refinement(&self, key: ward_check::ty::Refinement) -> Option<&RefinementFn> {
        self.module(key.module)
            .refinements
            .iter()
            .find(|r| r.key == key)
    }

    pub fn tool(&self, def: DefId) -> Option<&Tool> {
        self.module(def.module).tools.iter().find(|t| t.def == def)
    }

    /// Name of a record or enum.
    pub fn adt_name(&self, def: DefId) -> &str {
        self.record(def)
            .map(|r| r.name.as_str())
            .or_else(|| self.enum_(def).map(|e| e.name.as_str()))
            .unwrap_or("?")
    }
}

pub struct Module {
    /// Dotted import name (`support.tickets`); the file stem for the entry module.
    pub name: String,
    /// Source path, for provenance.
    pub path: String,
    pub records: Vec<Record>,
    pub enums: Vec<Enum>,
    pub fns: Vec<Fn>,
    pub tools: Vec<Tool>,
    /// Not in `fns`: they aren't Wardscript functions and have no `DefId` of their own.
    pub refinements: Vec<RefinementFn>,
    /// `test "..." { ... }`, run by `ward test`.
    pub tests: Vec<Test>,
}

pub struct Test {
    pub name: String,
    pub site: Site,
    /// A function with no parameters; its `name` is the generated Python name.
    pub func: Fn,
}

pub struct Record {
    pub def: DefId,
    pub name: String,
    pub is_pub: bool,
    pub generics: Vec<String>,
    /// In declaration order; types refer to `generics` as `Ty::Param`.
    pub fields: Vec<Field>,
}

pub struct Field {
    pub name: String,
    pub ty: Ty,
}

pub struct Enum {
    pub def: DefId,
    pub name: String,
    pub is_pub: bool,
    pub generics: Vec<String>,
    pub variants: Vec<Variant>,
}

impl Enum {
    /// All variants are plain names, like `enum Priority { Low, High }`.
    pub fn is_unit_only(&self) -> bool {
        self.variants.iter().all(|v| v.fields.is_empty())
    }
}

pub struct Variant {
    pub name: String,
    pub fields: Vec<Ty>,
}

/// `import mcp "gmail" as mail`
pub struct Tool {
    pub def: DefId,
    pub provider: String,
    pub source: String,
}

/// Which models an `ai fn` asks, in order, and how it retries provider errors. `None`
/// in `models` is the runtime's default model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelPolicy {
    pub models: Vec<Option<String>>,
    pub retries: Option<u32>,
    pub backoff: Option<f64>,
}

/// A tool function's schema, as the runtime needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolSchema {
    /// The name the server knows the function by.
    pub mcp_name: String,
    /// Parameter names, in call order.
    pub params: Vec<String>,
    /// Which parameters are sinks.
    pub sinks: Vec<bool>,
    /// The result is decoded as this type.
    pub returns: Ty,
}

pub struct Fn {
    pub def: DefId,
    pub name: String,
    pub is_pub: bool,
    pub generics: Vec<String>,
    pub params: Vec<LocalId>,
    /// Parameters that reach a sink. Callers outside Wardscript must vouch for them.
    pub trusted: Vec<bool>,
    /// `budget {...}` limits, checked by the runtime: `tokens`, `calls`, `cost`, `time`.
    pub budget: Vec<(String, BudgetValue)>,
    /// An `ai fn`'s `model {...}` clause.
    pub model: Option<ModelPolicy>,
    pub ret: Ty,
    pub throws: Option<Ty>,
    pub locals: Arena<Local>,
    pub exprs: Arena<Expr>,
    pub stmts: Arena<Stmt>,
    pub pats: Arena<Pat>,
    pub body: Body,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BudgetValue {
    Int(i64),
    Float(f64),
}

pub enum Body {
    Block(Block),
    /// `ai fn`: the model answers `prompt` with a value of the return type.
    Ai {
        prompt: ExprId,
        /// `check {...}`: conditions on the answer, bound to `it`.
        checks: Vec<Check>,
        it: Option<LocalId>,
    },
}

pub struct Check {
    pub cond: ExprId,
    /// Told to the model when the answer fails: the written reason, or the condition.
    pub reason: String,
}

/// A refinement's condition as a function of `it`, and what the runtime needs to
/// describe it.
pub struct RefinementFn {
    pub key: ward_check::ty::Refinement,
    /// The generated function's name.
    pub name: String,
    /// The condition as written, for error messages.
    pub text: String,
    /// JSON Schema keywords it implies, with JSON values.
    pub schema: Vec<(String, String)>,
    /// `fn(it) -> Bool`.
    pub func: Fn,
}

pub struct Local {
    pub name: String,
    pub ty: Ty,
}

/// Where in the source a trust decision is made, e.g. `support.wardscript:58:23`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    pub path: String,
    pub line: u32,
    pub column: u32,
}

impl std::fmt::Display for Site {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.path, self.line, self.column)
    }
}

pub struct Block {
    pub stmts: Vec<StmtId>,
    /// The block's value. When the block's value is `()`, this is evaluated and discarded.
    pub tail: Option<ExprId>,
}

pub enum Stmt {
    Let {
        local: LocalId,
        value: ExprId,
    },
    /// `local.path = value`. Values are immutable, so this rebinds `local` to a copy
    /// with the element at `path` replaced.
    Assign {
        local: LocalId,
        path: Vec<Place>,
        value: ExprId,
    },
    Expr(ExprId),
    Return(Option<ExprId>),
    Throw(ExprId),
    /// Over a list's elements, or a map's keys.
    For {
        local: LocalId,
        iter: ExprId,
        body: Block,
    },
    While {
        cond: ExprId,
        body: Block,
    },
    /// `assert cond => "message"` in a test. `message` defaults to the condition.
    Assert {
        cond: ExprId,
        message: String,
        site: Site,
    },
}

pub enum Place {
    /// A record field. `record` is `None` when the base is a tool result.
    Field { record: Option<DefId>, name: String },
    /// A list element or map entry, depending on the container's type.
    Index { container: Ty, index: ExprId },
}

pub struct Expr {
    pub kind: ExprKind,
    pub ty: Ty,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Lit {
    Int(i64),
    /// As written, minus `_` separators.
    Float(String),
    Str(String),
    Bool(bool),
}

pub enum TemplatePart {
    Lit(String),
    Expr(ExprId),
}

pub enum ExprKind {
    Lit(Lit),
    Local(LocalId),
    /// A string with interpolations; each part is converted to text by its type.
    Template(Vec<TemplatePart>),
    /// A call to a Wardscript function (including `ai fn`s).
    Call {
        func: DefId,
        args: Vec<ExprId>,
    },
    /// `mail.send(...)`: a call to an imported tool.
    ToolCall {
        tool: DefId,
        name: String,
        args: Vec<ExprId>,
        site: Site,
        /// From `ward.lock`; `None` for a tool without a schema.
        schema: Option<ToolSchema>,
    },
    /// A built-in method; the receiver's type is `recv`'s type.
    Method {
        method: Method,
        recv: ExprId,
        args: Vec<ExprId>,
    },
    /// A method call on a tool result, whose type isn't known yet.
    DynMethod {
        recv: ExprId,
        name: String,
        args: Vec<ExprId>,
    },
    Unary {
        op: UnOp,
        operand: ExprId,
    },
    /// Operand types are the same, except for `&&`/`||` (both `Bool`).
    Binary {
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
    },
    List(Vec<ExprId>),
    /// Fields in source order (evaluation order), as indices into the record's fields.
    Record {
        record: DefId,
        fields: Vec<(usize, ExprId)>,
    },
    Variant {
        enum_: DefId,
        index: usize,
        args: Vec<ExprId>,
    },
    Some(ExprId),
    None,
    /// `record` is `None` when the base is a tool result.
    Field {
        base: ExprId,
        record: Option<DefId>,
        name: String,
    },
    Index {
        base: ExprId,
        index: ExprId,
    },
    If {
        cond: ExprId,
        then: Block,
        else_: Option<Block>,
    },
    Match {
        scrutinee: ExprId,
        arms: Vec<Arm>,
    },
    TryCatch {
        body: Block,
        err: Option<LocalId>,
        handler: Block,
    },
    Block(Block),
    /// `validate(value, rule)`: `value` if `rule(value)` holds, otherwise throws a `String`.
    Validate {
        value: ExprId,
        rule: DefId,
        site: Site,
    },
    Approve {
        value: ExprId,
        site: Site,
    },
    Declassify {
        value: ExprId,
        reason: ExprId,
        site: Site,
    },
}

pub struct Arm {
    pub pat: PatId,
    pub body: ExprId,
}

pub struct Pat {
    pub kind: PatKind,
    pub ty: Ty,
}

pub enum PatKind {
    Wild,
    Bind(LocalId),
    Lit(Lit),
    Some(PatId),
    None,
    Variant {
        enum_: DefId,
        index: usize,
        args: Vec<PatId>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    StrLen,
    StrIsEmpty,
    StrContains,
    StartsWith,
    EndsWith,
    Trim,
    Lower,
    Upper,
    Split,
    Lines,
    Replace,
    ListLen,
    ListIsEmpty,
    ListContains,
    ListGet,
    ListFirst,
    ListLast,
    ListPush,
    MapLen,
    MapIsEmpty,
    MapGet,
    MapContainsKey,
    MapKeys,
    MapValues,
    MapInsert,
    IsSome,
    IsNone,
    UnwrapOr,
    ToString,
    ToFloat,
    Round,
}

impl Method {
    pub fn resolve(recv: &Ty, name: &str) -> Option<Method> {
        use Method::*;
        Some(match (recv, name) {
            (Ty::String, "len") => StrLen,
            (Ty::String, "is_empty") => StrIsEmpty,
            (Ty::String, "contains") => StrContains,
            (Ty::String, "starts_with") => StartsWith,
            (Ty::String, "ends_with") => EndsWith,
            (Ty::String, "trim") => Trim,
            (Ty::String, "lower") => Lower,
            (Ty::String, "upper") => Upper,
            (Ty::String, "split") => Split,
            (Ty::String, "lines") => Lines,
            (Ty::String, "replace") => Replace,
            (Ty::List(_), "len") => ListLen,
            (Ty::List(_), "is_empty") => ListIsEmpty,
            (Ty::List(_), "contains") => ListContains,
            (Ty::List(_), "get") => ListGet,
            (Ty::List(_), "first") => ListFirst,
            (Ty::List(_), "last") => ListLast,
            (Ty::List(_), "push") => ListPush,
            (Ty::Map(..), "len") => MapLen,
            (Ty::Map(..), "is_empty") => MapIsEmpty,
            (Ty::Map(..), "get") => MapGet,
            (Ty::Map(..), "contains_key") => MapContainsKey,
            (Ty::Map(..), "keys") => MapKeys,
            (Ty::Map(..), "values") => MapValues,
            (Ty::Map(..), "insert") => MapInsert,
            (Ty::Option(_), "is_some") => IsSome,
            (Ty::Option(_), "is_none") => IsNone,
            (Ty::Option(_), "unwrap_or") => UnwrapOr,
            (Ty::Int | Ty::Float | Ty::Bool, "to_string") => ToString,
            (Ty::Int, "to_float") => ToFloat,
            (Ty::Float, "round") => Round,
            _ => return None,
        })
    }
}
