//! The translation table between the protocol and `aevia-analysis`.
//!
//! Everything that knows a protocol field name, a numeric enum or a JSON shape
//! lives here. The analysis crate has none of it; this module has nothing else.
//! Two tables are the whole coupling: one for each enum the protocol numbers.

use aevia_analysis::{
    CodeAction, CodeActionKind, Completion, CompletionKind, Diagnostic, FileEdit, Hover, Location,
    Position, Range, RenamePlaceholder, SemanticToken, Severity, SignatureHelp, Symbol, SymbolKind,
    TextEdit, TokenKind,
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
        // The protocol's `Field` and `Keyword`.
        CompletionKind::Field => 5,
        CompletionKind::Keyword => 14,
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
///
/// A location from `aevia-analysis` is a *file identity*: the open document's
/// name (already a URI) for the buffer being edited, and the loader's source
/// path for a declaration in another module. The protocol wants a URI in both
/// cases, so a path is converted here — the analysis crate deliberately does
/// not know what a URI is.
#[must_use]
pub fn location_json(location: &Location) -> Value {
    json!({ "uri": document_uri(&location.document), "range": range_json(location.range) })
}

/// Writes the workspace-wide symbol list (`workspace/symbol`).
///
/// The protocol calls these `SymbolInformation`: a name, a kind and a location
/// — not the nested outline `textDocument/documentSymbol` returns.
#[must_use]
pub fn workspace_symbols_json(symbols: &[aevia_analysis::WorkspaceSymbol]) -> Value {
    Value::Array(
        symbols
            .iter()
            .map(|entry| {
                json!({
                    "name": entry.symbol.name,
                    "kind": symbol_kind(entry.symbol.kind),
                    "location": {
                        "uri": document_uri(&entry.document),
                        // The name, not the whole declaration: a workspace
                        // result jumps to the declaration's name.
                        "range": range_json(entry.symbol.selection),
                    },
                })
            })
            .collect(),
    )
}

/// A file identity as a URI: a path becomes a `file:` URI, a URI passes
/// through.
fn document_uri(document: &str) -> String {
    if document.contains("://") {
        document.to_string()
    } else {
        uri_from_path(std::path::Path::new(document))
    }
}

/// Writes the `textDocument/signatureHelp` result.
///
/// Each parameter label is a `{ label }` object, the form a client accepts on
/// 3.16+; the documentation is Markdown so `##` comments render as text rather
/// than literal punctuation.
#[must_use]
pub fn signature_help_json(help: &SignatureHelp) -> Value {
    let signatures: Vec<Value> = help
        .signatures
        .iter()
        .map(|signature| {
            let parameters: Vec<Value> = signature
                .parameters
                .iter()
                .map(|parameter| json!({ "label": parameter }))
                .collect();
            let mut value = json!({
                "label": signature.label,
                "parameters": parameters,
            });
            if let Some(documentation) = &signature.documentation {
                value["documentation"] = json!({
                    "kind": "markdown",
                    "value": documentation,
                });
            }
            value
        })
        .collect();
    json!({
        "signatures": signatures,
        "activeSignature": help.active_signature,
        "activeParameter": help.active_parameter,
    })
}

/// Writes the locations `textDocument/references` returns.
#[must_use]
pub fn references_json(locations: &[Location]) -> Value {
    Value::Array(locations.iter().map(location_json).collect())
}

/// Writes the `textDocument/prepareRename` result: the range to highlight and
/// the current name.
#[must_use]
pub fn prepare_rename_json(placeholder: &RenamePlaceholder) -> Value {
    json!({
        "range": range_json(placeholder.range),
        "placeholder": placeholder.placeholder,
    })
}

/// Writes a rename as a `WorkspaceEdit` with `documentChanges`.
///
/// `documentChanges` (rather than the older `changes` map) is what a client
/// needs to apply a multi-file edit atomically; one `TextDocumentEdit` is one
/// file, and `version: null` means "apply to the buffer as it is".
#[must_use]
pub fn rename_json(edits: &[FileEdit]) -> Value {
    let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
    for edit in edits {
        let uri = document_uri(&edit.document);
        let position = groups
            .iter()
            .position(|(existing, _)| *existing == uri)
            .unwrap_or_else(|| {
                groups.push((uri.clone(), Vec::new()));
                groups.len() - 1
            });
        groups[position].1.push(text_edit_json(&edit.edit));
    }
    let document_changes: Vec<Value> = groups
        .into_iter()
        .map(|(uri, edits)| {
            json!({
                "textDocument": { "uri": uri, "version": Value::Null },
                "edits": edits,
            })
        })
        .collect();
    json!({ "documentChanges": document_changes })
}

/// The filesystem path a `file:` URI names, or `None` for another scheme.
///
/// Only `file:` is a document this server can load from disk; a virtual
/// document (`untitled:`, a URL) keeps the single-file analysis.
#[must_use]
pub fn path_from_uri(uri: &str) -> Option<std::path::PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let path = percent_decode(rest);
    // `file:///C:/x` leaves `/C:/x`; a drive letter is not a directory.
    let path = match path.strip_prefix('/') {
        Some(rest)
            if rest.len() >= 2
                && rest.as_bytes()[0].is_ascii_alphabetic()
                && rest.as_bytes()[1] == b':' =>
        {
            rest.to_string()
        }
        _ => path,
    };
    Some(std::path::PathBuf::from(path))
}

/// The `file:` URI of a path, for a location in another module.
#[must_use]
pub fn uri_from_path(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let absolute = text.strip_prefix('/').unwrap_or(&text);
    let is_drive = absolute.len() >= 2
        && absolute.as_bytes()[0].is_ascii_alphabetic()
        && absolute.as_bytes()[1] == b':';
    if is_drive {
        format!("file:///{absolute}")
    } else if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

/// Decodes `%XX` escapes in a URI path; an invalid escape is kept literally.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = (bytes[index + 1] as char).to_digit(16);
            let low = (bytes[index + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                out.push(u8::try_from(high * 16 + low).unwrap_or(b'?'));
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
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

/// The protocol's code-action kind.
fn code_action_kind(kind: CodeActionKind) -> &'static str {
    match kind {
        CodeActionKind::QuickFix => "quickfix",
        CodeActionKind::RefactorRewrite => "refactor.rewrite",
        CodeActionKind::RefactorExtract => "refactor.extract",
    }
}

/// Writes the code actions available in a document.
///
/// Each action carries a `WorkspaceEdit` with the one document this server has
/// open: the protocol allows edits across files, and this server deliberately
/// knows only the document it was asked about.
#[must_use]
pub fn code_actions_json(uri: &str, actions: &[CodeAction]) -> Value {
    Value::Array(
        actions
            .iter()
            .map(|action| {
                let edits: Vec<Value> = action.edits.iter().map(text_edit_json).collect();
                let mut changes = serde_json::Map::new();
                changes.insert(uri.to_string(), Value::Array(edits));
                json!({
                    "title": action.title,
                    "kind": code_action_kind(action.kind),
                    "edit": { "changes": changes },
                })
            })
            .collect(),
    )
}

/// The token types this server's legend advertises.
///
/// The four §9.10 categories come first; `dimension` and `unit` are not in the
/// protocol's standard set, which is why a legend exists — a client that does
/// not know a name renders the token with its default style rather than
/// dropping the text.
pub const TOKEN_TYPES: [&str; 7] = [
    "type",
    "dimension",
    "unit",
    "operator",
    "function",
    "variable",
    "enumMember",
];

/// The index of a token kind in [`TOKEN_TYPES`].
fn token_type(kind: TokenKind) -> u32 {
    match kind {
        TokenKind::Type => 0,
        TokenKind::Dimension => 1,
        TokenKind::Unit => 2,
        TokenKind::Operator => 3,
        TokenKind::Function => 4,
        TokenKind::Variable => 5,
        TokenKind::Constructor => 6,
    }
}

/// Encodes tokens as the protocol's delta stream.
///
/// The protocol sends five numbers per token — line delta, character delta,
/// length, token type and modifiers — with the deltas relative to the previous
/// token. The analysis crate returns absolute ranges in UTF-16 units, so the
/// conversion is the delta computation and nothing else.
#[must_use]
pub fn semantic_tokens_json(tokens: &[SemanticToken]) -> Value {
    let mut data: Vec<u32> = Vec::with_capacity(tokens.len() * 5);
    let mut previous_line = 0u32;
    let mut previous_column = 0u32;
    for token in tokens {
        let line = token.range.start.line;
        let column = token.range.start.column;
        let delta_line = line - previous_line;
        let delta_column = if delta_line == 0 {
            column.saturating_sub(previous_column)
        } else {
            column
        };
        let length = token.range.end.column.saturating_sub(column).max(1);
        data.extend([delta_line, delta_column, length, token_type(token.kind), 0]);
        previous_line = line;
        previous_column = column;
    }
    json!({ "data": data })
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
        "typeDefinitionProvider": true,
        "documentSymbolProvider": true,
        "documentFormattingProvider": true,
        "codeActionProvider": true,
        "workspaceSymbolProvider": true,
        "referencesProvider": true,
        "renameProvider": { "prepareProvider": true },
        "signatureHelpProvider": {
            "triggerCharacters": ["(", ","],
            "retriggerCharacters": [","],
        },
        "semanticTokensProvider": {
            "legend": {
                "tokenTypes": TOKEN_TYPES,
                "tokenModifiers": [],
            },
            "full": true,
            "range": false,
        },
        // Not protocol vocabulary the client acts on: a statement of what the
        // server does, so a client author can tell why an unsaved buffer keeps
        // its imports and why repeated requests are cheap.
        "experimental": {
            "multiModuleAnalysis": {
                "savedFiles": true,
                "unsavedBuffers": "entry-overlay, imports resolved from disk",
            },
            "analysisCache": {
                "strategy": "entry content hash + file mtime/length",
                "capacity": aevia_analysis::cache::CAPACITY,
            },
        },
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
    fn a_file_uri_maps_to_its_path() {
        let path = path_from_uri("file:///C:/aevia/src/A.aev").expect("a file URI");
        assert_eq!(
            path.to_string_lossy().replace('\\', "/"),
            "C:/aevia/src/A.aev"
        );
        // Percent escapes are decoded: a path may contain a space.
        let path = path_from_uri("file:///tmp/a%20b.aev").expect("a file URI");
        assert!(path.to_string_lossy().ends_with("a b.aev"), "{path:?}");
        // Anything else is a virtual document, not a file to load.
        assert_eq!(path_from_uri("untitled:Untitled-1"), None);
        assert_eq!(path_from_uri("https://example.com/x.aev"), None);
    }

    #[test]
    fn a_path_becomes_a_file_uri() {
        assert_eq!(
            uri_from_path(std::path::Path::new("C:/aevia/src/A.aev")),
            "file:///C:/aevia/src/A.aev"
        );
        assert_eq!(
            uri_from_path(std::path::Path::new("/home/me/A.aev")),
            "file:///home/me/A.aev"
        );
    }

    #[test]
    fn a_location_in_another_module_is_reported_as_a_uri() {
        let range = Range::new(Position::new(1, 0), Position::new(1, 4));
        let from_path = location_json(&Location {
            document: "C:/aevia/src/A.aev".to_string(),
            range,
        });
        assert_eq!(from_path["uri"], "file:///C:/aevia/src/A.aev");
        // A name that is already a URI is passed through unchanged.
        let from_uri = location_json(&Location {
            document: "file:///a.aev".to_string(),
            range,
        });
        assert_eq!(from_uri["uri"], "file:///a.aev");
    }

    #[test]
    fn capabilities_claim_only_what_is_implemented() {
        let capabilities = capabilities();
        assert_eq!(capabilities["textDocumentSync"]["change"], 2);
        assert_eq!(capabilities["textDocumentSync"]["save"], false);
        assert_eq!(capabilities["positionEncoding"], "utf-16");
        assert_eq!(capabilities["completionProvider"]["resolveProvider"], false);
        assert_eq!(capabilities["codeActionProvider"], true);
        assert_eq!(capabilities["typeDefinitionProvider"], true);
        assert_eq!(capabilities["workspaceSymbolProvider"], true);
        assert_eq!(capabilities["referencesProvider"], true);
        assert_eq!(capabilities["renameProvider"]["prepareProvider"], true);
        assert_eq!(
            capabilities["signatureHelpProvider"]["triggerCharacters"],
            serde_json::json!(["(", ","])
        );
        assert_eq!(
            capabilities["signatureHelpProvider"]["retriggerCharacters"],
            serde_json::json!([","])
        );
        assert_eq!(capabilities["semanticTokensProvider"]["full"], true);
        assert_eq!(
            capabilities["semanticTokensProvider"]["legend"]["tokenTypes"],
            serde_json::json!(TOKEN_TYPES)
        );
        // The multi-module path, its unsaved-buffer overlay, and the analysis
        // cache are stated, so a client author knows what the server does
        // rather than inferring it from a buffer's diagnostics.
        assert_eq!(
            capabilities["experimental"]["multiModuleAnalysis"]["savedFiles"],
            true
        );
        assert_eq!(
            capabilities["experimental"]["multiModuleAnalysis"]["unsavedBuffers"],
            "entry-overlay, imports resolved from disk"
        );
        assert_eq!(
            capabilities["experimental"]["analysisCache"]["capacity"],
            aevia_analysis::cache::CAPACITY
        );
    }

    #[test]
    fn a_code_action_carries_a_workspace_edit_for_its_uri() {
        let action = CodeAction {
            title: "Add type annotation `x: Float`".to_string(),
            kind: CodeActionKind::QuickFix,
            edits: vec![TextEdit {
                range: Range::new(Position::new(0, 5), Position::new(0, 5)),
                new_text: ": Float".to_string(),
            }],
        };
        let value = code_actions_json("file:///a.aev", &[action]);
        assert_eq!(value[0]["kind"], "quickfix");
        assert_eq!(value[0]["edit"]["changes"]["file:///a.aev"][0]["newText"], ": Float");
    }

    #[test]
    fn semantic_tokens_are_delta_encoded() {
        // Two tokens on one line, then one on the next: the second line resets
        // the character delta to its absolute column, which is the part a
        // naive encoder gets wrong.
        let tokens = [
            SemanticToken {
                range: Range::new(Position::new(0, 4), Position::new(0, 5)),
                kind: TokenKind::Variable,
            },
            SemanticToken {
                range: Range::new(Position::new(0, 10), Position::new(0, 12)),
                kind: TokenKind::Unit,
            },
            SemanticToken {
                range: Range::new(Position::new(1, 2), Position::new(1, 6)),
                kind: TokenKind::Type,
            },
        ];
        let value = semantic_tokens_json(&tokens);
        assert_eq!(value["data"], serde_json::json!([0, 4, 1, 5, 0, 0, 6, 2, 2, 0, 1, 2, 4, 0, 0]));
    }

    #[test]
    fn a_workspace_symbol_names_its_file_as_a_uri() {
        let symbol = aevia_analysis::Symbol {
            name: "sqrt".to_string(),
            kind: SymbolKind::Function,
            range: Range::new(Position::new(2, 3), Position::new(2, 7)),
            selection: Range::new(Position::new(2, 3), Position::new(2, 7)),
            detail: Some("Real[L] -> Real[L]".to_string()),
            children: Vec::new(),
        };
        let entry = aevia_analysis::WorkspaceSymbol {
            document: "C:/aevia/src/A.aev".to_string(),
            symbol,
        };
        let value = workspace_symbols_json(&[entry]);
        assert_eq!(value[0]["name"], "sqrt");
        assert_eq!(value[0]["kind"], 12);
        assert_eq!(
            value[0]["location"]["uri"],
            "file:///C:/aevia/src/A.aev"
        );
        assert_eq!(
            value[0]["location"]["range"]["start"],
            serde_json::json!({"line": 2, "character": 3})
        );
    }

    #[test]
    fn a_rename_is_one_document_change_per_file_in_order() {
        let range = |line, start, end| {
            Range::new(Position::new(line, start), Position::new(line, end))
        };
        let edits = [
            FileEdit {
                document: "C:/aevia/src/A.aev".to_string(),
                edit: TextEdit {
                    range: range(2, 3, 7),
                    new_text: "amount".to_string(),
                },
            },
            FileEdit {
                document: "C:/aevia/src/B.aev".to_string(),
                edit: TextEdit {
                    range: range(4, 15, 19),
                    new_text: "amount".to_string(),
                },
            },
            FileEdit {
                document: "C:/aevia/src/A.aev".to_string(),
                edit: TextEdit {
                    range: range(6, 1, 5),
                    new_text: "amount".to_string(),
                },
            },
        ];
        let value = rename_json(&edits);
        let changes = value["documentChanges"]
            .as_array()
            .expect("documentChanges");
        assert_eq!(changes.len(), 2, "{value:#?}");
        assert_eq!(changes[0]["textDocument"]["uri"], "file:///C:/aevia/src/A.aev");
        assert_eq!(changes[0]["textDocument"]["version"], Value::Null);
        assert_eq!(changes[0]["edits"].as_array().map(Vec::len), Some(2));
        assert_eq!(changes[0]["edits"][0]["newText"], "amount");
        assert_eq!(
            changes[0]["edits"][0]["range"]["start"],
            serde_json::json!({"line": 2, "character": 3})
        );
        assert_eq!(changes[1]["textDocument"]["uri"], "file:///C:/aevia/src/B.aev");
    }

    #[test]
    fn references_and_prepare_rename_write_their_shapes() {
        let range = Range::new(Position::new(1, 2), Position::new(1, 6));
        let locations = [Location {
            document: "C:/aevia/src/A.aev".to_string(),
            range,
        }];
        let value = references_json(&locations);
        assert_eq!(value.as_array().map(Vec::len), Some(1));
        assert_eq!(value[0]["uri"], "file:///C:/aevia/src/A.aev");
        assert_eq!(
            value[0]["range"]["end"],
            serde_json::json!({"line": 1, "character": 6})
        );

        let placeholder = RenamePlaceholder {
            range,
            placeholder: "sqrt".to_string(),
        };
        let value = prepare_rename_json(&placeholder);
        assert_eq!(value["placeholder"], "sqrt");
        assert_eq!(
            value["range"]["start"],
            serde_json::json!({"line": 1, "character": 2})
        );
    }

    #[test]
    fn signature_help_writes_labels_parameters_and_markdown() {
        let help = SignatureHelp {
            signatures: vec![
                aevia_analysis::SignatureInfo {
                    label: "sum: (Tensor<1, Float>) -> Float".to_string(),
                    parameters: vec!["Tensor<1, Float>".to_string()],
                    documentation: None,
                },
                aevia_analysis::SignatureInfo {
                    label: "sum: (Tensor<1, Float>, Int) -> Float".to_string(),
                    parameters: vec!["Tensor<1, Float>".to_string(), "Int".to_string()],
                    documentation: Some("Reduce along an axis.".to_string()),
                },
            ],
            active_signature: 1,
            active_parameter: 0,
        };
        let value = signature_help_json(&help);
        assert_eq!(value["activeSignature"], 1);
        assert_eq!(value["activeParameter"], 0);
        assert_eq!(value["signatures"].as_array().map(Vec::len), Some(2));
        assert_eq!(
            value["signatures"][0]["parameters"][0]["label"],
            "Tensor<1, Float>"
        );
        assert_eq!(
            value["signatures"][1]["documentation"]["kind"],
            "markdown"
        );
        assert_eq!(
            value["signatures"][1]["documentation"]["value"],
            "Reduce along an axis."
        );
    }
}
