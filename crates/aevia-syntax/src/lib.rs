//! Lexer, layout algorithm, parser and AST for Aevia.
//!
//! The crate is deliberately free of any knowledge about types, units or the
//! runtime: it turns text into a [`ast::Module`] plus [`diagnostic::Diagnostic`]
//! values. Unit *expressions* are kept as syntax ([`aevia_dim::UnitExpr`]) and
//! resolved later, which is what lets a file use `3 km` before declaring `km`
//! (spec §1.8.1).
//!
//! ```
//! use aevia_syntax::{lex, parse_module};
//!
//! let source = "let g = 9.80665 m/s^2\n";
//! let lexed = lex(0, source).expect("lexes");
//! let (module, diagnostics) = parse_module(0, source, &lexed);
//! assert!(!diagnostics.has_errors());
//! assert_eq!(module.decls.len(), 1);
//! ```

#![forbid(unsafe_code)]
#![allow(clippy::large_enum_variant)]

pub mod ast;
pub mod diagnostic;
pub mod lexer;
pub mod parser;
pub mod span;
pub mod token;

pub use ast::{Decl, DeclKind, Expr, ExprKind, Module, Pat, PatKind, Ty, TyKind};
pub use diagnostic::{Diagnostic, Diagnostics, Severity, Style};
pub use lexer::{lex, lex_inline, LexOutput};
pub use parser::{parse_expression, parse_module};
pub use span::{FileId, SourceFile, SourceMap, Span};
pub use token::{Keyword, Punct, StringPart, Token, TokenKind};
