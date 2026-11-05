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
    /// A record field, offered after `.`.
    Field,
    /// A language keyword offered in expression position.
    Keyword,
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
    /// Reached through an `import` or `open` of another module.
    Imported,
    /// A prelude builtin.
    Prelude,
    /// A language keyword, which no module declares.
    Language,
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

/// One declaration in a workspace-wide symbol search.
///
/// A [`Symbol`] is relative to one file; a workspace answer must say *which*
/// file, because the declaration may live in a module the editor never opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceSymbol {
    /// The file the declaration is written in.
    pub document: String,
    /// The declaration, with its range in that file.
    pub symbol: Symbol,
}

/// A replacement of one range with new text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    /// What to replace.
    pub range: Range,
    /// What to put there.
    pub new_text: String,
}

/// One file and the edits a rename makes in it.
///
/// A rename can touch several files — a declaration and its qualified uses in
/// importers — so the result is per file; the transport turns each into one
/// edit group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEdit {
    /// The file the edits belong to.
    pub document: String,
    /// The edits, in document order.
    pub edit: TextEdit,
}

/// What `prepare_rename` reports for a renamable name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenamePlaceholder {
    /// The name under the cursor, to highlight.
    pub range: Range,
    /// The current name, to preselect in the editor's input box.
    pub placeholder: String,
}

/// Why a rename was refused.
///
/// A refusal is data, not a stringly-typed error: the transport renders the
/// message and the analysis tests check the variant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenameError {
    /// The name has no declaration this program may edit (a prelude builtin, a
    /// unit, a module segment, an instance method, …).
    NotRenamable(String),
    /// The requested new name is not a valid identifier for this declaration.
    InvalidName(String),
    /// The new name is already declared in the scope the rename targets.
    Collision(String),
}

impl RenameError {
    /// The sentence to show the user.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            | Self::NotRenamable(message)
            | Self::InvalidName(message)
            | Self::Collision(message) => message,
        }
    }
}

/// One callable signature offered while typing a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureInfo {
    /// The whole signature, for the editor to show, e.g.
    /// `sqrt: (Real[L]) -> Real[L]`.
    pub label: String,
    /// One label per parameter, in order.
    pub parameters: Vec<String>,
    /// The declaration's `##` documentation, when it has any.
    pub documentation: Option<String>,
}

/// What `signature_help` reports at a cursor inside a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureHelp {
    /// Every overload the callee has.
    pub signatures: Vec<SignatureInfo>,
    /// Which overload the call's arity (or the checker's pinned overload)
    /// selects.
    pub active_signature: usize,
    /// Which argument the cursor is in; a trailing comma means the next one.
    pub active_parameter: u32,
}

/// What sort of change a code action makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeActionKind {
    /// A fix for a diagnostic the checker reported.
    QuickFix,
    /// A change that reorganises the code without changing what it does.
    RefactorRewrite,
    /// Pulling an expression out into a named binding.
    RefactorExtract,
}

/// One change the user can ask for at a position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeAction {
    /// What the editor shows in the menu.
    pub title: String,
    /// What sort of change it is.
    pub kind: CodeActionKind,
    /// The edits that carry it out, in document order.
    pub edits: Vec<TextEdit>,
}

/// What a highlighted token is.
///
/// The set is deliberately the one §9.10 names — types, dimensions, units and
/// operator provenance — plus the ordinary "this is a function/variable"
/// distinction any editor expects. A token kind a renderer does not know can
/// be ignored; no token is emitted without a reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    /// A nominal type or a type alias.
    Type,
    /// A dimension such as `L`.
    Dimension,
    /// A unit such as `m` or `°C`, including the unit part of a literal.
    Unit,
    /// An operator whose implementation the checker resolved (`Rssn.sin`, a
    /// class-method call).
    Operator,
    /// A function.
    Function,
    /// A local or module-level binding.
    Variable,
    /// A variant constructor.
    Constructor,
}

/// One highlighted span.
///
/// Tokens never overlap and are ordered by position, so a client can encode
/// them as a delta stream without sorting or merging.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticToken {
    /// The name's range.
    pub range: Range,
    /// What it is.
    pub kind: TokenKind,
}
