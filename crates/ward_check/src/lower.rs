//! Item signatures and the translation of written types (`List<Ticket>`) into `Ty`.

use ward_resolve::{DefId, ModuleId, Prim, TypeRes};
use ward_syntax::Diagnostic;
use ward_syntax::ast::{FnBody, Item, TypeId, TypeKind};
use ward_syntax::diag::codes;

use crate::ty::Ty;
use crate::{Checker, FnSig};

impl Checker<'_> {
    pub fn collect_signatures(&mut self) {
        let program = self.program;
        let mut llm_returns = Vec::new();
        for m in program.module_ids() {
            for (item, it) in program.module(m).ast.items.iter().enumerate() {
                let def = DefId { module: m, item };
                match it {
                    Item::Record(r) => {
                        let fields = r
                            .fields
                            .iter()
                            .map(|f| (f.name.name.clone(), self.lower(m, f.ty)))
                            .collect();
                        self.records.insert(def, fields);
                    }
                    Item::Enum(e) => {
                        let variants = e
                            .variants
                            .iter()
                            .map(|v| {
                                let fields = v.fields.iter().map(|&t| self.lower(m, t)).collect();
                                (v.name.name.clone(), fields)
                            })
                            .collect();
                        self.enums.insert(def, variants);
                    }
                    Item::Alias(_) => {
                        self.alias(def);
                    }
                    Item::Fn(f) => {
                        let params = f.params.iter().map(|p| self.lower(m, p.ty)).collect();
                        let ret = f.ret.map_or(Ty::Unit, |t| self.lower(m, t));
                        let throws = f.throws.map(|t| self.lower(m, t));
                        if let (FnBody::Ai { .. }, Some(ret_ty)) = (&f.body, f.ret) {
                            llm_returns.push((m, f.name.name.as_str(), ret.clone(), ret_ty));
                        }
                        let sig = FnSig {
                            generics: f.generics.len(),
                            params,
                            ret,
                            throws,
                        };
                        self.fns.insert(def, sig);
                    }
                    Item::Import(_) => {}
                }
            }
        }
        // After collection, so records declared later in the file are known.
        for (m, name, ret, ret_ty) in llm_returns {
            self.check_schema(m, name, &ret, ret_ty);
        }
    }

    /// Translates a written type. Generic parameters stay as `Ty::Param`.
    pub fn lower(&mut self, m: ModuleId, id: TypeId) -> Ty {
        let program = self.program;
        let ty = &program.module(m).ast.types[id];
        let TypeKind::Named { path, args } = &ty.kind else {
            return Ty::Error;
        };
        let Some(&res) = self.res.module(m).types.get(id) else {
            return Ty::Error;
        };
        let lowered: Vec<Ty> = args.iter().map(|&a| self.lower(m, a)).collect();
        let name = path
            .segments
            .last()
            .map_or(String::new(), |s| s.name.clone());

        let expected = match res {
            TypeRes::Prim(p) => p.arity(),
            TypeRes::Param(_) => 0,
            TypeRes::Def(d) => match program.item(d) {
                Item::Record(r) => r.generics.len(),
                Item::Enum(e) => e.generics.len(),
                Item::Alias(a) => a.generics.len(),
                _ => 0,
            },
        };
        if lowered.len() != expected {
            let s = |n: usize| if n == 1 { "" } else { "s" };
            self.error(
                m,
                Diagnostic::error(
                    codes::WRONG_TYPE_ARG_COUNT,
                    format!(
                        "`{name}` takes {expected} type argument{} but {} {} given",
                        s(expected),
                        lowered.len(),
                        if lowered.len() == 1 { "was" } else { "were" },
                    ),
                    ty.span,
                )
                .with_label(format!("expected {expected} type argument{}", s(expected))),
            );
        }
        let mut args = lowered;
        args.resize(expected, Ty::Error);
        let arg = |i: usize| args.get(i).cloned().unwrap_or(Ty::Error);

        match res {
            TypeRes::Param(i) => Ty::Param(i),
            TypeRes::Prim(p) => match p {
                Prim::Int => Ty::Int,
                Prim::Float => Ty::Float,
                Prim::String => Ty::String,
                Prim::Bool => Ty::Bool,
                Prim::List => Ty::list(arg(0)),
                Prim::Option => Ty::option(arg(0)),
                Prim::Map => Ty::map(arg(0), arg(1)),
                // Labels are tracked separately from types (M4).
                Prim::Untrusted | Prim::Trusted => arg(0),
            },
            TypeRes::Def(d) => match program.item(d) {
                Item::Alias(_) => self.alias(d).subst(&args),
                _ => Ty::Adt(d, args),
            },
        }
    }

    /// The aliased type, in terms of the alias's own generic parameters.
    fn alias(&mut self, def: DefId) -> Ty {
        if let Some(t) = self.aliases.get(&def) {
            return t.clone();
        }
        let Item::Alias(a) = self.program.item(def) else {
            return Ty::Error;
        };
        if self.expanding.contains(&def) {
            self.error(
                def.module,
                Diagnostic::error(
                    codes::CYCLIC_ALIAS,
                    format!("type alias `{}` refers to itself", a.name.name),
                    a.name.span,
                )
                .with_label("cyclic alias")
                .with_help("use a record or enum to define a recursive type"),
            );
            self.aliases.insert(def, Ty::Error);
            return Ty::Error;
        }
        self.expanding.push(def);
        let t = self.lower(def.module, a.ty);
        self.expanding.pop();
        self.aliases.entry(def).or_insert(t).clone()
    }

    fn check_schema(&mut self, m: ModuleId, fn_name: &str, ret: &Ty, ret_ty: TypeId) {
        let Some(problem) = self.schema_problem(ret, &mut Vec::new()) else {
            return;
        };
        let span = self.program.module(m).ast.types[ret_ty].span;
        self.error(
            m,
            Diagnostic::error(
                codes::LLM_RETURN_NOT_SCHEMA,
                format!("the return type of `ai fn {fn_name}` has no JSON schema"),
                span,
            )
            .with_label(problem)
            .with_help(
                "the model's answer is validated against a JSON schema; use strings, numbers, \
                 booleans, lists, options, `Map<String, _>`, records and enums",
            ),
        );
    }

    /// Why `ty` can't be expressed as a JSON schema, if it can't.
    fn schema_problem(&self, ty: &Ty, visiting: &mut Vec<DefId>) -> Option<String> {
        match ty {
            Ty::Int | Ty::Float | Ty::String | Ty::Bool => None,
            Ty::Error | Ty::Never | Ty::Var(_) => None,
            Ty::List(t) | Ty::Option(t) => self.schema_problem(t, visiting),
            Ty::Map(k, v) => match **k {
                Ty::String | Ty::Error => self.schema_problem(v, visiting),
                _ => Some("JSON object keys must be `String`".into()),
            },
            Ty::Unit => Some("`()` has no JSON representation".into()),
            Ty::Param(_) => Some("a generic type has no fixed schema".into()),
            Ty::Dynamic => Some("tool types aren't known yet".into()),
            Ty::Adt(d, args) => {
                if visiting.contains(d) {
                    return None;
                }
                visiting.push(*d);
                let fields: Vec<Ty> = match (self.records.get(d), self.enums.get(d)) {
                    (Some(fields), _) => fields.iter().map(|(_, t)| t.subst(args)).collect(),
                    (_, Some(variants)) => variants
                        .iter()
                        .flat_map(|(_, ts)| ts.iter().map(|t| t.subst(args)))
                        .collect(),
                    _ => Vec::new(),
                };
                let problem = fields.iter().find_map(|t| self.schema_problem(t, visiting));
                visiting.pop();
                problem
            }
        }
    }
}
