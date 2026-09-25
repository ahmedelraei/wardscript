//! AST → canonical source. `print` drops comments; `format` (`ward fmt`) keeps them,
//! the blank lines between statements, and integers as written.

use crate::ast::*;
use crate::{Diagnostic, Severity};

pub fn print(module: &Module) -> String {
    let mut p = Printer::new(module, false);
    p.module();
    p.out
}

/// A `// comment` in the source.
#[derive(Clone, Debug, PartialEq)]
struct Comment {
    start: u32,
    text: String,
    /// Nothing but whitespace before it on its line.
    own_line: bool,
    /// A blank line before it.
    blank_before: bool,
}

fn blank_before(src: &str, pos: usize) -> bool {
    let before = src.get(..pos).unwrap_or("");
    let gap = before.len() - before.trim_end().len();
    before[before.len() - gap..].matches('\n').count() >= 2
}

/// The comments of `src`: `//` in the text between tokens.
fn comments(src: &str) -> Vec<Comment> {
    let mut diags = Vec::new();
    let tokens = crate::lexer::lex(src, 0, &mut diags);
    let mut out = Vec::new();
    let mut prev_end = 0usize;
    for t in tokens {
        let gap_start = prev_end;
        let gap_end = t.span.start as usize;
        prev_end = t.span.end as usize;
        let Some(gap) = src.get(gap_start..gap_end) else {
            continue;
        };
        let mut i = 0;
        while let Some(found) = gap[i..].find("//") {
            let start = gap_start + i + found;
            let end = src[start..].find('\n').map_or(src.len(), |n| start + n);
            let line_start = src[..start].rfind('\n').map_or(0, |n| n + 1);
            out.push(Comment {
                start: start as u32,
                text: src[start..end].trim_end().to_owned(),
                own_line: src[line_start..start].trim().is_empty(),
                blank_before: blank_before(src, start),
            });
            i = end - gap_start;
            if i >= gap.len() {
                break;
            }
        }
    }
    out
}

/// Formats a file, keeping its comments. Refuses a file with syntax errors.
pub fn format(src: &str) -> Result<String, Vec<Diagnostic>> {
    let parse = crate::parse(src);
    let errors: Vec<Diagnostic> = parse
        .diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut p = Printer::new(&parse.module, false);
    p.comments = comments(src);
    p.src = Some(src);
    p.module();
    Ok(p.out)
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
    /// Comments to keep, in order, and how many are printed.
    comments: Vec<Comment>,
    next_comment: usize,
    /// The source, to keep blank lines between statements.
    src: Option<&'m str>,
}

impl<'m> Printer<'m> {
    fn new(m: &'m Module, parens_all: bool) -> Self {
        Printer {
            m,
            out: String::new(),
            indent: 0,
            parens_all,
            comments: Vec::new(),
            next_comment: 0,
            src: None,
        }
    }

    /// Ends the current line of code with `// text`.
    fn trailing(&mut self, text: &str) {
        let line_start = self.out.rfind('\n').map_or(0, |i| i + 1);
        if self.out[line_start..].trim().is_empty() && line_start > 0 {
            // At the start of a new line: the comment belongs on the line before.
            let prev = line_start - 1;
            self.out.insert_str(prev, &format!("  {text}"));
        } else {
            self.out.push_str("  ");
            self.out.push_str(text);
        }
    }

    /// An empty line, when positioned at the start of an indented line.
    fn blank_line(&mut self) {
        let trimmed = self.out.trim_end_matches(' ').len();
        self.out.truncate(trimmed);
        if !self.out.ends_with("\n\n") && !self.out.is_empty() {
            self.newline();
        } else {
            for _ in 0..self.indent {
                self.out.push_str("    ");
            }
        }
    }

    /// Before a node that starts at `pos`, at the start of its line: the comments before
    /// it, and a blank line if the source has one (`blank`).
    fn lead(&mut self, pos: u32, blank: bool) {
        let mut first = true;
        while let Some(c) = self.comments.get(self.next_comment).cloned() {
            if c.start >= pos {
                break;
            }
            self.next_comment += 1;
            if c.own_line {
                // A blank line between comments always stays; before the first one,
                // only where the caller allows it.
                if c.blank_before && (!first || blank) {
                    self.blank_line();
                }
                first = false;
                self.w(&c.text);
                self.newline();
            } else {
                self.trailing(&c.text);
            }
        }
        // A blank line between the comments and the node stays too.
        if blank || !first {
            if let Some(src) = self.src {
                if blank_before(src, pos as usize) {
                    self.blank_line();
                }
            }
        }
    }

    /// At the end of the last line of a list or block that ends at `end`: the comments
    /// left before it.
    fn tail(&mut self, end: u32) {
        while let Some(c) = self.comments.get(self.next_comment).cloned() {
            if c.start >= end {
                break;
            }
            self.next_comment += 1;
            if c.own_line {
                if c.blank_before {
                    self.newline();
                    let trimmed = self.out.trim_end_matches(' ').len();
                    self.out.truncate(trimmed);
                }
                self.newline();
                self.w(&c.text);
            } else {
                self.trailing(&c.text);
            }
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
            self.lead(item.span().start, false);
            self.item(item);
        }
        self.tail(u32::MAX);
        if !self.out.is_empty() {
            self.w("\n");
        }
    }

    fn annotations(&mut self, annotations: &[Annotation]) {
        for a in annotations {
            self.w("@");
            self.w(&a.name.name);
            if !a.args.is_empty() {
                self.w("(");
                self.sep(&a.args, ", ", |p, arg| {
                    p.w(&arg.name.name);
                    if let Some((v, _)) = &arg.value {
                        p.w(" = ");
                        p.string(v);
                    }
                });
                self.w(")");
            }
            self.newline();
        }
    }

    fn item(&mut self, item: &Item) {
        match item {
            Item::Fn(f) => self.annotations(&f.annotations),
            Item::Import(i) => self.annotations(&i.annotations),
            _ => {}
        }
        match item {
            Item::Test(t) => {
                self.w("test ");
                self.string(&t.name);
                self.w(" ");
                self.block(&t.body);
            }
            Item::Import(i) => self.import(i),
            Item::Record(r) => {
                self.vis(r.is_pub);
                self.w("type ");
                self.w(&r.name.name);
                self.generics(&r.generics);
                self.w(" ");
                let end = r.span.end;
                self.braced_lines(
                    &r.fields,
                    |f| f.span.start,
                    end,
                    |p, f| {
                        p.w(&f.name.name);
                        p.w(": ");
                        p.ty(f.ty);
                    },
                );
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
                let end = e.span.end;
                self.braced_lines(
                    &e.variants,
                    |v| v.span.start,
                    end,
                    |p, v| {
                        p.w(&v.name.name);
                        if !v.fields.is_empty() {
                            p.w("(");
                            p.sep(&v.fields, ", ", |p, t| p.ty(*t));
                            p.w(")");
                        }
                    },
                );
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

    /// `{}` when empty, otherwise one comma-terminated entry per line. `start` gives
    /// where each entry starts and `end` where the list ends, for comments.
    fn braced_lines<X>(
        &mut self,
        items: &[X],
        start: impl Fn(&X) -> u32,
        end: u32,
        mut f: impl FnMut(&mut Self, &X),
    ) {
        if items.is_empty() {
            self.w("{}");
            return;
        }
        self.w("{");
        self.indent += 1;
        for x in items {
            self.newline();
            self.lead(start(x), false);
            f(self, x);
            self.w(",");
        }
        self.tail(end);
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
        if f.is_ai {
            self.w("ai ");
        }
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
        if let Some(throws) = f.throws {
            self.w(" throws ");
            self.ty(throws);
        }
        // With clauses, each goes on its own line and the body's `{` starts a new line.
        let multiline =
            f.uses.is_some() || f.budget.is_some() || f.model.is_some() || f.checks.is_some();
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
        if let Some(model) = &f.model {
            self.newline();
            self.w("model {");
            self.sep(&model.entries, ", ", |p, e| {
                p.w(&e.name.name);
                p.w(": ");
                match &e.value {
                    ModelValue::Name(i) => p.w(&i.name),
                    ModelValue::Number(n, _) => p.w(n),
                    ModelValue::Names(names, _) => {
                        p.w("[");
                        p.sep(names, ", ", |p, i| p.w(&i.name));
                        p.w("]");
                    }
                }
            });
            self.w("}");
        }
        if let Some(checks) = &f.checks {
            self.newline();
            self.w("check {");
            self.indent += 1;
            for e in &checks.entries {
                self.newline();
                self.lead(e.span.start, false);
                self.expr(e.cond, false);
                if let Some((reason, _)) = &e.reason {
                    self.w(" => ");
                    self.string(reason);
                }
                self.w(",");
            }
            self.indent -= 1;
            self.newline();
            self.w("}");
        }
        self.indent -= 1;
        if multiline {
            self.newline();
        } else {
            self.w(" ");
        }
        match &f.body {
            FnBody::Block(b) => self.block(b),
            FnBody::Ai { prompt } => {
                self.w("{");
                self.indent += 1;
                self.newline();
                self.expr(*prompt, false);
                self.indent -= 1;
                self.newline();
                self.w("}");
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
        if let Some(cond) = self.m.types[id].refinement {
            self.w(" where ");
            self.expr(cond, true);
        }
    }

    fn block(&mut self, b: &Block) {
        if b.stmts.is_empty() && b.tail.is_none() {
            self.w("{}");
            return;
        }
        self.w("{");
        self.indent += 1;
        for (i, &s) in b.stmts.iter().enumerate() {
            self.newline();
            self.lead(self.m.stmts[s].span.start, i > 0);
            self.stmt(s);
            // `x;` last in a block discards the value; without the `;` it would be the
            // block's value.
            let last = i + 1 == b.stmts.len() && b.tail.is_none();
            if let (true, StmtKind::Expr { semi: true, .. }) = (last, &self.m.stmts[s].kind) {
                self.w(";");
            }
        }
        if let Some(tail) = b.tail {
            self.newline();
            self.lead(self.m.exprs[tail].span.start, !b.stmts.is_empty());
            self.stmt_expr(tail);
        }
        self.tail(b.span.end);
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
            }
            StmtKind::Assign { target, value } => {
                self.stmt_expr(*target);
                self.w(" = ");
                self.expr(*value, false);
            }
            StmtKind::Expr { expr, .. } => self.stmt_expr(*expr),
            StmtKind::Return(value) => {
                self.w("return");
                if let Some(v) = value {
                    self.w(" ");
                    self.expr(*v, false);
                }
            }
            StmtKind::Throw(value) => {
                self.w("throw ");
                self.expr(*value, false);
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
            StmtKind::Assert { cond, message } => {
                self.w("assert ");
                self.expr(*cond, false);
                if let Some((m, _)) = message {
                    self.w(" => ");
                    self.string(m);
                }
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
            ExprKind::If { .. }
            | ExprKind::Match { .. }
            | ExprKind::TryCatch { .. }
            | ExprKind::Block(_) => true,
            ExprKind::Binary { lhs: e, .. }
            | ExprKind::Field { base: e, .. }
            | ExprKind::Index { base: e, .. }
            | ExprKind::Call { callee: e, .. }
            | ExprKind::Propagate(e) => {
                self.prec(*e) >= self.prec(id) && self.starts_block_like(*e)
            }
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
            // An integer as written, `_` separators and all, when formatting.
            ExprKind::Lit(Lit::Int(_)) if self.src.is_some() => {
                let span = self.m.exprs[id].span;
                let text = self
                    .src
                    .and_then(|s| s.get(span.range()))
                    .unwrap_or_default()
                    .to_owned();
                self.w(&text);
            }
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
            ExprKind::Propagate(e) => {
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
                    self.lead(arm.span.start, false);
                    self.pat(arm.pat);
                    self.w(" => ");
                    self.expr(arm.body, false);
                    if !m.exprs[arm.body].kind.is_block_like() {
                        self.w(",");
                    }
                }
                self.tail(m.exprs[id].span.end);
                self.indent -= 1;
                self.newline();
                self.w("}");
            }
            ExprKind::TryCatch { body, err, handler } => {
                self.w("try ");
                self.block(body);
                self.w(" catch ");
                self.w(err.as_ref().map_or("_", |e| e.name.as_str()));
                self.w(" ");
                self.block(handler);
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
