use std::collections::HashMap;

use ward_syntax::ast::*;
use ward_syntax::diag::codes;
use ward_syntax::{Diagnostic, Span};

use crate::{
    Builtin, DefId, Local, LocalId, ModuleId, ModuleRes, Prim, Program, ProgramDiagnostic,
    Resolution, TypeRes, ValueRes, did_you_mean,
};

#[derive(Clone, Copy)]
enum ItemRes {
    Fn(DefId),
    /// A record or alias.
    Type(DefId),
    Enum(DefId),
    Module(ModuleId),
    Tool(DefId),
}

struct ItemEntry {
    res: ItemRes,
    span: Span,
    is_pub: bool,
}

/// Module-level names. Types and values share one namespace.
#[derive(Default)]
struct ModuleScope {
    items: HashMap<String, ItemEntry>,
}

const BUILTINS: [(&str, Builtin); 5] = [
    ("Some", Builtin::Some),
    ("None", Builtin::None),
    ("validate", Builtin::Validate),
    ("approve", Builtin::Approve),
    ("declassify", Builtin::Declassify),
];

pub fn resolve(program: &Program) -> (Resolution, Vec<ProgramDiagnostic>) {
    let mut diags = Vec::new();
    let scopes: Vec<ModuleScope> = program
        .module_ids()
        .map(|m| collect_items(program, m, &mut diags))
        .collect();
    let modules = program
        .module_ids()
        .map(|m| {
            let mut r = BodyResolver {
                program,
                scopes: &scopes,
                m,
                ast: &program.module(m).ast,
                res: ModuleRes::default(),
                diags: &mut diags,
                locals: Vec::new(),
                generics: Vec::new(),
            };
            r.module();
            r.res
        })
        .collect();
    (Resolution { modules }, diags)
}

fn duplicate(name: &str, span: Span, first: Span, what: &str) -> Diagnostic {
    Diagnostic::error(
        codes::DUPLICATE_DEFINITION,
        format!("{what} `{name}` is defined more than once"),
        span,
    )
    .with_label("redefined here")
    .with_secondary(first, "first defined here")
}

fn collect_items(
    program: &Program,
    m: ModuleId,
    diags: &mut Vec<ProgramDiagnostic>,
) -> ModuleScope {
    let module = program.module(m);
    let mut scope = ModuleScope::default();
    for (item, it) in module.ast.items.iter().enumerate() {
        let def = DefId { module: m, item };
        let (name, res, is_pub) = match it {
            Item::Fn(f) => (&f.name, ItemRes::Fn(def), f.is_pub),
            Item::Record(r) => (&r.name, ItemRes::Type(def), r.is_pub),
            Item::Alias(a) => (&a.name, ItemRes::Type(def), a.is_pub),
            Item::Enum(e) => (&e.name, ItemRes::Enum(def), e.is_pub),
            Item::Import(imp) => match &imp.kind {
                ImportKind::Module(path) => {
                    // Unresolved imports were reported by the loader.
                    let Some(&target) = module.imports.get(&item) else {
                        continue;
                    };
                    let Some(name) = imp.alias.as_ref().or(path.segments.last()) else {
                        continue;
                    };
                    (name, ItemRes::Module(target), false)
                }
                ImportKind::Tool { .. } => match &imp.alias {
                    Some(alias) => (alias, ItemRes::Tool(def), false),
                    None => continue,
                },
            },
        };
        if let Some(first) = scope.items.get(&name.name) {
            diags.push(ProgramDiagnostic {
                module: m,
                diagnostic: duplicate(&name.name, name.span, first.span, "the name"),
            });
            continue;
        }
        scope.items.insert(
            name.name.clone(),
            ItemEntry {
                res,
                span: name.span,
                is_pub,
            },
        );
    }
    scope
}

enum Lookup {
    Value(ValueRes),
    /// A record or alias name used where a value is expected.
    Type(DefId),
    NotFound,
}

struct BodyResolver<'a> {
    program: &'a Program,
    scopes: &'a [ModuleScope],
    m: ModuleId,
    ast: &'a Module,
    res: ModuleRes,
    diags: &'a mut Vec<ProgramDiagnostic>,
    /// Innermost scope last.
    locals: Vec<HashMap<String, LocalId>>,
    generics: Vec<String>,
}

impl BodyResolver<'_> {
    fn error(&mut self, diagnostic: Diagnostic) {
        self.diags.push(ProgramDiagnostic {
            module: self.m,
            diagnostic,
        });
    }

    fn scope(&self) -> &ModuleScope {
        &self.scopes[self.m.0 as usize]
    }

    fn module(&mut self) {
        let ast = self.ast;
        for (item, it) in ast.items.iter().enumerate() {
            match it {
                Item::Fn(f) => self.fn_decl(item, f),
                Item::Record(r) => {
                    self.set_generics(&r.generics);
                    let mut seen: HashMap<&str, Span> = HashMap::new();
                    for field in &r.fields {
                        if let Some(&first) = seen.get(field.name.name.as_str()) {
                            self.error(duplicate(
                                &field.name.name,
                                field.name.span,
                                first,
                                "field",
                            ));
                        }
                        seen.insert(&field.name.name, field.name.span);
                        self.ty(field.ty);
                    }
                }
                Item::Alias(a) => {
                    self.set_generics(&a.generics);
                    self.ty(a.ty);
                }
                Item::Enum(e) => {
                    self.set_generics(&e.generics);
                    let mut seen: HashMap<&str, Span> = HashMap::new();
                    for v in &e.variants {
                        if let Some(&first) = seen.get(v.name.name.as_str()) {
                            self.error(duplicate(&v.name.name, v.name.span, first, "variant"));
                        }
                        seen.insert(&v.name.name, v.name.span);
                        for &t in &v.fields {
                            self.ty(t);
                        }
                    }
                }
                Item::Import(_) => {}
            }
        }
    }

    fn set_generics(&mut self, generics: &[Ident]) {
        self.generics = generics.iter().map(|g| g.name.clone()).collect();
    }

    fn fn_decl(&mut self, item: usize, f: &FnDecl) {
        self.set_generics(&f.generics);
        self.locals.push(HashMap::new());
        let mut params = Vec::new();
        for p in &f.params {
            self.ty(p.ty);
            if let Some(&first) = self.locals.last().and_then(|s| s.get(&p.name.name)) {
                let first = self.res.locals[first].span;
                self.error(duplicate(&p.name.name, p.name.span, first, "parameter"));
            }
            params.push(self.bind(&p.name));
        }
        self.res.params.insert(item, params);
        if let Some(ret) = f.ret {
            self.ty(ret);
        }
        if let Some(throws) = f.throws {
            self.ty(throws);
        }
        for entry in f.budget.iter().flatten() {
            self.expr(entry.value);
        }
        if let Some(checks) = &f.checks {
            self.locals.push(HashMap::new());
            let it = self.bind(&Ident {
                name: "it".to_owned(),
                span: checks.span,
            });
            self.res.check_its.insert(item, it);
            for e in &checks.entries {
                self.expr(e.cond);
            }
            self.locals.pop();
        }
        match &f.body {
            FnBody::Block(b) => self.block(b),
            FnBody::Ai { prompt } => self.expr(*prompt),
        }
        self.locals.pop();
    }

    fn bind(&mut self, name: &Ident) -> LocalId {
        let id = self.res.locals.alloc(Local {
            name: name.name.clone(),
            span: name.span,
        });
        if let Some(scope) = self.locals.last_mut() {
            scope.insert(name.name.clone(), id);
        }
        id
    }

    fn lookup(&self, name: &str) -> Lookup {
        if let Some(&id) = self.locals.iter().rev().find_map(|s| s.get(name)) {
            return Lookup::Value(ValueRes::Local(id));
        }
        if let Some(entry) = self.scope().items.get(name) {
            return match entry.res {
                ItemRes::Fn(d) => Lookup::Value(ValueRes::Fn(d)),
                ItemRes::Enum(d) => Lookup::Value(ValueRes::Enum(d)),
                ItemRes::Module(m) => Lookup::Value(ValueRes::Module(m)),
                ItemRes::Tool(d) => Lookup::Value(ValueRes::Tool(d)),
                ItemRes::Type(d) => Lookup::Type(d),
            };
        }
        match BUILTINS.iter().find(|(n, _)| *n == name) {
            Some(&(_, b)) => Lookup::Value(ValueRes::Builtin(b)),
            None => Lookup::NotFound,
        }
    }

    fn value_candidates(&self) -> Vec<&str> {
        let locals = self
            .locals
            .iter()
            .flat_map(|s| s.keys().map(String::as_str));
        let items = self
            .scope()
            .items
            .iter()
            .filter_map(|(n, e)| (!matches!(e.res, ItemRes::Type(_))).then_some(n.as_str()));
        locals
            .chain(items)
            .chain(BUILTINS.iter().map(|(n, _)| *n))
            .collect()
    }

    fn with_suggestion(d: Diagnostic, name: &str, candidates: Vec<&str>) -> Diagnostic {
        match did_you_mean(name, candidates) {
            Some(s) => d.with_help(format!("a similar name exists: `{s}`")),
            None => d,
        }
    }

    /// Resolves a name in value position, reporting it if it isn't a value.
    fn value(&mut self, name: &Ident) -> Option<ValueRes> {
        match self.lookup(&name.name) {
            Lookup::Value(v) => Some(v),
            Lookup::Type(def) => {
                let d = self.type_not_value(&name.name, name.span);
                let d = match self.program.item(def) {
                    Item::Record(_) => {
                        d.with_help(format!("build a value with `{} {{ ... }}`", name.name))
                    }
                    _ => d,
                };
                self.error(d);
                None
            }
            Lookup::NotFound if Prim::ALL.iter().any(|(n, _)| *n == name.name) => {
                let d = self.type_not_value(&name.name, name.span);
                self.error(d);
                None
            }
            Lookup::NotFound => {
                let d = Diagnostic::error(
                    codes::UNRESOLVED_VALUE,
                    format!("cannot find `{}` in this scope", name.name),
                    name.span,
                )
                .with_label("not found");
                let d = Self::with_suggestion(d, &name.name, self.value_candidates());
                self.error(d);
                None
            }
        }
    }

    fn type_not_value(&self, name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            codes::WRONG_KIND_OF_NAME,
            format!("`{name}` is a type, not a value"),
            span,
        )
        .with_label("used as a value here")
    }

    /// `base.name` where `base` is a namespace (enum, module or tool). `None` for a plain
    /// field access on a value, or after reporting an error.
    fn member(&mut self, base: ValueRes, name: &Ident) -> Option<ValueRes> {
        match base {
            ValueRes::Enum(def) => {
                let Item::Enum(e) = self.program.item(def) else {
                    return None;
                };
                if let Some(i) = e.variants.iter().position(|v| v.name.name == name.name) {
                    return Some(ValueRes::Variant(def, i));
                }
                let d = Diagnostic::error(
                    codes::NO_SUCH_MEMBER,
                    format!("no variant `{}` in enum `{}`", name.name, e.name.name),
                    name.span,
                )
                .with_label("unknown variant");
                let names = e.variants.iter().map(|v| v.name.name.as_str()).collect();
                let d = Self::with_suggestion(d, &name.name, names);
                self.error(d);
                None
            }
            ValueRes::Module(mid) => {
                let res = self.module_member(mid, name)?;
                match res {
                    ItemRes::Fn(d) => Some(ValueRes::Fn(d)),
                    ItemRes::Enum(d) => Some(ValueRes::Enum(d)),
                    ItemRes::Type(_) => {
                        let d = self.type_not_value(&name.name, name.span);
                        self.error(d);
                        None
                    }
                    // Imports are never `pub`, so they aren't reachable from outside.
                    ItemRes::Module(_) | ItemRes::Tool(_) => None,
                }
            }
            ValueRes::Tool(def) => {
                if let Some(server) = self.program.tool_schema(def) {
                    if server.function(&name.name).is_none() {
                        let d = Diagnostic::error(
                            codes::NO_SUCH_MEMBER,
                            format!("tool `{}` has no function `{}`", server.source, name.name),
                            name.span,
                        )
                        .with_label("unknown tool function");
                        let names = server.functions.iter().map(|f| f.name.as_str()).collect();
                        let d = Self::with_suggestion(d, &name.name, names);
                        self.error(d);
                        return None;
                    }
                }
                Some(ValueRes::ToolMember(def))
            }
            ValueRes::ToolMember(def) => Some(ValueRes::ToolMember(def)),
            ValueRes::Local(_) | ValueRes::Fn(_) | ValueRes::Variant(..) | ValueRes::Builtin(_) => {
                None
            }
        }
    }

    fn module_member(&mut self, mid: ModuleId, name: &Ident) -> Option<ItemRes> {
        let scope = &self.scopes[mid.0 as usize];
        let module = self.program.module(mid);
        let (path, module_name) = (&module.path, &module.name);
        let Some(entry) = scope.items.get(&name.name) else {
            let d = Diagnostic::error(
                codes::NO_SUCH_MEMBER,
                format!("cannot find `{}` in module `{module_name}`", name.name),
                name.span,
            )
            .with_label("not found in that module");
            let names = scope
                .items
                .iter()
                .filter(|(_, e)| e.is_pub)
                .map(|(n, _)| n.as_str())
                .collect();
            let d = Self::with_suggestion(d, &name.name, names);
            self.error(d);
            return None;
        };
        if !entry.is_pub {
            self.error(
                Diagnostic::error(
                    codes::PRIVATE_ITEM,
                    format!("`{}` is private", name.name),
                    name.span,
                )
                .with_label("not visible outside its module")
                .with_help(format!("mark it `pub` in `{path}`")),
            );
            return None;
        }
        Some(entry.res)
    }

    fn block(&mut self, b: &Block) {
        self.locals.push(HashMap::new());
        for &s in &b.stmts {
            self.stmt(s);
        }
        if let Some(tail) = b.tail {
            self.expr(tail);
        }
        self.locals.pop();
    }

    fn stmt(&mut self, id: StmtId) {
        let ast = self.ast;
        match &ast.stmts[id].kind {
            StmtKind::Let { name, ty, init } => {
                if let Some(ty) = ty {
                    self.ty(*ty);
                }
                self.expr(*init);
                let local = self.bind(name);
                self.res.stmt_locals.insert(id, local);
            }
            StmtKind::Assign { target, value } => {
                self.expr(*target);
                self.expr(*value);
            }
            StmtKind::Expr { expr, .. } => self.expr(*expr),
            StmtKind::Return(value) => {
                if let Some(v) = value {
                    self.expr(*v);
                }
            }
            StmtKind::Throw(value) => self.expr(*value),
            StmtKind::For { var, iter, body } => {
                self.expr(*iter);
                self.locals.push(HashMap::new());
                let local = self.bind(var);
                self.res.stmt_locals.insert(id, local);
                self.block(body);
                self.locals.pop();
            }
            StmtKind::While { cond, body } => {
                self.expr(*cond);
                self.block(body);
            }
        }
    }

    fn expr(&mut self, id: ExprId) {
        let ast = self.ast;
        match &ast.exprs[id].kind {
            ExprKind::Lit(_) | ExprKind::Error => {}
            ExprKind::Template(parts) => {
                for part in parts {
                    if let TemplatePart::Expr(e) = part {
                        self.expr(*e);
                    }
                }
            }
            ExprKind::Name(name) => {
                if let Some(v) = self.value(name) {
                    self.res.values.insert(id, v);
                }
            }
            ExprKind::Field { base, name } => {
                self.expr(*base);
                if let Some(&base_res) = self.res.values.get(*base) {
                    if let Some(v) = self.member(base_res, name) {
                        self.res.values.insert(id, v);
                    }
                }
            }
            ExprKind::Call { callee, args } => {
                self.expr(*callee);
                for &a in args {
                    self.expr(a);
                }
            }
            ExprKind::Index { base, index } => {
                self.expr(*base);
                self.expr(*index);
            }
            ExprKind::Propagate(e) => self.expr(*e),
            ExprKind::Unary { operand, .. } => self.expr(*operand),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(*lhs);
                self.expr(*rhs);
            }
            ExprKind::List(items) => {
                for &e in items {
                    self.expr(e);
                }
            }
            ExprKind::Record { path, fields } => {
                self.record_target(id, path);
                for (i, f) in fields.iter().enumerate() {
                    match f.value {
                        Some(v) => self.expr(v),
                        None => {
                            if let Some(v) = self.value(&f.name) {
                                self.res.shorthands.insert((id, i), v);
                            }
                        }
                    }
                }
            }
            ExprKind::If { cond, then, else_ } => {
                self.expr(*cond);
                self.block(then);
                if let Some(e) = else_ {
                    self.expr(*e);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expr(*scrutinee);
                for arm in arms {
                    self.locals.push(HashMap::new());
                    self.pat(arm.pat, &mut HashMap::new());
                    self.expr(arm.body);
                    self.locals.pop();
                }
            }
            ExprKind::TryCatch { body, err, handler } => {
                self.block(body);
                self.locals.push(HashMap::new());
                if let Some(err) = err {
                    let local = self.bind(err);
                    self.res.catch_locals.insert(id, local);
                }
                self.block(handler);
                self.locals.pop();
            }
            ExprKind::Block(b) => self.block(b),
        }
    }

    fn record_target(&mut self, id: ExprId, path: &Path) {
        let Some(name) = path.segments.last() else {
            return;
        };
        let found = if path.segments.len() == 1 {
            self.scope().items.get(&name.name).map(|e| e.res)
        } else {
            match self.type_path(path) {
                Some(TypeRes::Def(d)) => Some(ItemRes::Type(d)),
                // `type_path` reported the problem.
                _ => return,
            }
        };
        match found {
            Some(ItemRes::Type(def)) if matches!(self.program.item(def), Item::Record(_)) => {
                self.res.records.insert(id, def);
            }
            Some(_) => self.error(
                Diagnostic::error(
                    codes::WRONG_KIND_OF_NAME,
                    format!("`{}` is not a record type", name.name),
                    name.span,
                )
                .with_label("expected a record type"),
            ),
            None => {
                let d = Diagnostic::error(
                    codes::UNRESOLVED_TYPE,
                    format!("cannot find record type `{}`", name.name),
                    name.span,
                )
                .with_label("not found");
                let d = Self::with_suggestion(d, &name.name, self.type_candidates());
                self.error(d);
            }
        }
    }

    fn pat(&mut self, id: PatId, seen: &mut HashMap<String, Span>) {
        let ast = self.ast;
        match &ast.pats[id].kind {
            PatKind::Wild | PatKind::Lit(_) | PatKind::Error => {}
            PatKind::Name(name)
                if name.name == "None" && !self.scope().items.contains_key("None") =>
            {
                self.res
                    .pat_variants
                    .insert(id, ValueRes::Builtin(Builtin::None));
            }
            PatKind::Name(name) => {
                if let Some(&first) = seen.get(&name.name) {
                    self.error(
                        duplicate(&name.name, name.span, first, "the binding")
                            .with_help("each name can be bound only once per pattern"),
                    );
                }
                seen.insert(name.name.clone(), name.span);
                let local = self.bind(name);
                self.res.pat_bindings.insert(id, local);
            }
            PatKind::Variant { path, args } => {
                if let Some(v) = self.pat_path(path) {
                    let is_variant = matches!(
                        v,
                        ValueRes::Variant(..) | ValueRes::Builtin(Builtin::Some | Builtin::None)
                    );
                    if is_variant {
                        self.res.pat_variants.insert(id, v);
                    } else {
                        self.error(
                            Diagnostic::error(
                                codes::WRONG_KIND_OF_NAME,
                                "expected an enum variant in this pattern",
                                path.span,
                            )
                            .with_label("not a variant")
                            .with_help("variants are written `Enum.Variant`, or `Some`, `None`"),
                        );
                    }
                }
                for &a in args.iter().flatten() {
                    self.pat(a, seen);
                }
            }
        }
    }

    /// Resolves a pattern path like `Priority.High` or `Some`. Locals are not in scope here.
    fn pat_path(&mut self, path: &Path) -> Option<ValueRes> {
        let (first, rest) = path.segments.split_first()?;
        let saved = std::mem::take(&mut self.locals);
        let res = self.value(first);
        self.locals = saved;
        let mut res = res?;
        for seg in rest {
            match self.member(res, seg) {
                Some(next) => res = next,
                None => {
                    if !matches!(res, ValueRes::Enum(_) | ValueRes::Module(_)) {
                        self.error(
                            Diagnostic::error(
                                codes::NO_SUCH_MEMBER,
                                format!("`{}` has no members", first.name),
                                seg.span,
                            )
                            .with_label("unexpected path segment"),
                        );
                    }
                    return None;
                }
            }
        }
        Some(res)
    }

    fn type_candidates(&self) -> Vec<&str> {
        let items = self.scope().items.iter().filter_map(|(n, e)| {
            matches!(e.res, ItemRes::Type(_) | ItemRes::Enum(_)).then_some(n.as_str())
        });
        self.generics
            .iter()
            .map(String::as_str)
            .chain(items)
            .chain(Prim::ALL.iter().map(|(n, _)| *n))
            .collect()
    }

    fn ty(&mut self, id: TypeId) {
        let ast = self.ast;
        let TypeKind::Named { path, args } = &ast.types[id].kind else {
            return;
        };
        for &a in args {
            self.ty(a);
        }
        if let Some(res) = self.type_path(path) {
            self.res.types.insert(id, res);
        }
        if let Some(cond) = ast.types[id].refinement {
            // Only `it` is in scope: a refinement is about the value alone.
            let saved = std::mem::take(&mut self.locals);
            self.locals.push(HashMap::new());
            let it = self.bind(&Ident {
                name: "it".to_owned(),
                span: ast.types[id].span,
            });
            self.res.refinement_its.insert(id, it);
            self.expr(cond);
            self.locals = saved;
        }
    }

    fn type_path(&mut self, path: &Path) -> Option<TypeRes> {
        match path.segments.as_slice() {
            [name] => {
                if let Some(i) = self.generics.iter().position(|g| *g == name.name) {
                    return Some(TypeRes::Param(i));
                }
                match self.scope().items.get(&name.name).map(|e| e.res) {
                    Some(ItemRes::Type(d) | ItemRes::Enum(d)) => return Some(TypeRes::Def(d)),
                    Some(_) => {
                        self.not_a_type(name);
                        return None;
                    }
                    None => {}
                }
                if let Some(&(_, p)) = Prim::ALL.iter().find(|(n, _)| *n == name.name) {
                    return Some(TypeRes::Prim(p));
                }
                let d = Diagnostic::error(
                    codes::UNRESOLVED_TYPE,
                    format!("cannot find type `{}`", name.name),
                    name.span,
                )
                .with_label("not found");
                let d = Self::with_suggestion(d, &name.name, self.type_candidates());
                self.error(d);
                None
            }
            [module, name] => {
                let Some(ItemRes::Module(mid)) =
                    self.scope().items.get(&module.name).map(|e| e.res)
                else {
                    self.error(
                        Diagnostic::error(
                            codes::UNRESOLVED_TYPE,
                            format!("cannot find module `{}`", module.name),
                            module.span,
                        )
                        .with_label("expected an imported module"),
                    );
                    return None;
                };
                match self.module_member(mid, name)? {
                    ItemRes::Type(d) | ItemRes::Enum(d) => Some(TypeRes::Def(d)),
                    _ => {
                        self.not_a_type(name);
                        None
                    }
                }
            }
            _ => {
                self.error(
                    Diagnostic::error(
                        codes::UNRESOLVED_TYPE,
                        "type paths have at most two segments (`module.Type`)",
                        path.span,
                    )
                    .with_label("too many segments"),
                );
                None
            }
        }
    }

    fn not_a_type(&mut self, name: &Ident) {
        self.error(
            Diagnostic::error(
                codes::WRONG_KIND_OF_NAME,
                format!("`{}` is not a type", name.name),
                name.span,
            )
            .with_label("expected a type"),
        );
    }
}
