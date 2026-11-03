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
pub mod diagnostic;
pub mod document;
mod occurrence;
pub mod position;
pub mod prelude;
pub mod types;

pub use analysis::Analysis;
pub use diagnostic::{Diagnostic, Related, Severity};
pub use document::{Change, Document, DocumentStore, EditError};
pub use position::{LineIndex, Position, Range};
pub use types::{
    Completion, CompletionKind, Hover, Location, Origin, Symbol, SymbolKind, TextEdit,
};
