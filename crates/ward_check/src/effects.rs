//! Effects (`uses {...}`), budgets and the Rule of Two.
//!
//! A function's effects are what its body does plus everything its callees do, so they
//! are computed to a fixpoint. Effects are named through the function's own imports:
//! `llm`, a whole tool (`mail`) or one tool function (`mail.send`).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ward_resolve::{
    Builtin, DefId, ModuleId, ModuleRes, Program, ProgramDiagnostic, Resolution, ValueRes,
};
use ward_syntax::ast::{
    Annotation, Block, ExprId, ExprKind, FnBody, FnDecl, ImportKind, Item, Lit, Module, StmtKind,
    TemplatePart, UnOp,
};
use ward_syntax::diag::codes;
use ward_syntax::{Diagnostic, Span};

use crate::trust::declares_untrusted;
use ward_resolve::tools::ToolServer;

const MAX_ITERATIONS: usize = 64;
/// Minimum call counts stop growing here, so recursion can't make them diverge.
const CALLS_CAP: u64 = 1 << 20;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Effect {
    Llm,
    Tool { source: String, func: String },
}

/// The Rule of Two's three capabilities.
const UNTRUSTED: usize = 0;
const PRIVATE: usize = 1;
const EXTERNAL: usize = 2;
const CAP_NAMES: [&str; 3] = [
    "processes untrusted input",
    "reads private data",
    "changes external state or communicates",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToolClass {
    Readonly,
    Private,
    External,
}

#[derive(Clone)]
struct Witness {
    span: Span,
    note: String,
}

#[derive(Clone, Default)]
struct Facts {
    effects: BTreeMap<Effect, Witness>,
    caps: [Option<Witness>; 3],
    min_calls: u64,
}

impl Facts {
    fn shape(&self) -> (Vec<Effect>, [bool; 3], u64) {
        (
            self.effects.keys().cloned().collect(),
            self.caps.each_ref().map(Option::is_some),
            self.min_calls,
        )
    }

    fn has_all_caps(&self) -> bool {
        self.caps.iter().all(Option::is_some)
    }
}

enum Event {
    Llm(Span, String),
    Tool {
        source: String,
        func: String,
        class: ToolClass,
        span: Span,
        used: bool,
    },
    Call(DefId, Span),
    Untrusted(Span, String),
}

const BUDGET_KEYS: [(&str, bool); 4] = [
    ("tokens", true),
    ("calls", true),
    ("cost", false),
    ("time", false),
];

pub(crate) fn check(program: &Program, res: &Resolution) -> Vec<ProgramDiagnostic> {
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
    let mut cx = Cx {
        program,
        res,
        diags: Vec::new(),
        facts: HashMap::new(),
    };
    cx.check_import_annotations();
    let events: HashMap<DefId, Vec<Event>> = fns
        .iter()
        .map(|&(def, f)| (def, cx.events(def, f)))
        .collect();
    for _ in 0..MAX_ITERATIONS {
        let mut changed = false;
        for &(def, f) in &fns {
            let facts = cx.facts_of(def, f, events.get(&def).map_or(&[][..], Vec::as_slice));
            if cx.facts.get(&def).map(Facts::shape) != Some(facts.shape()) {
                changed = true;
            }
            cx.facts.insert(def, facts);
        }
        if !changed {
            break;
        }
    }
    for &(def, f) in &fns {
        let evs = events.get(&def).map_or(&[][..], Vec::as_slice);
        cx.check_effects(def, f);
        let limits = cx.check_budget(def, f);
        cx.check_callee_budgets(def, evs, &limits);
        cx.check_rule_of_two(def, f, evs);
    }
    cx.diags
}

struct Cx<'p> {
    program: &'p Program,
    res: &'p Resolution,
    diags: Vec<ProgramDiagnostic>,
    facts: HashMap<DefId, Facts>,
}

fn names(annotation: &Annotation) -> impl Iterator<Item = &str> {
    annotation
        .args
        .iter()
        .filter(|a| a.value.is_none())
        .map(|a| a.name.name.as_str())
}

fn unknown_import_annotation(a: &Annotation) -> Diagnostic {
    Diagnostic::error(
        codes::INVALID_ANNOTATION,
        format!("unknown annotation `{}` on an import", a.name.name),
        a.name.span,
    )
    .with_label("not recognized here")
    .with_help(
        "tool imports take `@private(f, ...)`, `@readonly(f, ...)`, `@sink(f.param, ...)` \
         and `@not_sink(f.param, ..., reason = \"...\")`",
    )
}

fn invalid(span: Span, message: String, label: &str) -> Diagnostic {
    Diagnostic::error(codes::INVALID_ANNOTATION, message, span).with_label(label.to_owned())
}

/// Checks an annotation on a tool import against the tool's schema, if it has one.
fn import_annotation_error(a: &Annotation, schema: Option<&ToolServer>) -> Option<Diagnostic> {
    let kind = a.name.name.as_str();
    let params = matches!(kind, "sink" | "not_sink");
    if !params && !matches!(kind, "private" | "readonly") {
        return Some(unknown_import_annotation(a));
    }
    if params && schema.is_none() {
        return Some(
            invalid(
                a.name.span,
                format!("`@{kind}` needs the tool's schema"),
                "this tool isn't in `ward.lock`",
            )
            .with_help("add the server to `mcp.json` and run `ward lock`; without a schema every argument is a sink"),
        );
    }
    let mut reason = None;
    for arg in &a.args {
        match &arg.value {
            Some((text, _)) if kind == "not_sink" && arg.name.name == "reason" => {
                reason = Some(text.trim());
            }
            Some(_) => {
                return Some(invalid(
                    arg.span,
                    format!("`@{kind}` doesn't take `{}`", arg.name.name),
                    if params {
                        "expected a parameter, like `send.body`"
                    } else {
                        "expected a name, like `read_file`"
                    },
                ));
            }
            None => {
                let (func, param) = match arg.name.name.split_once('.') {
                    Some((f, p)) if params => (f, Some(p)),
                    None if !params => (arg.name.name.as_str(), None),
                    _ => {
                        return Some(invalid(
                            arg.span,
                            if params {
                                format!("`@{kind}` takes tool parameters, like `send.body`")
                            } else {
                                format!("`@{kind}` takes tool function names")
                            },
                            "not a valid argument here",
                        ));
                    }
                };
                let Some(schema) = schema else { continue };
                let Some(f) = schema.function(func) else {
                    let d = invalid(
                        arg.name.span,
                        format!("tool `{}` has no function `{func}`", schema.source),
                        "unknown tool function",
                    );
                    let names: Vec<&str> =
                        schema.functions.iter().map(|f| f.name.as_str()).collect();
                    return Some(match ward_resolve::did_you_mean(func, names) {
                        Some(s) => d.with_help(format!("a similar name exists: `{s}`")),
                        None => d,
                    });
                };
                if let Some(p) = param {
                    if !f.params.iter().any(|x| x.name == p) {
                        let names: Vec<String> =
                            f.params.iter().map(|x| format!("`{}`", x.name)).collect();
                        return Some(
                            invalid(
                                arg.name.span,
                                format!("`{}.{func}` has no parameter `{p}`", schema.source),
                                "unknown parameter",
                            )
                            .with_help(format!("its parameters are {}", names.join(", "))),
                        );
                    }
                }
            }
        }
    }
    if kind == "not_sink" && reason.is_none_or(str::is_empty) {
        return Some(
            invalid(
                a.name.span,
                "`@not_sink` needs a reason".to_owned(),
                "say why untrusted data may reach these parameters",
            )
            .with_help("add `reason = \"...\"`; it's what a reviewer reads"),
        );
    }
    None
}

impl<'p> Cx<'p> {
    fn err(&mut self, module: ModuleId, d: Diagnostic) {
        self.diags.push(ProgramDiagnostic {
            module,
            diagnostic: d,
        });
    }

    fn tool_source(&self, def: DefId) -> Option<&'p str> {
        match self.program.item(def) {
            Item::Import(i) => match &i.kind {
                ImportKind::Tool { source, .. } => Some(source.as_str()),
                ImportKind::Module(_) => None,
            },
            _ => None,
        }
    }

    fn tool_class(&self, def: DefId, func: &str) -> ToolClass {
        let Item::Import(i) = self.program.item(def) else {
            return ToolClass::External;
        };
        // The schema's hint, unless an annotation says otherwise.
        let hinted = self
            .program
            .tool_schema(def)
            .and_then(|s| s.function(func))
            .is_some_and(|f| f.read_only);
        for a in &i.annotations {
            if names(a).any(|n| n == func) {
                match a.name.name.as_str() {
                    "private" => return ToolClass::Private,
                    "readonly" => return ToolClass::Readonly,
                    _ => {}
                }
            }
        }
        if hinted {
            ToolClass::Readonly
        } else {
            ToolClass::External
        }
    }

    /// The name a module imports tool `source` by.
    fn alias_in(&self, module: ModuleId, source: &str) -> Option<&'p str> {
        self.program
            .module(module)
            .ast
            .items
            .iter()
            .find_map(|it| match it {
                Item::Import(i) => match &i.kind {
                    ImportKind::Tool { source: s, .. } if s == source => {
                        i.alias.as_ref().map(|a| a.name.as_str())
                    }
                    _ => None,
                },
                _ => None,
            })
    }

    fn show(&self, module: ModuleId, e: &Effect) -> String {
        match e {
            Effect::Llm => "llm".to_owned(),
            Effect::Tool { source, func } => match self.alias_in(module, source) {
                Some(alias) => format!("{alias}.{func}"),
                None => format!("\"{source}\".{func}"),
            },
        }
    }

    fn check_import_annotations(&mut self) {
        let program = self.program;
        for m in program.module_ids() {
            for it in &program.module(m).ast.items {
                let Item::Import(i) = it else { continue };
                let schema = match &i.kind {
                    ImportKind::Tool { source, .. } => Some(program.tool_server(source)),
                    ImportKind::Module(_) => None,
                };
                for a in &i.annotations {
                    let d = match schema {
                        Some(schema) => import_annotation_error(a, schema),
                        None => Some(unknown_import_annotation(a)),
                    };
                    if let Some(d) = d {
                        self.err(m, d);
                    }
                }
            }
        }
    }

    fn events(&self, def: DefId, f: &'p FnDecl) -> Vec<Event> {
        let module = def.module;
        let mut w = Walker {
            cx: self,
            ast: &self.program.module(module).ast,
            mres: self.res.module(module),
            out: Vec::new(),
        };
        for p in &f.params {
            if declares_untrusted(self.program, self.res, module, p.ty) {
                w.out.push(Event::Untrusted(
                    p.span,
                    format!("`{}` is declared `Untrusted`", p.name.name),
                ));
            }
        }
        match &f.body {
            FnBody::Block(b) => w.block(b, false),
            FnBody::Ai { .. } => w.out.push(Event::Llm(
                f.name.span,
                "an `ai fn` calls the model".to_owned(),
            )),
        }
        // Checks run with the call: what they use, the `ai fn` uses.
        for e in f.checks.iter().flat_map(|c| &c.entries) {
            w.expr(e.cond, true);
        }
        w.out
    }

    fn facts_of(&self, def: DefId, f: &'p FnDecl, events: &[Event]) -> Facts {
        let mut facts = Facts::default();
        let add_cap = |facts: &mut Facts, i: usize, span: Span, note: String| {
            facts.caps[i].get_or_insert(Witness { span, note });
        };
        for ev in events {
            match ev {
                Event::Llm(span, note) => {
                    facts.effects.entry(Effect::Llm).or_insert(Witness {
                        span: *span,
                        note: note.clone(),
                    });
                }
                Event::Tool {
                    source,
                    func,
                    class,
                    span,
                    used,
                } => {
                    let e = Effect::Tool {
                        source: source.clone(),
                        func: func.clone(),
                    };
                    let shown = self.show(def.module, &e);
                    facts.effects.entry(e).or_insert(Witness {
                        span: *span,
                        note: format!("calls `{shown}` here"),
                    });
                    match class {
                        ToolClass::Private => add_cap(
                            &mut facts,
                            PRIVATE,
                            *span,
                            format!("`{shown}` reads private data"),
                        ),
                        ToolClass::External => add_cap(
                            &mut facts,
                            EXTERNAL,
                            *span,
                            format!("`{shown}` may change external state"),
                        ),
                        ToolClass::Readonly => {}
                    }
                    // Private data is the system's own; it's the second capability, not the first.
                    if *used && *class != ToolClass::Private {
                        add_cap(
                            &mut facts,
                            UNTRUSTED,
                            *span,
                            format!("the result of `{shown}` is untrusted"),
                        );
                    }
                }
                Event::Untrusted(span, note) => {
                    add_cap(&mut facts, UNTRUSTED, *span, note.clone());
                }
                Event::Call(callee, span) => {
                    let Some(cf) = self.facts.get(callee) else {
                        continue;
                    };
                    let name = fn_name(self.program, *callee);
                    let is_ai = matches!(self.program.item(*callee), Item::Fn(f) if f.is_ai);
                    for e in cf.effects.keys() {
                        let shown = self.show(def.module, e);
                        let note = if is_ai {
                            format!("calls `ai fn {name}`, which uses `{shown}`")
                        } else {
                            format!("calls `{name}`, which uses `{shown}`")
                        };
                        facts
                            .effects
                            .entry(e.clone())
                            .or_insert(Witness { span: *span, note });
                    }
                    for (i, c) in cf.caps.iter().enumerate() {
                        if c.is_some() {
                            add_cap(
                                &mut facts,
                                i,
                                *span,
                                format!("calls `{name}`, which {}", CAP_NAMES[i]),
                            );
                        }
                    }
                }
            }
        }
        facts.min_calls = match &f.body {
            FnBody::Ai { .. } => 1,
            FnBody::Block(b) => {
                let module = def.module;
                let mc = MinCalls {
                    cx: self,
                    ast: &self.program.module(module).ast,
                    mres: self.res.module(module),
                };
                mc.block(b)
            }
        };
        facts
    }

    fn check_effects(&mut self, def: DefId, f: &FnDecl) {
        let module = def.module;
        let Some(facts) = self.facts.get(&def).cloned() else {
            return;
        };
        // What each declared effect covers: `None` for a whole tool.
        let mut declared: Vec<(Span, Effect, bool, bool)> = Vec::new();
        for path in f.uses.iter().flatten() {
            let segs: Vec<&str> = path.segments.iter().map(|s| s.name.as_str()).collect();
            let tool = |name: &str| {
                self.program
                    .module(module)
                    .ast
                    .items
                    .iter()
                    .find_map(|it| match it {
                        Item::Import(i) if i.alias.as_ref().is_some_and(|a| a.name == name) => {
                            match &i.kind {
                                ImportKind::Tool { source, .. } => Some(source.clone()),
                                ImportKind::Module(_) => None,
                            }
                        }
                        _ => None,
                    })
            };
            let effect = match segs.as_slice() {
                ["llm"] => Some((Effect::Llm, false)),
                [t] => tool(t).map(|source| {
                    (
                        Effect::Tool {
                            source,
                            func: String::new(),
                        },
                        true,
                    )
                }),
                [t, func] => tool(t).map(|source| {
                    (
                        Effect::Tool {
                            source,
                            func: (*func).to_owned(),
                        },
                        false,
                    )
                }),
                _ => None,
            };
            match effect {
                Some((e, whole)) => declared.push((path.span, e, whole, false)),
                None => {
                    let text = segs.join(".");
                    self.err(
                        module,
                        Diagnostic::error(
                            codes::UNKNOWN_EFFECT,
                            format!("unknown effect `{text}`"),
                            path.span,
                        )
                        .with_label("not an effect")
                        .with_help(
                            "effects are `llm`, a tool imported with `import mcp`, or one of its \
                             functions, like `mail.send`",
                        ),
                    );
                }
            }
        }
        let covers = |d: &Effect, whole: bool, e: &Effect| match (d, e) {
            (Effect::Llm, Effect::Llm) => true,
            (
                Effect::Tool {
                    source: ds,
                    func: df,
                },
                Effect::Tool {
                    source: es,
                    func: ef,
                },
            ) => ds == es && (whole || df == ef),
            _ => false,
        };
        let is_ai = f.is_ai;
        let mut missing = Vec::new();
        for (e, w) in &facts.effects {
            // An `ai fn` needn't declare the `llm` it implies.
            if is_ai && *e == Effect::Llm {
                continue;
            }
            let mut found = false;
            for d in &mut declared {
                if covers(&d.1, d.2, e) {
                    d.3 = true;
                    found = true;
                }
            }
            if !found {
                missing.push((e.clone(), w.clone()));
            }
        }
        if !missing.is_empty() {
            let mut clause: Vec<String> = f
                .uses
                .iter()
                .flatten()
                .map(|p| {
                    p.segments
                        .iter()
                        .map(|s| s.name.as_str())
                        .collect::<Vec<_>>()
                        .join(".")
                })
                .collect();
            for (e, _) in &missing {
                clause.push(self.show(module, e));
            }
            let clause = format!("uses {{{}}}", clause.join(", "));
            for (e, w) in missing {
                let shown = self.show(module, &e);
                let mut d = Diagnostic::error(
                    codes::MISSING_EFFECT,
                    format!("`{}` uses `{shown}` but doesn't declare it", f.name.name),
                    w.span,
                )
                .with_label(w.note)
                .with_secondary(f.name.span, "in this function");
                d = match &e {
                    Effect::Tool { source, .. } if self.alias_in(module, source).is_none() => d
                        .with_help(format!(
                            "import the tool to name its effect, e.g. `import mcp \"{source}\" as x`, then declare `uses {{x}}`"
                        )),
                    _ => d.with_help(format!("declare it: `{clause}`")),
                };
                self.err(module, d);
            }
        }
        for (span, e, _, used) in declared {
            let implied = is_ai && e == Effect::Llm;
            if !used {
                let d = if implied {
                    Diagnostic::warning(
                        codes::UNUSED_EFFECT,
                        "`ai fn` implies `llm`; declaring it is redundant",
                        span,
                    )
                    .with_label("remove this")
                } else {
                    Diagnostic::warning(
                        codes::UNUSED_EFFECT,
                        format!("`{}` declares an effect it doesn't use", f.name.name),
                        span,
                    )
                    .with_label("never used")
                    .with_help("remove it, so the declared effects say what the function does")
                };
                self.err(module, d);
            }
        }
    }

    /// Validates the `budget` clause and returns its limits.
    fn check_budget(&mut self, def: DefId, f: &FnDecl) -> BTreeMap<&'static str, (f64, Span)> {
        let module = def.module;
        let ast = &self.program.module(module).ast;
        let mut limits = BTreeMap::new();
        for entry in f.budget.iter().flatten() {
            let name = entry.name.name.as_str();
            let Some(&(key, integer)) = BUDGET_KEYS.iter().find(|(k, _)| *k == name) else {
                self.err(
                    module,
                    Diagnostic::error(
                        codes::INVALID_BUDGET,
                        format!("unknown budget `{name}`"),
                        entry.name.span,
                    )
                    .with_label("not a budget")
                    .with_help(
                        "budgets are `tokens`, `calls`, `cost` (dollars) and `time` (seconds)",
                    ),
                );
                continue;
            };
            let span = ast.exprs[entry.value].span;
            let value = match &ast.exprs[entry.value].kind {
                ExprKind::Lit(Lit::Int(n)) => Some(*n as f64),
                ExprKind::Lit(Lit::Float(s)) if !integer => s.parse::<f64>().ok(),
                _ => None,
            };
            let Some(value) = value else {
                let what = if integer {
                    "a whole number"
                } else {
                    "a number"
                };
                let negative = matches!(
                    ast.exprs[entry.value].kind,
                    ExprKind::Unary { op: UnOp::Neg, .. }
                );
                let label = if negative {
                    "budgets can't be negative".to_owned()
                } else {
                    format!("expected {what} written out")
                };
                self.err(
                    module,
                    Diagnostic::error(
                        codes::INVALID_BUDGET,
                        format!("budget `{name}` must be {what}"),
                        span,
                    )
                    .with_label(label)
                    .with_help("budgets are checked when compiling, so they must be literals"),
                );
                continue;
            };
            if let Some((_, first)) = limits.get(key) {
                let first: Span = *first;
                self.err(
                    module,
                    Diagnostic::error(
                        codes::INVALID_BUDGET,
                        format!("budget `{name}` is set twice"),
                        entry.name.span,
                    )
                    .with_label("set again here")
                    .with_secondary(first, "first set here"),
                );
                continue;
            }
            limits.insert(key, (value, span));
        }
        if let (Some(&(calls, span)), Some(facts)) = (limits.get("calls"), self.facts.get(&def)) {
            let min = facts.min_calls;
            if (min as f64) > calls {
                self.err(
                    module,
                    Diagnostic::error(
                        codes::BUDGET_EXCEEDED,
                        format!(
                            "`{}` always makes at least {min} model {}, but its budget allows {calls}",
                            f.name.name,
                            if min == 1 { "call" } else { "calls" }
                        ),
                        span,
                    )
                    .with_label("this budget is always exceeded")
                    .with_help("raise the budget, or make fewer model calls"),
                );
            }
        }
        limits
    }

    fn budget_of(&self, def: DefId, key: &str) -> Option<f64> {
        let Item::Fn(f) = self.program.item(def) else {
            return None;
        };
        let ast = &self.program.module(def.module).ast;
        f.budget
            .iter()
            .flatten()
            .find(|e| e.name.name == key)
            .and_then(|e| match &ast.exprs[e.value].kind {
                ExprKind::Lit(Lit::Int(n)) => Some(*n as f64),
                ExprKind::Lit(Lit::Float(s)) => s.parse().ok(),
                _ => None,
            })
    }

    fn check_callee_budgets(
        &mut self,
        def: DefId,
        events: &[Event],
        limits: &BTreeMap<&'static str, (f64, Span)>,
    ) {
        let caller = fn_name(self.program, def);
        let mut seen = BTreeSet::new();
        for ev in events {
            let Event::Call(callee, span) = ev else {
                continue;
            };
            for (key, (limit, limit_span)) in limits {
                let Some(theirs) = self.budget_of(*callee, key) else {
                    continue;
                };
                if theirs > *limit && seen.insert((callee.module, callee.item, *key)) {
                    let name = fn_name(self.program, *callee);
                    self.err(
                        def.module,
                        Diagnostic::warning(
                            codes::CALLEE_BUDGET,
                            format!(
                                "`{name}`'s `{key}` budget ({theirs}) is more than `{caller}`'s ({limit})"
                            ),
                            *span,
                        )
                        .with_label(format!("`{name}` can't use its whole budget here"))
                        .with_secondary(*limit_span, format!("`{caller}`'s budget"))
                        .with_help(format!("lower `{name}`'s budget, or raise `{caller}`'s")),
                    );
                }
            }
        }
    }

    fn check_rule_of_two(&mut self, def: DefId, f: &FnDecl, events: &[Event]) {
        let module = def.module;
        let mut allow: Option<Span> = None;
        for a in &f.annotations {
            if a.name.name != "allow" {
                self.err(
                    module,
                    Diagnostic::error(
                        codes::INVALID_ANNOTATION,
                        format!("unknown annotation `{}`", a.name.name),
                        a.name.span,
                    )
                    .with_label("not recognized")
                    .with_help("functions take `@allow(rule_of_two, reason = \"...\")`"),
                );
                continue;
            }
            let mut ok = true;
            let mut lint = false;
            let mut reason = None;
            for arg in &a.args {
                match (arg.name.name.as_str(), &arg.value) {
                    ("rule_of_two", None) => lint = true,
                    ("reason", Some((text, span))) => {
                        reason = Some((text.trim().is_empty(), *span))
                    }
                    _ => {
                        ok = false;
                        self.err(
                            module,
                            Diagnostic::error(
                                codes::INVALID_ANNOTATION,
                                format!("`@allow` doesn't take `{}`", arg.name.name),
                                arg.span,
                            )
                            .with_label("not recognized")
                            .with_help("write `@allow(rule_of_two, reason = \"...\")`"),
                        );
                    }
                }
            }
            if !lint && ok {
                ok = false;
                self.err(
                    module,
                    Diagnostic::error(
                        codes::INVALID_ANNOTATION,
                        "`@allow` must name what it allows",
                        a.span,
                    )
                    .with_label("allows nothing")
                    .with_help("write `@allow(rule_of_two, reason = \"...\")`"),
                );
            }
            match reason {
                Some((true, span)) => {
                    ok = false;
                    self.err(
                        module,
                        Diagnostic::error(
                            codes::INVALID_ANNOTATION,
                            "the reason for an `@allow` can't be empty",
                            span,
                        )
                        .with_label("say why this is safe"),
                    );
                }
                None if ok => {
                    ok = false;
                    self.err(
                        module,
                        Diagnostic::error(
                            codes::INVALID_ANNOTATION,
                            "`@allow(rule_of_two)` needs a reason",
                            a.span,
                        )
                        .with_label("no reason given")
                        .with_help("say why this is safe: `@allow(rule_of_two, reason = \"...\")`"),
                    );
                }
                _ => {}
            }
            if ok {
                allow = Some(a.span);
            }
        }
        let Some(facts) = self.facts.get(&def) else {
            return;
        };
        // Report where the three first meet, not again in every caller.
        let callee_violates = events.iter().any(|ev| match ev {
            Event::Call(c, _) => self.facts.get(c).is_some_and(Facts::has_all_caps),
            _ => false,
        });
        let violates = facts.has_all_caps() && !callee_violates;
        match (violates, allow) {
            (true, None) => {
                let mut d = Diagnostic::error(
                    codes::RULE_OF_TWO,
                    format!(
                        "`{}` breaks the Rule of Two: it processes untrusted input, reads private data and changes external state",
                        f.name.name
                    ),
                    f.name.span,
                )
                .with_label("has all three");
                for w in facts.caps.iter().flatten() {
                    d = d.with_secondary(w.span, w.note.clone());
                }
                d = d.with_help(
                    "split it so no function has all three, or, if a human reviews what it does, \
                     `@allow(rule_of_two, reason = \"...\")`",
                );
                self.err(module, d);
            }
            (false, Some(span)) if !facts.has_all_caps() => {
                self.err(
                    module,
                    Diagnostic::warning(
                        codes::UNNECESSARY_ALLOW,
                        format!("`{}` doesn't break the Rule of Two", f.name.name),
                        span,
                    )
                    .with_label("nothing to allow")
                    .with_help("remove the annotation"),
                );
            }
            _ => {}
        }
    }
}

fn fn_name(program: &Program, def: DefId) -> &str {
    match program.item(def) {
        Item::Fn(f) => &f.name.name,
        _ => "?",
    }
}

struct Walker<'a, 'p> {
    cx: &'a Cx<'p>,
    ast: &'p Module,
    mres: &'p ModuleRes,
    out: Vec<Event>,
}

impl Walker<'_, '_> {
    fn block(&mut self, b: &Block, used: bool) {
        for &s in &b.stmts {
            match &self.ast.stmts[s].kind {
                StmtKind::Let { init, .. } => self.expr(*init, true),
                StmtKind::Assign { target, value } => {
                    self.expr(*target, true);
                    self.expr(*value, true);
                }
                StmtKind::Expr { expr, .. } => self.expr(*expr, false),
                StmtKind::Return(e) => {
                    if let Some(e) = e {
                        self.expr(*e, true);
                    }
                }
                StmtKind::Throw(e) => self.expr(*e, true),
                StmtKind::For { iter, body, .. } => {
                    self.expr(*iter, true);
                    self.block(body, false);
                }
                StmtKind::While { cond, body } => {
                    self.expr(*cond, true);
                    self.block(body, false);
                }
            }
        }
        if let Some(t) = b.tail {
            self.expr(t, used);
        }
    }

    fn expr(&mut self, e: ExprId, used: bool) {
        let ast = self.ast;
        match &ast.exprs[e].kind {
            ExprKind::Lit(_) | ExprKind::Name(_) | ExprKind::Error => {}
            ExprKind::Template(parts) => {
                for p in parts {
                    if let TemplatePart::Expr(x) = p {
                        self.expr(*x, true);
                    }
                }
            }
            ExprKind::Field { base, .. } => {
                if self.mres.values.get(e).is_none() {
                    self.expr(*base, true);
                }
            }
            ExprKind::Call { callee, args } => {
                self.call(e, *callee, args, used);
            }
            ExprKind::Index { base, index } => {
                self.expr(*base, true);
                self.expr(*index, true);
            }
            ExprKind::Propagate(inner) => self.expr(*inner, used),
            ExprKind::Unary { operand, .. } => self.expr(*operand, true),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(*lhs, true);
                self.expr(*rhs, true);
            }
            ExprKind::List(items) => {
                for &x in items {
                    self.expr(x, true);
                }
            }
            ExprKind::Record { fields, .. } => {
                for f in fields {
                    if let Some(v) = f.value {
                        self.expr(v, true);
                    }
                }
            }
            ExprKind::If { cond, then, else_ } => {
                self.expr(*cond, true);
                self.block(then, used);
                if let Some(x) = else_ {
                    self.expr(*x, used);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expr(*scrutinee, true);
                for a in arms {
                    self.expr(a.body, used);
                }
            }
            ExprKind::TryCatch { body, handler, .. } => {
                self.block(body, used);
                self.block(handler, used);
            }
            ExprKind::Block(b) => self.block(b, used),
        }
    }

    fn call(&mut self, e: ExprId, callee: ExprId, args: &[ExprId], used: bool) {
        let ast = self.ast;
        let span = ast.exprs[e].span;
        match self.mres.values.get(callee) {
            None => {
                if let ExprKind::Field { base, .. } = &ast.exprs[callee].kind {
                    self.expr(*base, true);
                }
            }
            Some(&ValueRes::Fn(d)) => {
                self.out.push(Event::Call(d, span));
                if let (Item::Fn(f), true) = (self.cx.program.item(d), used) {
                    if f.is_ai {
                        self.out.push(Event::Untrusted(
                            span,
                            format!("the output of `ai fn {}` is untrusted", f.name.name),
                        ));
                    }
                }
            }
            Some(&ValueRes::ToolMember(d)) => {
                if let (ExprKind::Field { name, .. }, Some(source)) =
                    (&ast.exprs[callee].kind, self.cx.tool_source(d))
                {
                    self.out.push(Event::Tool {
                        source: source.to_owned(),
                        func: name.name.clone(),
                        class: self.cx.tool_class(d, &name.name),
                        span,
                        used,
                    });
                }
            }
            Some(ValueRes::Builtin(Builtin::Validate)) => {
                if let Some(&rule) = args.get(1) {
                    if let Some(&ValueRes::Fn(d)) = self.mres.values.get(rule) {
                        self.out.push(Event::Call(d, span));
                    }
                }
                if let Some(&x) = args.first() {
                    self.expr(x, true);
                }
                return;
            }
            _ => {}
        }
        for &a in args {
            self.expr(a, true);
        }
    }
}

/// The fewest model calls any run of a function makes.
struct MinCalls<'a, 'p> {
    cx: &'a Cx<'p>,
    ast: &'p Module,
    mres: &'p ModuleRes,
}

impl MinCalls<'_, '_> {
    fn block(&self, b: &Block) -> u64 {
        let mut n: u64 = 0;
        for &s in &b.stmts {
            match &self.ast.stmts[s].kind {
                StmtKind::Let { init, .. } => n += self.expr(*init),
                StmtKind::Assign { target, value } => {
                    n += self.expr(*target) + self.expr(*value);
                }
                StmtKind::Expr { expr, .. } => n += self.expr(*expr),
                StmtKind::Return(e) => {
                    return (n + e.map_or(0, |e| self.expr(e))).min(CALLS_CAP);
                }
                StmtKind::Throw(e) => return (n + self.expr(*e)).min(CALLS_CAP),
                // The body may run zero times.
                StmtKind::For { iter, .. } => n += self.expr(*iter),
                StmtKind::While { cond, .. } => n += self.expr(*cond),
            }
        }
        (n + b.tail.map_or(0, |t| self.expr(t))).min(CALLS_CAP)
    }

    fn all(&self, es: &[ExprId]) -> u64 {
        es.iter().map(|&e| self.expr(e)).sum()
    }

    fn expr(&self, e: ExprId) -> u64 {
        let n = match &self.ast.exprs[e].kind {
            ExprKind::Lit(_) | ExprKind::Name(_) | ExprKind::Error => 0,
            ExprKind::Template(parts) => parts
                .iter()
                .map(|p| match p {
                    TemplatePart::Expr(x) => self.expr(*x),
                    TemplatePart::Lit(_) => 0,
                })
                .sum(),
            ExprKind::Field { base, .. } => {
                if self.mres.values.get(e).is_some() {
                    0
                } else {
                    self.expr(*base)
                }
            }
            ExprKind::Call { callee, args } => {
                let own = match self.mres.values.get(*callee) {
                    Some(ValueRes::Fn(d)) => self.cx.facts.get(d).map_or(0, |f| f.min_calls),
                    None => match &self.ast.exprs[*callee].kind {
                        ExprKind::Field { base, .. } => self.expr(*base),
                        _ => 0,
                    },
                    _ => 0,
                };
                own + self.all(args)
            }
            ExprKind::Index { base, index } => self.expr(*base) + self.expr(*index),
            ExprKind::Propagate(x) | ExprKind::Unary { operand: x, .. } => self.expr(*x),
            // The right side of `&&` and `||` may not run.
            ExprKind::Binary { op, lhs, rhs } => {
                use ward_syntax::ast::BinOp;
                if matches!(op, BinOp::And | BinOp::Or) {
                    self.expr(*lhs)
                } else {
                    self.expr(*lhs) + self.expr(*rhs)
                }
            }
            ExprKind::List(items) => self.all(items),
            ExprKind::Record { fields, .. } => fields
                .iter()
                .filter_map(|f| f.value)
                .map(|v| self.expr(v))
                .sum(),
            ExprKind::If { cond, then, else_ } => {
                let t = self.block(then);
                let el = else_.map_or(0, |x| self.expr(x));
                self.expr(*cond) + t.min(el)
            }
            ExprKind::Match { scrutinee, arms } => {
                let arms = arms.iter().map(|a| self.expr(a.body)).min().unwrap_or(0);
                self.expr(*scrutinee) + arms
            }
            // Anything in the `try` may throw before it calls the model.
            ExprKind::TryCatch { .. } => 0,
            ExprKind::Block(b) => self.block(b),
        };
        n.min(CALLS_CAP)
    }
}
