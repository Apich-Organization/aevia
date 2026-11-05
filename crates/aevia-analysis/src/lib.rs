//! Semantic queries over Aevia sources, with no knowledge of any editor.
//!
//! This crate is the language server's brain and nothing else. It has no
//! transport, no JSON, and no vocabulary from the Language Server Protocol:
//! [`Analysis`] takes a [`Document`] and a byte offset and returns Rust values
//! that this crate defines ([`Diagnostic`], [`Hover`], [`Completion`],
//! [`Location`], [`Symbol`], [`TextEdit`]).
//!
//! That boundary is the point. The editor adapter is a translation table; a
//! playground, a CLI `check --json`, a Zed extension, a Neovim plugin and a
//! documentation generator all consume *this* API directly, and none of them
//! has to reimplement a byte-offset-to-position mapping or a diagnostic
//! conversion.
//!
//! Beyond the six first queries the crate answers the two §9.10 features that
//! need a compiler: [`Analysis::code_actions`] (add a type annotation, convert
//! a Celsius literal to kelvin, extract a `let`, fix a dimension mismatch —
//! each validated by compiling the rewritten text) and
//! [`Analysis::semantic_tokens`] (types, dimensions, units and resolved
//! operators, read from the checker's own records).
//!
//! **One document or a whole program.** [`Analysis::new`] compiles a document's
//! text alone: the right answer for a scratch buffer. [`Analysis::from_path`]
//! loads a file **and its import graph** through `aevia_core::driver`, so an
//! imported qualified name (`Math.sqrt`), an `open`ed name, or a field of an
//! imported record resolves instead of being reported as unbound. An unsaved
//! edit is loaded as an entry **overlay**: the buffer's text is the entry
//! module, the imports still come from disk. [`Analysis::from_program`] answers
//! from a program a caller already has, and [`AnalysisCache`] is the small
//! content-hash/mtime-keyed cache that makes repeated editor requests cheap
//! without ever serving a stale dependency.
//!
//! [`Analysis::workspace_symbols`] returns declarations from every module of the
//! loaded program, each with its own file and range — the analysis half of a
//! workspace-wide symbol search. [`Analysis::references`], [`Analysis::rename`]
//! and [`Analysis::prepare_rename`] share one canonical declaration identity
//! across modules; [`Analysis::signature_help`] resolves the call under the
//! cursor to its overloads and active argument, and [`Analysis::completion`]
//! narrows to a record's fields after `.` and to keywords where an expression
//! can start.
//!
//! # The two things that are easy to get wrong
//!
//! **Positions.** Compiler spans are byte ranges; editor positions are lines
//! and UTF-16 code units. `∫`, `°C`, `Ω` and `²` are multi-byte but one code
//! unit, and an emoji is four bytes but *two* code units. [`LineIndex`] is the
//! only place in the tree that knows the difference, and it is tested against
//! all of them.
//!
//! **Honesty.** [`Analysis::hover`] reports the type the checker actually
//! recorded — the declared scheme of a name, the pinned overload of a
//! `Rssn.<op>` call, or the type of the innermost expression under the cursor
//! — and returns `None` when the checker left the type undecided. It does not
//! guess.
//!
//! ```
//! use aevia_analysis::{Analysis, Document};
//!
//! let document = Document::new("playground.aev", 1, "let g = 9.80665 m/s^2\n");
//! let analysis = Analysis::new(&document);
//! assert!(analysis.diagnostics().is_empty());
//!
//! // The byte offset of `g` in the second token.
//! let hover = analysis.hover(4).expect("`g` is an exported binding");
//! assert!(hover.contents.contains("Float"));
//!
//! // Formatting is delegated, and is offered only when it changes something.
//! let messy = Document::new("p.aev", 1, "let  x=1\n");
//! assert!(Analysis::new(&messy).formatting().is_some());
//! ```

#![forbid(unsafe_code)]
#![allow(clippy::module_name_repetitions)]

pub mod analysis;
mod actions;
pub mod cache;
pub mod diagnostic;
pub mod document;
mod occurrence;
pub mod position;
pub mod prelude;
mod tokens;
pub mod types;
mod walk;

pub use analysis::Analysis;
pub use cache::AnalysisCache;
pub use diagnostic::{Diagnostic, Related, Severity};
pub use document::{Change, Document, DocumentStore, EditError};
pub use position::{LineIndex, Position, Range};
pub use types::{
    CodeAction, CodeActionKind, Completion, CompletionKind, FileEdit, Hover, Location, Origin,
    RenameError, RenamePlaceholder, SemanticToken, SignatureHelp, SignatureInfo, Symbol, SymbolKind,
    TextEdit, TokenKind, WorkspaceSymbol,
};
