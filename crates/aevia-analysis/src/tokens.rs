//! Semantic highlighting: which words are types, dimensions, units, operators.
//!
//! The categories are the ones §9.10 names. Three sources feed them, in order:
//!
//! 1. the occurrence walk — a name that resolves to a unit, a dimension, a
//!    function or a class-method call is classified from the checker's own
//!    records, so the highlight and the hover cannot disagree;
//! 2. the checker's tables for a name the occurrence walk did not bind, such
//!    as a prelude builtin or an SI unit used bare;
//! 3. a lexical pass for the dimension names inside a type (`Real[L]`) and
//!    for unit literals — the AST stores the resolved dimension and unit
//!    expression, not their tokens, so the tokens are read back from the
//!    document. Every one of those tokens is a word the compiler itself
//!    resolved, which is what keeps the list from inventing highlights.
//!
//! Tokens never overlap and come out in position order.

use crate::document::Document;
use crate::occurrence::{self, Occurrence, Role};
use crate::types::{SemanticToken, TokenKind};
use aevia_syntax::token::TokenKind as SyntaxToken;
use aevia_types::check::{CheckedModule, Export};
use std::collections::HashSet;

/// The built-in types the parser recognises as their own `TyKind`.
///
/// They never become a name occurrence, so the lexical pass is what highlights
/// them. The list is the surface spelling of every primitive type the grammar
/// has a production for; a user type is a `TyKind::Named` and is already an
/// occurrence.
const PRIMITIVE_TYPES: [&str; 22] = [
    "Int", "Float", "Real", "Complex", "Bool", "Prop", "String", "Str", "Unit", "Tensor", "Matrix",
    "Sparse", "Result", "Option", "Set", "Table", "RateOf", "Io", "Dyn", "Point", "Sym", "Nat",
];

/// Classifies every highlighted word in `document`.
#[must_use]
pub(crate) fn tokens(
    document: &Document,
    file: aevia_syntax::FileId,
    occurrences: &[Occurrence],
    checked: &CheckedModule,
) -> Vec<SemanticToken> {
    let text = document.text();
    let mut spans: Vec<(u32, u32, TokenKind)> = Vec::new();
    for occurrence in occurrences {
        let Some(kind) = classify(occurrence, checked) else {
            continue;
        };
        let span = match &occurrence.role {
            Role::Qualified(_) => occurrence.span,
            Role::Name(name)
            | Role::Constructor(name)
            | Role::TypeName(name)
            | Role::Unit(name)
            | Role::Method { name, .. } => {
                occurrence::name_span(text, occurrence.span, name)
            }
        };
        if span.start < span.end {
            spans.push((span.start, span.end, kind));
        }
    }

    // A type the parser turns into its own `TyKind` (`Int`, `Float`, `Real`,
    // …) has no name occurrence, and a dimension name inside `Real[L]` has no
    // span in the AST at all. Both are still words the compiler resolved: the
    // first is recognised here, the second from the dimension environment.
    // Unknown upper-case words — module segments, attributes — are left
    // unhighlighted rather than guessed at.
    if let Ok(lexed) = aevia_syntax::lex(file, text) {
        let covered: HashSet<(u32, u32)> = spans.iter().map(|(start, end, _)| (*start, *end)).collect();
        for token in lexed.tokens {
            let SyntaxToken::UpperIdent(name) = &token.kind else {
                continue;
            };
            if covered.contains(&(token.span.start, token.span.end)) {
                continue;
            }
            let kind = if checked.dims.lookup(name).is_some() {
                TokenKind::Dimension
            } else if checked.units.contains(name) {
                TokenKind::Unit
            } else if PRIMITIVE_TYPES.contains(&name.as_str()) {
                TokenKind::Type
            } else {
                continue;
            };
            spans.push((token.span.start, token.span.end, kind));
        }
    }

    // Nested occurrences can describe the same word (a declaration name and
    // its binder); one highlight is enough, and tokens must not overlap.
    spans.sort_unstable_by_key(|(start, end, _)| (*start, *end));
    spans.dedup();
    let mut tokens: Vec<SemanticToken> = Vec::new();
    let mut last_end = 0u32;
    for (start, end, kind) in spans {
        if start < last_end {
            continue;
        }
        last_end = end;
        tokens.push(SemanticToken {
            range: document.range_of(start, end),
            kind,
        });
    }
    tokens
}

/// The token kind for one occurrence, from the checker's records.
fn classify(occurrence: &Occurrence, checked: &CheckedModule) -> Option<TokenKind> {
    match &occurrence.role {
        Role::Name(name) => {
            if let Some(export) = checked.exports.get(name) {
                return Some(export_kind(export));
            }
            // A class-method call: the interesting fact is which instance
            // implements it, which is what "operator provenance" means here.
            if let Some(call) = occurrence.call {
                if checked
                    .dictionaries
                    .iter()
                    .any(|use_| use_.span == call && use_.method == *name)
                {
                    return Some(TokenKind::Operator);
                }
            }
            // A binding the walk resolved is a local no matter what else
            // shares its spelling: the SI table has a unit called `d` (day)
            // and a function parameter may be named `d`.
            if occurrence.binding.is_some() {
                return Some(TokenKind::Variable);
            }
            if checked.units.contains(name) {
                return Some(TokenKind::Unit);
            }
            if checked.dims.lookup(name).is_some() {
                return Some(TokenKind::Dimension);
            }
            if aevia_types::builtins::schemes(name).is_some() {
                return Some(TokenKind::Function);
            }
            Some(TokenKind::Variable)
        }
        // `Rssn.sin` is a pinned kernel operator: its whole dotted name is the
        // provenance the user is being shown.
        Role::Qualified(path) => {
            if path.first().map(String::as_str) == Some("Rssn") {
                Some(TokenKind::Operator)
            } else {
                Some(TokenKind::Type)
            }
        }
        Role::Constructor(_) => Some(TokenKind::Constructor),
        Role::Method { .. } => Some(TokenKind::Function),
        Role::TypeName(name) => match checked.exports.get(name) {
            Some(export) => Some(export_kind(export)),
            None if checked.dims.lookup(name).is_some() => Some(TokenKind::Dimension),
            None if checked.units.contains(name) => Some(TokenKind::Unit),
            None => Some(TokenKind::Type),
        },
        Role::Unit(_) => Some(TokenKind::Unit),
    }
}

/// The token kind of an exported name.
fn export_kind(export: &Export) -> TokenKind {
    match export {
        Export::Value { arity, .. } if *arity > 0 => TokenKind::Function,
        Export::Value { .. } => TokenKind::Variable,
        Export::Ctor { .. } => TokenKind::Constructor,
        Export::TypeCtor { .. } | Export::Type { .. } | Export::Alias { .. } => TokenKind::Type,
        Export::Unit { .. } => TokenKind::Unit,
        Export::Dimension { .. } => TokenKind::Dimension,
    }
}
