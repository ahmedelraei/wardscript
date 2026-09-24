use ward_resolve::DefId;

#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    Int,
    Float,
    String,
    Bool,
    Unit,
    /// The type of an expression that never produces a value (`return`); fits anywhere.
    Never,
    List(Box<Ty>),
    Map(Box<Ty>, Box<Ty>),
    Option(Box<Ty>),
    /// A record or enum with its type arguments.
    Adt(DefId, Vec<Ty>),
    /// The `n`th generic parameter of the item being checked. Only equal to itself.
    Param(usize),
    /// An inference variable.
    Var(u32),
    /// Tool results: accepted anywhere until MCP imports give them real types (M7).
    Dynamic,
    /// Already reported; accepted anywhere so one mistake doesn't cascade.
    Error,
}

impl Ty {
    /// The type of a tool parameter or result, from its schema.
    pub fn from_tool(t: &ward_resolve::tools::ToolTy) -> Ty {
        use ward_resolve::tools::ToolTy;
        match t {
            ToolTy::String => Ty::String,
            ToolTy::Int => Ty::Int,
            ToolTy::Float => Ty::Float,
            ToolTy::Bool => Ty::Bool,
            ToolTy::List(t) => Ty::list(Ty::from_tool(t)),
            ToolTy::Option(t) => Ty::option(Ty::from_tool(t)),
            ToolTy::Any => Ty::Dynamic,
        }
    }

    /// A tool parameter's type: an optional one is an `Option`, which may be left out.
    pub fn tool_param(p: &ward_resolve::tools::ToolParam) -> Ty {
        let t = Ty::from_tool(&p.ty);
        if p.required || matches!(t, Ty::Option(_) | Ty::Dynamic) {
            t
        } else {
            Ty::option(t)
        }
    }

    pub fn list(t: Ty) -> Ty {
        Ty::List(Box::new(t))
    }

    pub fn option(t: Ty) -> Ty {
        Ty::Option(Box::new(t))
    }

    pub fn map(k: Ty, v: Ty) -> Ty {
        Ty::Map(Box::new(k), Box::new(v))
    }

    /// Replaces generic parameters with `args` (missing ones become `Error`).
    pub fn subst(&self, args: &[Ty]) -> Ty {
        self.map_children(&|t| match t {
            Ty::Param(i) => Some(args.get(*i).cloned().unwrap_or(Ty::Error)),
            _ => None,
        })
    }

    /// Replaces unsolved inference variables with `Error`.
    pub fn without_vars(&self) -> Ty {
        self.map_children(&|t| matches!(t, Ty::Var(_)).then_some(Ty::Error))
    }

    /// Rebuilds the type bottom-up; `f` may replace any node.
    fn map_children(&self, f: &dyn Fn(&Ty) -> Option<Ty>) -> Ty {
        if let Some(t) = f(self) {
            return t;
        }
        let g = |t: &Ty| Box::new(t.map_children(f));
        match self {
            Ty::List(t) => Ty::List(g(t)),
            Ty::Option(t) => Ty::Option(g(t)),
            Ty::Map(k, v) => Ty::Map(g(k), g(v)),
            Ty::Adt(d, args) => Ty::Adt(*d, args.iter().map(|a| a.map_children(f)).collect()),
            t => t.clone(),
        }
    }

    pub fn any(&self, pred: &dyn Fn(&Ty) -> bool) -> bool {
        pred(self)
            || match self {
                Ty::List(t) | Ty::Option(t) => t.any(pred),
                Ty::Map(a, b) => a.any(pred) || b.any(pred),
                Ty::Adt(_, args) => args.iter().any(|a| a.any(pred)),
                _ => false,
            }
    }

    /// Types that unify with anything.
    pub fn is_lenient(&self) -> bool {
        matches!(self, Ty::Never | Ty::Dynamic | Ty::Error)
    }
}

#[derive(Default)]
pub struct Unifier {
    vars: Vec<Option<Ty>>,
}

impl Unifier {
    pub fn fresh(&mut self) -> Ty {
        self.vars.push(None);
        Ty::Var(self.vars.len() as u32 - 1)
    }

    /// Follows bound variables at the top level only.
    pub fn shallow(&self, t: &Ty) -> Ty {
        let mut t = t.clone();
        while let Ty::Var(v) = t {
            match self.vars.get(v as usize) {
                Some(Some(bound)) => t = bound.clone(),
                _ => break,
            }
        }
        t
    }

    pub fn resolve(&self, t: &Ty) -> Ty {
        t.map_children(&|t| match t {
            Ty::Var(_) => {
                let s = self.shallow(t);
                match s {
                    Ty::Var(_) => Some(s),
                    other => Some(self.resolve(&other)),
                }
            }
            _ => None,
        })
    }

    pub fn unify(&mut self, a: &Ty, b: &Ty) -> bool {
        let (a, b) = (self.shallow(a), self.shallow(b));
        match (&a, &b) {
            (Ty::Var(x), Ty::Var(y)) if x == y => true,
            // `Never` fits anywhere but tells us nothing about the variable.
            (Ty::Var(_), Ty::Never) | (Ty::Never, Ty::Var(_)) => true,
            (Ty::Var(v), t) | (t, Ty::Var(v)) => {
                let occurs = self.resolve(t).any(&|x| *x == Ty::Var(*v));
                if !occurs {
                    self.vars[*v as usize] = Some(t.clone());
                }
                !occurs
            }
            (x, y) if x.is_lenient() || y.is_lenient() => true,
            (Ty::List(x), Ty::List(y)) | (Ty::Option(x), Ty::Option(y)) => self.unify(x, y),
            (Ty::Map(k1, v1), Ty::Map(k2, v2)) => {
                let first = self.unify(k1, k2);
                self.unify(v1, v2) && first
            }
            (Ty::Adt(d1, a1), Ty::Adt(d2, a2)) => {
                d1 == d2
                    && a1.len() == a2.len()
                    && a1
                        .iter()
                        .zip(a2)
                        .fold(true, |ok, (x, y)| self.unify(x, y) && ok)
            }
            (x, y) => x == y,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unifies_through_variables() {
        let mut u = Unifier::default();
        let a = u.fresh();
        assert!(u.unify(&Ty::list(a.clone()), &Ty::list(Ty::Int)));
        assert_eq!(u.resolve(&a), Ty::Int);
        assert!(!u.unify(&a, &Ty::String));
    }

    #[test]
    fn occurs_check_rejects_infinite_types() {
        let mut u = Unifier::default();
        let a = u.fresh();
        assert!(!u.unify(&a, &Ty::list(a.clone())));
    }

    #[test]
    fn lenient_types_fit_anywhere() {
        let mut u = Unifier::default();
        assert!(u.unify(&Ty::Never, &Ty::Int));
        assert!(u.unify(&Ty::list(Ty::Dynamic), &Ty::list(Ty::String)));
        assert!(!u.unify(&Ty::Int, &Ty::Float));
    }
}
