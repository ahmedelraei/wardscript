//! Match exhaustiveness and reachability, using the usefulness algorithm from
//! Maranget, "Warnings for pattern matching" (JFP 2007).

use crate::ty::Ty;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ctor {
    True,
    False,
    Some,
    None,
    Ok,
    Err,
    Variant(usize),
}

/// A pattern reduced to what matters for coverage.
#[derive(Clone, Debug)]
pub enum DPat {
    Wild,
    Ctor(Ctor, Vec<DPat>),
    /// A literal of a type with infinitely many values; only `_` covers them all.
    Lit(String),
}

/// A value shape that no row matches, used to explain what's missing.
#[derive(Clone, Debug)]
pub enum Witness {
    Wild,
    Ctor(Ctor, Vec<Witness>),
}

pub trait Ctors {
    /// All constructors of `ty` with their payload types, or `None` if its values
    /// can't be enumerated (numbers, strings, unknown types).
    fn ctors(&self, ty: &Ty) -> Option<Vec<(Ctor, Vec<Ty>)>>;
}

/// A witness for the first column that `rows` don't cover, if any.
pub fn missing(rows: &[DPat], ty: &Ty, cx: &dyn Ctors) -> Option<Witness> {
    let rows: Vec<Vec<DPat>> = rows.iter().map(|p| vec![p.clone()]).collect();
    useful(&rows, &[DPat::Wild], std::slice::from_ref(ty), cx).and_then(|mut w| w.pop())
}

/// Whether `pat` matches some value that none of `rows` match.
pub fn is_reachable(rows: &[DPat], pat: &DPat, ty: &Ty, cx: &dyn Ctors) -> bool {
    let rows: Vec<Vec<DPat>> = rows.iter().map(|p| vec![p.clone()]).collect();
    useful(
        &rows,
        std::slice::from_ref(pat),
        std::slice::from_ref(ty),
        cx,
    )
    .is_some()
}

fn useful(rows: &[Vec<DPat>], q: &[DPat], tys: &[Ty], cx: &dyn Ctors) -> Option<Vec<Witness>> {
    let Some((head, rest)) = q.split_first() else {
        return rows.is_empty().then(Vec::new);
    };
    let ty = tys.first().cloned().unwrap_or(Ty::Error);
    let rest_tys = tys.get(1..).unwrap_or(&[]);
    match head {
        DPat::Ctor(c, args) => {
            let sub = payload(cx, &ty, *c).unwrap_or_else(|| vec![Ty::Error; args.len()]);
            let q2 = [pad(args, sub.len()), rest.to_vec()].concat();
            let tys2 = [sub.clone(), rest_tys.to_vec()].concat();
            useful(&specialize(rows, *c, sub.len()), &q2, &tys2, cx)
                .map(|w| rebuild(*c, sub.len(), w))
        }
        DPat::Lit(l) => {
            let spec: Vec<Vec<DPat>> = rows
                .iter()
                .filter(|r| match r.first() {
                    Some(DPat::Wild) => true,
                    Some(DPat::Lit(x)) => x == l,
                    _ => false,
                })
                .map(|r| r[1..].to_vec())
                .collect();
            useful(&spec, rest, rest_tys, cx).map(|w| [vec![Witness::Wild], w].concat())
        }
        DPat::Wild => {
            let used: Vec<Ctor> = rows
                .iter()
                .filter_map(|r| match r.first() {
                    Some(DPat::Ctor(c, _)) => Some(*c),
                    _ => None,
                })
                .collect();
            let all = cx.ctors(&ty);
            if let Some(all) = all.as_ref().filter(|a| !a.is_empty()) {
                if all.iter().all(|(c, _)| used.contains(c)) {
                    for (c, sub) in all {
                        let q2 = [vec![DPat::Wild; sub.len()], rest.to_vec()].concat();
                        let tys2 = [sub.clone(), rest_tys.to_vec()].concat();
                        if let Some(w) = useful(&specialize(rows, *c, sub.len()), &q2, &tys2, cx) {
                            return Some(rebuild(*c, sub.len(), w));
                        }
                    }
                    return None;
                }
            }
            let default: Vec<Vec<DPat>> = rows
                .iter()
                .filter(|r| matches!(r.first(), Some(DPat::Wild)))
                .map(|r| r[1..].to_vec())
                .collect();
            let w = useful(&default, rest, rest_tys, cx)?;
            let head = all
                .and_then(|all| all.into_iter().find(|(c, _)| !used.contains(c)))
                .map_or(Witness::Wild, |(c, sub)| {
                    Witness::Ctor(c, vec![Witness::Wild; sub.len()])
                });
            Some([vec![head], w].concat())
        }
    }
}

fn payload(cx: &dyn Ctors, ty: &Ty, c: Ctor) -> Option<Vec<Ty>> {
    cx.ctors(ty)?
        .into_iter()
        .find(|(x, _)| *x == c)
        .map(|(_, sub)| sub)
}

fn pad(args: &[DPat], arity: usize) -> Vec<DPat> {
    let mut v: Vec<DPat> = args.iter().take(arity).cloned().collect();
    v.resize(arity, DPat::Wild);
    v
}

fn specialize(rows: &[Vec<DPat>], c: Ctor, arity: usize) -> Vec<Vec<DPat>> {
    rows.iter()
        .filter_map(|r| {
            let (head, tail) = r.split_first()?;
            match head {
                DPat::Ctor(c2, args) if *c2 == c => {
                    Some([pad(args, arity), tail.to_vec()].concat())
                }
                DPat::Wild => Some([vec![DPat::Wild; arity], tail.to_vec()].concat()),
                _ => None,
            }
        })
        .collect()
}

fn rebuild(c: Ctor, arity: usize, mut w: Vec<Witness>) -> Vec<Witness> {
    let rest = w.split_off(arity.min(w.len()));
    [vec![Witness::Ctor(c, w)], rest].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Cx;

    impl Ctors for Cx {
        fn ctors(&self, ty: &Ty) -> Option<Vec<(Ctor, Vec<Ty>)>> {
            match ty {
                Ty::Bool => Some(vec![(Ctor::True, vec![]), (Ctor::False, vec![])]),
                Ty::Option(t) => Some(vec![
                    (Ctor::None, vec![]),
                    (Ctor::Some, vec![(**t).clone()]),
                ]),
                _ => None,
            }
        }
    }

    fn some(p: DPat) -> DPat {
        DPat::Ctor(Ctor::Some, vec![p])
    }

    #[test]
    fn nested_option_of_bool() {
        let ty = Ty::option(Ty::Bool);
        let rows = [
            DPat::Ctor(Ctor::None, vec![]),
            some(DPat::Ctor(Ctor::True, vec![])),
        ];
        let w = missing(&rows, &ty, &Cx);
        assert!(
            matches!(w, Some(Witness::Ctor(Ctor::Some, ref a)) if matches!(a[..], [Witness::Ctor(Ctor::False, _)]))
        );
        let rows = [rows[0].clone(), rows[1].clone(), some(DPat::Wild)];
        assert!(missing(&rows, &ty, &Cx).is_none());
    }

    #[test]
    fn wildcard_makes_later_arms_unreachable() {
        let rows = [DPat::Wild];
        assert!(!is_reachable(
            &rows,
            &DPat::Ctor(Ctor::True, vec![]),
            &Ty::Bool,
            &Cx
        ));
        assert!(!is_reachable(&rows, &DPat::Lit("1".into()), &Ty::Int, &Cx));
    }

    #[test]
    fn literals_need_a_wildcard() {
        let rows = [DPat::Lit("1".into()), DPat::Lit("2".into())];
        assert!(matches!(missing(&rows, &Ty::Int, &Cx), Some(Witness::Wild)));
    }
}
