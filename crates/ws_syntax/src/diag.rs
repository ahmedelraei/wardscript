use crate::span::Span;

/// A stable diagnostic code (`W0xxx`). Once assigned, a code's meaning never changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Code(pub &'static str);

/// Syntax diagnostics (W00xx). Documented in `docs/spec/diagnostics.md`.
pub mod codes {
    use super::Code;

    pub const UNEXPECTED_CHAR: Code = Code("W0001");
    pub const UNTERMINATED_STRING: Code = Code("W0002");
    pub const INVALID_ESCAPE: Code = Code("W0003");
    pub const BAD_INTERPOLATION: Code = Code("W0004");
    pub const INVALID_NUMBER: Code = Code("W0005");
    pub const EXPECTED_TOKEN: Code = Code("W0010");
    pub const EXPECTED_ITEM: Code = Code("W0011");
    pub const EXPECTED_EXPR: Code = Code("W0012");
    pub const EXPECTED_TYPE: Code = Code("W0013");
    pub const EXPECTED_PATTERN: Code = Code("W0014");
    pub const MISSING_SEMICOLON: Code = Code("W0015");
    pub const LLM_PROMPT_NOT_STRING: Code = Code("W0016");
    pub const LLM_FN_WITHOUT_RETURN_TYPE: Code = Code("W0017");
    pub const DUPLICATE_CLAUSE: Code = Code("W0018");
    pub const CHAINED_COMPARISON: Code = Code("W0019");
    pub const INVALID_ASSIGN_TARGET: Code = Code("W0020");
    pub const UNCLOSED_DELIMITER: Code = Code("W0021");
    pub const INTERPOLATION_NOT_ALLOWED: Code = Code("W0022");
    pub const PUB_IMPORT: Code = Code("W0023");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub span: Span,
    pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: Code,
    pub severity: Severity,
    pub message: String,
    /// The first label is the primary one.
    pub labels: Vec<Label>,
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn error(code: Code, message: impl Into<String>, span: Span) -> Self {
        Diagnostic {
            code,
            severity: Severity::Error,
            message: message.into(),
            labels: vec![Label {
                span,
                message: None,
            }],
            help: None,
        }
    }

    pub fn span(&self) -> Span {
        self.labels.first().map_or_else(Span::default, |l| l.span)
    }

    pub fn with_label(mut self, message: impl Into<String>) -> Self {
        if let Some(primary) = self.labels.first_mut() {
            primary.message = Some(message.into());
        }
        self
    }

    pub fn with_secondary(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            message: Some(message.into()),
        });
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}
