//! Refinements as JSON Schema, where their shape allows: `it.len() <= 80` is
//! `maxLength: 80`. The rest is only checked at run time.

use ward_resolve::{ModuleRes, ValueRes};
use ward_syntax::ast::{self, BinOp, ExprId, ExprKind, Lit, UnOp};

use crate::Ty;

/// Schema keywords (name, JSON number) implied by `cond`, for a value of type `base`.
pub fn schema(
    ast: &ast::Module,
    res: &ModuleRes,
    cond: ExprId,
    base: &Ty,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    collect(ast, res, cond, base, &mut out);
    out
}

enum Side {
    /// `it.len()`
    Len,
    /// `it`
    It,
    Number(f64, String),
}

fn side(ast: &ast::Module, res: &ModuleRes, e: ExprId) -> Option<Side> {
    match &ast.exprs[e].kind {
        ExprKind::Name(_) => {
            matches!(res.values.get(e), Some(ValueRes::Local(_))).then_some(Side::It)
        }
        ExprKind::Call { callee, args } if args.is_empty() => match &ast.exprs[*callee].kind {
            ExprKind::Field { base, name } if name.name == "len" => {
                matches!(side(ast, res, *base), Some(Side::It)).then_some(Side::Len)
            }
            _ => None,
        },
        ExprKind::Lit(Lit::Int(n)) => Some(Side::Number(*n as f64, n.to_string())),
        ExprKind::Lit(Lit::Float(s)) => s.parse().ok().map(|v| Side::Number(v, s.clone())),
        ExprKind::Unary {
            op: UnOp::Neg,
            operand,
        } => match side(ast, res, *operand)? {
            Side::Number(v, s) => Some(Side::Number(-v, format!("-{s}"))),
            _ => None,
        },
        _ => None,
    }
}

fn flip(op: BinOp) -> BinOp {
    match op {
        BinOp::Lt => BinOp::Gt,
        BinOp::Le => BinOp::Ge,
        BinOp::Gt => BinOp::Lt,
        BinOp::Ge => BinOp::Le,
        other => other,
    }
}

fn collect(
    ast: &ast::Module,
    res: &ModuleRes,
    e: ExprId,
    base: &Ty,
    out: &mut Vec<(String, String)>,
) {
    let ExprKind::Binary { op, lhs, rhs } = &ast.exprs[e].kind else {
        return;
    };
    if *op == BinOp::And {
        collect(ast, res, *lhs, base, out);
        collect(ast, res, *rhs, base, out);
        return;
    }
    let (subject, op, n, text) = match (side(ast, res, *lhs), side(ast, res, *rhs)) {
        (Some(s @ (Side::Len | Side::It)), Some(Side::Number(n, t))) => (s, *op, n, t),
        (Some(Side::Number(n, t)), Some(s @ (Side::Len | Side::It))) => (s, flip(*op), n, t),
        _ => return,
    };
    let whole = n.fract() == 0.0;
    let int = |v: f64| format!("{}", v as i64);
    let mut push = |k: &str, v: String| out.push((k.to_owned(), v));
    match subject {
        Side::Len => {
            let (min, max) = match base {
                Ty::String => ("minLength", "maxLength"),
                Ty::List(_) => ("minItems", "maxItems"),
                _ => return,
            };
            if !whole || n < 0.0 {
                return;
            }
            match op {
                BinOp::Lt if n >= 1.0 => push(max, int(n - 1.0)),
                BinOp::Le => push(max, int(n)),
                BinOp::Gt => push(min, int(n + 1.0)),
                BinOp::Ge => push(min, int(n)),
                BinOp::Eq => {
                    push(min, int(n));
                    push(max, int(n));
                }
                _ => {}
            }
        }
        Side::It if matches!(base, Ty::Int | Ty::Float) => match op {
            BinOp::Lt => push("exclusiveMaximum", text),
            BinOp::Le => push("maximum", text),
            BinOp::Gt => push("exclusiveMinimum", text),
            BinOp::Ge => push("minimum", text),
            BinOp::Eq => push("const", text),
            _ => {}
        },
        _ => {}
    }
}
