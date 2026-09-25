//! Classes and interfaces: inheritance, overriding, abstract methods and `init`
//! rules, and member lookup through supertypes.

use std::collections::HashMap;

use ward_resolve::{DefId, TypeRes, ValueRes};
use ward_syntax::Diagnostic;
use ward_syntax::ast::{ClassKind, ExprKind, FnBody, FnDecl, Item, StmtKind};
use ward_syntax::diag::codes;

use crate::ty::Ty;
use crate::{Checker, ClassInfo};

/// `class` and its supertypes: the class, its bases nearest first, then the interfaces
/// they implement and those extend. Stops at a cycle (reported by `check_classes`).
pub fn ancestors(classes: &HashMap<DefId, ClassInfo>, class: DefId) -> Vec<DefId> {
    let mut out = vec![class];
    let mut cur = class;
    while let Some(base) = classes.get(&cur).and_then(|c| c.base) {
        if out.contains(&base) {
            break;
        }
        out.push(base);
        cur = base;
    }
    let mut i = 0;
    while i < out.len() {
        for &iface in classes.get(&out[i]).map_or(&[][..], |c| &c.interfaces) {
            if !out.contains(&iface) {
                out.push(iface);
            }
        }
        i += 1;
    }
    out
}

/// Whether an object of `sub` can be used as a `sup`.
pub fn is_subclass(classes: &HashMap<DefId, ClassInfo>, sub: DefId, sup: DefId) -> bool {
    ancestors(classes, sub).contains(&sup)
}

/// The field `name` of `class` or a base: the class declaring it, its type, and `pub`.
pub fn find_field(
    classes: &HashMap<DefId, ClassInfo>,
    class: DefId,
    name: &str,
) -> Option<(DefId, Ty, bool)> {
    ancestors(classes, class).into_iter().find_map(|c| {
        let info = classes.get(&c)?;
        let (_, t, is_pub) = info.fields.iter().find(|(n, ..)| n == name)?;
        Some((c, t.clone(), *is_pub))
    })
}

/// The method `name` of `class` or a supertype, as seen from `class`: the bases'
/// before the interfaces'.
pub fn find_method(classes: &HashMap<DefId, ClassInfo>, class: DefId, name: &str) -> Option<DefId> {
    ancestors(classes, class)
        .into_iter()
        .find_map(|c| own_method(classes, c, name))
}

fn own_method(classes: &HashMap<DefId, ClassInfo>, class: DefId, name: &str) -> Option<DefId> {
    let info = classes.get(&class)?;
    info.methods
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, d)| *d)
}

/// The `init` that constructing `class` runs: its own or the nearest base's.
pub fn init_of(classes: &HashMap<DefId, ClassInfo>, class: DefId) -> Option<DefId> {
    find_method(classes, class, "init")
}

/// Every member name of `class` and its supertypes, for "did you mean" suggestions.
pub fn member_names(classes: &HashMap<DefId, ClassInfo>, class: DefId) -> Vec<&str> {
    ancestors(classes, class)
        .into_iter()
        .filter_map(|c| classes.get(&c))
        .flat_map(|info| {
            info.fields
                .iter()
                .map(|(n, ..)| n.as_str())
                .chain(info.methods.iter().map(|(n, _)| n.as_str()))
        })
        .filter(|n| *n != "init")
        .collect()
}

pub fn is_abstract(program: &ward_resolve::Program, def: DefId) -> bool {
    method_decl(program, def).is_some_and(|f| matches!(f.body, FnBody::Abstract))
}

fn method_decl(program: &ward_resolve::Program, def: DefId) -> Option<&FnDecl> {
    match program.item(def) {
        Item::Fn(f) => Some(f),
        _ => None,
    }
}

fn class_kind(program: &ward_resolve::Program, def: DefId) -> Option<ClassKind> {
    match program.item(def) {
        Item::Class(c) => Some(c.kind),
        _ => None,
    }
}

/// `Interface.method` or `Class.method`.
fn qualified(program: &ward_resolve::Program, def: DefId) -> String {
    let Some(f) = method_decl(program, def) else {
        return "?".to_owned();
    };
    let owner = f.method.map_or("?", |m| {
        class_name(
            program,
            DefId {
                module: def.module,
                item: m.class,
            },
        )
    });
    format!("{owner}.{}", f.name.name)
}

impl Checker<'_> {
    pub fn check_classes(&mut self) {
        let program = self.program;
        let mut defs: Vec<DefId> = self.classes.keys().copied().collect();
        defs.sort_by_key(|d| (d.module, d.item));
        for &def in &defs {
            self.check_supers(def);
        }
        for &def in &defs {
            let Item::Class(c) = program.item(def) else {
                continue;
            };
            if let Some(g) = c.generics.first() {
                self.error(
                    def.module,
                    Diagnostic::error(
                        codes::INVALID_CLASS,
                        format!("`{}` can't be generic", c.name.name),
                        g.span,
                    )
                    .with_label("generic parameters aren't supported on classes and interfaces")
                    .with_help("use a generic record, or a class per element type"),
                );
            }
            if let (ClassKind::Interface, Some(field)) = (c.kind, c.fields.first()) {
                self.error(
                    def.module,
                    Diagnostic::error(
                        codes::INVALID_CLASS,
                        format!("interface `{}` can't have fields", c.name.name),
                        field.name.span,
                    )
                    .with_label("an interface only declares methods")
                    .with_help("declare the field in the classes that implement it"),
                );
            }
            for &m in &c.methods {
                self.check_method(
                    def,
                    DefId {
                        module: def.module,
                        item: m,
                    },
                );
            }
            if c.kind == ClassKind::Class {
                self.check_implemented(def);
            }
            if c.kind != ClassKind::Interface {
                self.check_init(def);
            }
        }
    }

    /// After `:`, a class has at most one base class, first, which is `open` or
    /// abstract; the rest are interfaces. An interface extends only interfaces. Invalid
    /// supertypes are dropped.
    fn check_supers(&mut self, def: DefId) {
        let program = self.program;
        let Item::Class(c) = program.item(def) else {
            return;
        };
        let m = def.module;
        let mut base = None;
        let mut interfaces = Vec::new();
        for (i, &t) in c.supers.iter().enumerate() {
            let span = program.module(m).ast.types[t].span;
            let problem = match self.res.module(m).types.get(t) {
                // An unknown name was reported by the resolver.
                None => continue,
                Some(&TypeRes::Def(d)) => match program.item(d) {
                    Item::Class(sc) if sc.kind == ClassKind::Interface => {
                        interfaces.push(d);
                        None
                    }
                    Item::Class(_) if c.kind == ClassKind::Interface => Some((
                        format!("interface `{}` can only extend interfaces", c.name.name),
                        "an interface can't inherit from a class".to_owned(),
                    )),
                    Item::Class(sc) if i > 0 => Some((
                        format!("`{}` must come first, before the interfaces", sc.name.name),
                        if base.is_some() || i > 0 {
                            "a class has one base class, written first: `class A: Base, Named`"
                                .to_owned()
                        } else {
                            String::new()
                        },
                    )),
                    Item::Class(sc) if !sc.is_open && sc.kind == ClassKind::Class => Some((
                        format!(
                            "class `{}` isn't `open`, so it can't be extended",
                            sc.name.name
                        ),
                        format!("declare it `open class {}`", sc.name.name),
                    )),
                    Item::Class(_) => {
                        base = Some(d);
                        None
                    }
                    _ => Some((
                        format!("`{}` can only extend a class or an interface", c.name.name),
                        "records and enums can't be extended".to_owned(),
                    )),
                },
                Some(_) => Some((
                    format!("`{}` can only extend a class or an interface", c.name.name),
                    "records and enums can't be extended".to_owned(),
                )),
            };
            if let Some((message, help)) = problem {
                self.error(
                    m,
                    Diagnostic::error(codes::INVALID_BASE_CLASS, message, span)
                        .with_label("invalid supertype")
                        .with_help(help),
                );
            }
        }
        // A cycle through this class: drop what closes it.
        let cyclic = |s: &DefId, classes: &HashMap<DefId, ClassInfo>| is_subclass(classes, *s, def);
        let mut cycle = base.is_some_and(|b| cyclic(&b, &self.classes));
        if cycle {
            base = None;
        }
        let before = interfaces.len();
        interfaces.retain(|i| !cyclic(i, &self.classes));
        cycle |= interfaces.len() != before;
        if cycle {
            self.error(
                m,
                Diagnostic::error(
                    codes::INVALID_BASE_CLASS,
                    format!("`{}` inherits from itself", c.name.name),
                    c.name.span,
                )
                .with_label("cyclic inheritance")
                .with_help("a class or interface can't be its own ancestor"),
            );
        }
        if let Some(info) = self.classes.get_mut(&def) {
            info.base = base;
            info.interfaces = interfaces;
        }
    }

    fn check_method(&mut self, class: DefId, def: DefId) {
        let program = self.program;
        let Some(f) = method_decl(program, def) else {
            return;
        };
        let Some(info) = f.method else {
            return;
        };
        let m = def.module;
        let kind = class_kind(program, class).unwrap_or(ClassKind::Class);
        let misuse = if info.is_init && kind == ClassKind::Interface {
            Some((
                "an interface has no `init`",
                "objects are created from classes",
            ))
        } else if !info.is_init && f.name.name == "init" {
            Some((
                "a method can't be called `init`",
                "declare the constructor without `fn`: `init(...) { ... }`",
            ))
        } else if info.is_abstract && kind == ClassKind::Class {
            Some((
                "only an abstract class can have abstract methods",
                "declare the class `abstract class`, or give the method a body",
            ))
        } else if info.is_abstract && f.is_ai {
            Some((
                "an abstract method can't be an `ai fn`",
                "declare it `abstract fn`; an implementation may be an `ai fn`",
            ))
        } else {
            None
        };
        if let Some((message, help)) = misuse {
            self.error(
                m,
                Diagnostic::error(codes::INVALID_CLASS, message, f.name.span)
                    .with_label("not allowed here")
                    .with_help(help),
            );
            return;
        }
        if info.is_init {
            if info.is_open || info.is_override {
                self.error(
                    m,
                    Diagnostic::error(
                        codes::INVALID_OVERRIDE,
                        "`init` can't be `open` or `override`",
                        f.name.span,
                    )
                    .with_label("constructors aren't inherited like methods")
                    .with_help("remove the modifier; call the base's with `super.init(...)`"),
                );
            }
            return;
        }
        let supers: Vec<DefId> = ancestors(&self.classes, class)
            .into_iter()
            .skip(1)
            .collect();
        if let Some((owner, _, _)) = supers
            .iter()
            .find_map(|&s| find_field(&self.classes, s, &f.name.name))
        {
            let owner = class_name(program, owner);
            self.error(
                m,
                Diagnostic::error(
                    codes::INVALID_OVERRIDE,
                    format!("`{}` is already a field of `{owner}`", f.name.name),
                    f.name.span,
                )
                .with_label("a method can't replace a field"),
            );
            return;
        }
        // Every supertype's method of this name is overridden, e.g. a base class's and
        // an interface's.
        let overridden: Vec<DefId> = supers
            .iter()
            .filter_map(|&s| own_method(&self.classes, s, &f.name.name))
            .collect();
        if overridden.is_empty() {
            if info.is_override {
                self.error(
                    m,
                    Diagnostic::error(
                        codes::INVALID_OVERRIDE,
                        format!("`{}` overrides nothing", f.name.name),
                        f.name.span,
                    )
                    .with_label("no supertype has a method with this name")
                    .with_help("remove `override`"),
                );
            }
            return;
        }
        for &base_def in &overridden {
            let Some(bf) = method_decl(program, base_def) else {
                continue;
            };
            let shown = qualified(program, base_def);
            let base_class = shown.split('.').next().unwrap_or("?").to_owned();
            let overridable = bf
                .method
                .is_some_and(|i| i.is_open || i.is_override || i.is_abstract);
            let problem = if !info.is_override {
                Some((
                    format!("`{}` overrides `{shown}` without saying so", f.name.name),
                    format!("mark it `override fn {}`", f.name.name),
                ))
            } else if !overridable {
                Some((
                    format!("`{shown}` isn't `open`, so it can't be overridden"),
                    format!("declare it `open fn {}` in `{base_class}`", f.name.name),
                ))
            } else if !self.same_signature(def, base_def) || f.is_pub != bf.is_pub {
                Some((
                    format!(
                        "`{}` must have the same signature as `{shown}`",
                        f.name.name
                    ),
                    "an override takes the same parameters, returns and throws the same \
                     types, and has the same visibility (an interface's methods are `pub`)"
                        .to_owned(),
                ))
            } else {
                None
            };
            if let Some((message, help)) = problem {
                let mut d = Diagnostic::error(codes::INVALID_OVERRIDE, message, f.name.span)
                    .with_label("invalid override")
                    .with_help(help);
                if base_def.module == m {
                    d = d.with_secondary(bf.name.span, "the overridden method");
                }
                self.error(m, d);
                return;
            }
        }
        // A call to any method this one overrides may run it.
        for b in overridden {
            let list = self.overrides.entry(b).or_default();
            if !list.contains(&def) {
                list.push(def);
            }
        }
    }

    /// A class that can be created implements every abstract method it inherits.
    fn check_implemented(&mut self, class: DefId) {
        let program = self.program;
        let Item::Class(c) = program.item(class) else {
            return;
        };
        let mut missing: Vec<DefId> = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for s in ancestors(&self.classes, class).into_iter().skip(1) {
            let Some(info) = self.classes.get(&s) else {
                continue;
            };
            for (name, d) in &info.methods {
                if !is_abstract(program, *d) || seen.contains(&name.as_str()) {
                    continue;
                }
                seen.push(name);
                let implemented = ancestors(&self.classes, class)
                    .into_iter()
                    .filter(|a| class_kind(program, *a) != Some(ClassKind::Interface))
                    .filter_map(|a| own_method(&self.classes, a, name))
                    .any(|m| !is_abstract(program, m));
                if !implemented {
                    missing.push(*d);
                }
            }
        }
        if missing.is_empty() {
            return;
        }
        let names: Vec<String> = missing
            .iter()
            .map(|&d| format!("`{}`", qualified(program, d)))
            .collect();
        let first = method_decl(program, missing[0]).map_or("run", |f| f.name.name.as_str());
        self.error(
            class.module,
            Diagnostic::error(
                codes::MISSING_IMPLEMENTATION,
                format!(
                    "class `{}` doesn't implement {}",
                    c.name.name,
                    names.join(", ")
                ),
                c.name.span,
            )
            .with_label("missing implementations")
            .with_help(format!(
                "add `pub override fn {first}(...)`, or declare the class `abstract class`"
            )),
        );
    }

    fn same_signature(&self, a: DefId, b: DefId) -> bool {
        let (Some(x), Some(y)) = (self.fns.get(&a), self.fns.get(&b)) else {
            return false;
        };
        let rest =
            |s: &crate::FnSig| -> Vec<Ty> { s.params.iter().skip(1).map(Ty::unrefined).collect() };
        rest(x) == rest(y)
            && x.ret.unrefined() == y.ret.unrefined()
            && x.throws.as_ref().map(Ty::unrefined) == y.throws.as_ref().map(Ty::unrefined)
    }

    /// Every own field is set by `init`, which starts with `super.init(...)` when a base
    /// has an `init`.
    fn check_init(&mut self, class: DefId) {
        let program = self.program;
        let Item::Class(c) = program.item(class) else {
            return;
        };
        let m = class.module;
        let Some(info) = self.classes.get(&class).cloned() else {
            return;
        };
        let own_init = info
            .methods
            .iter()
            .find(|(n, d)| {
                n == "init"
                    && method_decl(program, *d).is_some_and(|f| f.method.is_some_and(|i| i.is_init))
            })
            .map(|(_, d)| *d);
        let base_init = info.base.and_then(|b| init_of(&self.classes, b));
        let Some(init) = own_init else {
            if let Some(first) = c.fields.first() {
                self.error(
                    m,
                    Diagnostic::error(
                        codes::UNINITIALIZED_FIELD,
                        format!("class `{}` has fields but no `init`", c.name.name),
                        c.name.span,
                    )
                    .with_label("add an `init` that sets its fields")
                    .with_secondary(first.name.span, "this field is never set")
                    .with_help(format!(
                        "e.g. `init({name}: ...) {{ self.{name} = {name} }}`",
                        name = first.name.name
                    )),
                );
            }
            return;
        };
        let Some(f) = method_decl(program, init) else {
            return;
        };
        let FnBody::Block(body) = &f.body else {
            return;
        };
        let ast = &program.module(m).ast;
        let mres = self.res.module(m);
        let self_local = mres
            .params
            .get(&init.item)
            .and_then(|ps| ps.first())
            .copied();
        let mut set: Vec<&str> = Vec::new();
        for &s in &body.stmts {
            let StmtKind::Assign { target, .. } = &ast.stmts[s].kind else {
                continue;
            };
            if let ExprKind::Field { base, name } = &ast.exprs[*target].kind {
                let on_self = matches!(
                    (mres.values.get(*base), self_local),
                    (Some(ValueRes::Local(l)), Some(s)) if *l == s
                );
                if on_self {
                    set.push(&name.name);
                }
            }
        }
        for field in &c.fields {
            if !set.contains(&field.name.name.as_str()) {
                self.error(
                    m,
                    Diagnostic::error(
                        codes::UNINITIALIZED_FIELD,
                        format!("`init` doesn't set the field `{}`", field.name.name),
                        f.name.span,
                    )
                    .with_label(format!("`self.{}` is never assigned here", field.name.name))
                    .with_secondary(field.name.span, "declared here")
                    .with_help(format!(
                        "assign it in `init`'s body, outside any `if` or loop: `self.{} = ...`",
                        field.name.name
                    )),
                );
            }
        }
        if base_init.is_some() && super_init_call(ast, mres, body).is_none() {
            self.error(
                m,
                Diagnostic::error(
                    codes::INVALID_SUPER,
                    format!(
                        "`init` of `{}` must start by calling `super.init(...)`",
                        c.name.name
                    ),
                    f.name.span,
                )
                .with_label("the base class's fields aren't set up")
                .with_help("make `super.init(...)` the first statement"),
            );
        }
    }
}

/// `super.init(...)` as the first statement of `body`: its call expression.
pub(crate) fn super_init_call(
    ast: &ward_syntax::ast::Module,
    mres: &ward_resolve::ModuleRes,
    body: &ward_syntax::ast::Block,
) -> Option<ward_syntax::ast::ExprId> {
    let first = match body.stmts.first() {
        Some(&s) => match &ast.stmts[s].kind {
            StmtKind::Expr { expr, .. } => *expr,
            _ => return None,
        },
        None => body.tail?,
    };
    let mut call = first;
    if let ExprKind::Propagate(inner) = &ast.exprs[call].kind {
        call = *inner;
    }
    let ExprKind::Call { callee, .. } = &ast.exprs[call].kind else {
        return None;
    };
    let ExprKind::Field { base, name } = &ast.exprs[*callee].kind else {
        return None;
    };
    (name.name == "init" && matches!(mres.values.get(*base), Some(ValueRes::Super(_))))
        .then_some(call)
}

pub fn class_name(program: &ward_resolve::Program, def: DefId) -> &str {
    match program.item(def) {
        Item::Class(c) => &c.name.name,
        _ => "?",
    }
}
