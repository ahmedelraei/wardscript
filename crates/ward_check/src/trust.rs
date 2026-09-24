//! Trust labels: proves that untrusted data can't reach a sink (a tool argument, or anything
//! declared `Trusted`) without `validate`, `approve` or `declassify`.
//!
//! Signatures carry labels; bodies are inferred. A plain parameter is label-polymorphic:
//! each function gets a summary of which parameters reach its result, what it throws and
//! its sinks, and summaries are iterated to a fixpoint so recursion works. Every label
//! remembers one path from its source, which W0107 shows.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use ward_resolve::{
    Builtin, DefId, LocalId, ModuleId, ModuleRes, Prim, Program, ProgramDiagnostic, Resolution,
    TypeRes, ValueRes,
};
use ward_syntax::ast::{
    BinOp, Block, ExprId, ExprKind, FnBody, FnDecl, Item, Module, PatId, PatKind, StmtKind,
    TemplatePart, TypeId, TypeKind,
};
use ward_syntax::diag::codes;
use ward_syntax::{Diagnostic, LineIndex, Span};

use crate::{ModuleTypes, Ty};

/// A loop body is re-checked until its labels stop changing; this only guards against bugs.
const MAX_ITERATIONS: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Declared {
    Plain,
    Trusted,
    Untrusted,
}

/// One step of the path untrusted data takes.
struct Step {
    module: ModuleId,
    span: Span,
    note: String,
    prev: Option<Rc<Step>>,
}

#[derive(Clone)]
struct StepData {
    module: ModuleId,
    span: Span,
    note: String,
}

type Chain = Rc<Step>;

fn push(prev: &Chain, module: ModuleId, span: Span, note: &str) -> Chain {
    if prev.module == module && prev.span == span {
        return prev.clone();
    }
    Rc::new(Step {
        module,
        span,
        note: note.to_owned(),
        prev: Some(prev.clone()),
    })
}

fn source(module: ModuleId, span: Span, note: String) -> Chain {
    Rc::new(Step {
        module,
        span,
        note,
        prev: None,
    })
}

/// Source first.
fn steps(chain: &Chain) -> Vec<StepData> {
    let mut out = Vec::new();
    let mut cur = Some(chain);
    while let Some(s) = cur {
        out.push(StepData {
            module: s.module,
            span: s.span,
            note: s.note.clone(),
        });
        cur = s.prev.as_ref();
    }
    out.reverse();
    out
}

/// `Untrusted` if `untrusted` is set; otherwise as trusted as the parameters in `params`.
#[derive(Clone, Default)]
struct Label {
    untrusted: Option<Chain>,
    params: BTreeMap<usize, Chain>,
}

impl Label {
    fn untrusted(chain: Chain) -> Label {
        Label {
            untrusted: Some(chain),
            params: BTreeMap::new(),
        }
    }

    fn join(&mut self, other: &Label) {
        if self.untrusted.is_none() {
            self.untrusted.clone_from(&other.untrusted);
        }
        for (p, c) in &other.params {
            self.params.entry(*p).or_insert_with(|| c.clone());
        }
    }

    fn joined(mut self, other: &Label) -> Label {
        self.join(other);
        self
    }

    fn step(&self, module: ModuleId, span: Span, note: &str) -> Label {
        Label {
            untrusted: self.untrusted.as_ref().map(|c| push(c, module, span, note)),
            params: self
                .params
                .iter()
                .map(|(p, c)| (*p, push(c, module, span, note)))
                .collect(),
        }
    }

    fn shape(&self) -> (bool, Vec<usize>) {
        (
            self.untrusted.is_some(),
            self.params.keys().copied().collect(),
        )
    }
}

/// How data that reaches a parameter goes on to a sink, starting at the parameter.
#[derive(Clone)]
struct SinkPath {
    steps: Vec<StepData>,
    sink: String,
}

#[derive(Clone, Default)]
struct Summary {
    ret: Label,
    throws: Label,
    /// Parameters that must be trusted.
    sinks: BTreeMap<usize, SinkPath>,
}

type Shape = ((bool, Vec<usize>), (bool, Vec<usize>), Vec<usize>);

impl Summary {
    fn shape(&self) -> Shape {
        (
            self.ret.shape(),
            self.throws.shape(),
            self.sinks.keys().copied().collect(),
        )
    }
}

/// Which parameters of each function must be trusted, and the W0107 diagnostics.
pub(crate) fn check(
    program: &Program,
    res: &Resolution,
    types: &[ModuleTypes],
) -> (Vec<ProgramDiagnostic>, HashMap<DefId, Vec<bool>>) {
    let fns: Vec<(DefId, &FnDecl)> =
        program
            .module_ids()
            .flat_map(|m| {
                program.module(m).ast.items.iter().enumerate().filter_map(
                    move |(item, it)| match it {
                        Item::Fn(f) => Some((DefId { module: m, item }, f)),
                        _ => None,
                    },
                )
            })
            .collect();
    let mut t = Trust {
        program,
        res,
        types,
        summaries: HashMap::new(),
        diags: Vec::new(),
        reported: HashSet::new(),
    };
    // Summaries only grow, so this terminates; the bound is a guard against bugs.
    for _ in 0..MAX_ITERATIONS {
        let mut changed = false;
        for &(def, f) in &fns {
            let s = t.function(def, f, false);
            let old = t.summaries.get(&def).map(Summary::shape);
            if old.as_ref() != Some(&s.shape()) {
                changed = true;
            }
            t.summaries.insert(def, s);
        }
        if !changed {
            break;
        }
    }
    for &(def, f) in &fns {
        t.function(def, f, true);
    }
    let trusted = fns
        .iter()
        .map(|&(def, f)| {
            let sinks = t.summaries.get(&def).map(|s| &s.sinks);
            let params = (0..f.params.len())
                .map(|i| sinks.is_some_and(|s| s.contains_key(&i)))
                .collect();
            (def, params)
        })
        .collect();
    (t.diags, trusted)
}

/// Whether `Untrusted<...>` appears in the type `ty` of `module`, through aliases.
pub(crate) fn declares_untrusted(
    program: &Program,
    res: &Resolution,
    module: ModuleId,
    ty: TypeId,
) -> bool {
    let t = Trust {
        program,
        res,
        types: &[],
        summaries: HashMap::new(),
        diags: Vec::new(),
        reported: HashSet::new(),
    };
    t.declared(module, ty) == Declared::Untrusted
}

struct Trust<'p> {
    program: &'p Program,
    res: &'p Resolution,
    types: &'p [ModuleTypes],
    summaries: HashMap<DefId, Summary>,
    diags: Vec<ProgramDiagnostic>,
    /// (module, span, sink) of each W0107, so fixpoint passes and loops report once.
    reported: HashSet<(ModuleId, Span, String)>,
}

impl<'p> Trust<'p> {
    fn declared(&self, module: ModuleId, ty: TypeId) -> Declared {
        self.declared_depth(module, ty, 0)
    }

    fn declared_depth(&self, module: ModuleId, ty: TypeId, depth: usize) -> Declared {
        // Cyclic aliases were already reported by the type checker.
        if depth > 32 {
            return Declared::Plain;
        }
        let ast = &self.program.module(module).ast;
        let TypeKind::Named { args, .. } = &ast.types[ty].kind else {
            return Declared::Plain;
        };
        let inner = |t: &Self| {
            let any_untrusted = args
                .iter()
                .any(|&a| t.declared_depth(module, a, depth + 1) == Declared::Untrusted);
            if any_untrusted {
                Declared::Untrusted
            } else {
                Declared::Plain
            }
        };
        match self.res.module(module).types.get(ty) {
            Some(TypeRes::Prim(Prim::Untrusted)) => Declared::Untrusted,
            Some(TypeRes::Prim(Prim::Trusted)) => match inner(self) {
                Declared::Untrusted => Declared::Untrusted,
                _ => Declared::Trusted,
            },
            Some(TypeRes::Def(d)) => match self.program.item(*d) {
                Item::Alias(a) => match self.declared_depth(d.module, a.ty, depth + 1) {
                    Declared::Plain => inner(self),
                    other => other,
                },
                _ => inner(self),
            },
            _ => inner(self),
        }
    }

    fn function(&mut self, def: DefId, f: &'p FnDecl, report: bool) -> Summary {
        let module = def.module;
        let (program, res, types) = (self.program, self.res, self.types);
        let mres = res.module(module);
        let mut cx = FnCx {
            t: self,
            module,
            ast: &program.module(module).ast,
            mres,
            types: types.get(module.0 as usize),
            f,
            env: HashMap::new(),
            pc: Label::default(),
            tries: Vec::new(),
            out: Summary::default(),
            report,
        };
        let params = mres.params.get(&def.item).cloned().unwrap_or_default();
        for (i, (p, &local)) in f.params.iter().zip(&params).enumerate() {
            let label = match cx.t.declared(module, p.ty) {
                Declared::Untrusted => Label::untrusted(source(
                    module,
                    p.span,
                    format!("`{}` is declared `Untrusted`", p.name.name),
                )),
                Declared::Trusted => {
                    cx.out.sinks.insert(
                        i,
                        SinkPath {
                            steps: Vec::new(),
                            sink: format!(
                                "the `Trusted` parameter `{}` of `{}`",
                                p.name.name, f.name.name
                            ),
                        },
                    );
                    Label::default()
                }
                Declared::Plain => {
                    let mut l = Label::default();
                    l.params.insert(
                        i,
                        source(module, p.name.span, format!("parameter `{}`", p.name.name)),
                    );
                    l
                }
            };
            cx.env.insert(local, label);
        }
        match &f.body {
            FnBody::Block(b) => {
                let v = cx.block(b);
                cx.ret(v, b.tail.map_or(b.span, |e| cx.span(e)));
            }
            FnBody::Ai { .. } => {
                if let Some(ret) = f.ret {
                    if cx.t.declared(module, ret) == Declared::Trusted {
                        let span = cx.ast.types[ret].span;
                        let chain = source(
                            module,
                            f.name.span,
                            format!("output of `ai fn {}`", f.name.name),
                        );
                        cx.meet(
                            &Label::untrusted(chain),
                            span,
                            "declared `Trusted` here",
                            &SinkPath {
                                steps: Vec::new(),
                                sink: "a `Trusted` return type".to_owned(),
                            },
                        );
                    }
                }
            }
        }
        cx.out
    }
}

struct TryFrame {
    /// When code after a throw point runs depends on whether it threw.
    raise: Label,
    thrown: Label,
    env: HashMap<LocalId, Label>,
}

struct FnCx<'a, 'p> {
    t: &'a mut Trust<'p>,
    module: ModuleId,
    ast: &'p Module,
    mres: &'p ModuleRes,
    types: Option<&'p ModuleTypes>,
    f: &'p FnDecl,
    env: HashMap<LocalId, Label>,
    pc: Label,
    tries: Vec<TryFrame>,
    out: Summary,
    report: bool,
}

fn join_env(a: &mut HashMap<LocalId, Label>, b: &HashMap<LocalId, Label>) {
    for (k, v) in b {
        a.entry(*k).or_default().join(v);
    }
}

fn env_shape(env: &HashMap<LocalId, Label>) -> Vec<(LocalId, (bool, Vec<usize>))> {
    let mut v: Vec<_> = env.iter().map(|(k, l)| (*k, l.shape())).collect();
    v.sort_by_key(|(k, _)| k.into_raw());
    v
}

impl FnCx<'_, '_> {
    fn span(&self, e: ExprId) -> Span {
        self.ast.exprs[e].span
    }

    /// The program counter label: what decides whether the current code runs.
    fn pc(&self) -> Label {
        self.tries
            .iter()
            .fold(self.pc.clone(), |pc, t| pc.joined(&t.raise))
    }

    fn step(&self, l: &Label, span: Span, note: &str) -> Label {
        l.step(self.module, span, note)
    }

    fn ret(&mut self, v: Label, span: Span) {
        let v = v.joined(&self.pc());
        if let Some(ret) = self.f.ret {
            if self.t.declared(self.module, ret) == Declared::Trusted {
                self.meet(
                    &v,
                    span,
                    "returned here",
                    &SinkPath {
                        steps: Vec::new(),
                        sink: format!("the `Trusted` return value of `{}`", self.f.name.name),
                    },
                );
            }
        }
        self.out.ret.join(&v);
    }

    fn throw(&mut self, v: Label) {
        let v = v.joined(&self.pc());
        match self.tries.last_mut() {
            Some(frame) => {
                frame.thrown.join(&v);
                frame.raise.join(&v);
                join_env(&mut frame.env, &self.env);
            }
            None => self.out.throws.join(&v),
        }
    }

    /// `v` arrives at `span` and continues along `path`, which must only carry trusted data.
    fn meet(&mut self, v: &Label, span: Span, what: &str, path: &SinkPath) {
        let here = StepData {
            module: self.module,
            span,
            note: what.to_owned(),
        };
        if let Some(chain) = &v.untrusted {
            if self.report {
                self.report(steps(chain), here.clone(), path);
            }
        }
        for (p, chain) in &v.params {
            self.out.sinks.entry(*p).or_insert_with(|| {
                let mut s = steps(chain);
                s.push(here.clone());
                s.extend(path.steps.iter().cloned());
                SinkPath {
                    steps: s,
                    sink: path.sink.clone(),
                }
            });
        }
    }

    fn report(&mut self, before: Vec<StepData>, here: StepData, path: &SinkPath) {
        let key = (self.module, here.span, path.sink.clone());
        if !self.t.reported.insert(key) {
            return;
        }
        // Numbered, because labels are shown in source order rather than path order.
        let mut path_steps: Vec<&StepData> = Vec::new();
        let mut seen = HashSet::new();
        for s in before.iter().chain([&here]).chain(&path.steps) {
            if seen.insert((s.module, s.span)) {
                path_steps.push(s);
            }
        }
        let here_n = path_steps
            .iter()
            .position(|s| s.module == here.module && s.span == here.span)
            .map_or(0, |i| i + 1);
        let mut d = Diagnostic::error(
            codes::UNTRUSTED_TO_SINK,
            format!("untrusted data reaches {}", path.sink),
            here.span,
        )
        .with_label(format!("{here_n}. {}", here.note));
        let mut notes = Vec::new();
        for (i, s) in path_steps.iter().enumerate() {
            let note = format!("{}. {}", i + 1, s.note);
            if s.module == here.module && s.span == here.span {
                continue;
            }
            if s.module == self.module {
                d = d.with_secondary(s.span, note);
            } else {
                let m = self.t.program.module(s.module);
                let lc = LineIndex::new(&m.src).line_col(s.span.start);
                notes.push(format!("{note} ({}:{}:{})", m.path, lc.line, lc.column));
            }
        }
        for n in notes {
            d = d.with_note(n);
        }
        d = d.with_help(
            "check the value with `validate(x, rule)?`, have a human `approve(x)` it, or, if it \
             is safe, `declassify(x, \"why\")`",
        );
        self.t.diags.push(ProgramDiagnostic {
            module: self.module,
            diagnostic: d,
        });
    }

    fn block(&mut self, b: &Block) -> Label {
        for &s in &b.stmts {
            self.stmt(s);
        }
        match b.tail {
            Some(e) => self.expr(e),
            None => Label::default(),
        }
    }

    fn stmt(&mut self, id: ward_syntax::ast::StmtId) {
        let ast = self.ast;
        let stmt = &ast.stmts[id];
        match &stmt.kind {
            StmtKind::Let { name, ty, init } => {
                let mut v = self.expr(*init);
                if let Some(ty) = *ty {
                    let ty_span = ast.types[ty].span;
                    match self.t.declared(self.module, ty) {
                        Declared::Trusted => self.meet(
                            &v,
                            self.span(*init),
                            "assigned here",
                            &SinkPath {
                                steps: Vec::new(),
                                sink: format!("`{}`, declared `Trusted`", name.name),
                            },
                        ),
                        Declared::Untrusted => {
                            v.join(&Label::untrusted(source(
                                self.module,
                                ty_span,
                                format!("`{}` is declared `Untrusted`", name.name),
                            )));
                        }
                        Declared::Plain => {}
                    }
                }
                if let Some(&local) = self.mres.stmt_locals.get(id) {
                    let v = self.step(&v, name.span, &format!("stored in `{}`", name.name));
                    self.env.insert(local, v);
                }
            }
            StmtKind::Assign { target, value } => self.assign(*target, *value),
            StmtKind::Expr { expr, .. } => {
                self.expr(*expr);
            }
            StmtKind::Return(e) => {
                let (v, span) = match e {
                    Some(e) => (self.expr(*e), self.span(*e)),
                    None => (Label::default(), stmt.span),
                };
                self.ret(v, span);
            }
            StmtKind::Throw(e) => {
                let v = self.expr(*e);
                self.throw(v);
            }
            StmtKind::For { var, iter, body } => {
                let it = self.expr(*iter);
                let it = self.step(&it, self.span(*iter), "iterated over here");
                let local = self.mres.stmt_locals.get(id).copied();
                let name = var.name.clone();
                let var_span = var.span;
                self.run_loop(
                    |cx| {
                        if let Some(local) = local {
                            let v =
                                cx.step(&it, var_span, &format!("each element becomes `{name}`"));
                            cx.env.insert(local, v);
                        }
                        it.clone()
                    },
                    body,
                );
            }
            StmtKind::While { cond, body } => {
                let cond = *cond;
                self.run_loop(
                    |cx| {
                        let c = cx.expr(cond);
                        cx.step(&c, cx.span(cond), "the loop runs while this holds")
                    },
                    body,
                );
            }
        }
    }

    /// Runs `head` then `body` until the labels of every variable stop changing. Everything
    /// the body writes depends on what `head` returns.
    fn run_loop(&mut self, mut head: impl FnMut(&mut Self) -> Label, body: &Block) {
        let saved = self.pc.clone();
        for _ in 0..MAX_ITERATIONS {
            let before = self.env.clone();
            let c = head(self);
            self.pc = saved.clone().joined(&c);
            self.block(body);
            join_env(&mut self.env, &before);
            if env_shape(&self.env) == env_shape(&before) {
                break;
            }
        }
        self.pc = saved;
    }

    fn root_local(&self, mut e: ExprId) -> Option<LocalId> {
        loop {
            match &self.ast.exprs[e].kind {
                ExprKind::Name(_) => {
                    return match self.mres.values.get(e) {
                        Some(ValueRes::Local(l)) => Some(*l),
                        _ => None,
                    };
                }
                ExprKind::Field { base, .. } | ExprKind::Index { base, .. } => e = *base,
                _ => return None,
            }
        }
    }

    fn assign(&mut self, target: ExprId, value: ExprId) {
        let v = self.expr(value);
        let pc = self.pc();
        let span = self.span(target);
        let mut v = v.joined(&self.step(
            &pc,
            span,
            "assigned here, where whether this runs depends on it",
        ));
        // Indices and field-declared labels, walking from the target to its variable.
        let mut e = target;
        let mut strong = true;
        loop {
            match &self.ast.exprs[e].kind {
                ExprKind::Index { base, index } => {
                    let i = self.expr(*index);
                    v.join(&i);
                    strong = false;
                    e = *base;
                }
                ExprKind::Field { base, name } => {
                    if let Some(Declared::Trusted) = self.field_declared(*base, &name.name) {
                        self.meet(
                            &v,
                            self.span(value),
                            "assigned here",
                            &SinkPath {
                                steps: Vec::new(),
                                sink: format!("the `Trusted` field `{}`", name.name),
                            },
                        );
                    }
                    strong = false;
                    e = *base;
                }
                _ => break,
            }
        }
        let Some(local) = self.root_local(target) else {
            return;
        };
        let name = &self.mres.locals[local].name;
        let v = self.step(&v, span, &format!("assigned to `{name}`"));
        if strong {
            self.env.insert(local, v);
        } else {
            self.env.entry(local).or_default().join(&v);
        }
    }

    /// The declared label of field `name` of the record `base` evaluates to.
    fn field_declared(&self, base: ExprId, name: &str) -> Option<Declared> {
        let Some(Ty::Adt(d, _)) = self.types.and_then(|t| t.exprs.get(base)) else {
            return None;
        };
        let Item::Record(r) = self.t.program.item(*d) else {
            return None;
        };
        let field = r.fields.iter().find(|f| f.name.name == name)?;
        Some(self.t.declared(d.module, field.ty))
    }

    fn exprs(&mut self, es: &[ExprId]) -> Vec<Label> {
        es.iter().map(|&e| self.expr(e)).collect()
    }

    fn join_all(ls: &[Label]) -> Label {
        ls.iter().fold(Label::default(), |a, b| a.joined(b))
    }

    fn expr(&mut self, e: ExprId) -> Label {
        let ast = self.ast;
        let span = self.span(e);
        match &ast.exprs[e].kind {
            ExprKind::Lit(_) | ExprKind::Error => Label::default(),
            ExprKind::Template(parts) => {
                let mut l = Label::default();
                for p in parts {
                    if let TemplatePart::Expr(x) = p {
                        l.join(&self.expr(*x));
                    }
                }
                l
            }
            ExprKind::Name(_) => match self.mres.values.get(e) {
                Some(ValueRes::Local(local)) => self.env.get(local).cloned().unwrap_or_default(),
                _ => Label::default(),
            },
            ExprKind::Field { base, name } => {
                if self.mres.values.get(e).is_some() {
                    return Label::default();
                }
                let b = self.expr(*base);
                match self.field_declared(*base, &name.name) {
                    Some(Declared::Untrusted) => b.joined(&Label::untrusted(source(
                        self.module,
                        name.span,
                        format!("field `{}` is declared `Untrusted`", name.name),
                    ))),
                    _ => b,
                }
            }
            ExprKind::Call { callee, args } => self.call(e, *callee, args),
            ExprKind::Index { base, index } => {
                let b = self.expr(*base);
                b.joined(&self.expr(*index))
            }
            ExprKind::Propagate(inner) => self.expr(*inner),
            ExprKind::Unary { operand, .. } => self.expr(*operand),
            ExprKind::Binary { op, lhs, rhs } => {
                let l = self.expr(*lhs);
                if matches!(op, BinOp::And | BinOp::Or) {
                    // The right side only runs depending on the left.
                    let saved = self.pc.clone();
                    self.pc.join(&l);
                    let r = self.expr(*rhs);
                    self.pc = saved;
                    l.joined(&r)
                } else {
                    l.joined(&self.expr(*rhs))
                }
            }
            ExprKind::List(items) => {
                let ls = self.exprs(items);
                Self::join_all(&ls)
            }
            ExprKind::Record { fields, .. } => {
                let record = self.mres.records.get(e).copied();
                let mut l = Label::default();
                for (i, f) in fields.iter().enumerate() {
                    let (v, vspan) = match f.value {
                        Some(x) => (self.expr(x), self.span(x)),
                        None => {
                            let v = match self.mres.shorthands.get(&(e, i)) {
                                Some(ValueRes::Local(local)) => {
                                    self.env.get(local).cloned().unwrap_or_default()
                                }
                                _ => Label::default(),
                            };
                            (v, f.name.span)
                        }
                    };
                    let declared = record.and_then(|d| match self.t.program.item(d) {
                        Item::Record(r) => r
                            .fields
                            .iter()
                            .find(|fd| fd.name.name == f.name.name)
                            .map(|fd| self.t.declared(d.module, fd.ty)),
                        _ => None,
                    });
                    if declared == Some(Declared::Trusted) {
                        self.meet(
                            &v,
                            vspan,
                            "assigned here",
                            &SinkPath {
                                steps: Vec::new(),
                                sink: format!("the `Trusted` field `{}`", f.name.name),
                            },
                        );
                    }
                    l.join(&v);
                }
                l
            }
            ExprKind::If { cond, then, else_ } => {
                let c = self.expr(*cond);
                let c = self.step(&c, self.span(*cond), "the branch taken depends on this");
                let saved_pc = self.pc.clone();
                let before = self.env.clone();
                self.pc.join(&c);
                let v1 = self.block(then);
                let after_then = std::mem::replace(&mut self.env, before);
                let v2 = match else_ {
                    Some(x) => self.expr(*x),
                    None => Label::default(),
                };
                join_env(&mut self.env, &after_then);
                self.pc = saved_pc;
                v1.joined(&v2).joined(&c)
            }
            ExprKind::Match { scrutinee, arms } => {
                let s = self.expr(*scrutinee);
                let s = self.step(&s, self.span(*scrutinee), "the arm taken depends on this");
                let saved_pc = self.pc.clone();
                let before = self.env.clone();
                let mut after = before.clone();
                self.pc.join(&s);
                let mut v = s.clone();
                for arm in arms {
                    self.env = before.clone();
                    self.bind(arm.pat, &s);
                    v.join(&self.expr(arm.body));
                    join_env(&mut after, &self.env);
                }
                self.env = after;
                self.pc = saved_pc;
                v
            }
            ExprKind::TryCatch { body, handler, .. } => {
                self.tries.push(TryFrame {
                    raise: Label::default(),
                    thrown: Label::default(),
                    env: HashMap::new(),
                });
                let vb = self.block(body);
                let frame = self.tries.pop().unwrap_or(TryFrame {
                    raise: Label::default(),
                    thrown: Label::default(),
                    env: HashMap::new(),
                });
                let thrown = self.step(
                    &frame.thrown,
                    span,
                    "whether this `try` threw depends on it",
                );
                let after_body = self.env.clone();
                join_env(&mut self.env, &frame.env);
                if let Some(&local) = self.mres.catch_locals.get(e) {
                    self.env.insert(local, thrown.clone());
                }
                let saved_pc = self.pc.clone();
                self.pc.join(&thrown);
                let vh = self.block(handler);
                self.pc = saved_pc;
                join_env(&mut self.env, &after_body);
                vb.joined(&vh).joined(&thrown)
            }
            ExprKind::Block(b) => self.block(b),
        }
    }

    fn bind(&mut self, p: PatId, l: &Label) {
        let ast = self.ast;
        match &ast.pats[p].kind {
            PatKind::Name(name) => {
                if let Some(&local) = self.mres.pat_bindings.get(p) {
                    let v = self.step(l, name.span, &format!("bound to `{}`", name.name));
                    self.env.insert(local, v);
                }
            }
            PatKind::Variant {
                args: Some(args), ..
            } => {
                let variant = match self.mres.pat_variants.get(p) {
                    Some(ValueRes::Variant(d, i)) => Some((*d, *i)),
                    _ => None,
                };
                for (j, &a) in args.iter().enumerate() {
                    let declared = variant.and_then(|(d, i)| match self.t.program.item(d) {
                        Item::Enum(en) => en
                            .variants
                            .get(i)
                            .and_then(|v| v.fields.get(j))
                            .map(|&ty| self.t.declared(d.module, ty)),
                        _ => None,
                    });
                    let sub = if declared == Some(Declared::Untrusted) {
                        l.clone().joined(&Label::untrusted(source(
                            self.module,
                            ast.pats[a].span,
                            "this variant's field is declared `Untrusted`".to_owned(),
                        )))
                    } else {
                        l.clone()
                    };
                    self.bind(a, &sub);
                }
            }
            _ => {}
        }
    }

    fn call(&mut self, e: ExprId, callee: ExprId, args: &[ExprId]) -> Label {
        let ast = self.ast;
        let span = self.span(e);
        let Some(&res) = self.mres.values.get(callee) else {
            // A method: its result depends on the receiver and the arguments.
            let mut l = match &ast.exprs[callee].kind {
                ExprKind::Field { base, .. } => self.expr(*base),
                _ => self.expr(callee),
            };
            for a in self.exprs(args) {
                l.join(&a);
            }
            return l;
        };
        match res {
            ValueRes::Fn(d) => {
                let ls = self.exprs(args);
                self.call_fn(d, span, args, &ls)
            }
            ValueRes::ToolMember(d) => {
                let name = self.src_text(self.span(callee));
                let program = self.t.program;
                let func = match &ast.exprs[callee].kind {
                    ExprKind::Field { name, .. } => program
                        .tool_schema(d)
                        .and_then(|s| s.function(&name.name))
                        .map(|f| (f, crate::tools::tool_sinks(program, d, &name.name))),
                    _ => None,
                };
                let ls = self.exprs(args);
                for (i, (&a, l)) in args.iter().zip(&ls).enumerate() {
                    let what = match &func {
                        Some((f, sinks)) => {
                            if !sinks
                                .as_ref()
                                .and_then(|s| s.get(i))
                                .copied()
                                .unwrap_or(true)
                            {
                                continue;
                            }
                            match f.params.get(i) {
                                Some(p) => format!("passed to `{name}` as `{}` here", p.name),
                                None => format!("passed to `{name}` here"),
                            }
                        }
                        None => format!("passed to `{name}` here"),
                    };
                    self.meet(
                        l,
                        self.span(a),
                        &what,
                        &SinkPath {
                            steps: Vec::new(),
                            sink: format!("the tool call `{name}`"),
                        },
                    );
                }
                Label::untrusted(source(
                    self.module,
                    span,
                    format!("result of the tool call `{name}`"),
                ))
            }
            ValueRes::Builtin(Builtin::Approve | Builtin::Declassify) => {
                self.exprs(args);
                Label::default()
            }
            ValueRes::Builtin(Builtin::Validate) => {
                let ls = self.exprs(args);
                let x = ls.first().cloned().unwrap_or_default();
                // The rule sees the unchecked value.
                if let (Some(&rule), Some(&xe)) = (args.get(1), args.first()) {
                    if let Some(&ValueRes::Fn(d)) = self.mres.values.get(rule) {
                        self.call_fn(d, span, &[xe], std::slice::from_ref(&x));
                    }
                }
                let t = self.step(&x, span, "`validate` throws depending on this value");
                self.throw(t);
                Label::default()
            }
            _ => {
                let ls = self.exprs(args);
                Self::join_all(&ls)
            }
        }
    }

    fn src_text(&self, span: Span) -> String {
        self.t
            .program
            .module(self.module)
            .src
            .get(span.range())
            .unwrap_or("?")
            .to_owned()
    }

    fn call_fn(&mut self, d: DefId, span: Span, args: &[ExprId], ls: &[Label]) -> Label {
        let Item::Fn(callee) = self.t.program.item(d) else {
            return Label::default();
        };
        let name = &callee.name.name;
        let summary = self.t.summaries.get(&d).cloned().unwrap_or_default();
        for (i, path) in &summary.sinks {
            let (Some(&a), Some(l)) = (args.get(*i), ls.get(*i)) else {
                continue;
            };
            let param = callee.params.get(*i).map_or("?", |p| p.name.name.as_str());
            self.meet(
                l,
                self.span(a),
                &format!("passed to `{name}` as `{param}`"),
                path,
            );
        }
        if callee.is_ai {
            return Label::untrusted(source(
                self.module,
                span,
                format!("output of `ai fn {name}`"),
            ));
        }
        let note = format!("returned by `{name}`");
        let instantiate = |cx: &Self, l: &Label| {
            let mut out = Label {
                untrusted: l
                    .untrusted
                    .as_ref()
                    .map(|c| push(c, cx.module, span, &note)),
                params: BTreeMap::new(),
            };
            for p in l.params.keys() {
                if let Some(a) = ls.get(*p) {
                    out.join(&cx.step(a, span, &note));
                }
            }
            out
        };
        let ret = instantiate(self, &summary.ret);
        if callee.throws.is_some() {
            let thrown = instantiate(self, &summary.throws);
            self.throw(thrown);
        }
        ret
    }
}
