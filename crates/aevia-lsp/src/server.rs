//! The protocol state machine.
//!
//! One struct, one dispatch table, and no knowledge of how bytes arrive: the
//! transport hands it a parsed message and it hands back a parsed message
//! through a channel. That is what lets the end-to-end test drive the whole
//! server with a scripted byte stream.
//!
//! The rules the protocol sets and this module enforces:
//!
//! * a request before `initialize` is `-32002`, a request after `shutdown` is
//!   `-32600`;
//! * an unknown request is `-32601`, an unknown notification is ignored — a
//!   client may send `$/cancelRequest` and a server that dies on it is broken;
//! * `exit` terminates, with code 0 only if `shutdown` came first.

use aevia_analysis::{Analysis, Change, Document, DocumentStore};
use serde_json::{Value, json};
use std::sync::mpsc::Sender;

/// What the caller should do after a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// Keep reading.
    Continue,
    /// The client sent `exit`; `graceful` is false when it skipped `shutdown`.
    Exit {
        /// Whether `shutdown` was seen first.
        graceful: bool,
    },
}

/// Where the server is in its life cycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Uninitialized,
    Running,
    ShutDown,
}

/// A JSON-RPC error.
#[derive(Debug)]
struct RpcError {
    code: i64,
    message: String,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_REQUEST: i64 = -32600;
const INVALID_PARAMS: i64 = -32602;
const SERVER_NOT_INITIALIZED: i64 = -32002;
const PARSE_ERROR: i64 = -32700;

/// The server's state and its open documents.
pub struct Server {
    documents: DocumentStore,
    state: State,
    out: Sender<Value>,
}

impl Server {
    /// Creates a server that writes its messages to `out`.
    #[must_use]
    pub fn new(out: Sender<Value>) -> Self {
        Self {
            documents: DocumentStore::new(),
            state: State::Uninitialized,
            out,
        }
    }

    /// Handles one parsed message.
    pub fn handle(&mut self, message: Value) -> Control {
        let id = message.get("id").cloned();
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_string);
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        match (method, id) {
            (Some(method), Some(id)) => {
                match self.request(&method, &params) {
                    Ok(result) => self.respond(id, result),
                    Err(error) => self.fail(id, &error),
                }
                Control::Continue
            }
            (Some(method), None) => self.notification(&method, &params),
            // A response to a request this server never sent: it asks the
            // client nothing, so there is nothing to match it against.
            (None, _) => Control::Continue,
        }
    }

    /// Answers a message whose body was not JSON.
    ///
    /// The protocol asks for a `-32700` with a null id, and the connection
    /// survives: the frame was consumed, so the next message still parses.
    pub fn parse_error(&self, detail: &str) {
        self.fail(
            Value::Null,
            &RpcError::new(PARSE_ERROR, format!("the message was not JSON: {detail}")),
        );
    }

    /// Handles a request.
    fn request(&mut self, method: &str, params: &Value) -> Result<Value, RpcError> {
        match method {
            "initialize" => {
                if self.state != State::Uninitialized {
                    return Err(RpcError::new(
                        INVALID_REQUEST,
                        "the server is already initialized",
                    ));
                }
                self.state = State::Running;
                Ok(json!({
                    "capabilities": crate::protocol::capabilities(),
                    "serverInfo": {
                        "name": "aevia-lsp",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                }))
            }
            "shutdown" => {
                if self.state != State::Running {
                    return Err(RpcError::new(
                        INVALID_REQUEST,
                        "the server has not been initialized",
                    ));
                }
                self.state = State::ShutDown;
                Ok(Value::Null)
            }
            _ => {
                match self.state {
                    State::Uninitialized => {
                        return Err(RpcError::new(
                            SERVER_NOT_INITIALIZED,
                            "the client must send `initialize` first",
                        ));
                    }
                    State::ShutDown => {
                        return Err(RpcError::new(
                            INVALID_REQUEST,
                            "the server has been shut down",
                        ));
                    }
                    State::Running => {}
                }
                self.document_request(method, params)
            }
        }
    }

    /// Handles one of the document requests.
    fn document_request(&self, method: &str, params: &Value) -> Result<Value, RpcError> {
        // The method is checked before the parameters: an unknown request must
        // be `-32601` whatever its params look like.
        if !matches!(
            method,
            "textDocument/hover"
                | "textDocument/completion"
                | "textDocument/definition"
                | "textDocument/documentSymbol"
                | "textDocument/formatting"
        ) {
            return Err(RpcError::new(
                METHOD_NOT_FOUND,
                format!("`{method}` is not a method this server implements"),
            ));
        }
        let uri = text_document_uri(params)
            .ok_or_else(|| RpcError::new(INVALID_PARAMS, "no `textDocument.uri`"))?;
        let Some(document) = self.documents.get(uri) else {
            // A request about a document that is not open is answerable with
            // "nothing", which is what every client expects after a close.
            return Ok(Value::Null);
        };
        let analysis = Analysis::new(document);
        let offset = || -> Result<u32, RpcError> {
            let position = params
                .get("position")
                .and_then(crate::protocol::position)
                .ok_or_else(|| RpcError::new(INVALID_PARAMS, "no valid `position`"))?;
            document.offset(position).ok_or_else(|| {
                RpcError::new(INVALID_PARAMS, "the position is outside the document")
            })
        };
        Ok(match method {
            "textDocument/hover" => analysis
                .hover(offset()?)
                .map_or(Value::Null, |hover| crate::protocol::hover_json(&hover)),
            "textDocument/completion" => {
                crate::protocol::completion_json(&analysis.completion(offset()?))
            }
            "textDocument/definition" => analysis
                .definition(offset()?)
                .map_or(Value::Null, |location| crate::protocol::location_json(&location)),
            "textDocument/documentSymbol" => crate::protocol::symbols_json(&analysis.symbols()),
            _ => analysis.formatting().map_or(Value::Null, |edit| {
                Value::Array(vec![crate::protocol::text_edit_json(&edit)])
            }),
        })
    }

    /// Handles a notification.
    fn notification(&mut self, method: &str, params: &Value) -> Control {
        match method {
            "initialized" => Control::Continue,
            "exit" => Control::Exit {
                graceful: self.state == State::ShutDown,
            },
            "textDocument/didOpen" => {
                self.did_open(params);
                Control::Continue
            }
            "textDocument/didChange" => {
                self.did_change(params);
                Control::Continue
            }
            "textDocument/didClose" => {
                self.did_close(params);
                Control::Continue
            }
            // `$/cancelRequest`, `$/setTrace`, `textDocument/didSave` and
            // anything a future client invents: ignored, never fatal.
            _ => Control::Continue,
        }
    }

    /// `textDocument/didOpen`.
    fn did_open(&mut self, params: &Value) {
        let Some(item) = params.get("textDocument") else {
            return;
        };
        let Some(uri) = item.get("uri").and_then(Value::as_str) else {
            return;
        };
        let version = item.get("version").and_then(Value::as_i64).unwrap_or(0);
        let text = item
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        self.documents
            .open(Document::new(uri, version, text));
        self.publish(uri);
    }

    /// `textDocument/didChange`, incremental or full.
    fn did_change(&mut self, params: &Value) {
        let Some(uri) = text_document_uri(params).map(str::to_string) else {
            return;
        };
        let version = params
            .get("textDocument")
            .and_then(|item| item.get("version"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let Some(changes) = params.get("contentChanges").and_then(Value::as_array) else {
            return;
        };
        let Some(document) = self.documents.get_mut(&uri) else {
            return;
        };
        for change in changes {
            let Some(text) = change.get("text").and_then(Value::as_str) else {
                continue;
            };
            let edit = match change.get("range").and_then(crate::protocol::range) {
                Some(range) => Change::ranged(range, text),
                None => Change::full(text),
            };
            // A refused edit leaves the document as it was; the client and the
            // server then still agree on the text, which matters more than
            // applying half an edit.
            let _ = document.apply(&edit, version);
        }
        self.publish(&uri);
    }

    /// `textDocument/didClose`.
    fn did_close(&mut self, params: &Value) {
        let Some(uri) = text_document_uri(params) else {
            return;
        };
        self.documents.close(uri);
        // Clearing the diagnostics is part of closing: a client that keeps
        // them shows errors for a file it no longer has.
        self.notify(
            "textDocument/publishDiagnostics",
            json!({ "uri": uri, "diagnostics": [] }),
        );
    }

    /// Compiles a document and sends its diagnostics.
    fn publish(&self, uri: &str) {
        let Some(document) = self.documents.get(uri) else {
            return;
        };
        let analysis = Analysis::new(document);
        let diagnostics: Vec<Value> = analysis
            .diagnostics()
            .iter()
            .map(|diagnostic| crate::protocol::diagnostic_json(uri, diagnostic))
            .collect();
        self.notify(
            "textDocument/publishDiagnostics",
            json!({
                "uri": uri,
                "version": document.version(),
                "diagnostics": diagnostics,
            }),
        );
    }

    fn send(&self, message: Value) {
        // A closed channel means the writer is gone, which means the process is
        // ending; there is nothing left to report to.
        let _ = self.out.send(message);
    }

    fn respond(&self, id: Value, result: Value) {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }

    fn fail(&self, id: Value, error: &RpcError) {
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": error.code, "message": error.message },
        }));
    }

    fn notify(&self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }
}

/// The `textDocument.uri` of a request or notification.
fn text_document_uri(params: &Value) -> Option<&str> {
    params
        .get("textDocument")?
        .get("uri")?
        .as_str()
}
