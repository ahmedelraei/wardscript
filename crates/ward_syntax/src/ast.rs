use la_arena::{Arena, Idx};

use crate::span::Span;

pub type ExprId = Idx<Expr>;
pub type StmtId = Idx<Stmt>;
pub type TypeId = Idx<TypeExpr>;
pub type PatId = Idx<Pat>;

/// One source file. Nodes live in the arenas and are referenced by ID.
#[derive(Debug, Default, PartialEq)]
pub struct Module {
    pub items: Vec<Item>,
    pub exprs: Arena<Expr>,
    pub stmts: Arena<Stmt>,
    pub types: Arena<TypeExpr>,
    pub pats: Arena<Pat>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

/// Dot-separated names: `mail.send`, `Priority.High`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Path {
    pub segments: Vec<Ident>,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub enum Item {
    Fn(FnDecl),
    Record(RecordDecl),
    Alias(AliasDecl),
    Enum(EnumDecl),
    Import(Import),
}

impl Item {
    pub fn span(&self) -> Span {
        match self {
            Item::Fn(d) => d.span,
            Item::Record(d) => d.span,
            Item::Alias(d) => d.span,
            Item::Enum(d) => d.span,
            Item::Import(d) => d.span,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct FnDecl {
    pub annotations: Vec<Annotation>,
    pub is_pub: bool,
    /// `ai fn`: the body is a prompt and the model produces the result.
    pub is_ai: bool,
    pub name: Ident,
    pub generics: Vec<Ident>,
    pub params: Vec<Param>,
    pub ret: Option<TypeId>,
    /// `throws E`: the type of error the function may throw.
    pub throws: Option<TypeId>,
    pub uses: Option<Vec<Path>>,
    pub budget: Option<Vec<BudgetEntry>>,
    pub body: FnBody,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub enum FnBody {
    Block(Block),
    /// The body of an `ai fn`: `{ "prompt with {params}" }`, a string or template literal.
    Ai {
        prompt: ExprId,
    },
}

#[derive(Debug, PartialEq)]
pub struct Param {
    pub name: Ident,
    pub ty: TypeId,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub struct BudgetEntry {
    pub name: Ident,
    pub value: ExprId,
}

#[derive(Debug, PartialEq)]
pub struct RecordDecl {
    pub is_pub: bool,
    pub name: Ident,
    pub generics: Vec<Ident>,
    pub fields: Vec<FieldDecl>,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub struct FieldDecl {
    pub name: Ident,
    pub ty: TypeId,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub struct AliasDecl {
    pub is_pub: bool,
    pub name: Ident,
    pub generics: Vec<Ident>,
    pub ty: TypeId,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub struct EnumDecl {
    pub is_pub: bool,
    pub name: Ident,
    pub generics: Vec<Ident>,
    pub variants: Vec<Variant>,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub struct Variant {
    pub name: Ident,
    pub fields: Vec<TypeId>,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub struct Import {
    pub annotations: Vec<Annotation>,
    pub kind: ImportKind,
    pub alias: Option<Ident>,
    pub span: Span,
}

/// `@allow(rule_of_two, reason = "...")`. Only functions and imports take annotations;
/// the checker decides which names and arguments mean something.
#[derive(Debug, PartialEq)]
pub struct Annotation {
    pub name: Ident,
    pub args: Vec<AnnotationArg>,
    pub span: Span,
}

/// `name` or `name = "value"`.
#[derive(Debug, PartialEq)]
pub struct AnnotationArg {
    pub name: Ident,
    pub value: Option<(String, Span)>,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub enum ImportKind {
    /// `import support.tickets`
    Module(Path),
    /// `import mcp "gmail" as mail`
    Tool {
        provider: Ident,
        source: String,
        source_span: Span,
    },
}

#[derive(Debug, PartialEq)]
pub struct TypeExpr {
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub enum TypeKind {
    /// `String`, `List<T>`, `Untrusted<Ticket>`
    Named {
        path: Path,
        args: Vec<TypeId>,
    },
    Error,
}

#[derive(Debug, PartialEq)]
pub struct Block {
    pub stmts: Vec<StmtId>,
    /// Final expression without `;`: the block's value.
    pub tail: Option<ExprId>,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub enum StmtKind {
    Let {
        name: Ident,
        ty: Option<TypeId>,
        init: ExprId,
    },
    Assign {
        target: ExprId,
        value: ExprId,
    },
    /// `semi`: written with a trailing `;`. Only affects a block's last statement, where
    /// `x;` discards the value and a bare `x` would be the block's value instead.
    Expr {
        expr: ExprId,
        semi: bool,
    },
    Return(Option<ExprId>),
    /// `throw e`
    Throw(ExprId),
    For {
        var: Ident,
        iter: ExprId,
        body: Block,
    },
    While {
        cond: ExprId,
        body: Block,
    },
}

#[derive(Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub enum ExprKind {
    Lit(Lit),
    /// A string literal with at least one `{expr}` interpolation.
    Template(Vec<TemplatePart>),
    Name(Ident),
    Field {
        base: ExprId,
        name: Ident,
    },
    Call {
        callee: ExprId,
        args: Vec<ExprId>,
    },
    Index {
        base: ExprId,
        index: ExprId,
    },
    /// `call()?`: marks a call that may throw; the error propagates.
    Propagate(ExprId),
    Unary {
        op: UnOp,
        operand: ExprId,
    },
    Binary {
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
    },
    List(Vec<ExprId>),
    /// `Ticket { ... }` or, for a type from another module, `tickets.Ticket { ... }`.
    Record {
        path: Path,
        fields: Vec<FieldInit>,
    },
    If {
        cond: ExprId,
        then: Block,
        /// Either a `Block` or another `If` expression.
        else_: Option<ExprId>,
    },
    Match {
        scrutinee: ExprId,
        arms: Vec<Arm>,
    },
    /// `try { ... } catch err { ... }`. `err` is `None` for `catch _`.
    TryCatch {
        body: Block,
        err: Option<Ident>,
        handler: Block,
    },
    Block(Block),
    Error,
}

impl ExprKind {
    /// Block-like expressions end a statement without `;`.
    pub fn is_block_like(&self) -> bool {
        matches!(
            self,
            ExprKind::If { .. }
                | ExprKind::Match { .. }
                | ExprKind::TryCatch { .. }
                | ExprKind::Block(_)
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Lit {
    Int(i64),
    /// Kept as written (minus `_` separators) so printing round-trips exactly.
    Float(String),
    Str(String),
    Bool(bool),
}

#[derive(Debug, PartialEq)]
pub enum TemplatePart {
    Lit(String),
    Expr(ExprId),
}

#[derive(Debug, PartialEq)]
pub struct FieldInit {
    pub name: Ident,
    /// `None` for the shorthand `Ticket { title }`.
    pub value: Option<ExprId>,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub struct Arm {
    pub pat: PatId,
    pub body: ExprId,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

impl UnOp {
    pub fn as_str(self) -> &'static str {
        match self {
            UnOp::Neg => "-",
            UnOp::Not => "!",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl BinOp {
    pub fn as_str(self) -> &'static str {
        match self {
            BinOp::Or => "||",
            BinOp::And => "&&",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
        }
    }

    /// Binding power; higher binds tighter. Comparisons are non-associative.
    pub fn prec(self) -> u8 {
        match self {
            BinOp::Or => 1,
            BinOp::And => 2,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => 3,
            BinOp::Add | BinOp::Sub => 4,
            BinOp::Mul | BinOp::Div | BinOp::Rem => 5,
        }
    }

    pub fn is_comparison(self) -> bool {
        self.prec() == 3
    }
}

pub const PREC_UNARY: u8 = 6;
pub const PREC_POSTFIX: u8 = 7;

#[derive(Debug, PartialEq)]
pub struct Pat {
    pub kind: PatKind,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub enum PatKind {
    Wild,
    /// A lone name: a binding, or a unit variant like `None` (decided by name resolution).
    Name(Ident),
    Lit(Lit),
    /// `Priority.High`, `Some(x)`, `Priority.Custom(label)`. `args` is `None` without parens.
    Variant {
        path: Path,
        args: Option<Vec<PatId>>,
    },
    Error,
}
