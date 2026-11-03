//! `aevia` — the command line interface.
//!
//! The commands are the ones of §9.9: `run`, `check`, `repl`, `test`, `fmt`,
//! `doc`, `bench`, `init`, `add`, `update`. Each one shares the same pipeline
//! (`compile` from `aevia-core`), so a program that type-checks in `check`
//! behaves the same in `run`.
//!
//! `clean` and `lsp` are deliberately absent from the dispatch table: there is
//! nothing to clean (no build cache is written) and there is no `aevia-lsp`
//! crate. A command that accepted a request and did nothing would be worse than
//! one that is not there.

mod package;
mod tooling;

use aevia_core::driver::{self, Program};
use aevia_core::manifest::Package;
use aevia_core::{compile, Interp, Output};
use aevia_syntax::{DeclKind, Diagnostics, Style};
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The options that apply to every command.
///
/// They are parsed before dispatch so the stack size is known *before* the
/// worker thread is spawned — a flag applied after the thread exists could not
/// change its stack.
#[derive(Clone, Copy, Debug)]
struct Options {
    /// Stack the interpreter runs on, in MiB.
    max_stack_mb: usize,
    /// Tail-call budget; `None` keeps the interpreter's default.
    budget: Option<usize>,
    /// When to colour diagnostics.
    color: ColorChoice,
}

/// The `--color` setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColorChoice {
    /// Colour when the output is a terminal.
    Auto,
    /// Always colour.
    Always,
    /// Never colour.
    Never,
}

impl Default for Options {
    fn default() -> Self {
        // The interpreter is recursive, so a deep but legal program needs more
        // stack than the 1 MiB a main thread gets on Windows.
        Self {
            max_stack_mb: 256,
            budget: None,
            color: ColorChoice::Auto,
        }
    }
}

impl Options {
    /// Splits `args` into options and the command line that follows.
    fn parse(args: &[String]) -> Result<(Self, Vec<String>), String> {
        let mut options = Self::default();
        let mut rest = Vec::new();
        let mut index = 0;
        while index < args.len() {
            let arg = args[index].clone();
            let (name, inline) = match arg.split_once('=') {
                Some((name, value)) => (name.to_string(), Some(value.to_string())),
                None => (arg.clone(), None),
            };
            let take = |what: &str, index: &mut usize| -> Result<String, String> {
                if let Some(value) = inline.clone() {
                    return Ok(value);
                }
                *index += 1;
                args.get(*index)
                    .cloned()
                    .ok_or_else(|| format!("`{what}` expects a value"))
            };
            match name.as_str() {
                "--max-stack" => {
                    let raw = take("--max-stack", &mut index)?;
                    options.max_stack_mb = raw.parse().map_err(|_| {
                        format!("`--max-stack` expects a number of MiB, found `{raw}`")
                    })?;
                    if options.max_stack_mb == 0 {
                        return Err("`--max-stack` must be at least 1 MiB".to_string());
                    }
                }
                "--color" => {
                    let raw = take("--color", &mut index)?;
                    options.color = match raw.as_str() {
                        "auto" => ColorChoice::Auto,
                        "always" => ColorChoice::Always,
                        "never" => ColorChoice::Never,
                        other => {
                            return Err(format!(
                                "`--color` expects auto, always or never, found `{other}`"
                            ));
                        }
                    };
                }
                "--budget" => {
                    let raw = take("--budget", &mut index)?;
                    options.budget = Some(raw.parse().map_err(|_| {
                        format!("`--budget` expects a number of tail calls, found `{raw}`")
                    })?);
                }
                _ => rest.push(arg),
            }
            index += 1;
        }
        Ok((options, rest))
    }
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let (options, args) = match Options::parse(&raw) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("aevia: {message}");
            return ExitCode::from(2);
        }
    };
    let stack = options.max_stack_mb.saturating_mul(1024 * 1024);
    // A dedicated thread also turns an exhausted stack into a normal `E0805`
    // diagnostic instead of a process abort.
    let worker = std::thread::Builder::new()
        .name("aevia".to_string())
        .stack_size(stack)
        .spawn(move || run_cli(options, args));
    match worker {
        Ok(handle) => handle.join().unwrap_or_else(|_| ExitCode::from(101)),
        Err(error) => {
            eprintln!("aevia: cannot start the interpreter thread: {error}");
            ExitCode::from(101)
        }
    }
}

/// The rendering style the options ask for.
fn style(options: Options) -> Style {
    let ansi = match options.color {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        // A pipe or a file gets no escape sequences.
        ColorChoice::Auto => std::io::stderr().is_terminal(),
    };
    if ansi { Style::Ansi } else { Style::Plain }
}

/// Applies the command-line options to an interpreter.
fn apply_options(interp: &mut Interp, options: Options) {
    if let Some(budget) = options.budget {
        interp.max_tail_calls = budget;
    }
}

fn run_cli(options: Options, args: Vec<String>) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("run") => run_command(options, &args[1..]),
        Some("check") => check_command(options, &args[1..]),
        Some("test") => test_command(options, &args[1..]),
        Some("repl") => repl(options),
        Some("fmt") => tooling::fmt(options, &args[1..]),
        Some("doc") => tooling::doc(options, &args[1..]),
        Some("bench") => tooling::bench(options, &args[1..]),
        Some("init") => package::init(&args[1..]),
        Some("add") => package::add(&args[1..]),
        Some("update") => package::update(&args[1..]),
        Some("rssn") => rssn_command(&args[1..]),
        Some("--version" | "-V") => {
            println!("aevia {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") | None => {
            help();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("aevia: unknown command `{other}`");
            help();
            ExitCode::from(2)
        }
    }
}

fn help() {
    println!(
        "aevia — a scientific language where dimensions are types

usage:
  aevia run [file.aev]      compile and run a program (the package entry by default)
  aevia check [file.aev]    type-check without running
  aevia test [file.aev]     run the `test` declarations
  aevia repl                interactive session
  aevia fmt [--check] [files…]  format source (the package's `src/` by default)
  aevia doc [--out DIR] [files…]  write Markdown API docs (default `docs/`)
  aevia bench [--filter PAT] [files…]  time every `@bench` function
  aevia init <name>         create a package: aevia.toml, src/main.aev, aevia.lock
  aevia add <path>          add a path dependency and re-resolve the lock
  aevia update              re-resolve aevia.lock
  aevia rssn <manifest>     audit a Kernel Manifest and print a summary
  aevia --version           print the version

options (anywhere on the command line):
  --max-stack <MiB>         stack the interpreter runs on (default 256)
  --budget <calls>          tail calls before a runaway loop is `E0805`
  --color <when>            auto, always, or never (default auto)

packages:
  there is no registry. `aevia add` takes a directory holding an `aevia.toml`,
  and `aevia update` re-resolves those paths only — a version range is never
  resolved, so a bare `aevia-std = \"0.1\"` in `[dependencies]` is `E0605`.

not implemented, and therefore not in the dispatch table: `clean`, `lsp`."
    );
}


fn report(diagnostics: &Diagnostics, sources: &aevia_syntax::SourceMap, style: Style) -> bool {
    if diagnostics.is_empty() {
        return false;
    }
    eprint!("{}", diagnostics.render_with(sources, style));
    diagnostics.has_errors()
}

/// The file a command works on: the argument, or the package's entry.
///
/// §9.9 writes `aevia run [file]`, so inside a package the file is optional.
fn entry_path(args: &[String], command: &str) -> Result<PathBuf, ExitCode> {
    if let Some(first) = args.first() {
        if first.starts_with('-') {
            eprintln!("aevia {command}: unknown option `{first}`");
            return Err(ExitCode::from(2));
        }
        if let Some(extra) = args.get(1) {
            // `run file -- args` is in §9.9, but this runtime calls a
            // zero-argument `main`, so there is nowhere for an argument to go.
            // Saying so is better than dropping it.
            eprintln!(
                "aevia {command}: unexpected argument `{extra}`; this runtime calls a \
                 zero-argument `main`, so a program cannot take arguments yet"
            );
            return Err(ExitCode::from(2));
        }
        return Ok(PathBuf::from(first));
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match Package::find(&cwd) {
        Ok(Some(package)) => Ok(package.entry_path()),
        Ok(None) => {
            eprintln!(
                "aevia {command}: expected a file (there is no `aevia.toml` in `{}` or any parent \
                 to take the entry from)",
                cwd.display()
            );
            Err(ExitCode::from(2))
        }
        Err(error) => {
            eprintln!("aevia {command}: {}", error.render());
            Err(ExitCode::from(1))
        }
    }
}

/// Loads a program, resolving its imports in its package when it has one.
///
/// A file that is not under a package's `src/` keeps the older resolution —
/// the entry file's directory — so `aevia run examples/modules/main.aev` still
/// reads `examples/modules/Geometry/Shapes.aev`.
fn load_at(path: &Path) -> Program {
    match Package::owning(path) {
        Ok(Some(package)) => match package.roots() {
            Ok(roots) => driver::load_with_roots(path, roots),
            Err(error) => manifest_failure(&error),
        },
        Ok(None) => driver::load(path),
        Err(error) => manifest_failure(&error),
    }
}

fn manifest_failure(error: &aevia_core::manifest::ManifestError) -> Program {
    let mut diagnostics = Diagnostics::new();
    diagnostics.push(aevia_syntax::Diagnostic::error(
        error.code,
        error.message.clone(),
        aevia_syntax::Span::default(),
    ));
    Program {
        sources: aevia_syntax::SourceMap::new(),
        modules: Vec::new(),
        diagnostics,
    }
}

fn run_command(options: Options, args: &[String]) -> ExitCode {
    let path = match entry_path(args, "run") {
        Ok(path) => path,
        Err(code) => return code,
    };
    let program = match load_program(&path, style(options)) {
        Ok(program) => program,
        Err(code) => return code,
    };
    if report(&program.diagnostics, &program.sources, style(options)) {
        return ExitCode::from(1);
    }
    let Some(entry) = program.entry() else {
        eprintln!("aevia: no module produced");
        return ExitCode::from(1);
    };
    let mut interp = Interp::new(&entry.checked);
    apply_options(&mut interp, options);
    interp.output = Output::Console;
    let load_diagnostics = driver::run_modules(&mut interp, &program);
    if report(&load_diagnostics, &program.sources, style(options)) {
        return ExitCode::from(1);
    }
    match interp.run_main() {
        Ok(Some(_)) => ExitCode::SUCCESS,
        Ok(None) => {
            eprintln!("aevia: this program has no `main`");
            ExitCode::from(1)
        }
        Err(error) => {
            let mut diagnostics = Diagnostics::new();
            diagnostics.push(error.diagnostic());
            report(&diagnostics, &program.sources, style(options));
            ExitCode::from(1)
        }
    }
}

/// Loads a program and reports the diagnostics that stop it.
fn load_program(path: &Path, style: Style) -> Result<Program, ExitCode> {
    let program = load_at(path);
    if program.modules.is_empty() && program.diagnostics.has_errors() {
        report(&program.diagnostics, &program.sources, style);
        return Err(ExitCode::from(1));
    }
    Ok(program)
}

fn check_command(options: Options, args: &[String]) -> ExitCode {
    let path = match entry_path(args, "check") {
        Ok(path) => path,
        Err(code) => return code,
    };
    let program = load_at(&path);
    if report(&program.diagnostics, &program.sources, style(options)) {
        ExitCode::from(1)
    } else {
        let count = program.modules.len();
        if count > 1 {
            println!("{}: ok ({count} modules)", path.display());
        } else {
            println!("{}: ok", path.display());
        }
        ExitCode::SUCCESS
    }
}

fn test_command(options: Options, args: &[String]) -> ExitCode {
    let path = match entry_path(args, "test") {
        Ok(path) => path,
        Err(code) => return code,
    };
    let program = match load_program(&path, style(options)) {
        Ok(program) => program,
        Err(code) => return code,
    };
    if report(&program.diagnostics, &program.sources, style(options)) {
        return ExitCode::from(1);
    }
    let Some(entry) = program.entry() else {
        return ExitCode::from(1);
    };
    let mut interp = Interp::new(&entry.checked);
    apply_options(&mut interp, options);
    let load_diagnostics = driver::run_modules(&mut interp, &program);
    if report(&load_diagnostics, &program.sources, style(options)) {
        return ExitCode::from(1);
    }
    let module = &entry.module;
    let root = interp.root();
    let mut passed = 0;
    let mut failed = 0;
    for decl in &module.decls {
        if let DeclKind::Test { name, body } = &decl.kind {
            match interp.eval(body, &root) {
                Ok(aevia_core::value::Value::Bool(true)) => {
                    println!("test {name} ... ok");
                    passed += 1;
                }
                Ok(other) => {
                    println!(
                        "test {name} ... FAILED (returned {})",
                        aevia_core::value::show(&other)
                    );
                    failed += 1;
                }
                Err(error) => {
                    println!("test {name} ... FAILED ({})", error.message);
                    failed += 1;
                }
            }
        }
    }
    println!("\n{passed} passed, {failed} failed");
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn rssn_command(args: &[String]) -> ExitCode {
    let registry = aevia_rssn::registry();
    if let Err(error) = aevia_rssn::check_compatibility(registry) {
        eprintln!("aevia rssn: {error}");
        return ExitCode::from(1);
    }
    let mut emit = None;
    let mut audit = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--emit" => {
                let Some(path) = args.get(index + 1) else {
                    eprintln!("aevia rssn: --emit needs a path");
                    return ExitCode::from(2);
                };
                emit = Some(path.clone());
                index += 2;
            }
            "--audit" => {
                audit = true;
                index += 1;
            }
            other => {
                eprintln!("aevia rssn: unknown argument `{other}`");
                return ExitCode::from(2);
            }
        }
    }

    println!("rssn {}", registry.version);
    println!("api version: {}", registry.api_version);
    println!("manifest: {}", registry.manifest_hash);
    let ops: Vec<_> = registry.ops().collect();
    let mut declared = 0;
    let mut overlay = 0;
    let mut derived = 0;
    let mut none = 0;
    for op in &ops {
        match op.source {
            aevia_rssn::Source::Declared => declared += 1,
            aevia_rssn::Source::Overlay => overlay += 1,
            aevia_rssn::Source::Derived => derived += 1,
            aevia_rssn::Source::None => none += 1,
        }
    }
    println!("operators: {}", ops.len());
    println!(
        "signatures: {declared} declared, {overlay} overlay, {derived} derived, {none} dynamic-only"
    );
    println!("units: {}", registry.units.len());
    println!("constants: {}", registry.constants.len());
    println!(
        "capabilities: jit={} higher_order={} structured_results={} exact_units={}",
        registry.capabilities.jit,
        registry.capabilities.higher_order,
        registry.capabilities.structured_results,
        registry.capabilities.exact_units
    );

    if audit {
        for op in &ops {
            if op.source == aevia_rssn::Source::None {
                continue;
            }
            if op.signatures.is_empty() {
                println!("{}:{} has no signature", op.rule_set, op.name);
            }
        }
    }

    if let Some(directory) = emit {
        if let Err(error) = std::fs::create_dir_all(&directory) {
            eprintln!("aevia rssn: cannot create `{directory}`: {error}");
            return ExitCode::from(1);
        }
        for (name, text) in aevia_rssn::codegen::files(registry) {
            let path = std::path::Path::new(&directory).join(name);
            match std::fs::write(&path, text) {
                Ok(()) => println!("wrote {}", path.display()),
                Err(error) => {
                    eprintln!("aevia rssn: cannot write `{}`: {error}", path.display());
                    return ExitCode::from(1);
                }
            }
        }
    }
    ExitCode::SUCCESS
}

/// One interpreter for the whole session (§9.8).
///
/// Each line used to recompile the accumulated text *and* build a fresh
/// interpreter, which re-ran every top-level `let` and its side effects and
/// reset `symbol_counts`, so `symbol("x")` on two lines produced the same
/// symbol. The session now appends declarations to one interpreter.
struct Session {
    declarations: String,
    interp: Option<Interp>,
    loaded: usize,
    /// The options every interpreter in this session is built with.
    options: Options,
}

impl Session {
    fn new(options: Options) -> Self {
        Self {
            declarations: String::new(),
            interp: None,
            loaded: 0,
            options,
        }
    }

    fn reset(&mut self) {
        self.declarations.clear();
        self.interp = None;
        self.loaded = 0;
    }

    /// Brings the interpreter in line with `compilation`, evaluating only the
    /// declarations that have not run yet.
    ///
    /// `limit` is the number of declarations to load; an expression line
    /// compiles as `… let __repl = <expr>`, and that synthetic declaration is
    /// evaluated by [`Session::evaluate`] instead of being loaded.
    fn sync(&mut self, compilation: &aevia_core::Compilation) -> Result<(), String> {
        self.sync_to(compilation, usize::MAX)
    }

    fn sync_to(
        &mut self,
        compilation: &aevia_core::Compilation,
        limit: usize,
    ) -> Result<(), String> {
        let Some(module) = &compilation.module else {
            return Ok(());
        };
        let interp = self
            .interp
            .get_or_insert_with(|| {
                let mut interp = Interp::new(&compilation.checked);
                apply_options(&mut interp, self.options);
                interp
            });
        // A later line may declare a unit or a dimension.
        interp.units = compilation.checked.units.clone();
        interp.dims = compilation.checked.dims.clone();
        // A later line may also call `Rssn.<op>`; its pinned key belongs to
        // that compilation (PLAN D1).
        for call in &compilation.checked.rssn_calls {
            interp.rssn_calls.insert(call.span, call.key.clone());
        }
        let limit = limit.min(module.decls.len());
        while self.loaded < limit {
            let decl = &module.decls[self.loaded];
            interp.load_decl(decl).map_err(|error| error.message)?;
            self.loaded += 1;
        }
        Ok(())
    }

    /// Evaluates an expression against the session's bindings.
    fn evaluate(
        &mut self,
        compilation: &aevia_core::Compilation,
        expression: &aevia_syntax::Expr,
    ) -> Result<aevia_core::value::Value, String> {
        let declarations = compilation
            .module
            .as_ref()
            .map_or(0, |module| module.decls.len())
            .saturating_sub(1);
        self.sync_to(compilation, declarations)?;
        let interp = self.interp.as_mut().ok_or("no session")?;
        let root = interp.root();
        interp.eval(expression, &root).map_err(|error| error.message)
    }
}

fn repl(options: Options) -> ExitCode {
    println!("Aevia — dimensions are types. Type `:help` for commands, `:quit` to exit.");
    let stdin = io::stdin();
    let mut session = Session::new(options);
    let mut lines = stdin.lock().lines();
    loop {
        print!("> ");
        let _ = io::stdout().flush();
        let Some(Ok(line)) = lines.next() else {
            println!();
            return ExitCode::SUCCESS;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(command) = trimmed.strip_prefix(':') {
            match handle_repl_command(command, &mut session) {
                ReplControl::Continue => continue,
                ReplControl::Quit => return ExitCode::SUCCESS,
            }
        }
        let mut input = line.clone();
        while needs_continuation(&input) {
            print!("  ");
            let _ = io::stdout().flush();
            match lines.next() {
                Some(Ok(next)) => {
                    input.push('\n');
                    input.push_str(&next);
                }
                _ => break,
            }
        }
        if starts_declaration(input.trim()) {
            let candidate = format!("{}{input}\n", session.declarations);
            let compilation = compile("<repl>", &candidate);
            if compilation.diagnostics.has_errors() {
                eprint!("{}", compilation.diagnostics.render(&compilation.sources));
            } else if let Err(message) = session.sync(&compilation) {
                eprintln!("{message}");
            } else {
                session.declarations = candidate;
                eprintln!("ok");
            }
        } else {
            let candidate = format!("{}let __repl = {input}\n", session.declarations);
            let compilation = compile("<repl>", &candidate);
            if compilation.diagnostics.has_errors() {
                eprint!("{}", compilation.diagnostics.render(&compilation.sources));
                continue;
            }
            let Some(module) = &compilation.module else {
                continue;
            };
            let Some(last) = module.decls.last() else {
                continue;
            };
            let aevia_syntax::DeclKind::Let { value, .. } = &last.kind else {
                continue;
            };
            let ty = compilation
                .checked
                .exports
                .get("__repl")
                .map(|export| match export {
                    aevia_types::Export::Value { scheme, .. } => scheme.ty.to_string(),
                    _ => String::new(),
                })
                .unwrap_or_default();
            match session.evaluate(&compilation, value) {
                Ok(value) => println!("{ty} = {}", aevia_core::value::show(&value)),
                Err(message) => eprintln!("{message}"),
            }
        }
    }
}

enum ReplControl {
    Continue,
    Quit,
}

fn handle_repl_command(command: &str, session: &mut Session) -> ReplControl {
    let mut parts = command.splitn(2, ' ');
    let name = parts.next().unwrap_or("");
    let argument = parts.next().unwrap_or("").trim();
    match name {
        "quit" | "q" => return ReplControl::Quit,
        "reset" => {
            session.reset();
            println!("session cleared");
        }
        "help" => {
            println!(
                ":type <expr>   show the type of an expression
:dims          list the base dimensions
:units [d]     list units, optionally for one dimension
:load <file>   load declarations from a file
:reset         clear the session
:quit          exit"
            );
        }
        "dims" => {
            let env = aevia_dim::DimEnv::si();
            for base in env.base_names() {
                println!("{base}");
            }
        }
        "units" => {
            let table = aevia_rssn::unit_table();
            let filter = if argument.is_empty() {
                None
            } else {
                aevia_dim::DimEnv::si().parse(argument).ok()
            };
            for unit in table.list() {
                if filter.as_ref().is_none_or(|dim| unit.dim == *dim) {
                    println!("{} ({})", unit.display, unit.dim.to_plain_string());
                }
            }
        }
        "type" => {
            // A builtin has a scheme, not just an instantiation; show it as
            // written in the standard library.
            if let Some(first) = aevia_types::builtins::schemes(argument)
                .and_then(|schemes| schemes.into_iter().next())
            {
                let quantified = if first.vars.is_empty() && first.dims.is_empty() {
                    String::new()
                } else {
                    let mut names: Vec<String> =
                        first.vars.iter().map(|id| format!("t{id}")).collect();
                    names.extend(first.dims.iter().map(|id| format!("d{id}")));
                    format!("forall {}. ", names.join(", "))
                };
                println!("{argument} : {quantified}{}", first.ty);
                return ReplControl::Continue;
            }
            let candidate = format!("{}let __repl_type = {argument}\n", session.declarations);
            let compilation = compile("<repl>", &candidate);
            if compilation.diagnostics.has_errors() {
                eprint!("{}", compilation.diagnostics.render(&compilation.sources));
            } else if let Some(aevia_types::Export::Value { scheme, .. }) =
                compilation.checked.exports.get("__repl_type")
            {
                println!("{}", scheme.ty);
            }
        }
        "load" => match std::fs::read_to_string(argument) {
            Ok(text) => {
                let candidate = format!("{}{text}\n", session.declarations);
                let compilation = compile(argument, &candidate);
                if compilation.diagnostics.has_errors() {
                    eprint!("{}", compilation.diagnostics.render(&compilation.sources));
                } else if let Err(message) = session.sync(&compilation) {
                    eprintln!("{message}");
                } else {
                    session.declarations = candidate;
                    println!("loaded {argument}");
                }
            }
            Err(error) => eprintln!("cannot read `{argument}`: {error}"),
        },
        other => eprintln!("unknown command `:{other}`"),
    }
    ReplControl::Continue
}

fn needs_continuation(input: &str) -> bool {
    let trimmed = input.trim_end();
    if trimmed.ends_with('=')
        || trimmed.ends_with("do")
        || trimmed.ends_with("then")
        || trimmed.ends_with("else")
        || trimmed.ends_with("->")
        || trimmed.ends_with("=>")
        || trimmed.ends_with('|')
    {
        return true;
    }
    let mut depth = 0i32;
    let mut in_string = false;
    let mut previous = '\0';
    for c in trimmed.chars() {
        if in_string {
            if c == '"' && previous != '\\' {
                in_string = false;
            }
        } else {
            match c {
                '"' => in_string = true,
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                _ => {}
            }
        }
        previous = c;
    }
    depth > 0 || in_string
}

fn starts_declaration(input: &str) -> bool {
    const PREFIXES: [&str; 12] = [
        "let ",
        "fn ",
        "type ",
        "unit ",
        "dimension ",
        "import ",
        "open ",
        "test ",
        "class ",
        "instance ",
        "derive ",
        "@",
    ];
    PREFIXES.iter().any(|prefix| input.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|item| (*item).to_string()).collect()
    }

    #[test]
    fn options_are_taken_from_anywhere_on_the_line() {
        // Both spellings have to work: a user who puts the flag after the
        // command is not making a mistake.
        let (before, rest) = Options::parse(&args(&["--budget", "10", "run", "a.aev"])).expect("parses");
        assert_eq!(before.budget, Some(10));
        assert_eq!(rest, args(&["run", "a.aev"]));

        let (after, rest) = Options::parse(&args(&["run", "--budget=10", "a.aev"])).expect("parses");
        assert_eq!(after.budget, Some(10));
        assert_eq!(rest, args(&["run", "a.aev"]));
    }

    #[test]
    fn a_bad_option_is_reported_rather_than_ignored() {
        // Silently ignoring an option is how a user ends up believing a limit
        // is in force when it is not.
        assert!(Options::parse(&args(&["--budget", "lots", "run"])).is_err());
        assert!(Options::parse(&args(&["--budget"])).is_err());
        assert!(Options::parse(&args(&["--max-stack", "0", "run"])).is_err());
        assert!(Options::parse(&args(&["--color", "sometimes", "run"])).is_err());
    }

    #[test]
    fn the_style_follows_the_color_choice() {
        let mut options = Options::default();
        options.color = ColorChoice::Always;
        assert_eq!(style(options), Style::Ansi);
        options.color = ColorChoice::Never;
        assert_eq!(style(options), Style::Plain);
    }

    /// Feeds declaration lines into one session, as the REPL does.
    fn declare(session: &mut Session, line: &str) {
        let candidate = format!("{}{line}\n", session.declarations);
        let compilation = compile("<repl>", &candidate);
        assert!(
            !compilation.diagnostics.has_errors(),
            "compile errors: {:?}",
            compilation
                .diagnostics
                .items
                .iter()
                .map(|item| (&item.code, &item.message))
                .collect::<Vec<_>>()
        );
        session.sync(&compilation).expect("loads");
        session.declarations = candidate;
    }

    /// Evaluates one expression line in the session.
    fn evaluate(session: &mut Session, line: &str) -> String {
        let candidate = format!("{}let __repl = {line}\n", session.declarations);
        let compilation = compile("<repl>", &candidate);
        assert!(
            !compilation.diagnostics.has_errors(),
            "compile errors: {:?}",
            compilation
                .diagnostics
                .items
                .iter()
                .map(|item| (&item.code, &item.message))
                .collect::<Vec<_>>()
        );
        let module = compilation.module.as_ref().expect("module");
        let Some(aevia_syntax::DeclKind::Let { value, .. }) = module.decls.last().map(|d| &d.kind)
        else {
            panic!("no expression");
        };
        let value = session.evaluate(&compilation, value).expect("evaluates");
        aevia_core::value::repr(&value)
    }

    #[test]
    fn a_session_keeps_its_bindings() {
        // §9.8: declarations accumulate. A fresh interpreter per line lost
        // every earlier binding.
        let mut session = Session::new(Options::default());
        declare(&mut session, "let f = fn (n: Int) => n + 1");
        assert_eq!(evaluate(&mut session, "f(41)"), "42");
    }

    #[test]
    fn a_declaration_runs_once() {
        // `symbol("q")` allocates `q`, then `q#2`. If the first line were
        // re-evaluated on the second, `s2` would be `q#3`.
        let mut session = Session::new(Options::default());
        declare(&mut session, "let s1 = symbol(\"q\")");
        declare(&mut session, "let s2 = symbol(\"q\")");
        assert_eq!(evaluate(&mut session, "repr(s1 + s2)"), "\"q + q#2\"");
    }

    #[test]
    fn resetting_clears_the_session() {
        let mut session = Session::new(Options::default());
        declare(&mut session, "let a = 1");
        session.reset();
        assert!(session.declarations.is_empty());
        assert!(session.interp.is_none());
    }
}
