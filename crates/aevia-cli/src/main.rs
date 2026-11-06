//! `aevia` — the command line interface.
//!
//! The commands are the ones of §9.9: `run`, `check`, `repl`, `test`, `fmt`,
//! `doc`, `bench`, `init`, `add`, `update`. Each one shares the same pipeline
//! (`compile` from `aevia-core`), so a program that type-checks in `check`
//! behaves the same in `run`.
//!
//! `clean` removes the `.aevia/` build directory of §9.11 — the directory the
//! toolchain designates for build products — and says plainly that no build
//! cache is written yet, so `clean` can never claim to have removed something
//! it did not. `lsp` starts the `aevia-lsp` server of §9.10 on standard I/O.
//!
//! The §9.9 flags `--release` and `--jobs` are accepted and reported as having
//! no effect: Aevia is interpreted and no phase of the toolchain is parallel,
//! so silently ignoring them would let a user believe an optimisation level or
//! a thread count was in force.

mod doctest;
mod package;
mod tooling;

use aevia_core::Interp;
use aevia_core::Output;
use aevia_core::driver::Program;
use aevia_core::driver::{
    self,
};
use aevia_core::manifest::Package;
use aevia_syntax::DeclKind;
use aevia_syntax::Diagnostics;
use aevia_syntax::Style;
use std::io::BufRead;
use std::io::IsTerminal;
use std::io::Write;
use std::io::{
    self,
};
use std::path::Path;
use std::path::PathBuf;
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
    /// `--release`: accepted, with a note that there is no optimisation level.
    release: bool,
    /// `--jobs N`: accepted, with a note that no phase is parallel.
    jobs: Option<usize>,
    /// `--locked`: the lock file must already match the package tree.
    locked: bool,
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
            release: false,
            jobs: None,
            locked: false,
        }
    }
}

impl Options {
    /// What the user asked for that this toolchain cannot honour.
    ///
    /// The §9.9 flags are accepted so a script written against the documented
    /// command line runs, but each is reported rather than dropped: a flag
    /// that is silently ignored leaves the user believing it took effect.
    fn notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if self.release {
            notes.push(
                "`--release` has no effect: Aevia is interpreted, so there is no optimisation \
                 level to select (§9.9)"
                    .to_string(),
            );
        }
        if let Some(jobs) = self.jobs {
            notes.push(format!(
                "`--jobs {jobs}` has no effect: no phase of this toolchain runs in parallel, so \
                 the value cannot schedule anything (§9.9)"
            ));
        }
        notes
    }

    /// Splits `args` into options and the command line that follows.
    fn parse(args: &[String]) -> Result<(Self, Vec<String>), String> {
        let mut options = Self::default();
        let mut rest = Vec::new();
        let mut index = 0;
        while index < args.len() {
            let arg = args[index].clone();
            let (name, inline) = match arg.split_once('=') {
                | Some((name, value)) => (name.to_string(), Some(value.to_string())),
                | None => (arg.clone(), None),
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
                | "--max-stack" => {
                    let raw = take("--max-stack", &mut index)?;
                    options.max_stack_mb = raw.parse().map_err(|_| {
                        format!("`--max-stack` expects a number of MiB, found `{raw}`")
                    })?;
                    if options.max_stack_mb == 0 {
                        return Err("`--max-stack` must be at least 1 MiB".to_string());
                    }
                },
                | "--color" => {
                    let raw = take("--color", &mut index)?;
                    options.color = match raw.as_str() {
                        | "auto" => ColorChoice::Auto,
                        | "always" => ColorChoice::Always,
                        | "never" => ColorChoice::Never,
                        | other => {
                            return Err(format!(
                                "`--color` expects auto, always or never, found `{other}`"
                            ));
                        },
                    };
                },
                | "--budget" => {
                    let raw = take("--budget", &mut index)?;
                    options.budget = Some(raw.parse().map_err(|_| {
                        format!("`--budget` expects a number of tail calls, found `{raw}`")
                    })?);
                },
                | "--release" => options.release = true,
                | "--locked" => options.locked = true,
                | "--jobs" => {
                    let raw = take("--jobs", &mut index)?;
                    let jobs: usize = raw.parse().map_err(|_| {
                        format!("`--jobs` expects a number of threads, found `{raw}`")
                    })?;
                    if jobs == 0 {
                        return Err("`--jobs` must be at least 1".to_string());
                    }
                    options.jobs = Some(jobs);
                },
                | _ => rest.push(arg),
            }
            index += 1;
        }
        Ok((options, rest))
    }
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let (options, args) = match Options::parse(&raw) {
        | Ok(parsed) => parsed,
        | Err(message) => {
            eprintln!("aevia: {message}");
            return ExitCode::from(2);
        },
    };
    let stack = options.max_stack_mb.saturating_mul(1024 * 1024);
    // A dedicated thread also turns an exhausted stack into a normal `E0805`
    // diagnostic instead of a process abort.
    let worker = std::thread::Builder::new()
        .name("aevia".to_string())
        .stack_size(stack)
        .spawn(move || run_cli(options, args));
    match worker {
        | Ok(handle) => handle.join().unwrap_or_else(|_| ExitCode::from(101)),
        | Err(error) => {
            eprintln!("aevia: cannot start the interpreter thread: {error}");
            ExitCode::from(101)
        },
    }
}

/// The rendering style the options ask for.
fn style(options: Options) -> Style {
    let ansi = match options.color {
        | ColorChoice::Always => true,
        | ColorChoice::Never => false,
        // A pipe or a file gets no escape sequences.
        | ColorChoice::Auto => std::io::stderr().is_terminal(),
    };
    if ansi {
        Style::Ansi
    } else {
        Style::Plain
    }
}

/// Applies the command-line options to an interpreter.
fn apply_options(
    interp: &mut Interp,
    options: Options,
) {
    if let Some(budget) = options.budget {
        interp.max_tail_calls = budget;
    }
}

fn run_cli(
    options: Options,
    args: Vec<String>,
) -> ExitCode {
    for note in options.notes() {
        eprintln!("aevia: note: {note}");
    }
    if options.locked {
        if let Err(code) = locked_precheck(&args) {
            return code;
        }
    }
    match args.first().map(String::as_str) {
        | Some("run") => run_command(options, &args[1..]),
        | Some("check") => check_command(options, &args[1..]),
        | Some("test") => test_command(options, &args[1..]),
        | Some("repl") => repl(options),
        | Some("fmt") => tooling::fmt(options, &args[1..]),
        | Some("doc") => tooling::doc(options, &args[1..]),
        | Some("bench") => tooling::bench(options, &args[1..]),
        | Some("clean") => package::clean(&args[1..]),
        | Some("lsp") => lsp_command(&args[1..]),
        | Some("init") => package::init(&args[1..]),
        | Some("add") => package::add(&args[1..], options.locked),
        | Some("update") => package::update(&args[1..], options.locked),
        | Some("rssn") => rssn_command(&args[1..]),
        | Some("--version" | "-V") => {
            println!("aevia {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        },
        | Some("--help" | "-h") | None => {
            help();
            ExitCode::SUCCESS
        },
        | Some(other) => {
            eprintln!("aevia: unknown command `{other}`");
            help();
            ExitCode::from(2)
        },
    }
}

/// Checks `--locked` before a command that reads a package runs.
///
/// `--locked` means "the lock file is part of the input, and an out-of-date
/// one is an error rather than something to rewrite". `update` and `add` own
/// their own handling because they are the commands that would write the lock.
fn locked_precheck(args: &[String]) -> Result<(), ExitCode> {
    match args.first().map(String::as_str) {
        | Some("run" | "check" | "test" | "fmt" | "doc" | "bench") => verify_lock(),
        // They handle `--locked` themselves: they may legitimately decide
        // whether a change is allowed.
        | Some("update" | "add") => Ok(()),
        | Some(other) => {
            eprintln!("aevia: `--locked` is not meaningful for `{other}`");
            Err(ExitCode::from(2))
        },
        | None => {
            eprintln!("aevia: `--locked` needs a command that reads a package");
            Err(ExitCode::from(2))
        },
    }
}

/// Fails when `aevia.lock` does not already match the package tree.
fn verify_lock() -> Result<(), ExitCode> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let package = match Package::find(&cwd) {
        | Ok(Some(package)) => package,
        | Ok(None) => {
            eprintln!(
                "aevia: `--locked` needs an `aevia.toml` in `{}` or any parent to read a lock \
                 file from",
                cwd.display()
            );
            return Err(ExitCode::from(2));
        },
        | Err(error) => {
            eprintln!("aevia: `--locked`: {}", error.render());
            return Err(ExitCode::from(1));
        },
    };
    let lock_path = package.dir.join("aevia.lock");
    let resolved = match package.resolve() {
        | Ok(lock) => lock,
        | Err(error) => {
            eprintln!("aevia: `--locked`: {}", error.render());
            return Err(ExitCode::from(1));
        },
    };
    let expected = resolved.render();
    let on_disk = std::fs::read_to_string(&lock_path).ok();
    if on_disk.as_deref() != Some(expected.as_str()) {
        eprintln!(
            "aevia: `--locked`: `{}` is out of date; run `aevia update` without `--locked`",
            lock_path.display()
        );
        return Err(ExitCode::from(1));
    }
    Ok(())
}

/// `aevia lsp [--stdio]` — the §9.10 server on standard I/O.
fn lsp_command(args: &[String]) -> ExitCode {
    if let Some(unknown) = args.iter().find(|argument| argument.as_str() != "--stdio") {
        eprintln!(
            "aevia lsp: `{unknown}` is not an option; the only transport is standard input/\
             output (`--stdio` is accepted because clients pass it)"
        );
        return ExitCode::from(2);
    }
    match aevia_lsp::serve_stdio() {
        | aevia_lsp::Exit::Graceful => ExitCode::SUCCESS,
        | aevia_lsp::Exit::Ungraceful => ExitCode::from(1),
    }
}

fn help() {
    println!(
        "aevia — a scientific language where dimensions are types

usage:
  aevia run [--example NAME] [file.aev] [-- args…]  compile and run a program
                            (the package entry by default; program arguments follow `--`)
  aevia check [--all] [file.aev…]  type-check without running
  aevia test [--filter PAT] [file.aev]  run the `test` declarations
  aevia test --doc [--filter PAT] [files…]  compile the ```aevia blocks of `##` docs
  aevia repl                interactive session
  aevia fmt [--check] [files…]  format source (the package's `src/` by default)
  aevia doc [--open] [--out DIR] [files…]  write Markdown API docs (default `docs/`)
  aevia bench [--filter PAT] [--iterations N] [--warmup N] [--save-baseline FILE]
              [--compare-baseline FILE] [--threshold RATIO] [files…]
                            time every `@bench` function and gate on a stored baseline
  aevia init <name>         create a package: aevia.toml, src/main.aev, aevia.lock
  aevia add <path>          add a path dependency and re-resolve the lock
  aevia update              re-resolve aevia.lock
  aevia clean               remove the package's `.aevia/` build directory
  aevia lsp [--stdio]       start the language server on standard input/output
  aevia rssn [--emit] [--out DIR]  audit a Kernel Manifest and generate the Rssn module
  aevia --version           print the version

options (anywhere on the command line):
  --max-stack <MiB>         stack the interpreter runs on (default 256)
  --budget <calls>          tail calls before a runaway loop is `E0805`
  --color <when>            auto, always, or never (default auto)
  --locked                  fail instead of rewriting an out-of-date aevia.lock
  --release                 accepted for §9.9; reported as having no effect
  --jobs <N>                accepted for §9.9; reported as having no effect

doctests (aevia test --doc, §7.5.5/§9.12):
  every `aevia` fenced block in a `##` doc comment is compiled. A block whose
  first line is `compile_fail` must fail to compile, and `compile_fail E0401`
  must fail with that code. Each snippet compiles as its own module: the
  declaring module's imports are copied in, but its declarations are not in
  scope, so a snippet has to be self-contained.

bench protocol (aevia bench, §9.12):
  compare only release builds (`cargo build --release`), on the same machine.
  Each workload reports the median of N iterations (default 100) and a 95%
  confidence interval. `--save-baseline FILE` writes JSON with the profile,
  iteration protocol and per-workload medians; `--compare-baseline FILE`
  compares medians and exits 1 when a workload is more than `--threshold`
  (default 0.05, also written `5%`) above its stored median.

editor support (aevia lsp, §9.10):
  the server analyses saved files and unsaved buffers through the package
  loader, so imports resolve in both; loaded programs are cached by the entry
  file's content hash and the mtime/length of every file under the package
  roots, and `workspace/symbol` lists declarations from every loaded module.

packages:
  there is no registry. `aevia add` takes a directory holding an `aevia.toml`,
  and `aevia update` re-resolves those paths only — a version range is never
  resolved, so a bare `aevia-std = \"0.1\"` in `[dependencies]` is `E0605`.

known gaps (each command says so rather than pretending):
  `clean` removes `.aevia/`, but this toolchain writes no build cache yet, so
  it normally reports that there is nothing to clean. `--release` and `--jobs`
  are accepted and reported as having no effect."
    );
}

fn report(
    diagnostics: &Diagnostics,
    sources: &aevia_syntax::SourceMap,
    style: Style,
) -> bool {
    if diagnostics.is_empty() {
        return false;
    }
    eprint!("{}", diagnostics.render_with(sources, style));
    diagnostics.has_errors()
}

/// The file a command works on: the argument, or the package's entry.
///
/// §9.9 writes `aevia run [file]`, so inside a package the file is optional.
fn entry_path(
    args: &[String],
    command: &str,
) -> Result<PathBuf, ExitCode> {
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
        | Ok(Some(package)) => Ok(package.entry_path()),
        | Ok(None) => {
            eprintln!(
                "aevia {command}: expected a file (there is no `aevia.toml` in `{}` or any parent \
                 to take the entry from)",
                cwd.display()
            );
            Err(ExitCode::from(2))
        },
        | Err(error) => {
            eprintln!("aevia {command}: {}", error.render());
            Err(ExitCode::from(1))
        },
    }
}

/// Loads a program, resolving its imports in its package when it has one.
///
/// A file that is not under a package's `src/` keeps the older resolution —
/// the entry file's directory — so `aevia run examples/modules/main.aev` still
/// reads `examples/modules/Geometry/Shapes.aev`.
fn load_at(path: &Path) -> Program {
    match Package::owning(path) {
        | Ok(Some(package)) => {
            match package.roots() {
                | Ok(roots) => driver::load_with_roots(path, roots),
                | Err(error) => manifest_failure(&error),
            }
        },
        | Ok(None) => driver::load(path),
        | Err(error) => manifest_failure(&error),
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
        dependencies: Vec::new(),
        modules: Vec::new(),
        diagnostics,
    }
}

fn run_command(
    options: Options,
    args: &[String],
) -> ExitCode {
    // §9.9: `aevia run [file] [-- args…]`. Everything after `--` belongs to
    // the program; without `--` an extra positional is a mistake, not an
    // argument, because the runtime cannot know which the user meant.
    let mut file: Option<String> = None;
    let mut example: Option<String> = None;
    let mut program_args: Vec<String> = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let argument = args[index].as_str();
        if argument == "--" {
            program_args = args[index + 1..].to_vec();
            break;
        }
        match argument {
            | "--example" => {
                index += 1;
                let Some(name) = args.get(index) else {
                    eprintln!("aevia run: `--example` expects an example name");
                    return ExitCode::from(2);
                };
                example = Some(name.clone());
            },
            | other if other.starts_with('-') => {
                eprintln!(
                    "aevia run: unknown option `{other}` (the options are `--example NAME`; \
                     program arguments go after `--`)"
                );
                return ExitCode::from(2);
            },
            | other if file.is_none() => file = Some(other.to_string()),
            | other => {
                eprintln!(
                    "aevia run: unexpected argument `{other}`; program arguments go after `--`, \
                     as in `aevia run file.aev -- {other}`"
                );
                return ExitCode::from(2);
            },
        }
        index += 1;
    }
    if example.is_some() && file.is_some() {
        eprintln!("aevia run: `--example` and a file are different programs; pass one of them");
        return ExitCode::from(2);
    }
    let path = if let Some(name) = example {
        match example_path(&name) {
            | Ok(path) => path,
            | Err(code) => return code,
        }
    } else {
        let files: Vec<String> = file.into_iter().collect();
        match entry_path(&files, "run") {
            | Ok(path) => path,
            | Err(code) => return code,
        }
    };
    let program = match load_program(&path, style(options)) {
        | Ok(program) => program,
        | Err(code) => return code,
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
    // Host arguments exist before the program's top-level declarations run.
    interp.set_args(program_args);
    let load_diagnostics = driver::run_modules(&mut interp, &program);
    if report(&load_diagnostics, &program.sources, style(options)) {
        return ExitCode::from(1);
    }
    match interp.run_main() {
        | Ok(Some(_)) => ExitCode::SUCCESS,
        | Ok(None) => {
            eprintln!("aevia: this program has no `main`");
            ExitCode::from(1)
        },
        | Err(error) => {
            let mut diagnostics = Diagnostics::new();
            diagnostics.push(error.diagnostic());
            report(&diagnostics, &program.sources, style(options));
            ExitCode::from(1)
        },
    }
}

/// The file `aevia run --example NAME` names.
///
/// §7.5.3 puts examples in the package's `examples/` directory; outside a
/// package the same directory next to the working directory is used. A missing
/// example lists the ones that exist, so a typo is fixable without `ls`.
fn example_path(name: &str) -> Result<PathBuf, ExitCode> {
    let stem = name.strip_suffix(".aev").unwrap_or(name);
    if stem.is_empty() || stem.contains(['/', '\\']) || stem.contains("..") || stem.starts_with('.')
    {
        eprintln!(
            "aevia run: `{name}` is not an example name; examples live directly in \
             `examples/` and have no path separators"
        );
        return Err(ExitCode::from(2));
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let base = match Package::find(&cwd) {
        | Ok(Some(package)) => package.dir.join("examples"),
        | Ok(None) => cwd.join("examples"),
        | Err(error) => {
            eprintln!("aevia run: {}", error.render());
            return Err(ExitCode::from(1));
        },
    };
    let path = base.join(format!("{stem}.aev"));
    if path.is_file() {
        return Ok(path);
    }
    let mut available: Vec<String> = std::fs::read_dir(&base)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| {
                    let path = entry.path();
                    if path.extension().is_some_and(|extension| extension == "aev") {
                        path.file_stem()
                            .map(|stem| stem.to_string_lossy().to_string())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    available.sort();
    if available.is_empty() {
        eprintln!(
            "aevia run: there is no example `{stem}` (and no `examples/` directory in `{}`)",
            base.display()
        );
    } else {
        eprintln!(
            "aevia run: there is no example `{stem}`; available examples in `{}`: {}",
            base.display(),
            available.join(", ")
        );
    }
    Err(ExitCode::from(2))
}

/// Loads a program and reports the diagnostics that stop it.
fn load_program(
    path: &Path,
    style: Style,
) -> Result<Program, ExitCode> {
    let program = load_at(path);
    if program.modules.is_empty() && program.diagnostics.has_errors() {
        report(&program.diagnostics, &program.sources, style);
        return Err(ExitCode::from(1));
    }
    Ok(program)
}

fn check_command(
    options: Options,
    args: &[String],
) -> ExitCode {
    let all = args.iter().any(|argument| argument == "--all");
    let files: Vec<String> = args
        .iter()
        .filter(|argument| argument.as_str() != "--all")
        .cloned()
        .collect();
    if let Some(unknown) = files.iter().find(|argument| argument.starts_with('-')) {
        eprintln!(
            "aevia check: unknown option `{unknown}` (the option is `--all`; a file is optional)"
        );
        return ExitCode::from(2);
    }
    if all {
        return check_all(options, &files);
    }
    // Expand a pattern the shell did not expand (`examples/*.aev` under
    // PowerShell), then check each file it named.
    let files: Vec<String> = package::expand_globs(&files)
        .into_iter()
        .map(|path| path.display().to_string())
        .collect();
    if files.len() > 1 {
        return check_files(options, &files);
    }
    let path = match entry_path(&files, "check") {
        | Ok(path) => path,
        | Err(code) => return code,
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

/// `aevia check <file>…`: one file per argument, each checked on its own.
fn check_files(
    options: Options,
    files: &[String],
) -> ExitCode {
    let mut failed = 0usize;
    for name in files {
        let path = PathBuf::from(name);
        let program = load_at(&path);
        if report(&program.diagnostics, &program.sources, style(options)) {
            failed += 1;
        } else {
            println!("{}: ok", path.display());
        }
    }
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// `aevia check --all`: every module of the package, not only the entry's
/// import graph.
///
/// `check` with no `--all` checks what the entry reaches. A module that
/// nothing imports is still part of the package (§7.5.3), and `--all` is the
/// command that checks it.
fn check_all(
    options: Options,
    files: &[String],
) -> ExitCode {
    if let Some(extra) = files.first() {
        eprintln!(
            "aevia check: `--all` checks every module of the package, so it takes no file \
             (found `{extra}`)"
        );
        return ExitCode::from(2);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let package = match Package::find(&cwd) {
        | Ok(Some(package)) => package,
        | Ok(None) => {
            eprintln!(
                "aevia check: `--all` needs an `aevia.toml` in `{}` or any parent",
                cwd.display()
            );
            return ExitCode::from(2);
        },
        | Err(error) => {
            eprintln!("aevia check: {}", error.render());
            return ExitCode::from(1);
        },
    };
    let paths = package::sources_under(&package.source_dir());
    if paths.is_empty() {
        eprintln!(
            "aevia check: `{}` holds no `.aev` files",
            package.source_dir().display()
        );
        return ExitCode::from(1);
    }
    let roots = match package.roots() {
        | Ok(roots) => roots,
        | Err(error) => {
            eprintln!("aevia check: {}", error.render());
            return ExitCode::from(1);
        },
    };
    // One loader for every entry: a module shared by several entries is parsed
    // and checked once, not once per entry.
    let (program, stats) =
        driver::load_all_with_roots(&paths, roots, &aevia_types::check::CheckOptions::default());
    // The loader names each source by the path it was asked for, so each entry
    // maps to the file id its diagnostics carry.
    let mut ids: std::collections::HashMap<String, aevia_syntax::FileId> =
        std::collections::HashMap::new();
    let mut file: aevia_syntax::FileId = 0;
    while let Some(source) = program.sources.file(file) {
        ids.entry(source.name.clone()).or_insert(file);
        file = file.saturating_add(1);
    }
    let mut failed = 0usize;
    let mut shown: std::collections::HashSet<aevia_syntax::FileId> =
        std::collections::HashSet::new();
    for path in &paths {
        let Some(&id) = ids.get(&path.display().to_string()) else {
            eprintln!("aevia check: `{}` could not be loaded", path.display());
            failed += 1;
            continue;
        };
        let mut own = Diagnostics::new();
        for item in &program.diagnostics.items {
            if item.span.file == id {
                own.push(item.clone());
            }
        }
        shown.insert(id);
        if report(&own, &program.sources, style(options)) {
            failed += 1;
        } else {
            println!("{}: ok", path.display());
        }
    }
    // A diagnostic in a file that is not one of the package's own modules (a
    // dependency's source) is still part of the run; show it once.
    let mut imported = Diagnostics::new();
    for item in &program.diagnostics.items {
        if !shown.contains(&item.span.file) {
            imported.push(item.clone());
        }
    }
    if report(&imported, &program.sources, style(options)) {
        failed += 1;
    }
    println!(
        "checked {} entries: {} modules, {} checks",
        stats.entries, stats.modules, stats.checks
    );
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn test_command(
    options: Options,
    args: &[String],
) -> ExitCode {
    let mut filter: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
    let mut doc = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            | "--filter" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    eprintln!("aevia test: `--filter` expects a pattern");
                    return ExitCode::from(2);
                };
                filter = Some(value.clone());
            },
            // §7.5.5/§9.12: compile the ```aevia blocks of the `##` docs.
            | "--doc" => doc = true,
            | other if other.starts_with('-') => {
                eprintln!(
                    "aevia test: unknown option `{other}` (the options are `--filter PAT` and \
                     `--doc`)"
                );
                return ExitCode::from(2);
            },
            | other => files.push(other.to_string()),
        }
        index += 1;
    }
    if doc {
        return doctest::run(options, &files, filter.as_deref());
    }
    let path = match entry_path(&files, "test") {
        | Ok(path) => path,
        | Err(code) => return code,
    };
    let program = match load_program(&path, style(options)) {
        | Ok(program) => program,
        | Err(code) => return code,
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
    let mut matched = 0usize;
    for decl in &module.decls {
        if let DeclKind::Test { name, body } = &decl.kind {
            if let Some(pattern) = &filter {
                if !name.contains(pattern.as_str()) {
                    continue;
                }
            }
            matched += 1;
            match interp.eval(body, &root) {
                | Ok(aevia_core::value::Value::Bool(true)) => {
                    println!("test {name} ... ok");
                    passed += 1;
                },
                | Ok(other) => {
                    println!(
                        "test {name} ... FAILED (returned {})",
                        aevia_core::value::show(&other)
                    );
                    failed += 1;
                },
                | Err(error) => {
                    println!("test {name} ... FAILED ({})", error.message);
                    failed += 1;
                },
            }
        }
    }
    if filter.is_some() && matched == 0 {
        // A filter that matches nothing must not read as "all tests passed".
        eprintln!(
            "aevia test: the filter `{}` matched no test",
            filter.as_deref().unwrap_or_default()
        );
        return ExitCode::from(1);
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
    let mut emit = false;
    let mut out: Option<String> = None;
    let mut audit = false;
    let mut namespaces = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            // `--emit` is a flag; §9.9 calls the destination `--out`. The old
            // `--emit DIR` spelling still works, because a directory after
            // `--emit` can only have been the destination.
            | "--emit" => {
                emit = true;
                if let Some(next) = args.get(index + 1) {
                    if !next.starts_with('-') {
                        out = Some(next.clone());
                        index += 1;
                    }
                }
                index += 1;
            },
            | "--out" => {
                let Some(path) = args.get(index + 1) else {
                    eprintln!("aevia rssn: `--out` needs a directory");
                    return ExitCode::from(2);
                };
                out = Some(path.clone());
                index += 2;
            },
            | "--namespaces" => {
                namespaces = true;
                index += 1;
            },
            | "--audit" => {
                audit = true;
                index += 1;
            },
            | other => {
                eprintln!("aevia rssn: unknown argument `{other}`");
                return ExitCode::from(2);
            },
        }
    }
    if out.is_some() && !emit {
        // A destination with nothing to emit into it is a request to emit.
        emit = true;
    }
    let emit_directory = if emit {
        let Some(directory) = out else {
            eprintln!(
                "aevia rssn: `--emit` needs a destination; write `aevia rssn --emit --out DIR`"
            );
            return ExitCode::from(2);
        };
        Some(directory)
    } else {
        None
    };

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
            | aevia_rssn::Source::Declared => declared += 1,
            | aevia_rssn::Source::Overlay => overlay += 1,
            | aevia_rssn::Source::Derived => derived += 1,
            | aevia_rssn::Source::None => none += 1,
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
        use aevia_rssn::bindings::Coverage;
        let catalog = aevia_rssn::bindings::catalog(registry);
        let typed = catalog
            .iter()
            .filter(|row| matches!(row.coverage, Coverage::Typed))
            .count();
        let dynamic = catalog
            .iter()
            .filter(|row| matches!(row.coverage, Coverage::Dynamic(_)))
            .count();
        let unavailable = catalog.len() - typed - dynamic;
        println!(
            "bindings: {typed} exact typed, {dynamic} dynamic/approximate, {unavailable} unavailable"
        );
        for row in &catalog {
            let (status, reasons): (&str, &[String]) = match &row.coverage {
                | Coverage::Typed => ("typed", &[]),
                | Coverage::Dynamic(reasons) => ("dynamic", reasons),
                | Coverage::Unavailable(reasons) => ("unavailable", reasons),
            };
            println!(
                "{}:{}[{}] -> {}: {}{}",
                row.op.rule_set,
                row.op.name,
                row.overload
                    .map_or_else(|| "unsigned".to_string(), |i| (i + 1).to_string()),
                row.name,
                status,
                if reasons.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", reasons.join("; "))
                }
            );
        }
    }
    if namespaces && emit_directory.is_none() {
        eprintln!("aevia rssn: --namespaces requires --emit --out DIR");
        return ExitCode::from(2);
    }

    if let Some(directory) = emit_directory {
        if let Err(error) = std::fs::create_dir_all(&directory) {
            eprintln!("aevia rssn: cannot create `{directory}`: {error}");
            return ExitCode::from(1);
        }
        let mut files = aevia_rssn::codegen::files(registry);
        if namespaces {
            files.extend(aevia_rssn::codegen::namespace_files(registry));
        }
        for (name, text) in files {
            let path = std::path::Path::new(&directory).join(name);
            match std::fs::write(&path, text) {
                | Ok(()) => println!("wrote {}", path.display()),
                | Err(error) => {
                    eprintln!("aevia rssn: cannot write `{}`: {error}", path.display());
                    return ExitCode::from(1);
                },
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
    /// Stable across source revisions; distinct for independent/reset sessions.
    identity: aevia_core::source::SourceIdentity,
}

fn repl_source_identity() -> aevia_core::source::SourceIdentity {
    static NEXT_SESSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let session = NEXT_SESSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    aevia_core::source::SourceIdentity::virtual_source(format!("aevia-repl://{}/{session}/Repl", std::process::id()))
}

impl Session {
    fn new(options: Options) -> Self {
        Self {
            declarations: String::new(),
            interp: None,
            loaded: 0,
            options,
            identity: repl_source_identity(),
        }
    }

    fn reset(&mut self) {
        self.declarations.clear();
        self.interp = None;
        self.loaded = 0;
        self.identity = repl_source_identity();
    }

    fn compile(&self, source: &str) -> aevia_core::Compilation {
        aevia_core::compile_with_source_identity("<repl>", source, &self.identity)
    }

    /// Brings the interpreter in line with `compilation`, evaluating only the
    /// declarations that have not run yet.
    ///
    /// `limit` is the number of declarations to load; an expression line
    /// compiles as `… let __repl = <expr>`, and that synthetic declaration is
    /// evaluated by [`Session::evaluate`] instead of being loaded.
    fn sync(
        &mut self,
        compilation: &aevia_core::Compilation,
    ) -> Result<(), String> {
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
        let interp = self.interp.get_or_insert_with(|| {
            let mut interp = Interp::new(&compilation.checked);
            apply_options(&mut interp, self.options);
            interp
        });
        let already_loaded = self.loaded.min(module.decls.len());
        interp.prepare_incremental_module(&compilation.checked, &module.decls[..already_loaded]);
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
        interp
            .eval(expression, &root)
            .map_err(|error| error.message)
    }
}

fn repl(options: Options) -> ExitCode {
    println!("Aevia — dimensions are types. Type `:help` for commands, `:quit` to exit.");
    let stdin = io::stdin();
    let mut session = Session::new(options);
    let mut lines = stdin.lock().lines().peekable();
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
                | ReplControl::Continue => continue,
                | ReplControl::Quit => return ExitCode::SUCCESS,
            }
        }
        // A block opened with `=`/`do`/`where`/`with` is one input: read
        // until the construct is closed instead of compiling the first line on
        // its own. The look-ahead only starts for an opener — a complete line
        // must be evaluated when Enter is pressed, not after the next one.
        let mut input = line.clone();
        if needs_continuation(&input) {
            loop {
                let next = lines
                    .peek()
                    .and_then(|line| line.as_ref().ok())
                    .map(String::as_str);
                if !needs_continuation(&input) && !continues_block(&input, next) {
                    break;
                }
                print!("  ");
                let _ = io::stdout().flush();
                match lines.next() {
                    | Some(Ok(next)) => {
                        input.push('\n');
                        input.push_str(&next);
                    },
                    | _ => break,
                }
            }
        }
        if starts_declaration(input.trim()) {
            let candidate = format!("{}{input}\n", session.declarations);
            let compilation = session.compile(&candidate);
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
            let compilation = session.compile(&candidate);
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
                .map(|export| {
                    match export {
                        | aevia_types::Export::Value { scheme, .. } => scheme.ty.to_string(),
                        | _ => String::new(),
                    }
                })
                .unwrap_or_default();
            match session.evaluate(&compilation, value) {
                | Ok(value) => println!("{ty} = {}", aevia_core::value::show(&value)),
                | Err(message) => eprintln!("{message}"),
            }
        }
    }
}

enum ReplControl {
    Continue,
    Quit,
}

fn handle_repl_command(
    command: &str,
    session: &mut Session,
) -> ReplControl {
    let mut parts = command.splitn(2, ' ');
    let name = parts.next().unwrap_or("");
    let argument = parts.next().unwrap_or("").trim();
    match name {
        | "quit" | "q" => return ReplControl::Quit,
        | "reset" => {
            session.reset();
            println!("session cleared");
        },
        | "help" => {
            println!(
                ":type <expr>   show the type of an expression
:dims          list the base dimensions
:units [d]     list units, optionally for one dimension
:ast <expr>    show the parsed AST
:core <expr>   show the Core IR of the expression
:time <expr>   evaluate the expression and report how long it took
:latex <expr>  render the expression's value as LaTeX
:load <file>   load declarations from a file
:reset         clear the session
:quit          exit"
            );
        },
        | "dims" => {
            let env = aevia_dim::DimEnv::si();
            for base in env.base_names() {
                println!("{base}");
            }
        },
        | "units" => {
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
        },
        | "type" => {
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
            let compilation = session.compile(&candidate);
            if compilation.diagnostics.has_errors() {
                eprint!("{}", compilation.diagnostics.render(&compilation.sources));
            } else if let Some(aevia_types::Export::Value { scheme, .. }) =
                compilation.checked.exports.get("__repl_type")
            {
                println!("{}", scheme.ty);
            }
        },
        // The three inspection commands parse or type-check the expression but
        // never evaluate it, so they leave the session exactly as it was.
        | "ast" => {
            match repl_ast(argument) {
                | Ok(text) => println!("{text}"),
                | Err(message) => eprintln!("{message}"),
            }
        },
        | "core" => {
            match repl_core(session, argument) {
                | Ok(text) => println!("{text}"),
                | Err(message) => eprintln!("{message}"),
            }
        },
        | "time" => {
            match repl_time(session, argument) {
                | Ok(text) => println!("{text}"),
                | Err(message) => eprintln!("{message}"),
            }
        },
        | "latex" => {
            match repl_latex(session, argument) {
                | Ok(text) => println!("{text}"),
                | Err(message) => eprintln!("{message}"),
            }
        },
        | "load" => {
            match std::fs::read_to_string(argument) {
                | Ok(text) => {
                    let candidate = format!("{}{text}\n", session.declarations);
                    let compilation = aevia_core::compile_with_source_identity(argument, &candidate, &session.identity);
                    if compilation.diagnostics.has_errors() {
                        eprint!("{}", compilation.diagnostics.render(&compilation.sources));
                    } else if let Err(message) = session.sync(&compilation) {
                        eprintln!("{message}");
                    } else {
                        session.declarations = candidate;
                        println!("loaded {argument}");
                    }
                },
                | Err(error) => eprintln!("cannot read `{argument}`: {error}"),
            }
        },
        | other => eprintln!("unknown command `:{other}`"),
    }
    ReplControl::Continue
}

/// The text the REPL prints for a message, with the trailing newline removed.
fn rendered(
    diagnostics: &aevia_syntax::Diagnostics,
    sources: &aevia_syntax::SourceMap,
) -> String {
    diagnostics.render(sources).trim_end().to_string()
}

/// `:ast e` — parse `e` and render the tree.
///
/// Only the parser runs here: `:ast` answers "how did the parser read this
/// text", which is true even when the expression would not type-check. The
/// session's declarations are deliberately not prepended — the question is
/// about the expression alone, and an unbound name is not a syntax error.
fn repl_ast(expression: &str) -> Result<String, String> {
    if expression.is_empty() {
        return Err("`:ast` expects an expression, as in `:ast f(1) + 2`".to_string());
    }
    let candidate = format!("let __repl_ast = {expression}\n");
    let lexed = aevia_syntax::lex(0, &candidate).map_err(|diagnostics| {
        let mut sources = aevia_syntax::SourceMap::new();
        sources.add("<repl>", &candidate);
        rendered(&diagnostics, &sources)
    })?;
    let (module, diagnostics) = aevia_syntax::parse_module(0, &candidate, &lexed);
    if diagnostics.has_errors() {
        let mut sources = aevia_syntax::SourceMap::new();
        sources.add("<repl>", &candidate);
        return Err(rendered(&diagnostics, &sources));
    }
    let Some(decl) = module.decls.last() else {
        return Err("the expression did not parse".to_string());
    };
    let DeclKind::Let { value, .. } = &decl.kind else {
        return Err("the expression did not parse".to_string());
    };
    let mut out = String::new();
    print_ast(&ast_node(value), "", true, &mut out);
    Ok(out.trim_end().to_string())
}

/// `:core e` — type-check `e` in the session and render its Core IR (§9.2.2).
///
/// The lowering is `aevia_ir`'s, with the checker's pinned `Rssn` overloads and
/// class instances — this is the same Core IR the interpreter would run, not a
/// second rendering of the AST.
fn repl_core(
    session: &Session,
    expression: &str,
) -> Result<String, String> {
    if expression.is_empty() {
        return Err("`:core` expects an expression, as in `:core f(1) + 2`".to_string());
    }
    let compilation = compile_repl_expression(session, "core", expression)?;
    let module = compilation
        .module
        .as_ref()
        .ok_or("the expression did not parse")?;
    let decl = module.decls.last().ok_or("the expression did not parse")?;
    let lowered = aevia_ir::lower::lower_decl_with_runtime_evidence(
        decl,
        &compilation.checked.rssn_calls,
        &compilation.checked.dictionaries,
        &compilation.checked.literal_reps,
        &compilation.checked.method_refs,
        &compilation.checked.constructor_refs,
        &compilation.checked.expr_types,
    )
    .ok_or("this declaration has no Core IR")?;
    Ok(format!("{lowered:#?}"))
}

/// `:time e` — evaluate `e` once and report the wall-clock time.
///
/// One evaluation, so the number is what the user's expression costs; the
/// value is printed first because a timing without the result it timed is not
/// reproducible.
fn repl_time(
    session: &mut Session,
    expression: &str,
) -> Result<String, String> {
    if expression.is_empty() {
        return Err("`:time` expects an expression, as in `:time simplify(x^2)`".to_string());
    }
    let compilation = compile_repl_expression(session, "time", expression)?;
    let expression = repl_expression(&compilation)?.clone();
    // Loading the declarations the expression may use is not part of the
    // expression's cost, so it happens before the clock starts.
    let declarations = compilation
        .module
        .as_ref()
        .map_or(0, |module| module.decls.len())
        .saturating_sub(1);
    session.sync_to(&compilation, declarations)?;
    let interp = session.interp.as_mut().ok_or("no session")?;
    let root = interp.root();
    let start = std::time::Instant::now();
    let value = interp
        .eval(&expression, &root)
        .map_err(|error| error.message)?;
    let elapsed = start.elapsed();
    let shown = aevia_core::value::show(&value);
    Ok(format!("{shown}\ntime: {}", repl_duration(elapsed)))
}

/// `:latex e` — evaluate `e` and render the value with `to_latex`'s engine.
fn repl_latex(
    session: &mut Session,
    expression: &str,
) -> Result<String, String> {
    if expression.is_empty() {
        return Err("`:latex` expects an expression, as in `:latex sqrt(2 * h / g)`".to_string());
    }
    let compilation = compile_repl_expression(session, "latex", expression)?;
    let value = repl_value(session, &compilation)?;
    aevia_core::value::to_math(&value, aevia_core::value::MathSyntax::Latex)
}

/// Compiles `let __repl_<label> = <expression>` against the session's
/// declarations, failing with the rendered diagnostics.
fn compile_repl_expression(
    session: &Session,
    label: &str,
    expression: &str,
) -> Result<aevia_core::Compilation, String> {
    let candidate = format!(
        "{}let __repl_{label} = {expression}\n",
        session.declarations
    );
    let compilation = session.compile(&candidate);
    if compilation.diagnostics.has_errors() {
        return Err(rendered(&compilation.diagnostics, &compilation.sources));
    }
    Ok(compilation)
}

/// The synthetic expression declaration of a REPL compilation.
fn repl_expression<'a>(
    compilation: &'a aevia_core::Compilation
) -> Result<&'a aevia_syntax::Expr, String> {
    let module = compilation
        .module
        .as_ref()
        .ok_or("the expression did not parse")?;
    let decl = module.decls.last().ok_or("the expression did not parse")?;
    match &decl.kind {
        | DeclKind::Let { value, .. } => Ok(value),
        | _ => Err("the expression did not parse".to_string()),
    }
}

/// Evaluates the synthetic expression declaration against the session.
fn repl_value(
    session: &mut Session,
    compilation: &aevia_core::Compilation,
) -> Result<aevia_core::value::Value, String> {
    let expression = repl_expression(compilation)?.clone();
    session.evaluate(compilation, &expression)
}

/// A duration in the largest unit that keeps three significant decimals.
fn repl_duration(value: std::time::Duration) -> String {
    let nanos = value.as_nanos();
    if nanos < 1_000 {
        format!("{nanos} ns")
    } else if nanos < 1_000_000 {
        format!("{:.3} µs", value.as_secs_f64() * 1e6)
    } else if nanos < 1_000_000_000 {
        format!("{:.3} ms", value.as_secs_f64() * 1e3)
    } else {
        format!("{:.3} s", value.as_secs_f64())
    }
}

/// One node of the rendered AST.
struct AstNode {
    label: String,
    children: Vec<AstNode>,
}

impl AstNode {
    fn leaf(label: String) -> Self {
        Self {
            label,
            children: Vec::new(),
        }
    }
}

/// Builds the display tree of one expression.
///
/// Brackets are not in the AST (§9.2.1), so the tree is the precedence-resolved
/// shape: `(1 + 2) * 3` renders with `*` at the root.
fn ast_node(expr: &aevia_syntax::Expr) -> AstNode {
    use aevia_syntax::ast::BlockStmt;
    use aevia_syntax::ast::DoStmt;
    use aevia_syntax::ast::ExprKind;
    use aevia_syntax::ast::SliceAst;
    use aevia_syntax::ast::StrPiece;
    let node = |label: String, children: Vec<AstNode>| AstNode { label, children };
    match &expr.kind {
        | ExprKind::UnitLit => AstNode::leaf("UnitLit".to_string()),
        | ExprKind::Error => AstNode::leaf("Error".to_string()),
        | ExprKind::Bool(value) => AstNode::leaf(format!("Bool {value}")),
        | ExprKind::Int(value) => AstNode::leaf(format!("Int {value}")),
        | ExprKind::Float(text) => AstNode::leaf(format!("Float {text}")),
        | ExprKind::Str(pieces) => {
            node(
                "Str".to_string(),
                pieces
                    .iter()
                    .filter_map(|piece| {
                        match piece {
                            | StrPiece::Expr(inner) => Some(ast_node(inner)),
                            | StrPiece::Text(_) => None,
                        }
                    })
                    .collect(),
            )
        },
        | ExprKind::Quantity { value, unit } => {
            let coefficient = match value {
                | aevia_syntax::ast::NumberLit::Int(value) => value.to_string(),
                | aevia_syntax::ast::NumberLit::Decimal(text) => text.clone(),
            };
            AstNode::leaf(format!("Quantity {coefficient} {}", unit.to_source()))
        },
        | ExprKind::Var(name) => AstNode::leaf(format!("Var {name}")),
        | ExprKind::QName(path) => AstNode::leaf(format!("QName {}", path.join("."))),
        | ExprKind::Unary { op, operand } => node(format!("Unary {op:?}"), vec![ast_node(operand)]),
        | ExprKind::Binary { op, left, right } => {
            node(
                format!("Binary {}", op.as_str()),
                vec![ast_node(left), ast_node(right)],
            )
        },
        | ExprKind::Call { callee, args } => {
            let mut children = vec![ast_node(callee)];
            children.extend(args.iter().map(ast_node));
            node("Call".to_string(), children)
        },
        | ExprKind::TypeApp { callee, args } => {
            let mut children = vec![ast_node(callee)];
            children.extend(args.iter().map(ty_node));
            node("TypeApp".to_string(), children)
        },
        | ExprKind::Lambda { params, ret, body } => {
            let mut children: Vec<AstNode> = params
                .iter()
                .map(|param| {
                    let mut node = AstNode::leaf(format!("param {:?}", param.pattern.kind));
                    if let Some(ty) = &param.ty {
                        node.children.push(ty_node(ty));
                    }
                    node
                })
                .collect();
            if let Some(ret) = ret {
                children.push(ty_node(ret));
            }
            children.push(ast_node(body));
            node("Lambda".to_string(), children)
        },
        | ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            node(
                "If".to_string(),
                vec![ast_node(cond), ast_node(then_branch), ast_node(else_branch)],
            )
        },
        | ExprKind::Match { scrutinee, arms } => {
            let mut children = vec![ast_node(scrutinee)];
            for arm in arms {
                let mut arm_node = AstNode::leaf(format!("arm {:?}", arm.pattern.kind));
                if let Some(guard) = &arm.guard {
                    arm_node.children.push(ast_node(guard));
                }
                arm_node.children.push(ast_node(&arm.body));
                children.push(arm_node);
            }
            node("Match".to_string(), children)
        },
        | ExprKind::LetIn {
            pattern,
            ty,
            value,
            body,
        } => {
            let mut children = vec![ast_node(value)];
            if let Some(ty) = ty {
                children.push(ty_node(ty));
            }
            children.push(AstNode::leaf(format!("bind {:?}", pattern.kind)));
            children.push(ast_node(body));
            node("LetIn".to_string(), children)
        },
        | ExprKind::Block { stmts, tail } => {
            let mut children = Vec::new();
            for stmt in stmts {
                match stmt {
                    | BlockStmt::Let {
                        pattern, ty, value, ..
                    } => {
                        let mut stmt_node = AstNode::leaf(format!("let {:?}", pattern.kind));
                        if let Some(ty) = ty {
                            stmt_node.children.push(ty_node(ty));
                        }
                        stmt_node.children.push(ast_node(value));
                        children.push(stmt_node);
                    },
                    | BlockStmt::Fn(decl) => {
                        children.push(AstNode::leaf(format!("fn {}", decl_name(decl))));
                    },
                }
            }
            if let Some(tail) = tail {
                children.push(ast_node(tail));
            }
            node("Block".to_string(), children)
        },
        | ExprKind::Do { stmts } => {
            let mut children = Vec::new();
            for stmt in stmts {
                match stmt {
                    | DoStmt::Let { pattern, value, .. } => {
                        let mut stmt_node = AstNode::leaf(format!("let {:?}", pattern.kind));
                        stmt_node.children.push(ast_node(value));
                        children.push(stmt_node);
                    },
                    | DoStmt::Bind { pattern, value, .. } => {
                        let mut stmt_node = AstNode::leaf(format!("bind {:?}", pattern.kind));
                        stmt_node.children.push(ast_node(value));
                        children.push(stmt_node);
                    },
                    | DoStmt::Expr(inner) => children.push(ast_node(inner)),
                }
            }
            node("Do".to_string(), children)
        },
        | ExprKind::Record { base, fields } => {
            let mut children: Vec<AstNode> = base.iter().map(|base| ast_node(base)).collect();
            children.extend(fields.iter().map(|(name, value)| {
                let mut field = AstNode::leaf(format!("field {name}"));
                field.children.push(ast_node(value));
                field
            }));
            node("Record".to_string(), children)
        },
        | ExprKind::Field { base, name } => node(format!("Field .{name}"), vec![ast_node(base)]),
        | ExprKind::Tuple(items) => node("Tuple".to_string(), items.iter().map(ast_node).collect()),
        | ExprKind::Tensor(items) => {
            node("Tensor".to_string(), items.iter().map(ast_node).collect())
        },
        | ExprKind::Index { base, indices } => {
            let mut children = vec![ast_node(base)];
            for index in indices {
                match index {
                    | SliceAst::Index(inner) => children.push(ast_node(inner)),
                    | SliceAst::Range {
                        start,
                        end,
                        inclusive,
                        step,
                    } => {
                        let mut range = AstNode::leaf(format!("range inclusive={inclusive}"));
                        for part in [start, end, step].into_iter().flatten() {
                            range.children.push(ast_node(part));
                        }
                        children.push(range);
                    },
                }
            }
            node("Index".to_string(), children)
        },
        | ExprKind::Annot { expr, ty } => {
            node("Annot".to_string(), vec![ast_node(expr), ty_node(ty)])
        },
        | ExprKind::Placeholder => AstNode::leaf("Placeholder".to_string()),
        | ExprKind::Try(inner) => node("Try".to_string(), vec![ast_node(inner)]),
        | ExprKind::Factorial { expr, double } => {
            node(
                if *double {
                    "Factorial !!".to_string()
                } else {
                    "Factorial !".to_string()
                },
                vec![ast_node(expr)],
            )
        },
    }
}

/// A type annotation as a leaf node, printed with the formatter's type printer
/// so it reads exactly as the source would.
fn ty_node(ty: &aevia_syntax::ast::Ty) -> AstNode {
    AstNode::leaf(format!("type {}", type_source(ty)))
}

fn type_source(ty: &aevia_syntax::ast::Ty) -> String {
    aevia_fmt::print::Printer::new(Vec::new())
        .type_(ty)
        .unwrap_or_else(|_| "<unprintable>".to_string())
}

fn decl_name(decl: &aevia_syntax::ast::Decl) -> String {
    match &decl.kind {
        | DeclKind::Fn { name, .. }
        | DeclKind::Type { name, .. }
        | DeclKind::Class { name, .. }
        | DeclKind::Dimension { name, .. }
        | DeclKind::Unit { name, .. } => name.clone(),
        | _ => "<declaration>".to_string(),
    }
}

/// Renders one AST node and its subtree.
fn print_ast(
    node: &AstNode,
    prefix: &str,
    last: bool,
    out: &mut String,
) {
    out.push_str(prefix);
    out.push_str(if last { "└─ " } else { "├─ " });
    out.push_str(&node.label);
    out.push('\n');
    let child_prefix = format!("{prefix}{}", if last { "   " } else { "│  " });
    let count = node.children.len();
    for (index, child) in node.children.iter().enumerate() {
        print_ast(child, &child_prefix, index + 1 == count, out);
    }
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
        // `class … where`, `instance … where` and `match … with` all open a
        // block whose body is on the following lines.
        || trimmed.ends_with("where")
        || trimmed.ends_with("with")
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
                | '"' => in_string = true,
                | '(' | '[' | '{' => depth += 1,
                | ')' | ']' | '}' => depth -= 1,
                | _ => {},
            }
        }
        previous = c;
    }
    depth > 0 || in_string
}

/// True when `next` is part of an indented block `input` opened.
///
/// The REPL has to read a whole block before compiling it. Two things say the
/// block is not finished: the next line is more indented than the first, or
/// it is a `match` arm of an input that starts with `match`.
fn continues_block(
    input: &str,
    next: Option<&str>,
) -> bool {
    let Some(next) = next else {
        return false;
    };
    let first = input.lines().next().unwrap_or(input);
    let base = indentation(first);
    if next.trim().is_empty() {
        return true;
    }
    let trimmed = next.trim_start();
    if trimmed.starts_with('|') && first.contains("match") {
        return true;
    }
    indentation(next) > base
}

/// The number of leading space/tab bytes of a line.
fn indentation(line: &str) -> usize {
    line.len() - line.trim_start().len()
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
        let (before, rest) =
            Options::parse(&args(&["--budget", "10", "run", "a.aev"])).expect("parses");
        assert_eq!(before.budget, Some(10));
        assert_eq!(rest, args(&["run", "a.aev"]));

        let (after, rest) =
            Options::parse(&args(&["run", "--budget=10", "a.aev"])).expect("parses");
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
    fn declare(
        session: &mut Session,
        line: &str,
    ) {
        let candidate = format!("{}{line}\n", session.declarations);
        let compilation = session.compile(&candidate);
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
    fn evaluate(
        session: &mut Session,
        line: &str,
    ) -> String {
        let candidate = format!("{}let __repl = {line}\n", session.declarations);
        let compilation = session.compile(&candidate);
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

    #[test]
    fn the_ineffective_flags_are_parsed_and_reported() {
        let (options, rest) =
            Options::parse(&args(&["--release", "--jobs", "4", "--locked", "check"]))
                .expect("parses");
        assert!(options.release);
        assert_eq!(options.jobs, Some(4));
        assert!(options.locked);
        assert_eq!(rest, args(&["check"]));
        let notes = options.notes().join("\n");
        assert!(notes.contains("--release"), "{notes}");
        assert!(notes.contains("--jobs 4"), "{notes}");
        // A job count of zero is a mistake, not a request for the default.
        assert!(Options::parse(&args(&["--jobs", "0", "check"])).is_err());
        // Flags that take no value do not swallow the command.
        let (options, rest) = Options::parse(&args(&["--locked", "run", "a.aev"])).expect("parses");
        assert!(options.locked);
        assert_eq!(rest, args(&["run", "a.aev"]));
    }

    // --- §9.8: `:ast`, `:core`, `:time`, `:latex` --------------------------

    #[test]
    fn the_ast_command_shows_the_precedence_resolved_tree() {
        let text = repl_ast("f(1) + 2").expect("renders");
        assert!(text.contains("Binary +"), "{text}");
        assert!(text.contains("Call"), "{text}");
        assert!(text.contains("Var f"), "{text}");
        assert!(text.contains("Int 1") && text.contains("Int 2"), "{text}");

        // Brackets are not in the AST, so the tree shows `*` above `+`.
        let text = repl_ast("(1 + 2) * 3").expect("renders");
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].contains("Binary *"), "{text}");
        assert!(lines[1].contains("Binary +"), "{text}");
    }

    #[test]
    fn the_ast_command_is_about_syntax_not_types() {
        // `:ast` answers "how did the parser read this"; an unbound name is
        // not a syntax error, so the tree is still shown.
        let text = repl_ast("not_declared + 1").expect("parses");
        assert!(text.contains("Var not_declared"), "{text}");
        assert!(repl_ast("").is_err());
        assert!(repl_ast("f(").is_err());
    }

    #[test]
    fn the_core_command_shows_the_lowered_ir() {
        let session = Session::new(Options::default());
        let text = repl_core(&session, "1 + 2").expect("lowers");
        // The IR is ANF: a block ending in a tail, with the addition as a
        // statement of its own.
        assert!(text.contains("Block"), "{text}");
        assert!(text.contains("Binary"), "{text}");
        assert!(text.contains("Return"), "{text}");
        assert!(text.contains("Int"), "{text}");
        assert!(repl_core(&session, "").is_err());
    }

    #[test]
    fn the_time_command_prints_a_value_and_a_duration() {
        let mut session = Session::new(Options::default());
        declare(&mut session, "let a = 40");
        let text = repl_time(&mut session, "a + 2").expect("timed");
        assert!(text.contains("42"), "{text}");
        assert!(
            text.contains(" ns")
                || text.contains(" µs")
                || text.contains(" ms")
                || text.contains(" s\n")
                || text.ends_with(" s"),
            "no duration in {text:?}"
        );
    }

    #[test]
    fn the_latex_command_renders_the_value() {
        let mut session = Session::new(Options::default());
        assert_eq!(
            repl_latex(&mut session, "1/2").expect("renders"),
            r"\frac{1}{2}"
        );
        // A symbolic value goes through the same `to_latex` engine; a bare
        // symbol is typeset upright, which is what `to_latex("x")` does.
        declare(&mut session, "let s = symbol(\"x\")");
        assert_eq!(
            repl_latex(&mut session, "s").expect("renders"),
            r"\mathrm{x}"
        );
    }

    #[test]
    fn the_inspection_commands_leave_the_session_alone() {
        let mut session = Session::new(Options::default());
        declare(&mut session, "let a = 1");
        let loaded = session.loaded;
        let _ = repl_ast("a + 1").expect("ast");
        let _ = repl_core(&session, "a + 1").expect("core");
        assert_eq!(session.declarations, "let a = 1\n");
        assert_eq!(session.loaded, loaded, "inspection must not evaluate");
    }

    #[test]
    fn an_indented_block_is_one_input() {
        // §9.8: the REPL reads a whole indented block before compiling it.
        assert!(needs_continuation("test \"a\" ="));
        assert!(continues_block("test \"a\" =", Some("    let a = 1")));
        assert!(continues_block(
            "test \"a\" =\n    let a = 1",
            Some("    a == 1")
        ));
        // The block ends at the next unindented line — an ordinary
        // declaration or a `:command`.
        assert!(!continues_block(
            "test \"a\" =\n    let a = 1\n    a == 1",
            Some(":quit")
        ));
        assert!(!continues_block("let a = 1", Some("let b = 2")));
        // A `match` block is arms at the same indentation as the `match`.
        assert!(needs_continuation("match x with"));
        assert!(continues_block("match x with", Some("| A -> 1")));
        assert!(continues_block("match x with\n| A -> 1", Some("| _ -> 2")));
        assert!(!continues_block(
            "match x with\n| A -> 1",
            Some("let y = 2")
        ));
        // `class … where` and `instance … where` open their bodies too.
        assert!(needs_continuation("class Show a where"));
        assert!(needs_continuation("instance Show for Float where"));
        assert!(needs_continuation("let r = do"));
        // A complete line followed by EOF is not continued.
        assert!(!continues_block("let a = 1", None));
    }
    #[test]
    fn repl_sessions_and_reset_have_distinct_stable_source_origins() {
        let mut first = Session::new(Options::default());
        let second = Session::new(Options::default());
        let source = "type Token = | Token(Int)\n";
        let a = first.compile(source);
        let again = first.compile(&format!("{source}let other = 1\n"));
        let b = second.compile(source);
        let nominal = |checked: &aevia_types::CheckedModule| match checked.exports.get("Token").unwrap() {
            aevia_types::Export::Type { adt } | aevia_types::Export::TypeCtor { adt, .. } => aevia_types::adt::key_of(*adt),
            _ => panic!("type"),
        };
        assert_eq!(nominal(&a.checked), nominal(&again.checked));
        assert_ne!(nominal(&a.checked), nominal(&b.checked));
        first.reset();
        assert_ne!(nominal(&a.checked), nominal(&first.compile(source).checked));
    }

    #[test]
    fn repl_late_declarations_install_literal_dimension_and_constructor_evidence() {
        let mut session = Session::new(Options::default());
        declare(&mut session, "let original = symbol(\"session\")");
        declare(&mut session, "type Box = | Wrap(Int)");
        declare(&mut session, "fn get(): Box = Wrap(7)");
        declare(&mut session, "let floating: Float = 1");
        declare(&mut session, "let exact: Real = 1.25");
        let interp = session.interp.as_ref().unwrap();
        assert!(matches!(interp.globals.get("floating"), Some(aevia_core::value::Value::Num(aevia_core::value::NumVal { kind: aevia_core::value::NumKind::Float(_), .. }))));
        assert!(matches!(interp.globals.get("exact"), Some(aevia_core::value::Value::Num(aevia_core::value::NumVal { kind: aevia_core::value::NumKind::Exact(_), .. }))));
        assert_eq!(evaluate(&mut session, "get()"), "Wrap(7)");
        declare(&mut session, "fn reject_time(): Bool = match check<L>(dyn(2.0 s)) with\n    | Ok(_) -> false\n    | Err(_) -> true");
        assert_eq!(evaluate(&mut session, "reject_time()"), "true");
        assert_eq!(evaluate(&mut session, "repr(original)"), "\"session\"");
    }

    #[test]
    fn repl_appended_instances_keep_their_own_implementation_cursor() {
        let mut session = Session::new(Options::default());
        declare(&mut session, "class Pretty a where\n    pretty(x: a): String");
        declare(&mut session, "instance Pretty for Int where\n    pretty(x) = \"integer\"");
        declare(&mut session, "instance Pretty for Bool where\n    pretty(x) = \"boolean\"");
        assert_eq!(evaluate(&mut session, "pretty(1)"), "\"integer\"");
        assert_eq!(evaluate(&mut session, "pretty(true)"), "\"boolean\"");
    }

    #[test]
    fn repl_private_and_public_instances_are_bound_by_source_site_after_append() {
        let mut session = Session::new(Options::default());
        declare(&mut session, "private class Hidden a where\n    hidden(x: a): String");
        declare(&mut session, "class Visible a where\n    visible(x: a): String");
        declare(&mut session, "instance Hidden for Int where\n    hidden(x) = \"private\"");
        declare(&mut session, "instance Visible for Bool where\n    visible(x) = \"public\"");
        declare(&mut session, "let first_class = hidden");
        assert_eq!(evaluate(&mut session, "hidden(1)"), "\"private\"");
        assert_eq!(evaluate(&mut session, "visible(true)"), "\"public\"");
        assert_eq!(evaluate(&mut session, "first_class(1)"), "\"private\"");
    }

    #[test]
    fn repl_core_inspection_uses_the_same_literal_and_method_identity_evidence_as_execution() {
        let mut session = Session::new(Options::default());
        declare(&mut session, "private class Hidden a where\n    hidden(x: a): String");
        declare(&mut session, "instance Hidden for Int where\n    hidden(x) = \"private\"");
        let method = repl_core(&session, "hidden").expect("method IR");
        assert!(method.contains("ClassMethodRef"), "{method}");
        assert!(method.contains("Hidden"), "{method}");
        let scalar = repl_core(&session, "(1 : Float)").expect("typed literal IR");
        assert!(scalar.contains("Float("), "{scalar}");
    }

}
