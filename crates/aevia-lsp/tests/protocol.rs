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
            r#"{{"jsonrpc":"2.0","id":7,"method":"textDocument/rename","params":{{"textDocument":{{"uri":"{URI}"}},"position":{{"line":0,"character":5}},"newName":"h"}}}}"#
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
    assert_eq!(capabilities["documentSymbolProvider"], true);
    assert_eq!(capabilities["documentFormattingProvider"], true);
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
fn a_request_for_an_unopened_document_answers_null() {
    let (_, replies) = run(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{"textDocument":{"uri":"file:///nowhere.aev"},"position":{"line":0,"character":0}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}"#,
        r#"{"jsonrpc":"2.0","method":"exit"}"#,
    ]);
    assert!(reply(&replies, 2)["result"].is_null());
}
