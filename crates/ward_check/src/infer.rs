//! Type checking of function bodies. Bidirectional: `check` pushes an expected type
//! down (so errors land on the innermost culprit), `infer` synthesizes one, and
//! unification fills in what neither knows yet (`let xs = [];`).

use la_arena::ArenaMap;
use ward_resolve::{Builtin, DefId, LocalId, ModuleId, ModuleRes, ValueRes, did_you_mean};
use ward_syntax::ast::*;
use ward_syntax::diag::codes;
use ward_syntax::{Diagnostic, Span};

use crate::exhaust::{self, Ctor, Ctors, DPat, Witness};
use crate::methods;
use crate::ty::{Ty, Unifier};
use crate::{Checker, ModuleTypes};

pub(crate) fn check_fn(c: &mut Checker, def: DefId, f: &FnDecl, out: &mut ModuleTypes) {
    let program = c.program;
    let resolution = c.res;
    let (params, ret, throws) = match c.fns.get(&def) {
        Some(sig) => (sig.params.clone(), sig.ret.clone(), sig.throws.clone()),
        None => return,
    };
    let mres = resolution.module(def.module);
    let mut cx = Cx {
        c,
        m: def.module,
        ast: &program.module(def.module).ast,
        src: &program.module(def.module).src,
        mres,
        u: Unifier::default(),
        locals: ArenaMap::default(),
        exprs: Vec::new(),
        ret: ret.clone(),
        fn_name: f.name.name.clone(),
        generics: f.generics.iter().map(|g| g.name.clone()).collect(),
        lets: Vec::new(),
        throws,
        handlers: Vec::new(),
        propagating: None,
        thrown: ArenaMap::default(),
    };
    for (&local, ty) in mres
        .params
        .get(&def.item)
        .into_iter()
        .flatten()
        .zip(&params)
    {
        cx.locals.insert(local, ty.clone());
    }
    for entry in f.budget.iter().flatten() {
        cx.infer(entry.value);
    }
    match &f.body {
        FnBody::Block(b) => {
            cx.block(b, Some(&ret));
        }
        FnBody::Ai { prompt } => {
            cx.check(*prompt, &Ty::String);
        }
    }
    // `check {...}`: conditions on the answer, called `it`, next to the parameters.
    if let (Some(checks), Some(&it)) = (&f.checks, mres.check_its.get(&def.item)) {
        cx.locals.insert(it, ret.clone());
        for e in &checks.entries {
            cx.check(e.cond, &Ty::Bool);
        }
    }
    cx.finish(out);
}

/// A test body: like a function that returns nothing and may throw anything, since a
/// thrown error just fails the test.
pub(crate) fn check_test(c: &mut Checker, m: ModuleId, t: &TestDecl, out: &mut ModuleTypes) {
    let program = c.program;
    let mres = c.res.module(m);
    let mut cx = Cx {
        c,
        m,
        ast: &program.module(m).ast,
        src: &program.module(m).src,
        mres,
        u: Unifier::default(),
        locals: ArenaMap::default(),
        exprs: Vec::new(),
        ret: Ty::Unit,
        fn_name: format!("test \"{}\"", t.name),
        generics: Vec::new(),
        lets: Vec::new(),
        throws: Some(Ty::Dynamic),
        handlers: Vec::new(),
        propagating: None,
        thrown: ArenaMap::default(),
    };
    cx.block(&t.body, Some(&Ty::Unit));
    cx.finish(out);
}

/// A refinement's condition, with `it` of type `base`.
pub(crate) fn check_refinement(
    c: &mut Checker,
    m: ModuleId,
    ty: TypeId,
    cond: ExprId,
    base: Ty,
    out: &mut ModuleTypes,
) {
    let program = c.program;
    let mres = c.res.module(m);
    let Some(&it) = mres.refinement_its.get(ty) else {
        return;
    };
    let mut cx = Cx {
        c,
        m,
        ast: &program.module(m).ast,
        src: &program.module(m).src,
        mres,
        u: Unifier::default(),
        locals: ArenaMap::default(),
        exprs: Vec::new(),
        ret: Ty::Unit,
        fn_name: "a refinement".to_owned(),
        generics: Vec::new(),
        lets: Vec::new(),
        throws: None,
        handlers: Vec::new(),
        propagating: None,
        thrown: ArenaMap::default(),
    };
    cx.locals.insert(it, base);
    cx.check(cond, &Ty::Bool);
    cx.finish(out);
}

struct Cx<'a, 'p> {
    c: &'a mut Checker<'p>,
    m: ModuleId,
    ast: &'p Module,
    src: &'p str,
    mres: &'p ModuleRes,
    u: Unifier,
    locals: ArenaMap<LocalId, Ty>,
    exprs: Vec<(ExprId, Ty)>,
    ret: Ty,
    fn_name: String,
    generics: Vec<String>,
    /// `let` bindings, checked at the end for types that were never pinned down.
    lets: Vec<(LocalId, Span)>,
    /// The function's declared `throws` type.
    throws: Option<Ty>,
    /// Enclosing `try` blocks, innermost last.
    handlers: Vec<Handler>,
    /// The call currently being checked under a `?`.
    propagating: Option<ExprId>,
    /// Error type of each call marked with `?`.
    thrown: ArenaMap<ExprId, Ty>,
}

struct Handler {
    /// The type the `catch` variable will have.
    ty: Ty,
    used: bool,
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("{n} {word}")
    } else {
        format!("{n} {word}s")
    }
}

impl Cx<'_, '_> {
    fn err(&mut self, d: Diagnostic) {
        self.c.error(self.m, d);
    }

    fn span(&self, e: ExprId) -> Span {
        self.ast.exprs[e].span
    }

    fn text(&self, span: Span) -> &str {
        self.src.get(span.range()).unwrap_or("")
    }

    fn record(&mut self, e: ExprId, t: Ty) -> Ty {
        self.exprs.push((e, t.clone()));
        t
    }

    fn finish(self, out: &mut ModuleTypes) {
        let mut unsolved = Vec::new();
        for &(local, span) in &self.lets {
            let t = self.locals.get(local).map(|t| self.u.resolve(t));
            if t.is_some_and(|t| t.any(&|x| matches!(x, Ty::Var(_)))) {
                unsolved.push((local, span));
            }
        }
        for (local, span) in unsolved {
            let name = self.mres.locals[local].name.clone();
            let shown = self.show(self.locals.get(local).unwrap_or(&Ty::Error));
            self.c.error(
                self.m,
                Diagnostic::error(
                    codes::ANNOTATION_NEEDED,
                    format!("type annotations needed for `{name}`"),
                    span,
                )
                .with_label(format!("the type `{shown}` isn't fully known"))
                .with_help(format!("add a type, e.g. `let {name}: List<String> = ...`")),
            );
        }
        for (e, t) in &self.exprs {
            out.exprs.insert(*e, self.u.resolve(t).without_vars());
        }
        for (local, t) in self.locals.iter() {
            out.locals.insert(local, self.u.resolve(t).without_vars());
        }
        for (e, t) in self.thrown.iter() {
            out.throws.insert(e, self.u.resolve(t).without_vars());
        }
    }

    fn show(&self, t: &Ty) -> String {
        self.u.resolve(t).display(self.c.program, &self.generics)
    }

    fn coerce(&mut self, found: &Ty, expected: &Ty, span: Span) -> bool {
        if self.u.unify(found, expected) {
            return true;
        }
        let (f, x) = (self.show(found), self.show(expected));
        self.err(
            Diagnostic::error(
                codes::TYPE_MISMATCH,
                format!("mismatched types: expected `{x}`, found `{f}`"),
                span,
            )
            .with_label(format!("expected `{x}`, found `{f}`")),
        );
        false
    }

    fn check(&mut self, e: ExprId, expected: &Ty) -> Ty {
        let ast = self.ast;
        let t = match &ast.exprs[e].kind {
            ExprKind::If { cond, then, else_ } => {
                self.if_expr(*cond, then, *else_, Some(expected), e)
            }
            ExprKind::Match { scrutinee, arms } => {
                self.match_expr(*scrutinee, arms, Some(expected))
            }
            ExprKind::TryCatch { body, err, handler } => {
                self.try_catch(e, body, err.is_some(), handler, Some(expected))
            }
            ExprKind::Block(b) => self.block(b, Some(expected)),
            ExprKind::List(items) if matches!(self.u.shallow(expected), Ty::List(_)) => {
                let elem = match self.u.shallow(expected) {
                    Ty::List(t) => *t,
                    _ => Ty::Error,
                };
                for &item in items {
                    self.check(item, &elem);
                }
                Ty::list(elem)
            }
            _ => {
                let t = self.infer_inner(e);
                let span = self.span(e);
                self.coerce(&t, expected, span);
                t
            }
        };
        self.record(e, t)
    }

    fn infer(&mut self, e: ExprId) -> Ty {
        let t = self.infer_inner(e);
        self.record(e, t)
    }

    fn infer_inner(&mut self, e: ExprId) -> Ty {
        let ast = self.ast;
        let span = self.span(e);
        match &ast.exprs[e].kind {
            ExprKind::Lit(l) => lit_ty(l),
            ExprKind::Template(parts) => {
                for part in parts {
                    if let TemplatePart::Expr(p) = part {
                        self.infer(*p);
                    }
                }
                Ty::String
            }
            ExprKind::Name(_) => match self.mres.values.get(e) {
                Some(&res) => self.value_ty(res, span),
                None => Ty::Error,
            },
            ExprKind::Field { base, name } => match self.mres.values.get(e) {
                Some(&res) => self.value_ty(res, span),
                None => self.field(*base, name),
            },
            ExprKind::Call { callee, args } => self.call(e, *callee, args),
            ExprKind::Index { base, index } => {
                let bt = self.infer(*base);
                match self.u.resolve(&bt) {
                    Ty::List(t) => {
                        self.check(*index, &Ty::Int);
                        *t
                    }
                    Ty::Map(k, v) => {
                        self.check(*index, &k);
                        *v
                    }
                    t @ (Ty::Dynamic | Ty::Error | Ty::Never) => {
                        self.infer(*index);
                        t
                    }
                    _ => {
                        self.infer(*index);
                        let shown = self.show(&bt);
                        self.err(
                            Diagnostic::error(
                                codes::INVALID_OPERANDS,
                                format!("cannot index into a value of type `{shown}`"),
                                span,
                            )
                            .with_label("only lists and maps can be indexed"),
                        );
                        Ty::Error
                    }
                }
            }
            ExprKind::Propagate(inner) => self.propagate_call(*inner, span),
            ExprKind::Unary { op, operand } => match op {
                UnOp::Not => {
                    self.check(*operand, &Ty::Bool);
                    Ty::Bool
                }
                UnOp::Neg => {
                    let t = self.infer(*operand);
                    let r = self.u.resolve(&t);
                    if !matches!(r, Ty::Int | Ty::Float | Ty::Var(_)) && !r.is_lenient() {
                        let shown = self.show(&t);
                        self.err(
                            Diagnostic::error(
                                codes::INVALID_OPERANDS,
                                format!("cannot negate a value of type `{shown}`"),
                                span,
                            )
                            .with_label("expected `Int` or `Float`"),
                        );
                        return Ty::Error;
                    }
                    t
                }
            },
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, *lhs, *rhs),
            ExprKind::List(items) => {
                let elem = self.u.fresh();
                for &item in items {
                    self.check(item, &elem);
                }
                Ty::list(elem)
            }
            ExprKind::Record { path, fields } => self.record_lit(e, path, fields),
            ExprKind::If { cond, then, else_ } => self.if_expr(*cond, then, *else_, None, e),
            ExprKind::Match { scrutinee, arms } => self.match_expr(*scrutinee, arms, None),
            ExprKind::TryCatch { body, err, handler } => {
                self.try_catch(e, body, err.is_some(), handler, None)
            }
            ExprKind::Block(b) => self.block(b, None),
            ExprKind::Error => Ty::Error,
        }
    }

    fn not_a_value(&mut self, span: Span, message: String, help: String) -> Ty {
        self.err(
            Diagnostic::error(codes::NOT_A_VALUE, message, span)
                .with_label("not a value")
                .with_help(help),
        );
        Ty::Error
    }

    /// The type of a resolved name used as a value (not called).
    fn value_ty(&mut self, res: ValueRes, span: Span) -> Ty {
        let text = self.text(span).to_owned();
        match res {
            ValueRes::Local(id) => self.locals.get(id).cloned().unwrap_or(Ty::Error),
            ValueRes::Fn(_) => self.not_a_value(
                span,
                format!("function `{text}` can't be used as a value"),
                format!("call it: `{text}(...)`"),
            ),
            ValueRes::Variant(d, i) => {
                if self.variant_payload(d, i).is_empty() {
                    self.adt_fresh(d)
                } else {
                    self.not_a_value(
                        span,
                        format!("variant `{text}` has fields and must be called"),
                        format!("call it: `{text}(...)`"),
                    )
                }
            }
            ValueRes::Builtin(Builtin::None) => Ty::option(self.u.fresh()),
            ValueRes::Builtin(b) => self.not_a_value(
                span,
                format!("`{}` can't be used as a value", b.name()),
                format!("call it: `{}(...)`", b.name()),
            ),
            ValueRes::Enum(_) | ValueRes::Module(_) | ValueRes::Tool(_) => self.not_a_value(
                span,
                format!("`{text}` is a namespace, not a value"),
                format!("use one of its members: `{text}.name`"),
            ),
            ValueRes::ToolMember(_) => self.not_a_value(
                span,
                format!("tool function `{text}` can't be used as a value"),
                format!("call it: `{text}(...)`"),
            ),
        }
    }

    fn enum_generics(&self, d: DefId) -> usize {
        match self.c.program.item(d) {
            Item::Enum(e) => e.generics.len(),
            Item::Record(r) => r.generics.len(),
            _ => 0,
        }
    }

    fn fresh_args(&mut self, d: DefId) -> Vec<Ty> {
        (0..self.enum_generics(d)).map(|_| self.u.fresh()).collect()
    }

    fn adt_fresh(&mut self, d: DefId) -> Ty {
        let args = self.fresh_args(d);
        Ty::Adt(d, args)
    }

    fn variant_payload(&self, d: DefId, i: usize) -> Vec<Ty> {
        self.c
            .enums
            .get(&d)
            .and_then(|vs| vs.get(i))
            .map(|(_, ts)| ts.clone())
            .unwrap_or_default()
    }

    fn unknown_type_here(&mut self, span: Span, what: &str) -> Ty {
        self.err(
            Diagnostic::error(
                codes::ANNOTATION_NEEDED,
                format!("the type of this value must be known to {what}"),
                span,
            )
            .with_label("type not known yet")
            .with_help("add a type annotation to the variable"),
        );
        Ty::Error
    }

    /// `base` names an enum, module or tool: a failed member lookup was already reported.
    fn is_namespace(&self, base: ExprId) -> bool {
        matches!(
            self.mres.values.get(base),
            Some(ValueRes::Enum(_) | ValueRes::Module(_) | ValueRes::Tool(_))
        )
    }

    fn field(&mut self, base: ExprId, name: &Ident) -> Ty {
        if self.is_namespace(base) {
            return Ty::Error;
        }
        let bt = self.infer(base);
        let base_span = self.span(base);
        let r = self.u.resolve(&bt);
        match &r {
            Ty::Adt(d, args) => {
                if let Some(fields) = self.c.records.get(d) {
                    if let Some((_, t)) = fields.iter().find(|(n, _)| *n == name.name) {
                        return t.subst(args);
                    }
                    let names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
                    let suggestion = did_you_mean(&name.name, names).map(str::to_owned);
                    self.no_field(&r, name, suggestion);
                } else {
                    self.no_field(&r, name, None);
                }
                Ty::Error
            }
            Ty::Dynamic => Ty::Dynamic,
            Ty::Error | Ty::Never => Ty::Error,
            Ty::Var(_) => self.unknown_type_here(base_span, &format!("access `.{}`", name.name)),
            _ => {
                self.no_field(&r, name, None);
                Ty::Error
            }
        }
    }

    fn no_field(&mut self, ty: &Ty, name: &Ident, suggestion: Option<String>) {
        let shown = self.show(ty);
        let d = Diagnostic::error(
            codes::NO_SUCH_FIELD,
            format!("no field `{}` on type `{shown}`", name.name),
            name.span,
        )
        .with_label("unknown field");
        let d = match suggestion {
            Some(s) => d.with_help(format!("a field with a similar name exists: `{s}`")),
            None => d,
        };
        self.err(d);
    }

    fn arg_count(&mut self, what: &str, expected: usize, found: usize, span: Span) {
        self.err(
            Diagnostic::error(
                codes::WRONG_ARG_COUNT,
                format!(
                    "{what} takes {} but {} {} given",
                    plural(expected, "argument"),
                    found,
                    if found == 1 { "was" } else { "were" }
                ),
                span,
            )
            .with_label(format!("expected {}", plural(expected, "argument"))),
        );
    }

    fn args(&mut self, args: &[ExprId], params: &[Ty], what: &str, span: Span) {
        if args.len() != params.len() {
            self.arg_count(what, params.len(), args.len(), span);
        }
        for (i, &a) in args.iter().enumerate() {
            match params.get(i) {
                Some(p) => {
                    self.check(a, p);
                }
                None => {
                    self.infer(a);
                }
            }
        }
    }

    fn infer_all(&mut self, args: &[ExprId]) {
        for &a in args {
            self.infer(a);
        }
    }

    fn call(&mut self, e: ExprId, callee: ExprId, args: &[ExprId]) -> Ty {
        let ast = self.ast;
        let span = self.span(e);
        let callee_span = self.span(callee);
        let callee_text = self.text(callee_span).to_owned();
        let Some(&res) = self.mres.values.get(callee) else {
            if let ExprKind::Field { base, name } = &ast.exprs[callee].kind {
                return self.method_call(*base, name, args, span);
            }
            let t = self.infer(callee);
            self.infer_all(args);
            if !self.u.resolve(&t).is_lenient() {
                self.not_callable(
                    callee_span,
                    "this expression is not a function".to_owned(),
                    &t,
                );
            }
            return Ty::Error;
        };
        match res {
            ValueRes::Fn(d) => {
                let (generics, params, ret) = match self.c.fns.get(&d) {
                    Some(sig) => (sig.generics, sig.params.clone(), sig.ret.clone()),
                    None => return Ty::Error,
                };
                let inst: Vec<Ty> = (0..generics).map(|_| self.u.fresh()).collect();
                let params: Vec<Ty> = params.iter().map(|p| p.subst(&inst)).collect();
                self.args(args, &params, &format!("function `{callee_text}`"), span);
                if let Some(t) = self.c.fns.get(&d).and_then(|sig| sig.throws.clone()) {
                    self.call_throws(e, t.subst(&inst), &callee_text, span);
                }
                ret.subst(&inst)
            }
            ValueRes::Variant(d, i) => {
                let inst = self.fresh_args(d);
                let payload: Vec<Ty> = self
                    .variant_payload(d, i)
                    .iter()
                    .map(|t| t.subst(&inst))
                    .collect();
                self.args(args, &payload, &format!("variant `{callee_text}`"), span);
                Ty::Adt(d, inst)
            }
            ValueRes::Builtin(Builtin::Validate) => {
                let t = self.validate(args, span);
                self.call_throws(e, Ty::String, "validate", span);
                t
            }
            ValueRes::Builtin(b) => self.builtin_call(b, args, span, callee_span),
            ValueRes::ToolMember(d) => {
                let program = self.c.program;
                let func = match &ast.exprs[callee].kind {
                    ExprKind::Field { name, .. } => {
                        program.tool_schema(d).and_then(|s| s.function(&name.name))
                    }
                    _ => None,
                };
                let Some(func) = func else {
                    self.infer_all(args);
                    return Ty::Dynamic;
                };
                let params: Vec<Ty> = func.params.iter().map(Ty::tool_param).collect();
                let required = func.params.iter().filter(|p| p.required).count();
                let what = format!("tool function `{callee_text}`");
                if (required..=params.len()).contains(&args.len()) {
                    // Optional parameters at the end may be left out.
                    self.args(args, &params[..args.len()], &what, span);
                } else if args.len() < required && required < params.len() {
                    self.err(
                        Diagnostic::error(
                            codes::WRONG_ARG_COUNT,
                            format!(
                                "{what} takes {required} to {} arguments but {} {} given",
                                params.len(),
                                args.len(),
                                if args.len() == 1 { "was" } else { "were" }
                            ),
                            span,
                        )
                        .with_label(format!(
                            "expected at least {}",
                            plural(required, "argument")
                        )),
                    );
                    self.infer_all(args);
                } else {
                    self.args(args, &params, &what, span);
                }
                // A tool reports failure with a message (MCP's `isError`).
                self.call_throws(e, Ty::String, &callee_text, span);
                Ty::from_tool(&func.result)
            }
            ValueRes::Local(id) => {
                self.infer_all(args);
                let t = self.locals.get(id).cloned().unwrap_or(Ty::Error);
                if !self.u.resolve(&t).is_lenient() {
                    self.not_callable(
                        callee_span,
                        format!("`{callee_text}` is not a function"),
                        &t,
                    );
                }
                Ty::Error
            }
            ValueRes::Enum(_) | ValueRes::Module(_) | ValueRes::Tool(_) => {
                self.infer_all(args);
                self.err(
                    Diagnostic::error(
                        codes::NOT_CALLABLE,
                        format!("`{callee_text}` is a namespace, not a function"),
                        callee_span,
                    )
                    .with_label("not a function"),
                );
                Ty::Error
            }
        }
    }

    fn not_callable(&mut self, span: Span, message: String, ty: &Ty) {
        let shown = self.show(ty);
        self.err(
            Diagnostic::error(codes::NOT_CALLABLE, message, span)
                .with_label(format!("this has type `{shown}`")),
        );
    }

    fn builtin_call(&mut self, b: Builtin, args: &[ExprId], span: Span, callee_span: Span) -> Ty {
        let arity = |n: usize| args.len() == n;
        match b {
            Builtin::Some | Builtin::Approve => {
                if !arity(1) {
                    self.arg_count(&format!("`{}`", b.name()), 1, args.len(), span);
                }
                let t = match args.first() {
                    Some(&a) => self.infer(a),
                    None => Ty::Error,
                };
                self.infer_all(args.get(1..).unwrap_or(&[]));
                if b == Builtin::Some { Ty::option(t) } else { t }
            }
            Builtin::Declassify => {
                if !arity(2) {
                    self.arg_count("`declassify`", 2, args.len(), span);
                }
                let t = args.first().map_or(Ty::Error, |&a| self.infer(a));
                if let Some(&reason) = args.get(1) {
                    self.check(reason, &Ty::String);
                }
                self.infer_all(args.get(2..).unwrap_or(&[]));
                t
            }
            Builtin::Validate => self.validate(args, span),
            Builtin::None => {
                self.infer_all(args);
                self.err(
                    Diagnostic::error(codes::NOT_CALLABLE, "`None` is not a function", callee_span)
                        .with_label("not a function")
                        .with_help("write `None` without parentheses"),
                );
                Ty::Error
            }
        }
    }

    /// `validate(x, rule)`: `rule` names a function `fn(T) -> Bool`. Returns `x`, or throws
    /// a `String` saying which rule failed.
    fn validate(&mut self, args: &[ExprId], span: Span) -> Ty {
        let [value, rule] = args else {
            self.arg_count("`validate`", 2, args.len(), span);
            self.infer_all(args);
            return Ty::Error;
        };
        let t = self.infer(*value);
        let rule_span = self.span(*rule);
        let ok = match self.mres.values.get(*rule) {
            Some(&ValueRes::Fn(d)) => match self.c.fns.get(&d) {
                Some(sig) if sig.params.len() == 1 => {
                    let inst: Vec<Ty> = (0..sig.generics).map(|_| self.u.fresh()).collect();
                    let (param, ret) = (sig.params[0].subst(&inst), sig.ret.subst(&inst));
                    self.u.unify(&param, &t) && self.u.unify(&ret, &Ty::Bool)
                }
                _ => false,
            },
            // An unresolved name was already reported.
            None if matches!(self.ast.exprs[*rule].kind, ExprKind::Name(_)) => true,
            _ => {
                self.infer(*rule);
                false
            }
        };
        if !ok {
            let shown = self.show(&t);
            self.err(
                Diagnostic::error(
                    codes::INVALID_VALIDATE_RULE,
                    format!("`validate` needs a rule: a function `fn({shown}) -> Bool`"),
                    rule_span,
                )
                .with_label("not a matching rule function")
                .with_help(format!(
                    "declare one, e.g. `fn is_valid(x: {shown}) -> Bool {{ ... }}`, and pass its name"
                )),
            );
        }
        t
    }

    fn method_call(&mut self, base: ExprId, name: &Ident, args: &[ExprId], span: Span) -> Ty {
        if self.is_namespace(base) {
            self.infer_all(args);
            return Ty::Error;
        }
        let bt = self.infer(base);
        let base_span = self.span(base);
        let r = self.u.resolve(&bt);
        match &r {
            Ty::Dynamic => {
                self.infer_all(args);
                return Ty::Dynamic;
            }
            Ty::Error | Ty::Never => {
                self.infer_all(args);
                return Ty::Error;
            }
            Ty::Var(_) => {
                self.infer_all(args);
                return self.unknown_type_here(base_span, &format!("call `.{}()`", name.name));
            }
            _ => {}
        }
        if let Some(sig) = methods::method(&r, &name.name) {
            self.args(args, &sig.params, &format!("method `{}`", name.name), span);
            return sig.ret;
        }
        self.infer_all(args);
        let shown = self.show(&r);
        let is_field = matches!(&r, Ty::Adt(d, _)
            if self.c.records.get(d).is_some_and(|fs| fs.iter().any(|(n, _)| *n == name.name)));
        let d = if is_field {
            Diagnostic::error(
                codes::NOT_CALLABLE,
                format!("`{}` is a field of `{shown}`, not a method", name.name),
                name.span,
            )
            .with_label("not a method")
            .with_help(format!("remove the parentheses: `.{}`", name.name))
        } else {
            let d = Diagnostic::error(
                codes::NO_SUCH_METHOD,
                format!("no method `{}` on type `{shown}`", name.name),
                name.span,
            )
            .with_label("unknown method");
            match did_you_mean(&name.name, methods::names(&r)) {
                Some(s) => d.with_help(format!("a method with a similar name exists: `{s}`")),
                None => d,
            }
        };
        self.err(d);
        Ty::Error
    }

    /// A call to a throwing function. Without `?` the error would be ignored silently.
    fn call_throws(&mut self, call: ExprId, thrown: Ty, callee: &str, span: Span) {
        if self.propagating == Some(call) {
            self.thrown.insert(call, thrown);
            return;
        }
        let shown = self.show(&thrown);
        self.err(
            Diagnostic::error(
                codes::MISSING_PROPAGATE,
                format!("`{callee}` can throw `{shown}`, but this call isn't marked with `?`"),
                span,
            )
            .with_label("add `?` after this call")
            .with_help(format!(
                "write `{callee}(...)?` to pass the error on, inside a `try` block or a function that `throws {shown}`"
            )),
        );
    }

    /// `call()?`
    fn propagate_call(&mut self, inner: ExprId, span: Span) -> Ty {
        let saved = self.propagating.replace(inner);
        let t = self.infer(inner);
        self.propagating = saved;
        if let Some(thrown) = self.thrown.get(inner).cloned() {
            self.propagate(&thrown, span);
            return t;
        }
        // Tool calls may throw at runtime; their types arrive with M7.
        if !matches!(self.u.resolve(&t), Ty::Dynamic | Ty::Error) {
            self.err(
                Diagnostic::error(
                    codes::INVALID_TRY,
                    "`?` on something that can't throw",
                    span,
                )
                .with_label("this can't throw")
                .with_help("`?` marks calls to functions that declare `throws`; remove it here"),
            );
        }
        t
    }

    /// Hands an error to the innermost `try`, or to the function's `throws`.
    fn propagate(&mut self, thrown: &Ty, span: Span) {
        let shown = self.show(thrown);
        if let Some(h) = self.handlers.last_mut() {
            h.used = true;
            let caught = h.ty.clone();
            if !self.u.unify(thrown, &caught) {
                let caught = self.show(&caught);
                self.err(
                    Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        format!("this throws `{shown}`, but the enclosing `try` already catches `{caught}`"),
                        span,
                    )
                    .with_label(format!("throws `{shown}`"))
                    .with_help("one `try` block catches one error type"),
                );
            }
            return;
        }
        let fn_name = self.fn_name.clone();
        match self.throws.clone() {
            Some(declared) => {
                if !self.u.unify(thrown, &declared) {
                    let declared = self.show(&declared);
                    self.err(
                        Diagnostic::error(
                            codes::TYPE_MISMATCH,
                            format!("this throws `{shown}`, but `{fn_name}` declares `throws {declared}`"),
                            span,
                        )
                        .with_label(format!("throws `{shown}`"))
                        .with_help(format!(
                            "catch it with `try {{ ... }} catch err {{ ... }}`, or change `{fn_name}` to `throws {shown}`"
                        )),
                    );
                }
            }
            None => self.err(
                Diagnostic::error(
                    codes::UNHANDLED_THROW,
                    format!("`{shown}` is thrown here but never caught"),
                    span,
                )
                .with_label(format!("may throw `{shown}`"))
                .with_help(format!(
                    "catch it with `try {{ ... }} catch err {{ ... }}`, or declare it on `{fn_name}`: `-> T throws {shown}`"
                )),
            ),
        }
    }

    fn try_catch(
        &mut self,
        e: ExprId,
        body: &Block,
        has_err: bool,
        handler: &Block,
        expected: Option<&Ty>,
    ) -> Ty {
        let caught = self.u.fresh();
        self.handlers.push(Handler {
            ty: caught.clone(),
            used: false,
        });
        let body_ty = self.block(body, expected);
        let used = self.handlers.pop().is_some_and(|h| h.used);
        if !used {
            let span = self.span(e);
            self.err(
                Diagnostic::warning(
                    codes::TRY_CANNOT_THROW,
                    "nothing in this `try` block can throw",
                    Span::new(span.start, span.start + 3),
                )
                .with_label("unnecessary `try`")
                .with_help("remove the `try`, or mark calls that throw with `?`"),
            );
            self.u.unify(&caught, &Ty::Error);
        }
        if has_err {
            if let Some(&local) = self.mres.catch_locals.get(e) {
                self.locals.insert(local, caught);
            }
        }
        if let Some(x) = expected {
            self.block(handler, Some(x));
            return x.clone();
        }
        if matches!(self.u.shallow(&body_ty), Ty::Never) {
            return self.block(handler, None);
        }
        self.block(handler, Some(&body_ty));
        body_ty
    }

    fn binary(&mut self, op: BinOp, lhs: ExprId, rhs: ExprId) -> Ty {
        match op {
            BinOp::And | BinOp::Or => {
                self.check(lhs, &Ty::Bool);
                self.check(rhs, &Ty::Bool);
                Ty::Bool
            }
            BinOp::Eq | BinOp::Ne => {
                let lt = self.infer(lhs);
                self.check(rhs, &lt);
                Ty::Bool
            }
            _ => {
                let lt = self.infer(lhs);
                let r = self.u.resolve(&lt);
                let ok = r.is_lenient()
                    || matches!(r, Ty::Var(_) | Ty::Int | Ty::Float)
                    || (op.is_comparison() && r == Ty::String)
                    || (op == BinOp::Add && matches!(r, Ty::String | Ty::List(_)));
                if !ok {
                    self.infer(rhs);
                    let shown = self.show(&r);
                    let lhs_span = self.span(lhs);
                    let what = if op.is_comparison() {
                        "numbers and strings"
                    } else if op == BinOp::Add {
                        "numbers, strings and lists"
                    } else {
                        "numbers"
                    };
                    self.err(
                        Diagnostic::error(
                            codes::INVALID_OPERANDS,
                            format!("`{}` can't be applied to `{shown}`", op.as_str()),
                            lhs_span,
                        )
                        .with_label(format!("this has type `{shown}`"))
                        .with_help(format!("`{}` works on {what}", op.as_str())),
                    );
                    return Ty::Error;
                }
                self.check(rhs, &lt);
                if op.is_comparison() { Ty::Bool } else { lt }
            }
        }
    }

    fn record_lit(&mut self, e: ExprId, path: &Path, fields: &[FieldInit]) -> Ty {
        let name = path.segments.last().map_or("", |s| s.name.as_str());
        let name_span = path.span;
        let Some(&def) = self.mres.records.get(e) else {
            for f in fields {
                if let Some(v) = f.value {
                    self.infer(v);
                }
            }
            return Ty::Error;
        };
        let args = self.fresh_args(def);
        let decls = self.c.records.get(&def).cloned().unwrap_or_default();
        let mut seen: Vec<&str> = Vec::new();
        for (i, f) in fields.iter().enumerate() {
            let decl = decls.iter().find(|(n, _)| *n == f.name.name);
            let expected = decl.map_or(Ty::Error, |(_, t)| t.subst(&args));
            if decl.is_none() {
                let names: Vec<&str> = decls.iter().map(|(n, _)| n.as_str()).collect();
                let d = Diagnostic::error(
                    codes::NO_SUCH_FIELD,
                    format!("record `{name}` has no field `{}`", f.name.name),
                    f.name.span,
                )
                .with_label("unknown field");
                let d = match did_you_mean(&f.name.name, names) {
                    Some(s) => d.with_help(format!("a field with a similar name exists: `{s}`")),
                    None => d,
                };
                self.err(d);
            } else if seen.contains(&f.name.name.as_str()) {
                self.err(
                    Diagnostic::error(
                        codes::DUPLICATE_FIELD,
                        format!("field `{}` is set more than once", f.name.name),
                        f.name.span,
                    )
                    .with_label("already set earlier"),
                );
            }
            seen.push(&f.name.name);
            match f.value {
                Some(v) => {
                    self.check(v, &expected);
                }
                None => {
                    let t = match self.mres.shorthands.get(&(e, i)) {
                        Some(&res) => self.value_ty(res, f.name.span),
                        None => Ty::Error,
                    };
                    self.coerce(&t, &expected, f.name.span);
                }
            }
        }
        let missing: Vec<String> = decls
            .iter()
            .filter(|(n, _)| !seen.contains(&n.as_str()))
            .map(|(n, _)| format!("`{n}`"))
            .collect();
        if !missing.is_empty() {
            self.err(
                Diagnostic::error(
                    codes::MISSING_FIELDS,
                    format!(
                        "missing {} in `{}`: {}",
                        if missing.len() == 1 {
                            "field"
                        } else {
                            "fields"
                        },
                        name,
                        missing.join(", ")
                    ),
                    name_span,
                )
                .with_label("some fields aren't set")
                .with_help("every field of a record must be given a value"),
            );
        }
        Ty::Adt(def, args)
    }

    fn if_expr(
        &mut self,
        cond: ExprId,
        then: &Block,
        else_: Option<ExprId>,
        expected: Option<&Ty>,
        e: ExprId,
    ) -> Ty {
        self.check(cond, &Ty::Bool);
        let Some(else_) = else_ else {
            self.block(then, Some(&Ty::Unit));
            if let Some(x) = expected {
                if !self.u.unify(&Ty::Unit, x) {
                    let shown = self.show(x);
                    let span = self.span(e);
                    self.err(
                        Diagnostic::error(
                            codes::TYPE_MISMATCH,
                            format!("`if` without `else` has type `()`, but `{shown}` is expected"),
                            span,
                        )
                        .with_label(format!("expected `{shown}`"))
                        .with_help("add an `else` branch that produces a value"),
                    );
                }
            }
            return Ty::Unit;
        };
        if let Some(x) = expected {
            self.block(then, Some(x));
            self.check(else_, x);
            return x.clone();
        }
        let t = self.block(then, None);
        if matches!(self.u.shallow(&t), Ty::Never) {
            self.infer(else_)
        } else {
            self.check(else_, &t);
            t
        }
    }

    fn block(&mut self, b: &Block, expected: Option<&Ty>) -> Ty {
        let mut diverges = false;
        for &s in &b.stmts {
            diverges |= self.stmt(s);
        }
        if let Some(tail) = b.tail {
            // A block whose value isn't used may end with any expression.
            if expected.is_some_and(|x| self.u.resolve(x) == Ty::Unit) {
                self.infer(tail);
                return Ty::Unit;
            }
            return match expected {
                Some(x) => self.check(tail, x),
                None => self.infer(tail),
            };
        }
        if diverges {
            return Ty::Never;
        }
        if let Some(x) = expected {
            if !self.u.unify(&Ty::Unit, x) {
                let shown = self.show(x);
                let end = Span::new(b.span.end.saturating_sub(1), b.span.end);
                let last_is_expr = b.stmts.last().is_some_and(|&s| {
                    matches!(self.ast.stmts[s].kind, StmtKind::Expr { semi: true, .. })
                });
                let help = if last_is_expr {
                    "remove the `;` after the last expression to make it the block's value"
                        .to_owned()
                } else {
                    format!("end the block with an expression of type `{shown}`")
                };
                self.err(
                    Diagnostic::error(
                        codes::TYPE_MISMATCH,
                        format!("mismatched types: expected `{shown}`, found `()`"),
                        end,
                    )
                    .with_label("this block has no final expression")
                    .with_help(help),
                );
            }
        }
        Ty::Unit
    }

    /// Returns whether the statement always leaves the function.
    fn stmt(&mut self, id: StmtId) -> bool {
        let ast = self.ast;
        match &ast.stmts[id].kind {
            StmtKind::Let { ty, init, .. } => {
                let t = match ty {
                    Some(ty) => self.c.lower(self.m, *ty),
                    None => self.u.fresh(),
                };
                self.check(*init, &t);
                if let Some(&local) = self.mres.stmt_locals.get(id) {
                    self.locals.insert(local, t);
                    self.lets.push((local, ast.stmts[id].span));
                }
                false
            }
            StmtKind::Assign { target, value } => {
                self.assign(*target, *value);
                false
            }
            StmtKind::Assert { cond, .. } => {
                self.check(*cond, &Ty::Bool);
                false
            }
            StmtKind::Expr { expr, .. } => {
                let t = self.infer(*expr);
                matches!(self.u.shallow(&t), Ty::Never)
            }
            StmtKind::Return(value) => {
                let ret = self.ret.clone();
                match value {
                    Some(v) => {
                        self.check(*v, &ret);
                    }
                    None => {
                        let span = ast.stmts[id].span;
                        self.coerce(&Ty::Unit, &ret, span);
                    }
                }
                true
            }
            StmtKind::Throw(value) => {
                let t = self.infer(*value);
                let span = ast.stmts[id].span;
                self.propagate(&t, span);
                true
            }
            StmtKind::For { iter, body, .. } => {
                let it = self.infer(*iter);
                let iter_span = self.span(*iter);
                let elem = match self.u.resolve(&it) {
                    Ty::List(t) => *t,
                    Ty::Map(k, _) => *k,
                    t @ (Ty::Dynamic | Ty::Error | Ty::Never) => t,
                    Ty::Var(_) => self.unknown_type_here(iter_span, "loop over it"),
                    t => {
                        let shown = self.show(&t);
                        self.err(
                            Diagnostic::error(
                                codes::INVALID_OPERANDS,
                                format!("can't loop over a value of type `{shown}`"),
                                iter_span,
                            )
                            .with_label("expected a `List` or `Map`"),
                        );
                        Ty::Error
                    }
                };
                if let Some(&local) = self.mres.stmt_locals.get(id) {
                    self.locals.insert(local, elem);
                }
                self.block(body, None);
                false
            }
            StmtKind::While { cond, body } => {
                self.check(*cond, &Ty::Bool);
                self.block(body, None);
                false
            }
        }
    }

    fn assign(&mut self, target: ExprId, value: ExprId) {
        let ast = self.ast;
        let span = self.span(target);
        let t = match &ast.exprs[target].kind {
            ExprKind::Name(_) => match self.mres.values.get(target) {
                Some(&ValueRes::Local(id)) => self.locals.get(id).cloned().unwrap_or(Ty::Error),
                Some(_) => {
                    let text = self.text(span).to_owned();
                    self.err(
                        Diagnostic::error(
                            codes::INVALID_ASSIGNMENT,
                            format!("cannot assign to `{text}`"),
                            span,
                        )
                        .with_label("not a variable")
                        .with_help("only variables, fields and list elements can be assigned to"),
                    );
                    Ty::Error
                }
                None => Ty::Error,
            },
            _ => self.infer(target),
        };
        self.record(target, t.clone());
        self.check(value, &t);
    }

    fn match_expr(&mut self, scrutinee: ExprId, arms: &[Arm], expected: Option<&Ty>) -> Ty {
        let st = self.infer(scrutinee);
        let mut result: Option<Ty> = expected.cloned();
        let mut rows = Vec::new();
        for arm in arms {
            self.pat(arm.pat, &st);
            match &result {
                Some(x) => {
                    let x = x.clone();
                    self.check(arm.body, &x);
                }
                None => {
                    let t = self.infer(arm.body);
                    if !matches!(self.u.shallow(&t), Ty::Never) {
                        result = Some(t);
                    }
                }
            }
            let dp = self.dpat(arm.pat);
            let sty = self.u.resolve(&st);
            if sty != Ty::Error && !exhaust::is_reachable(&rows, &dp, &sty, &*self) {
                let span = self.ast.pats[arm.pat].span;
                self.err(
                    Diagnostic::warning(codes::UNREACHABLE_PATTERN, "unreachable pattern", span)
                        .with_label("this arm is never reached")
                        .with_help("an earlier arm already matches every value this one would"),
                );
            }
            rows.push(dp);
        }
        let sty = self.u.resolve(&st);
        if sty != Ty::Error {
            if let Some(w) = exhaust::missing(&rows, &sty, &*self) {
                let shown = self.fmt_witness(&w, &sty);
                let span = self.span(scrutinee);
                self.err(
                    Diagnostic::error(
                        codes::NON_EXHAUSTIVE_MATCH,
                        format!("non-exhaustive match: `{shown}` not covered"),
                        span,
                    )
                    .with_label(format!("pattern `{shown}` not covered"))
                    .with_help("add an arm for it, or a catch-all arm `_ => ...`"),
                );
            }
        }
        result.unwrap_or(Ty::Never)
    }

    fn pat(&mut self, p: PatId, ty: &Ty) {
        let ast = self.ast;
        let span = ast.pats[p].span;
        match &ast.pats[p].kind {
            PatKind::Wild | PatKind::Error => {}
            PatKind::Name(_) => {
                if let Some(ValueRes::Builtin(Builtin::None)) = self.mres.pat_variants.get(p) {
                    let t = Ty::option(self.u.fresh());
                    self.coerce(&t, ty, span);
                } else if let Some(&local) = self.mres.pat_bindings.get(p) {
                    self.locals.insert(local, ty.clone());
                }
            }
            PatKind::Lit(l) => {
                self.coerce(&lit_ty(l), ty, span);
            }
            PatKind::Variant { path, args } => {
                let name = path
                    .segments
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                let (ctor_ty, payload) = match self.mres.pat_variants.get(p) {
                    Some(ValueRes::Builtin(b @ (Builtin::Some | Builtin::None))) => {
                        let a = self.u.fresh();
                        let payload = if *b == Builtin::Some {
                            vec![a.clone()]
                        } else {
                            vec![]
                        };
                        (Ty::option(a), payload)
                    }
                    Some(&ValueRes::Variant(d, i)) => {
                        let inst = self.fresh_args(d);
                        let payload = self
                            .variant_payload(d, i)
                            .iter()
                            .map(|t| t.subst(&inst))
                            .collect();
                        (Ty::Adt(d, inst), payload)
                    }
                    _ => {
                        for &sub in args.iter().flatten() {
                            self.pat(sub, &Ty::Error);
                        }
                        return;
                    }
                };
                self.coerce(&ctor_ty, ty, span);
                let given = args.as_ref().map_or(0, Vec::len);
                if given != payload.len() && (args.is_some() || !payload.is_empty()) {
                    let d = Diagnostic::error(
                        codes::WRONG_ARG_COUNT,
                        format!(
                            "variant `{name}` has {}, but this pattern has {given}",
                            plural(payload.len(), "field")
                        ),
                        span,
                    )
                    .with_label(format!("expected {}", plural(payload.len(), "field")));
                    let d = if payload.is_empty() {
                        d.with_help(format!("write `{name}` without parentheses"))
                    } else {
                        let wild = vec!["_"; payload.len()].join(", ");
                        d.with_help(format!("use `_` to ignore fields: `{name}({wild})`"))
                    };
                    self.err(d);
                }
                for (i, &sub) in args.iter().flatten().enumerate() {
                    let t = payload.get(i).cloned().unwrap_or(Ty::Error);
                    self.pat(sub, &t);
                }
            }
        }
    }

    fn dpat(&self, p: PatId) -> DPat {
        let ast = self.ast;
        match &ast.pats[p].kind {
            PatKind::Wild | PatKind::Error => DPat::Wild,
            PatKind::Name(_) => match self.mres.pat_variants.get(p) {
                Some(ValueRes::Builtin(Builtin::None)) => DPat::Ctor(Ctor::None, vec![]),
                _ => DPat::Wild,
            },
            PatKind::Lit(Lit::Bool(b)) => {
                DPat::Ctor(if *b { Ctor::True } else { Ctor::False }, vec![])
            }
            PatKind::Lit(l) => DPat::Lit(format!("{l:?}")),
            PatKind::Variant { args, .. } => {
                let subs = args.iter().flatten().map(|&a| self.dpat(a)).collect();
                let ctor = match self.mres.pat_variants.get(p) {
                    Some(ValueRes::Builtin(Builtin::Some)) => Ctor::Some,
                    Some(ValueRes::Builtin(Builtin::None)) => Ctor::None,
                    Some(&ValueRes::Variant(_, i)) => Ctor::Variant(i),
                    _ => return DPat::Wild,
                };
                DPat::Ctor(ctor, subs)
            }
        }
    }

    fn fmt_witness(&self, w: &Witness, ty: &Ty) -> String {
        let Witness::Ctor(c, args) = w else {
            return "_".into();
        };
        let ty = self.u.resolve(ty);
        let name = match (c, &ty) {
            (Ctor::True, _) => "true".to_owned(),
            (Ctor::False, _) => "false".to_owned(),
            (Ctor::Some, _) => "Some".to_owned(),
            (Ctor::None, _) => "None".to_owned(),
            (Ctor::Variant(i), Ty::Adt(d, _)) => {
                let variant = self
                    .c
                    .enums
                    .get(d)
                    .and_then(|vs| vs.get(*i))
                    .map_or("?", |(n, _)| n);
                format!("{}.{variant}", item_name(self.c.program.item(*d)))
            }
            (Ctor::Variant(_), _) => "?".to_owned(),
        };
        if args.is_empty() {
            return name;
        }
        let subs = self
            .ctors(&ty)
            .and_then(|cs| cs.into_iter().find(|(x, _)| x == c))
            .map(|(_, subs)| subs)
            .unwrap_or_default();
        let inner: Vec<String> = args
            .iter()
            .enumerate()
            .map(|(i, a)| self.fmt_witness(a, subs.get(i).unwrap_or(&Ty::Error)))
            .collect();
        format!("{name}({})", inner.join(", "))
    }
}

impl Ctors for Cx<'_, '_> {
    fn ctors(&self, ty: &Ty) -> Option<Vec<(Ctor, Vec<Ty>)>> {
        match self.u.resolve(ty) {
            Ty::Bool => Some(vec![(Ctor::True, vec![]), (Ctor::False, vec![])]),
            Ty::Option(t) => Some(vec![(Ctor::None, vec![]), (Ctor::Some, vec![*t])]),
            Ty::Adt(d, args) => self.c.enums.get(&d).map(|vs| {
                vs.iter()
                    .enumerate()
                    .map(|(i, (_, ts))| {
                        (
                            Ctor::Variant(i),
                            ts.iter().map(|t| t.subst(&args)).collect(),
                        )
                    })
                    .collect()
            }),
            _ => None,
        }
    }
}

fn lit_ty(l: &Lit) -> Ty {
    match l {
        Lit::Int(_) => Ty::Int,
        Lit::Float(_) => Ty::Float,
        Lit::Str(_) => Ty::String,
        Lit::Bool(_) => Ty::Bool,
    }
}

fn item_name(item: &Item) -> &str {
    match item {
        Item::Fn(f) => &f.name.name,
        Item::Record(r) => &r.name.name,
        Item::Alias(a) => &a.name.name,
        Item::Enum(e) => &e.name.name,
        Item::Import(_) => "import",
        Item::Test(t) => &t.name,
    }
}
