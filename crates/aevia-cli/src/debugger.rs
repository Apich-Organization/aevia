//! Debug Adapter Protocol transport and a serial source debugging session.

mod variables;

use self::variables::VariableStore;
use aevia_core::Interp;
use aevia_core::cancellation::CancellationToken;
use aevia_core::debugger::Debugger;
use aevia_core::driver::Program;
use aevia_core::eval::EvalError;
use aevia_core::eval::Frame;
use aevia_core::source::canonical_path;
use aevia_core::value::Scope;
use aevia_lsp::transport::Reader;
use aevia_lsp::transport::Writer;
use aevia_syntax::Expr;
use aevia_syntax::FileId;
use aevia_syntax::SourceMap;
use aevia_syntax::Span;
use aevia_syntax::visit::Visitor;
use aevia_syntax::visit::walk_expr;
use serde_json::Value as Json;
use serde_json::json;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::io::BufReader;
use std::io::{
    self,
};
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::mpsc::Sender;

const THREAD_ID: u8 = 1;
const LOCAL_BINDINGS: usize = 1;
const MODULE_BINDINGS: usize = 2;
const MAX_FRAMES: usize = 256;

#[allow(
    clippy::single_call_fn,
    reason = "the debug command owns its protocol transport and worker lifecycle"
)]
pub fn serve(
    options: super::Options,
    args: &[String],
) -> ExitCode {
    if args.iter().any(|arg| arg != "--stdio") {
        eprintln!("usage: aevia debug [--stdio]");
        return ExitCode::from(2);
    }
    let (out, output) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        let mut writer = Writer::new(io::stdout().lock());
        for (sequence, message) in output.into_iter().enumerate() {
            let Some(mut value): Option<Json> = message else {
                break;
            };
            set_field(&mut value, "seq", json!(sequence.saturating_add(1)));
            writer.send(&value)?;
        }
        Ok::<_, io::Error>(())
    });
    let mut adapter = Adapter::new(options, out);
    let mut reader = Reader::new(BufReader::new(io::stdin().lock()));
    let mut code = ExitCode::SUCCESS;
    loop {
        match reader.next_message() {
            | Ok(Some(request)) if field(&request, "type") == "request" => {
                if field(&request, "command") == "disconnect" {
                    adapter.control.disconnect();
                    adapter.response(&request, Ok(Json::Null));
                    break;
                }
                adapter.handle(&request);
            },
            | Ok(Some(_)) => {},
            | Ok(None) => break,
            | Err(error) => {
                eprintln!("aevia debug: {error}");
                code = ExitCode::from(1);
                break;
            },
        }
    }
    adapter.control.disconnect();
    // The program may be in a blocking IO operation. Shutdown the writer
    // after the disconnect acknowledgement without waiting for the debuggee.
    let _ = adapter.out.send(None);
    if !matches!(writer.join(), Ok(Ok(()))) {
        return ExitCode::from(1);
    }
    code
}

#[derive(Clone, Copy)]
struct ClientCoordinates {
    line_base: usize,
    column_base: usize,
}

impl Default for ClientCoordinates {
    fn default() -> Self {
        Self {
            line_base: 1,
            column_base: 1,
        }
    }
}

impl ClientCoordinates {
    fn line_in(
        self,
        line: &Json,
    ) -> Option<usize> {
        let line = usize::try_from(line.as_u64()?).ok()?;
        line.checked_sub(self.line_base)?.checked_add(1)
    }

    const fn line_out(
        self,
        line: usize,
    ) -> usize {
        line.saturating_sub(1).saturating_add(self.line_base)
    }

    const fn col_out(
        self,
        column: usize,
    ) -> usize {
        column.saturating_sub(1).saturating_add(self.column_base)
    }
}

type Outbound = Sender<Option<Json>>;
type Sites = BTreeMap<PathBuf, BTreeMap<usize, usize>>;
type DebugResult = Result<Json, String>;

fn field<'a>(
    value: &'a Json,
    key: &str,
) -> &'a Json {
    value.get(key).unwrap_or(&Json::Null)
}

fn set_field(
    value: &mut Json,
    key: &str,
    field: Json,
) {
    if let Some(object) = value.as_object_mut() {
        object.insert(key.to_string(), field);
    }
}

#[derive(Clone, Copy, Default)]
enum Resume {
    #[default]
    Continue,
    Entry,
    Pause,
    StepIn,
    Next(usize),
    StepOut(usize),
}

#[derive(Clone)]
struct Breakpoint {
    id: usize,
    line: usize,
    supported: bool,
}

impl Breakpoint {
    fn wire(
        &self,
        source: &Path,
        sites: Option<&Sites>,
        coords: ClientCoordinates,
    ) -> Json {
        let column = sites
            .and_then(|sites| sites.get(source))
            .and_then(|lines| lines.get(&self.line));
        let mut breakpoint = json!({
            "id": self.id,
            "verified": self.supported && column.is_some(),
            "line": coords.line_out(self.line),
        });
        if !self.supported {
            set_field(
                &mut breakpoint,
                "message",
                json!("Aevia supports unconditional line breakpoints."),
            );
        } else if let Some(column) = column {
            set_field(&mut breakpoint, "column", json!(coords.col_out(*column)));
        } else {
            set_field(
                &mut breakpoint,
                "message",
                json!(if sites.is_some() {
                    "There is no executable Aevia expression on this line."
                } else {
                    "Waiting for the program's source to load."
                }),
            );
        }
        breakpoint
    }
}

struct Stopped {
    frames: Vec<Json>,
    depth: usize,
    variables: VariableStore,
    exception: Option<(String, String)>,
}

#[derive(Default)]
struct State {
    resume: Resume,
    stopped: Option<Stopped>,
    sites: Option<Sites>,
    breakpoints: BTreeMap<PathBuf, Vec<Breakpoint>>,
    next_breakpoint: usize,
    break_on_error: bool,
    ended: bool,
}

#[derive(Default)]
struct Control {
    state: Mutex<State>,
    resumed: Condvar,
    cancellation: CancellationToken,
}

impl Control {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn disconnect(&self) {
        self.cancellation.cancel();
        self.state().ended = true;
        self.resumed.notify_all();
    }

    fn wait_for_resume(
        &self,
        state: MutexGuard<'_, State>,
    ) {
        let _state = self
            .resumed
            .wait_while(state, |state| state.stopped.is_some() && !state.ended)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
}

struct Launch {
    file: PathBuf,
    cwd: PathBuf,
    args: Vec<String>,
    stdin: Vec<String>,
    stop_on_entry: bool,
    no_debug: bool,
}

impl Launch {
    #[allow(
        clippy::single_call_fn,
        reason = "validates a complete DAP launch request before publishing it"
    )]
    fn read(arguments: &Json) -> Result<Self, String> {
        let strings = |name: &str| -> Result<Vec<String>, String> {
            let Some(value) = arguments.get(name) else {
                return Ok(Vec::new());
            };
            let array = value
                .as_array()
                .ok_or_else(|| format!("`{name}` must contain strings"))?;
            array
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| format!("`{name}` must contain strings"))
                })
                .collect()
        };
        let file = field(arguments, "program")
            .as_str()
            .filter(|name| !name.is_empty())
            .ok_or("provide the Aevia source path in `program`")?;
        let file = std::path::absolute(file).map_err(|error| error.to_string())?;
        if !file.is_file() {
            return Err(format!(
                "Aevia source file does not exist: {}",
                file.display()
            ));
        }
        let containing = file.parent().ok_or("the program has no parent directory")?;
        let cwd = field(arguments, "cwd").as_str().map_or_else(
            || {
                containing
                    .ancestors()
                    .find(|ancestor| ancestor.join("aevia.toml").is_file())
                    .unwrap_or(containing)
                    .to_path_buf()
            },
            PathBuf::from,
        );
        if !cwd.is_dir() {
            return Err(format!(
                "Aevia working directory does not exist: {}",
                cwd.display()
            ));
        }
        Ok(Self {
            file,
            cwd,
            args: strings("args")?,
            stdin: strings("stdin")?,
            stop_on_entry: field(arguments, "stopOnEntry") == true,
            no_debug: field(arguments, "noDebug") == true,
        })
    }
}

struct Adapter {
    control: Arc<Control>,
    out: Outbound,
    options: super::Options,
    coordinates: ClientCoordinates,
    launch: Option<Launch>,
    initialized: bool,
    configured: bool,
    started: bool,
}

impl Adapter {
    #[allow(
        clippy::single_call_fn,
        reason = "creates one isolated adapter and synchronization state per connection"
    )]
    fn new(
        options: super::Options,
        out: Outbound,
    ) -> Self {
        Self {
            control: Arc::default(),
            out,
            options,
            coordinates: ClientCoordinates::default(),
            launch: None,
            initialized: false,
            configured: false,
            started: false,
        }
    }

    fn response(
        &self,
        request: &Json,
        result: DebugResult,
    ) {
        let mut response = json!({
            "type": "response",
            "request_seq": field(request, "seq"),
            "command": field(request, "command"),
            "success": result.is_ok(),
        });
        match result {
            | Ok(body) if !body.is_null() => {
                set_field(&mut response, "body", body);
            },
            | Ok(_) => {},
            | Err(message) => {
                set_field(&mut response, "message", json!(message));
            },
        }
        let _ = self.out.send(Some(response));
    }

    fn handle(
        &mut self,
        request: &Json,
    ) {
        let args = field(request, "arguments");
        let command = field(request, "command").as_str().unwrap_or_default();
        let result = match command {
            | "initialize" if !self.initialized => {
                let result = self.initialize(args);
                let initialized = result.is_ok();
                self.response(request, result);
                if initialized {
                    event(&self.out, "initialized", Json::Null);
                }
                return;
            },
            | _ if !self.initialized => Err("initialize the debug adapter first".into()),
            | "launch" if self.launch.is_none() && !self.started => {
                match Launch::read(args) {
                    | Ok(launch) => {
                        self.launch = Some(launch);
                        self.response(request, Ok(Json::Null));
                        self.start_if_ready();
                        return;
                    },
                    | Err(error) => Err(error),
                }
            },
            | "configurationDone" => {
                self.configured = true;
                self.response(request, Ok(Json::Null));
                self.start_if_ready();
                return;
            },
            | "setBreakpoints" => self.set_breakpoints(args),
            | "setExceptionBreakpoints" => {
                self.control.state().break_on_error = field(args, "filters")
                    .as_array()
                    .is_some_and(|filters| filters.iter().any(|filter| filter == "uncaught"));
                Ok(Json::Null)
            },
            | "threads" => {
                let threads = if self.started && !self.control.state().ended {
                    json!([{ "id": THREAD_ID, "name": "Aevia" }])
                } else {
                    json!([])
                };
                Ok(json!({ "threads": threads }))
            },
            | "stackTrace" | "scopes" | "variables" | "evaluate" | "exceptionInfo" => {
                self.inspect(command, args)
            },
            | "continue" | "next" | "stepIn" | "stepOut" => {
                let mut state = self.control.state();
                let Some(stopped) = state.stopped.take() else {
                    self.response(request, Err("the Aevia program is not stopped".into()));
                    return;
                };
                state.resume = match command {
                    | "next" => Resume::Next(stopped.depth),
                    | "stepIn" => Resume::StepIn,
                    | "stepOut" => Resume::StepOut(stopped.depth),
                    | _ => Resume::Continue,
                };
                let result = if command == "continue" {
                    json!({ "allThreadsContinued": true })
                } else {
                    Json::Null
                };
                self.response(request, Ok(result));
                drop(state);
                self.control.resumed.notify_all();
                return;
            },
            | "pause" if self.started => {
                let mut state = self.control.state();
                if !state.ended && state.stopped.is_none() {
                    state.resume = Resume::Pause;
                }
                drop(state);
                Ok(Json::Null)
            },
            | "terminate" if self.started => {
                let mut state = self.control.state();
                self.response(request, Ok(Json::Null));
                if !state.ended {
                    state.ended = true;
                    state.stopped = None;
                    self.control.cancellation.cancel();
                    event(&self.out, "terminated", Json::Null);
                }
                drop(state);
                self.control.resumed.notify_all();
                return;
            },
            | command => Err(format!("unsupported Aevia debug request `{command}`")),
        };
        self.response(request, result);
    }

    fn initialize(
        &mut self,
        args: &Json,
    ) -> DebugResult {
        if field(args, "pathFormat")
            .as_str()
            .is_some_and(|format| format != "path")
        {
            return Err("Aevia debug expects file paths".into());
        }
        self.initialized = true;
        self.coordinates = ClientCoordinates {
            line_base: usize::from(field(args, "linesStartAt1") != false),
            column_base: usize::from(field(args, "columnsStartAt1") != false),
        };
        Ok(json!({
            "supportsConfigurationDoneRequest": true,
            "supportsTerminateRequest": true,
            "supportsExceptionInfoRequest": true,
            "supportsEvaluateForHovers": true,
            "exceptionBreakpointFilters": [
                { "filter": "uncaught", "label": "Uncaught Aevia error", "default": false }
            ]
        }))
    }

    fn start_if_ready(&mut self) {
        if self.configured
            && let Some(launch) = self.launch.take()
        {
            self.started = true;
            self.start(launch);
        }
    }

    fn set_breakpoints(
        &self,
        args: &Json,
    ) -> DebugResult {
        let path = field(field(args, "source"), "path")
            .as_str()
            .filter(|path| !path.is_empty())
            .ok_or("a breakpoint needs the path of a source file")?;
        let path = canonical_path(Path::new(path));
        let mut state = self.control.state();
        let mut next_id = state.next_breakpoint;
        let mut breakpoints = Vec::new();
        for request in field(args, "breakpoints").as_array().into_iter().flatten() {
            let line = self
                .coordinates
                .line_in(field(request, "line"))
                .ok_or("breakpoint line is invalid")?;
            next_id = next_id.checked_add(1).ok_or("breakpoint IDs exhausted")?;
            breakpoints.push(Breakpoint {
                id: next_id,
                line,
                supported: ["condition", "hitCondition", "logMessage"]
                    .iter()
                    .all(|property| field(request, property).is_null()),
            });
        }
        let response = breakpoints
            .iter()
            .map(|bp| bp.wire(&path, state.sites.as_ref(), self.coordinates))
            .collect::<Vec<_>>();
        state.next_breakpoint = next_id;
        state.breakpoints.insert(path, breakpoints);
        drop(state);
        Ok(json!({ "breakpoints": response }))
    }

    fn inspect(
        &self,
        command: &str,
        args: &Json,
    ) -> DebugResult {
        let state = self.control.state();
        let stopped = state
            .stopped
            .as_ref()
            .ok_or("pause the Aevia program before inspecting it")?;
        let response = match command {
            | "stackTrace" => {
                let start = field(args, "startFrame")
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .unwrap_or(0);
                let count = field(args, "levels")
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .filter(|n| *n != 0)
                    .unwrap_or(usize::MAX);
                let frames = stopped
                    .frames
                    .iter()
                    .skip(start)
                    .take(count)
                    .collect::<Vec<_>>();
                json!({ "stackFrames": frames, "totalFrames": stopped.frames.len() })
            },
            | "scopes" => {
                let scopes = if field(args, "frameId") == 1 {
                    json!([
                        { "name": "Locals", "variablesReference": LOCAL_BINDINGS, "expensive": false },
                        { "name": "Module bindings", "variablesReference": MODULE_BINDINGS, "expensive": false }
                    ])
                } else {
                    json!([])
                };
                json!({ "scopes": scopes })
            },
            | "variables" => {
                let reference = field(args, "variablesReference")
                    .as_u64()
                    .and_then(|reference| usize::try_from(reference).ok())
                    .unwrap_or(0);
                let start = field(args, "start")
                    .as_u64()
                    .and_then(|start| usize::try_from(start).ok())
                    .unwrap_or(0);
                let count = field(args, "count")
                    .as_u64()
                    .and_then(|count| usize::try_from(count).ok())
                    .filter(|count| *count != 0)
                    .unwrap_or(usize::MAX);
                let variables = stopped
                    .variables
                    .list(reference)
                    .iter()
                    .skip(start)
                    .take(count)
                    .collect::<Vec<_>>();
                json!({ "variables": variables })
            },
            | "evaluate" => {
                if field(args, "frameId").as_u64().is_some_and(|id| id != 1) {
                    return Err("only the selected execution frame exposes Aevia bindings".into());
                }
                let name = field(args, "expression")
                    .as_str()
                    .unwrap_or_default()
                    .trim();
                let binding = stopped
                    .variables
                    .list(LOCAL_BINDINGS)
                    .iter()
                    .chain(stopped.variables.list(MODULE_BINDINGS))
                    .find(|binding| field(binding, "name") == name)
                    .ok_or_else(|| format!("no visible Aevia binding named `{name}`"))?;
                json!({
                    "result": field(binding, "value"),
                    "type": field(binding, "type"),
                    "variablesReference": field(binding, "variablesReference"),
                })
            },
            | "exceptionInfo" => {
                let (id, message) = stopped
                    .exception
                    .as_ref()
                    .ok_or("the program did not stop on an exception")?;
                json!({ "exceptionId": id, "description": message, "breakMode": "unhandled" })
            },
            | _ => return Err(format!("unsupported Aevia inspection request `{command}`")),
        };
        drop(state);
        Ok(response)
    }

    fn start(
        &self,
        launch: Launch,
    ) {
        let control = self.control.clone();
        let out = self.out.clone();
        let options = self.options;
        let coords = self.coordinates;
        let stack = options.max_stack_mb.saturating_mul(1024 * 1024);
        let worker = std::thread::Builder::new()
            .name("aevia-debuggee".into())
            .stack_size(stack)
            .spawn(move || {
                aevia_core::install_stack_budget(stack);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_program(options, launch, &control, &out, coords)
                }));
                let exit_code = result.unwrap_or_else(|_| {
                    output(
                        &out,
                        "stderr",
                        "The Aevia interpreter stopped unexpectedly.\n",
                    );
                    101
                });
                finish(&control, &out, exit_code);
            });
        if let Err(error) = worker {
            output(
                &self.out,
                "stderr",
                &format!("Could not start the Aevia interpreter: {error}\n"),
            );
            finish(&self.control, &self.out, 101);
        }
    }
}

fn event(
    out: &Outbound,
    name: &str,
    body: Json,
) {
    let mut message = json!({ "type": "event", "event": name });
    if !body.is_null() {
        set_field(&mut message, "body", body);
    }
    let _ = out.send(Some(message));
}

fn output(
    out: &Outbound,
    category: &str,
    text: &str,
) {
    event(
        out,
        "output",
        json!({ "category": category, "output": text }),
    );
}

fn finish(
    control: &Control,
    out: &Outbound,
    exit_code: i32,
) {
    let mut state = control.state();
    if !state.ended {
        state.ended = true;
        state.stopped = None;
        drop(state);
        event(out, "exited", json!({ "exitCode": exit_code }));
        event(out, "terminated", Json::Null);
    }
}

struct Files {
    paths: HashMap<FileId, (PathBuf, String)>,
    sources: SourceMap,
}

impl Files {
    #[allow(
        clippy::single_call_fn,
        reason = "builds the debugger source map from one compiled program"
    )]
    fn load(program: &Program) -> Self {
        let paths = program
            .modules
            .iter()
            .filter_map(|module| {
                let name = &program.sources.file(module.file)?.name;
                let path = Path::new(name);
                path.is_absolute()
                    .then(|| (module.file, (canonical_path(path), name.clone())))
            })
            .collect();
        Self {
            paths,
            sources: program.sources.clone(),
        }
    }

    fn location(
        &self,
        span: Span,
    ) -> Option<(usize, usize)> {
        if span.is_empty() || !self.paths.contains_key(&span.file) {
            return None;
        }
        let source = self.sources.file(span.file)?;
        let (line, column) = source.line_col(span.start);
        let utf16 = source
            .line_text(line)
            .chars()
            .take(column.saturating_sub(1))
            .map(char::len_utf16)
            .sum::<usize>();
        Some((line, utf16.saturating_add(1)))
    }

    fn collect_sites(
        &self,
        program: &Program,
    ) -> Sites {
        struct Collector<'a> {
            files: &'a Files,
            sites: Sites,
        }
        impl<'ast> Visitor<'ast> for Collector<'_> {
            fn visit_expr(
                &mut self,
                expr: &'ast Expr,
            ) {
                if let Some((line, column)) = self.files.location(expr.span)
                    && let Some((path, _)) = self.files.paths.get(&expr.span.file)
                {
                    self.sites
                        .entry(path.clone())
                        .or_default()
                        .entry(line)
                        .and_modify(|first| *first = (*first).min(column))
                        .or_insert(column);
                }
                walk_expr(self, expr);
            }
        }
        let mut collector = Collector {
            files: self,
            sites: Sites::new(),
        };
        for module in &program.modules {
            collector.visit_module(&module.module);
        }
        collector.sites
    }

    fn stack_frame(
        &self,
        id: usize,
        name: &str,
        at: Span,
        coords: ClientCoordinates,
    ) -> Json {
        let mut frame = json!({ "id": id, "name": name, "line": 0, "column": 0 });
        if let Some((line, column)) = self.location(at)
            && let Some((_, path)) = self.paths.get(&at.file)
        {
            let name = Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(path);
            set_field(&mut frame, "line", json!(coords.line_out(line)));
            set_field(&mut frame, "column", json!(coords.col_out(column)));
            set_field(&mut frame, "source", json!({ "name": name, "path": path }));
        }
        frame
    }
}

struct Observer {
    out: Outbound,
    control: Arc<Control>,
    files: Files,
    coords: ClientCoordinates,
    last_line: RefCell<Vec<Option<(FileId, usize)>>>,
    no_debug: bool,
}

impl Observer {
    fn snapshot(
        &self,
        span: Span,
        stack: &[Frame],
        scope: Option<&Scope>,
    ) -> Stopped {
        let frames = if stack.is_empty() {
            vec![self.files.stack_frame(1, "<module>", span, self.coords)]
        } else {
            stack
                .iter()
                .enumerate()
                .rev()
                .take(MAX_FRAMES)
                .enumerate()
                .map(|(id, (index, frame))| {
                    let at = stack
                        .get(index.saturating_add(1))
                        .map_or(span, |caller| caller.span);
                    self.files
                        .stack_frame(id.saturating_add(1), &frame.function, at, self.coords)
                })
                .collect()
        };
        let variables = scope.map_or_else(VariableStore::default, VariableStore::capture);
        Stopped {
            frames,
            depth: stack.len(),
            variables,
            exception: None,
        }
    }

    fn exception(
        &self,
        error: &EvalError,
    ) {
        let mut state = self.control.state();
        if !self.no_debug && state.break_on_error && !state.ended {
            let mut stopped = self.snapshot(error.span.unwrap_or_default(), &error.stack, None);
            stopped.exception = Some((error.code.to_owned(), error.message.clone()));
            state.stopped = Some(stopped);
            event(
                &self.out,
                "stopped",
                json!({
                    "reason": "exception", "threadId": THREAD_ID, "allThreadsStopped": true, "text": error.message
                }),
            );
            self.control.wait_for_resume(state);
        }
    }
}

impl Debugger for Observer {
    fn expression(
        &self,
        span: Span,
        stack: &[Frame],
        scope: &Scope,
    ) {
        if self.no_debug {
            return;
        }
        let Some((line, _)) = self.files.location(span) else {
            return;
        };
        let Some((path, _)) = self.files.paths.get(&span.file) else {
            return;
        };
        let depth = stack.len();
        let previous = {
            let mut last = self.last_line.borrow_mut();
            last.resize(depth.saturating_add(1), None);
            last.get_mut(depth)
                .and_then(|entry| entry.replace((span.file, line)))
        };
        let changed = previous != Some((span.file, line));
        let mut state = self.control.state();
        if state.ended {
            return;
        }
        let hit = if changed {
            state
                .breakpoints
                .get(path)
                .into_iter()
                .flatten()
                .filter(|bp| bp.supported && bp.line == line)
                .map(|bp| bp.id)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let reason = match state.resume {
            | Resume::Entry => "entry",
            | Resume::Pause => "pause",
            | _ if !hit.is_empty() => "breakpoint",
            | Resume::StepIn if changed => "step",
            | Resume::Next(max_depth) if changed && depth <= max_depth => "step",
            | Resume::StepOut(max_depth) if depth < max_depth => "step",
            | _ => return,
        };
        state.stopped = Some(self.snapshot(span, stack, Some(scope)));
        event(
            &self.out,
            "stopped",
            json!({
                "reason": reason, "threadId": THREAD_ID, "allThreadsStopped": true, "hitBreakpointIds": hit
            }),
        );
        self.control.wait_for_resume(state);
    }

    fn output(
        &self,
        text: &str,
        newline: bool,
        stderr: bool,
    ) {
        let category = if stderr { "stderr" } else { "stdout" };
        if newline {
            output(&self.out, category, &format!("{text}\n"));
        } else {
            output(&self.out, category, text);
        }
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "runs the debuggee within the debugger thread's panic boundary"
)]
fn run_program(
    options: super::Options,
    launch: Launch,
    control: &Arc<Control>,
    out: &Outbound,
    coords: ClientCoordinates,
) -> i32 {
    if let Err(error) = std::env::set_current_dir(&launch.cwd) {
        output(
            out,
            "stderr",
            &format!("Could not open {}: {error}\n", launch.cwd.display()),
        );
        return 1;
    }
    let program = super::load_at_with(&launch.file, false);
    if !program.diagnostics.is_empty() {
        output(
            out,
            "stderr",
            &format!("{}\n", program.diagnostics.render(&program.sources)),
        );
    }
    if program.diagnostics.has_errors() {
        return 1;
    }
    let Some(entry) = program.entry() else {
        output(
            out,
            "stderr",
            "The program did not produce an Aevia module.\n",
        );
        return 1;
    };
    let files = Files::load(&program);
    {
        let mut state = control.state();
        if state.ended {
            return 1;
        }
        let sites = files.collect_sites(&program);
        for (path, breakpoints) in &state.breakpoints {
            for bp in breakpoints {
                event(
                    out,
                    "breakpoint",
                    json!({ "reason": "changed", "breakpoint": bp.wire(path, Some(&sites), coords) }),
                );
            }
        }
        state.sites = Some(sites);
        if launch.stop_on_entry {
            state.resume = Resume::Entry;
        }
    }
    let observer = Rc::new(Observer {
        out: out.clone(),
        control: control.clone(),
        files,
        coords,
        last_line: RefCell::default(),
        no_debug: launch.no_debug,
    });
    let mut interp = Interp::new(&entry.checked);
    super::apply_options(&mut interp, options);
    interp.set_debugger(observer.clone());
    interp.set_cancellation_token(control.cancellation.clone());
    interp.set_args(launch.args);
    let input: RefCell<VecDeque<_>> = RefCell::new(launch.stdin.into());
    #[allow(
        clippy::result_large_err,
        reason = "the host line-reader API shares the interpreter's runtime error type"
    )]
    let read_line = move || Ok(input.borrow_mut().pop_front().unwrap_or_default());
    interp.set_line_reader(Rc::new(read_line));
    let diagnostics = aevia_core::driver::run_modules(&mut interp, &program);
    if !diagnostics.is_empty() {
        output(
            out,
            "stderr",
            &format!("{}\n", diagnostics.render(&program.sources)),
        );
    }
    if diagnostics.has_errors() {
        return 1;
    }
    match interp.run_main() {
        | Ok(Some(_)) => 0,
        | Ok(None) => {
            output(out, "stderr", "The program has no `main`.\n");
            1
        },
        | Err(error) => {
            if !control.cancellation.is_cancelled() {
                output(
                    out,
                    "stderr",
                    &format!("{}\n", error.diagnostic().render(&program.sources)),
                );
                observer.exception(&error);
            }
            1
        },
    }
}
