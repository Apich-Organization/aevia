//! The result types of the semantic queries.
//!
//! These are this crate's own types. They carry no numeric protocol codes and
//! no protocol field names; an adapter that needs `CompletionItemKind::Unit`
//! maps [`CompletionKind::Unit`] to it, and one that needs a paragraph of HTML
//! maps the Markdown in [`Hover::contents`] itself. The mapping is the
//! adapter's whole job, and it is one table per type.

use crate::position::Range;

/// A place in a document.
///
/// `document` is the name the document was opened under — a URI, a path, or
/// `scratch` in a playground. This crate never parses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    /// Which document.
    pub document: String,
    /// Where in it.
    pub range: Range,
}

/// What the cursor is on, as Markdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hover {
    /// The Markdown body.
    pub contents: String,
    /// The range the hover describes, when it is narrower than the word.
    pub range: Option<Range>,
}

/// The sort of thing a completion names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompletionKind {
    /// A function with parameters.
    Function,
    /// A binding that is not a function.
    Value,
    /// A variant constructor.
    Constructor,
    /// A unit such as `m` or `km`.
    Unit,
    /// A dimension such as `L` or `Currency`.
    Dimension,
    /// A nominal type or a type alias.
    Type,
}

/// One candidate name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    /// What to insert.
    pub label: String,
    /// What it is.
    pub kind: CompletionKind,
    /// The type, rendered by the checker, when there is one.
    pub detail: Option<String>,
    /// Where it came from: `prelude` or the module.
    pub origin: Origin,
}

/// Where a completion came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    /// Declared by the document being edited.
    Module,
    /// A prelude builtin.
    Prelude,
}

/// The sort of thing a symbol names.
///
/// The set is the one every editor's outline understands; a dimension has no
/// kind of its own in any of them, so it is a [`SymbolKind::TypeParameter`] —
/// it is a type-level name, which is what that kind means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    /// `fn`
    Function,
    /// A method of an `instance`.
    Method,
    /// A `let` binding.
    Variable,
    /// A `unit` or a `test`.
    Constant,
    /// A `type` with one constructor.
    Struct,
    /// A `type` with several constructors.
    Enum,
    /// One constructor of a variant type.
    EnumMember,
    /// A `class`.
    Interface,
    /// An `instance`.
    Object,
    /// A `dimension`.
    TypeParameter,
    /// An `import`.
    Namespace,
}

/// One entry in a document's outline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    /// What to show.
    pub name: String,
    /// What it is.
    pub kind: SymbolKind,
    /// The whole declaration.
    pub range: Range,
    /// The name alone, for the editor to highlight.
    pub selection: Range,
    /// The type or signature, when the checker recorded one.
    pub detail: Option<String>,
    /// Nested declarations.
    pub children: Vec<Symbol>,
}

/// A replacement of one range with new text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    /// What to replace.
    pub range: Range,
    /// What to put there.
    pub new_text: String,
}
