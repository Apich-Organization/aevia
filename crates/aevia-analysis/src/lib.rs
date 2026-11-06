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
//! edit can be loaded with an entry overlay, or [`SourceOverlays`] can supply
//! every unsaved physical source in one snapshot. [`Analysis::from_program`]
//! answers from a program a caller already has. [`AnalysisCache`] compares the
//! exact source reads and import candidates observed by the loader, without
//! walking unrelated package files or relying on timestamps.
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
//! use aevia_analysis::Analysis;
//! use aevia_analysis::Document;
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

mod actions;
pub mod analysis;
pub mod cache;
pub mod diagnostic;
pub mod document;
mod occurrence;
pub mod position;
pub mod prelude;
mod tokens;
pub mod types;
mod walk;

pub use aevia_core::source::SourceOverlays;
pub use aevia_core::source::canonical_path as canonical_source_path;
pub use aevia_core::source::file_uri_path as physical_source_path_from_uri;
pub use aevia_core::source::has_uri_scheme as is_source_uri;
pub use analysis::Analysis;
pub use cache::AnalysisCache;
pub use diagnostic::Diagnostic;
pub use diagnostic::Related;
pub use diagnostic::Severity;
pub use document::Change;
pub use document::Document;
pub use document::DocumentStore;
pub use document::EditError;
pub use position::LineIndex;
pub use position::Position;
pub use position::Range;
pub use types::CodeAction;
pub use types::CodeActionKind;
pub use types::Completion;
pub use types::CompletionKind;
pub use types::FileEdit;
pub use types::Hover;
pub use types::Location;
pub use types::Origin;
pub use types::RenameError;
pub use types::RenamePlaceholder;
pub use types::SemanticToken;
pub use types::SignatureHelp;
pub use types::SignatureInfo;
pub use types::Symbol;
pub use types::SymbolKind;
pub use types::TextEdit;
pub use types::TokenKind;
pub use types::WorkspaceSymbol;
