//! `aevia` — the command line interface.
//!
//! The commands are the ones of §9.9: `run`, `check`, `repl`, `test`, `fmt`,
//! `doc`, `bench`, `init`, `add`, `update`. Each one shares the same pipeline
//! (`compile` from `aevia-core`), so a program that type-checks in `check`
//! behaves the same in `run`.
//!
//! `clean` removes the `.aevia/` build directory of §9.11 — the directory the
//! toolchain designates for build products — preserving downloaded registry
//! sources unless `--registry` is supplied. `lsp` starts the `aevia-lsp` server
//! of §9.10 on standard I/O.
//!
//! The `--release` flag enables conservative Core IR optimizations.
//! `--jobs N` configures
//! the Rayon worker thread pool used for parallel evaluation (`par_map`).

#![cfg_attr(
    not(test),
    deny(
        missing_docs,
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::nursery,
        clippy::panic,
        clippy::pedantic,
        clippy::single_call_fn,
        clippy::unwrap_used
    )
)]

// CLI-owned output shares one colour policy; language programs and LSP write
// directly to their own streams and keep their byte-for-byte output.
macro_rules! print {
    ($($arg:tt)*) => { crate::terminal::write(format_args!($($arg)*), false, false) };
}
macro_rules! println {
    () => { crate::terminal::write(format_args!(""), false, true) };
    ($($arg:tt)*) => { crate::terminal::write(format_args!($($arg)*), false, true) };
}
macro_rules! eprint {
    ($($arg:tt)*) => { crate::terminal::write(format_args!($($arg)*), true, false) };
}
macro_rules! eprintln {
    () => { crate::terminal::write(format_args!(""), true, true) };
    ($($arg:tt)*) => { crate::terminal::write(format_args!($($arg)*), true, true) };
}

mod help;
mod lint_cmd;
mod terminal;

mod debugger;
mod doctest;
mod package;
mod registry;
mod repl_doc;
mod repl_input;
mod repl_interrupt;
mod repl_source;
mod toolchain;
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
use std::fmt::Write as _;
use std::io::Write;
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
    /// `--release`: optimize Core IR control flow before execution.
    release: bool,
    /// `--jobs N`: worker threads for parallel evaluation (§9.7); the default
    /// uses `AEVIA_JOBS`, then `RAYON_NUM_THREADS` or available CPUs.
    jobs: Option<usize>,
    /// `--locked`: the lock file must already match the package tree.
    locked: bool,
    /// `--caps fs,rand`: the runtime capabilities a program starts with
    /// (§9.14); `None` grants all of them.
    caps: Option<u8>,
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
            max_stack_mb: 1024,
            budget: None,
            color: ColorChoice::Auto,
            release: false,
            jobs: None,
            locked: false,
            caps: None,
        }
    }
}

impl Options {
    /// Splits `args` into options and the command line that follows.
    #[allow(
        clippy::single_call_fn,
        reason = "parses global options before creating the interpreter thread"
    )]
    fn parse(args: &[String]) -> Result<(Self, Vec<String>), String> {
        let mut options = Self::default();
        let mut rest = Vec::new();
        let mut index = 0;
        while let Some(argument) = args.get(index) {
            let arg = argument.clone();
            // Everything after `--` belongs to the program, including names
            // that happen to look like global interpreter flags.
            if arg == "--" {
                rest.extend(args.iter().skip(index).cloned());
                break;
            }
            let (name, inline) = match arg.split_once('=') {
                | Some((name, value)) => (name.to_string(), Some(value.to_string())),
                | None => (arg.clone(), None),
            };
            let take = |what: &str, index: &mut usize| -> Result<String, String> {
                if let Some(value) = inline.clone() {
                    return Ok(value);
                }
                *index = index.saturating_add(1);
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
                        format!("`--budget` expects a number of execution steps, found `{raw}`")
                    })?);
                },
                | "--caps" => {
                    let raw = take("--caps", &mut index)?;
                    options.caps = Some(aevia_core::value::caps::parse(&raw).map_err(|unknown| {
                        format!(
                            "`--caps` expects a comma-separated list of fs, net, env, rand, gpu, found `{unknown}`"
                        )
                    })?);
                },
                | "--release" | "--locked" if inline.is_some() => {
                    return Err(format!(
                        "`{name}` does not take a value; omit the flag to disable it"
                    ));
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
            index = index.saturating_add(1);
        }
        Ok((options, rest))
    }
}

fn main() -> ExitCode {
    if let Some(status) = toolchain::launch() {
        return status;
    }
    let raw: Vec<String> = std::env::args().skip(1).collect();
    terminal::configure(terminal::from_args(&raw));
    if let Some((command, args)) = raw.split_first()
        && command == "toolchain"
    {
        return toolchain::run(args);
    }
    let (options, args) = match Options::parse(&raw) {
        | Ok(parsed) => parsed,
        | Err(message) => {
            eprintln!("aevia: {message}");
            return ExitCode::from(2);
        },
    };
    terminal::configure(options.color);
    let stack = options.max_stack_mb.saturating_mul(1024 * 1024);
    // A dedicated thread also turns an exhausted stack into a normal `E0805`
    // diagnostic instead of a process abort.
    let worker = std::thread::Builder::new()
        .name("aevia".to_string())
        .stack_size(stack)
        .spawn(move || {
            aevia_core::install_stack_budget(stack);
            run_cli(options, &args)
        });
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
    let ansi = terminal::enabled(options.color, true);
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
    interp.optimize_ir = options.release;
    if options.release {
        interp.mode = aevia_core::Mode::Ir;
    }
    if let Some(budget) = options.budget {
        interp.max_tail_calls = budget;
        interp.max_io_steps = budget;
    }
    if let Some(caps) = options.caps {
        interp.capabilities = caps;
    }
    if let Some(jobs) = options.jobs {
        interp.set_jobs(jobs);
    }
    // The CLI thread declared its stack (`install_stack_budget`), so non-tail
    // recursion is bounded by the stack that is actually left; the fixed
    // call count stays only as a runaway guard.
    interp.max_depth = interp.max_depth.max(10_000_000);
    interp.max_io_depth = interp.max_io_depth.max(1_000_000);
}

#[allow(
    clippy::single_call_fn,
    reason = "this is the command router on the interpreter thread"
)]
fn run_cli(
    options: Options,
    args: &[String],
) -> ExitCode {
    if let Some(code) = help::route(args) {
        return code;
    }
    if options.locked
        && let Err(code) = locked_precheck(args)
    {
        return code;
    }
    let Some((command, tail)) = args.split_first() else {
        return repl(options);
    };
    match command.as_str() {
        | "run" => run_command(options, tail),
        | "check" => check_command(options, tail),
        | "lint" => lint_cmd::run(options, tail),
        | "watch" => watch_command(options, tail),
        | "test" => test_command(options, tail),
        | "repl" if tail.is_empty() => repl(options),
        | "repl" => {
            eprintln!(
                "aevia repl: unexpected argument `{}`; use aevia repl --help",
                tail.first().map_or("", String::as_str)
            );
            ExitCode::from(2)
        },
        | "fmt" => tooling::fmt(options, tail),
        | "doc" => tooling::doc(tail),
        | "bench" => tooling::bench(options, tail),
        | "clean" => package::clean(tail),
        | "lsp" => lsp_command(tail),
        | "debug" => debugger::serve(options, tail),
        | "completions" => completions_command(tail),
        | "init" => package::init(tail),
        | "add" => package::add(tail, options.locked),
        | "update" => package::update(tail, options.locked),
        | "fetch" => package::fetch(tail),
        | "tree" => package::tree(tail),
        | "rssn" => rssn_command(tail),
        // The registry client is a separate binary; `aevia pkg` forwards to it
        // so the toolchain has one entry point. See `registry.rs`.
        | "pkg" => registry::run(tail),
        // The commands people reach for most, without the `pkg` prefix.
        | "publish" => registry::publish(tail),
        | "docs" => registry::docs(tail),
        | command if registry::is_forwarded(command) => registry::run(args),
        | "--version" | "-V" => {
            println!("aevia {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        },
        | "--help" | "-h" => {
            help();
            ExitCode::SUCCESS
        },
        | other => {
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
#[allow(
    clippy::single_call_fn,
    reason = "keeps lock verification before the dispatched command's side effects"
)]
fn locked_precheck(args: &[String]) -> Result<(), ExitCode> {
    match args.first().map(String::as_str) {
        | Some("test") if args.iter().any(|arg| arg == "--doc") => Ok(()),
        | Some("run" | "check" | "lint" | "test" | "fmt" | "doc" | "bench") => verify_lock(),
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
#[allow(
    clippy::single_call_fn,
    reason = "reports lock errors for the package in the current directory"
)]
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
    verify_package_lock(&package)
}

fn verify_package_lock(package: &Package) -> Result<(), ExitCode> {
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

// Shell source is a fixture so argument validation does not get lost in the script.
const ZSH_COMPLETIONS: &str = "#compdef aevia\n\
             _aevia() {\n\
             \x20   local -a commands\n\
             \x20   commands=(\n\
             \x20       'run:Compile and run a program'\n\
             \x20       'check:Type-check without evaluating'\n\
             \x20       'lint:Run compiler and style lints'\n\
             \x20       'watch:Recheck when dependencies change'\n\
             \x20       'test:Run tests or doc examples'\n\
             \x20       'repl:Interactive session'\n\
             \x20       'fmt:Format Aevia source'\n\
             \x20       'doc:Generate API documentation'\n\
             \x20       'bench:Measure benchmarks'\n\
             \x20       'clean:Remove generated build products'\n\
             \x20       'lsp:Start the language server'\n\
             \x20       'debug:Start the source debug adapter'\n\
             \x20       'completions:Generate shell completions'
\
             \x20       'toolchain:Manage installed Aevia versions'\n\
             \x20       'init:Create a package'\n\
             \x20       'add:Add a dependency'\n\
             \x20       'update:Update lock file'\n\
             \x20       'fetch:Download locked dependencies'\n\
             \x20       'tree:Display dependency tree'\n\
             \x20       'rssn:Inspect the scientific kernel'\n\
             \x20       'pkg:Run the registry client'\n\
             \x20       'publish:Publish a package'\n\
             \x20       'docs:Locate package documentation'\n\
             \x20       'help:Show command help'\n\
             \x20   )\n\
             \x20   if (( CURRENT == 2 )); then\n\
             \x20       _describe 'command' commands\n\
             \x20   else\n\
             \x20       _files\n\
             \x20   fi\n\
             }\n\
             _aevia \"$@\"\n";

/// Generates shell completion source for the selected shell.
#[allow(
    clippy::single_call_fn,
    reason = "the completion command maintains its own shell-specific output"
)]
fn completions_command(args: &[String]) -> ExitCode {
    let [shell] = args else {
        eprintln!("aevia completions: expected <bash|zsh|fish|powershell|elvish>");
        return ExitCode::from(2);
    };
    let shell = shell.to_ascii_lowercase();
    let commands = "run check lint watch test repl fmt doc bench clean lsp debug completions toolchain init add update fetch tree rssn pkg publish docs login search info yank verify help";
    let script = match shell.as_str() {
        | "bash" => {
            format!(
                "_aevia_completions() {{\n\
             \x20   local cur prev\n\
             \x20   cur=\"${{COMP_WORDS[COMP_CWORD]}}\"\n\
             \x20   prev=\"${{COMP_WORDS[COMP_CWORD-1]}}\"\n\
             \x20   if [[ $COMP_CWORD -eq 1 ]]; then\n\
             \x20       COMPREPLY=( $(compgen -W \"{commands}\" -- \"$cur\") )\n\
             \x20       return 0\n\
             \x20   fi\n\
             \x20   case \"${{COMP_WORDS[1]}}\" in\n\
             \x20       completions)\n\
             \x20           COMPREPLY=( $(compgen -W \"bash zsh fish powershell elvish\" -- \"$cur\") )\n\
             \x20           return 0\n\
             \x20           ;;\n\
             \x20       doc)\n\
             \x20           COMPREPLY=( $(compgen -W \"--out --open --stdlib --json\" -- \"$cur\") )\n\
             \x20           return 0\n\
             \x20           ;;\n\
             \x20       run|check|test)\n\
             \x20           COMPREPLY=( $(compgen -W \"--relax-result --no-cache --help\" -f -- \"$cur\") )\n\
             \x20           return 0\n\
             \x20           ;;\n\
             \x20   esac\n\
             \x20   COMPREPLY=( $(compgen -f -- \"$cur\") )\n\
             }}\n\
             complete -F _aevia_completions aevia\n"
            )
        },
        | "zsh" => ZSH_COMPLETIONS.to_owned(),
        | "fish" => {
            let mut out = String::from("# Fish completions for aevia\n");
            for cmd in commands.split_whitespace() {
                let _ = writeln!(
                    out,
                    "complete -c aevia -n '__fish_use_subcommand' -a '{cmd}'"
                );
            }
            out.push_str("complete -c aevia -n '__fish_seen_subcommand_from completions' -a 'bash zsh fish powershell elvish'\n");
            out.push_str("complete -c aevia -n '__fish_seen_subcommand_from doc' -l stdlib -l json -l open -l out\n");
            out.push_str("complete -c aevia -n '__fish_seen_subcommand_from run check test' -l relax-result\n");
            out
        },
        | "powershell" => {
            format!(
                "Register-ArgumentCompleter -Native -CommandName aevia -ScriptBlock {{\n\
             \x20   param($wordToComplete, $commandAst, $cursorPosition)\n\
             \x20   $commands = @({})\n\
             \x20   $commands | Where-Object {{ $_ -like \"$wordToComplete*\" }} | ForEach-Object {{\n\
             \x20       [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)\n\
             \x20   }}\n\
             }}\n",
                commands
                    .split_whitespace()
                    .map(|c| format!("'{c}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        },
        | "elvish" => {
            format!(
                "set edit:completion:arg-completer[aevia] = {{|@args|\n\
             \x20   if (== (count $args) 2) {{\n\
             \x20       put {commands}\n\
             \x20   }}\n\
             }}\n"
            )
        },
        | other => {
            eprintln!(
                "aevia completions: unsupported shell `{other}` (expected bash, zsh, fish, powershell, or elvish)"
            );
            return ExitCode::from(2);
        },
    };
    print!("{script}");
    ExitCode::SUCCESS
}

#[allow(
    clippy::single_call_fn,
    reason = "the LSP entry point validates its invocation before taking standard streams"
)]
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
    help::overview();
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

fn report_with_relax(
    diagnostics: &Diagnostics,
    sources: &aevia_syntax::SourceMap,
    style: Style,
    relax_result: bool,
) -> bool {
    if !relax_result {
        return report(diagnostics, sources, style);
    }
    let mut filtered = Diagnostics::new();
    for item in &diagnostics.items {
        if item.code != "W0202" {
            filtered.push(item.clone());
        }
    }
    report(&filtered, sources, style)
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

/// Loads a program, resolving its imports in its package when it has one,
/// optionally through the per-module `.aevi` cache (§9.11) that lives in
/// `.aevia/cache/modules/` next to the entry file.
///
/// A file that is not under a package's `src/` keeps the older resolution —
/// the entry file's directory — so `aevia run examples/modules/main.aev` still
/// reads `examples/modules/Geometry/Shapes.aev`.
fn load_at_with(
    path: &Path,
    cached: bool,
) -> Program {
    let roots = match Package::owning(path) {
        | Ok(Some(package)) => {
            match package.roots() {
                | Ok(roots) => roots,
                | Err(error) => return manifest_failure(&error),
            }
        },
        | Ok(None) => aevia_core::manifest::Roots::adjacent(path),
        | Err(error) => return manifest_failure(&error),
    };
    if cached {
        let disk = aevia_core::disk_cache::DiskCache::beside(path);
        let (program, _) = driver::load_with_disk_cache(
            path,
            roots,
            &aevia_core::source::SourceOverlays::new(),
            &aevia_types::check::CheckOptions::default(),
            &disk,
        );
        program
    } else {
        driver::load_with_roots(path, roots)
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

#[allow(
    clippy::single_call_fn,
    clippy::too_many_lines,
    reason = "groups input validation, configured evaluation, and diagnostics for run"
)]
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
    let mut no_cache = false;
    let mut relax_result = false;
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        let argument = argument.as_str();
        if argument == "--" {
            program_args = args.iter().skip(index.saturating_add(1)).cloned().collect();
            break;
        }
        match argument {
            | "--no-cache" => no_cache = true,
            | "--relax-result" => relax_result = true,
            | "--example" => {
                index = index.saturating_add(1);
                let Some(name) = args.get(index) else {
                    eprintln!("aevia run: `--example` expects an example name");
                    return ExitCode::from(2);
                };
                example = Some(name.clone());
            },
            | other if other.starts_with('-') => {
                eprintln!(
                    "aevia run: unknown option `{other}` (the options are `--example NAME`, `--no-cache`, and `--relax-result`; \
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
        index = index.saturating_add(1);
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
    let program = match load_program_with(&path, style(options), !no_cache) {
        | Ok(program) => program,
        | Err(code) => return code,
    };
    if report_with_relax(
        &program.diagnostics,
        &program.sources,
        style(options),
        relax_result,
    ) {
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
    interp.set_args(program_args.clone());
    // §9.7: worker threads rebuild the program from the same entry file, so
    // closures cross by source span. A replica keeps the interpreter's own
    // call-depth guard: it has no `install_stack_budget` of its own.
    interp.set_replica(std::sync::Arc::new(move || {
        let program = load_at_with(&path, !no_cache);
        if program.diagnostics.has_errors() {
            return Err("the program no longer checks".to_string());
        }
        let entry = program.entry().ok_or("no module produced")?;
        let mut replica = Interp::new(&entry.checked);
        replica.optimize_ir = options.release;
        if options.release {
            replica.mode = aevia_core::Mode::Ir;
        }
        if let Some(budget) = options.budget {
            replica.max_tail_calls = budget;
            replica.max_io_steps = budget;
        }
        if let Some(caps) = options.caps {
            replica.capabilities = caps;
        }
        replica.set_args(program_args.clone());
        let diagnostics = driver::run_modules(&mut replica, &program);
        if diagnostics.has_errors() {
            return Err("the program's top level failed on a worker".to_string());
        }
        Ok(replica)
    }));
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
#[allow(
    clippy::single_call_fn,
    reason = "locates installed examples and reports valid names on failure"
)]
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
    let mut available: Vec<String> = std::fs::read_dir(&base).map_or_default(|entries| {
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
    });
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
    load_program_with(path, style, false)
}

/// [`load_program`] with the per-module `.aevi` cache switched on or off.
fn load_program_with(
    path: &Path,
    style: Style,
    cached: bool,
) -> Result<Program, ExitCode> {
    let program = load_at_with(path, cached);
    if program.modules.is_empty() && program.diagnostics.has_errors() {
        report(&program.diagnostics, &program.sources, style);
        return Err(ExitCode::from(1));
    }
    Ok(program)
}

#[allow(
    clippy::single_call_fn,
    reason = "the check command chooses file, package, and cache diagnostics"
)]
fn check_command(
    options: Options,
    args: &[String],
) -> ExitCode {
    let (mut cli_overrides, rest) = match lint_cmd::parse_lint_flags(args, "check") {
        | Ok(pair) => pair,
        | Err(code) => return code,
    };
    let all = rest.iter().any(|argument| argument == "--all");
    let no_cache = rest.iter().any(|argument| argument == "--no-cache");
    let relax_result = rest.iter().any(|argument| argument == "--relax-result");
    if relax_result {
        cli_overrides.push(aevia_lint::CliOverride {
            level: aevia_lint::Level::Allow,
            target: "W0202".to_string(),
        });
    }
    let files: Vec<String> = rest
        .iter()
        .filter(|argument| {
            argument.as_str() != "--all"
                && argument.as_str() != "--no-cache"
                && argument.as_str() != "--relax-result"
        })
        .cloned()
        .collect();
    if let Some(unknown) = files.iter().find(|argument| argument.starts_with('-')) {
        eprintln!(
            "aevia check: unknown option `{unknown}` (the options are `--all`, `--no-cache`, \
             `--relax-result`, and `-A/-W/-D/-F LINT`; a file is optional)"
        );
        return ExitCode::from(2);
    }
    if all {
        return check_all(options, &files, cli_overrides);
    }
    // Expand a pattern the shell did not expand (`examples/*.aev` under
    // PowerShell), then check each file it named.
    let files: Vec<String> = package::expand_globs(&files)
        .into_iter()
        .map(|path| path.display().to_string())
        .collect();
    if files.len() > 1 {
        return check_files(options, &files, &cli_overrides, no_cache);
    }
    let path = match entry_path(&files, "check") {
        | Ok(path) => path,
        | Err(code) => return code,
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let start_dir = path.parent().unwrap_or(&cwd);
    let lint_config = aevia_lint::LintConfig {
        manifest_lints: lint_cmd::load_manifest_lints(start_dir),
        cli_overrides,
        run_extra_lints: false,
    };
    let has_lint_overrides =
        !lint_config.manifest_lints.is_empty() || !lint_config.cli_overrides.is_empty();
    let cache = (!no_cache && !has_lint_overrides)
        .then(|| check_cache::Entry::for_entry(&path, style(options)));
    if let Some(Some(hit)) = cache.as_ref().map(check_cache::Entry::replay) {
        return hit;
    }
    let program = load_at_with(&path, !no_cache);
    let store = aevia_lint::LintStore::with_builtins();
    let root_files: Option<Vec<aevia_syntax::FileId>> = program.entry().map(|m| vec![m.file]);
    let lint_report =
        aevia_lint::Driver::run_program(&program, &store, &lint_config, root_files.as_deref());
    let rendered = if lint_report.diagnostics.is_empty() {
        String::new()
    } else {
        lint_report
            .diagnostics
            .render_with(&program.sources, style(options))
    };
    eprint!("{rendered}");
    let failed = lint_report.diagnostics.has_errors();
    let summary = if failed {
        String::new()
    } else {
        let count = program.modules.len();
        if count > 1 {
            format!("{}: ok ({count} modules)\n", path.display())
        } else {
            format!("{}: ok\n", path.display())
        }
    };
    print!("{summary}");
    if let Some(cache) = cache {
        cache.store(&program.dependencies, failed, &rendered, &summary);
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// The cross-process check cache (§9.11.2).
///
/// `aevia check <file>` records, under `.aevia/cache/` next to the entry,
/// every input the load observed — each file read with a hash of its text,
/// and each import candidate that did not exist — together with the
/// compiler's identity and the rendered result. A later `check` whose inputs
/// all still match replays the result without loading anything; any change
/// (an edited file, a newly created module that would now be imported, a
/// rebuilt compiler, another colour style) is a miss. The record describes a
/// whole program, so it is exact without serialising checked interfaces.
mod check_cache {
    use super::ExitCode;
    use super::Path;
    use super::PathBuf;
    use super::Style;

    pub struct Entry {
        file: PathBuf,
        header: String,
    }

    #[allow(
        clippy::single_call_fn,
        reason = "cache identity includes the exact compiler executable metadata"
    )]
    fn compiler_identity() -> String {
        std::env::current_exe()
            .and_then(|exe| std::fs::metadata(&exe).map(|meta| (exe, meta)))
            .map_or_default(|(exe, meta)| {
                format!(
                    "{}|{}|{:?}",
                    exe.display(),
                    meta.len(),
                    meta.modified().ok()
                )
            })
    }

    impl Entry {
        #[allow(
            clippy::single_call_fn,
            reason = "constructs a cache key and header for one entry file"
        )]
        pub(super) fn for_entry(
            entry: &Path,
            style: Style,
        ) -> Self {
            use std::hash::Hash as _;
            use std::hash::Hasher as _;
            let canonical = aevia_core::source::canonical_path(entry);
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            canonical.hash(&mut hasher);
            let directory = entry
                .parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
                .join(".aevia")
                .join("cache");
            Self {
                file: directory.join(format!("check-{:016x}.txt", hasher.finish())),
                header: format!(
                    "aevia-check-cache 1\ncompiler {}\nentry {}\nstyle {style:?}\n",
                    compiler_identity(),
                    canonical.display()
                ),
            }
        }

        /// Replays a matching record: its output and its exit status.
        #[allow(
            clippy::single_call_fn,
            reason = "keeps validation of a cached check report separate from cache-key generation"
        )]
        pub(super) fn replay(&self) -> Option<ExitCode> {
            let text = std::fs::read_to_string(&self.file).ok()?;
            let rest = text.strip_prefix(&self.header)?;
            let (inputs, output) = rest.split_once("\0status ")?;
            for line in inputs.lines() {
                let stamp = line.strip_prefix("dep ")?;
                if !aevia_core::source::stamp_is_current(stamp) {
                    return None;
                }
            }
            let (status, output) = output.split_once('\n')?;
            let (stderr, stdout) = output.split_once("\0stdout\n")?;
            eprint!("{stderr}");
            print!("{stdout}");
            Some(if status == "fail" {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            })
        }

        pub(super) fn store(
            &self,
            dependencies: &[aevia_core::source::SourceDependency],
            failed: bool,
            stderr: &str,
            stdout: &str,
        ) {
            let mut text = self.header.clone();
            for dependency in dependencies {
                text.push_str("dep ");
                text.push_str(&dependency.stamp());
                text.push('\n');
            }
            text.push_str(if failed {
                "\0status fail\n"
            } else {
                "\0status ok\n"
            });
            text.push_str(stderr);
            text.push_str("\0stdout\n");
            text.push_str(stdout);
            if let Some(directory) = self.file.parent()
                && std::fs::create_dir_all(directory).is_ok()
            {
                let _ = std::fs::write(&self.file, text);
            }
        }
    }
}

/// `aevia watch <file> [--once] [--runs N]` (§9.11): re-checks on every change.
///
/// The session keeps a [`driver::CheckCache`], so a module whose source and
/// imported interfaces did not change is not checked again: after the first
/// load the standard library and untouched package modules are reused, and an
/// edit to one file rechecks that file and only those importers whose view of
/// its interface changed. Each round prints how many modules were checked and
/// how many were reused. `--once` (or `--runs N`) stops after that many
/// rounds; the default runs until interrupted.
#[allow(
    clippy::single_call_fn,
    reason = "the watch command owns its incremental polling and diagnostics loop"
)]
fn watch_command(
    options: Options,
    args: &[String],
) -> ExitCode {
    let mut files = Vec::new();
    let mut runs: Option<usize> = None;
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        match argument.as_str() {
            | "--once" => runs = Some(1),
            | "--runs" => {
                index = index.saturating_add(1);
                let Some(count) = args.get(index).and_then(|text| text.parse().ok()) else {
                    eprintln!("aevia watch: `--runs` expects a count");
                    return ExitCode::from(2);
                };
                runs = Some(count);
            },
            | other if other.starts_with('-') => {
                eprintln!(
                    "aevia watch: unknown option `{other}` (the options are `--once`, `--runs N`)"
                );
                return ExitCode::from(2);
            },
            | other => files.push(other.to_string()),
        }
        index = index.saturating_add(1);
    }
    let path = match entry_path(&files, "watch") {
        | Ok(path) => path,
        | Err(code) => return code,
    };
    let roots = match Package::owning(&path) {
        | Ok(Some(package)) => {
            match package.roots() {
                | Ok(roots) => roots,
                | Err(error) => {
                    eprintln!("aevia watch: {}", error.message);
                    return ExitCode::from(1);
                },
            }
        },
        | Ok(None) => aevia_core::manifest::Roots::adjacent(&path),
        | Err(error) => {
            eprintln!("aevia watch: {}", error.message);
            return ExitCode::from(1);
        },
    };
    let cache = std::rc::Rc::new(std::cell::RefCell::new(driver::CheckCache::new()));
    let overlays = aevia_core::source::SourceOverlays::new();
    let mut round = 0usize;
    let mut failed;
    loop {
        round = round.saturating_add(1);
        let started = std::time::Instant::now();
        let (program, stats) = driver::load_incremental(
            &path,
            roots.clone(),
            &overlays,
            &aevia_types::check::CheckOptions::default(),
            &cache,
        );
        let elapsed = started.elapsed();
        failed = report(&program.diagnostics, &program.sources, style(options));
        let reused = cache.borrow().reused;
        println!(
            "[{round}] {}: {} ({} modules: {} checked, {reused} reused) in {:.1} ms",
            path.display(),
            if failed { "errors" } else { "ok" },
            stats.modules,
            stats.checks,
            elapsed.as_secs_f64() * 1000.0
        );
        if runs.is_some_and(|runs| round >= runs) {
            break;
        }
        // Wait until some observed input — a read file, or an import
        // candidate that did not exist — changes.
        loop {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if program
                .dependencies
                .iter()
                .any(|dependency| !dependency.is_current(&overlays))
            {
                break;
            }
        }
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// `aevia check <file>…`: one file per argument, each checked on its own.
#[allow(
    clippy::single_call_fn,
    reason = "individual roots use their own lint settings and check output"
)]
fn check_files(
    options: Options,
    files: &[String],
    cli_overrides: &[aevia_lint::CliOverride],
    no_cache: bool,
) -> ExitCode {
    let store = aevia_lint::LintStore::with_builtins();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut failed = 0usize;
    for name in files {
        let path = PathBuf::from(name);
        let start_dir = path.parent().unwrap_or(&cwd);
        let lint_config = aevia_lint::LintConfig {
            manifest_lints: lint_cmd::load_manifest_lints(start_dir),
            cli_overrides: cli_overrides.to_vec(),
            run_extra_lints: false,
        };
        let program = load_at_with(&path, !no_cache);
        let root_files: Option<Vec<aevia_syntax::FileId>> = program.entry().map(|m| vec![m.file]);
        let lint_report =
            aevia_lint::Driver::run_program(&program, &store, &lint_config, root_files.as_deref());
        if report(&lint_report.diagnostics, &program.sources, style(options)) {
            failed = failed.saturating_add(1);
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
#[allow(
    clippy::single_call_fn,
    clippy::too_many_lines,
    reason = "collects package roots and checks them in one compilation"
)]
fn check_all(
    options: Options,
    files: &[String],
    cli_overrides: Vec<aevia_lint::CliOverride>,
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
    let store = aevia_lint::LintStore::with_builtins();
    let lint_config = aevia_lint::LintConfig {
        manifest_lints: package.manifest.lints,
        cli_overrides,
        run_extra_lints: false,
    };
    let mut ids: std::collections::HashMap<String, aevia_syntax::FileId> =
        std::collections::HashMap::new();
    let mut file: aevia_syntax::FileId = 0;
    while let Some(source) = program.sources.file(file) {
        ids.entry(source.name.clone()).or_insert(file);
        file = file.saturating_add(1);
    }
    let root_ids: Vec<aevia_syntax::FileId> = paths
        .iter()
        .filter_map(|p| {
            ids.get(&aevia_core::source::canonical_path(p).display().to_string())
                .copied()
        })
        .collect();
    let lint_report =
        aevia_lint::Driver::run_program(&program, &store, &lint_config, Some(&root_ids));
    let mut failed = 0usize;
    let mut shown: std::collections::HashSet<aevia_syntax::FileId> =
        std::collections::HashSet::new();
    for path in &paths {
        let Some(&id) = ids.get(
            &aevia_core::source::canonical_path(path)
                .display()
                .to_string(),
        ) else {
            eprintln!("aevia check: `{}` could not be loaded", path.display());
            failed = failed.saturating_add(1);
            continue;
        };
        let mut own = Diagnostics::new();
        for item in &lint_report.diagnostics.items {
            if item.span.file == id {
                own.push(item.clone());
            }
        }
        shown.insert(id);
        if report(&own, &program.sources, style(options)) {
            failed = failed.saturating_add(1);
        } else {
            println!("{}: ok", path.display());
        }
    }
    // A diagnostic in a file that is not one of the package's own modules (a
    // dependency's source) is still part of the run; show it once.
    let mut imported = Diagnostics::new();
    for item in &lint_report.diagnostics.items {
        if !shown.contains(&item.span.file) {
            imported.push(item.clone());
        }
    }
    if report(&imported, &program.sources, style(options)) {
        failed = failed.saturating_add(1);
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

#[allow(
    clippy::single_call_fn,
    clippy::too_many_lines,
    reason = "keeps the test command's suite selection, execution and exit status together"
)]
fn test_command(
    options: Options,
    args: &[String],
) -> ExitCode {
    let mut filter: Option<String> = None;
    let mut exact: Option<&str> = None;
    let mut files: Vec<String> = Vec::new();
    let mut doc = false;
    let mut relax_result = false;
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        match argument.as_str() {
            | "--filter" => {
                index = index.saturating_add(1);
                let Some(value) = args.get(index) else {
                    eprintln!("aevia test: `--filter` expects a pattern");
                    return ExitCode::from(2);
                };
                filter = Some(value.clone());
            },
            // §7.5.5/§9.12: compile the ```aevia blocks of the `##` docs.
            | "--doc" => doc = true,
            | "--relax-result" => relax_result = true,
            | option if option.starts_with("--exact=") => exact = option.strip_prefix("--exact="),
            | other if other.starts_with('-') => {
                eprintln!(
                    "aevia test: unknown option `{other}` (the options are `--filter PAT`, \
                     `--exact=NAME`, `--doc`, and `--relax-result`)"
                );
                return ExitCode::from(2);
            },
            | other => files.push(other.to_string()),
        }
        index = index.saturating_add(1);
    }
    if exact.is_some() && (doc || filter.is_some()) {
        eprintln!("aevia test: use `--exact=NAME` on source tests without `--filter` or `--doc`");
        return ExitCode::from(2);
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
    if report_with_relax(
        &program.diagnostics,
        &program.sources,
        style(options),
        relax_result,
    ) {
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
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut matched = 0usize;
    for decl in &module.decls {
        if let DeclKind::Test { name, body } = &decl.kind {
            if exact.is_some_and(|expected| name != expected)
                || filter
                    .as_ref()
                    .is_some_and(|pattern| !name.contains(pattern.as_str()))
            {
                continue;
            }
            matched = matched.saturating_add(1);
            match interp.eval(body, &root) {
                | Ok(aevia_core::value::Value::Bool(true)) => {
                    println!("test {name} ... ok");
                    passed = passed.saturating_add(1);
                },
                | Ok(other) => {
                    println!(
                        "test {name} ... FAILED (returned {})",
                        aevia_core::value::show(&other)
                    );
                    failed = failed.saturating_add(1);
                },
                | Err(error) => {
                    println!("test {name} ... FAILED ({})", error.message);
                    failed = failed.saturating_add(1);
                },
            }
        }
    }
    if let Some(filter) = exact.or(filter.as_deref())
        && matched == 0
    {
        // A filter that matches nothing must not read as "all tests passed".
        eprintln!("aevia test: the filter `{}` matched no test", filter);
        return ExitCode::from(1);
    }
    println!("\n{passed} passed, {failed} failed");
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[allow(
    clippy::single_call_fn,
    clippy::too_many_lines,
    reason = "pairs RSSN's registry inspection and export options in one command"
)]
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
    while let Some(argument) = args.get(index) {
        match argument.as_str() {
            // `--emit` is a flag; §9.9 calls the destination `--out`. The old
            // `--emit DIR` spelling still works, because a directory after
            // `--emit` can only have been the destination.
            | "--emit" => {
                emit = true;
                if let Some(next) = args.get(index.saturating_add(1))
                    && !next.starts_with('-')
                {
                    out = Some(next.clone());
                    index = index.saturating_add(1);
                }
                index = index.saturating_add(1);
            },
            | "--out" => {
                let Some(path) = args.get(index.saturating_add(1)) else {
                    eprintln!("aevia rssn: `--out` needs a directory");
                    return ExitCode::from(2);
                };
                out = Some(path.clone());
                index = index.saturating_add(2);
            },
            | "--namespaces" => {
                namespaces = true;
                index = index.saturating_add(1);
            },
            | "--audit" => {
                audit = true;
                index = index.saturating_add(1);
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
    let mut declared = 0usize;
    let mut overlay = 0usize;
    let mut derived = 0usize;
    let mut none = 0usize;
    for op in &ops {
        match op.source {
            | aevia_rssn::Source::Declared => declared = declared.saturating_add(1),
            | aevia_rssn::Source::Overlay => overlay = overlay.saturating_add(1),
            | aevia_rssn::Source::Derived => derived = derived.saturating_add(1),
            | aevia_rssn::Source::None => none = none.saturating_add(1),
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
        let unavailable = catalog.len().saturating_sub(typed).saturating_sub(dynamic);
        let callback_aliases = catalog
            .iter()
            .filter(|row| row.typed_callback_name.is_some())
            .count();
        println!(
            "bindings: {typed} exact typed, {dynamic} dynamic/approximate, {unavailable} unavailable"
        );
        println!("typed callback aliases: {callback_aliases} (also counted in dynamic bindings)");
        for row in &catalog {
            let (status, reasons): (&str, &[String]) = match &row.coverage {
                | Coverage::Typed => ("typed", &[]),
                | Coverage::Dynamic(reasons) => ("dynamic", reasons),
                | Coverage::Unavailable(reasons) => ("unavailable", reasons),
            };
            println!(
                "{}:{}[{}] -> {}: {}{}{}",
                row.op.rule_set,
                row.op.name,
                row.overload.map_or_else(
                    || "unsigned".to_string(),
                    |i| i.saturating_add(1).to_string()
                ),
                row.name,
                status,
                if reasons.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", reasons.join("; "))
                },
                row.typed_callback_name
                    .as_ref()
                    .map_or_else(String::new, |name| {
                        format!("; typed callback alias: {name}")
                    })
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
    command_failed: bool,
    /// The options every interpreter in this session is built with.
    options: Options,
    /// Stable across source revisions; distinct for independent/reset sessions.
    identity: aevia_core::source::SourceIdentity,
    compiler: std::cell::RefCell<aevia_core::driver::ReplCompiler>,
    import_runtime: aevia_core::driver::ReplRuntime,
    source_origin: Option<PathBuf>,
    cancellation: aevia_core::CancellationToken,
    completion_cache: Vec<String>,
    documentation: repl_doc::Catalog,
    output_format: aevia_core::display::Format,
    source_history: repl_source::History,
    input_source: Option<repl_source::Input>,
}

fn repl_source_identity() -> aevia_core::source::SourceIdentity {
    static NEXT_SESSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let session = NEXT_SESSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    aevia_core::source::SourceIdentity::virtual_source(format!(
        "aevia-repl://{}/{session}/Repl",
        std::process::id()
    ))
}

impl Session {
    #[allow(
        clippy::single_call_fn,
        reason = "sets up the REPL's persistent interpreter state once per session"
    )]
    fn new(options: Options) -> Self {
        let identity = repl_source_identity();
        let directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            declarations: String::new(),
            interp: None,
            loaded: 0,
            command_failed: false,
            options,
            compiler: std::cell::RefCell::new(aevia_core::driver::ReplCompiler::new(
                &directory,
                identity.clone(),
            )),
            import_runtime: aevia_core::driver::ReplRuntime::default(),
            source_origin: None,
            cancellation: aevia_core::CancellationToken::new(),
            completion_cache: Vec::new(),
            documentation: repl_doc::Catalog::default(),
            output_format: aevia_core::display::Format::Show,
            source_history: repl_source::History::default(),
            input_source: None,
            identity,
        }
    }

    fn reset(&mut self) {
        self.declarations.clear();
        self.interp = None;
        self.loaded = 0;
        self.identity = repl_source_identity();
        let directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        self.compiler = std::cell::RefCell::new(aevia_core::driver::ReplCompiler::new(
            &directory,
            self.identity.clone(),
        ));
        self.import_runtime = aevia_core::driver::ReplRuntime::default();
        self.source_origin = None;
        self.completion_cache.clear();
        self.documentation = repl_doc::Catalog::default();
        self.source_history = repl_source::History::default();
    }

    fn compile(
        &self,
        source: &str,
    ) -> repl_source::Compilation {
        let current = source
            .strip_prefix(&self.declarations)
            .filter(|tail| !tail.is_empty())
            .map(|tail| {
                let entered = tail.strip_suffix('\n').unwrap_or(tail);
                let input = self.input_source.clone().unwrap_or_else(|| {
                    self.source_history
                        .input(entered, self.source_origin.as_deref())
                });
                repl_source::Region::declaration(
                    self.declarations.len(),
                    source.len(),
                    entered.len(),
                    input,
                )
            });
        self.compile_mapped(source, current)
    }

    fn compile_mapped(
        &self,
        source: &str,
        current: Option<repl_source::Region>,
    ) -> repl_source::Compilation {
        let raw = if self.cancellation.is_cancelled() {
            self.compiler.borrow().interrupted(source)
        } else {
            let compilation = self
                .compiler
                .borrow_mut()
                .compile(source, self.source_origin.as_deref());
            if self.cancellation.is_cancelled() {
                self.compiler.borrow().interrupted(source)
            } else {
                compilation
            }
        };
        let view = self
            .source_history
            .view(current, self.input_source.as_ref());
        repl_source::Compilation { raw, view }
    }

    fn expression_input(
        &self,
        expression: &str,
    ) -> repl_source::Input {
        self.input_source
            .as_ref()
            .and_then(|input| input.argument(expression))
            .unwrap_or_else(|| self.source_history.input(expression, None))
    }

    /// Set a temporary resolution origin for a :load fragment; restore the
    /// returned origin after submission, on both success and failure.
    const fn set_source_origin(
        &mut self,
        origin: Option<PathBuf>,
    ) -> Option<PathBuf> {
        std::mem::replace(&mut self.source_origin, origin)
    }

    fn imports(&self) -> Vec<String> {
        self.compiler.borrow().imports().to_vec()
    }

    fn completion_names(&self) -> Vec<String> {
        self.completion_cache.clone()
    }

    /// Brings the interpreter in line with `compilation`, evaluating only the
    /// declarations that have not run yet.
    ///
    /// `limit` is the number of declarations to load; an expression line
    /// compiles as `… let __repl = <expr>`, and that synthetic declaration is
    /// evaluated by [`Session::evaluate`] instead of being loaded.
    fn sync(
        &mut self,
        compilation: &repl_source::Compilation,
    ) -> Result<(), String> {
        self.sync_to(compilation, usize::MAX)
    }

    fn sync_to(
        &mut self,
        compilation: &repl_source::Compilation,
        limit: usize,
    ) -> Result<(), String> {
        let Some(module) = &compilation.module else {
            return Ok(());
        };
        let interp = self.interp.get_or_insert_with(|| {
            let mut interp = Interp::new(&aevia_types::CheckedModule::default());
            apply_options(&mut interp, self.options);
            interp.set_cancellation_token(self.cancellation.clone());
            interp.set_line_reader(crate::repl_program_line_reader(self.cancellation.clone()));
            interp
        });
        let limit = limit.min(module.decls.len());
        let diagnostics = self
            .import_runtime
            .synchronize(interp, compilation, self.loaded, limit);
        if diagnostics.has_errors() {
            return Err(compilation
                .render(&diagnostics, style(self.options))
                .trim_end()
                .to_string());
        }
        self.loaded = limit;
        if limit == module.decls.len() {
            self.source_history.accept(&compilation.view);
        }
        self.compiler.borrow_mut().commit(compilation);
        self.completion_cache = compilation
            .checked
            .exports
            .iter()
            .filter_map(|(name, export)| {
                let temporary =
                    matches!(export, aevia_types::Export::Value { decl, .. } if *decl >= limit);
                (!temporary || self.completion_cache.contains(name)).then(|| name.clone())
            })
            .collect();
        self.completion_cache
            .extend(self.compiler.borrow().imported_names().iter().cloned());
        self.completion_cache.sort();
        self.completion_cache.dedup();
        Ok(())
    }

    /// Evaluates an expression against the session's bindings.
    fn evaluate(
        &mut self,
        compilation: &repl_source::Compilation,
        expression: &aevia_syntax::Expr,
    ) -> Result<aevia_core::value::Value, String> {
        let declarations = compilation
            .module
            .as_ref()
            .map_or(0, |module| module.decls.len())
            .saturating_sub(1);
        self.sync_to(compilation, declarations)?;
        let interp = self.interp.as_mut().ok_or("no session")?;
        evaluate_repl_expression(interp, compilation, expression, self.options)
    }
}

fn evaluate_repl_expression(
    interp: &mut Interp,
    compilation: &repl_source::Compilation,
    expression: &aevia_syntax::Expr,
    options: Options,
) -> Result<aevia_core::value::Value, String> {
    let root = interp.root();
    if interp.optimize_ir {
        let decl = compilation
            .module
            .as_ref()
            .and_then(|m| m.decls.last())
            .ok_or("no expression")?;
        let mut lowered = aevia_ir::lower::lower_decl_with_runtime_evidence(
            decl,
            &compilation.checked.rssn_calls,
            &compilation.checked.dictionaries,
            &compilation.checked.literal_reps,
            &compilation.checked.method_refs,
            &compilation.checked.constructor_refs,
            &compilation.checked.expr_types,
            &compilation.checked.constant_regions,
        )
        .ok_or("no expression IR")?;
        aevia_ir::optimize::declaration(&mut lowered);
        if let aevia_ir::lower::LoweredDecl::Let(decl) = lowered {
            return aevia_core::eval_ir::eval_top(interp, &decl.block, decl.nslots, &root)
                .map_err(|error| repl_runtime_error(&error, compilation, options));
        }
    }
    interp
        .eval(expression, &root)
        .map_err(|error| repl_runtime_error(&error, compilation, options))
}

#[allow(
    clippy::single_call_fn,
    reason = "connects cancellable terminal reads to one REPL interpreter"
)]
fn repl_program_line_reader(
    cancellation: aevia_core::CancellationToken
) -> std::rc::Rc<dyn Fn() -> Result<String, aevia_core::EvalError>> {
    repl_interrupt::line_reader(cancellation)
}

fn repl(options: Options) -> ExitCode {
    let mut input = match repl_input::Input::new(terminal::enabled(options.color, false)) {
        | Ok(input) => input,
        | Err(error) => {
            eprintln!("aevia repl: {error}");
            return ExitCode::from(1);
        },
    };
    let interactive = input.interactive();
    if interactive {
        println!("Aevia {} — dimensions are types", env!("CARGO_PKG_VERSION"));
        println!("Enter an expression, or paste a program (including main). :help shows examples.");
        println!("Tab completes names; blank line ends a block; Ctrl-C cancels; Ctrl-D exits.");
    }
    let mut session = Session::new(options);
    let mut signals = repl_interrupt::Signals::new(session.cancellation.clone());
    let mut failed = false;
    loop {
        let source = match input.read() {
            | Ok(repl_input::Event::Input(source)) => source,
            | Ok(repl_input::Event::Interrupted) => continue,
            | Ok(repl_input::Event::Eof) => break,
            | Err(error) => {
                eprintln!("aevia repl: {error}");
                failed = true;
                break;
            },
        };
        let trimmed = source.trim();
        if trimmed.is_empty() {
            continue;
        }
        let _activity = match signals.begin() {
            | Ok(activity) => activity,
            | Err(message) => {
                eprintln!("aevia repl: {message}");
                return ExitCode::from(1);
            },
        };
        let original_input = session.source_history.input(&source, None);
        session.input_source = Some(original_input.clone());
        if let Some(command) = trimmed.strip_prefix(':') {
            if matches!(
                handle_repl_command(command, &mut session),
                ReplControl::Quit
            ) {
                break;
            }
            failed |= session.command_failed;
        } else {
            let mut offset = 0;
            for unit in repl_input::split_submissions(&source) {
                if let Some(start) = source
                    .get(offset..)
                    .and_then(|text| text.find(&unit))
                    .map(|start| offset.saturating_add(start))
                {
                    session.input_source =
                        Some(original_input.select(start..start.saturating_add(unit.len())));
                    offset = start.saturating_add(unit.len());
                } else {
                    session.input_source = None;
                }
                if !submit_repl(&mut session, &unit, interactive) {
                    failed = true;
                    break;
                }
            }
        }
        session.input_source = None;
        if !session.cancellation.is_cancelled() {
            let names = session.completion_names();
            if !session.cancellation.is_cancelled() {
                input.set_bindings(names);
            }
        }
    }
    if failed && !interactive {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// A submission is compiled in full before any new declaration is evaluated.
/// Only a newly supplied `main` runs automatically; later expressions never
/// replay it or the session's previous declarations.
fn submit_repl(
    session: &mut Session,
    input: &str,
    interactive: bool,
) -> bool {
    let previous = session.input_source.take();
    session.input_source = Some(
        previous
            .as_ref()
            .filter(|source| source.text() == input)
            .cloned()
            .unwrap_or_else(|| {
                session
                    .source_history
                    .input(input, session.source_origin.as_deref())
            }),
    );
    let result = submit_repl_input(session, input, interactive);
    session.input_source = previous;
    result
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps the REPL's per-input compilation and evaluation transaction"
)]
fn submit_repl_input(
    session: &mut Session,
    input: &str,
    interactive: bool,
) -> bool {
    if input.trim().is_empty() {
        return true;
    }
    let normalized = match repl_module_fragment(input, !session.declarations.trim().is_empty()) {
        | Ok(source) => source,
        | Err(message) => {
            eprintln!("{message}");
            return false;
        },
    };
    let input = normalized.as_deref().unwrap_or(input);
    if repl_input::declaration(input) {
        let candidate = format!("{}{input}\n", session.declarations);
        let compilation = session.compile(&candidate);
        if repl_report(&compilation, session.options) {
            return false;
        }
        let new_main =
            compilation.module.as_ref().is_some_and(|module| {
                module.decls.iter().skip(session.loaded).any(|decl| {
                match &decl.kind {
                    | DeclKind::Fn { name, .. } => name == "main",
                    | DeclKind::Let { pattern, .. } => matches!(
                        &pattern.kind, aevia_syntax::ast::PatKind::Var(name) if name == "main"
                    ),
                    | _ => false,
                }
            })
            });
        if let Err(message) = session.sync(&compilation) {
            // Batch loading restores its bindings on failure. Restore the
            // last accepted source evidence too, without replaying initializers.
            let accepted = session.compile(&session.declarations);
            if let (Some(interp), Some(module)) = (&mut session.interp, &accepted.module) {
                interp.prepare_incremental_module(&accepted.checked, &module.decls);
            }
            eprintln!("{message}");
            return false;
        }
        session.declarations = candidate;
        let origins = compilation.view.origins(&compilation.sources);
        session.documentation =
            repl_doc::Catalog::with_origins(&compilation, &|span| origins.location(span));
        if new_main {
            return run_repl_main(session);
        }
        if interactive {
            println!("ok");
        }
        true
    } else {
        let compilation = match compile_repl_expression(session, "value", input) {
            | Ok(compilation) => compilation,
            | Err(message) => {
                eprintln!("{message}");
                return false;
            },
        };
        repl_report(&compilation, session.options);
        let ty = compilation
            .checked
            .exports
            .get("__repl_value")
            .and_then(|export| {
                if let aevia_types::Export::Value { scheme, .. } = export {
                    Some(scheme.ty.to_string())
                } else {
                    None
                }
            })
            .unwrap_or_default();
        match repl_value(session, &compilation) {
            | Ok(aevia_core::value::Value::Unit) if ty.starts_with("IO ") => true,
            | Ok(value) => {
                match aevia_core::display::render(&value, session.output_format) {
                    | Ok(text) => {
                        if text.contains('\n') {
                            println!("{} =\n{text}", terminal::paint(&ty, "36", false));
                        } else {
                            println!("{ty} = {text}");
                        }
                        true
                    },
                    | Err(message) => {
                        eprintln!("{message}");
                        false
                    },
                }
            },
            | Err(message) => {
                eprintln!("{message}");
                false
            },
        }
    }
}

/// A file's ordinary module header names its original namespace. Appended
/// standalone declarations join the current REPL namespace; blank the header
/// in place so line numbers and all following source offsets stay intact.
#[allow(
    clippy::single_call_fn,
    reason = "validates a loaded module header before rewriting it"
)]
fn repl_module_fragment(
    input: &str,
    appending: bool,
) -> Result<Option<String>, String> {
    use aevia_syntax::Keyword;
    use aevia_syntax::TokenKind;
    if !appending {
        return Ok(None);
    }
    let Ok(lexed) = aevia_syntax::lex(0, input) else {
        return Ok(None);
    };
    let first = lexed.tokens.iter().find(|token| {
        !matches!(
            token.kind,
            TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent | TokenKind::Eof
        )
    });
    let Some(first) =
        first.filter(|token| matches!(token.kind, TokenKind::Keyword(Keyword::Module)))
    else {
        return Ok(None);
    };
    let start = usize::try_from(first.span.start).map_err(|_| "invalid module header location")?;
    let (before, header) = input
        .split_at_checked(start)
        .ok_or("invalid module header location")?;
    let length = header.find(['\r', '\n']).unwrap_or(header.len());
    let end = start.saturating_add(length);
    if lexed.tokens.iter().any(|token| {
        (token.span.start as usize) < end && matches!(token.kind, TokenKind::Keyword(Keyword::With))
    }) {
        return Err("a module with its own unit system needs a fresh session; use :reset before loading it, or aevia run FILE".to_string());
    }
    let suffix = header.get(length..).ok_or("invalid module header range")?;
    let mut source = String::with_capacity(input.len());
    source.push_str(before);
    source.extend(std::iter::repeat_n(' ', length));
    source.push_str(suffix);
    Ok(Some(source))
}

fn repl_runtime_error(
    error: &aevia_core::EvalError,
    compilation: &repl_source::Compilation,
    options: Options,
) -> String {
    if error.code == aevia_core::cancellation::INTERRUPTED {
        return terminal::paint("interrupted (E0812)", "33", true);
    }
    let mut diagnostics = Diagnostics::new();
    diagnostics.push(error.diagnostic());
    compilation
        .render(&diagnostics, style(options))
        .trim_end()
        .to_string()
}

fn repl_report(
    compilation: &repl_source::Compilation,
    options: Options,
) -> bool {
    let message = compilation.render(&compilation.diagnostics, style(options));
    if !message.is_empty() {
        eprint!("{message}");
    }
    compilation.diagnostics.has_errors()
}

fn run_repl_main(session: &mut Session) -> bool {
    match session.interp.as_mut().map(Interp::run_main) {
        | Some(Ok(Some(_))) => true,
        | Some(Err(error)) => {
            let compilation = session.compile(&session.declarations);
            eprintln!(
                "{}",
                repl_runtime_error(&error, &compilation, session.options)
            );
            false
        },
        | _ => {
            eprintln!("no main is loaded; paste a program or use :load FILE first");
            false
        },
    }
}

enum ReplControl {
    Continue,
    Quit,
}

#[allow(
    clippy::single_call_fn,
    clippy::too_many_lines,
    reason = "keeps the complete REPL command dispatch and output policy together"
)]
fn handle_repl_command(
    command: &str,
    session: &mut Session,
) -> ReplControl {
    session.command_failed = false;
    // All command error paths also determine the exit status of a piped session.
    macro_rules! eprintln {
        ($($arg:tt)*) => {{
            session.command_failed = true;
            terminal::write(format_args!($($arg)*), true, true);
        }};
    }
    let mut parts = command.trim().splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or("");
    let argument = parts.next().unwrap_or("").trim();
    match name {
        | "quit" | "q" => return ReplControl::Quit,
        | "reset" => {
            session.reset();
            println!("session cleared");
        },
        | "help" | "h" | "?" => {
            session.command_failed = !repl_item_help(session, argument);
        },
        | "doc" => {
            match session.documentation.render(argument) {
                | Ok(text) => println!("{text}"),
                | Err(message) => eprintln!("{message}"),
            }
        },
        | "apropos" => {
            match session.documentation.search(argument) {
                | Ok(text) => println!("{text}"),
                | Err(message) => eprintln!("{message}"),
            }
        },
        | "format" => {
            if argument.is_empty() {
                println!(
                    "output format: {}\nAvailable: show, repr, pretty, latex, typst\nUse :format NAME; direct commands such as :typst EXPR render just once.",
                    repl_format_name(session.output_format)
                );
            } else if let Some(format) = repl_parse_format(argument) {
                session.output_format = format;
                println!("output format: {}", repl_format_name(format));
            } else {
                eprintln!(
                    "unknown output format `{argument}`; choose show, repr, pretty, latex, or typst"
                );
            }
        },
        | "run" => {
            session.command_failed = !run_repl_main(session);
        },
        | "show" if argument.is_empty() => {
            print!("{}", session.declarations);
        },
        | "show" | "print" | "repr" | "pretty" | "pprint" | "typst" => {
            let format = match name {
                | "typst" => aevia_core::display::Format::Typst,
                | "repr" => aevia_core::display::Format::Repr,
                | "pretty" | "pprint" => aevia_core::display::Format::Pretty,
                | _ => aevia_core::display::Format::Show,
            };
            match repl_render(session, name, argument, format) {
                | Ok(text) => println!("{text}"),
                | Err(message) => eprintln!("{message}"),
            }
        },
        | "source" => {
            match session.source_history.source(argument) {
                | Ok(text) => println!("{text}"),
                | Err(message) => eprintln!("{message}"),
            }
        },
        | "imports" => {
            let imports = session.imports();
            if imports.is_empty() {
                println!("no modules imported; try import Math, or :load FILE");
            } else {
                for import in imports {
                    println!("{import}");
                }
            }
        },
        | "bindings" | "vars" => {
            let compilation = session.compile(&session.declarations);
            for (name, export) in &compilation.checked.exports {
                if let aevia_types::Export::Value { scheme, .. } = export {
                    println!("{name} : {}", scheme.ty);
                }
            }
        },
        | "save" => {
            if argument.is_empty() {
                eprintln!("usage: :save FILE — write accepted declarations to a new file");
            } else {
                use std::fs::OpenOptions;
                match OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(repl_file_argument(argument))
                {
                    | Ok(mut file) => {
                        match file.write_all(session.declarations.as_bytes()) {
                            | Ok(()) => println!("saved {argument}"),
                            | Err(error) => eprintln!("cannot save `{argument}`: {error}"),
                        }
                    },
                    | Err(error) => eprintln!("cannot save `{argument}`: {error}"),
                }
            }
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
                match aevia_dim::DimEnv::si().parse(argument) {
                    | Ok(dim) => Some(dim),
                    | Err(_) => {
                        eprintln!(
                            "unknown dimension `{argument}`; use :dims, for example :units L or :units L/T"
                        );
                        return ReplControl::Continue;
                    },
                }
            };
            for unit in table.list() {
                if filter.as_ref().is_none_or(|dim| unit.dim == *dim) {
                    println!("{} ({})", unit.display, unit.dim.to_plain_string());
                }
            }
        },
        | "type" | "t" => {
            // A builtin has a scheme, not just an instantiation; show it as
            // written in the standard library.
            let user_bound = session
                .interp
                .as_ref()
                .is_some_and(|interp| interp.globals.contains_key(argument))
                || session
                    .completion_names()
                    .iter()
                    .any(|name| name == argument);
            if !user_bound
                && let Some(first) = aevia_types::builtins::schemes(argument)
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
                println!(
                    "{}",
                    terminal::paint(
                        &format!("{argument} : {quantified}{}", first.ty),
                        "36",
                        false
                    )
                );
                return ReplControl::Continue;
            }
            if argument.is_empty() {
                eprintln!("usage: :type EXPRESSION — for example :type 3 m / 2 s");
                return ReplControl::Continue;
            }
            match compile_repl_expression(session, "type", argument) {
                | Err(message) => eprintln!("{message}"),
                | Ok(compilation) => {
                    if let Some(aevia_types::Export::Value { scheme, .. }) =
                        compilation.checked.exports.get("__repl_type")
                    {
                        println!("{}", terminal::paint(&scheme.ty.to_string(), "36", false));
                    }
                },
            }
        },
        // The three inspection commands parse or type-check the expression but
        // never evaluate it, so they leave the session exactly as it was.
        | "ast" => {
            match repl_ast_with_input(
                argument,
                &session.expression_input(argument),
                style(session.options),
            ) {
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
        | "paste-unfinished" => {
            eprintln!(
                "unfinished paste: expected :end on its own line; no pasted code was executed"
            );
        },
        | "load" | "l" => {
            if argument.is_empty() {
                eprintln!(
                    "usage: :load FILE — append a file's declarations and run its main if present"
                );
            } else {
                match std::fs::read_to_string(repl_file_argument(argument)) {
                    | Ok(text) => {
                        let previous = session
                            .set_source_origin(Some(PathBuf::from(repl_file_argument(argument))));
                        let accepted = submit_repl(session, &text, false);
                        session.set_source_origin(previous);
                        if accepted {
                            println!("loaded {argument}");
                        } else {
                            session.command_failed = true;
                        }
                    },
                    | Err(error) => eprintln!("cannot read `{argument}`: {error}"),
                }
            }
        },
        | other => eprintln!("unknown command `:{other}`; type :help for commands and examples"),
    }
    ReplControl::Continue
}

fn repl_file_argument(argument: &str) -> &str {
    argument
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| {
            argument
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
        })
        .unwrap_or(argument)
}

/// Prefer real item documentation; a leading colon explicitly requests a command.
#[allow(
    clippy::single_call_fn,
    reason = "applies interactive documentation lookup to the active REPL session"
)]
fn repl_item_help(
    session: &Session,
    topic: &str,
) -> bool {
    if topic.is_empty() || topic.starts_with(':') {
        return repl_help(topic);
    }
    match session.documentation.render(topic) {
        | Ok(text) => {
            println!("{text}");
            true
        },
        | Err(message) => {
            if matches!(
                topic,
                "paste"
                    | "main"
                    | "files"
                    | "source"
                    | "load"
                    | "save"
                    | "show"
                    | "units"
                    | "dims"
                    | "type"
                    | "t"
                    | "time"
                    | "latex"
                    | "typst"
                    | "pretty"
                    | "pprint"
                    | "repr"
                    | "print"
                    | "format"
                    | "doc"
                    | "apropos"
                    | "ast"
                    | "core"
                    | "imports"
                    | "import"
                    | "open"
                    | "interrupt"
                    | "cancel"
                    | "run"
                    | "bindings"
                    | "vars"
                    | "reset"
                    | "quit"
                    | "q"
                    | "help"
                    | "h"
                    | "?"
            ) {
                repl_help(topic)
            } else {
                eprintln!("{message}");
                false
            }
        },
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "keeps the REPL help topics and their text together"
)]
fn repl_help(topic: &str) -> bool {
    match topic.trim_start_matches(':') {
        | "" => {
            println!(
                "\
Aevia interactive guide

Try these expressions (exact arithmetic and units):
  1 + 2
  let distance = 20 m
  distance / 4 s
  :type distance
  println(\"hello\")

Expressions display their type and value. IO actions execute immediately.
Declarations persist; earlier declarations never run again for a new input.
Redefine a let or fn name to overwrite it, including a new type or dimension.
Paste a whole program, including fn main(): IO Unit = ...; a new main runs once.

Input and editing:
  Paste multiple lines, then Enter to submit. Bracketed paste stays together.
  Indented blocks end with a blank line; parentheses/brackets must be balanced.
  :paste followed by :end submits a block on terminals without bracketed paste.
  :cancel cancels explicit paste. Ctrl-C cancels input or evaluation; Ctrl-D exits.
  Up/Down browse session history; Left/Right, Home/End edit; Tab completes names.
  Piped input has no banner or prompts: aevia < program.aev
  import Math and import Dep.Module use the current package's dependency resolver.
  :imports lists loaded imports; :reset clears them and observes file changes.

Session commands:
  :type EXPR     inspect a type without running the expression (:t alias)
  :bindings      list user bindings and types (:vars alias)
  :show          show accepted declarations
  :source [N]    list original accepted inputs or inspect <repl #N> source
  :imports       list imported modules and aliases
  :load FILE     append a source file; resolve its package imports; run a new main (:l alias)
  :save FILE     save declarations to a new file (never overwrites)
  :run           run the loaded main again
  :reset         clear declarations and runtime state
  :dims          list base dimension names
  :units [DIM]   list units; e.g. :units L or :units L/T
  :ast EXPR      inspect parsing without evaluating
  :core EXPR     inspect typed Core IR without evaluating
  :time EXPR     run once and report elapsed time
  :latex EXPR / :typst EXPR   typeset one evaluated value
  :pretty EXPR   print two-dimensional mathematics (:pprint alias)
  :repr EXPR     render source representation; :print EXPR renders human text
  :format NAME   set normal result format: show, repr, pretty, latex, typst
  :help ITEM     signature, documentation, and examples for a function/type/unit
  :doc ITEM      item documentation; :apropos TEXT searches names and docs
  :help :CMD     help for a REPL command; :help alone shows this guide (:h, :?)
  :quit          leave the session (:q alias)

Use aevia help COMMAND in your shell for CLI help."
            );
        },
        | "paste" | "main" => {
            println!(
                "\
Paste this complete program, then Enter (or finish an indented block with a blank line):
  fn double(x: Int): Int = x * 2
  fn main(): IO Unit = do
      println(\"answer = {{double(21)}}\")

The entire declaration group type-checks before execution. Its main runs once.
Use :run to repeat main; expressions entered later do not repeat it.
For explicit paste, enter :paste, paste the code, and enter :end on its own line.
Use :cancel before :end to discard the block."
            );
        },
        | "source" => {
            println!(
                "usage: :source [N]\nList accepted input numbers, or show the exact input for <repl #N>.\nLoaded file snapshots keep their original names and text. :reset clears this source history."
            );
        },
        | "files" | "load" | "save" | "show" => {
            println!(
                "\
:load path/to/program.aev appends declarations and runs a newly loaded main.
Paths may contain spaces; surrounding quotes are optional.
:show prints accepted declarations. :save session.aev writes a new source file.
:reset starts over. :load supports the same standalone declarations as pasted code.
Imports use the loaded file's package and directory, then stay available at the prompt.
Dependencies initialize once per session. :reset clears bindings and refreshes imports."
            );
        },
        | "units" | "dims" => {
            println!(
                "\
Quantities carry dimensions: 3 m + 2 m is valid; 3 m + 2 s is a type error.
Try :type 3 m / 2 s, :dims, :units L, or :units L/T.
Numbers are exact by default. Use approx(EXPR) when you want a numerical result."
            );
        },
        | "type" | "t" => {
            println!(
                "usage: :type EXPRESSION\nExamples: :type 3 m / 2 s, :type map\nChecks the type without executing effects; session bindings are in scope."
            );
        },
        | "latex" | "typst" | "pretty" | "pprint" | "repr" | "print" | "format" => {
            println!(
                "Output utilities:\n  :latex 1/2       LaTeX math source\n  :typst 1/2       Typst math source\n  :pretty 1/2      two-dimensional terminal mathematics (:pprint alias)\n  :repr \"hello\"   source representation, including string quotes\n  :print 1/2       human-readable output without the type prefix\n  :show EXPR       human-readable output (:show alone lists declarations)\n  :format pretty  use pretty output for subsequent expressions and :time\n  :format show    restore the default human-readable format\n  :format         show the current format and choices\n\nEach expression evaluates once; IO executes once. Output mode changes do not rerun\ndeclarations and survive :reset. Program print/println output keeps its own format.\nLanguage functions: show, repr, to_pretty, to_latex, to_typst. Use :help to_typst.\nUnsupported mathematical values report an error; use show/repr for functions or tasks."
            );
        },
        | "help" | "h" | "?" | "doc" | "apropos" => {
            println!(
                "Item documentation:\n  :help sqrt       signature, description, and examples\n  :doc Math.sqrt   documentation for an imported qualified item\n  :? my_function   latest accepted definition and its ## documentation\n  :apropos matrix  search names and documentation\n  :help :show      help for the REPL command rather than the show function\n\nDocumentation lookup does not evaluate expressions. Imports use frozen source\ndocumentation; :reset reloads modules. Add ## comments above your own declarations."
            );
        },
        | "time" | "ast" | "core" => {
            println!(
                "usage: :{topic} EXPRESSION\nExample: :{topic} 1 + 2\n:ast parses; :core also type-checks; :time evaluates once using :format's output mode."
            );
        },
        | "imports" | "import" | "open" => {
            println!(
                "\
Import package or standard-library modules directly:
  import Math as M
  M.sqrt(4.0)
  open Math {{ sin, cos }}

Imports at the prompt use the package containing the current directory.
:load FILE resolves new imports from that file's own package and directory.
:imports lists active modules and aliases. Tab completes imported qualified names.
Loaded dependencies are frozen and initialize once; :reset starts a fresh session
and observes edited module files. A failed import leaves the prior session intact."
            );
        },
        | "interrupt" | "cancel" => {
            println!(
                "\
Ctrl-C cancels a pending draft or the current evaluation and returns to the prompt.
Accepted bindings remain available. Cancelled declaration/import batches roll back.
Effects completed before an interruption (for example a printed line) stay completed.
A native library call without cancellation checkpoints finishes its current call
before reporting interruption. Ctrl-D at the prompt exits; :reset clears the session."
            );
        },
        | "run" => {
            println!("usage: :run\nExecute the main already loaded by paste or :load again.");
        },
        | "bindings" | "vars" => {
            println!("usage: :bindings\nList names and types declared in this session.");
        },
        | "reset" => {
            println!(
                "usage: :reset\nClear all declarations, values, and runtime state. Editor history stays available."
            );
        },
        | "quit" | "q" => {
            println!("usage: :quit\nExit the session. Ctrl-D also exits at the prompt.");
        },
        | other => {
            eprintln!("unknown help topic `{other}`; use :help for the full guide");
            return false;
        },
    }
    true
}

/// `:ast e` — parse `e` and render the tree.
///
/// Only the parser runs here: `:ast` answers "how did the parser read this
/// text", which is true even when the expression would not type-check. The
/// session's declarations are deliberately not prepended — the question is
/// about the expression alone, and an unbound name is not a syntax error.
#[cfg(test)]
fn repl_ast(expression: &str) -> Result<String, String> {
    if expression.is_empty() {
        return Err("`:ast` expects an expression, as in `:ast f(1) + 2`".to_string());
    }
    let history = repl_source::History::default();
    repl_ast_with_input(expression, &history.input(expression, None), Style::Plain)
}

#[allow(
    clippy::single_call_fn,
    reason = "parses AST inspection input with its own source diagnostics"
)]
fn repl_ast_with_input(
    expression: &str,
    input: &repl_source::Input,
    output_style: Style,
) -> Result<String, String> {
    if expression.is_empty() {
        return Err("usage: :ast EXPRESSION".into());
    }
    let mut sources = aevia_syntax::SourceMap::new();
    sources.add("<repl>", expression);
    let history = repl_source::History::default();
    let region = repl_source::Region::expression(0, 0, expression.len(), input.clone());
    let view = history.view(Some(region), Some(input));
    let value = aevia_syntax::parse_expression(0, expression).map_err(|diagnostics| {
        view.render(&diagnostics, &sources, output_style)
            .trim_end()
            .to_string()
    })?;
    let mut out = String::new();
    print_ast(&ast_node(&value), "", true, &mut out);
    Ok(out.trim_end().to_string())
}

/// `:core e` — type-check `e` in the session and render its Core IR (§9.2.2).
///
/// The lowering is `aevia_ir`'s, with the checker's pinned `Rssn` overloads and
/// class instances — this is the same Core IR the interpreter would run, not a
/// second rendering of the AST.
#[allow(
    clippy::single_call_fn,
    reason = "builds Core IR for REPL inspection without executing the expression"
)]
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
    let mut lowered = aevia_ir::lower::lower_decl_with_runtime_evidence(
        decl,
        &compilation.checked.rssn_calls,
        &compilation.checked.dictionaries,
        &compilation.checked.literal_reps,
        &compilation.checked.method_refs,
        &compilation.checked.constructor_refs,
        &compilation.checked.expr_types,
        &compilation.checked.constant_regions,
    )
    .ok_or("this declaration has no Core IR")?;
    if session.options.release {
        aevia_ir::optimize::declaration(&mut lowered);
    }
    Ok(format!("{lowered:#?}"))
}

/// `:time e` — evaluate `e` once and report the wall-clock time.
///
/// One evaluation, so the number is what the user's expression costs; the
/// value is printed first because a timing without the result it timed is not
/// reproducible.
#[allow(
    clippy::single_call_fn,
    reason = "evaluates one REPL expression and reports its elapsed time"
)]
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
    let start = std::time::Instant::now();
    let value = evaluate_repl_expression(interp, &compilation, &expression, session.options)?;
    let value = if let aevia_core::value::Value::Io(action) = value {
        interp
            .run_io(&action)
            .map_err(|error| repl_runtime_error(&error, &compilation, session.options))?
    } else {
        value
    };
    let elapsed = start.elapsed();
    let shown = aevia_core::display::render(&value, session.output_format)?;
    Ok(format!("{shown}\ntime: {}", repl_duration(elapsed)))
}

/// `:latex e` — evaluate `e` and render the value with `to_latex`'s engine.
#[allow(
    clippy::single_call_fn,
    reason = "renders the LaTeX inspection command using the session's definitions"
)]
fn repl_latex(
    session: &mut Session,
    expression: &str,
) -> Result<String, String> {
    repl_render(
        session,
        "latex",
        expression,
        aevia_core::display::Format::Latex,
    )
}

fn repl_render(
    session: &mut Session,
    command: &str,
    expression: &str,
    format: aevia_core::display::Format,
) -> Result<String, String> {
    if expression.is_empty() {
        return Err(format!(
            "usage: :{command} EXPRESSION — for example :{command} 1/2"
        ));
    }
    let compilation = compile_repl_expression(session, "render", expression)?;
    let value = repl_value(session, &compilation)?;
    aevia_core::display::render(&value, format)
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps all supported REPL display formats in one place"
)]
fn repl_parse_format(name: &str) -> Option<aevia_core::display::Format> {
    use aevia_core::display::Format;
    Some(match name {
        | "show" | "plain" | "default" => Format::Show,
        | "repr" => Format::Repr,
        | "pretty" => Format::Pretty,
        | "latex" => Format::Latex,
        | "typst" => Format::Typst,
        | _ => return None,
    })
}

const fn repl_format_name(format: aevia_core::display::Format) -> &'static str {
    use aevia_core::display::Format;
    match format {
        | Format::Show => "show",
        | Format::Repr => "repr",
        | Format::Pretty => "pretty",
        | Format::Latex => "latex",
        | Format::Typst => "typst",
    }
}

/// Compiles `let __repl_<label> = <expression>` against the session's
/// declarations, failing with the rendered diagnostics.
fn compile_repl_expression(
    session: &Session,
    label: &str,
    expression: &str,
) -> Result<repl_source::Compilation, String> {
    let prefix = format!("let __repl_{label} = ");
    let candidate = format!("{}{prefix}{expression}\n", session.declarations);
    let input = session.expression_input(expression);
    let region = repl_source::Region::expression(
        session.declarations.len(),
        prefix.len(),
        candidate.len(),
        input,
    );
    let compilation = session.compile_mapped(&candidate, Some(region));
    if compilation.diagnostics.has_errors() {
        return Err(compilation
            .render(&compilation.diagnostics, style(session.options))
            .trim_end()
            .to_string());
    }
    Ok(compilation)
}

/// The synthetic expression declaration of a REPL compilation.
fn repl_expression(compilation: &aevia_core::Compilation) -> Result<&aevia_syntax::Expr, String> {
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
    compilation: &repl_source::Compilation,
) -> Result<aevia_core::value::Value, String> {
    let options = session.options;
    let expression = repl_expression(compilation)?.clone();
    let value = session.evaluate(compilation, &expression)?;
    if let aevia_core::value::Value::Io(action) = value {
        session
            .interp
            .as_mut()
            .ok_or("no session")?
            .run_io(&action)
            .map_err(|error| repl_runtime_error(&error, compilation, options))
    } else {
        Ok(value)
    }
}

/// A duration in the largest unit that keeps three significant decimals.
#[allow(
    clippy::single_call_fn,
    reason = "formats elapsed REPL time using the appropriate unit"
)]
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
    children: Vec<Self>,
}

impl AstNode {
    const fn leaf(label: String) -> Self {
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
#[allow(
    clippy::too_many_lines,
    reason = "keeps exhaustive syntax-to-tree rendering in a single match"
)]
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

#[allow(
    clippy::single_call_fn,
    reason = "renders type annotations for the AST display"
)]
fn type_source(ty: &aevia_syntax::ast::Ty) -> String {
    aevia_fmt::print::Printer::new(Vec::new())
        .type_(ty)
        .unwrap_or_else(|_| "<unprintable>".to_string())
}

#[allow(
    clippy::single_call_fn,
    reason = "labels each definition kind for the AST display"
)]
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
        print_ast(child, &child_prefix, index.saturating_add(1) == count, out);
    }
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
        let mut options = Options {
            color: ColorChoice::Always,
            ..Options::default()
        };
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
    fn repl_overwrites_a_binding_with_a_new_dimension_after_a_rejected_input() {
        for options in [
            Options::default(),
            Options {
                release: true,
                ..Options::default()
            },
        ] {
            let mut session = Session::new(options);
            assert!(submit_repl(&mut session, "let a = 1 km", false));
            assert!(submit_repl(&mut session, "let b = 1 s", false));
            assert!(!submit_repl(&mut session, "let c = a + b", false));
            assert!(submit_repl(&mut session, "let b = 1 km", false));
            assert!(submit_repl(&mut session, "let c = a + b", false));
            assert_eq!(evaluate(&mut session, "b / 1 km"), "1");
            assert_eq!(evaluate(&mut session, "c / 1 km"), "2");
        }
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
    fn release_and_worker_flags_are_parsed() {
        let (options, rest) =
            Options::parse(&args(&["--release", "--jobs", "4", "--locked", "check"]))
                .expect("parses");
        assert!(options.release);
        assert_eq!(options.jobs, Some(4));
        assert!(options.locked);
        assert_eq!(rest, args(&["check"]));
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
        // A symbolic value goes through the same `to_latex` engine; a
        // single-letter symbol is an (italic) math variable.
        declare(&mut session, "let s = symbol(\"x\")");
        assert_eq!(repl_latex(&mut session, "s").expect("renders"), "x");
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
    fn indented_and_match_blocks_are_whole_submissions() {
        let input = "test \"a\" =\n    let a = 1\n    a == 1\n:quit\n";
        let units = repl_input::split_submissions(input);
        assert_eq!(units.len(), 2, "{units:?}");
        assert!(units[0].contains("a == 1"));
        assert_eq!(units[1], ":quit");
        let units = repl_input::split_submissions("match x with\n| A -> 1\n| _ -> 2\nlet y = 2\n");
        assert_eq!(units.len(), 2, "{units:?}");
        assert!(units[0].contains("| _ -> 2"));
        let input = "class Show a where\n    fn show(x: a): String\ninstance Show for Float where\n    fn show(x: Float): String = \"number\"\nlet r = do\n    println(\"hello\")\n";
        let units = repl_input::split_submissions(input);
        assert_eq!(
            units.len(),
            1,
            "consecutive declarations batch together: {units:?}"
        );
    }

    #[test]
    fn repl_sessions_and_reset_have_distinct_stable_source_origins() {
        let mut first = Session::new(Options::default());
        let second = Session::new(Options::default());
        let source = "type Token = | Token(Int)\n";
        let a = first.compile(source);
        let again = first.compile(&format!("{source}let other = 1\n"));
        let b = second.compile(source);
        let nominal = |checked: &aevia_types::CheckedModule| {
            match checked.exports.get("Token").unwrap() {
                | aevia_types::Export::Type { adt } | aevia_types::Export::TypeCtor { adt, .. } => {
                    aevia_types::adt::key_of(*adt)
                },
                | _ => panic!("type"),
            }
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
        assert!(matches!(
            interp.globals.get("floating"),
            Some(aevia_core::value::Value::Num(aevia_core::value::NumVal {
                kind: aevia_core::value::NumKind::Float(_),
                ..
            }))
        ));
        assert!(matches!(
            interp.globals.get("exact"),
            Some(aevia_core::value::Value::Num(aevia_core::value::NumVal {
                kind: aevia_core::value::NumKind::Exact(_),
                ..
            }))
        ));
        assert_eq!(evaluate(&mut session, "get()"), "Wrap(7)");
        declare(
            &mut session,
            "fn reject_time(): Bool = match check<L>(dyn(2.0 s)) with\n    | Ok(_) -> false\n    | Err(_) -> true",
        );
        assert_eq!(evaluate(&mut session, "reject_time()"), "true");
        assert_eq!(evaluate(&mut session, "repr(original)"), "\"session\"");
    }

    #[test]
    fn repl_appended_instances_keep_their_own_implementation_cursor() {
        let mut session = Session::new(Options::default());
        declare(
            &mut session,
            "class Pretty a where\n    pretty(x: a): String",
        );
        declare(
            &mut session,
            "instance Pretty for Int where\n    pretty(x) = \"integer\"",
        );
        declare(
            &mut session,
            "instance Pretty for Bool where\n    pretty(x) = \"boolean\"",
        );
        assert_eq!(evaluate(&mut session, "pretty(1)"), "\"integer\"");
        assert_eq!(evaluate(&mut session, "pretty(true)"), "\"boolean\"");
    }

    #[test]
    fn repl_private_and_public_instances_are_bound_by_source_site_after_append() {
        let mut session = Session::new(Options::default());
        declare(
            &mut session,
            "private class Hidden a where\n    hidden(x: a): String",
        );
        declare(
            &mut session,
            "class Visible a where\n    visible(x: a): String",
        );
        declare(
            &mut session,
            "instance Hidden for Int where\n    hidden(x) = \"private\"",
        );
        declare(
            &mut session,
            "instance Visible for Bool where\n    visible(x) = \"public\"",
        );
        declare(&mut session, "let first_class = hidden");
        assert_eq!(evaluate(&mut session, "hidden(1)"), "\"private\"");
        assert_eq!(evaluate(&mut session, "visible(true)"), "\"public\"");
        assert_eq!(evaluate(&mut session, "first_class(1)"), "\"private\"");
    }

    #[test]
    fn repl_core_inspection_uses_the_same_literal_and_method_identity_evidence_as_execution() {
        let mut session = Session::new(Options::default());
        declare(
            &mut session,
            "private class Hidden a where\n    hidden(x: a): String",
        );
        declare(
            &mut session,
            "instance Hidden for Int where\n    hidden(x) = \"private\"",
        );
        let method = repl_core(&session, "hidden").expect("method IR");
        assert!(method.contains("ClassMethodRef"), "{method}");
        assert!(method.contains("Hidden"), "{method}");
        let scalar = repl_core(&session, "(1 : Float)").expect("typed literal IR");
        assert!(scalar.contains("Float("), "{scalar}");
    }
}
