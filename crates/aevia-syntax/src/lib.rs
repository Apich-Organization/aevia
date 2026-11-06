//! Lexer, layout algorithm, parser and AST for Aevia.
//!
//! The crate is deliberately free of any knowledge about types, units or the
//! runtime: it turns text into a [`ast::Module`] plus [`diagnostic::Diagnostic`]
//! values. Unit *expressions* are kept as syntax ([`aevia_dim::UnitExpr`]) and
//! resolved later, which is what lets a file use `3 km` before declaring `km`
//! (spec §1.8.1).
//!
//! ```
//! use aevia_syntax::lex;
//! use aevia_syntax::parse_module;
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
pub mod fingerprint;
pub mod lexer;
pub mod parser;
pub mod span;
pub mod token;

pub use ast::Decl;
pub use ast::DeclKind;
pub use ast::Expr;
pub use ast::ExprKind;
pub use ast::Module;
pub use ast::Pat;
pub use ast::PatKind;
pub use ast::Ty;
pub use ast::TyKind;
pub use diagnostic::Diagnostic;
pub use diagnostic::Diagnostics;
pub use diagnostic::Severity;
pub use diagnostic::Style;
pub use lexer::LexOutput;
pub use lexer::lex;
pub use lexer::lex_inline;
pub use parser::parse_expression;
pub use parser::parse_module;
pub use span::FileId;
pub use span::SourceFile;
pub use span::SourceMap;
pub use span::Span;
pub use token::Keyword;
pub use token::Punct;
pub use token::StringPart;
pub use token::Token;
pub use token::TokenKind;
