//! Refinements (`String where it.len() <= 80`) and `check {...}` clauses. A refinement
//! may only use `it`, literals, operators, fields, methods and enum variants, so it can
//! be checked anywhere, turned into JSON Schema where possible, and never has effects.
//! It goes where decoded data is described: record fields, variant payloads, type
//! aliases and `ai fn` return types.

use std::collections::HashSet;

use ward_resolve::{Builtin, DefId, ModuleId, ValueRes};
use ward_syntax::Diagnostic;
use ward_syntax::ast::{ExprId, ExprKind, Item, TemplatePart, TypeId};
use ward_syntax::diag::codes;

use crate::{Checker, ModuleTypes};

/// Where refinements are allowed in module `m`.
fn allowed_sites(c: &Checker, m: ModuleId) -> HashSet<TypeId> {
    let mut out = HashSet::new();
    for item in &c.program.module(m).ast.items {
        match item {
            Item::Record(r) => out.extend(r.fields.iter().map(|f| f.ty)),
            Item::Enum(e) => out.extend(e.variants.iter().flat_map(|v| v.fields.iter().copied())),
            Item::Alias(a) => {
                out.insert(a.ty);
            }
            Item::Fn(f) if f.is_ai => out.extend(f.ret),
            _ => {}
        }
    }
    out
}

pub(crate) fn check(c: &mut Checker, types: &mut [ModuleTypes]) {
    let program = c.program;
    for m in program.module_ids() {
        let allowed = allowed_sites(c, m);
        let ast = &program.module(m).ast;
        for (id, t) in ast.types.iter() {
            let Some(cond) = t.refinement else { continue };
            if !allowed.contains(&id) {
                c.error(
                    m,
                    Diagnostic::error(
                        codes::MISPLACED_REFINEMENT,
                        "a refinement can't go here",
                        t.span,
                    )
                    .with_label("refined type")
                    .with_help(
                        "refinements describe decoded data: put them on record fields, type \
                         aliases and `ai fn` return types, and check other values with `validate`",
                    ),
                );
                continue;
            }
            if let Some(d) = impure(c, m, cond) {
                c.error(m, d);
                continue;
            }
            let base = c.quietly(|c| c.lower_unrefined(m, id));
            if let Some(out) = types.get_mut(m.0 as usize) {
                crate::infer::check_refinement(c, m, id, cond, base, out);
            }
        }
    }
}

/// The first part of a refinement that isn't allowed in one.
fn impure(c: &Checker, m: ModuleId, e: ExprId) -> Option<Diagnostic> {
    let ast = &c.program.module(m).ast;
    let res = c.res.module(m);
    let not_here = |span, what: &str| {
        Some(
            Diagnostic::error(
                codes::INVALID_REFINEMENT,
                format!("{what} can't be used in a refinement"),
                span,
            )
            .with_label("not allowed here")
            .with_help(
                "a refinement uses `it`, literals, operators, fields, methods and enum \
                 variants; check anything else in the `ai fn`'s `check {...}` clause",
            ),
        )
    };
    let expr = &ast.exprs[e];
    let sub = |es: Vec<ExprId>| es.into_iter().find_map(|x| impure(c, m, x));
    match &expr.kind {
        ExprKind::Lit(_) | ExprKind::Error | ExprKind::Name(_) => None,
        ExprKind::Template(parts) => sub(parts
            .iter()
            .filter_map(|p| match p {
                TemplatePart::Expr(x) => Some(*x),
                TemplatePart::Lit(_) => None,
            })
            .collect()),
        ExprKind::Field { base, .. } => impure(c, m, *base),
        ExprKind::Call { callee, args } => match res.values.get(*callee) {
            // A method on a value.
            None => match &ast.exprs[*callee].kind {
                ExprKind::Field { base, .. } => impure(c, m, *base).or_else(|| sub(args.clone())),
                _ => not_here(expr.span, "this call"),
            },
            Some(ValueRes::Builtin(Builtin::Some) | ValueRes::Variant(..)) => sub(args.clone()),
            Some(ValueRes::Fn(d)) => not_here(expr.span, &fn_what(c, *d)),
            Some(_) => not_here(expr.span, "this call"),
        },
        ExprKind::Index { .. } | ExprKind::Unary { .. } | ExprKind::Binary { .. } => {
            let mut children = Vec::new();
            match &expr.kind {
                ExprKind::Index { base, index } => children.extend([*base, *index]),
                ExprKind::Unary { operand, .. } => children.push(*operand),
                ExprKind::Binary { lhs, rhs, .. } => children.extend([*lhs, *rhs]),
                _ => {}
            }
            sub(children)
        }
        ExprKind::List(items) => sub(items.clone()),
        ExprKind::Propagate(_) => not_here(expr.span, "`?`"),
        ExprKind::Record { .. } => not_here(expr.span, "a record literal"),
        ExprKind::If { .. } | ExprKind::Match { .. } => not_here(expr.span, "`if` or `match`"),
        ExprKind::TryCatch { .. } | ExprKind::Block(_) => not_here(expr.span, "a block"),
    }
}

fn fn_what(c: &Checker, d: DefId) -> String {
    match c.program.item(d) {
        Item::Fn(f) if f.is_ai => format!("a call of `ai fn {}`", f.name.name),
        Item::Fn(f) => format!("a call of `{}`", f.name.name),
        _ => "this call".to_owned(),
    }
}
