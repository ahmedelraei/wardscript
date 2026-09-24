//! AST → canonical source. Comments are not preserved.

use crate::ast::*;

pub fn print(module: &Module) -> String {
    let mut p = Printer::new(module, false);
    p.module();
    p.out
}

/// Prints one expression with every unary and binary operation parenthesized, which makes
/// precedence and associativity visible in tests.
pub fn print_expr_parenthesized(module: &Module, expr: ExprId) -> String {
    let mut p = Printer::new(module, true);
    p.expr(expr, false);
    p.out
}

struct Printer<'m> {
    m: &'m Module,
    out: String,
    indent: usize,
    parens_all: bool,
}

impl<'m> Printer<'m> {
    fn new(m: &'m Module, parens_all: bool) -> Self {
        Printer {
            m,
            out: String::new(),
            indent: 0,
            parens_all,
        }
    }

    fn w(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn newline(&mut self) {
        self.out.push('\n');
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
    }

    fn sep<X>(&mut self, items: &[X], sep: &str, mut f: impl FnMut(&mut Self, &X)) {
        for (i, x) in items.iter().enumerate() {
            if i > 0 {
                self.w(sep);
            }
            f(self, x);
        }
    }

    fn module(&mut self) {
        let mut prev_import = false;
        for (i, item) in self.m.items.iter().enumerate() {
            let is_import = matches!(item, Item::Import(_));
            if i > 0 {
                self.w(if is_import && prev_import {
                    "\n"
                } else {
                    "\n\n"
                });
            }
            prev_import = is_import;
            self.item(item);
        }
        if !self.m.items.is_empty() {
            self.w("\n");
        }
    }

    fn item(&mut self, item: &Item) {
        match item {
            Item::Import(i) => self.import(i),
            Item::Record(r) => {
                self.vis(r.is_pub);
                self.w("type ");
                self.w(&r.name.name);
                self.generics(&r.generics);
                self.w(" ");
                self.braced_lines(&r.fields, |p, f| {
                    p.w(&f.name.name);
                    p.w(": ");
                    p.ty(f.ty);
                });
            }
            Item::Alias(a) => {
                self.vis(a.is_pub);
                self.w("type ");
                self.w(&a.name.name);
                self.generics(&a.generics);
                self.w(" = ");
                self.ty(a.ty);
            }
            Item::Enum(e) => {
                self.vis(e.is_pub);
                self.w("enum ");
                self.w(&e.name.name);
                self.generics(&e.generics);
                self.w(" ");
                self.braced_lines(&e.variants, |p, v| {
                    p.w(&v.name.name);
                    if !v.fields.is_empty() {
                        p.w("(");
                        p.sep(&v.fields, ", ", |p, t| p.ty(*t));
                        p.w(")");
                    }
                });
            }
            Item::Fn(f) => self.fn_decl(f),
        }
    }

    fn vis(&mut self, is_pub: bool) {
        if is_pub {
            self.w("pub ");
        }
    }

    fn generics(&mut self, generics: &[Ident]) {
        if !generics.is_empty() {
            self.w("<");
            self.sep(generics, ", ", |p, g| p.w(&g.name));
            self.w(">");
        }
    }

    /// `{}` when empty, otherwise one comma-terminated entry per line.
    fn braced_lines<X>(&mut self, items: &[X], mut f: impl FnMut(&mut Self, &X)) {
        if items.is_empty() {
            self.w("{}");
            return;
        }
        self.w("{");
        self.indent += 1;
        for x in items {
            self.newline();
            f(self, x);
            self.w(",");
        }
        self.indent -= 1;
        self.newline();
        self.w("}");
    }

    fn import(&mut self, i: &Import) {
        self.w("import ");
        match &i.kind {
            ImportKind::Module(path) => self.path(path),
            ImportKind::Tool {
                provider, source, ..
            } => {
                self.w(&provider.name);
                self.w(" ");
                self.string(source);
            }
        }
        if let Some(alias) = &i.alias {
            self.w(" as ");
            self.w(&alias.name);
        }
    }

    fn path(&mut self, path: &Path) {
        self.sep(&path.segments, ".", |p, s| p.w(&s.name));
    }

    fn fn_decl(&mut self, f: &FnDecl) {
        self.vis(f.is_pub);
        self.w("fn ");
        self.w(&f.name.name);
        self.generics(&f.generics);
        self.w("(");
        self.sep(&f.params, ", ", |p, param| {
            p.w(&param.name.name);
            p.w(": ");
            p.ty(param.ty);
        });
        self.w(")");
        if let Some(ret) = f.ret {
            self.w(" -> ");
            self.ty(ret);
        }
        // Headers with clauses or an llm body put each part on its own line.
        let multiline =
            f.uses.is_some() || f.budget.is_some() || matches!(f.body, FnBody::Llm { .. });
        self.indent += 1;
        if let Some(uses) = &f.uses {
            self.newline();
            self.w("uses {");
            self.sep(uses, ", ", |p, e| p.path(e));
            self.w("}");
        }
        if let Some(budget) = &f.budget {
            self.newline();
            self.w("budget {");
            self.sep(budget, ", ", |p, b| {
                p.w(&b.name.name);
                p.w(": ");
                p.expr(b.value, false);
            });
            self.w("}");
        }
        match &f.body {
            FnBody::Llm { prompt } => {
                self.newline();
                self.w("by llm ");
                self.expr(*prompt, false);
                self.indent -= 1;
            }
            FnBody::Block(b) => {
                self.indent -= 1;
                if multiline {
                    self.newline();
                } else {
                    self.w(" ");
                }
                self.block(b);
            }
        }
    }

    fn ty(&mut self, id: TypeId) {
        match &self.m.types[id].kind {
            TypeKind::Named { path, args } => {
                self.path(path);
                if !args.is_empty() {
                    self.w("<");
                    self.sep(args, ", ", |p, a| p.ty(*a));
                    self.w(">");
                }
            }
            TypeKind::Error => self.w("<error>"),
        }
    }

    fn block(&mut self, b: &Block) {
        if b.stmts.is_empty() && b.tail.is_none() {
            self.w("{}");
            return;
        }
        self.w("{");
        self.indent += 1;
        for &s in &b.stmts {
            self.newline();
            self.stmt(s);
        }
        if let Some(tail) = b.tail {
            self.newline();
            self.stmt_expr(tail);
        }
        self.indent -= 1;
        self.newline();
        self.w("}");
    }

    fn stmt(&mut self, id: StmtId) {
        match &self.m.stmts[id].kind {
            StmtKind::Let { name, ty, init } => {
                self.w("let ");
                self.w(&name.name);
                if let Some(ty) = ty {
                    self.w(": ");
                    self.ty(*ty);
                }
                self.w(" = ");
                self.expr(*init, false);
                self.w(";");
            }
            StmtKind::Assign { target, value } => {
                self.stmt_expr(*target);
                self.w(" = ");
                self.expr(*value, false);
                self.w(";");
            }
            StmtKind::Expr { expr, semi } => {
                self.stmt_expr(*expr);
                if *semi {
                    self.w(";");
                }
            }
            StmtKind::Return(value) => {
                self.w("return");
                if let Some(v) = value {
                    self.w(" ");
                    self.expr(*v, false);
                }
                self.w(";");
            }
            StmtKind::For { var, iter, body } => {
                self.w("for ");
                self.w(&var.name);
                self.w(" in ");
                self.expr(*iter, true);
                self.w(" ");
                self.block(body);
            }
            StmtKind::While { cond, body } => {
                self.w("while ");
                self.expr(*cond, true);
                self.w(" ");
                self.block(body);
            }
        }
    }

    /// In statement position a leading `if`/`match`/block would end the statement early,
    /// so an expression that merely *starts* with one needs parentheses.
    fn stmt_expr(&mut self, id: ExprId) {
        let e = &self.m.exprs[id];
        if !e.kind.is_block_like() && self.starts_block_like(id) {
            self.w("(");
            self.expr(id, false);
            self.w(")");
        } else {
            self.expr(id, false);
        }
    }

    fn starts_block_like(&self, id: ExprId) -> bool {
        match &self.m.exprs[id].kind {
            ExprKind::If { .. } | ExprKind::Match { .. } | ExprKind::Block(_) => true,
            ExprKind::Binary { lhs: e, .. }
            | ExprKind::Field { base: e, .. }
            | ExprKind::Index { base: e, .. }
            | ExprKind::Call { callee: e, .. }
            | ExprKind::Try(e) => self.prec(*e) >= self.prec(id) && self.starts_block_like(*e),
            _ => false,
        }
    }

    fn prec(&self, id: ExprId) -> u8 {
        match &self.m.exprs[id].kind {
            ExprKind::Binary { op, .. } => op.prec(),
            ExprKind::Unary { .. } => PREC_UNARY,
            _ => PREC_POSTFIX,
        }
    }

    fn expr_in_parens(&mut self, id: ExprId, parens: bool, no_struct: bool) {
        if parens {
            self.w("(");
            self.expr(id, false);
            self.w(")");
        } else {
            self.expr(id, no_struct);
        }
    }

    /// `no_struct`: we're in an `if`/`while`/`match`/`for` head, where a bare record literal
    /// would be read as the start of the body.
    fn expr(&mut self, id: ExprId, no_struct: bool) {
        let m = self.m;
        match &m.exprs[id].kind {
            ExprKind::Lit(lit) => self.lit(lit),
            ExprKind::Template(parts) => {
                self.w("\"");
                for part in parts {
                    match part {
                        TemplatePart::Lit(s) => self.w(&escape(s)),
                        TemplatePart::Expr(e) => {
                            self.w("{");
                            self.expr(*e, false);
                            self.w("}");
                        }
                    }
                }
                self.w("\"");
            }
            ExprKind::Name(n) => self.w(&n.name),
            ExprKind::Field { base, name } => {
                self.postfix_base(*base, no_struct);
                self.w(".");
                self.w(&name.name);
            }
            ExprKind::Call { callee, args } => {
                self.postfix_base(*callee, no_struct);
                self.w("(");
                self.sep(args, ", ", |p, a| p.expr(*a, false));
                self.w(")");
            }
            ExprKind::Index { base, index } => {
                self.postfix_base(*base, no_struct);
                self.w("[");
                self.expr(*index, false);
                self.w("]");
            }
            ExprKind::Try(e) => {
                self.postfix_base(*e, no_struct);
                self.w("?");
            }
            ExprKind::Unary { op, operand } => {
                if self.parens_all {
                    self.w("(");
                }
                self.w(op.as_str());
                let parens = !self.parens_all && self.prec(*operand) < PREC_UNARY;
                self.expr_in_parens(*operand, parens, no_struct);
                if self.parens_all {
                    self.w(")");
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let p = op.prec();
                if self.parens_all {
                    self.w("(");
                }
                let lp = self.prec(*lhs);
                let lhs_parens = !self.parens_all && (lp < p || (op.is_comparison() && lp == p));
                self.expr_in_parens(*lhs, lhs_parens, no_struct);
                self.w(" ");
                self.w(op.as_str());
                self.w(" ");
                let rhs_parens = !self.parens_all && self.prec(*rhs) <= p;
                self.expr_in_parens(*rhs, rhs_parens, no_struct);
                if self.parens_all {
                    self.w(")");
                }
            }
            ExprKind::List(items) => {
                self.w("[");
                self.sep(items, ", ", |p, e| p.expr(*e, false));
                self.w("]");
            }
            ExprKind::Record { path, fields } => {
                if no_struct {
                    self.w("(");
                }
                self.path(path);
                if fields.is_empty() {
                    self.w(" {}");
                } else {
                    self.w(" { ");
                    self.sep(fields, ", ", |p, f| {
                        p.w(&f.name.name);
                        if let Some(v) = f.value {
                            p.w(": ");
                            p.expr(v, false);
                        }
                    });
                    self.w(" }");
                }
                if no_struct {
                    self.w(")");
                }
            }
            ExprKind::If { cond, then, else_ } => {
                self.w("if ");
                self.expr(*cond, true);
                self.w(" ");
                self.block(then);
                if let Some(e) = else_ {
                    self.w(" else ");
                    self.expr(*e, false);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.w("match ");
                self.expr(*scrutinee, true);
                if arms.is_empty() {
                    self.w(" {}");
                    return;
                }
                self.w(" {");
                self.indent += 1;
                for arm in arms {
                    self.newline();
                    self.pat(arm.pat);
                    self.w(" => ");
                    self.expr(arm.body, false);
                    if !m.exprs[arm.body].kind.is_block_like() {
                        self.w(",");
                    }
                }
                self.indent -= 1;
                self.newline();
                self.w("}");
            }
            ExprKind::Block(b) => self.block(b),
            ExprKind::Error => self.w("<error>"),
        }
    }

    fn postfix_base(&mut self, base: ExprId, no_struct: bool) {
        let parens = self.prec(base) < PREC_POSTFIX;
        self.expr_in_parens(base, parens, no_struct);
    }

    fn lit(&mut self, lit: &Lit) {
        match lit {
            Lit::Int(v) => self.w(&v.to_string()),
            Lit::Float(s) => self.w(s),
            Lit::Str(s) => self.string(s),
            Lit::Bool(b) => self.w(if *b { "true" } else { "false" }),
        }
    }

    fn string(&mut self, s: &str) {
        self.w("\"");
        self.w(&escape(s));
        self.w("\"");
    }

    fn pat(&mut self, id: PatId) {
        match &self.m.pats[id].kind {
            PatKind::Wild => self.w("_"),
            PatKind::Name(n) => self.w(&n.name),
            PatKind::Lit(lit) => self.lit(lit),
            PatKind::Variant { path, args } => {
                self.path(path);
                if let Some(args) = args {
                    self.w("(");
                    self.sep(args, ", ", |p, a| p.pat(*a));
                    self.w(")");
                }
            }
            PatKind::Error => self.w("<error>"),
        }
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            '{' => out.push_str("{{"),
            '}' => out.push_str("}}"),
            _ => out.push(c),
        }
    }
    out
}
