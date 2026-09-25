//! Function bodies. Wardscript is expression-oriented and Python isn't, so an `if`,
//! `match`, `try` or block whose value is used becomes statements that store the value
//! in its destination (a variable, `return`, or nowhere). Operands evaluated before a
//! hoisted expression are saved to temporaries first, keeping left-to-right order.

use std::collections::HashSet;

use ward_ir::*;

use crate::names::{self, Scope, may_be_none};

// Python operator precedence, loosest first.
const TERNARY: u8 = 1;
const OR: u8 = 2;
const AND: u8 = 3;
const NOT: u8 = 4;
const CMP: u8 = 5;
const ADD: u8 = 6;
const MUL: u8 = 7;
const UNARY: u8 = 8;
const ATOM: u8 = 9;

/// A Python expression and how tightly it binds.
struct Py {
    text: String,
    prec: u8,
}

impl Py {
    fn new(text: impl Into<String>, prec: u8) -> Py {
        Py {
            text: text.into(),
            prec,
        }
    }

    fn atom(text: impl Into<String>) -> Py {
        Py::new(text, ATOM)
    }

    /// The text, parenthesized unless it binds at least as tightly as `min`.
    fn at(&self, min: u8) -> String {
        if self.prec >= min {
            self.text.clone()
        } else {
            format!("({})", self.text)
        }
    }
}

/// Where a value goes.
#[derive(Clone)]
enum Dest {
    Return,
    Assign(String),
    Discard,
}

pub struct FnGen<'a, 'p> {
    f: &'a Fn,
    scope: &'a mut Scope<'p>,
    locals: Vec<String>,
    taken: HashSet<String>,
    tmp: u32,
    indent: usize,
    pub lines: Vec<String>,
    /// Generate `async def`s that await calls, models, approvals and tools.
    pub asyncio: bool,
}

impl<'a, 'p> FnGen<'a, 'p> {
    /// `module_names` are the module's top-level Python names, which locals must not
    /// shadow: Python makes a name local to the whole function once it's assigned.
    pub fn new(f: &'a Fn, scope: &'a mut Scope<'p>, module_names: &HashSet<String>) -> Self {
        let mut taken = module_names.clone();
        taken.extend(["_rt", "_dc", "_enum", "_typing"].map(String::from));
        let mut locals = Vec::with_capacity(f.locals.len());
        for (_, local) in f.locals.iter() {
            let base = names::ident(&local.name);
            let mut name = base.clone();
            let mut n = 2;
            while taken.contains(&name) {
                name = format!("{base}_{n}");
                n += 1;
            }
            taken.insert(name.clone());
            locals.push(name);
        }
        FnGen {
            f,
            scope,
            locals,
            taken,
            tmp: 0,
            indent: 1,
            lines: Vec::new(),
            asyncio: false,
        }
    }

    pub fn local(&self, l: LocalId) -> &str {
        self.locals
            .get(l.into_raw().into_u32() as usize)
            .map_or("_", String::as_str)
    }

    fn line(&mut self, text: impl AsRef<str>) {
        self.lines
            .push(format!("{}{}", "    ".repeat(self.indent), text.as_ref()));
    }

    fn fresh(&mut self) -> String {
        loop {
            self.tmp += 1;
            let name = format!("_t{}", self.tmp);
            if self.taken.insert(name.clone()) {
                return name;
            }
        }
    }

    /// Runs `f` one level deeper, adding `pass` if it wrote nothing.
    fn nested(&mut self, f: impl FnOnce(&mut Self)) {
        self.indent += 1;
        let before = self.lines.len();
        f(self);
        if self.lines.len() == before {
            self.line("pass");
        }
        self.indent -= 1;
    }

    pub fn body(&mut self) {
        let f = self.f;
        for (&p, _) in f.params.iter().zip(&f.trusted).filter(|(_, t)| **t) {
            let name = self.local(p).to_owned();
            self.line(format!(
                "{name} = _rt.vouched({name}, {}, {})",
                names::string(&f.locals[p].name),
                names::string(&f.name)
            ));
        }
        match &f.body {
            Body::Abstract => self.line(format!(
                "raise NotImplementedError({})",
                names::string(&f.name)
            )),
            Body::Block(b) => {
                let dest = if f.ret == Ty::Unit {
                    Dest::Discard
                } else {
                    Dest::Return
                };
                let before = self.lines.len();
                self.block_into(b, &dest);
                if self.lines.len() == before {
                    self.line("pass");
                }
            }
            Body::Ai { prompt, checks, it } => {
                // `check {...}` becomes a function of the answer: the first failed
                // check's reason, or `None`.
                let check = match (checks.is_empty(), it) {
                    (false, Some(it)) => {
                        let name = self.fresh();
                        let def = if self.asyncio { "async def" } else { "def" };
                        let it = self.local(*it).to_owned();
                        self.line(format!("{def} {name}({it}):"));
                        self.nested(|g| {
                            for c in checks {
                                let cond = g.expr(c.cond);
                                g.line(format!("if not {}:", cond.at(ATOM)));
                                g.nested(|g| {
                                    g.line(format!("return {}", names::string(&c.reason)))
                                });
                            }
                            g.line("return None");
                        });
                        Some(name)
                    }
                    _ => None,
                };
                let prompt = self.expr(*prompt);
                let returns = self.scope.descriptor(&f.ret);
                let call = if self.asyncio {
                    "await _rt.ai_async"
                } else {
                    "_rt.ai"
                };
                let mut policy = String::new();
                if let Some(m) = &f.model {
                    if m.models.len() > 1 || m.models.first().is_some_and(Option::is_some) {
                        let models: Vec<String> = m
                            .models
                            .iter()
                            .map(|a| a.as_deref().map_or("None".to_owned(), names::string))
                            .collect();
                        let models = match models.len() {
                            1 => format!("({},)", models[0]),
                            _ => format!("({})", models.join(", ")),
                        };
                        policy.push_str(&format!(", models={models}"));
                    }
                    if let Some(r) = m.retries {
                        policy.push_str(&format!(", retries={r}"));
                    }
                    if let Some(b) = m.backoff {
                        policy.push_str(&format!(", backoff={b:?}"));
                    }
                }
                if let Some(check) = check {
                    policy.push_str(&format!(", check={check}"));
                }
                self.line(format!(
                    "return {call}({}, {}, {returns}{policy})",
                    names::string(&f.name),
                    prompt.text
                ));
            }
        }
    }

    /// A runtime operation that has an async version.
    fn op(&self, name: &str) -> String {
        if self.asyncio {
            format!("_rt.{name}_async")
        } else {
            format!("_rt.{name}")
        }
    }

    fn awaited(&self, call: String) -> Py {
        if self.asyncio {
            Py::new(format!("await {call}"), UNARY)
        } else {
            Py::atom(call)
        }
    }

    fn ex(&self, e: ExprId) -> &'a Expr {
        &self.f.exprs[e]
    }

    fn block_into(&mut self, b: &Block, dest: &Dest) {
        for &s in &b.stmts {
            self.stmt(s);
        }
        match b.tail {
            Some(t) => self.expr_into(t, dest),
            None => {
                let diverges = b
                    .stmts
                    .last()
                    .is_some_and(|&s| matches!(self.f.stmts[s], Stmt::Return(_) | Stmt::Throw(_)));
                if let Dest::Assign(name) = dest {
                    if !diverges {
                        self.line(format!("{name} = None"));
                    }
                }
            }
        }
    }

    fn emit(&mut self, value: Py, dest: &Dest, e: ExprId) {
        match dest {
            Dest::Return => self.line(format!("return {}", value.text)),
            Dest::Assign(name) => self.line(format!("{name} = {}", value.text)),
            Dest::Discard => {
                // A discarded variable or literal would be a no-op statement.
                if !matches!(
                    self.ex(e).kind,
                    ExprKind::Local(_) | ExprKind::Lit(_) | ExprKind::None
                ) {
                    self.line(value.text);
                }
            }
        }
    }

    fn expr_into(&mut self, e: ExprId, dest: &Dest) {
        let f = self.f;
        match &f.exprs[e].kind {
            ExprKind::If { .. } if !self.ternary(e) => self.if_into(e, dest),
            ExprKind::Match { scrutinee, arms } => self.match_into(*scrutinee, arms, dest),
            ExprKind::TryCatch { body, err, handler } => self.try_into(body, *err, handler, dest),
            ExprKind::Block(b) => self.block_into(b, dest),
            _ => {
                let value = self.expr(e);
                self.emit(value, dest, e);
            }
        }
    }

    fn if_into(&mut self, mut e: ExprId, dest: &Dest) {
        let f = self.f;
        let mut keyword = "if";
        loop {
            let ExprKind::If { cond, then, else_ } = &f.exprs[e].kind else {
                return;
            };
            let cond = self.expr(*cond);
            self.line(format!("{keyword} {}:", cond.text));
            match else_ {
                None => {
                    self.nested(|g| g.block_into(then, &Dest::Discard));
                    // Without `else`, the value is `()`.
                    if !matches!(dest, Dest::Discard) {
                        self.emit(Py::atom("None"), dest, e);
                    }
                    return;
                }
                Some(else_) => {
                    self.nested(|g| g.block_into(then, dest));
                    // `else { if ... }` becomes `elif`, unless its condition needs
                    // statements of its own.
                    if let (true, Some(next)) = (else_.stmts.is_empty(), else_.tail) {
                        if let ExprKind::If { cond, .. } = &f.exprs[next].kind {
                            if !self.needs_stmts(*cond) {
                                e = next;
                                keyword = "elif";
                                continue;
                            }
                        }
                    }
                    self.line("else:");
                    self.nested(|g| g.block_into(else_, dest));
                    return;
                }
            }
        }
    }

    fn match_into(&mut self, scrutinee: ExprId, arms: &[Arm], dest: &Dest) {
        let value = self.expr(scrutinee);
        let subject = match self.ex(scrutinee).kind {
            _ if self.is_pure(scrutinee) => value.at(ATOM),
            _ => {
                let t = self.fresh();
                self.line(format!("{t} = {}", value.text));
                t
            }
        };
        if arms.is_empty() {
            self.line("raise AssertionError(\"unreachable: match on a type with no values\")");
            return;
        }
        let last = arms.len() - 1;
        for (i, arm) in arms.iter().enumerate() {
            let mut conds = Vec::new();
            let mut binds = Vec::new();
            self.pat(arm.pat, &subject, &mut conds, &mut binds);
            let arm_body = |g: &mut Self| {
                for (name, access) in &binds {
                    g.line(format!("{name} = {access}"));
                }
                g.expr_into(arm.body, dest);
            };
            if i == 0 && conds.is_empty() {
                // A catch-all first arm: the later ones can't match.
                arm_body(self);
                return;
            }
            if i == last || conds.is_empty() {
                self.line("else:");
                self.nested(arm_body);
                return;
            }
            let keyword = if i == 0 { "if" } else { "elif" };
            self.line(format!("{keyword} {}:", conds.join(" and ")));
            self.nested(arm_body);
        }
    }

    /// Conditions under which `access` matches `p`, and the variables it binds.
    fn pat(
        &mut self,
        p: PatId,
        access: &str,
        conds: &mut Vec<String>,
        binds: &mut Vec<(String, String)>,
    ) {
        let f = self.f;
        match &f.pats[p].kind {
            PatKind::Wild => {}
            PatKind::Bind(l) => binds.push((self.local(*l).to_owned(), access.to_owned())),
            PatKind::Lit(Lit::Bool(true)) => conds.push(access.to_owned()),
            PatKind::Lit(Lit::Bool(false)) => conds.push(format!("not {access}")),
            PatKind::Lit(l) => conds.push(format!("{access} == {}", lit(l).text)),
            PatKind::None => conds.push(format!("{access} is None")),
            PatKind::Some(sub) => {
                conds.push(format!("{access} is not None"));
                let inner = if may_be_none(&f.pats[*sub].ty) {
                    format!("_rt.unwrap({access})")
                } else {
                    access.to_owned()
                };
                self.pat(*sub, &inner, conds, binds);
            }
            PatKind::Variant { enum_, index, args } => {
                let class = self.scope.variant(*enum_, *index);
                let unit_only = self
                    .scope
                    .program
                    .enum_(*enum_)
                    .is_some_and(Enum::is_unit_only);
                if unit_only {
                    conds.push(format!("{access} is {class}"));
                } else {
                    conds.push(format!("isinstance({access}, {class})"));
                }
                for (i, &sub) in args.iter().enumerate() {
                    self.pat(sub, &format!("{access}._{i}"), conds, binds);
                }
            }
        }
    }

    fn try_into(&mut self, body: &Block, err: Option<LocalId>, handler: &Block, dest: &Dest) {
        self.line("try:");
        self.nested(|g| g.block_into(body, dest));
        match err {
            Some(l) => {
                let e = self.fresh();
                self.line(format!("except _rt.Thrown as {e}:"));
                let name = self.local(l).to_owned();
                self.nested(|g| {
                    g.line(format!("{name} = {e}.value"));
                    g.block_into(handler, dest);
                });
            }
            None => {
                self.line("except _rt.Thrown:");
                self.nested(|g| g.block_into(handler, dest));
            }
        }
    }

    fn stmt(&mut self, s: StmtId) {
        let f = self.f;
        match &f.stmts[s] {
            Stmt::Let { local, value } => {
                let name = self.local(*local).to_owned();
                self.expr_into(*value, &Dest::Assign(name));
            }
            Stmt::Assign { local, path, value } => self.assign(*local, path, *value),
            Stmt::SetField {
                obj,
                field,
                path,
                value,
                ..
            } => self.set_field(*obj, field, path, *value),
            Stmt::Expr(e) => self.expr_into(*e, &Dest::Discard),
            Stmt::Return(None) => self.line("return"),
            Stmt::Return(Some(e)) => {
                if f.ret == Ty::Unit {
                    self.expr_into(*e, &Dest::Discard);
                    self.line("return");
                } else {
                    self.expr_into(*e, &Dest::Return);
                }
            }
            Stmt::Throw(e) => {
                let value = self.expr(*e);
                self.line(format!("raise _rt.Thrown({})", value.text));
            }
            Stmt::Assert {
                cond,
                message,
                site,
            } => {
                let c = self.expr(*cond);
                self.line(format!("if not {}:", c.at(ATOM)));
                self.nested(|g| {
                    g.line(format!(
                        "raise _rt.TestFailure({}, {})",
                        names::string(message),
                        names::string(&site.to_string())
                    ));
                });
            }
            Stmt::For { local, iter, body } => {
                let iter = self.expr(*iter);
                let name = self.local(*local).to_owned();
                self.line(format!("for {name} in {}:", iter.at(OR)));
                self.nested(|g| g.block_into(body, &Dest::Discard));
            }
            Stmt::While { cond, body } => {
                if self.needs_stmts(*cond) {
                    self.line("while True:");
                    self.nested(|g| {
                        let c = g.expr(*cond);
                        g.line(format!("if not {}:", c.at(NOT)));
                        g.nested(|g| g.line("break"));
                        g.block_into(body, &Dest::Discard);
                    });
                } else {
                    let c = self.expr(*cond);
                    self.line(format!("while {}:", c.text));
                    self.nested(|g| g.block_into(body, &Dest::Discard));
                }
            }
        }
    }

    /// `x.a[i] = v` rebinds `x` to a copy: `x = replace(x, a=list_set(x.a, i, v))`.
    fn assign(&mut self, local: LocalId, path: &[Place], value: ExprId) {
        let name = self.local(local).to_owned();
        if path.is_empty() {
            self.expr_into(value, &Dest::Assign(name));
            return;
        }
        let new = self.updated(&name, path, value);
        self.line(format!("{name} = {new}"));
    }

    /// `obj.field.a = v` sets the field in place: `obj.field = replace(obj.field, a=v)`.
    fn set_field(&mut self, obj: ExprId, field: &str, path: &[Place], value: ExprId) {
        // The object is evaluated before the value, as written.
        let o = if matches!(self.ex(obj).kind, ExprKind::Local(_)) {
            self.expr(obj).at(ATOM)
        } else {
            let t = self.fresh();
            self.expr_into(obj, &Dest::Assign(t.clone()));
            t
        };
        let target = format!("{o}.{}", names::attr(field));
        if path.is_empty() {
            let v = if self.needs_stmts(value) {
                self.hoist(value)
            } else {
                self.expr(value)
            };
            self.line(format!("{target} = {}", v.at(TERNARY)));
            return;
        }
        let new = self.updated(&target, path, value);
        self.line(format!("{target} = {new}"));
    }

    /// `root` with the element at `path` replaced by `value`, as an expression.
    fn updated(&mut self, root: &str, path: &[Place], value: ExprId) -> String {
        let name = root.to_owned();
        let value_hoists = self.needs_stmts(value);
        let mut keys = Vec::new();
        for place in path {
            if let Place::Index { index, .. } = place {
                let k = self.expr(*index);
                let simple = matches!(self.ex(*index).kind, ExprKind::Lit(_))
                    || (!value_hoists && matches!(self.ex(*index).kind, ExprKind::Local(_)));
                if simple {
                    keys.push(k.text);
                } else {
                    let t = self.fresh();
                    self.line(format!("{t} = {}", k.text));
                    keys.push(t);
                }
            }
        }
        let value = self.expr(value);

        // Containers from the variable inwards: `x`, `x.a`, ...
        let mut containers = vec![name.clone()];
        let mut key = keys.iter();
        let mut places = Vec::new();
        for place in path {
            let k = match place {
                Place::Index { .. } => key.next().cloned().unwrap_or_default(),
                Place::Field { .. } => String::new(),
            };
            places.push((place, k));
        }
        for (place, k) in &places[..places.len() - 1] {
            let c = containers.last().cloned().unwrap_or_default();
            containers.push(self.access(&c, place, k));
        }
        let mut new = value.text;
        for ((place, k), c) in places.iter().zip(&containers).rev() {
            new = self.update(c, place, k, &new);
        }
        new
    }

    fn access(&mut self, c: &str, place: &Place, key: &str) -> String {
        match place {
            Place::Field {
                record: Some(_),
                name,
            } => format!("{c}.{}", names::attr(name)),
            Place::Field { record: None, name } => {
                format!("_rt.field({c}, {})", names::string(name))
            }
            Place::Index { container, .. } => match container {
                Ty::List(_) => format!("_rt.index({c}, {key})"),
                Ty::Map(..) => format!("_rt.map_index({c}, {key})"),
                _ => format!("{c}[{key}]"),
            },
        }
    }

    fn update(&mut self, c: &str, place: &Place, key: &str, value: &str) -> String {
        match place {
            Place::Field {
                record: Some(_),
                name,
            } => format!("_dc.replace({c}, {}={value})", names::attr(name)),
            Place::Field { record: None, name } => {
                format!("_rt.with_field({c}, {}, {value})", names::string(name))
            }
            Place::Index { container, .. } => match container {
                Ty::List(_) => format!("_rt.list_set({c}, {key}, {value})"),
                Ty::Map(..) => format!("{{**{c}, {key}: {value}}}"),
                _ => format!("_rt.with_field({c}, {key}, {value})"),
            },
        }
    }

    /// Whether evaluating `e` takes statements, not just a Python expression.
    fn needs_stmts(&self, e: ExprId) -> bool {
        match &self.ex(e).kind {
            ExprKind::If { .. } => !self.ternary(e),
            ExprKind::Match { .. } | ExprKind::TryCatch { .. } => true,
            ExprKind::Block(b) => {
                !b.stmts.is_empty() || b.tail.is_some_and(|t| self.needs_stmts(t))
            }
            _ => self.children(e).into_iter().any(|c| self.needs_stmts(c)),
        }
    }

    /// `if c { a } else { b }` with plain expressions becomes `a if c else b`.
    fn ternary(&self, e: ExprId) -> bool {
        let ExprKind::If {
            cond,
            then,
            else_: Some(else_),
        } = &self.ex(e).kind
        else {
            return false;
        };
        let simple = |b: &Block| b.stmts.is_empty() && b.tail.is_some_and(|t| !self.needs_stmts(t));
        !self.needs_stmts(*cond) && simple(then) && simple(else_)
    }

    /// Subexpressions evaluated in order, for expressions that aren't control flow.
    fn children(&self, e: ExprId) -> Vec<ExprId> {
        match &self.ex(e).kind {
            ExprKind::Lit(_) | ExprKind::Local(_) | ExprKind::None => vec![],
            ExprKind::Template(parts) => parts
                .iter()
                .filter_map(|p| match p {
                    TemplatePart::Expr(x) => Some(*x),
                    TemplatePart::Lit(_) => None,
                })
                .collect(),
            ExprKind::Call { args, .. }
            | ExprKind::ToolCall { args, .. }
            | ExprKind::Variant { args, .. }
            | ExprKind::List(args) => args.clone(),
            ExprKind::Method { recv, args, .. }
            | ExprKind::DynMethod { recv, args, .. }
            | ExprKind::MethodCall { recv, args, .. } => {
                std::iter::once(*recv).chain(args.iter().copied()).collect()
            }
            ExprKind::New { args, .. } => args.clone(),
            ExprKind::Unary { operand, .. } => vec![*operand],
            ExprKind::Binary { lhs, rhs, .. } => vec![*lhs, *rhs],
            ExprKind::Record { fields, .. } => fields.iter().map(|&(_, v)| v).collect(),
            ExprKind::Some(x)
            | ExprKind::Field { base: x, .. }
            | ExprKind::Validate { value: x, .. }
            | ExprKind::Approve { value: x, .. } => vec![*x],
            ExprKind::Index { base, index } => vec![*base, *index],
            ExprKind::Declassify { value, reason, .. } => vec![*value, *reason],
            ExprKind::If { cond, then, else_ } => {
                let mut out = vec![*cond];
                out.extend(then.tail);
                out.extend(else_.as_ref().and_then(|b| b.tail));
                out
            }
            ExprKind::Match { .. } | ExprKind::TryCatch { .. } | ExprKind::Block(_) => vec![],
        }
    }

    /// Evaluates `es` left to right. When a later one needs statements, earlier values
    /// are saved first so the statements can't run before them.
    fn operands(&mut self, es: &[ExprId]) -> Vec<Py> {
        let mut out = Vec::with_capacity(es.len());
        for (i, &e) in es.iter().enumerate() {
            let value = self.expr(e);
            let constant = matches!(self.ex(e).kind, ExprKind::Lit(_) | ExprKind::None);
            if !constant && es[i + 1..].iter().any(|&l| self.needs_stmts(l)) {
                let t = self.fresh();
                self.line(format!("{t} = {}", value.text));
                out.push(Py::atom(t));
            } else {
                out.push(value);
            }
        }
        out
    }

    fn hoist(&mut self, e: ExprId) -> Py {
        let t = self.fresh();
        self.expr_into(e, &Dest::Assign(t.clone()));
        Py::atom(t)
    }

    fn args(&mut self, es: &[ExprId]) -> String {
        self.operands(es)
            .iter()
            .map(|p| p.at(TERNARY))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn expr(&mut self, e: ExprId) -> Py {
        let f = self.f;
        let expr = &f.exprs[e];
        match &expr.kind {
            ExprKind::Lit(l) => lit(l),
            ExprKind::Local(l) => Py::atom(self.local(*l)),
            ExprKind::None => Py::atom("None"),
            ExprKind::Template(parts) => self.template(parts),
            ExprKind::Call { func, args } => {
                let name = self.scope.func(*func);
                // The checker proved these arguments trusted, so this call vouches for them.
                let trusted = self
                    .scope
                    .program
                    .func(*func)
                    .map(|f| f.trusted.clone())
                    .unwrap_or_default();
                let args = self
                    .operands(args)
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        if trusted.get(i).copied().unwrap_or(false) {
                            format!("_rt.Trusted({})", p.at(TERNARY))
                        } else {
                            p.at(TERNARY)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                self.awaited(format!("{name}({args})"))
            }
            ExprKind::ToolCall {
                tool,
                name,
                args,
                site,
                schema,
            } => {
                let source = self
                    .scope
                    .program
                    .tool(*tool)
                    .map_or("?", |t| t.source.as_str());
                let mut parts = vec![
                    names::string(source),
                    names::string(name),
                    names::string(&site.to_string()),
                ];
                let args = self.args(args);
                if !args.is_empty() {
                    parts.push(args);
                }
                if let Some(s) = schema {
                    let tuple = |items: Vec<String>| match items.len() {
                        1 => format!("({},)", items[0]),
                        _ => format!("({})", items.join(", ")),
                    };
                    if s.mcp_name != *name {
                        parts.push(format!("mcp_name={}", names::string(&s.mcp_name)));
                    }
                    parts.push(format!(
                        "names={}",
                        tuple(s.params.iter().map(|p| names::string(p)).collect())
                    ));
                    parts.push(format!(
                        "sinks={}",
                        tuple(
                            s.sinks
                                .iter()
                                .map(|&b| if b { "True" } else { "False" }.to_owned())
                                .collect()
                        )
                    ));
                    parts.push(format!("returns={}", self.scope.descriptor(&s.returns)));
                }
                let call = self.op("call_tool");
                self.awaited(format!("{call}({})", parts.join(", ")))
            }
            ExprKind::Method { method, recv, args } => self.method(*method, *recv, args),
            ExprKind::New { class, args } => {
                let name = self.scope.adt(*class);
                let init = self.init_of(*class);
                let (_, args) = self.method_args(init, None, args);
                if self.asyncio {
                    Py::new(format!("await {name}._new({args})"), UNARY)
                } else {
                    Py::atom(format!("{name}({args})"))
                }
            }
            ExprKind::MethodCall {
                method,
                recv,
                args,
                is_virtual,
            } => {
                let program = self.scope.program;
                let Some(m) = program.func(*method) else {
                    return Py::atom("None");
                };
                let is_init = m.method.is_some_and(|mo| mo.is_init);
                let py_name = method_name(m, self.asyncio);
                let (r, rest) = self.method_args(Some(*method), Some(*recv), args);
                let r = r.unwrap_or_default();
                if *is_virtual {
                    self.awaited(format!("{r}.{py_name}({rest})"))
                } else {
                    let class = m
                        .method
                        .map_or_else(|| "?".to_owned(), |mo| self.scope.adt(mo.class));
                    let args = if rest.is_empty() {
                        r
                    } else {
                        format!("{r}, {rest}")
                    };
                    let call = format!("{class}.{py_name}({args})");
                    if is_init && !self.asyncio {
                        Py::atom(call)
                    } else {
                        self.awaited(call)
                    }
                }
            }
            ExprKind::DynMethod { recv, name, args } => {
                let mut ops = self.operands(&[*recv]);
                let recv = ops.pop().map_or_else(String::new, |p| p.at(ATOM));
                let args = self.args(args);
                Py::atom(format!("{recv}.{}({args})", names::attr(name)))
            }
            ExprKind::Unary {
                op: UnOp::Not,
                operand,
            } if matches!(
                self.ex(*operand).kind,
                ExprKind::Method {
                    method: Method::StrContains | Method::ListContains | Method::MapContainsKey,
                    ..
                }
            ) =>
            {
                let ExprKind::Method { recv, args, .. } = &self.ex(*operand).kind else {
                    return Py::atom("None");
                };
                self.contains(*recv, args, true)
            }
            ExprKind::Unary { op, operand } => {
                let v = self.expr(*operand);
                match op {
                    UnOp::Not => Py::new(format!("not {}", v.at(NOT)), NOT),
                    UnOp::Neg => Py::new(format!("-{}", v.at(UNARY)), UNARY),
                }
            }
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, *lhs, *rhs),
            ExprKind::List(items) => Py::atom(format!("[{}]", self.args(items))),
            ExprKind::Record { record, fields } => {
                let class = self.scope.adt(*record);
                let decl: Vec<String> = self
                    .scope
                    .program
                    .record(*record)
                    .map(|r| r.fields.iter().map(|f| names::attr(&f.name)).collect())
                    .unwrap_or_default();
                let values: Vec<ExprId> = fields.iter().map(|&(_, v)| v).collect();
                let values = self.operands(&values);
                let args: Vec<String> = fields
                    .iter()
                    .zip(values)
                    .map(|(&(i, _), v)| {
                        let name = decl.get(i).map_or("?", String::as_str);
                        format!("{name}={}", v.at(TERNARY))
                    })
                    .collect();
                Py::atom(format!("{class}({})", args.join(", ")))
            }
            ExprKind::Variant { enum_, index, args } => {
                let class = self.scope.variant(*enum_, *index);
                let unit_only = self
                    .scope
                    .program
                    .enum_(*enum_)
                    .is_some_and(Enum::is_unit_only);
                if unit_only {
                    Py::atom(class)
                } else {
                    let args = self.args(args);
                    Py::atom(format!("{class}({args})"))
                }
            }
            ExprKind::Some(x) => {
                let v = self.expr(*x);
                if may_be_none(&f.exprs[*x].ty) {
                    Py::atom(format!("_rt.some({})", v.at(TERNARY)))
                } else {
                    v
                }
            }
            ExprKind::Field { base, record, name } => {
                let b = self.expr(*base);
                match record {
                    Some(_) => Py::atom(format!("{}.{}", b.at(ATOM), names::attr(name))),
                    None => Py::atom(format!(
                        "_rt.field({}, {})",
                        b.at(TERNARY),
                        names::string(name)
                    )),
                }
            }
            ExprKind::Index { base, index } => {
                let ops = self.operands(&[*base, *index]);
                let (b, i) = (ops[0].at(TERNARY), ops[1].at(TERNARY));
                match &f.exprs[*base].ty {
                    Ty::List(_) => Py::atom(format!("_rt.index({b}, {i})")),
                    Ty::Map(..) => Py::atom(format!("_rt.map_index({b}, {i})")),
                    _ => Py::atom(format!("{}[{i}]", ops[0].at(ATOM))),
                }
            }
            ExprKind::If { cond, then, else_ } if self.ternary(e) => {
                let (Some(then), Some(else_)) = (then.tail, else_.as_ref().and_then(|b| b.tail))
                else {
                    return self.hoist(e);
                };
                let c = self.expr(*cond);
                let a = self.expr(then);
                let b = self.expr(else_);
                Py::new(
                    format!("{} if {} else {}", a.at(OR), c.at(OR), b.at(TERNARY)),
                    TERNARY,
                )
            }
            ExprKind::Block(b) if b.stmts.is_empty() => match b.tail {
                Some(t) => self.expr(t),
                None => Py::atom("None"),
            },
            ExprKind::If { .. }
            | ExprKind::Match { .. }
            | ExprKind::TryCatch { .. }
            | ExprKind::Block(_) => self.hoist(e),
            ExprKind::Validate { value, rule, site } => {
                let v = self.expr(*value);
                let mut rule_fn = self.scope.func(*rule);
                let rule_def = self.scope.program.func(*rule);
                if rule_def.is_some_and(|f| f.trusted.first() == Some(&true)) {
                    rule_fn = format!("lambda _v: {rule_fn}(_rt.Trusted(_v))");
                }
                let rule_name = rule_def.map_or("?", |f| f.name.as_str());
                let call = self.op("validate");
                self.awaited(format!(
                    "{call}({}, {rule_fn}, {}, {})",
                    v.at(TERNARY),
                    names::string(rule_name),
                    names::string(&site.to_string())
                ))
            }
            ExprKind::Approve { value, site } => {
                let v = self.expr(*value);
                let call = self.op("approve");
                self.awaited(format!(
                    "{call}({}, {})",
                    v.at(TERNARY),
                    names::string(&site.to_string())
                ))
            }
            ExprKind::Declassify {
                value,
                reason,
                site,
            } => {
                let args = self.args(&[*value, *reason]);
                Py::atom(format!(
                    "_rt.declassify({args}, {})",
                    names::string(&site.to_string())
                ))
            }
        }
    }

    fn binary(&mut self, op: BinOp, lhs: ExprId, rhs: ExprId) -> Py {
        if matches!(op, BinOp::And | BinOp::Or) && self.needs_stmts(rhs) {
            // `a && { ... }`: only run the right side's statements when needed.
            let t = self.fresh();
            let l = self.expr(lhs);
            self.line(format!("{t} = {}", l.text));
            let test = if op == BinOp::And {
                t.clone()
            } else {
                format!("not {t}")
            };
            self.line(format!("if {test}:"));
            let dest = Dest::Assign(t.clone());
            self.nested(|g| g.expr_into(rhs, &dest));
            return Py::atom(t);
        }
        let ops = self.operands(&[lhs, rhs]);
        let (l, r) = (&ops[0], &ops[1]);
        let is_int = self.ex(lhs).ty == Ty::Int;
        let is_float = self.ex(lhs).ty == Ty::Float;
        let infix = |sym: &str, prec: u8| {
            // Comparisons chain in Python (`a == b == c`), so they take no comparison
            // operands unparenthesized; the others are left-associative.
            let left = if prec == CMP { CMP + 1 } else { prec };
            Py::new(format!("{} {sym} {}", l.at(left), r.at(prec + 1)), prec)
        };
        match op {
            BinOp::Or => infix("or", OR),
            BinOp::And => infix("and", AND),
            BinOp::Eq => infix("==", CMP),
            BinOp::Ne => infix("!=", CMP),
            BinOp::Lt => infix("<", CMP),
            BinOp::Le => infix("<=", CMP),
            BinOp::Gt => infix(">", CMP),
            BinOp::Ge => infix(">=", CMP),
            BinOp::Add => infix("+", ADD),
            BinOp::Sub => infix("-", ADD),
            BinOp::Mul => infix("*", MUL),
            BinOp::Div if is_int => {
                Py::atom(format!("_rt.idiv({}, {})", l.at(TERNARY), r.at(TERNARY)))
            }
            BinOp::Rem if is_int => {
                Py::atom(format!("_rt.irem({}, {})", l.at(TERNARY), r.at(TERNARY)))
            }
            BinOp::Div if is_float => {
                Py::atom(format!("_rt.fdiv({}, {})", l.at(TERNARY), r.at(TERNARY)))
            }
            BinOp::Rem if is_float => {
                Py::atom(format!("_rt.frem({}, {})", l.at(TERNARY), r.at(TERNARY)))
            }
            BinOp::Div => infix("/", MUL),
            BinOp::Rem => infix("%", MUL),
        }
    }

    fn method(&mut self, method: Method, recv: ExprId, args: &[ExprId]) -> Py {
        use Method::*;
        if matches!(method, StrContains | ListContains | MapContainsKey) {
            return self.contains(recv, args, false);
        }
        let mut all = vec![recv];
        all.extend_from_slice(args);
        let ops = self.operands(&all);
        let r = &ops[0];
        let a = |i: usize| ops.get(i + 1).map_or_else(String::new, |p| p.at(TERNARY));
        let call = |f: &str| Py::atom(format!("{f}({})", r.at(TERNARY)));
        let meth = |m: &str, args: String| Py::atom(format!("{}.{m}({args})", r.at(ATOM)));
        match method {
            StrLen | ListLen | MapLen => call("len"),
            StrIsEmpty | ListIsEmpty | MapIsEmpty => {
                Py::new(format!("len({}) == 0", r.at(TERNARY)), CMP)
            }
            StrContains | ListContains | MapContainsKey => Py::atom("None"),
            StartsWith => meth("startswith", a(0)),
            EndsWith => meth("endswith", a(0)),
            Trim => meth("strip", String::new()),
            Lower => meth("lower", String::new()),
            Upper => meth("upper", String::new()),
            Split => meth("split", a(0)),
            Lines => meth("splitlines", String::new()),
            Replace => meth("replace", format!("{}, {}", a(0), a(1))),
            ParseInt => call("_rt.parse_int"),
            ParseFloat => call("_rt.parse_float"),
            ListGet => Py::atom(format!("_rt.list_get({}, {})", r.at(TERNARY), a(0))),
            ListFirst => call("_rt.first"),
            ListLast => call("_rt.last"),
            ListPush => Py::atom(format!("[*{}, {}]", r.at(ATOM), a(0))),
            MapGet => Py::atom(format!("_rt.map_get({}, {})", r.at(TERNARY), a(0))),
            MapKeys => call("list"),
            MapValues => Py::atom(format!("list({}.values())", r.at(ATOM))),
            MapInsert => Py::atom(format!("{{**{}, {}: {}}}", r.at(ATOM), a(0), a(1))),
            IsSome => Py::new(format!("{} is not None", r.at(CMP + 1)), CMP),
            IsNone => Py::new(format!("{} is None", r.at(CMP + 1)), CMP),
            UnwrapOr => Py::atom(format!("_rt.unwrap_or({}, {})", r.at(TERNARY), a(0))),
            ToString if self.ex(recv).ty == Ty::Bool => call("_rt.to_str"),
            ToString => call("str"),
            ToFloat => call("float"),
            Round => call("_rt.round_half_away"),
        }
    }

    /// `x in xs`, or `x not in xs`.
    fn contains(&mut self, recv: ExprId, args: &[ExprId], negate: bool) -> Py {
        let op = if negate { "not in" } else { "in" };
        let Some(&arg) = args.first() else {
            return Py::atom("None");
        };
        // Python evaluates `x` before `xs`; save `xs` first if the order could matter.
        let r = if self.is_pure(recv) || self.is_pure(arg) {
            let ops = self.operands(&[recv, arg]);
            let a = ops[1].at(CMP + 1);
            return Py::new(format!("{a} {op} {}", ops[0].at(CMP + 1)), CMP);
        } else {
            let r = self.expr(recv);
            let t = self.fresh();
            self.line(format!("{t} = {}", r.text));
            t
        };
        let a = self.expr(arg);
        Py::new(format!("{} {op} {r}", a.at(CMP + 1)), CMP)
    }

    /// No calls: evaluating it has no effects and can't be affected by others'.
    fn is_pure(&self, e: ExprId) -> bool {
        match &self.ex(e).kind {
            ExprKind::Lit(_) | ExprKind::Local(_) | ExprKind::None => true,
            // An object's fields can change under it.
            ExprKind::Field {
                base,
                record: Some(d),
                ..
            } => self.scope.program.class(*d).is_none() && self.is_pure(*base),
            _ => false,
        }
    }

    /// The `init` constructing `class` runs: its own or the nearest base's.
    fn init_of(&self, class: DefId) -> Option<DefId> {
        let program = self.scope.program;
        let mut cur = Some(class);
        while let Some(c) = cur.and_then(|d| program.class(d)) {
            let own = c.methods.iter().copied().find(|&m| {
                program
                    .func(m)
                    .is_some_and(|f| f.method.is_some_and(|mo| mo.is_init))
            });
            if own.is_some() {
                return own;
            }
            cur = c.base;
        }
        None
    }

    /// The receiver and the arguments of a method call, vouching for the arguments the
    /// checker proved trusted.
    fn method_args(
        &mut self,
        method: Option<DefId>,
        recv: Option<ExprId>,
        args: &[ExprId],
    ) -> (Option<String>, String) {
        let trusted = method
            .and_then(|m| self.scope.program.func(m))
            .map(|f| f.trusted.clone())
            .unwrap_or_default();
        let all: Vec<ExprId> = recv.into_iter().chain(args.iter().copied()).collect();
        let ops = self.operands(&all);
        let mut ops = ops.into_iter();
        let recv = recv.and_then(|_| ops.next()).map(|p| p.at(ATOM));
        let rest: Vec<String> = ops
            .enumerate()
            .map(|(i, p)| {
                // Parameter 0 is `self`.
                if trusted.get(i + 1).copied().unwrap_or(false) {
                    format!("_rt.Trusted({})", p.at(TERNARY))
                } else {
                    p.at(TERNARY)
                }
            })
            .collect();
        (recv, rest.join(", "))
    }

    fn template(&mut self, parts: &[TemplatePart]) -> Py {
        let exprs: Vec<ExprId> = parts
            .iter()
            .filter_map(|p| match p {
                TemplatePart::Expr(x) => Some(*x),
                TemplatePart::Lit(_) => None,
            })
            .collect();
        let values = self.operands(&exprs);
        let mut pieces = Vec::new();
        let mut values = values.into_iter();
        for part in parts {
            match part {
                TemplatePart::Lit(s) => pieces.push((true, s.clone())),
                TemplatePart::Expr(x) => {
                    let v = values.next().unwrap_or_else(|| Py::atom("None"));
                    let text = match self.ex(*x).ty {
                        Ty::String | Ty::Int | Ty::Float => v.at(TERNARY),
                        _ => format!("_rt.to_str({})", v.at(TERNARY)),
                    };
                    pieces.push((false, text));
                }
            }
        }
        // An f-string when every interpolation is plain enough to nest in one.
        let plain = pieces.iter().all(|(is_lit, text)| {
            *is_lit || !text.contains(['"', '\'', '\\', '{', '}', '\n', '!', ':'])
        });
        if plain {
            let mut body = String::new();
            for (is_lit, text) in &pieces {
                if *is_lit {
                    let s = names::string(text);
                    let inner = s.get(1..s.len() - 1).unwrap_or("");
                    body.push_str(&inner.replace('{', "{{").replace('}', "}}"));
                } else {
                    body.push('{');
                    body.push_str(text);
                    body.push('}');
                }
            }
            return Py::atom(format!("f\"{body}\""));
        }
        let joined: Vec<String> = pieces
            .into_iter()
            .map(
                |(is_lit, text)| {
                    if is_lit { names::string(&text) } else { text }
                },
            )
            .collect();
        Py::atom(format!("\"\".join([{}])", joined.join(", ")))
    }
}

/// The Python name of a method: `__init__` for `init` (`_init` in async code, since
/// constructors can't await).
pub fn method_name(f: &Fn, asyncio: bool) -> String {
    match f.method {
        Some(m) if m.is_init => if asyncio { "_init" } else { "__init__" }.to_owned(),
        _ => names::attr(&f.name),
    }
}

fn lit(l: &Lit) -> Py {
    match l {
        Lit::Int(i) => Py::atom(i.to_string()),
        Lit::Float(f) => Py::atom(match f.parse::<f64>() {
            Ok(v) if v.is_finite() => format!("{v:?}"),
            _ => f.clone(),
        }),
        Lit::Str(s) => Py::atom(names::string(s)),
        Lit::Bool(true) => Py::atom("True"),
        Lit::Bool(false) => Py::atom("False"),
    }
}
