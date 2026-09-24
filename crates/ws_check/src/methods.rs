//! Built-in methods on primitive types (the prelude). Lists and maps are immutable:
//! `push` and `insert` return a new collection.

use crate::ty::Ty;

pub struct MethodSig {
    pub params: Vec<Ty>,
    pub ret: Ty,
}

pub fn method(recv: &Ty, name: &str) -> Option<MethodSig> {
    let sig = |params: Vec<Ty>, ret: Ty| Some(MethodSig { params, ret });
    match (recv, name) {
        (Ty::String, "len") => sig(vec![], Ty::Int),
        (Ty::String, "is_empty") => sig(vec![], Ty::Bool),
        (Ty::String, "contains" | "starts_with" | "ends_with") => sig(vec![Ty::String], Ty::Bool),
        (Ty::String, "trim" | "lower" | "upper") => sig(vec![], Ty::String),
        (Ty::String, "split") => sig(vec![Ty::String], Ty::list(Ty::String)),
        (Ty::String, "lines") => sig(vec![], Ty::list(Ty::String)),
        (Ty::String, "replace") => sig(vec![Ty::String, Ty::String], Ty::String),

        (Ty::List(_), "len") => sig(vec![], Ty::Int),
        (Ty::List(_), "is_empty") => sig(vec![], Ty::Bool),
        (Ty::List(t), "contains") => sig(vec![(**t).clone()], Ty::Bool),
        (Ty::List(t), "get") => sig(vec![Ty::Int], Ty::option((**t).clone())),
        (Ty::List(t), "first" | "last") => sig(vec![], Ty::option((**t).clone())),
        (Ty::List(t), "push") => sig(vec![(**t).clone()], recv.clone()),

        (Ty::Map(..), "len") => sig(vec![], Ty::Int),
        (Ty::Map(..), "is_empty") => sig(vec![], Ty::Bool),
        (Ty::Map(k, v), "get") => sig(vec![(**k).clone()], Ty::option((**v).clone())),
        (Ty::Map(k, _), "contains_key") => sig(vec![(**k).clone()], Ty::Bool),
        (Ty::Map(k, _), "keys") => sig(vec![], Ty::list((**k).clone())),
        (Ty::Map(_, v), "values") => sig(vec![], Ty::list((**v).clone())),
        (Ty::Map(k, v), "insert") => sig(vec![(**k).clone(), (**v).clone()], recv.clone()),

        (Ty::Option(_), "is_some" | "is_none") => sig(vec![], Ty::Bool),
        (Ty::Option(t), "unwrap_or") => sig(vec![(**t).clone()], (**t).clone()),
        (Ty::Result(..), "is_ok" | "is_err") => sig(vec![], Ty::Bool),
        (Ty::Result(t, _), "unwrap_or") => sig(vec![(**t).clone()], (**t).clone()),

        (Ty::Int | Ty::Float | Ty::Bool, "to_string") => sig(vec![], Ty::String),
        (Ty::Int, "to_float") => sig(vec![], Ty::Float),
        (Ty::Float, "round") => sig(vec![], Ty::Int),
        _ => None,
    }
}

/// Method names available on `recv`, for "did you mean" suggestions.
pub fn names(recv: &Ty) -> Vec<&'static str> {
    const ALL: [&str; 27] = [
        "len",
        "is_empty",
        "contains",
        "starts_with",
        "ends_with",
        "trim",
        "lower",
        "upper",
        "split",
        "lines",
        "replace",
        "get",
        "first",
        "last",
        "push",
        "contains_key",
        "keys",
        "values",
        "insert",
        "is_some",
        "is_none",
        "unwrap_or",
        "is_ok",
        "is_err",
        "to_string",
        "to_float",
        "round",
    ];
    ALL.into_iter()
        .filter(|n| method(recv, n).is_some())
        .collect()
}
