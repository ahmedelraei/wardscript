use crate::span::Span;

/// A stable diagnostic code (`W0xxx`). Once assigned, a code's meaning never changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Code(pub &'static str);

/// Every diagnostic code, in one registry so numbers can't collide.
/// Documented in `docs/spec/diagnostics.md`.
pub mod codes {
    use super::Code;

    // W00xx: syntax

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
    pub const MISPLACED_ANNOTATION: Code = Code("W0024");

    // W010x: names and modules, plus W0107 (trust).
    pub const UNRESOLVED_VALUE: Code = Code("W0100");
    pub const UNRESOLVED_TYPE: Code = Code("W0101");
    pub const UNRESOLVED_MODULE: Code = Code("W0102");
    pub const DUPLICATE_DEFINITION: Code = Code("W0103");
    pub const PRIVATE_ITEM: Code = Code("W0104");
    pub const NO_SUCH_MEMBER: Code = Code("W0105");
    pub const WRONG_KIND_OF_NAME: Code = Code("W0106");
    pub const UNTRUSTED_TO_SINK: Code = Code("W0107");

    // W011x-W012x: types
    pub const TYPE_MISMATCH: Code = Code("W0110");
    pub const NO_SUCH_FIELD: Code = Code("W0111");
    pub const WRONG_TYPE_ARG_COUNT: Code = Code("W0112");
    pub const MISSING_FIELDS: Code = Code("W0113");
    pub const DUPLICATE_FIELD: Code = Code("W0114");
    pub const WRONG_ARG_COUNT: Code = Code("W0115");
    pub const NOT_CALLABLE: Code = Code("W0116");
    pub const NON_EXHAUSTIVE_MATCH: Code = Code("W0117");
    pub const INVALID_TRY: Code = Code("W0118");
    pub const INVALID_OPERANDS: Code = Code("W0119");
    pub const LLM_RETURN_NOT_SCHEMA: Code = Code("W0120");
    pub const ANNOTATION_NEEDED: Code = Code("W0121");
    pub const NO_SUCH_METHOD: Code = Code("W0122");
    pub const INVALID_VALIDATE_RULE: Code = Code("W0123");
    pub const NOT_A_VALUE: Code = Code("W0124");
    pub const CYCLIC_ALIAS: Code = Code("W0125");
    pub const INVALID_ASSIGNMENT: Code = Code("W0126");
    pub const UNREACHABLE_PATTERN: Code = Code("W0127");
    pub const MISSING_PROPAGATE: Code = Code("W0128");
    pub const UNHANDLED_THROW: Code = Code("W0129");
    pub const AI_FN_THROWS: Code = Code("W0130");
    pub const TRY_CANNOT_THROW: Code = Code("W0131");
    pub const INVALID_REFINEMENT: Code = Code("W0132");
    pub const MISPLACED_REFINEMENT: Code = Code("W0133");
    pub const CHECK_NOT_AI: Code = Code("W0134");

    // W014x: classes
    pub const INVALID_BASE_CLASS: Code = Code("W0140");
    pub const INVALID_OVERRIDE: Code = Code("W0141");
    pub const PRIVATE_MEMBER: Code = Code("W0142");
    pub const UNINITIALIZED_FIELD: Code = Code("W0143");
    pub const INVALID_SUPER: Code = Code("W0144");
    pub const INVALID_CLASS: Code = Code("W0145");
    pub const MISSING_IMPLEMENTATION: Code = Code("W0146");
    pub const ABSTRACT_INSTANCE: Code = Code("W0147");

    // W02xx: effects, budgets, Rule of Two
    pub const MISSING_EFFECT: Code = Code("W0200");
    pub const UNUSED_EFFECT: Code = Code("W0201");
    pub const UNKNOWN_EFFECT: Code = Code("W0202");
    pub const INVALID_BUDGET: Code = Code("W0210");
    pub const BUDGET_EXCEEDED: Code = Code("W0211");
    pub const CALLEE_BUDGET: Code = Code("W0212");
    pub const RULE_OF_TWO: Code = Code("W0220");
    pub const INVALID_ANNOTATION: Code = Code("W0221");
    pub const UNNECESSARY_ALLOW: Code = Code("W0222");
    pub const INVALID_MODEL_CLAUSE: Code = Code("W0230");
    pub const MODEL_CLAUSE_NOT_AI: Code = Code("W0231");

    // W03xx: tools
    pub const INVALID_LOCK: Code = Code("W0300");
    pub const TOOL_NOT_LOCKED: Code = Code("W0301");

    pub const ALL: &[Code] = &[
        UNEXPECTED_CHAR,
        UNTERMINATED_STRING,
        INVALID_ESCAPE,
        BAD_INTERPOLATION,
        INVALID_NUMBER,
        EXPECTED_TOKEN,
        EXPECTED_ITEM,
        EXPECTED_EXPR,
        EXPECTED_TYPE,
        EXPECTED_PATTERN,
        MISSING_SEMICOLON,
        LLM_PROMPT_NOT_STRING,
        LLM_FN_WITHOUT_RETURN_TYPE,
        DUPLICATE_CLAUSE,
        CHAINED_COMPARISON,
        INVALID_ASSIGN_TARGET,
        UNCLOSED_DELIMITER,
        INTERPOLATION_NOT_ALLOWED,
        PUB_IMPORT,
        MISPLACED_ANNOTATION,
        UNRESOLVED_VALUE,
        UNRESOLVED_TYPE,
        UNRESOLVED_MODULE,
        DUPLICATE_DEFINITION,
        PRIVATE_ITEM,
        NO_SUCH_MEMBER,
        WRONG_KIND_OF_NAME,
        UNTRUSTED_TO_SINK,
        TYPE_MISMATCH,
        NO_SUCH_FIELD,
        WRONG_TYPE_ARG_COUNT,
        MISSING_FIELDS,
        DUPLICATE_FIELD,
        WRONG_ARG_COUNT,
        NOT_CALLABLE,
        NON_EXHAUSTIVE_MATCH,
        INVALID_TRY,
        INVALID_OPERANDS,
        LLM_RETURN_NOT_SCHEMA,
        ANNOTATION_NEEDED,
        NO_SUCH_METHOD,
        INVALID_VALIDATE_RULE,
        NOT_A_VALUE,
        CYCLIC_ALIAS,
        INVALID_ASSIGNMENT,
        UNREACHABLE_PATTERN,
        MISSING_PROPAGATE,
        UNHANDLED_THROW,
        AI_FN_THROWS,
        TRY_CANNOT_THROW,
        INVALID_REFINEMENT,
        MISPLACED_REFINEMENT,
        CHECK_NOT_AI,
        INVALID_BASE_CLASS,
        INVALID_OVERRIDE,
        PRIVATE_MEMBER,
        UNINITIALIZED_FIELD,
        INVALID_SUPER,
        INVALID_CLASS,
        MISSING_IMPLEMENTATION,
        ABSTRACT_INSTANCE,
        MISSING_EFFECT,
        UNUSED_EFFECT,
        UNKNOWN_EFFECT,
        INVALID_BUDGET,
        BUDGET_EXCEEDED,
        CALLEE_BUDGET,
        RULE_OF_TWO,
        INVALID_ANNOTATION,
        UNNECESSARY_ALLOW,
        INVALID_MODEL_CLAUSE,
        MODEL_CLAUSE_NOT_AI,
        INVALID_LOCK,
        TOOL_NOT_LOCKED,
    ];
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
    /// Extra context that has no span in this file, e.g. steps in another module.
    pub notes: Vec<String>,
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
            notes: Vec::new(),
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

    pub fn warning(code: Code, message: impl Into<String>, span: Span) -> Self {
        Diagnostic {
            severity: Severity::Warning,
            ..Diagnostic::error(code, message, span)
        }
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}
