use logos::Logos;

use crate::diag::{Diagnostic, codes};
use crate::span::Span;

#[derive(Default, Debug, Clone, PartialEq)]
pub enum LexError {
    #[default]
    UnexpectedChar,
    UnterminatedString,
}

#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[logos(error = LexError)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip(r"//[^\n]*", allow_greedy = true))]
pub enum TokenKind {
    #[token("fn")]
    Fn,
    #[token("pub")]
    Pub,
    #[token("let")]
    Let,
    #[token("type")]
    Type,
    #[token("enum")]
    Enum,
    #[token("match")]
    Match,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("for")]
    For,
    #[token("in")]
    In,
    #[token("while")]
    While,
    #[token("return")]
    Return,
    #[token("import")]
    Import,
    #[token("as")]
    As,
    #[token("uses")]
    Uses,
    #[token("budget")]
    Budget,
    #[token("ai")]
    Ai,
    #[token("true")]
    True,
    #[token("false")]
    False,

    #[regex("[A-Za-z_][A-Za-z0-9_]*")]
    Ident,
    #[token("_", priority = 10)]
    Underscore,
    #[regex("[0-9][0-9_]*")]
    Int,
    #[regex(r"[0-9][0-9_]*\.[0-9][0-9_]*")]
    Float,
    #[token("\"", lex_string)]
    Str,
    /// Produced by `lex` (not logos) for a string that runs to end of input.
    UnterminatedStr,

    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(",")]
    Comma,
    #[token(";")]
    Semi,
    #[token(":")]
    Colon,
    #[token(".")]
    Dot,
    #[token("->")]
    Arrow,
    #[token("=>")]
    FatArrow,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<")]
    Lt,
    #[token("<=")]
    LtEq,
    #[token(">")]
    Gt,
    #[token(">=")]
    GtEq,
    #[token("&&")]
    AndAnd,
    #[token("||")]
    OrOr,
    #[token("!")]
    Bang,
    #[token("=")]
    Eq,
    #[token("?")]
    Question,

    Eof,
}

fn lex_string(lex: &mut logos::Lexer<TokenKind>) -> Result<(), LexError> {
    let rest = lex.remainder();
    let mut escaped = false;
    for (i, c) in rest.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => {
                lex.bump(i + 1);
                return Ok(());
            }
            _ => {}
        }
    }
    lex.bump(rest.len());
    Err(LexError::UnterminatedString)
}

impl TokenKind {
    pub fn describe(self) -> &'static str {
        use TokenKind::*;
        match self {
            Fn => "`fn`",
            Pub => "`pub`",
            Let => "`let`",
            Type => "`type`",
            Enum => "`enum`",
            Match => "`match`",
            If => "`if`",
            Else => "`else`",
            For => "`for`",
            In => "`in`",
            While => "`while`",
            Return => "`return`",
            Import => "`import`",
            As => "`as`",
            Uses => "`uses`",
            Budget => "`budget`",
            Ai => "`ai`",
            True => "`true`",
            False => "`false`",
            Ident => "identifier",
            Underscore => "`_`",
            Int => "integer",
            Float => "float",
            Str | UnterminatedStr => "string",
            LParen => "`(`",
            RParen => "`)`",
            LBrace => "`{`",
            RBrace => "`}`",
            LBracket => "`[`",
            RBracket => "`]`",
            Comma => "`,`",
            Semi => "`;`",
            Colon => "`:`",
            Dot => "`.`",
            Arrow => "`->`",
            FatArrow => "`=>`",
            Plus => "`+`",
            Minus => "`-`",
            Star => "`*`",
            Slash => "`/`",
            Percent => "`%`",
            EqEq => "`==`",
            NotEq => "`!=`",
            Lt => "`<`",
            LtEq => "`<=`",
            Gt => "`>`",
            GtEq => "`>=`",
            AndAnd => "`&&`",
            OrOr => "`||`",
            Bang => "`!`",
            Eq => "`=`",
            Question => "`?`",
            Eof => "end of file",
        }
    }

    pub fn is_keyword(self) -> bool {
        use TokenKind::*;
        matches!(
            self,
            Fn | Pub
                | Let
                | Type
                | Enum
                | Match
                | If
                | Else
                | For
                | In
                | While
                | Return
                | Import
                | As
                | Uses
                | Budget
                | Ai
                | True
                | False
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    /// A line break comes between the previous token and this one; ends statements.
    pub nl_before: bool,
}

/// Lexes `src`, whose first byte sits at `offset` in the file. Always ends with an `Eof` token.
pub fn lex(src: &str, offset: u32, diags: &mut Vec<Diagnostic>) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut lexer = TokenKind::lexer(src);
    let mut prev_end = 0;
    while let Some(result) = lexer.next() {
        let r = lexer.span();
        let span = Span::new(offset + r.start as u32, offset + r.end as u32);
        // Skipped text (whitespace, comments, rejected chars) since the last token.
        let nl_before = src
            .get(prev_end..r.start)
            .is_some_and(|gap| gap.contains('\n'));
        prev_end = r.end;
        match result {
            Ok(kind) => tokens.push(Token {
                kind,
                span,
                nl_before,
            }),
            Err(LexError::UnterminatedString) => {
                let quote = Span::new(span.start, span.start + 1);
                diags.push(
                    Diagnostic::error(codes::UNTERMINATED_STRING, "unterminated string", quote)
                        .with_label("this string is never closed")
                        .with_help("add a closing `\"`"),
                );
                tokens.push(Token {
                    kind: TokenKind::UnterminatedStr,
                    span,
                    nl_before,
                });
            }
            Err(LexError::UnexpectedChar) => {
                let text = lexer.slice();
                diags.push(
                    Diagnostic::error(
                        codes::UNEXPECTED_CHAR,
                        format!("unexpected character `{}`", text.escape_debug()),
                        span,
                    )
                    .with_label("not valid in Wardscript source"),
                );
            }
        }
    }
    let end = offset + src.len() as u32;
    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span::new(end, end),
        nl_before: true,
    });
    tokens
}
