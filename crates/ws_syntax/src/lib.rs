//! Lexer, parser and arena-allocated AST for Wardscript.

pub mod ast;
pub mod diag;
pub mod lexer;
mod parser;
pub mod printer;
pub mod span;

pub use diag::{Code, Diagnostic, Label, Severity};
pub use parser::{Parse, parse};
pub use span::{LineCol, LineIndex, Span};
