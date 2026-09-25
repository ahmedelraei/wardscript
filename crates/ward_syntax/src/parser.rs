//! Hand-written recursive-descent parser with Pratt-style expression parsing.
//!
//! Errors never abort the whole parse. A failing rule reports once and returns `Bail`;
//! the nearest list, statement or item loop then skips to a synchronisation point.

use crate::ast::*;
use crate::diag::{Diagnostic, codes};
use crate::lexer::{Token, TokenKind as T, lex};
use crate::span::Span;

pub struct Parse {
    pub module: Module,
    /// Sorted by position.
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse(src: &str) -> Parse {
    let mut diags = Vec::new();
    let tokens = lex(src, 0, &mut diags);
    let lex_errors = diags.iter().map(|d| d.span().start).collect();
    let mut p = Parser {
        src,
        tokens,
        pos: 0,
        module: Module::default(),
        diags,
        last_error_at: None,
        in_interpolation: false,
        newlines: false,
        lex_errors,
        in_test: false,
        methods: Vec::new(),
    };
    p.items();
    let mut diagnostics = p.diags;
    diagnostics.sort_by_key(|d| d.span().start);
    Parse {
        module: p.module,
        diagnostics,
    }
}

/// The error has already been reported; the caller should recover.
struct Bail;

type PResult<T> = Result<T, Bail>;

/// Record literals aren't allowed where a `{` would be ambiguous (`if x { ... }`).
#[derive(Clone, Copy)]
struct Restrict {
    no_struct: bool,
}

const ANY: Restrict = Restrict { no_struct: false };
const NO_STRUCT: Restrict = Restrict { no_struct: true };

enum StmtOut {
    Stmt(StmtId),
    Tail(ExprId),
}

enum Piece {
    Text(String),
    /// Span of the expression source between the braces.
    Interp(Span),
}

struct Parser<'s> {
    src: &'s str,
    tokens: Vec<Token>,
    pos: usize,
    module: Module,
    diags: Vec<Diagnostic>,
    /// Token position of the last "expected ..." error, so one bad token reports once.
    last_error_at: Option<usize>,
    in_interpolation: bool,
    /// Whether a line break ends the current statement. True inside blocks and `match`
    /// arms; false inside `( )`, `[ ]`, record literals and at the top level.
    newlines: bool,
    /// Start offsets of characters the lexer rejected.
    lex_errors: Vec<u32>,
    /// Inside a `test` body, where `assert` starts a statement.
    in_test: bool,
    /// Methods of the class just parsed, pushed as items right after it.
    methods: Vec<FnDecl>,
}

impl Parser<'_> {
    fn tok(&self) -> Token {
        self.nth_tok(0)
    }

    fn nth_tok(&self, n: usize) -> Token {
        self.tokens
            .get(self.pos + n)
            .or(self.tokens.last())
            .copied()
            .unwrap_or(Token {
                kind: T::Eof,
                span: Span::default(),
                nl_before: true,
            })
    }

    fn peek(&self) -> T {
        self.tok().kind
    }

    fn at(&self, kind: T) -> bool {
        self.peek() == kind
    }

    fn bump(&mut self) -> Token {
        let t = self.tok();
        if t.kind != T::Eof {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, kind: T) -> Option<Token> {
        self.at(kind).then(|| self.bump())
    }

    fn prev_span(&self) -> Span {
        self.pos
            .checked_sub(1)
            .and_then(|i| self.tokens.get(i))
            .map_or_else(|| self.tok().span, |t| t.span)
    }

    fn text(&self, span: Span) -> &str {
        self.src.get(span.range()).unwrap_or("")
    }

    fn at_item_start(&self) -> bool {
        matches!(
            self.peek(),
            T::Fn | T::Ai | T::Pub | T::Type | T::Enum | T::Import | T::At
        ) || self.at_test_start()
            || self.at_class_start()
    }

    /// `class Name` or `open class`: both words are only keywords here.
    /// `class Name`, `open class`, `abstract class` or `interface Name`: these words
    /// are only keywords here.
    fn at_class_start(&self) -> bool {
        let next_is =
            |w: &str| self.nth_tok(1).kind == T::Ident && self.text(self.nth_tok(1).span) == w;
        (self.at_word("class") || self.at_word("interface")) && self.nth_tok(1).kind == T::Ident
            || (self.at_word("open") || self.at_word("abstract")) && next_is("class")
    }

    fn at_word(&self, word: &str) -> bool {
        self.at(T::Ident) && self.text(self.tok().span) == word
    }

    /// `test "name"`: `test` is only a keyword here.
    fn at_test_start(&self) -> bool {
        self.at(T::Ident)
            && self.text(self.tok().span) == "test"
            && matches!(self.nth_tok(1).kind, T::Str | T::UnterminatedStr)
    }

    fn found(&self, t: Token) -> String {
        match t.kind {
            T::Ident | T::Int | T::Float => format!("`{}`", self.text(t.span)),
            T::Eof if self.in_interpolation => "end of interpolation".into(),
            k => k.describe().into(),
        }
    }

    /// Reports a diagnostic about the current token, at most once per token.
    fn report(&mut self, d: Diagnostic) {
        if self.last_error_at == Some(self.pos) || self.after_lex_error() {
            return;
        }
        self.last_error_at = Some(self.pos);
        self.push(d);
    }

    /// A bad character was dropped just before the current token; an error here is noise.
    fn after_lex_error(&self) -> bool {
        let from = self.prev_span().end;
        let to = self.tok().span.start;
        self.lex_errors
            .iter()
            .any(|&o| from <= o && o < to.max(from + 1))
    }

    fn push(&mut self, mut d: Diagnostic) {
        // Zero-width spans (end of input) render poorly; point at a neighbouring char instead.
        let len = self.src.len() as u32;
        for label in &mut d.labels {
            let s = label.span;
            if s.start == s.end {
                label.span = if s.end < len {
                    Span::new(s.start, s.start + 1)
                } else {
                    Span::new(s.start.saturating_sub(1), s.end)
                };
            }
        }
        self.diags.push(d);
    }

    fn expected(&mut self, what: &str) -> Bail {
        let t = self.tok();
        let found = self.found(t);
        self.report(
            Diagnostic::error(
                codes::EXPECTED_TOKEN,
                format!("expected {what}, found {found}"),
                t.span,
            )
            .with_label(format!("expected {what}")),
        );
        Bail
    }

    fn expect(&mut self, kind: T) -> PResult<Token> {
        match self.eat(kind) {
            Some(t) => Ok(t),
            None => Err(self.expected(kind.describe())),
        }
    }

    fn expect_with_help(&mut self, kind: T, help: &str) -> PResult<Token> {
        if let Some(t) = self.eat(kind) {
            return Ok(t);
        }
        let t = self.tok();
        let found = self.found(t);
        self.report(
            Diagnostic::error(
                codes::EXPECTED_TOKEN,
                format!("expected {}, found {found}", kind.describe()),
                t.span,
            )
            .with_label(format!("expected {}", kind.describe()))
            .with_help(help),
        );
        Err(Bail)
    }

    /// Consumes the closing delimiter matching `open`. `what` describes the alternatives
    /// (e.g. "`,` or `)`") for the error message.
    fn close(&mut self, open: Token, close: T, what: &str) -> PResult<Token> {
        if let Some(t) = self.eat(close) {
            return Ok(t);
        }
        if self.at(T::Eof) || self.at_item_start() {
            self.unclosed(open, close);
            return Err(Bail);
        }
        Err(self.expected(what))
    }

    fn unclosed(&mut self, open: Token, close: T) {
        let prev_unterminated = self
            .pos
            .checked_sub(1)
            .and_then(|i| self.tokens.get(i))
            .is_some_and(|t| t.kind == T::UnterminatedStr);
        if self.at(T::Eof) && prev_unterminated {
            // The string swallowed the closing delimiter; W0002 already explains it.
            return;
        }
        let d = Diagnostic::error(
            codes::UNCLOSED_DELIMITER,
            format!("unclosed {}", open.kind.describe()),
            open.span,
        )
        .with_label("this delimiter is never closed");
        let d = if self.at(T::Eof) {
            d.with_help(format!("add a matching {}", close.describe()))
        } else {
            let found = self.found(self.tok());
            d.with_secondary(
                self.tok().span,
                format!("expected {} before {found}", close.describe()),
            )
        };
        self.report(d);
    }

    /// A line break before the current token, where line breaks end statements.
    fn at_line_break(&self) -> bool {
        self.newlines && self.tok().nl_before
    }

    /// Runs `f` with line breaks significant or not, restoring the previous setting.
    fn with_newlines<X>(&mut self, significant: bool, f: impl FnOnce(&mut Self) -> X) -> X {
        let saved = std::mem::replace(&mut self.newlines, significant);
        let out = f(self);
        self.newlines = saved;
        out
    }

    fn missing_stmt_end(&mut self) {
        let t = self.tok();
        let found = self.found(t);
        let prev = self.prev_span();
        self.report(
            Diagnostic::error(
                codes::MISSING_SEMICOLON,
                format!("expected a line break or `;`, found {found}"),
                prev,
            )
            .with_label("the statement should end after this")
            .with_help("put each statement on its own line, or separate them with `;`"),
        );
    }

    /// Consumes an optional `;`; otherwise the statement must end at a line break or `}`.
    fn stmt_end(&mut self) {
        if self.eat(T::Semi).is_none()
            && !matches!(self.peek(), T::RBrace | T::Eof)
            && !self.tok().nl_before
        {
            self.missing_stmt_end();
        }
    }

    /// Skips tokens until `stop` matches at nesting depth 0. Never skips past an unmatched
    /// closing delimiter, an item keyword, or end of input.
    fn skip_until(&mut self, stop: impl Fn(Token) -> bool) {
        let mut depth = 0u32;
        loop {
            let k = self.peek();
            if k == T::Eof || self.at_item_start() || (depth == 0 && stop(self.tok())) {
                return;
            }
            match k {
                T::LParen | T::LBrace | T::LBracket => depth += 1,
                T::RParen | T::RBrace | T::RBracket => {
                    if depth == 0 {
                        return;
                    }
                    depth -= 1;
                }
                _ => {}
            }
            self.bump();
        }
    }

    fn comma_list<X>(
        &mut self,
        open: T,
        close: T,
        elem: impl FnMut(&mut Self) -> PResult<X>,
    ) -> PResult<(Vec<X>, Span)> {
        self.with_newlines(false, |p| p.comma_list_inner(open, close, elem))
    }

    fn comma_list_inner<X>(
        &mut self,
        open: T,
        close: T,
        mut elem: impl FnMut(&mut Self) -> PResult<X>,
    ) -> PResult<(Vec<X>, Span)> {
        let open_tok = self.expect(open)?;
        let mut items = Vec::new();
        while !self.at(close) && !self.at(T::Eof) {
            match elem(self) {
                Ok(x) => items.push(x),
                Err(Bail) => self.skip_until(|t| t.kind == T::Comma || t.kind == close),
            }
            if self.eat(T::Comma).is_none() {
                break;
            }
        }
        let what = format!("`,` or {}", close.describe());
        let close_tok = match self.close(open_tok, close, &what) {
            Ok(t) => t,
            Err(Bail) if self.at(T::Eof) || self.at_item_start() => return Err(Bail),
            Err(Bail) => {
                // Resync on our own closing delimiter so the caller can carry on.
                self.skip_until(|t| t.kind == close);
                self.eat(close).ok_or(Bail)?
            }
        };
        Ok((items, open_tok.span.to(close_tok.span)))
    }

    fn alloc_expr(&mut self, kind: ExprKind, span: Span) -> ExprId {
        self.module.exprs.alloc(Expr { kind, span })
    }

    fn expr_span(&self, id: ExprId) -> Span {
        self.module.exprs[id].span
    }

    fn ident(&mut self) -> PResult<Ident> {
        let t = self.tok();
        match t.kind {
            T::Ident => {
                self.bump();
                Ok(Ident {
                    name: self.text(t.span).to_owned(),
                    span: t.span,
                })
            }
            k if k.is_keyword() => {
                // Keep going as if it were a name: the rest of the declaration is usually fine.
                self.report(
                    Diagnostic::error(
                        codes::EXPECTED_TOKEN,
                        format!("expected identifier, found keyword {}", k.describe()),
                        t.span,
                    )
                    .with_label("reserved keyword")
                    .with_help("keywords can't be used as names; pick another name"),
                );
                self.bump();
                Ok(Ident {
                    name: self.text(t.span).to_owned(),
                    span: t.span,
                })
            }
            _ => Err(self.expected("identifier")),
        }
    }

    fn path(&mut self) -> PResult<Path> {
        let first = self.ident()?;
        let mut span = first.span;
        let mut segments = vec![first];
        while self.at(T::Dot) && self.nth_tok(1).kind == T::Ident {
            self.bump();
            let seg = self.ident()?;
            span = span.to(seg.span);
            segments.push(seg);
        }
        Ok(Path { segments, span })
    }

    fn items(&mut self) {
        while !self.at(T::Eof) {
            let start = self.pos;
            if self.at_item_start() {
                match self.item() {
                    Ok(mut item) => {
                        let at = self.module.items.len();
                        let methods = std::mem::take(&mut self.methods);
                        if let Item::Class(c) = &mut item {
                            c.methods = (at + 1..at + 1 + methods.len()).collect();
                        }
                        self.module.items.push(item);
                        for mut m in methods {
                            if let Some(info) = &mut m.method {
                                info.class = at;
                            }
                            self.module.items.push(Item::Fn(m));
                        }
                    }
                    Err(Bail) => {
                        self.methods.clear();
                        self.recover_item(start);
                    }
                }
            } else {
                let t = self.tok();
                let found = self.found(t);
                let d = Diagnostic::error(
                    codes::EXPECTED_ITEM,
                    format!("expected an item, found {found}"),
                    t.span,
                )
                .with_label("expected `fn`, `type`, `enum` or `import`");
                let d = match t.kind {
                    T::Let
                    | T::If
                    | T::Match
                    | T::For
                    | T::While
                    | T::Return
                    | T::Throw
                    | T::Try => d.with_help("statements must be inside a function body"),
                    T::Semi => d.with_help("items don't end with `;`"),
                    _ => d,
                };
                self.report(d);
                self.recover_item(start);
            }
        }
    }

    fn recover_item(&mut self, start: usize) {
        if self.pos == start {
            self.bump();
        }
        while !self.at(T::Eof) && !self.at_item_start() {
            self.bump();
        }
    }

    fn item(&mut self) -> PResult<Item> {
        let start = self.tok().span;
        let mut attrs = Vec::new();
        while self.at(T::At) {
            attrs.push(self.annotation()?);
        }
        let mut item = self.item_after_annotations(start)?;
        match &mut item {
            Item::Fn(f) => f.annotations = attrs,
            Item::Import(i) => i.annotations = attrs,
            other => {
                let what = match other {
                    Item::Record(_) | Item::Alias(_) => "types",
                    Item::Class(_) => "classes",
                    Item::Test(_) => "tests",
                    _ => "enums",
                };
                for a in attrs {
                    self.push(
                        Diagnostic::error(
                            codes::MISPLACED_ANNOTATION,
                            format!("annotations aren't allowed on {what}"),
                            a.span,
                        )
                        .with_label("remove this annotation")
                        .with_help("only functions and imports take annotations"),
                    );
                }
            }
        }
        Ok(item)
    }

    /// `@name`, `@name(arg, key = "value")`
    fn annotation(&mut self) -> PResult<Annotation> {
        let at = self.bump();
        // `ident` would take a keyword as the name, swallowing the `fn` after a bare `@`.
        if !self.at(T::Ident) {
            return Err(self.expected("an annotation name, like `allow`"));
        }
        let name = self.ident()?;
        let args = if self.at(T::LParen) {
            self.comma_list(T::LParen, T::RParen, |p| p.annotation_arg())?
                .0
        } else {
            Vec::new()
        };
        Ok(Annotation {
            name,
            args,
            span: at.span.to(self.prev_span()),
        })
    }

    fn annotation_arg(&mut self) -> PResult<AnnotationArg> {
        let mut name = self.ident()?;
        // `send.body`: a dotted path, kept as one name.
        while self.eat(T::Dot).is_some() {
            let part = self.ident()?;
            name = Ident {
                name: format!("{}.{}", name.name, part.name),
                span: name.span.to(part.span),
            };
        }
        let value = match self.eat(T::Eq) {
            Some(_) => {
                if !matches!(self.peek(), T::Str | T::UnterminatedStr) {
                    return Err(self.expected("a string"));
                }
                let t = self.bump();
                Some((self.plain_string(t, "an annotation"), t.span))
            }
            None => None,
        };
        let span = value.as_ref().map_or(name.span, |(_, s)| name.span.to(*s));
        Ok(AnnotationArg { name, value, span })
    }

    fn item_after_annotations(&mut self, start: Span) -> PResult<Item> {
        if self.at_test_start() {
            return self.test_decl(start).map(Item::Test);
        }
        let pub_tok = self.eat(T::Pub);
        let is_pub = pub_tok.is_some();
        if self.at_class_start() {
            return self.class_decl(is_pub, start).map(Item::Class);
        }
        match self.peek() {
            T::Fn => self.fn_decl(is_pub, false, start).map(Item::Fn),
            T::Ai => {
                self.bump();
                if !self.at(T::Fn) {
                    return Err(self.expected("`fn` after `ai`"));
                }
                self.fn_decl(is_pub, true, start).map(Item::Fn)
            }
            T::Type => self.type_decl(is_pub, start),
            T::Enum => self.enum_decl(is_pub, start).map(Item::Enum),
            T::Import => {
                if let Some(p) = pub_tok {
                    self.push(
                        Diagnostic::error(codes::PUB_IMPORT, "imports can't be `pub`", p.span)
                            .with_label("remove this `pub`")
                            .with_help("re-exporting imported names isn't supported"),
                    );
                }
                self.import(start).map(Item::Import)
            }
            _ => {
                let t = self.tok();
                let found = self.found(t);
                self.report(
                    Diagnostic::error(
                        codes::EXPECTED_ITEM,
                        format!(
                            "expected `fn`, `type`, `enum` or `class` after `pub`, found {found}"
                        ),
                        t.span,
                    )
                    .with_label("expected an item"),
                );
                Err(Bail)
            }
        }
    }

    fn generic_params(&mut self) -> PResult<Vec<Ident>> {
        if !self.at(T::Lt) {
            return Ok(Vec::new());
        }
        Ok(self.comma_list(T::Lt, T::Gt, |p| p.ident())?.0)
    }

    fn fn_decl(&mut self, is_pub: bool, is_ai: bool, start: Span) -> PResult<FnDecl> {
        self.bump();
        let name = self.ident()?;
        self.fn_after_name(is_pub, is_ai, start, name, None)
    }

    /// Everything after a function's name. A method gets the implicit `self: Class`.
    fn fn_after_name(
        &mut self,
        is_pub: bool,
        is_ai: bool,
        start: Span,
        name: Ident,
        method: Option<(MethodInfo, &Ident)>,
    ) -> PResult<FnDecl> {
        let generics = self.generic_params()?;
        let (mut params, _) = self.comma_list(T::LParen, T::RParen, |p| p.param())?;
        if let Some((_, class)) = method {
            let path = Path {
                segments: vec![class.clone()],
                span: class.span,
            };
            let ty = self.module.types.alloc(TypeExpr {
                kind: TypeKind::Named {
                    path,
                    args: Vec::new(),
                },
                refinement: None,
                span: class.span,
            });
            params.insert(
                0,
                Param {
                    name: Ident {
                        name: "self".to_owned(),
                        span: name.span,
                    },
                    ty,
                    span: name.span,
                },
            );
        }
        let ret = match self.eat(T::Arrow) {
            Some(_) => Some(self.ty_top()?),
            None => None,
        };
        let throws = match self.eat(T::Throws) {
            Some(kw) => {
                let ty = self.ty()?;
                if is_ai {
                    let span = kw.span.to(self.module.types[ty].span);
                    self.push(
                        Diagnostic::error(
                            codes::AI_FN_THROWS,
                            "`ai fn` can't declare `throws`",
                            span,
                        )
                        .with_label("remove this")
                        .with_help(
                            "a failed model call is a runtime error, not a declared exception",
                        ),
                    );
                }
                Some(ty)
            }
            None => None,
        };

        if let Some((info, _)) = method.filter(|(m, _)| m.is_abstract) {
            if self.at(T::LBrace) {
                let brace = self.tok().span;
                self.push(
                    Diagnostic::error(
                        codes::INVALID_CLASS,
                        format!("abstract method `{}` can't have a body", name.name),
                        brace,
                    )
                    .with_label("remove the body")
                    .with_help("subclasses implement it with `override fn`"),
                );
                self.block()?;
            }
            return Ok(FnDecl {
                annotations: Vec::new(),
                is_pub,
                is_ai,
                name,
                generics,
                params,
                ret,
                throws,
                uses: None,
                budget: None,
                model: None,
                checks: None,
                body: FnBody::Abstract,
                method: Some(info),
                span: start.to(self.prev_span()),
            });
        }
        let mut uses: Option<(Span, Vec<Path>)> = None;
        let mut budget: Option<(Span, Vec<BudgetEntry>)> = None;
        let mut model: Option<ModelClause> = None;
        let mut checks: Option<CheckClause> = None;
        loop {
            match self.peek() {
                T::Ident
                    if self.text(self.tok().span) == "check"
                        && self.nth_tok(1).kind == T::LBrace =>
                {
                    let kw = self.bump();
                    let (entries, _) = self.with_newlines(false, |p| {
                        p.comma_list(T::LBrace, T::RBrace, |p| p.check_entry())
                    })?;
                    match &checks {
                        Some(first) => self.duplicate_clause("check", kw.span, first.span),
                        None => {
                            checks = Some(CheckClause {
                                entries,
                                span: kw.span,
                            });
                        }
                    }
                }
                // `model` is only a keyword here, so it stays usable as a name elsewhere.
                T::Ident
                    if self.text(self.tok().span) == "model"
                        && self.nth_tok(1).kind == T::LBrace =>
                {
                    let kw = self.bump();
                    let (entries, _) =
                        self.comma_list(T::LBrace, T::RBrace, |p| p.model_entry())?;
                    match &model {
                        Some(first) => self.duplicate_clause("model", kw.span, first.span),
                        None => {
                            model = Some(ModelClause {
                                entries,
                                span: kw.span,
                            });
                        }
                    }
                }
                T::Uses => {
                    let kw = self.bump();
                    let (effects, _) = self.comma_list(T::LBrace, T::RBrace, |p| p.effect())?;
                    match &uses {
                        Some((first, _)) => self.duplicate_clause("uses", kw.span, *first),
                        None => uses = Some((kw.span, effects)),
                    }
                }
                T::Budget => {
                    let kw = self.bump();
                    let (entries, _) =
                        self.comma_list(T::LBrace, T::RBrace, |p| p.budget_entry())?;
                    match &budget {
                        Some((first, _)) => self.duplicate_clause("budget", kw.span, *first),
                        None => budget = Some((kw.span, entries)),
                    }
                }
                _ => break,
            }
        }

        let body = if is_ai {
            if ret.is_none() {
                self.push(
                    Diagnostic::error(
                        codes::LLM_FN_WITHOUT_RETURN_TYPE,
                        format!("`ai fn {}` has no return type", name.name),
                        name.span,
                    )
                    .with_label("add a return type here")
                    .with_help(
                        "the model's answer is parsed and validated against the return type, \
                         e.g. `-> Ticket`",
                    ),
                );
            }
            FnBody::Ai {
                prompt: self.ai_body()?,
            }
        } else if self.at(T::LBrace) {
            FnBody::Block(self.block()?)
        } else {
            return Err(self.expected("a function body `{`"));
        };

        Ok(FnDecl {
            annotations: Vec::new(),
            is_pub,
            is_ai,
            name,
            generics,
            params,
            ret,
            throws,
            uses: uses.map(|(_, u)| u),
            budget: budget.map(|(_, b)| b),
            model,
            checks,
            body,
            method: method.map(|(m, _)| m),
            span: start.to(self.prev_span()),
        })
    }

    fn class_decl(&mut self, is_pub: bool, start: Span) -> PResult<ClassDecl> {
        let is_open = self.at_word("open");
        let mut kind = ClassKind::Class;
        if is_open {
            self.bump();
        } else if self.at_word("abstract") {
            self.bump();
            kind = ClassKind::Abstract;
        }
        if self.at_word("interface") {
            kind = ClassKind::Interface;
        }
        self.bump();
        let name = self.ident()?;
        let generics = self.generic_params()?;
        let mut supers = Vec::new();
        if self.eat(T::Colon).is_some() {
            supers.push(self.ty()?);
            while self.eat(T::Comma).is_some() {
                supers.push(self.ty()?);
            }
        }
        let mut fields = Vec::new();
        self.with_newlines(false, |p| -> PResult<()> {
            let open = p.expect(T::LBrace)?;
            while !p.at(T::RBrace) && !p.at(T::Eof) && !p.at_item_start_outside_class() {
                let member = p.tok().span;
                let member_pos = p.pos;
                if let Err(Bail) = p.member(&name, kind, member, &mut fields) {
                    // Members start on a new line, so a name there starts the next one.
                    p.skip_until(|t| {
                        t.kind == T::RBrace
                            || p_member_start(t.kind)
                            || (t.nl_before && t.kind == T::Ident)
                    });
                    if p.pos == member_pos {
                        p.bump();
                    }
                }
            }
            p.close(open, T::RBrace, "a field, `init` or a method")
                .map(|_| ())
        })?;
        Ok(ClassDecl {
            is_pub,
            is_open,
            kind,
            name,
            generics,
            supers,
            fields,
            methods: Vec::new(),
            span: start.to(self.prev_span()),
        })
    }

    /// Items other than those that also start class members (`fn`, `pub`, `ai`, `@`).
    fn at_item_start_outside_class(&self) -> bool {
        matches!(self.peek(), T::Type | T::Enum | T::Import)
            || self.at_test_start()
            || self.at_class_start()
    }

    /// A field (`name: Type`), `init(...) { ... }` or a method, with its modifiers.
    fn member(
        &mut self,
        class: &Ident,
        kind: ClassKind,
        start: Span,
        fields: &mut Vec<ClassField>,
    ) -> PResult<()> {
        let mut annotations = Vec::new();
        while self.at(T::At) {
            annotations.push(self.annotation()?);
        }
        // An interface's methods are all public.
        let is_pub = self.eat(T::Pub).is_some() || kind == ClassKind::Interface;
        let mut is_open = false;
        let mut is_override = false;
        let mut is_abstract = kind == ClassKind::Interface;
        loop {
            let next_starts_fn = matches!(self.nth_tok(1).kind, T::Fn | T::Ai | T::Ident);
            if self.at_word("open") && next_starts_fn && !is_open {
                is_open = true;
            } else if self.at_word("override") && next_starts_fn && !is_override {
                is_override = true;
            } else if self.at_word("abstract") && next_starts_fn && !is_abstract {
                is_abstract = true;
            } else {
                break;
            }
            self.bump();
        }
        let info = MethodInfo {
            class: 0,
            is_init: false,
            is_open,
            is_override,
            is_abstract,
        };
        let method = if self.at_word("init") && self.nth_tok(1).kind == T::LParen {
            let name = self.ident()?;
            let info = MethodInfo {
                is_init: true,
                ..info
            };
            let f = self.fn_after_name(is_pub, false, start, name, Some((info, class)))?;
            if let Some(ret) = f.ret {
                self.push(
                    Diagnostic::error(
                        codes::INVALID_CLASS,
                        "`init` can't declare a return type",
                        self.module.types[ret].span,
                    )
                    .with_label("remove this")
                    .with_help("`init` sets up `self`; calling the class returns the new object"),
                );
            }
            Some(f)
        } else if self.at(T::Fn) || self.at(T::Ai) {
            let is_ai = self.eat(T::Ai).is_some();
            if !self.at(T::Fn) {
                return Err(self.expected("`fn` after `ai`"));
            }
            self.bump();
            let name = self.ident()?;
            Some(self.fn_after_name(is_pub, is_ai, start, name, Some((info, class)))?)
        } else {
            None
        };
        if let Some(mut f) = method {
            f.annotations = annotations;
            self.methods.push(f);
            return Ok(());
        }
        if is_open || is_override || (is_abstract && kind != ClassKind::Interface) {
            return Err(self.expected("`fn` after the modifier"));
        }
        if !self.at(T::Ident) {
            return Err(self.expected("a field, `init` or a method"));
        }
        let name = self.ident()?;
        self.expect_with_help(T::Colon, "fields need a type: `name: Type`")?;
        let ty = self.ty_top()?;
        for a in annotations {
            self.push(
                Diagnostic::error(
                    codes::MISPLACED_ANNOTATION,
                    "annotations aren't allowed on fields",
                    a.span,
                )
                .with_label("remove this annotation")
                .with_help("only functions, methods and imports take annotations"),
            );
        }
        let span = start.to(self.module.types[ty].span);
        fields.push(ClassField {
            is_pub,
            name,
            ty,
            span,
        });
        // Fields may be separated by `,` or `;` as well as line breaks.
        if self.eat(T::Comma).is_none() {
            self.eat(T::Semi);
        }
        Ok(())
    }

    fn test_decl(&mut self, start: Span) -> PResult<TestDecl> {
        self.bump();
        let t = self.bump();
        let name = self.plain_string(t, "a test name");
        let saved = std::mem::replace(&mut self.in_test, true);
        let body = self.block();
        self.in_test = saved;
        Ok(TestDecl {
            name,
            name_span: t.span,
            body: body?,
            span: start.to(self.prev_span()),
        })
    }

    fn duplicate_clause(&mut self, clause: &str, second: Span, first: Span) {
        self.push(
            Diagnostic::error(
                codes::DUPLICATE_CLAUSE,
                format!("duplicate `{clause}` clause"),
                second,
            )
            .with_label("second clause here")
            .with_secondary(first, "first declared here")
            .with_help(format!("merge them into a single `{clause} {{ ... }}`")),
        );
    }

    fn param(&mut self) -> PResult<Param> {
        let name = self.ident()?;
        self.expect_with_help(T::Colon, "parameters need a type: `name: Type`")?;
        let ty = self.ty_top()?;
        let span = name.span.to(self.module.types[ty].span);
        Ok(Param { name, ty, span })
    }

    /// An effect name such as `llm` or `mail.send`.
    fn effect(&mut self) -> PResult<Path> {
        self.path()
    }

    fn budget_entry(&mut self) -> PResult<BudgetEntry> {
        let name = self.ident()?;
        self.expect_with_help(
            T::Colon,
            "budget entries are written `name: value`, e.g. `tokens: 2000`",
        )?;
        let value = self.expr()?;
        Ok(BudgetEntry { name, value })
    }

    fn check_entry(&mut self) -> PResult<CheckEntry> {
        let cond = self.expr()?;
        let reason = match self.eat(T::FatArrow) {
            Some(_) => {
                if !matches!(self.peek(), T::Str | T::UnterminatedStr) {
                    return Err(self.expected("a reason string, like `\"keep it short\"`"));
                }
                let t = self.bump();
                Some((self.plain_string(t, "a check's reason"), t.span))
            }
            None => None,
        };
        let start = self.module.exprs[cond].span;
        let span = reason.as_ref().map_or(start, |(_, s)| start.to(*s));
        Ok(CheckEntry { cond, reason, span })
    }

    fn model_entry(&mut self) -> PResult<ModelEntry> {
        let name = self.ident()?;
        self.expect_with_help(
            T::Colon,
            "model entries are written `name: value`, e.g. `primary: fast`",
        )?;
        let value = match self.peek() {
            T::Ident => ModelValue::Name(self.ident()?),
            T::Int | T::Float => {
                let t = self.bump();
                ModelValue::Number(self.text(t.span).to_owned(), t.span)
            }
            T::LBracket => {
                let (names, span) = self.comma_list(T::LBracket, T::RBracket, |p| p.ident())?;
                ModelValue::Names(names, span)
            }
            _ => {
                return Err(self.expected(
                    "a model alias like `fast`, a list like `[fast, smart]`, or a number",
                ));
            }
        };
        let span = name.span.to(value.span());
        Ok(ModelEntry { name, value, span })
    }

    /// `{ "prompt" }`: exactly one string literal. A malformed body still yields a function,
    /// with an error prompt, so the rest of the file checks normally.
    fn ai_body(&mut self) -> PResult<ExprId> {
        let open = self.expect(T::LBrace)?;
        if matches!(self.peek(), T::Str | T::UnterminatedStr) {
            let prompt = self.string_expr();
            if self.eat(T::RBrace).is_some() {
                return Ok(prompt);
            }
            if self.at(T::Eof) || self.at_item_start() {
                self.unclosed(open, T::RBrace);
                return Ok(prompt);
            }
            self.not_a_prompt("the prompt must be the only thing in an `ai fn` body");
            self.skip_to_close(T::RBrace);
            return Ok(prompt);
        }
        self.not_a_prompt("the body of an `ai fn` is a prompt string");
        self.skip_to_close(T::RBrace);
        let span = open.span.to(self.prev_span());
        Ok(self.alloc_expr(ExprKind::Error, span))
    }

    fn not_a_prompt(&mut self, label: &str) {
        let t = self.tok();
        let found = self.found(t);
        self.report(
            Diagnostic::error(
                codes::LLM_PROMPT_NOT_STRING,
                format!("expected a prompt string in `ai fn`, found {found}"),
                t.span,
            )
            .with_label(label.to_owned())
            .with_help(
                "write the prompt as a string literal, using `{...}` to insert parameters: \
                 `{ \"Summarize: {text}\" }`",
            ),
        );
    }

    /// Skips to and consumes the `close` matching an already-consumed opener.
    fn skip_to_close(&mut self, close: T) {
        self.skip_until(|t| t.kind == close);
        self.eat(close);
    }

    fn type_decl(&mut self, is_pub: bool, start: Span) -> PResult<Item> {
        self.bump();
        let name = self.ident()?;
        let generics = self.generic_params()?;
        if self.at(T::LBrace) {
            let (fields, _) = self.comma_list(T::LBrace, T::RBrace, |p| p.field_decl())?;
            Ok(Item::Record(RecordDecl {
                is_pub,
                name,
                generics,
                fields,
                span: start.to(self.prev_span()),
            }))
        } else if self.eat(T::Eq).is_some() {
            let ty = self.ty_top()?;
            Ok(Item::Alias(AliasDecl {
                is_pub,
                name,
                generics,
                ty,
                span: start.to(self.prev_span()),
            }))
        } else {
            Err(self.expected("`{` or `=`"))
        }
    }

    fn field_decl(&mut self) -> PResult<FieldDecl> {
        let name = self.ident()?;
        self.expect_with_help(T::Colon, "fields need a type: `name: Type`")?;
        let ty = self.ty_top()?;
        let span = name.span.to(self.module.types[ty].span);
        Ok(FieldDecl { name, ty, span })
    }

    fn enum_decl(&mut self, is_pub: bool, start: Span) -> PResult<EnumDecl> {
        self.bump();
        let name = self.ident()?;
        let generics = self.generic_params()?;
        let (variants, _) = self.comma_list(T::LBrace, T::RBrace, |p| {
            let name = p.ident()?;
            let (fields, span) = if p.at(T::LParen) {
                let (fields, s) = p.comma_list(T::LParen, T::RParen, |p| p.ty_top())?;
                (fields, name.span.to(s))
            } else {
                (Vec::new(), name.span)
            };
            Ok(Variant { name, fields, span })
        })?;
        Ok(EnumDecl {
            is_pub,
            name,
            generics,
            variants,
            span: start.to(self.prev_span()),
        })
    }

    fn import(&mut self, start: Span) -> PResult<Import> {
        self.bump();
        let tool_import =
            self.at(T::Ident) && matches!(self.nth_tok(1).kind, T::Str | T::UnterminatedStr);
        let (kind, alias) = if tool_import {
            let provider = self.ident()?;
            let str_tok = self.bump();
            let source = self.plain_string(str_tok, "an import source");
            self.expect_with_help(
                T::As,
                "tool imports need a name: `import mcp \"gmail\" as mail`",
            )?;
            let alias = self.ident()?;
            let kind = ImportKind::Tool {
                provider,
                source,
                source_span: str_tok.span,
            };
            (kind, Some(alias))
        } else {
            let path = self.path()?;
            let alias = match self.eat(T::As) {
                Some(_) => Some(self.ident()?),
                None => None,
            };
            (ImportKind::Module(path), alias)
        };
        Ok(Import {
            annotations: Vec::new(),
            kind,
            alias,
            span: start.to(self.prev_span()),
        })
    }

    fn ty(&mut self) -> PResult<TypeId> {
        if !self.at(T::Ident) {
            let t = self.tok();
            let found = self.found(t);
            self.report(
                Diagnostic::error(
                    codes::EXPECTED_TYPE,
                    format!("expected a type, found {found}"),
                    t.span,
                )
                .with_label("expected a type such as `String` or `List<Int>`"),
            );
            return Err(Bail);
        }
        let path = self.path()?;
        let mut span = path.span;
        let args = if self.at(T::Lt) {
            let (args, s) = self.comma_list(T::Lt, T::Gt, |p| p.ty())?;
            span = span.to(s);
            args
        } else {
            Vec::new()
        };
        Ok(self.module.types.alloc(TypeExpr {
            kind: TypeKind::Named { path, args },
            refinement: None,
            span,
        }))
    }

    /// A type that may be refined: `String where it.len() < 200`. Not inside type
    /// arguments, where `>` would be ambiguous; an alias names a refined type there.
    fn ty_top(&mut self) -> PResult<TypeId> {
        let ty = self.ty()?;
        if self.at(T::Ident) && self.text(self.tok().span) == "where" {
            self.bump();
            let cond = self.expr_bp(0, NO_STRUCT)?;
            let end = self.module.exprs[cond].span;
            let t = &mut self.module.types[ty];
            t.refinement = Some(cond);
            t.span = t.span.to(end);
        }
        Ok(ty)
    }

    fn block(&mut self) -> PResult<Block> {
        self.with_newlines(true, |p| p.block_inner())
    }

    fn block_inner(&mut self) -> PResult<Block> {
        let open = self.expect(T::LBrace)?;
        let mut stmts = Vec::new();
        let mut tail = None;
        let end = loop {
            if let Some(close) = self.eat(T::RBrace) {
                break close.span;
            }
            if self.at(T::Eof) || self.at_item_start() {
                self.unclosed(open, T::RBrace);
                break self.prev_span();
            }
            let before = self.pos;
            match self.stmt() {
                Ok(StmtOut::Stmt(s)) => stmts.push(s),
                Ok(StmtOut::Tail(e)) => tail = Some(e),
                Err(Bail) => {
                    let stmt_start = |t: Token| {
                        matches!(
                            t.kind,
                            T::Semi | T::Let | T::Return | T::Throw | T::For | T::While
                        ) || t.nl_before
                    };
                    // Don't resync on the failing token itself when it starts a line.
                    if self.pos == before && self.tok().nl_before {
                        self.bump();
                    }
                    self.skip_until(stmt_start);
                    self.eat(T::Semi);
                    if self.pos == before
                        && !matches!(self.peek(), T::RBrace | T::Eof)
                        && !self.at_item_start()
                    {
                        self.bump();
                    }
                }
            }
        };
        Ok(Block {
            stmts,
            tail,
            span: open.span.to(end),
        })
    }

    fn stmt(&mut self) -> PResult<StmtOut> {
        let start = self.tok().span;
        let kind = match self.peek() {
            T::Ident if self.in_test && self.text(self.tok().span) == "assert" => {
                self.bump();
                let cond = self.expr()?;
                let message = match self.eat(T::FatArrow) {
                    Some(_) => {
                        if !matches!(self.peek(), T::Str | T::UnterminatedStr) {
                            return Err(self.expected("a message string"));
                        }
                        let t = self.bump();
                        Some((self.plain_string(t, "an assertion message"), t.span))
                    }
                    None => None,
                };
                self.stmt_end();
                StmtKind::Assert { cond, message }
            }
            T::Let => {
                self.bump();
                let name = self.ident()?;
                let ty = match self.eat(T::Colon) {
                    Some(_) => Some(self.ty_top()?),
                    None => None,
                };
                self.expect(T::Eq)?;
                let init = self.expr()?;
                self.stmt_end();
                StmtKind::Let { name, ty, init }
            }
            T::Return => {
                self.bump();
                let value = if matches!(self.peek(), T::Semi | T::RBrace) || self.at_line_break() {
                    None
                } else {
                    Some(self.expr()?)
                };
                self.stmt_end();
                StmtKind::Return(value)
            }
            T::Throw => {
                self.bump();
                let value = self.expr()?;
                self.stmt_end();
                StmtKind::Throw(value)
            }
            T::For => {
                self.bump();
                let var = self.ident()?;
                self.expect(T::In)?;
                let iter = self.expr_bp(0, NO_STRUCT)?;
                let body = self.block()?;
                StmtKind::For { var, iter, body }
            }
            T::While => {
                self.bump();
                let cond = self.expr_bp(0, NO_STRUCT)?;
                let body = self.block()?;
                StmtKind::While { cond, body }
            }
            T::If | T::Match | T::Try | T::LBrace => {
                // Like Rust: a statement-position `if`/`match`/block ends at its `}`.
                let expr = self.block_like()?;
                if self.at(T::RBrace) {
                    return Ok(StmtOut::Tail(expr));
                }
                let semi = self.eat(T::Semi).is_some();
                StmtKind::Expr { expr, semi }
            }
            _ => {
                let expr = self.expr()?;
                if self.eat(T::Eq).is_some() {
                    let value = self.expr()?;
                    self.check_assign_target(expr);
                    self.stmt_end();
                    StmtKind::Assign {
                        target: expr,
                        value,
                    }
                } else if self.eat(T::Semi).is_some() {
                    StmtKind::Expr { expr, semi: true }
                } else if self.at(T::RBrace) || self.at(T::Eof) || self.at_item_start() {
                    return Ok(StmtOut::Tail(expr));
                } else {
                    if !self.tok().nl_before {
                        self.missing_stmt_end();
                    }
                    StmtKind::Expr { expr, semi: false }
                }
            }
        };
        let span = start.to(self.prev_span());
        Ok(StmtOut::Stmt(self.module.stmts.alloc(Stmt { kind, span })))
    }

    fn check_assign_target(&mut self, target: ExprId) {
        let e = &self.module.exprs[target];
        if matches!(
            e.kind,
            ExprKind::Name(_) | ExprKind::Field { .. } | ExprKind::Index { .. } | ExprKind::Error
        ) {
            return;
        }
        let span = e.span;
        self.push(
            Diagnostic::error(
                codes::INVALID_ASSIGN_TARGET,
                "invalid assignment target",
                span,
            )
            .with_label("can't assign to this")
            .with_help("only variables, fields and list elements can be assigned to"),
        );
    }

    fn expr(&mut self) -> PResult<ExprId> {
        self.expr_bp(0, ANY)
    }

    fn expr_bp(&mut self, min_prec: u8, r: Restrict) -> PResult<ExprId> {
        let mut lhs = self.unary(r)?;
        let mut prev_cmp: Option<Span> = None;
        while let Some(op) = binop(self.peek()) {
            if self.at_line_break() {
                break;
            }
            let prec = op.prec();
            if prec < min_prec {
                break;
            }
            let op_tok = self.bump();
            if op.is_comparison() {
                if let Some(first) = prev_cmp {
                    self.push(
                        Diagnostic::error(
                            codes::CHAINED_COMPARISON,
                            "comparison operators can't be chained",
                            op_tok.span,
                        )
                        .with_label("second comparison")
                        .with_secondary(first, "first comparison")
                        .with_help("split it with `&&`, e.g. `a < b && b < c`"),
                    );
                }
                prev_cmp = Some(op_tok.span);
            } else {
                prev_cmp = None;
            }
            let rhs = self.expr_bp(prec + 1, r)?;
            let span = self.expr_span(lhs).to(self.expr_span(rhs));
            lhs = self.alloc_expr(ExprKind::Binary { op, lhs, rhs }, span);
        }
        Ok(lhs)
    }

    fn unary(&mut self, r: Restrict) -> PResult<ExprId> {
        let op = match self.peek() {
            T::Minus => UnOp::Neg,
            T::Bang => UnOp::Not,
            _ => return self.postfix(r),
        };
        let t = self.bump();
        let operand = self.unary(r)?;
        let span = t.span.to(self.expr_span(operand));
        Ok(self.alloc_expr(ExprKind::Unary { op, operand }, span))
    }

    fn postfix(&mut self, r: Restrict) -> PResult<ExprId> {
        let mut e = self.primary(r)?;
        loop {
            let start = self.expr_span(e);
            // `.method()` may continue on the next line; nothing else does.
            if self.at_line_break() && !self.at(T::Dot) {
                return Ok(e);
            }
            let (kind, end) = match self.peek() {
                T::Dot => {
                    self.bump();
                    let name = self.ident()?;
                    let end = name.span;
                    (ExprKind::Field { base: e, name }, end)
                }
                T::LParen => {
                    let (args, s) = self.comma_list(T::LParen, T::RParen, |p| p.expr())?;
                    (ExprKind::Call { callee: e, args }, s)
                }
                T::LBracket => {
                    let open = self.bump();
                    let index = self.with_newlines(false, |p| p.expr())?;
                    let close = self.close(open, T::RBracket, "`]`")?;
                    (ExprKind::Index { base: e, index }, close.span)
                }
                T::Question => (ExprKind::Propagate(e), self.bump().span),
                _ => return Ok(e),
            };
            e = self.alloc_expr(kind, start.to(end));
        }
    }

    fn primary(&mut self, r: Restrict) -> PResult<ExprId> {
        let t = self.tok();
        match t.kind {
            T::Int => {
                self.bump();
                let kind = match self.int_value(t) {
                    Some(v) => ExprKind::Lit(Lit::Int(v)),
                    None => ExprKind::Error,
                };
                Ok(self.alloc_expr(kind, t.span))
            }
            T::Float => {
                self.bump();
                let text = self.text(t.span).replace('_', "");
                Ok(self.alloc_expr(ExprKind::Lit(Lit::Float(text)), t.span))
            }
            T::Str | T::UnterminatedStr => Ok(self.string_expr()),
            T::True | T::False => {
                self.bump();
                Ok(self.alloc_expr(ExprKind::Lit(Lit::Bool(t.kind == T::True)), t.span))
            }
            T::Ident => {
                if !r.no_struct && self.at_record_path() {
                    let path = self.path()?;
                    return self.record_lit(path);
                }
                let name = self.ident()?;
                let span = name.span;
                Ok(self.alloc_expr(ExprKind::Name(name), span))
            }
            T::LParen => {
                let open = self.bump();
                let e = self.with_newlines(false, |p| p.expr())?;
                self.close(open, T::RParen, "`)`")?;
                Ok(e)
            }
            T::LBracket => {
                let (items, span) = self.comma_list(T::LBracket, T::RBracket, |p| p.expr())?;
                Ok(self.alloc_expr(ExprKind::List(items), span))
            }
            T::If | T::Match | T::Try | T::LBrace => self.block_like(),
            _ => {
                let found = self.found(t);
                self.report(
                    Diagnostic::error(
                        codes::EXPECTED_EXPR,
                        format!("expected an expression, found {found}"),
                        t.span,
                    )
                    .with_label("expected an expression"),
                );
                Err(Bail)
            }
        }
    }

    fn int_value(&mut self, t: Token) -> Option<i64> {
        let text = self.text(t.span).replace('_', "");
        let value = text.parse().ok();
        if value.is_none() {
            self.push(
                Diagnostic::error(
                    codes::INVALID_NUMBER,
                    "integer literal is too large",
                    t.span,
                )
                .with_label("doesn't fit in a 64-bit signed integer")
                .with_help(format!("the largest integer is {}", i64::MAX)),
            );
        }
        value
    }

    /// `Name {` or `module.Name {` ahead.
    fn at_record_path(&self) -> bool {
        let mut n = 1;
        while self.nth_tok(n).kind == T::Dot && self.nth_tok(n + 1).kind == T::Ident {
            n += 2;
        }
        let brace = self.nth_tok(n);
        brace.kind == T::LBrace && !(self.newlines && brace.nl_before)
    }

    fn record_lit(&mut self, path: Path) -> PResult<ExprId> {
        let (fields, s) = self.comma_list(T::LBrace, T::RBrace, |p| {
            let name = p.ident()?;
            let value = match p.eat(T::Colon) {
                Some(_) => Some(p.expr()?),
                None => None,
            };
            let span = value.map_or(name.span, |v| name.span.to(p.expr_span(v)));
            Ok(FieldInit { name, value, span })
        })?;
        let span = path.span.to(s);
        Ok(self.alloc_expr(ExprKind::Record { path, fields }, span))
    }

    fn block_like(&mut self) -> PResult<ExprId> {
        match self.peek() {
            T::If => self.if_expr(),
            T::Try => self.try_catch(),
            T::Match => self.match_expr(),
            _ => {
                let block = self.block()?;
                let span = block.span;
                Ok(self.alloc_expr(ExprKind::Block(block), span))
            }
        }
    }

    fn try_catch(&mut self) -> PResult<ExprId> {
        let kw = self.bump();
        let body = self.block()?;
        self.expect_with_help(
            T::Catch,
            "a `try` block needs a handler: `try { ... } catch err { ... }`",
        )?;
        let err = if self.eat(T::Underscore).is_some() {
            None
        } else {
            Some(self.ident()?)
        };
        let handler = self.block()?;
        let span = kw.span.to(self.prev_span());
        Ok(self.alloc_expr(ExprKind::TryCatch { body, err, handler }, span))
    }

    fn if_expr(&mut self) -> PResult<ExprId> {
        let kw = self.bump();
        let cond = self.expr_bp(0, NO_STRUCT)?;
        let then = self.block()?;
        let else_ = if self.eat(T::Else).is_some() {
            if self.at(T::If) {
                Some(self.if_expr()?)
            } else {
                let block = self.block()?;
                let span = block.span;
                Some(self.alloc_expr(ExprKind::Block(block), span))
            }
        } else {
            None
        };
        let span = kw.span.to(self.prev_span());
        Ok(self.alloc_expr(ExprKind::If { cond, then, else_ }, span))
    }

    fn match_expr(&mut self) -> PResult<ExprId> {
        self.with_newlines(true, |p| p.match_inner())
    }

    fn match_inner(&mut self) -> PResult<ExprId> {
        let kw = self.bump();
        let scrutinee = self.expr_bp(0, NO_STRUCT)?;
        let open = self.expect(T::LBrace)?;
        let mut arms = Vec::new();
        loop {
            if self.eat(T::RBrace).is_some() {
                break;
            }
            if self.at(T::Eof) || self.at_item_start() {
                self.unclosed(open, T::RBrace);
                break;
            }
            let before = self.pos;
            match self.arm() {
                Ok(arm) => arms.push(arm),
                Err(Bail) => {
                    // Don't resync on the failing token itself when it starts a line.
                    if self.pos == before && self.tok().nl_before && !self.at(T::RBrace) {
                        self.bump();
                    }
                    self.skip_until(|t| t.kind == T::Comma || t.nl_before);
                    self.eat(T::Comma);
                    if self.pos == before && !matches!(self.peek(), T::RBrace | T::Eof) {
                        self.bump();
                    }
                }
            }
        }
        let span = kw.span.to(self.prev_span());
        Ok(self.alloc_expr(ExprKind::Match { scrutinee, arms }, span))
    }

    fn arm(&mut self) -> PResult<Arm> {
        let pat = self.pat()?;
        self.expect(T::FatArrow)?;
        let body = self.expr()?;
        let block_like = self.module.exprs[body].kind.is_block_like();
        if self.eat(T::Comma).is_none()
            && !self.at(T::RBrace)
            && !block_like
            && !self.tok().nl_before
        {
            self.expected("`,`, a line break or `}`");
        }
        let span = self.module.pats[pat].span.to(self.expr_span(body));
        Ok(Arm { pat, body, span })
    }

    fn pat(&mut self) -> PResult<PatId> {
        let t = self.tok();
        let (kind, span) = match t.kind {
            T::Underscore => {
                self.bump();
                (PatKind::Wild, t.span)
            }
            T::Int | T::Float | T::Str | T::UnterminatedStr | T::True | T::False => {
                (self.lit_pat(false), t.span)
            }
            T::Minus if matches!(self.nth_tok(1).kind, T::Int | T::Float) => {
                self.bump();
                let span = t.span.to(self.tok().span);
                (self.lit_pat(true), span)
            }
            T::Ident => {
                let path = self.path()?;
                if self.at(T::LParen) {
                    let (args, s) = self.comma_list(T::LParen, T::RParen, |p| p.pat())?;
                    let span = path.span.to(s);
                    (
                        PatKind::Variant {
                            path,
                            args: Some(args),
                        },
                        span,
                    )
                } else if path.segments.len() == 1 {
                    let span = path.span;
                    let name = path.segments.into_iter().next();
                    (name.map_or(PatKind::Error, PatKind::Name), span)
                } else {
                    let span = path.span;
                    (PatKind::Variant { path, args: None }, span)
                }
            }
            _ => {
                let found = self.found(t);
                self.report(
                    Diagnostic::error(
                        codes::EXPECTED_PATTERN,
                        format!("expected a pattern, found {found}"),
                        t.span,
                    )
                    .with_label("expected a pattern")
                    .with_help("patterns are `_`, a name, a literal, or a variant like `Some(x)`"),
                );
                return Err(Bail);
            }
        };
        Ok(self.module.pats.alloc(Pat { kind, span }))
    }

    fn lit_pat(&mut self, negative: bool) -> PatKind {
        let t = self.bump();
        match t.kind {
            T::Int => match self.int_value(t) {
                Some(v) => PatKind::Lit(Lit::Int(if negative { -v } else { v })),
                None => PatKind::Error,
            },
            T::Float => {
                let text = self.text(t.span).replace('_', "");
                PatKind::Lit(Lit::Float(if negative { format!("-{text}") } else { text }))
            }
            T::True | T::False => PatKind::Lit(Lit::Bool(t.kind == T::True)),
            _ => PatKind::Lit(Lit::Str(self.plain_string(t, "a pattern"))),
        }
    }

    /// Parses the string token at the cursor into a literal or a template.
    fn string_expr(&mut self) -> ExprId {
        let t = self.bump();
        if t.kind == T::UnterminatedStr {
            // Its extent is a guess, so don't report braces or escapes inside it.
            let text = self
                .text(Span::new(t.span.start + 1, t.span.end))
                .to_owned();
            return self.alloc_expr(ExprKind::Lit(Lit::Str(text)), t.span);
        }
        let pieces = self.decode_string(t);
        if !pieces.iter().any(|p| matches!(p, Piece::Interp(_))) {
            let text = pieces
                .into_iter()
                .map(|p| match p {
                    Piece::Text(s) => s,
                    Piece::Interp(_) => String::new(),
                })
                .collect();
            return self.alloc_expr(ExprKind::Lit(Lit::Str(text)), t.span);
        }
        let parts = pieces
            .into_iter()
            .map(|p| match p {
                Piece::Text(s) => TemplatePart::Lit(s),
                Piece::Interp(span) => TemplatePart::Expr(self.interpolation(span)),
            })
            .collect();
        self.alloc_expr(ExprKind::Template(parts), t.span)
    }

    /// A string where interpolation isn't allowed (`where_` names the context).
    fn plain_string(&mut self, t: Token, where_: &str) -> String {
        if t.kind == T::UnterminatedStr {
            return self
                .text(Span::new(t.span.start + 1, t.span.end))
                .to_owned();
        }
        let mut out = String::new();
        for piece in self.decode_string(t) {
            match piece {
                Piece::Text(s) => out.push_str(&s),
                Piece::Interp(span) => self.push(
                    Diagnostic::error(
                        codes::INTERPOLATION_NOT_ALLOWED,
                        format!("string interpolation isn't allowed in {where_}"),
                        Span::new(span.start - 1, span.end + 1),
                    )
                    .with_label("interpolation here")
                    .with_help("write `{{` for a literal brace"),
                ),
            }
        }
        out
    }

    fn decode_string(&mut self, t: Token) -> Vec<Piece> {
        let start = t.span.start as usize + 1;
        let end = if t.kind == T::Str {
            t.span.end as usize - 1
        } else {
            t.span.end as usize
        };
        let body = self.src.get(start..end).unwrap_or("");
        let at = |i: usize| (start + i) as u32;

        let mut pieces = Vec::new();
        let mut buf = String::new();
        let mut chars = body.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => {
                    let Some((_, e)) = chars.next() else { break };
                    match unescape(e) {
                        Some(ch) => buf.push(ch),
                        None => {
                            let span = Span::new(at(i), at(i) + 1 + e.len_utf8() as u32);
                            let help = if matches!(e, '{' | '}') {
                                format!("braces are escaped by doubling: write `{e}{e}`")
                            } else {
                                r#"valid escapes are \n \r \t \0 \\ \""#.to_owned()
                            };
                            self.push(
                                Diagnostic::error(
                                    codes::INVALID_ESCAPE,
                                    format!("unknown escape `\\{e}`"),
                                    span,
                                )
                                .with_label("unknown escape")
                                .with_help(help),
                            );
                            buf.push(e);
                        }
                    }
                }
                '{' if chars.peek().is_some_and(|&(_, n)| n == '{') => {
                    chars.next();
                    buf.push('{');
                }
                '}' if chars.peek().is_some_and(|&(_, n)| n == '}') => {
                    chars.next();
                    buf.push('}');
                }
                '{' => {
                    let Some(close) = matching_brace(&body[i + 1..]).map(|j| i + 1 + j) else {
                        self.push(
                            Diagnostic::error(
                                codes::BAD_INTERPOLATION,
                                "unclosed `{` in string",
                                Span::new(at(i), at(i) + 1),
                            )
                            .with_label("this `{` starts an interpolation")
                            .with_help("close it with `}`, or write `{{` for a literal brace"),
                        );
                        buf.push_str(&body[i..]);
                        break;
                    };
                    if !buf.is_empty() {
                        pieces.push(Piece::Text(std::mem::take(&mut buf)));
                    }
                    pieces.push(Piece::Interp(Span::new(at(i + 1), at(close))));
                    for (j, _) in chars.by_ref() {
                        if j == close {
                            break;
                        }
                    }
                }
                '}' => {
                    self.push(
                        Diagnostic::error(
                            codes::BAD_INTERPOLATION,
                            "unmatched `}` in string",
                            Span::new(at(i), at(i) + 1),
                        )
                        .with_label("no `{` opens this")
                        .with_help("write `}}` for a literal brace"),
                    );
                    buf.push('}');
                }
                _ => buf.push(c),
            }
        }
        if !buf.is_empty() || pieces.is_empty() {
            pieces.push(Piece::Text(buf));
        }
        pieces
    }

    /// Parses the expression inside `{...}` by re-lexing that slice of the file.
    fn interpolation(&mut self, span: Span) -> ExprId {
        let inner = self.text(span).to_owned();
        if inner.trim().is_empty() {
            self.push(
                Diagnostic::error(
                    codes::BAD_INTERPOLATION,
                    "empty interpolation `{}`",
                    Span::new(span.start - 1, span.end + 1),
                )
                .with_label("expected an expression inside")
                .with_help(
                    "put an expression inside, e.g. `{name}`, or write `{{}}` for literal braces",
                ),
            );
            return self.alloc_expr(ExprKind::Error, span);
        }

        let before = self.diags.len();
        let tokens = lex(&inner, span.start, &mut self.diags);
        let new_lex_errors: Vec<u32> = self.diags[before..]
            .iter()
            .map(|d| d.span().start)
            .collect();
        self.lex_errors.extend(new_lex_errors);
        let saved_tokens = std::mem::replace(&mut self.tokens, tokens);
        let saved_pos = std::mem::replace(&mut self.pos, 0);
        let saved_last = self.last_error_at.take();
        let saved_interp = std::mem::replace(&mut self.in_interpolation, true);
        let saved_newlines = std::mem::replace(&mut self.newlines, false);

        let expr = match self.expr() {
            Ok(e) => {
                if !self.at(T::Eof) {
                    let t = self.tok();
                    let found = self.found(t);
                    self.report(
                        Diagnostic::error(
                            codes::BAD_INTERPOLATION,
                            format!("unexpected {found} in interpolation"),
                            t.span,
                        )
                        .with_label("expected `}`")
                        .with_help("an interpolation holds a single expression"),
                    );
                }
                e
            }
            Err(Bail) => self.alloc_expr(ExprKind::Error, span),
        };

        self.tokens = saved_tokens;
        self.pos = saved_pos;
        self.last_error_at = saved_last;
        self.in_interpolation = saved_interp;
        self.newlines = saved_newlines;
        expr
    }
}

fn p_member_start(kind: T) -> bool {
    matches!(kind, T::Fn | T::Ai | T::Pub | T::At)
}

fn binop(kind: T) -> Option<BinOp> {
    Some(match kind {
        T::OrOr => BinOp::Or,
        T::AndAnd => BinOp::And,
        T::EqEq => BinOp::Eq,
        T::NotEq => BinOp::Ne,
        T::Lt => BinOp::Lt,
        T::LtEq => BinOp::Le,
        T::Gt => BinOp::Gt,
        T::GtEq => BinOp::Ge,
        T::Plus => BinOp::Add,
        T::Minus => BinOp::Sub,
        T::Star => BinOp::Mul,
        T::Slash => BinOp::Div,
        T::Percent => BinOp::Rem,
        _ => return None,
    })
}

fn unescape(c: char) -> Option<char> {
    Some(match c {
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        '0' => '\0',
        '\\' | '"' => c,
        _ => return None,
    })
}

/// Byte offset of the `}` closing an interpolation whose `{` precedes `s`.
fn matching_brace(s: &str) -> Option<usize> {
    let mut depth = 1u32;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}
