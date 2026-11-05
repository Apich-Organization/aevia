//! The end-to-end protocol test.
//!
//! A scripted byte stream goes in at one end of [`aevia_lsp::serve`] and a
//! scripted byte stream comes out at the other. Nothing in between is stubbed:
//! the framing, the JSON, the state machine, the document store and the real
//! compiler all run.
//!
//! The assertions parse the replies with `serde_json` and look at values, never
//! at the bytes: field order and whitespace are not part of the protocol.

use aevia_lsp::Exit;
use serde_json::Value;
use std::io::{BufReader, Cursor, Write};
use std::sync::{Arc, Mutex};

/// A writer a test can read back after the session ends.
#[derive(Clone)]
struct Shared(Arc<Mutex<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("lock").extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Frames `messages` the way a client does.
fn script(messages: &[&str]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for message in messages {
        bytes.extend_from_slice(format!("Content-Length: {}\r\n\r\n", message.len()).as_bytes());
        bytes.extend_from_slice(message.as_bytes());
    }
    bytes
}

/// Runs one session and returns its exit and the messages it sent.
fn run(messages: &[&str]) -> (Exit, Vec<Value>) {
    let output = Shared(Arc::new(Mutex::new(Vec::new())));
    let exit = aevia_lsp::serve(Cursor::new(script(messages)), output.clone());
    let bytes = output.0.lock().expect("lock").clone();
    let mut reader = aevia_lsp::transport::Reader::new(BufReader::new(Cursor::new(bytes)));
    let mut replies = Vec::new();
    while let Some(message) = reader.next_message().expect("a framed reply") {
        replies.push(message);
    }
    (exit, replies)
}

/// The reply carrying `id`.
fn reply(replies: &[Value], id: i64) -> &Value {
    replies
        .iter()
        .find(|message| message.get("id").and_then(Value::as_i64) == Some(id))
        .unwrap_or_else(|| panic!("no reply to request {id}: {replies:#?}"))
}

/// Every `publishDiagnostics` for `uri`, in order.
fn published<'a>(replies: &'a [Value], uri: &str) -> Vec<&'a Value> {
    replies
        .iter()
        .filter(|message| {
            message.get("method").and_then(Value::as_str)
                == Some("textDocument/publishDiagnostics")
                && message["params"]["uri"] == uri
        })
        .collect()
}

const URI: &str = "file:///playground.aev";

#[test]
fn a_full_session_speaks_the_protocol() {
    // A spacing problem on line 1 and a dimension error on line 2.
    let did_open = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{URI}","languageId":"aevia","version":1,"text":"let  g=9.80665 m/s^2\nlet bad = 1 m + 1\n"}}}}}}"#
    );
    let (exit, replies) = run(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"rootUri":null,"capabilities":{}}}"#,
        r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
        &did_open,
        &format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{{"textDocument":{{"uri":"{URI}"}},"position":{{"line":0,"character":5}}}}}}"#
        ),
        &format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"textDocument/completion","params":{{"textDocument":{{"uri":"{URI}"}},"position":{{"line":0,"character":5}}}}}}"#
        ),
        &format!(
            r#"{{"jsonrpc":"2.0","id":4,"method":"textDocument/definition","params":{{"textDocument":{{"uri":"{URI}"}},"position":{{"line":0,"character":5}}}}}}"#
        ),
        &format!(
            r#"{{"jsonrpc":"2.0","id":5,"method":"textDocument/documentSymbol","params":{{"textDocument":{{"uri":"{URI}"}}}}}}"#
        ),
        &format!(
            r#"{{"jsonrpc":"2.0","id":6,"method":"textDocument/formatting","params":{{"textDocument":{{"uri":"{URI}"}},"options":{{"tabSize":4,"insertSpaces":true}}}}}}"#
        ),
        &format!(
            r#"{{"jsonrpc":"2.0","id":7,"method":"textDocument/implementation","params":{{"textDocument":{{"uri":"{URI}"}},"position":{{"line":0,"character":5}}}}}}"#
        ),
        &format!(
            r#"{{"jsonrpc":"2.0","id":9,"method":"textDocument/codeAction","params":{{"textDocument":{{"uri":"{URI}"}},"range":{{"start":{{"line":1,"character":12}},"end":{{"line":1,"character":12}}}},"context":{{"diagnostics":[]}}}}}}"#
        ),
        &format!(
            r#"{{"jsonrpc":"2.0","id":10,"method":"textDocument/semanticTokens/full","params":{{"textDocument":{{"uri":"{URI}"}}}}}}"#
        ),
        r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":99}}"#,
        r#"{"jsonrpc":"2.0","id":8,"method":"shutdown","params":null}"#,
        r#"{"jsonrpc":"2.0","method":"exit"}"#,
    ]);

    assert_eq!(exit, Exit::Graceful, "{replies:#?}");

    // initialize: honest capabilities, and only what is implemented.
    let initialize = reply(&replies, 1);
    let capabilities = &initialize["result"]["capabilities"];
    assert_eq!(initialize["jsonrpc"], "2.0");
    assert_eq!(capabilities["textDocumentSync"]["openClose"], true);
    assert_eq!(capabilities["textDocumentSync"]["change"], 2);
    assert_eq!(capabilities["textDocumentSync"]["save"], false);
    assert_eq!(capabilities["hoverProvider"], true);
    assert_eq!(capabilities["completionProvider"]["resolveProvider"], false);
    assert_eq!(capabilities["definitionProvider"], true);
    assert_eq!(capabilities["typeDefinitionProvider"], true);
    assert_eq!(capabilities["documentSymbolProvider"], true);
    assert_eq!(capabilities["documentFormattingProvider"], true);
    assert_eq!(capabilities["codeActionProvider"], true);
    assert_eq!(capabilities["semanticTokensProvider"]["full"], true);
    let legend = capabilities["semanticTokensProvider"]["legend"]["tokenTypes"]
        .as_array()
        .expect("a legend");
    for token_type in ["type", "dimension", "unit", "operator"] {
        assert!(
            legend.iter().any(|entry| entry == token_type),
            "`{token_type}` is missing from {legend:?}"
        );
    }
    assert_eq!(capabilities["positionEncoding"], "utf-16");
    assert_eq!(initialize["result"]["serverInfo"]["name"], "aevia-lsp");

    // didOpen published the dimension error, at the right range.
    let diagnostics = published(&replies, URI);
    assert_eq!(diagnostics.len(), 1, "{replies:#?}");
    let published_here = &diagnostics[0]["params"];
    assert_eq!(published_here["version"], 1);
    let items = published_here["diagnostics"].as_array().expect("an array");
    assert_eq!(items.len(), 1, "{items:#?}");
    assert_eq!(items[0]["code"], "E0401");
    assert_eq!(items[0]["severity"], 1);
    assert_eq!(items[0]["source"], "aevia");
    assert_eq!(items[0]["range"]["start"], serde_json::json!({"line":1,"character":10}));
    assert_eq!(items[0]["range"]["end"], serde_json::json!({"line":1,"character":17}));
    assert!(
        items[0]["message"]
            .as_str()
            .is_some_and(|text| text.contains("dimension mismatch")),
        "{items:#?}"
    );
    assert_eq!(items[0]["relatedInformation"].as_array().map(Vec::len), Some(2));

    // hover: the exported scheme of `g`.
    let hover = reply(&replies, 2);
    let contents = hover["result"]["contents"]["value"]
        .as_str()
        .unwrap_or_else(|| panic!("no hover: {hover:#?}"));
    assert!(contents.contains("g:"), "{contents}");
    assert!(contents.contains("Float"), "{contents}");
    assert_eq!(hover["result"]["range"]["start"], serde_json::json!({"line":0,"character":5}));

    // completion: the module's names and the prelude.
    let completion = reply(&replies, 3);
    let labels: Vec<&str> = completion["result"]
        .as_array()
        .expect("an array")
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    assert!(labels.contains(&"g"), "{labels:?}");
    assert!(labels.contains(&"sqrt"), "{labels:?}");
    let sqrt = completion["result"]
        .as_array()
        .expect("an array")
        .iter()
        .find(|item| item["label"] == "sqrt")
        .expect("sqrt");
    assert_eq!(sqrt["kind"], 3);

    // definition: `g` is declared where it is written.
    let definition = reply(&replies, 4);
    assert_eq!(definition["result"]["uri"], URI);
    assert_eq!(
        definition["result"]["range"],
        serde_json::json!({"start":{"line":0,"character":5},"end":{"line":0,"character":6}})
    );

    // documentSymbol: two top-level bindings.
    let symbols = reply(&replies, 5)["result"].as_array().expect("an array").clone();
    let names: Vec<&str> = symbols.iter().filter_map(|s| s["name"].as_str()).collect();
    assert_eq!(names, ["g", "bad"]);
    assert_eq!(symbols[0]["kind"], 13);

    // formatting: one full-document edit, because line 1 is not formatted.
    let formatting = reply(&replies, 6);
    let edits = formatting["result"].as_array().expect("an array");
    assert_eq!(edits.len(), 1);
    // The formatter puts a blank line between top-level declarations.
    assert_eq!(
        edits[0]["newText"],
        "let g = 9.80665 m/s^2\n\nlet bad = 1 m + 1\n"
    );
    assert_eq!(edits[0]["range"]["start"], serde_json::json!({"line":0,"character":0}));
    assert_eq!(edits[0]["range"]["end"], serde_json::json!({"line":2,"character":0}));

    // An unknown request is `-32601`; the session survives it.
    let unknown = reply(&replies, 7);
    assert_eq!(unknown["error"]["code"], -32601);

    // codeAction: the dimension error on line 2 has a fix, and the fix is a
    // workspace edit whose text the server compiled before offering it.
    let code_actions = reply(&replies, 9);
    let actions = code_actions["result"].as_array().expect("an array");
    let fix = actions
        .iter()
        .find(|action| action["title"].as_str().is_some_and(|title| title.contains("Add `m`")))
        .unwrap_or_else(|| panic!("no dimension fix: {actions:#?}"));
    assert_eq!(fix["kind"], "quickfix");
    let edits = fix["edit"]["changes"][URI].as_array().expect("edits");
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0]["newText"], " m");
    assert_eq!(edits[0]["range"]["start"], serde_json::json!({"line":1,"character":17}));

    // semanticTokens/full: the unit `m` of `m/s^2` is a unit token. The
    // protocol's delta stream is checked structurally here; the analysis crate
    // tests the classifications themselves.
    let semantic = reply(&replies, 10);
    let data: &Vec<Value> = semantic["result"]["data"]
        .as_array()
        .unwrap_or_else(|| panic!("no semantic tokens: {semantic:#?}"));
    assert!(!data.is_empty());
    assert_eq!(data.len() % 5, 0, "{data:?}");
    let unit_index = legend
        .iter()
        .position(|entry| entry == "unit")
        .expect("a unit in the legend");
    assert!(
        data.chunks(5)
            .any(|token| token[3].as_u64() == Some(unit_index as u64)),
        "no unit token in {data:?}"
    );

    // shutdown answered with null.
    let shutdown = reply(&replies, 8);
    assert!(shutdown["result"].is_null());
}

#[test]
fn an_incremental_change_republishes_diagnostics() {
    let did_open = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{URI}","languageId":"aevia","version":1,"text":"let bad = 1 m + 1\n"}}}}}}"#
    );
    // Replace the trailing `1` with `1 m`: the dimension error goes away.
    let did_change = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"{URI}","version":2}},"contentChanges":[{{"range":{{"start":{{"line":0,"character":16}},"end":{{"line":0,"character":17}}}},"text":"1 m"}}]}}}}"#
    );
    let (exit, replies) = run(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        &did_open,
        &did_change,
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}"#,
        r#"{"jsonrpc":"2.0","method":"exit"}"#,
    ]);
    assert_eq!(exit, Exit::Graceful);

    let diagnostics = published(&replies, URI);
    assert_eq!(diagnostics.len(), 2, "{replies:#?}");
    assert_eq!(diagnostics[0]["params"]["version"], 1);
    assert_eq!(
        diagnostics[0]["params"]["diagnostics"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(diagnostics[1]["params"]["version"], 2);
    assert_eq!(
        diagnostics[1]["params"]["diagnostics"].as_array().map(Vec::len),
        Some(0),
        "the edit fixed the error: {replies:#?}"
    );
}

#[test]
fn a_full_text_change_is_accepted_too() {
    let did_open = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{URI}","languageId":"aevia","version":1,"text":"let bad = 1 m + 1\n"}}}}}}"#
    );
    let did_change = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"{URI}","version":2}},"contentChanges":[{{"text":"let ok = 2 m\n"}}]}}}}"#
    );
    let (_, replies) = run(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        &did_open,
        &did_change,
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}"#,
        r#"{"jsonrpc":"2.0","method":"exit"}"#,
    ]);
    let diagnostics = published(&replies, URI);
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(
        diagnostics[1]["params"]["diagnostics"].as_array().map(Vec::len),
        Some(0)
    );
}

#[test]
fn closing_a_document_clears_its_diagnostics() {
    let did_open = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{URI}","languageId":"aevia","version":1,"text":"let bad = 1 m + 1\n"}}}}}}"#
    );
    let did_close = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didClose","params":{{"textDocument":{{"uri":"{URI}"}}}}}}"#
    );
    let (_, replies) = run(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        &did_open,
        &did_close,
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}"#,
        r#"{"jsonrpc":"2.0","method":"exit"}"#,
    ]);
    let diagnostics = published(&replies, URI);
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(
        diagnostics[1]["params"]["diagnostics"].as_array().map(Vec::len),
        Some(0)
    );
}

#[test]
fn a_malformed_body_is_a_parse_error_and_the_session_continues() {
    let mut stream = script(&["this is not json"]);
    stream.extend(script(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}"#,
        r#"{"jsonrpc":"2.0","method":"exit"}"#,
    ]));
    let output = Shared(Arc::new(Mutex::new(Vec::new())));
    let exit = aevia_lsp::serve(Cursor::new(stream), output.clone());
    assert_eq!(exit, Exit::Graceful);
    let bytes = output.0.lock().expect("lock").clone();
    let mut reader = aevia_lsp::transport::Reader::new(BufReader::new(Cursor::new(bytes)));
    let mut replies = Vec::new();
    while let Some(message) = reader.next_message().expect("a framed reply") {
        replies.push(message);
    }
    let parse_error = replies
        .iter()
        .find(|message| message["error"]["code"] == -32700)
        .expect("a parse error");
    assert!(parse_error["id"].is_null());
    // The session is still alive: the initialize was answered.
    assert!(replies.iter().any(|message| message["id"] == 1));
}

#[test]
fn a_request_before_initialize_is_refused() {
    let (_, replies) = run(&[
        &format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"textDocument/hover","params":{{"textDocument":{{"uri":"{URI}"}},"position":{{"line":0,"character":0}}}}}}"#
        ),
        r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}"#,
        r#"{"jsonrpc":"2.0","method":"exit"}"#,
    ]);
    assert_eq!(reply(&replies, 1)["error"]["code"], -32002);
    assert!(reply(&replies, 2)["result"]["capabilities"].is_object());
}

#[test]
fn exit_without_shutdown_is_ungraceful() {
    let (exit, _) = run(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","method":"exit"}"#,
    ]);
    assert_eq!(exit, Exit::Ungraceful);
}

#[test]
fn a_type_definition_request_answers_the_type_declaration() {
    let uri = "file:///types.aev";
    let did_open = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{uri}","languageId":"aevia","version":1,"text":"type Shape = | Circle(Real[L]) | Dot\nlet s = Dot\n"}}}}}}"#
    );
    let (_, replies) = run(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        &did_open,
        &format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"textDocument/typeDefinition","params":{{"textDocument":{{"uri":"{uri}"}},"position":{{"line":1,"character":8}}}}}}"#
        ),
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}"#,
        r#"{"jsonrpc":"2.0","method":"exit"}"#,
    ]);
    let location = reply(&replies, 2);
    assert_eq!(location["result"]["uri"], uri);
    assert_eq!(
        location["result"]["range"],
        serde_json::json!({
            "start": {"line": 0, "character": 5},
            "end": {"line": 0, "character": 10},
        })
    );
}

#[test]
fn a_request_for_an_unopened_document_answers_null() {
    let (_, replies) = run(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{"textDocument":{"uri":"file:///nowhere.aev"},"position":{"line":0,"character":0}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}"#,
        r#"{"jsonrpc":"2.0","method":"exit"}"#,
    ]);
    assert!(reply(&replies, 2)["result"].is_null());
}

// --- multi-module analysis over a real package ----------------------------

/// A temporary package that removes itself.
struct TempPackage {
    root: std::path::PathBuf,
}

impl TempPackage {
    fn new(name: &str) -> Self {
        let mut root = std::env::temp_dir();
        root.push(format!("aevia-lsp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).expect("create package");
        std::fs::write(
            root.join("aevia.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nentry = \"src/B.aev\"\n",
        )
        .expect("write manifest");
        Self { root }
    }

    fn write(&self, name: &str, source: &str) -> std::path::PathBuf {
        let path = self.root.join(name);
        std::fs::write(&path, source).expect("write source");
        path
    }

    fn path(&self, name: &str) -> std::path::PathBuf {
        self.root.join(name)
    }
}

impl Drop for TempPackage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Runs a scripted session whose messages are built as JSON values.
fn run_json(messages: Vec<Value>) -> (Exit, Vec<Value>) {
    let owned: Vec<String> = messages.into_iter().map(|message| message.to_string()).collect();
    let borrowed: Vec<&str> = owned.iter().map(String::as_str).collect();
    run(&borrowed)
}

/// A package whose `B` imports `A`, and the URIs/positions a client would use.
fn imported_package(name: &str) -> (TempPackage, String, String) {
    let package = TempPackage::new(name);
    package.write(
        "src/A.aev",
        "module A\n\nfn sqrt(x: Real[L]): Real[L] = x\n",
    );
    let b_source = "module B\n\nimport A\n\nlet length = A.sqrt(2.0 m)\n";
    package.write("src/B.aev", b_source);
    let b_uri = aevia_lsp::protocol::uri_from_path(&package.path("src/B.aev"));
    let a_uri = aevia_lsp::protocol::uri_from_path(&package.path("src/A.aev"));
    (package, b_uri, a_uri)
}

#[test]
fn a_saved_file_is_analysed_with_its_imports() {
    let (_package, b_uri, a_uri) = imported_package("multi");
    let b_source = "module B\n\nimport A\n\nlet length = A.sqrt(2.0 m)\n";
    let position = serde_json::json!({ "line": 4, "character": 16 });
    let (exit, replies) = run_json(vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": {"textDocument": {
                "uri": b_uri, "languageId": "aevia", "version": 1, "text": b_source
            }}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "textDocument/hover",
            "params": {"textDocument": {"uri": b_uri}, "position": position.clone()}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 3, "method": "textDocument/completion",
            "params": {"textDocument": {"uri": b_uri}, "position": {"line": 4, "character": 0}}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 4, "method": "textDocument/definition",
            "params": {"textDocument": {"uri": b_uri}, "position": position}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 5, "method": "workspace/symbol",
            "params": {"query": "sqrt"}
        }),
        serde_json::json!({"jsonrpc":"2.0","id":6,"method":"shutdown","params":null}),
        serde_json::json!({"jsonrpc":"2.0","method":"exit"}),
    ]);
    assert_eq!(exit, Exit::Graceful, "{replies:#?}");

    // The saved file and its import both compile, so no false unbound-name or
    // missing-module diagnostics.
    let published = published(&replies, &b_uri);
    assert_eq!(published.len(), 1, "{replies:#?}");
    assert_eq!(
        published[0]["params"]["diagnostics"].as_array().map(Vec::len),
        Some(0),
        "{replies:#?}"
    );

    // Hover on the imported qualified name.
    let hover = reply(&replies, 2);
    let contents = hover["result"]["contents"]["value"]
        .as_str()
        .unwrap_or_else(|| panic!("no hover: {hover:#?}"));
    assert!(contents.contains("sqrt"), "{contents}");
    assert!(contents.contains("Real[L]"), "{contents}");

    // Completion includes the module and its export.
    let labels: Vec<&str> = reply(&replies, 3)["result"]
        .as_array()
        .expect("completion array")
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    assert!(labels.contains(&"A.sqrt"), "{labels:?}");

    // Definition jumps into the imported file, with that file's range.
    let definition = reply(&replies, 4);
    assert_eq!(definition["result"]["uri"], a_uri, "{definition:#?}");
    assert_eq!(
        definition["result"]["range"],
        serde_json::json!({
            "start": {"line": 2, "character": 3},
            "end": {"line": 2, "character": 7},
        })
    );

    // workspace/symbol sees declarations of the imported module, with that
    // module's file and range.
    let workspace = reply(&replies, 5);
    let symbols = workspace["result"]
        .as_array()
        .unwrap_or_else(|| panic!("no workspace symbols: {workspace:#?}"));
    assert_eq!(symbols.len(), 1, "{symbols:#?}");
    assert_eq!(symbols[0]["name"], "sqrt");
    assert_eq!(symbols[0]["kind"], 12);
    assert_eq!(symbols[0]["location"]["uri"], a_uri);
    assert_eq!(
        symbols[0]["location"]["range"]["start"],
        serde_json::json!({"line": 2, "character": 3})
    );
}

#[test]
fn a_workspace_symbol_request_with_no_open_documents_is_empty() {
    let (exit, replies) = run_json(vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "workspace/symbol",
            "params": {"query": ""}
        }),
        serde_json::json!({"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}),
        serde_json::json!({"jsonrpc":"2.0","method":"exit"}),
    ]);
    assert_eq!(exit, Exit::Graceful, "{replies:#?}");
    assert_eq!(
        reply(&replies, 2)["result"].as_array().map(Vec::len),
        Some(0),
        "{replies:#?}"
    );
}

#[test]
fn an_unsaved_buffer_keeps_its_imports_and_reflects_edits() {
    let (_package, b_uri, _a_uri) = imported_package("unsaved");
    let b_source = "module B\n\nimport A\n\nlet length = A.sqrt(2.0 m)\n";
    let edited = format!("{b_source}\nlet extra = A.sqrt(3.0 m)\n");
    let broken = format!("{edited}\nlet bad: Int = A.sqrt(1.0 m)\n");
    let (exit, replies) = run_json(vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": {"textDocument": {
                "uri": b_uri, "languageId": "aevia", "version": 1, "text": b_source
            }}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didChange",
            "params": {"textDocument": {"uri": b_uri, "version": 2},
                       "contentChanges": [{"text": edited}]}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didChange",
            "params": {"textDocument": {"uri": b_uri, "version": 3},
                       "contentChanges": [{"text": broken}]}
        }),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}),
        serde_json::json!({"jsonrpc":"2.0","method":"exit"}),
    ]);
    assert_eq!(exit, Exit::Graceful, "{replies:#?}");
    let published = published(&replies, &b_uri);
    assert_eq!(published.len(), 3, "{replies:#?}");
    assert_eq!(
        published[0]["params"]["diagnostics"].as_array().map(Vec::len),
        Some(0),
        "the saved file is clean"
    );
    // The unsaved buffer keeps its imports: the edit is valid, so the buffer is
    // still clean — no false unbound-name diagnostics.
    assert_eq!(
        published[1]["params"]["diagnostics"].as_array().map(Vec::len),
        Some(0),
        "a dirty buffer with valid imports was not clean: {replies:#?}"
    );
    // An unsaved edit that is wrong is reported, but as the real error, not as
    // a missing import.
    let diagnostics = published[2]["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics");
    assert!(!diagnostics.is_empty(), "the unsaved edit was not checked");
    let codes: Vec<&str> = diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic["code"].as_str())
        .collect();
    assert!(
        !codes.iter().any(|code| *code == "E0301" || *code == "E0607"),
        "a dirty buffer with valid imports reported import errors: {codes:?}"
    );
}

/// The byte offset of a protocol position in ASCII test text.
fn offset_at(text: &str, line: u64, character: u64) -> usize {
    let line_start: usize = text
        .split_inclusive('\n')
        .take(line as usize)
        .map(str::len)
        .sum();
    line_start + character as usize
}

/// Applies JSON text edits to one file's text, last edit first.
fn apply_changes(text: &str, edits: &[Value]) -> String {
    let mut replacements: Vec<(usize, usize, String)> = edits
        .iter()
        .map(|edit| {
            let start = edit["range"]["start"].clone();
            let end = edit["range"]["end"].clone();
            let start = offset_at(
                text,
                start["line"].as_u64().expect("line"),
                start["character"].as_u64().expect("character"),
            );
            let end = offset_at(
                text,
                end["line"].as_u64().expect("line"),
                end["character"].as_u64().expect("character"),
            );
            (
                start,
                end,
                edit["newText"].as_str().expect("newText").to_string(),
            )
        })
        .collect();
    replacements.sort_by(|left, right| right.0.cmp(&left.0));
    let mut out = text.to_string();
    for (start, end, new_text) in replacements {
        out.replace_range(start..end, &new_text);
    }
    out
}

#[test]
fn references_rename_and_its_edits_cross_files() {
    let (package, b_uri, a_uri) = imported_package("references-rename");
    let b_source = "module B\n\nimport A\n\nlet length = A.sqrt(2.0 m)\n";
    let a_source = "module A\n\nfn sqrt(x: Real[L]): Real[L] = x\n";
    let position = serde_json::json!({ "line": 4, "character": 16 });
    let (exit, replies) = run_json(vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": {"textDocument": {
                "uri": b_uri, "languageId": "aevia", "version": 1, "text": b_source
            }}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "textDocument/references",
            "params": {"textDocument": {"uri": b_uri}, "position": position.clone(),
                       "context": {"includeDeclaration": true}}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 3, "method": "textDocument/prepareRename",
            "params": {"textDocument": {"uri": b_uri}, "position": position}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 4, "method": "textDocument/rename",
            "params": {"textDocument": {"uri": b_uri},
                       "position": {"line": 4, "character": 16}, "newName": "magnitude"}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 5, "method": "textDocument/references",
            "params": {"textDocument": {"uri": b_uri},
                       "position": {"line": 4, "character": 16},
                       "context": {"includeDeclaration": false}}
        }),
        serde_json::json!({"jsonrpc":"2.0","id":6,"method":"shutdown","params":null}),
        serde_json::json!({"jsonrpc":"2.0","method":"exit"}),
    ]);
    assert_eq!(exit, Exit::Graceful, "{replies:#?}");

    // References: the declaration in A and the use in B, each in its file.
    let references = reply(&replies, 2)["result"]
        .as_array()
        .unwrap_or_else(|| panic!("no references: {replies:#?}"))
        .clone();
    assert_eq!(references.len(), 2, "{references:#?}");
    let uris: Vec<&str> = references
        .iter()
        .filter_map(|location| location["uri"].as_str())
        .collect();
    assert!(uris.contains(&a_uri.as_str()), "{uris:?}");
    assert!(uris.contains(&b_uri.as_str()), "{uris:?}");
    let declaration = references
        .iter()
        .find(|location| location["uri"] == a_uri)
        .expect("the declaration in A");
    assert_eq!(
        declaration["range"],
        serde_json::json!({
            "start": {"line": 2, "character": 3},
            "end": {"line": 2, "character": 7},
        })
    );
    let use_in_b = references
        .iter()
        .find(|location| location["uri"] == b_uri)
        .expect("the use in B");
    assert_eq!(
        use_in_b["range"],
        serde_json::json!({
            "start": {"line": 4, "character": 15},
            "end": {"line": 4, "character": 19},
        })
    );

    // `includeDeclaration: false` drops the declaration.
    assert_eq!(
        reply(&replies, 5)["result"].as_array().map(Vec::len),
        Some(1),
        "{replies:#?}"
    );

    // prepareRename names the occurrence under the cursor.
    let prepare = reply(&replies, 3);
    assert_eq!(prepare["result"]["placeholder"], "sqrt", "{prepare:#?}");
    assert_eq!(
        prepare["result"]["range"]["start"],
        serde_json::json!({"line": 4, "character": 15})
    );

    // rename: a WorkspaceEdit with one documentChange per file.
    let rename = reply(&replies, 4);
    let changes = rename["result"]["documentChanges"]
        .as_array()
        .unwrap_or_else(|| panic!("no documentChanges: {rename:#?}"))
        .clone();
    assert_eq!(changes.len(), 2, "{changes:#?}");
    let a_change = changes
        .iter()
        .find(|change| change["textDocument"]["uri"] == a_uri)
        .expect("the change in A");
    let b_change = changes
        .iter()
        .find(|change| change["textDocument"]["uri"] == b_uri)
        .expect("the change in B");
    let new_a = apply_changes(
        a_source,
        a_change["edits"].as_array().expect("A edits"),
    );
    let new_b = apply_changes(
        b_source,
        b_change["edits"].as_array().expect("B edits"),
    );
    assert!(new_a.contains("fn magnitude(x: Real[L])"), "{new_a}");
    assert!(!new_a.contains("sqrt"), "{new_a}");
    assert_eq!(new_b.matches("A.magnitude(").count(), 1, "{new_b}");

    // Apply the edit to the package and re-analyse: a second session over the
    // renamed files must be clean and the old name must be gone.
    package.write("src/A.aev", &new_a);
    package.write("src/B.aev", &new_b);
    let (exit, replies) = run_json(vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": {"textDocument": {
                "uri": b_uri, "languageId": "aevia", "version": 2, "text": new_b
            }}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "textDocument/hover",
            "params": {"textDocument": {"uri": b_uri},
                       "position": {"line": 4, "character": 17}}
        }),
        serde_json::json!({"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}),
        serde_json::json!({"jsonrpc":"2.0","method":"exit"}),
    ]);
    assert_eq!(exit, Exit::Graceful, "{replies:#?}");
    let published = published(&replies, &b_uri);
    assert_eq!(published.len(), 1, "{replies:#?}");
    assert_eq!(
        published[0]["params"]["diagnostics"].as_array().map(Vec::len),
        Some(0),
        "the renamed package does not check: {replies:#?}"
    );
    let hover = reply(&replies, 2)["result"]["contents"]["value"]
        .as_str()
        .unwrap_or_else(|| panic!("no hover: {replies:#?}"));
    assert!(hover.contains("magnitude"), "{hover}");
}

#[test]
fn rename_refusals_are_errors_with_reasons() {
    let package = TempPackage::new("rename-refusals");
    let a_source = "module A\n\nfn sqrt(x: Real[L]): Real[L] = x\nfn magnitude(): Int = 1\n";
    let b_source = "module B\n\nimport A\n\nlet length = A.sqrt(2.0 m)\n";
    package.write("src/A.aev", a_source);
    package.write("src/B.aev", b_source);
    let b_uri = aevia_lsp::protocol::uri_from_path(&package.path("src/B.aev"));
    let position = serde_json::json!({ "line": 4, "character": 16 });
    let (exit, replies) = run_json(vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": {"textDocument": {
                "uri": b_uri, "languageId": "aevia", "version": 1, "text": b_source
            }}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "textDocument/rename",
            "params": {"textDocument": {"uri": b_uri}, "position": position,
                       "newName": "magnitude"}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 3, "method": "textDocument/rename",
            "params": {"textDocument": {"uri": b_uri},
                       "position": {"line": 4, "character": 16}, "newName": "fn"}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 4, "method": "textDocument/prepareRename",
            "params": {"textDocument": {"uri": b_uri},
                       "position": {"line": 2, "character": 7}}
        }),
        serde_json::json!({"jsonrpc":"2.0","id":5,"method":"shutdown","params":null}),
        serde_json::json!({"jsonrpc":"2.0","method":"exit"}),
    ]);
    assert_eq!(exit, Exit::Graceful, "{replies:#?}");

    let collision = reply(&replies, 2);
    assert_eq!(collision["error"]["code"], -32803, "{collision:#?}");
    assert!(
        collision["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("already declared"),
        "{collision:#?}"
    );

    let keyword = reply(&replies, 3);
    assert_eq!(keyword["error"]["code"], -32803, "{keyword:#?}");
    assert!(
        keyword["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("keyword"),
        "{keyword:#?}"
    );

    // The import's module segment is not a declaration to rename.
    let module_segment = reply(&replies, 4);
    assert_eq!(module_segment["error"]["code"], -32803, "{module_segment:#?}");
    assert!(
        module_segment["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("no declaration"),
        "{module_segment:#?}"
    );
}

/// The protocol position of the byte `delta` characters after `needle`.
fn position_of(text: &str, needle: &str, delta: usize) -> serde_json::Value {
    let offset = text.find(needle).expect("the needle is in the source") + delta;
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    serde_json::json!({ "line": line, "character": offset - line_start })
}

#[test]
fn signature_help_answers_inside_a_call_and_is_null_outside() {
    let source = "fn f(x: Int, y: Float): Int = x\nlet a = f(1, 2.0)\n";
    let (exit, replies) = run_json(vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": {"textDocument": {
                "uri": URI, "languageId": "aevia", "version": 1, "text": source
            }}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "textDocument/signatureHelp",
            "params": {"textDocument": {"uri": URI},
                       "position": position_of(source, "(1", 1),
                       "context": {"triggerKind": 2, "triggerCharacter": "("}}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 3, "method": "textDocument/signatureHelp",
            "params": {"textDocument": {"uri": URI},
                       "position": position_of(source, "2.0", 0),
                       "context": {"triggerKind": 1}}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 4, "method": "textDocument/signatureHelp",
            "params": {"textDocument": {"uri": URI},
                       "position": {"line": 0, "character": 0},
                       "context": {"triggerKind": 1}}
        }),
        serde_json::json!({"jsonrpc":"2.0","id":5,"method":"shutdown","params":null}),
        serde_json::json!({"jsonrpc":"2.0","method":"exit"}),
    ]);
    assert_eq!(exit, Exit::Graceful, "{replies:#?}");

    // Triggered by `(`, the cursor is in the first argument.
    let first = reply(&replies, 2);
    assert_eq!(first["result"]["activeParameter"], 0, "{first:#?}");
    assert_eq!(first["result"]["activeSignature"], 0);
    assert_eq!(
        first["result"]["signatures"][0]["label"],
        "f: (Int, Float) -> Int"
    );
    assert_eq!(
        first["result"]["signatures"][0]["parameters"][0]["label"],
        "Int"
    );

    // Retriggered inside the second argument.
    let second = reply(&replies, 3);
    assert_eq!(second["result"]["activeParameter"], 1, "{second:#?}");

    // Not in a call: an explicit null, not a stale list.
    assert_eq!(reply(&replies, 4)["result"], Value::Null);
}

#[test]
fn signature_help_uses_an_imported_declarations_docs() {
    let package = TempPackage::new("signature-imported");
    package.write(
        "src/A.aev",
        "module A\n\n## Magnitude of x.\nfn magnitude(x: Real[L]): Real[L] = x\n",
    );
    package.write(
        "src/B.aev",
        "module B\n\nimport A\n\nlet m = A.magnitude(2.0 m)\n",
    );
    let b_source = "module B\n\nimport A\n\nlet m = A.magnitude(2.0 m)\n";
    let b_uri = aevia_lsp::protocol::uri_from_path(&package.path("src/B.aev"));
    let (exit, replies) = run_json(vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        serde_json::json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": {"textDocument": {
                "uri": b_uri, "languageId": "aevia", "version": 1, "text": b_source
            }}
        }),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "textDocument/signatureHelp",
            "params": {"textDocument": {"uri": b_uri},
                       "position": position_of(b_source, "2.0 m", 0),
                       "context": {"triggerKind": 2, "triggerCharacter": "("}}
        }),
        serde_json::json!({"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}),
        serde_json::json!({"jsonrpc":"2.0","method":"exit"}),
    ]);
    assert_eq!(exit, Exit::Graceful, "{replies:#?}");
    let help = reply(&replies, 2);
    assert_eq!(
        help["result"]["signatures"][0]["label"],
        "A.magnitude: (Real[L]) -> Real[L]",
        "{help:#?}"
    );
    assert_eq!(
        help["result"]["signatures"][0]["documentation"]["value"],
        "Magnitude of x."
    );
}
