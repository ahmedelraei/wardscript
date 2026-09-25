//! Function bodies. Wardscript is expression-oriented and TypeScript mostly isn't, so
//! an `if`, `match`, `try` or block whose value is used becomes statements that store
//! the value in its destination (a variable, `return`, or nowhere). Operands evaluated
//! before a hoisted expression are saved to temporaries first, keeping left-to-right
//! order. Every variable is declared at the top of the function (`let x!: T;`).

use std::collections::HashSet;

use ward_ir::*;

use crate::names::{self, Scope, may_be_null, primitive};

// TypeScript operator precedence, loosest first.
const TERNARY: u8 = 1;
const OR: u8 = 2;
const AND: u8 = 3;
const EQ: u8 = 4;
const CMP: u8 = 5;
const ADD: u8 = 6;
const MUL: u8 = 7;
const UNARY: u8 = 8;
const ATOM: u8 = 9;

/// A TypeScript expression and how tightly it binds.
struct Ts {
    text: String,
    prec: u8,
}

impl Ts {
    fn new(text: impl Into<String>, prec: u8) -> Ts {
        Ts {
            text: text.into(),
            prec,
        }
    }

    fn atom(text: impl Into<String>) -> Ts {
        Ts::new(text, ATOM)
    }

    fn at(&self, min: u8) -> String {
        if self.prec >= min {
            self.text.clone()
        } else {
            format!("({})", self.text)
        }
    }
}

#[derive(Clone)]
enum Dest {
    Return,
    Assign(String),
    Discard,
}

pub struct FnGen<'a, 'p> {
    f: &'a Fn,
    scope: &'a mut Scope<'p>,
    generics: Vec<String>,
    locals: Vec<String>,
    taken: HashSet<String>,
    tmp: u32,
    pub indent: usize,
    pub lines: Vec<String>,
    /// Temporaries made while generating, declared with the locals.
    temps: Vec<String>,
    /// Whether the body may `await` (everything but refinements).
    pub asyncio: bool,
}

impl<'a, 'p> FnGen<'a, 'p> {
    pub fn new(f: &'a Fn, scope: &'a mut Scope<'p>, module_names: &HashSet<String>) -> Self {
        let mut taken = module_names.clone();
        taken.insert("_rt".to_owned());
        taken.extend(f.generics.iter().cloned());
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
            generics: f.generics.clone(),
            f,
            scope,
            locals,
            taken,
            tmp: 0,
            indent: 1,
            lines: Vec::new(),
            temps: Vec::new(),
            asyncio: true,
        }
    }

    pub fn local(&self, l: LocalId) -> &str {
        self.locals
            .get(l.into_raw().into_u32() as usize)
            .map_or("_", String::as_str)
    }

    fn line(&mut self, text: impl AsRef<str>) {
        self.lines
            .push(format!("{}{}", "  ".repeat(self.indent), text.as_ref()));
    }

    fn fresh(&mut self) -> String {
        loop {
            self.tmp += 1;
            let name = format!("_t{}", self.tmp);
            if self.taken.insert(name.clone()) {
                self.temps.push(name.clone());
                return name;
            }
        }
    }

    fn nested(&mut self, f: impl FnOnce(&mut Self)) {
        self.indent += 1;
        f(self);
        self.indent -= 1;
    }

    /// `head {`, the nested lines, `}`.
    fn braced(&mut self, head: impl AsRef<str>, f: impl FnOnce(&mut Self)) {
        self.line(format!("{} {{", head.as_ref()));
        self.nested(f);
        self.line("}");
    }

    fn ann(&mut self, ty: &Ty) -> String {
        let generics = self.generics.clone();
        self.scope.annotation(ty, &generics)
    }

    /// `let x!: T;` for every variable but the parameters, and the temporaries.
    pub fn declarations(&mut self) -> Vec<String> {
        let f = self.f;
        let params: HashSet<LocalId> = f.params.iter().copied().collect();
        let mut out = Vec::new();
        for (id, local) in f.locals.iter() {
            if params.contains(&id) {
                continue;
            }
            let ty = if local.ty == Ty::Unit {
                "null".to_owned()
            } else {
                self.ann(&local.ty)
            };
            out.push(format!("let {}!: {ty};", self.local(id)));
        }
        for t in &self.temps {
            out.push(format!("let {t}!: any;"));
        }
        out
    }

    pub fn body(&mut self) {
        let f = self.f;
        for (&p, _) in f.params.iter().zip(&f.trusted).filter(|(_, t)| **t) {
            let name = self.local(p).to_owned();
            self.line(format!(
                "{name} = _rt.vouched({name}, {}, {});",
                names::string(&f.locals[p].name),
                names::string(&f.name)
            ));
        }
        match &f.body {
            Body::Abstract => self.line(format!("throw new Error({});", names::string(&f.name))),
            Body::Block(b) => {
                let dest = if matches!(f.ret, Ty::Unit) {
                    Dest::Discard
                } else {
                    Dest::Return
                };
                self.block_into(b, &dest);
            }
            Body::Ai { prompt, checks, it } => {
                let check = match (checks.is_empty(), it) {
                    (false, Some(it)) => {
                        let name = self.fresh();
                        let it_ty = self.ann(&f.locals[*it].ty);
                        let it = self.local(*it).to_owned();
                        // `it` is the check's own parameter, not the declared variable.
                        self.line(format!(
                            "{name} = async ({it}: {it_ty}): Promise<string | null> => {{"
                        ));
                        self.nested(|g| {
                            for c in checks {
                                let cond = g.expr(c.cond);
                                g.line(format!(
                                    "if (!{}) return {};",
                                    cond.at(ATOM),
                                    names::string(&c.reason)
                                ));
                            }
                            g.line("return null;");
                        });
                        self.line("};");
                        Some(name)
                    }
                    _ => None,
                };
                let prompt = self.expr(*prompt);
                let returns = self.scope.descriptor(&f.ret);
                let mut options = Vec::new();
                if let Some(m) = &f.model {
                    if m.models.len() > 1 || m.models.first().is_some_and(Option::is_some) {
                        let models: Vec<String> = m
                            .models
                            .iter()
                            .map(|a| a.as_deref().map_or("null".to_owned(), names::string))
                            .collect();
                        options.push(format!("models: [{}]", models.join(", ")));
                    }
                    if let Some(r) = m.retries {
                        options.push(format!("retries: {r}"));
                    }
                    if let Some(b) = m.backoff {
                        options.push(format!("backoff: {b:?}"));
                    }
                }
                if let Some(check) = check {
                    options.push(format!("check: {check}"));
                }
                let options = if options.is_empty() {
                    String::new()
                } else {
                    format!(", {{ {} }}", options.join(", "))
                };
                let ret = self.ann(&f.ret);
                self.line(format!(
                    "return (await _rt.ai({}, {}, {returns}{options})) as {ret};",
                    names::string(&f.name),
                    prompt.text
                ));
            }
        }
    }

    fn awaited(&self, call: String) -> Ts {
        if self.asyncio {
            Ts::new(format!("await {call}"), UNARY)
        } else {
            Ts::atom(call)
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
                if !diverges {
                    match dest {
                        Dest::Assign(name) => self.line(format!("{name} = null;")),
                        Dest::Return => self.line("return null;"),
                        Dest::Discard => {}
                    }
                }
            }
        }
    }

    fn emit(&mut self, value: Ts, dest: &Dest, e: ExprId) {
        match dest {
            Dest::Return => {
                if matches!(self.f.ret, Ty::Unit) {
                    self.emit(value, &Dest::Discard, e);
                    self.line("return;");
                } else {
                    self.line(format!("return {};", value.text));
                }
            }
            Dest::Assign(name) => self.line(format!("{name} = {};", value.at(TERNARY))),
            Dest::Discard => {
                if !matches!(
                    self.ex(e).kind,
                    ExprKind::Local(_) | ExprKind::Lit(_) | ExprKind::None
                ) {
                    self.line(format!("{};", value.text));
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
        let mut first = true;
        loop {
            let ExprKind::If { cond, then, else_ } = &f.exprs[e].kind else {
                return;
            };
            // Only the first condition may need statements: later ones were checked.
            let cond = self.expr(*cond);
            if first {
                self.line(format!("if ({}) {{", cond.text));
            } else {
                self.line(format!("}} else if ({}) {{", cond.text));
            }
            first = false;
            let Some(else_) = else_ else {
                self.nested(|g| g.block_into(then, &Dest::Discard));
                self.line("}");
                // Without `else`, the value is `()`.
                if !matches!(dest, Dest::Discard) {
                    self.emit(Ts::atom("null"), dest, e);
                }
                return;
            };
            self.nested(|g| g.block_into(then, dest));
            if let (true, Some(next)) = (else_.stmts.is_empty(), else_.tail) {
                if let ExprKind::If { cond, .. } = &f.exprs[next].kind {
                    if !self.needs_stmts(*cond) {
                        e = next;
                        continue;
                    }
                }
            }
            self.line("} else {");
            self.nested(|g| g.block_into(else_, dest));
            self.line("}");
            return;
        }
    }

    fn match_into(&mut self, scrutinee: ExprId, arms: &[Arm], dest: &Dest) {
        let value = self.expr(scrutinee);
        let subject = if self.is_pure(scrutinee) {
            value.at(ATOM)
        } else {
            let t = self.fresh();
            self.line(format!("{t} = {};", value.text));
            t
        };
        if arms.is_empty() {
            self.line("throw new Error(\"unreachable: match on a type with no values\");");
            return;
        }
        let last = arms.len() - 1;
        for (i, arm) in arms.iter().enumerate() {
            let mut conds = Vec::new();
            let mut binds = Vec::new();
            self.pat(arm.pat, &subject, &mut conds, &mut binds);
            let arm_body = |g: &mut Self| {
                for (name, access) in &binds {
                    g.line(format!("{name} = {access};"));
                }
                g.expr_into(arm.body, dest);
            };
            if i == 0 && conds.is_empty() {
                arm_body(self);
                return;
            }
            if i == last || conds.is_empty() {
                self.line("} else {");
                self.nested(arm_body);
                self.line("}");
                return;
            }
            let head = if i == 0 {
                format!("if ({}) {{", conds.join(" && "))
            } else {
                format!("}} else if ({}) {{", conds.join(" && "))
            };
            self.line(head);
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
            PatKind::Lit(Lit::Bool(true)) => conds.push(format!("{access} === true")),
            PatKind::Lit(Lit::Bool(false)) => conds.push(format!("{access} === false")),
            PatKind::Lit(l) => conds.push(format!("{access} === {}", lit(l).text)),
            PatKind::None => conds.push(format!("{access} === null")),
            PatKind::Some(sub) => {
                conds.push(format!("{access} !== null"));
                let inner = if may_be_null(&f.pats[*sub].ty) {
                    format!("_rt.unwrap({access})")
                } else {
                    format!("({access} as any)")
                };
                self.pat(*sub, &inner, conds, binds);
            }
            PatKind::Variant { enum_, index, args } => {
                let name = self
                    .scope
                    .program
                    .enum_(*enum_)
                    .and_then(|e| e.variants.get(*index))
                    .map_or("?", |v| v.name.as_str());
                if self.scope.is_unit_enum(*enum_) {
                    conds.push(format!("{access} === {}", names::string(name)));
                } else {
                    conds.push(format!("({access} as any).tag === {}", names::string(name)));
                }
                for (i, &sub) in args.iter().enumerate() {
                    self.pat(sub, &format!("({access} as any)._{i}"), conds, binds);
                }
            }
        }
    }

    fn try_into(&mut self, body: &Block, err: Option<LocalId>, handler: &Block, dest: &Dest) {
        self.line("try {");
        self.nested(|g| g.block_into(body, dest));
        let e = self.fresh();
        self.temps.retain(|t| *t != e); // A `catch` binding, not a variable.
        self.line(format!("}} catch ({e}) {{"));
        self.nested(|g| {
            g.line(format!("if (!({e} instanceof _rt.Thrown)) throw {e};"));
            if let Some(l) = err {
                let name = g.local(l).to_owned();
                g.line(format!("{name} = {e}.value as any;"));
            }
            g.block_into(handler, dest);
        });
        self.line("}");
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
            Stmt::Return(None) => self.line("return;"),
            Stmt::Return(Some(e)) => {
                if matches!(f.ret, Ty::Unit) {
                    self.expr_into(*e, &Dest::Discard);
                    self.line("return;");
                } else {
                    self.expr_into(*e, &Dest::Return);
                }
            }
            Stmt::Throw(e) => {
                let value = self.expr(*e);
                self.line(format!("throw new _rt.Thrown({});", value.text));
            }
            Stmt::For { local, iter, body } => {
                let it = self.expr(*iter);
                let name = self.local(*local).to_owned();
                let source = match &f.exprs[*iter].ty {
                    Ty::Map(..) => format!("{}.keys()", it.at(ATOM)),
                    _ => it.text,
                };
                self.braced(format!("for ({name} of {source})"), |g| {
                    g.block_into(body, &Dest::Discard);
                });
            }
            Stmt::While { cond, body } => {
                if self.needs_stmts(*cond) {
                    self.braced("while (true)", |g| {
                        let c = g.expr(*cond);
                        g.line(format!("if (!{}) break;", c.at(ATOM)));
                        g.block_into(body, &Dest::Discard);
                    });
                } else {
                    let c = self.expr(*cond);
                    self.braced(format!("while ({})", c.text), |g| {
                        g.block_into(body, &Dest::Discard);
                    });
                }
            }
            Stmt::Assert {
                cond,
                message,
                site,
            } => {
                let c = self.expr(*cond);
                self.line(format!(
                    "if (!{}) throw new _rt.TestFailure({}, {});",
                    c.at(ATOM),
                    names::string(message),
                    names::string(&site.to_string())
                ));
            }
        }
    }

    /// `x.a[i] = v` rebinds `x` to a copy.
    fn assign(&mut self, local: LocalId, path: &[Place], value: ExprId) {
        let name = self.local(local).to_owned();
        if path.is_empty() {
            self.expr_into(value, &Dest::Assign(name));
            return;
        }
        let new = self.updated(&name, path, value);
        self.line(format!("{name} = {new};"));
    }

    /// `obj.field.a = v` sets the field in place, to a copy with `a` replaced.
    fn set_field(&mut self, obj: ExprId, field: &str, path: &[Place], value: ExprId) {
        // The object is evaluated before the value, as written.
        let o = if matches!(self.ex(obj).kind, ExprKind::Local(_)) {
            self.expr(obj).at(ATOM)
        } else {
            let t = self.fresh();
            self.expr_into(obj, &Dest::Assign(t.clone()));
            t
        };
        let target = format!("{o}.{field}");
        if path.is_empty() {
            let v = if self.needs_stmts(value) {
                self.hoist(value)
            } else {
                self.expr(value)
            };
            self.line(format!("{target} = {};", v.at(TERNARY)));
            return;
        }
        let new = self.updated(&target, path, value);
        self.line(format!("{target} = {new};"));
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
                    self.line(format!("{t} = {};", k.text));
                    keys.push(t);
                }
            }
        }
        let value = self.expr(value);
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
        let mut new = value.at(TERNARY);
        for ((place, k), c) in places.iter().zip(&containers).rev() {
            new = self.replaced(c, place, k, &new);
        }
        new
    }

    fn access(&self, c: &str, place: &Place, key: &str) -> String {
        match place {
            Place::Field {
                record: Some(_),
                name,
            } => format!("{c}.{name}"),
            Place::Field { record: None, name } => {
                format!("_rt.field({c}, {})", names::string(name))
            }
            Place::Index { container, .. } => match container {
                Ty::List(_) => format!("_rt.index({c}, {key})"),
                Ty::Map(..) => format!("_rt.mapIndex({c}, {key})"),
                _ => format!("_rt.field({c}, {key})"),
            },
        }
    }

    fn replaced(&self, c: &str, place: &Place, key: &str, value: &str) -> String {
        match place {
            Place::Field {
                record: Some(_),
                name,
            } => format!("{{ ...{c}, {name}: {value} }}"),
            Place::Field { record: None, name } => {
                format!("_rt.withField({c}, {}, {value})", names::string(name))
            }
            Place::Index { container, .. } => match container {
                Ty::List(_) => format!("_rt.listSet({c}, {key}, {value})"),
                Ty::Map(..) => format!("_rt.mapSet({c}, {key}, {value})"),
                _ => format!("_rt.withField({c}, {key}, {value})"),
            },
        }
    }

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

    fn operands(&mut self, es: &[ExprId]) -> Vec<Ts> {
        let mut out = Vec::with_capacity(es.len());
        for (i, &e) in es.iter().enumerate() {
            let value = self.expr(e);
            let constant = matches!(self.ex(e).kind, ExprKind::Lit(_) | ExprKind::None);
            if !constant && es[i + 1..].iter().any(|&l| self.needs_stmts(l)) {
                let t = self.fresh();
                self.line(format!("{t} = {};", value.text));
                out.push(Ts::atom(t));
            } else {
                out.push(value);
            }
        }
        out
    }

    fn hoist(&mut self, e: ExprId) -> Ts {
        let t = self.fresh();
        self.expr_into(e, &Dest::Assign(t.clone()));
        Ts::atom(t)
    }

    fn args(&mut self, es: &[ExprId]) -> String {
        self.operands(es)
            .iter()
            .map(|p| p.at(TERNARY))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn expr(&mut self, e: ExprId) -> Ts {
        let f = self.f;
        let expr = &f.exprs[e];
        match &expr.kind {
            ExprKind::Lit(l) => lit(l),
            ExprKind::Local(l) => Ts::atom(self.local(*l)),
            ExprKind::None => Ts::atom("null"),
            ExprKind::Template(parts) => self.template(parts),
            ExprKind::New { class, args } => {
                let ctor = self.scope.constructor(*class);
                let init = crate::init_of(self.scope.program, *class);
                let args = self.method_args(init, args);
                self.awaited(format!("{ctor}({args})"))
            }
            ExprKind::MethodCall {
                method,
                recv,
                args,
                is_virtual,
            } => {
                let program = self.scope.program;
                let Some(m) = program.func(*method) else {
                    return Ts::atom("null");
                };
                let name = crate::method_name(program, m);
                let mut ops = self.operands(&[*recv]);
                let r = ops.pop().map_or_else(String::new, |p| p.at(ATOM));
                let args = self.method_args(Some(*method), args);
                if *is_virtual {
                    self.awaited(format!("{r}.{name}({args})"))
                } else {
                    let class = m
                        .method
                        .map_or_else(|| "?".to_owned(), |mo| self.scope.adt(mo.class));
                    let args = if args.is_empty() {
                        r
                    } else {
                        format!("{r}, {args}")
                    };
                    self.awaited(format!("{class}.prototype.{name}.call({args})"))
                }
            }
            ExprKind::Call { func, args } => {
                let name = self.scope.func(*func);
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
                            format!("_rt.trusted({})", p.at(TERNARY))
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
                    .map_or("?", |t| t.source.as_str())
                    .to_owned();
                let args = self.args(args);
                let mut options = Vec::new();
                if let Some(s) = schema {
                    if s.mcp_name != *name {
                        options.push(format!("mcpName: {}", names::string(&s.mcp_name)));
                    }
                    let ns: Vec<String> = s.params.iter().map(|p| names::string(p)).collect();
                    options.push(format!("names: [{}]", ns.join(", ")));
                    let sinks: Vec<&str> = s
                        .sinks
                        .iter()
                        .map(|&b| if b { "true" } else { "false" })
                        .collect();
                    options.push(format!("sinks: [{}]", sinks.join(", ")));
                    options.push(format!("returns: {}", self.scope.descriptor(&s.returns)));
                }
                let options = if options.is_empty() {
                    String::new()
                } else {
                    format!(", {{ {} }}", options.join(", "))
                };
                let ty = self.ann(&expr.ty);
                Ts::new(
                    format!(
                        "(await _rt.callTool({}, {}, {}, [{args}]{options})) as {ty}",
                        names::string(&source),
                        names::string(name),
                        names::string(&site.to_string())
                    ),
                    UNARY,
                )
            }
            ExprKind::Method { method, recv, args } => self.method(*method, *recv, args),
            ExprKind::DynMethod { recv, name, args } => {
                let mut ops = self.operands(&[*recv]);
                let recv = ops.pop().map_or_else(String::new, |p| p.at(ATOM));
                let args = self.args(args);
                Ts::atom(format!("({recv} as any).{name}({args})"))
            }
            ExprKind::Unary { op, operand } => {
                let v = self.expr(*operand);
                match op {
                    UnOp::Not => Ts::new(format!("!{}", v.at(UNARY)), UNARY),
                    UnOp::Neg => Ts::new(format!("-{}", v.at(UNARY)), UNARY),
                }
            }
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, *lhs, *rhs),
            ExprKind::List(items) => Ts::atom(format!("[{}]", self.args(items))),
            ExprKind::Record { record, fields } => {
                let decl: Vec<String> = self
                    .scope
                    .program
                    .record(*record)
                    .map(|r| r.fields.iter().map(|f| f.name.clone()).collect())
                    .unwrap_or_default();
                let values: Vec<ExprId> = fields.iter().map(|&(_, v)| v).collect();
                let values = self.operands(&values);
                let parts: Vec<String> = fields
                    .iter()
                    .zip(values)
                    .map(|(&(i, _), v)| {
                        let name = decl.get(i).map_or("?", String::as_str);
                        format!("{name}: {}", v.at(TERNARY))
                    })
                    .collect();
                let ty = self.ann(&expr.ty);
                Ts::atom(format!("({{ {} }} as {ty})", parts.join(", ")))
            }
            ExprKind::Variant { enum_, index, args } => {
                let name = self
                    .scope
                    .program
                    .enum_(*enum_)
                    .and_then(|e| e.variants.get(*index))
                    .map_or("?", |v| v.name.as_str())
                    .to_owned();
                if self.scope.is_unit_enum(*enum_) {
                    Ts::atom(names::string(&name))
                } else {
                    let ops = self.operands(args);
                    let mut parts = vec![format!("tag: {}", names::string(&name))];
                    parts.extend(
                        ops.iter()
                            .enumerate()
                            .map(|(i, v)| format!("_{i}: {}", v.at(TERNARY))),
                    );
                    let ty = self.ann(&expr.ty);
                    Ts::atom(format!(
                        "_rt.variant<{ty}>({{ {} }} as {ty})",
                        parts.join(", ")
                    ))
                }
            }
            ExprKind::Some(x) => {
                let v = self.expr(*x);
                if may_be_null(&f.exprs[*x].ty) {
                    Ts::atom(format!("_rt.some({})", v.at(TERNARY)))
                } else {
                    v
                }
            }
            ExprKind::Field { base, record, name } => {
                let b = self.expr(*base);
                match record {
                    Some(_) => Ts::atom(format!("{}.{name}", b.at(ATOM))),
                    None => Ts::atom(format!(
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
                    Ty::List(_) => Ts::atom(format!("_rt.index({b}, {i})")),
                    Ty::Map(..) => Ts::atom(format!("_rt.mapIndex({b}, {i})")),
                    _ => Ts::atom(format!("_rt.field({b}, {i})")),
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
                Ts::new(
                    format!("{} ? {} : {}", c.at(OR), a.at(TERNARY), b.at(TERNARY)),
                    TERNARY,
                )
            }
            ExprKind::Block(b) if b.stmts.is_empty() => match b.tail {
                Some(t) => self.expr(t),
                None => Ts::atom("null"),
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
                    rule_fn = format!("(_v: any) => {rule_fn}(_rt.trusted(_v))");
                }
                let rule_name = rule_def.map_or("?", |f| f.name.as_str()).to_owned();
                self.awaited(format!(
                    "_rt.validate({}, {rule_fn}, {}, {})",
                    v.at(TERNARY),
                    names::string(&rule_name),
                    names::string(&site.to_string())
                ))
            }
            ExprKind::Approve { value, site } => {
                let v = self.expr(*value);
                self.awaited(format!(
                    "_rt.approve({}, {})",
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
                Ts::atom(format!(
                    "_rt.declassify({args}, {})",
                    names::string(&site.to_string())
                ))
            }
        }
    }

    fn binary(&mut self, op: BinOp, lhs: ExprId, rhs: ExprId) -> Ts {
        if matches!(op, BinOp::And | BinOp::Or) && self.needs_stmts(rhs) {
            let t = self.fresh();
            let l = self.expr(lhs);
            self.line(format!("{t} = {};", l.text));
            let test = if op == BinOp::And {
                t.clone()
            } else {
                format!("!{t}")
            };
            let dest = Dest::Assign(t.clone());
            self.braced(format!("if ({test})"), |g| g.expr_into(rhs, &dest));
            return Ts::atom(t);
        }
        let ops = self.operands(&[lhs, rhs]);
        let (l, r) = (&ops[0], &ops[1]);
        let ty = self.ex(lhs).ty.clone();
        let is_int = ty == Ty::Int;
        let is_float = ty == Ty::Float;
        let prim = primitive(&ty, self.scope);
        let infix =
            |sym: &str, prec: u8| Ts::new(format!("{} {sym} {}", l.at(prec), r.at(prec + 1)), prec);
        match op {
            BinOp::Or => infix("||", OR),
            BinOp::And => infix("&&", AND),
            BinOp::Eq if prim => infix("===", EQ),
            BinOp::Ne if prim => infix("!==", EQ),
            BinOp::Eq => Ts::atom(format!("_rt.eq({}, {})", l.at(TERNARY), r.at(TERNARY))),
            BinOp::Ne => Ts::new(
                format!("!_rt.eq({}, {})", l.at(TERNARY), r.at(TERNARY)),
                UNARY,
            ),
            BinOp::Lt => infix("<", CMP),
            BinOp::Le => infix("<=", CMP),
            BinOp::Gt => infix(">", CMP),
            BinOp::Ge => infix(">=", CMP),
            BinOp::Add => infix("+", ADD),
            BinOp::Sub => infix("-", ADD),
            BinOp::Mul => infix("*", MUL),
            BinOp::Div if is_int => {
                Ts::atom(format!("_rt.idiv({}, {})", l.at(TERNARY), r.at(TERNARY)))
            }
            BinOp::Rem if is_int => {
                Ts::atom(format!("_rt.irem({}, {})", l.at(TERNARY), r.at(TERNARY)))
            }
            BinOp::Rem if is_float => {
                Ts::atom(format!("_rt.frem({}, {})", l.at(TERNARY), r.at(TERNARY)))
            }
            BinOp::Div => infix("/", MUL),
            BinOp::Rem => infix("%", MUL),
        }
    }

    fn method(&mut self, method: Method, recv: ExprId, args: &[ExprId]) -> Ts {
        use Method::*;
        let mut all = vec![recv];
        all.extend_from_slice(args);
        let ops = self.operands(&all);
        let r = &ops[0];
        let a = |i: usize| ops.get(i + 1).map_or_else(String::new, |p| p.at(TERNARY));
        let call = |f: &str| Ts::atom(format!("{f}({})", r.at(TERNARY)));
        let meth = |m: &str, args: String| Ts::atom(format!("{}.{m}({args})", r.at(ATOM)));
        let elem_prim = match &self.ex(recv).ty {
            Ty::List(t) => primitive(t, self.scope),
            _ => true,
        };
        match method {
            StrLen => call("_rt.strLen"),
            ListLen => Ts::atom(format!("{}.length", r.at(ATOM))),
            MapLen => Ts::atom(format!("{}.size", r.at(ATOM))),
            StrIsEmpty | ListIsEmpty => Ts::new(format!("{}.length === 0", r.at(ATOM)), EQ),
            MapIsEmpty => Ts::new(format!("{}.size === 0", r.at(ATOM)), EQ),
            StrContains => meth("includes", a(0)),
            ListContains if elem_prim => meth("includes", a(0)),
            ListContains => Ts::atom(format!("{}.some((_x) => _rt.eq(_x, {}))", r.at(ATOM), a(0))),
            MapContainsKey => meth("has", a(0)),
            StartsWith => meth("startsWith", a(0)),
            EndsWith => meth("endsWith", a(0)),
            Trim => meth("trim", String::new()),
            Lower => meth("toLowerCase", String::new()),
            Upper => meth("toUpperCase", String::new()),
            Split => meth("split", a(0)),
            Lines => call("_rt.lines"),
            Replace => meth("replaceAll", format!("{}, {}", a(0), a(1))),
            ListGet => Ts::atom(format!("_rt.listGet({}, {})", r.at(TERNARY), a(0))),
            ListFirst => call("_rt.first"),
            ListLast => call("_rt.last"),
            ListPush => Ts::atom(format!("[...{}, {}]", r.at(ATOM), a(0))),
            MapGet => Ts::atom(format!("_rt.mapGet({}, {})", r.at(TERNARY), a(0))),
            MapKeys => Ts::atom(format!("[...{}.keys()]", r.at(ATOM))),
            MapValues => Ts::atom(format!("[...{}.values()]", r.at(ATOM))),
            MapInsert => Ts::atom(format!("_rt.mapSet({}, {}, {})", r.at(TERNARY), a(0), a(1))),
            IsSome => Ts::new(format!("{} !== null", r.at(EQ + 1)), EQ),
            IsNone => Ts::new(format!("{} === null", r.at(EQ + 1)), EQ),
            UnwrapOr => Ts::atom(format!("_rt.unwrapOr({}, {})", r.at(TERNARY), a(0))),
            ToString if self.ex(recv).ty == Ty::Float => call("_rt.floatStr"),
            ToString => call("_rt.toStr"),
            ToFloat => Ts::atom(r.at(ATOM)),
            Round => call("_rt.roundHalfAway"),
        }
    }

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

    /// A method's arguments (without the receiver), vouching for those the checker
    /// proved trusted.
    fn method_args(&mut self, method: Option<DefId>, args: &[ExprId]) -> String {
        let trusted = method
            .and_then(|m| self.scope.program.func(m))
            .map(|f| f.trusted.clone())
            .unwrap_or_default();
        self.operands(args)
            .iter()
            .enumerate()
            .map(|(i, p)| {
                // Parameter 0 is `self`.
                if trusted.get(i + 1).copied().unwrap_or(false) {
                    format!("_rt.trusted({})", p.at(TERNARY))
                } else {
                    p.at(TERNARY)
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn template(&mut self, parts: &[TemplatePart]) -> Ts {
        let exprs: Vec<ExprId> = parts
            .iter()
            .filter_map(|p| match p {
                TemplatePart::Expr(x) => Some(*x),
                TemplatePart::Lit(_) => None,
            })
            .collect();
        let values = self.operands(&exprs);
        let mut values = values.into_iter();
        let mut out = String::from("`");
        for part in parts {
            match part {
                TemplatePart::Lit(s) => {
                    for c in s.chars() {
                        match c {
                            '`' => out.push_str("\\`"),
                            '\\' => out.push_str("\\\\"),
                            '$' => out.push_str("\\$"),
                            '\r' => out.push_str("\\r"),
                            c => out.push(c),
                        }
                    }
                }
                TemplatePart::Expr(x) => {
                    let v = values.next().unwrap_or_else(|| Ts::atom("null"));
                    let text = match self.ex(*x).ty {
                        Ty::String | Ty::Int => v.at(TERNARY),
                        Ty::Float => format!("_rt.floatStr({})", v.at(TERNARY)),
                        _ => format!("_rt.toStr({})", v.at(TERNARY)),
                    };
                    out.push_str("${");
                    out.push_str(&text);
                    out.push('}');
                }
            }
        }
        out.push('`');
        Ts::atom(out)
    }
}

fn lit(l: &Lit) -> Ts {
    match l {
        Lit::Int(n) if *n < 0 => Ts::new(n.to_string(), UNARY),
        Lit::Int(n) => Ts::atom(n.to_string()),
        Lit::Float(s) => Ts::atom(s.clone()),
        Lit::Str(s) => Ts::atom(names::string(s)),
        Lit::Bool(b) => Ts::atom(if *b { "true" } else { "false" }),
    }
}
