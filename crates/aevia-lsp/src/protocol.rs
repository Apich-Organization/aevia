//! The translation table between the protocol and `aevia-analysis`.
//!
//! Everything that knows a protocol field name, a numeric enum or a JSON shape
//! lives here. The analysis crate has none of it; this module has nothing else.
//! Two tables are the whole coupling: one for each enum the protocol numbers.

use aevia_analysis::{
    Completion, CompletionKind, Diagnostic, Hover, Location, Position, Range, Severity, Symbol,
    SymbolKind, TextEdit,
};
use serde_json::{Value, json};

/// Reads a protocol position (`{ line, character }`, UTF-16 code units).
///
/// Returns `None` rather than guessing when the object is malformed: a client
/// that sends a position without a line gets no answer, not the start of the
/// file.
#[must_use]
pub fn position(value: &Value) -> Option<Position> {
    let line = value.get("line")?.as_u64()?;
    let column = value.get("character")?.as_u64()?;
    Some(Position::new(
        u32::try_from(line).ok()?,
        u32::try_from(column).ok()?,
    ))
}

/// Reads a protocol range.
#[must_use]
pub fn range(value: &Value) -> Option<Range> {
    Some(Range::new(
        position(value.get("start")?)?,
        position(value.get("end")?)?,
    ))
}

/// Writes a position.
#[must_use]
pub fn position_json(position: Position) -> Value {
    json!({ "line": position.line, "character": position.column })
}

/// Writes a range.
#[must_use]
pub fn range_json(range: Range) -> Value {
    json!({ "start": position_json(range.start), "end": position_json(range.end) })
}

/// The protocol's severity number.
///
/// The protocol numbers the four ranks; the analysis crate does not, because a
/// playground that prints `error`/`warning` has no use for a number.
fn severity_number(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 1,
        Severity::Warning => 2,
        Severity::Info => 3,
        Severity::Hint => 4,
    }
}

/// The protocol's completion kind.
///
/// `Unit` (11) exists in the protocol and is exactly what a unit is; a
/// dimension has no kind of its own, so it is `TypeParameter` (25) — a
/// type-level name.
fn completion_kind(kind: CompletionKind) -> u8 {
    match kind {
        CompletionKind::Function => 3,
        CompletionKind::Value => 12,
        CompletionKind::Constructor => 4,
        CompletionKind::Unit => 11,
        CompletionKind::Dimension => 25,
        CompletionKind::Type => 22,
    }
}

/// The protocol's symbol kind.
fn symbol_kind(kind: SymbolKind) -> u8 {
    match kind {
        SymbolKind::Function => 12,
        SymbolKind::Method => 6,
        SymbolKind::Variable => 13,
        SymbolKind::Constant => 14,
        SymbolKind::Struct => 23,
        SymbolKind::Enum => 10,
        SymbolKind::EnumMember => 22,
        SymbolKind::Interface => 11,
        SymbolKind::Object => 19,
        SymbolKind::TypeParameter => 26,
        SymbolKind::Namespace => 3,
    }
}

/// Writes one diagnostic, with the document it belongs to.
#[must_use]
pub fn diagnostic_json(uri: &str, diagnostic: &Diagnostic) -> Value {
    let mut value = json!({
        "range": range_json(diagnostic.range),
        "severity": severity_number(diagnostic.severity),
        "source": "aevia",
        "message": diagnostic.message,
    });
    if let Some(code) = &diagnostic.code {
        value["code"] = json!(code);
    }
    if !diagnostic.related.is_empty() {
        let related: Vec<Value> = diagnostic
            .related
            .iter()
            .map(|related| {
                json!({
                    "location": { "uri": uri, "range": range_json(related.range) },
                    "message": related.message,
                })
            })
            .collect();
        value["relatedInformation"] = Value::Array(related);
    }
    value
}

/// Writes a hover.
#[must_use]
pub fn hover_json(hover: &Hover) -> Value {
    let mut value = json!({
        "contents": { "kind": "markdown", "value": hover.contents },
    });
    if let Some(range) = hover.range {
        value["range"] = range_json(range);
    }
    value
}

/// Writes a completion list.
#[must_use]
pub fn completion_json(items: &[Completion]) -> Value {
    let items: Vec<Value> = items
        .iter()
        .map(|item| {
            let mut value = json!({
                "label": item.label,
                "kind": completion_kind(item.kind),
            });
            if let Some(detail) = &item.detail {
                value["detail"] = json!(detail);
            }
            value
        })
        .collect();
    Value::Array(items)
}

/// Writes a location.
#[must_use]
pub fn location_json(location: &Location) -> Value {
    json!({ "uri": location.document, "range": range_json(location.range) })
}

/// Writes a document outline.
#[must_use]
pub fn symbols_json(symbols: &[Symbol]) -> Value {
    Value::Array(symbols.iter().map(symbol_value).collect())
}

fn symbol_value(symbol: &Symbol) -> Value {
    let mut value = json!({
        "name": symbol.name,
        "kind": symbol_kind(symbol.kind),
        "range": range_json(symbol.range),
        "selectionRange": range_json(symbol.selection),
    });
    if let Some(detail) = &symbol.detail {
        value["detail"] = json!(detail);
    }
    if !symbol.children.is_empty() {
        value["children"] = symbols_json(&symbol.children);
    }
    value
}

/// Writes a text edit.
#[must_use]
pub fn text_edit_json(edit: &TextEdit) -> Value {
    json!({ "range": range_json(edit.range), "newText": edit.new_text })
}

/// The capabilities this server actually implements.
///
/// Advertised from what the code does, not from what a client is: every field
/// here is backed by a handler in [`crate::server`], and nothing else is
/// claimed. `change: 2` is incremental sync; `save: false` says honestly that
/// no `didSave` handler exists.
#[must_use]
pub fn capabilities() -> Value {
    json!({
        "positionEncoding": "utf-16",
        "textDocumentSync": {
            "openClose": true,
            "change": 2,
            "save": false,
        },
        "hoverProvider": true,
        "completionProvider": { "resolveProvider": false },
        "definitionProvider": true,
        "documentSymbolProvider": true,
        "documentFormattingProvider": true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_position_round_trips() {
        let value = position_json(Position::new(3, 7));
        assert_eq!(value, json!({ "line": 3, "character": 7 }));
        assert_eq!(position(&value), Some(Position::new(3, 7)));
    }

    #[test]
    fn a_malformed_position_is_none() {
        assert_eq!(position(&json!({ "line": 1 })), None);
        assert_eq!(position(&json!({ "line": -1, "character": 0 })), None);
        assert_eq!(position(&json!("nope")), None);
    }

    #[test]
    fn severities_keep_their_rank() {
        assert_eq!(severity_number(Severity::Error), 1);
        assert_eq!(severity_number(Severity::Warning), 2);
        assert_eq!(severity_number(Severity::Info), 3);
        assert_eq!(severity_number(Severity::Hint), 4);
    }

    #[test]
    fn a_unit_and_a_dimension_are_distinguishable() {
        assert_eq!(completion_kind(CompletionKind::Unit), 11);
        assert_eq!(completion_kind(CompletionKind::Dimension), 25);
        assert_ne!(
            completion_kind(CompletionKind::Unit),
            completion_kind(CompletionKind::Value)
        );
    }

    #[test]
    fn capabilities_claim_only_what_is_implemented() {
        let capabilities = capabilities();
        assert_eq!(capabilities["textDocumentSync"]["change"], 2);
        assert_eq!(capabilities["textDocumentSync"]["save"], false);
        assert_eq!(capabilities["positionEncoding"], "utf-16");
        assert_eq!(capabilities["completionProvider"]["resolveProvider"], false);
    }
}
