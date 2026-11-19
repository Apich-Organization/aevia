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

use crate::command;
use crate::command::REQUEST_FAILED;
use aevia_analysis::Analysis;
use aevia_analysis::AnalysisCache;
use aevia_analysis::Change;
use aevia_analysis::Document;
use aevia_analysis::DocumentStore;
use aevia_analysis::SourceOverlays;
use aevia_analysis::canonical_source_path as canonical_path;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
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
    fn new(
        code: i64,
        message: impl Into<String>,
    ) -> Self {
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
const DOCUMENT_REQUESTS: &[&str] = &[
    "textDocument/hover",
    "textDocument/completion",
    "textDocument/definition",
    "textDocument/documentSymbol",
    "textDocument/formatting",
    "textDocument/codeAction",
    "textDocument/codeLens",
    "textDocument/semanticTokens/full",
    "textDocument/typeDefinition",
    "textDocument/references",
    "textDocument/documentHighlight",
    "textDocument/prepareRename",
    "textDocument/rename",
    "textDocument/signatureHelp",
];
const PARSE_ERROR: i64 = -32700;

/// The server's state and its open documents.
pub struct Server {
    documents: DocumentStore,
    state: State,
    out: Sender<Value>,
    /// Loaded programs by entry path. A hit validates observed inputs and
    /// reuses the checked graph and its query tables (see [`AnalysisCache`]).
    cache: RefCell<AnalysisCache>,
    /// The latest physical buffers in document order. URI aliases are resolved
    /// again before reuse so retargeted symlinks still follow the filesystem.
    overlays: RefCell<Option<OverlaySnapshot>>,
    /// Import candidates as well as read files, retained even after an LRU
    /// program eviction so an edit refreshes all affected open documents.
    dependencies: RefCell<BTreeMap<String, Vec<(PathBuf, PathBuf)>>>,
    commands: Vec<command::Task>,
}

struct OverlaySnapshot {
    paths: Vec<PathBuf>,
    sources: Rc<SourceOverlays>,
}

impl Server {
    /// Creates a server that writes its messages to `out`.
    #[must_use]
    pub fn new(out: Sender<Value>) -> Self {
        Self {
            documents: DocumentStore::new(),
            state: State::Uninitialized,
            out,
            cache: RefCell::new(AnalysisCache::new()),
            overlays: RefCell::new(None),
            dependencies: RefCell::new(BTreeMap::new()),
            commands: Vec::new(),
        }
    }

    /// Handles one parsed message.
    pub fn handle(
        &mut self,
        message: Value,
    ) -> Control {
        let mut message = match message {
            | Value::Object(fields) => fields,
            | _ => Map::new(),
        };
        let id = message.remove("id");
        let method = message.get("method").and_then(Value::as_str);
        let params = message.get("params").unwrap_or(&Value::Null);
        let is_response = method.is_none()
            && message.get("method").is_none()
            && id.is_some()
            && (message.get("result").is_some() || message.get("error").is_some());
        let id_is_valid = id.as_ref().is_none_or(|id| id.is_string() || id.is_i64());
        if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || (!is_response && (method.is_none() || !id_is_valid))
        {
            self.fail(
                if id_is_valid {
                    id.unwrap_or(Value::Null)
                } else {
                    Value::Null
                },
                &RpcError::new(
                    INVALID_REQUEST,
                    "expected a JSON-RPC 2.0 request or notification",
                ),
            );
            return Control::Continue;
        }
        self.commands.retain(|task| !task.finished());
        match (method, id) {
            | (Some("workspace/executeCommand"), Some(id)) => {
                let started = self
                    .require_running()
                    .and_then(|()| self.spawn_command(&id, params));
                if let Err(error) = started {
                    self.fail(id, &error);
                }
                Control::Continue
            },
            | (Some(method), Some(id)) => {
                match self.request(method, params) {
                    | Ok(result) => self.respond(id, result),
                    | Err(error) => self.fail(id, &error),
                }
                Control::Continue
            },
            | (Some(method), None) => self.notification(method, params),
            // A response to a request this server never sent: it asks the
            // client nothing, so there is nothing to match it against.
            | (None, _) => Control::Continue,
        }
    }

    /// Answers a message whose body was not JSON.
    ///
    /// The protocol asks for a `-32700` with a null id, and the connection
    /// survives: the frame was consumed, so the next message still parses.
    pub fn parse_error(
        &self,
        detail: &str,
    ) {
        self.fail(
            Value::Null,
            &RpcError::new(PARSE_ERROR, format!("the message was not JSON: {detail}")),
        );
    }

    /// Handles a request.
    fn request(
        &mut self,
        method: &str,
        params: &Value,
    ) -> Result<Value, RpcError> {
        match method {
            | "initialize" => {
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
            },
            | "shutdown" => {
                self.require_running()?;
                for task in &self.commands {
                    task.cancel();
                }
                self.state = State::ShutDown;
                Ok(Value::Null)
            },
            // A workspace request has no `textDocument`: it is answered from
            // the open documents' loaded programs.
            | "workspace/symbol" => {
                self.require_running()?;
                Ok(self.workspace_symbols(params))
            },
            | _ => {
                self.require_running()?;
                self.document_request(method, params)
            },
        }
    }

    /// Refuses a request outside the running state.
    fn require_running(&self) -> Result<(), RpcError> {
        match self.state {
            | State::Uninitialized => {
                Err(RpcError::new(
                    SERVER_NOT_INITIALIZED,
                    "the client must send `initialize` first",
                ))
            },
            | State::ShutDown => {
                Err(RpcError::new(
                    INVALID_REQUEST,
                    "the server has been shut down",
                ))
            },
            | State::Running => Ok(()),
        }
    }

    /// Handles one of the document requests.
    fn document_request(
        &self,
        method: &str,
        params: &Value,
    ) -> Result<Value, RpcError> {
        // The method is checked before the parameters: an unknown request must
        // be `-32601` whatever its params look like.
        if !DOCUMENT_REQUESTS.contains(&method) {
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
        if method == "textDocument/formatting" {
            return Ok(document.formatting().map_or(Value::Null, |edit| {
                Value::Array(vec![crate::protocol::text_edit_json(&edit)])
            }));
        }
        let offset = || -> Result<u32, RpcError> {
            let position = params
                .get("position")
                .and_then(crate::protocol::position)
                .ok_or_else(|| RpcError::new(INVALID_PARAMS, "no valid `position`"))?;
            document.offset(position).ok_or_else(|| {
                RpcError::new(INVALID_PARAMS, "the position is outside the document")
            })
        };
        let analysis = self.analysis_for(document, &self.overlays());
        Ok(match method {
            | "textDocument/hover" => {
                analysis
                    .hover(offset()?)
                    .map_or(Value::Null, |hover| crate::protocol::hover_json(&hover))
            },
            | "textDocument/completion" => {
                crate::protocol::completion_json(&analysis.completion(offset()?))
            },
            | "textDocument/definition" | "textDocument/typeDefinition" => {
                let offset = offset()?;
                let location = if method == "textDocument/definition" {
                    analysis.definition(offset)
                } else {
                    analysis.type_definition(offset)
                };
                location.map_or(Value::Null, |location| {
                    crate::protocol::location_json(&location)
                })
            },
            | "textDocument/documentSymbol" => crate::protocol::symbols_json(analysis.outline()),
            | "textDocument/codeAction" => document_code_actions(&analysis, uri, params)?,
            | "textDocument/codeLens" => {
                if crate::protocol::path_from_uri(uri).is_some() {
                    crate::protocol::code_lens_json(&analysis.code_lens(uri))
                } else {
                    Value::Array(Vec::new())
                }
            },
            | "textDocument/semanticTokens/full" => {
                crate::protocol::semantic_tokens_json(analysis.highlighted_tokens())
            },
            | "textDocument/signatureHelp" => {
                analysis
                    .signature_help(offset()?)
                    .map_or(Value::Null, |help| {
                        crate::protocol::signature_help_json(&help)
                    })
            },
            | "textDocument/references" => {
                let include_declaration = params
                    .get("context")
                    .and_then(|context| context.get("includeDeclaration"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                crate::protocol::references_json(
                    &analysis.references(offset()?, include_declaration),
                )
            },
            | "textDocument/documentHighlight" => {
                crate::protocol::document_highlights_json(&analysis.document_highlights(offset()?))
            },
            | "textDocument/prepareRename" => {
                match analysis.prepare_rename(offset()?) {
                    | Ok(placeholder) => crate::protocol::prepare_rename_json(&placeholder),
                    // A refusal is a request failure with the reason, not a
                    // silent null: the user asked a question and gets an answer.
                    | Err(error) => return Err(RpcError::new(REQUEST_FAILED, error.message())),
                }
            },
            | "textDocument/rename" => {
                let new_name = params
                    .get("newName")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RpcError::new(INVALID_PARAMS, "no `newName`"))?;
                match analysis.rename(offset()?, new_name) {
                    | Ok(edits) => self.versioned_rename(&edits, document),
                    | Err(error) => return Err(RpcError::new(REQUEST_FAILED, error.message())),
                }
            },
            | _ => Value::Null,
        })
    }

    /// Handles a notification.
    fn notification(
        &mut self,
        method: &str,
        params: &Value,
    ) -> Control {
        match method {
            | "exit" => {
                Control::Exit {
                    graceful: self.state == State::ShutDown,
                }
            },
            | "$/cancelRequest" => {
                if let Some(id) = params.get("id")
                    && let Some(task) = self.commands.iter().find(|task| &task.id == id)
                {
                    task.cancel();
                }
                Control::Continue
            },
            | _ if self.state != State::Running => Control::Continue,
            | "textDocument/didOpen" => {
                self.did_open(params);
                Control::Continue
            },
            | "textDocument/didChange" => {
                self.did_change(params);
                Control::Continue
            },
            | "textDocument/didClose" => {
                self.did_close(params);
                Control::Continue
            },
            | "textDocument/didSave" => {
                if let Some(uri) = text_document_uri(params) {
                    self.publish_affected([uri]);
                }
                Control::Continue
            },
            | "workspace/didChangeWatchedFiles" => {
                if let Some(changes) = params.get("changes").and_then(Value::as_array) {
                    let paths = changes
                        .iter()
                        .filter_map(|change| change.get("uri").and_then(Value::as_str));
                    self.publish_affected(paths);
                }
                Control::Continue
            },
            // `$/setTrace` and other unrecognized notifications are ignored.
            | _ => Control::Continue,
        }
    }

    /// `textDocument/didOpen`.
    fn did_open(
        &mut self,
        params: &Value,
    ) {
        let Some(item) = params.get("textDocument") else {
            return;
        };
        let Some(uri) = item.get("uri").and_then(Value::as_str) else {
            return;
        };
        let Some(version) = item.get("version").and_then(Value::as_i64) else {
            return;
        };
        let Some(text) = item.get("text").and_then(Value::as_str) else {
            return;
        };
        self.documents.open(Document::new(uri, version, text));
        self.overlays.get_mut().take();
        self.publish_affected([uri]);
    }

    /// `textDocument/didChange`, incremental or full.
    fn did_change(
        &mut self,
        params: &Value,
    ) {
        let Some(uri) = text_document_uri(params).map(str::to_string) else {
            return;
        };
        let Some(version) = params
            .get("textDocument")
            .and_then(|item| item.get("version"))
            .and_then(Value::as_i64)
        else {
            return;
        };
        let Some(changes) = params.get("contentChanges").and_then(Value::as_array) else {
            return;
        };
        let Some(document) = self.documents.get(&uri) else {
            return;
        };
        if version <= document.version() {
            return;
        }
        let mut edits = Vec::with_capacity(changes.len());
        for change in changes {
            let Some(text) = change.get("text").and_then(Value::as_str) else {
                return;
            };
            let edit = match change.get("range") {
                | Some(value) => {
                    let Some(range) = crate::protocol::range(value) else {
                        return;
                    };
                    Change::ranged(range, text)
                },
                | None => Change::full(text),
            };
            edits.push(edit);
        }
        // Invalid batches retain both text and version; a later full
        // replacement can resynchronize without exposing a half-applied source.
        let Some(doc) = self.documents.get_mut(&uri) else {
            return;
        };
        if doc.apply_batch(&edits, version).is_err() {
            return;
        }
        self.overlays.get_mut().take();
        self.publish_affected([uri.as_str()]);
    }

    /// `textDocument/didClose`.
    fn did_close(
        &mut self,
        params: &Value,
    ) {
        let Some(uri) = text_document_uri(params) else {
            return;
        };
        if self.documents.close(uri).is_none() {
            return;
        }
        self.dependencies.borrow_mut().remove(uri);
        self.overlays.get_mut().take();
        // Clearing the diagnostics is part of closing: a client that keeps
        // them shows errors for a file it no longer has.
        self.notify(
            "textDocument/publishDiagnostics",
            json!({ "uri": uri, "diagnostics": [] }),
        );
        self.publish_affected([uri]);
    }

    /// Refreshes readers of a changed buffer, including previously missing
    /// imports and documents whose programs have left the bounded cache.
    fn publish_affected<'a>(
        &self,
        changes: impl IntoIterator<Item = &'a str>,
    ) {
        use std::collections::BTreeSet;
        let changes = changes.into_iter().collect::<BTreeSet<_>>();
        let paths = changes
            .iter()
            .filter_map(|uri| crate::protocol::path_from_uri(uri))
            .collect::<Vec<_>>();
        // A package's manifest or lockfile can change imports in another
        // package. The existing cache still reuses unchanged programs.
        let package_change = paths.iter().any(|path| {
            path.file_name()
                .is_some_and(|name| name == "aevia.toml" || name == "aevia.lock")
        });
        let physical = paths
            .iter()
            .map(|path| canonical_path(path))
            .collect::<BTreeSet<_>>();
        let dependencies = self.dependencies.borrow();
        let documents: Vec<&Document> = self
            .documents
            .iter()
            .filter(|document| {
                package_change
                    || changes.contains(document.name())
                    || (!physical.is_empty()
                        && dependencies.get(document.name()).is_some_and(|inputs| {
                            inputs.iter().any(|(logical, old_physical)| {
                                physical.contains(old_physical)
                                    || physical.contains(&canonical_path(logical))
                            })
                        }))
            })
            .collect();
        drop(dependencies);
        if !documents.is_empty() {
            let overlays = self.overlays();
            for document in documents {
                self.publish(document, &overlays);
            }
        }
    }

    /// Compiles a document and sends its diagnostics.
    fn publish(
        &self,
        document: &Document,
        overlays: &SourceOverlays,
    ) {
        let uri = document.name();
        let analysis = self.analysis_for(document, overlays);
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

    fn send(
        &self,
        message: Value,
    ) {
        // A closed channel means the writer is gone, which means the process is
        // ending; there is nothing left to report to.
        let _ = self.out.send(message);
    }

    fn respond(
        &self,
        id: Value,
        result: Value,
    ) {
        self.send(Value::Object(Map::from_iter([
            ("jsonrpc".into(), Value::from("2.0")),
            ("id".into(), id),
            ("result".into(), result),
        ])));
    }

    fn fail(
        &self,
        id: Value,
        error: &RpcError,
    ) {
        self.send(Value::Object(Map::from_iter([
            ("jsonrpc".into(), Value::from("2.0")),
            ("id".into(), id),
            (
                "error".into(),
                json!({ "code": error.code, "message": error.message }),
            ),
        ])));
    }

    fn notify(
        &self,
        method: &str,
        params: Value,
    ) {
        self.send(Value::Object(Map::from_iter([
            ("jsonrpc".into(), Value::from("2.0")),
            ("method".into(), Value::from(method)),
            ("params".into(), params),
        ])));
    }
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keep code action range validation and kind filtering together"
    )
)]
fn document_code_actions(
    analysis: &Analysis<'_>,
    uri: &str,
    params: &Value,
) -> Result<Value, RpcError> {
    let range = params
        .get("range")
        .and_then(crate::protocol::range)
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, "no valid `range`"))?;
    let mut result = crate::protocol::code_actions_json(uri, &analysis.code_actions(range));
    if let Some(only) = params.pointer("/context/only").and_then(Value::as_array)
        && let Some(actions) = result.as_array_mut()
    {
        actions.retain(|action| {
            let Some(kind) = action.get("kind").and_then(Value::as_str) else {
                return false;
            };
            only.iter().filter_map(Value::as_str).any(|requested| {
                requested.is_empty()
                    || kind == requested
                    || kind
                        .strip_prefix(requested)
                        .is_some_and(|suffix| suffix.starts_with('.'))
            })
        });
    }
    Ok(result)
}

/// The `textDocument.uri` of a request or notification.
fn text_document_uri(params: &Value) -> Option<&str> {
    params.get("textDocument")?.get("uri")?.as_str()
}

impl Server {
    /// Validates and starts a run or test command for an open, saved document.
    fn spawn_command(
        &mut self,
        id: &Value,
        params: &Value,
    ) -> Result<(), RpcError> {
        if self.commands.len() >= command::MAX_CONCURRENT {
            return Err(RpcError::new(
                REQUEST_FAILED,
                "too many Aevia commands are already running",
            ));
        }
        if self.commands.iter().any(|task| task.id == *id) {
            return Err(RpcError::new(
                INVALID_REQUEST,
                "a command already has this request id",
            ));
        }
        let command = params
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::new(INVALID_PARAMS, "no `command`"))?;
        let args = params.get("arguments").and_then(Value::as_array);
        let uri = args
            .and_then(|items| items.first())
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::new(INVALID_PARAMS, "expected a document URI argument"))?;
        let document = self.documents.get(uri).ok_or_else(|| {
            RpcError::new(
                INVALID_PARAMS,
                "the command applies only to an open document",
            )
        })?;
        let path = aevia_analysis::physical_source_path_from_uri(uri)
            .filter(|path| path.is_absolute())
            .ok_or_else(|| RpcError::new(INVALID_PARAMS, "the document is not a `file:` URI"))?;
        if !saved_source_matches(&path, document.text()) {
            return Err(RpcError::new(
                REQUEST_FAILED,
                "save the Aevia file before running or testing it",
            ));
        }
        let mut process = std::process::Command::new(command::executable());
        process.current_dir(command::working_directory(&path));
        let label = match command {
            | "aevia.runFile" => {
                process.arg("run").arg(&path);
                "run"
            },
            | "aevia.testFile" => {
                process.arg("test").arg(&path);
                if let Some(filter) = args.and_then(|items| items.get(1)).and_then(Value::as_str) {
                    process.arg(format!("--exact={filter}"));
                }
                "test"
            },
            | other => {
                return Err(RpcError::new(
                    INVALID_PARAMS,
                    format!("unknown command `{other}`"),
                ));
            },
        };
        let task = command::Task::spawn(
            self.out.clone(),
            id.clone(),
            command::Job { label, process },
        )
        .map_err(|error| {
            RpcError::new(
                REQUEST_FAILED,
                format!("could not start Aevia {label}: {error}"),
            )
        })?;
        self.commands.push(task);
        Ok(())
    }

    /// Answers from the opened documents' programs: every declaration of every
    /// module they load, filtered by `query`.
    fn workspace_symbols(
        &self,
        params: &Value,
    ) -> Value {
        let query = params.get("query").and_then(Value::as_str).unwrap_or("");
        let mut seen = std::collections::HashSet::new();
        let mut symbols = Vec::new();
        let overlays = self.overlays();
        for document in self.documents.iter() {
            let analysis = self.analysis_for(document, &overlays);
            for symbol in analysis.workspace_symbols(query) {
                let key = (
                    symbol.document.clone(),
                    symbol.symbol.name.clone(),
                    symbol.symbol.range.start.line,
                    symbol.symbol.range.start.column,
                );
                if seen.insert(key) {
                    symbols.push(symbol);
                }
            }
        }
        crate::protocol::workspace_symbols_json(&symbols)
    }

    /// Edits refer to the exact unsaved snapshots used by analysis. Use open
    /// document URIs and versions so clients can refuse stale workspace edits.
    fn versioned_rename(
        &self,
        edits: &[aevia_analysis::FileEdit],
        focused: &Document,
    ) -> Value {
        let mut result = crate::protocol::rename_json(edits);
        if let Some(changes) = result
            .get_mut("documentChanges")
            .and_then(Value::as_array_mut)
        {
            for change in changes {
                let Some(target) = change
                    .get_mut("textDocument")
                    .and_then(Value::as_object_mut)
                else {
                    continue;
                };
                if let Some(uri) = target.get("uri").and_then(Value::as_str)
                    && let Some(document) = self.documents.get(uri)
                {
                    target.insert("version".into(), Value::from(document.version()));
                    continue;
                }
                let Some(path) = target
                    .get("uri")
                    .and_then(Value::as_str)
                    .and_then(crate::protocol::path_from_uri)
                else {
                    continue;
                };
                let physical = canonical_path(&path);
                let same_file = |document: &&Document| {
                    crate::protocol::path_from_uri(document.name())
                        .is_some_and(|path| canonical_path(&path) == physical)
                };
                let open = self.documents.iter().filter(same_file).last();
                let focused_matches = crate::protocol::path_from_uri(focused.name())
                    .is_some_and(|path| canonical_path(&path) == physical);
                if let Some(document) = if focused_matches {
                    Some(focused)
                } else {
                    open
                } {
                    target.insert("uri".into(), Value::String(document.name().to_owned()));
                    target.insert("version".into(), Value::from(document.version()));
                }
            }
        }
        result
    }

    /// Analyses a document: through the cached loader when it has a `file:`
    /// path that can bound a load, else the single-file path.
    ///
    /// The buffer text is the entry overlay in both cases, so an unsaved file
    /// keeps its imports while its diagnostics point at the unsaved text.
    fn analysis_for<'a>(
        &self,
        document: &'a Document,
        overlays: &SourceOverlays,
    ) -> Analysis<'a> {
        let Some(path) = crate::protocol::path_from_uri(document.name()) else {
            return Analysis::new(document);
        };
        let program = self.cache.borrow_mut().program_for_document_with_overlays(
            &path,
            document.text(),
            overlays,
        );
        if let Some(program) = &program {
            self.dependencies.borrow_mut().insert(
                document.name().to_owned(),
                program
                    .dependencies
                    .iter()
                    .map(|input| {
                        (
                            input.path().to_path_buf(),
                            input.physical_path().to_path_buf(),
                        )
                    })
                    .collect(),
            );
        }
        match program {
            | Some(program) => self.cache.borrow().analyze_program(document, program),
            | None => Analysis::new(document),
        }
    }

    fn overlays(&self) -> Rc<SourceOverlays> {
        let buffers = self
            .documents
            .iter()
            .filter_map(|document| {
                let path = crate::protocol::path_from_uri(document.name())?;
                Some((canonical_path(&path), document.text()))
            })
            .collect::<Vec<_>>();
        let mut cached = self.overlays.borrow_mut();
        if let Some(snapshot) = cached.as_ref()
            && snapshot
                .paths
                .iter()
                .eq(buffers.iter().map(|(path, _)| path))
        {
            return Rc::clone(&snapshot.sources);
        }
        let mut overlays = SourceOverlays::new();
        let mut paths = Vec::with_capacity(buffers.len());
        for (path, text) in buffers {
            overlays.insert(&path, text);
            paths.push(path);
        }
        let sources = Rc::new(overlays);
        *cached = Some(OverlaySnapshot {
            paths,
            sources: Rc::clone(&sources),
        });
        sources
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        for task in &self.commands {
            task.cancel();
        }
        for task in self.commands.drain(..) {
            task.join();
        }
    }
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "validate the on-disk identity used by editor execution"
    )
)]
fn saved_source_matches(
    path: &Path,
    text: &str,
) -> bool {
    use std::io::Read;
    if !path.is_file() {
        return false;
    }
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return false;
    }
    // Clients may normalize CRLF or remove the UTF-8 BOM when opening a file.
    let limit = text.len().saturating_mul(2).saturating_add(4);
    let mut saved = String::new();
    if file
        .take(u64::try_from(limit).unwrap_or(u64::MAX))
        .read_to_string(&mut saved)
        .is_err()
    {
        return false;
    }
    let saved = saved.strip_prefix('\u{feff}').unwrap_or(&saved);
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    saved == text
        || (saved.len() < limit
            && saved.lines().eq(text.lines())
            && saved.ends_with('\n') == text.ends_with('\n'))
}
