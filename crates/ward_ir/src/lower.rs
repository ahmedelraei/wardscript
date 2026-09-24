//! Checked AST -> WIR.

use std::collections::HashMap;

use la_arena::Arena;
use ward_check::{Analysis, Checked, ModuleTypes};
use ward_resolve::{Builtin, ModuleData, ModuleRes, ValueRes};
use ward_syntax::ast::{self, FnBody, ImportKind, Item};
use ward_syntax::{LineIndex, Severity, Span};

use crate::*;

#[derive(Debug, thiserror::Error)]
pub enum LowerError {
    #[error("the program has errors")]
    HasErrors,
    /// The checker accepted something lowering doesn't understand: a compiler bug.
    #[error("internal compiler error in `{module}`: {message}")]
    Internal { module: String, message: String },
}

/// Lowers a program that checked without errors.
pub fn lower(analysis: &Analysis) -> Result<Program, LowerError> {
    if analysis
        .diagnostics()
        .iter()
        .any(|d| d.diagnostic.severity == Severity::Error)
    {
        return Err(LowerError::HasErrors);
    }
    let checked = &analysis.checked;
    let modules = analysis
        .program
        .module_ids()
        .map(|m| {
            let data = analysis.program.module(m);
            let types = checked.types.get(m.0 as usize);
            let res = analysis.resolution.modules.get(m.0 as usize);
            match (types, res) {
                (Some(types), Some(res)) => lower_module(m, data, res, types, checked),
                _ => Err(LowerError::Internal {
                    module: data.name.clone(),
                    message: "module was not checked".into(),
                }),
            }
        })
        .collect::<Result<_, _>>()?;
    Ok(Program { modules })
}

fn lower_module(
    m: ModuleId,
    data: &ModuleData,
    res: &ModuleRes,
    types: &ModuleTypes,
    checked: &Checked,
) -> Result<Module, LowerError> {
    let internal = |message: String| LowerError::Internal {
        module: data.name.clone(),
        message,
    };
    let lines = LineIndex::new(&data.src);
    let file = site_path(data);
    let mut module = Module {
        name: data.name.clone(),
        path: data.path.clone(),
        records: Vec::new(),
        enums: Vec::new(),
        fns: Vec::new(),
        tools: Vec::new(),
    };
    for (item, it) in data.ast.items.iter().enumerate() {
        let def = DefId { module: m, item };
        let names = |gs: &[ast::Ident]| gs.iter().map(|g| g.name.clone()).collect();
        match it {
            Item::Record(r) => {
                let fields = checked
                    .records
                    .get(&def)
                    .ok_or_else(|| internal(format!("record `{}` has no layout", r.name.name)))?
                    .iter()
                    .map(|(name, ty)| Field {
                        name: name.clone(),
                        ty: ty.clone(),
                    })
                    .collect();
                module.records.push(Record {
                    def,
                    name: r.name.name.clone(),
                    is_pub: r.is_pub,
                    generics: names(&r.generics),
                    fields,
                });
            }
            Item::Enum(e) => {
                let variants = checked
                    .enums
                    .get(&def)
                    .ok_or_else(|| internal(format!("enum `{}` has no layout", e.name.name)))?
                    .iter()
                    .map(|(name, fields)| Variant {
                        name: name.clone(),
                        fields: fields.clone(),
                    })
                    .collect();
                module.enums.push(Enum {
                    def,
                    name: e.name.name.clone(),
                    is_pub: e.is_pub,
                    generics: names(&e.generics),
                    variants,
                });
            }
            Item::Import(ast::Import {
                kind: ImportKind::Tool {
                    provider, source, ..
                },
                ..
            }) => module.tools.push(Tool {
                def,
                provider: provider.name.clone(),
                source: source.clone(),
            }),
            Item::Fn(f) => {
                let sig = checked.fns.get(&def).ok_or_else(|| {
                    internal(format!("function `{}` has no signature", f.name.name))
                })?;
                let mut cx = FnLower {
                    ast: &data.ast,
                    res,
                    types,
                    checked,
                    lines: &lines,
                    file: &file,
                    module: &data.name,
                    fn_name: &f.name.name,
                    locals: Arena::default(),
                    local_map: HashMap::new(),
                    exprs: Arena::default(),
                    stmts: Arena::default(),
                    pats: Arena::default(),
                };
                let params = res
                    .params
                    .get(&item)
                    .into_iter()
                    .flatten()
                    .map(|&p| cx.local(p))
                    .collect::<Result<_, _>>()?;
                let body = match &f.body {
                    FnBody::Block(b) => Body::Block(cx.block(b)?),
                    FnBody::Ai { prompt } => Body::Ai {
                        prompt: cx.expr(*prompt)?,
                    },
                };
                let budget = f
                    .budget
                    .iter()
                    .flatten()
                    .map(|e| {
                        let value = match &data.ast.exprs[e.value].kind {
                            ast::ExprKind::Lit(ast::Lit::Int(n)) => Ok(BudgetValue::Int(*n)),
                            ast::ExprKind::Lit(ast::Lit::Float(s)) => s
                                .parse()
                                .map(BudgetValue::Float)
                                .map_err(|_| internal(format!("budget `{s}` isn't a number"))),
                            _ => Err(internal("budget isn't a literal".to_owned())),
                        }?;
                        Ok((e.name.name.clone(), value))
                    })
                    .collect::<Result<_, LowerError>>()?;
                module.fns.push(Fn {
                    budget,
                    def,
                    name: f.name.name.clone(),
                    is_pub: f.is_pub,
                    generics: names(&f.generics),
                    trusted: checked
                        .trusted_params
                        .get(&def)
                        .cloned()
                        .unwrap_or_else(|| vec![false; f.params.len()]),
                    params,
                    ret: sig.ret.clone(),
                    throws: sig.throws.clone(),
                    locals: cx.locals,
                    exprs: cx.exprs,
                    stmts: cx.stmts,
                    pats: cx.pats,
                    body,
                });
            }
            Item::Alias(_) | Item::Import(_) => {}
        }
    }
    Ok(module)
}

/// `support/tickets.wardscript`: stable no matter where `ward` was run from.
fn site_path(data: &ModuleData) -> String {
    let ext = std::path::Path::new(&data.path)
        .extension()
        .map_or("wardscript".into(), |e| e.to_string_lossy().into_owned());
    format!("{}.{ext}", data.name.replace('.', "/"))
}

struct FnLower<'a> {
    ast: &'a ast::Module,
    res: &'a ModuleRes,
    types: &'a ModuleTypes,
    checked: &'a Checked,
    lines: &'a LineIndex<'a>,
    file: &'a str,
    module: &'a str,
    fn_name: &'a str,
    locals: Arena<Local>,
    local_map: HashMap<ward_resolve::LocalId, LocalId>,
    exprs: Arena<Expr>,
    stmts: Arena<Stmt>,
    pats: Arena<Pat>,
}

type R<T> = Result<T, LowerError>;

impl FnLower<'_> {
    fn bug(&self, message: impl Into<String>) -> LowerError {
        LowerError::Internal {
            module: self.module.to_owned(),
            message: format!("in `{}`: {}", self.fn_name, message.into()),
        }
    }

    fn site(&self, span: Span) -> Site {
        let lc = self.lines.line_col(span.start);
        Site {
            path: self.file.to_owned(),
            line: lc.line,
            column: lc.column,
        }
    }

    fn local(&mut self, id: ward_resolve::LocalId) -> R<LocalId> {
        if let Some(&l) = self.local_map.get(&id) {
            return Ok(l);
        }
        let ty = self.types.locals.get(id).cloned().ok_or_else(|| {
            self.bug(format!(
                "variable `{}` has no type",
                self.res.locals[id].name
            ))
        })?;
        let l = self.locals.alloc(Local {
            name: self.res.locals[id].name.clone(),
            ty,
        });
        self.local_map.insert(id, l);
        Ok(l)
    }

    fn ty(&self, e: ast::ExprId) -> R<Ty> {
        self.types
            .exprs
            .get(e)
            .cloned()
            .ok_or_else(|| self.bug("expression has no type"))
    }

    fn alloc(&mut self, kind: ExprKind, ty: Ty) -> ExprId {
        self.exprs.alloc(Expr { kind, ty })
    }

    fn block(&mut self, b: &ast::Block) -> R<Block> {
        let stmts = b
            .stmts
            .iter()
            .map(|&s| self.stmt(s))
            .collect::<R<Vec<_>>>()?;
        let tail = b.tail.map(|t| self.expr(t)).transpose()?;
        Ok(Block { stmts, tail })
    }

    fn exprs(&mut self, es: &[ast::ExprId]) -> R<Vec<ExprId>> {
        es.iter().map(|&e| self.expr(e)).collect()
    }

    fn stmt(&mut self, id: ast::StmtId) -> R<StmtId> {
        let ast = self.ast;
        let stmt = match &ast.stmts[id].kind {
            ast::StmtKind::Let { init, .. } => {
                let value = self.expr(*init)?;
                let local = self.stmt_local(id)?;
                Stmt::Let { local, value }
            }
            ast::StmtKind::Assign { target, value } => {
                let mut path = Vec::new();
                let local = self.place(*target, &mut path)?;
                let value = self.expr(*value)?;
                Stmt::Assign { local, path, value }
            }
            ast::StmtKind::Expr { expr, .. } => Stmt::Expr(self.expr(*expr)?),
            ast::StmtKind::Return(v) => Stmt::Return(v.map(|v| self.expr(v)).transpose()?),
            ast::StmtKind::Throw(v) => Stmt::Throw(self.expr(*v)?),
            ast::StmtKind::For { iter, body, .. } => {
                let iter = self.expr(*iter)?;
                let local = self.stmt_local(id)?;
                let body = self.block(body)?;
                Stmt::For { local, iter, body }
            }
            ast::StmtKind::While { cond, body } => Stmt::While {
                cond: self.expr(*cond)?,
                body: self.block(body)?,
            },
        };
        Ok(self.stmts.alloc(stmt))
    }

    fn stmt_local(&mut self, id: ast::StmtId) -> R<LocalId> {
        let l = *self
            .res
            .stmt_locals
            .get(id)
            .ok_or_else(|| self.bug("statement binds no variable"))?;
        self.local(l)
    }

    /// Walks an assignment target down to its variable, collecting the path outwards-in.
    fn place(&mut self, e: ast::ExprId, path: &mut Vec<Place>) -> R<LocalId> {
        let ast = self.ast;
        match &ast.exprs[e].kind {
            ast::ExprKind::Name(_) => match self.res.values.get(e) {
                Some(&ValueRes::Local(l)) => self.local(l),
                _ => Err(self.bug("assignment to something that isn't a variable")),
            },
            ast::ExprKind::Field { base, name } => {
                let local = self.place(*base, path)?;
                let record = match self.ty(*base)? {
                    Ty::Adt(d, _) => Some(d),
                    _ => None,
                };
                path.push(Place::Field {
                    record,
                    name: name.name.clone(),
                });
                Ok(local)
            }
            ast::ExprKind::Index { base, index } => {
                let local = self.place(*base, path)?;
                let container = self.ty(*base)?;
                let index = self.expr(*index)?;
                path.push(Place::Index { container, index });
                Ok(local)
            }
            _ => Err(self.bug("invalid assignment target")),
        }
    }

    fn expr(&mut self, e: ast::ExprId) -> R<ExprId> {
        let ast = self.ast;
        let span = ast.exprs[e].span;
        let ty = self.ty(e)?;
        let kind = match &ast.exprs[e].kind {
            ast::ExprKind::Lit(l) => ExprKind::Lit(lit(l)),
            ast::ExprKind::Template(parts) => ExprKind::Template(
                parts
                    .iter()
                    .map(|p| {
                        Ok(match p {
                            ast::TemplatePart::Lit(s) => TemplatePart::Lit(s.clone()),
                            ast::TemplatePart::Expr(x) => TemplatePart::Expr(self.expr(*x)?),
                        })
                    })
                    .collect::<R<_>>()?,
            ),
            ast::ExprKind::Name(_) => match self.res.values.get(e) {
                Some(&res) => self.value(res)?,
                None => return Err(self.bug("unresolved name")),
            },
            ast::ExprKind::Field { base, name } => match self.res.values.get(e) {
                Some(&res) => self.value(res)?,
                None => {
                    let record = match self.ty(*base)? {
                        Ty::Adt(d, _) => Some(d),
                        _ => None,
                    };
                    ExprKind::Field {
                        base: self.expr(*base)?,
                        record,
                        name: name.name.clone(),
                    }
                }
            },
            ast::ExprKind::Call { callee, args } => self.call(*callee, args, span)?,
            ast::ExprKind::Index { base, index } => ExprKind::Index {
                base: self.expr(*base)?,
                index: self.expr(*index)?,
            },
            // Once checked, `?` only marks where an exception may pass through.
            ast::ExprKind::Propagate(inner) => return self.expr(*inner),
            ast::ExprKind::Unary { op, operand } => ExprKind::Unary {
                op: *op,
                operand: self.expr(*operand)?,
            },
            ast::ExprKind::Binary { op, lhs, rhs } => ExprKind::Binary {
                op: *op,
                lhs: self.expr(*lhs)?,
                rhs: self.expr(*rhs)?,
            },
            ast::ExprKind::List(items) => ExprKind::List(self.exprs(items)?),
            ast::ExprKind::Record { fields, .. } => self.record(e, fields)?,
            ast::ExprKind::If { cond, then, else_ } => ExprKind::If {
                cond: self.expr(*cond)?,
                then: self.block(then)?,
                else_: else_.map(|x| self.else_block(x)).transpose()?,
            },
            ast::ExprKind::Match { scrutinee, arms } => {
                let st = self.ty(*scrutinee)?;
                let scrutinee = self.expr(*scrutinee)?;
                let arms = arms
                    .iter()
                    .map(|a| {
                        Ok(Arm {
                            pat: self.pat(a.pat, &st)?,
                            body: self.expr(a.body)?,
                        })
                    })
                    .collect::<R<_>>()?;
                ExprKind::Match { scrutinee, arms }
            }
            ast::ExprKind::TryCatch { body, handler, .. } => {
                let body = self.block(body)?;
                let err = match self.res.catch_locals.get(e) {
                    Some(&l) => Some(self.local(l)?),
                    None => None,
                };
                ExprKind::TryCatch {
                    body,
                    err,
                    handler: self.block(handler)?,
                }
            }
            ast::ExprKind::Block(b) => ExprKind::Block(self.block(b)?),
            ast::ExprKind::Error => return Err(self.bug("error expression")),
        };
        Ok(self.alloc(kind, ty))
    }

    /// `else { ... }` or `else if ...`, as a block.
    fn else_block(&mut self, e: ast::ExprId) -> R<Block> {
        match &self.ast.exprs[e].kind {
            ast::ExprKind::Block(b) => self.block(b),
            _ => Ok(Block {
                stmts: Vec::new(),
                tail: Some(self.expr(e)?),
            }),
        }
    }

    /// A name used as a value: a variable, `None`, or a variant without fields.
    fn value(&mut self, res: ValueRes) -> R<ExprKind> {
        Ok(match res {
            ValueRes::Local(l) => ExprKind::Local(self.local(l)?),
            ValueRes::Variant(enum_, index) => ExprKind::Variant {
                enum_,
                index,
                args: Vec::new(),
            },
            ValueRes::Builtin(Builtin::None) => ExprKind::None,
            _ => return Err(self.bug("name is not a value")),
        })
    }

    fn call(&mut self, callee: ast::ExprId, args: &[ast::ExprId], span: Span) -> R<ExprKind> {
        let ast = self.ast;
        let Some(&res) = self.res.values.get(callee) else {
            let ast::ExprKind::Field { base, name } = &ast.exprs[callee].kind else {
                return Err(self.bug("call of something that isn't a function"));
            };
            let recv_ty = self.ty(*base)?;
            let recv = self.expr(*base)?;
            let args = self.exprs(args)?;
            return match Method::resolve(&recv_ty, &name.name) {
                Some(method) => Ok(ExprKind::Method { method, recv, args }),
                None if recv_ty == Ty::Dynamic => Ok(ExprKind::DynMethod {
                    recv,
                    name: name.name.clone(),
                    args,
                }),
                None => Err(self.bug(format!("unknown method `{}`", name.name))),
            };
        };
        Ok(match res {
            ValueRes::Fn(func) => ExprKind::Call {
                func,
                args: self.exprs(args)?,
            },
            ValueRes::Variant(enum_, index) => ExprKind::Variant {
                enum_,
                index,
                args: self.exprs(args)?,
            },
            ValueRes::Builtin(Builtin::Some) => ExprKind::Some(self.expr(self.arg(args, 0)?)?),
            ValueRes::Builtin(Builtin::Approve) => ExprKind::Approve {
                value: self.expr(self.arg(args, 0)?)?,
                site: self.site(span),
            },
            ValueRes::Builtin(Builtin::Declassify) => ExprKind::Declassify {
                value: self.expr(self.arg(args, 0)?)?,
                reason: self.expr(self.arg(args, 1)?)?,
                site: self.site(span),
            },
            ValueRes::Builtin(Builtin::Validate) => {
                let rule = match self.res.values.get(self.arg(args, 1)?) {
                    Some(&ValueRes::Fn(d)) => d,
                    _ => return Err(self.bug("`validate` rule is not a function")),
                };
                ExprKind::Validate {
                    value: self.expr(self.arg(args, 0)?)?,
                    rule,
                    site: self.site(span),
                }
            }
            ValueRes::ToolMember(tool) => {
                let ast::ExprKind::Field { name, .. } = &ast.exprs[callee].kind else {
                    return Err(self.bug("tool member without a name"));
                };
                ExprKind::ToolCall {
                    tool,
                    name: name.name.clone(),
                    args: self.exprs(args)?,
                }
            }
            _ => return Err(self.bug("call of something that isn't a function")),
        })
    }

    fn arg(&self, args: &[ast::ExprId], i: usize) -> R<ast::ExprId> {
        args.get(i)
            .copied()
            .ok_or_else(|| self.bug("builtin called with too few arguments"))
    }

    fn record(&mut self, e: ast::ExprId, fields: &[ast::FieldInit]) -> R<ExprKind> {
        let record = *self
            .res
            .records
            .get(e)
            .ok_or_else(|| self.bug("record literal without a record"))?;
        let decls = self
            .checked
            .records
            .get(&record)
            .ok_or_else(|| self.bug("record has no layout"))?;
        let mut out = Vec::new();
        for (i, f) in fields.iter().enumerate() {
            let index = decls
                .iter()
                .position(|(n, _)| *n == f.name.name)
                .ok_or_else(|| self.bug(format!("unknown field `{}`", f.name.name)))?;
            let value = match f.value {
                Some(v) => self.expr(v)?,
                None => match self.res.shorthands.get(&(e, i)) {
                    Some(&ValueRes::Local(l)) => {
                        let l = self.local(l)?;
                        let ty = self.locals[l].ty.clone();
                        self.alloc(ExprKind::Local(l), ty)
                    }
                    _ => return Err(self.bug("shorthand field without a variable")),
                },
            };
            out.push((index, value));
        }
        Ok(ExprKind::Record {
            record,
            fields: out,
        })
    }

    fn payload(&self, enum_: DefId, index: usize, args: &[Ty]) -> Vec<Ty> {
        self.checked
            .enums
            .get(&enum_)
            .and_then(|vs| vs.get(index))
            .map(|(_, ts)| ts.iter().map(|t| t.subst(args)).collect())
            .unwrap_or_default()
    }

    /// `ty` is the type of the value being matched, which the checker doesn't record
    /// for patterns.
    fn pat(&mut self, p: ast::PatId, ty: &Ty) -> R<PatId> {
        let ast = self.ast;
        let kind = match &ast.pats[p].kind {
            ast::PatKind::Wild => PatKind::Wild,
            ast::PatKind::Name(_) => match self.res.pat_variants.get(p) {
                Some(ValueRes::Builtin(Builtin::None)) => PatKind::None,
                _ => match self.res.pat_bindings.get(p) {
                    Some(&l) => PatKind::Bind(self.local(l)?),
                    None => return Err(self.bug("pattern name binds nothing")),
                },
            },
            ast::PatKind::Lit(l) => PatKind::Lit(lit(l)),
            ast::PatKind::Variant { args, .. } => {
                let args = args.as_deref().unwrap_or(&[]);
                match self.res.pat_variants.get(p) {
                    Some(ValueRes::Builtin(Builtin::None)) => PatKind::None,
                    Some(ValueRes::Builtin(Builtin::Some)) => {
                        let inner = match ty {
                            Ty::Option(t) => (**t).clone(),
                            _ => Ty::Error,
                        };
                        let sub = args
                            .first()
                            .ok_or_else(|| self.bug("`Some` pattern without a field"))?;
                        PatKind::Some(self.pat(*sub, &inner)?)
                    }
                    Some(&ValueRes::Variant(enum_, index)) => {
                        let targs = match ty {
                            Ty::Adt(_, a) => a.clone(),
                            _ => Vec::new(),
                        };
                        let payload = self.payload(enum_, index, &targs);
                        let args = args
                            .iter()
                            .enumerate()
                            .map(|(i, &a)| self.pat(a, payload.get(i).unwrap_or(&Ty::Error)))
                            .collect::<R<_>>()?;
                        PatKind::Variant { enum_, index, args }
                    }
                    _ => return Err(self.bug("pattern is not a variant")),
                }
            }
            ast::PatKind::Error => return Err(self.bug("error pattern")),
        };
        Ok(self.pats.alloc(Pat {
            kind,
            ty: ty.clone(),
        }))
    }
}

fn lit(l: &ast::Lit) -> Lit {
    match l {
        ast::Lit::Int(i) => Lit::Int(*i),
        ast::Lit::Float(f) => Lit::Float(f.clone()),
        ast::Lit::Str(s) => Lit::Str(s.clone()),
        ast::Lit::Bool(b) => Lit::Bool(*b),
    }
}
