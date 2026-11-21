//! `aevia lint` command implementation and shared lint-flag/config helpers for `aevia check`.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::ExitCode;

use aevia_core::driver;
use aevia_core::manifest::LintSetting;
use aevia_core::manifest::Package;
use aevia_lint::CliOverride;
use aevia_lint::Driver;
use aevia_lint::FixEngine;
use aevia_lint::Level;
use aevia_lint::LintConfig;
use aevia_lint::LintReport;
use aevia_lint::LintStore;
use aevia_lint::OutputFormat;
use aevia_syntax::FileId;
use aevia_syntax::SourceMap;

use crate::Options;
use crate::entry_path;
use crate::load_at_with;
use crate::package;
use crate::style;

/// Parses `-A/-W/-D/-F` (`--allow/--warn/--deny/--forbid`) flags, returning remaining args.
pub fn parse_lint_flags(
    args: &[String],
    command: &str,
) -> Result<(Vec<CliOverride>, Vec<String>), ExitCode> {
    let mut overrides = Vec::new();
    let mut rest = Vec::new();
    let mut i = 0;
    while let Some(argument) = args.get(i) {
        let arg = argument;
        let flag = match arg.as_str() {
            | "-A" | "--allow" => Some(Level::Allow),
            | "-W" | "--warn" => Some(Level::Warn),
            | "-D" | "--deny" => Some(Level::Deny),
            | "-F" | "--forbid" => Some(Level::Forbid),
            | _ => None,
        };
        if let Some(level) = flag {
            let Some(target) = args.get(i.saturating_add(1)) else {
                eprintln!("aevia {command}: `{arg}` expects a lint or group name");
                return Err(ExitCode::from(2));
            };
            overrides.push(CliOverride {
                level,
                target: target.clone(),
            });
            i = i.saturating_add(2);
            continue;
        }
        // Also support `--deny=warnings`, `-Dwarnings`, etc.
        if let Some((prefix, target)) = split_inline_flag(arg) {
            overrides.push(CliOverride {
                level: prefix,
                target: target.to_string(),
            });
            i = i.saturating_add(1);
            continue;
        }
        rest.push(arg.clone());
        i = i.saturating_add(1);
    }
    Ok((overrides, rest))
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps accepted lint flag spellings together"
)]
fn split_inline_flag(arg: &str) -> Option<(Level, &str)> {
    for (p, lvl) in [
        ("--allow=", Level::Allow),
        ("--warn=", Level::Warn),
        ("--deny=", Level::Deny),
        ("--forbid=", Level::Forbid),
        ("-A", Level::Allow),
        ("-W", Level::Warn),
        ("-D", Level::Deny),
        ("-F", Level::Forbid),
    ] {
        if let Some(rest) = arg.strip_prefix(p)
            && !rest.is_empty()
        {
            return Some((lvl, rest));
        }
    }
    None
}

/// Loads `[lints]` from the nearest `aevia.toml` (if any) starting from `start_dir`.
pub fn load_manifest_lints(start_dir: &Path) -> BTreeMap<String, LintSetting> {
    Package::find(start_dir)
        .ok()
        .flatten()
        .map_or_default(|pkg| pkg.manifest.lints)
}

/// Entry point for `aevia lint`.
#[allow(
    clippy::single_call_fn,
    clippy::too_many_lines,
    reason = "the lint command routes inspection, package checks, and file checks"
)]
pub fn run(
    options: Options,
    args: &[String],
) -> ExitCode {
    let store = LintStore::with_builtins();

    let (cli_overrides, rest) = match parse_lint_flags(args, "lint") {
        | Ok(pair) => pair,
        | Err(code) => return code,
    };

    let mut all = false;
    let mut no_cache = false;
    let mut fix = false;
    let mut allow_dirty = false;
    let mut list = false;
    let mut explain: Option<String> = None;
    let mut format = OutputFormat::Human;
    let mut files = Vec::new();

    let mut args = rest.into_iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            | "--all" => all = true,
            | "--no-cache" => no_cache = true,
            | "--fix" => fix = true,
            | "--allow-dirty" => allow_dirty = true,
            | "--list" => list = true,
            | "--explain" => {
                let Some(target) = args.next() else {
                    eprintln!("aevia lint: `--explain` expects a lint code or name");
                    return ExitCode::from(2);
                };
                explain = Some(target);
            },
            | "--format" => {
                let Some(fmt) = args.next() else {
                    eprintln!("aevia lint: `--format` expects `human`, `json`, or `sarif`");
                    return ExitCode::from(2);
                };
                let Some(parsed) = OutputFormat::parse(&fmt) else {
                    eprintln!(
                        "aevia lint: unknown format `{fmt}` (expected `human`, `json`, or `sarif`)"
                    );
                    return ExitCode::from(2);
                };
                format = parsed;
            },
            | other if other.starts_with('-') => {
                eprintln!(
                    "aevia lint: unknown option `{other}` (use `aevia lint --help` for options)"
                );
                return ExitCode::from(2);
            },
            | other => files.push(other.to_string()),
        }
    }

    if list {
        print!("{}", store.format_list());
        return ExitCode::SUCCESS;
    }
    if let Some(target) = explain {
        let Some(text) = store.format_explain(&target) else {
            eprintln!("aevia lint: unknown lint `{target}` (use `--list` to see all lints)");
            return ExitCode::from(2);
        };
        print!("{text}");
        return ExitCode::SUCCESS;
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if fix && !allow_dirty && is_git_dirty(&cwd) {
        eprintln!(
            "aevia lint: working tree has uncommitted changes; commit them or pass `--allow-dirty`"
        );
        return ExitCode::from(2);
    }

    if all {
        return lint_all(
            options,
            &store,
            cli_overrides,
            &files,
            fix,
            format,
            no_cache,
        );
    }

    let mut paths = package::expand_globs(&files);
    if paths.is_empty() {
        match entry_path(&[], "lint") {
            | Ok(path) => paths.push(path),
            | Err(code) => return code,
        }
    }
    let batch = paths.len() > 1 && format != OutputFormat::Human;
    let mut reports = Vec::new();
    let mut failed = false;
    for path in paths {
        let start_dir = path.parent().unwrap_or(&cwd);
        let config = LintConfig {
            manifest_lints: load_manifest_lints(start_dir),
            cli_overrides: cli_overrides.clone(),
            run_extra_lints: true,
        };
        let Some((sources, report)) = lint_path(&path, &store, &config, fix, no_cache) else {
            failed = true;
            continue;
        };
        failed |= report.has_errors();
        if batch {
            reports.push((report, sources));
        } else {
            print_report(&sources, &report, &store, format, options);
        }
    }
    if batch && !reports.is_empty() {
        let reports = reports.iter().map(|(report, sources)| (report, sources));
        let rendered = aevia_lint::format::render_many(reports, &store, format, style(options));
        print_structured(&rendered);
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "one report and fix cycle belongs to one source root"
)]
fn lint_path(
    path: &Path,
    store: &LintStore,
    config: &LintConfig,
    fix: bool,
    no_cache: bool,
) -> Option<(SourceMap, LintReport)> {
    let mut program = load_at_with(path, !no_cache);
    let root_files = program.entry().map(|module| [module.file]);
    let mut report = Driver::run_program(
        &program,
        store,
        config,
        root_files.as_ref().map(<[FileId; 1]>::as_slice),
    );
    if fix {
        match FixEngine::apply(&report.lint_diagnostics, &program.sources) {
            | Ok(summary) if summary.applied > 0 => {
                eprintln!(
                    "aevia lint: applied {} fix(es) in {} file(s)",
                    summary.applied,
                    summary.files_modified.len()
                );
                program = load_at_with(path, false);
                let root_files = program.entry().map(|module| [module.file]);
                report = Driver::run_program(
                    &program,
                    store,
                    config,
                    root_files.as_ref().map(<[FileId; 1]>::as_slice),
                );
            },
            | Ok(_) => {},
            | Err(error) => {
                eprintln!("aevia lint: {}: {error}", path.display());
                return None;
            },
        }
    }
    Some((program.sources, report))
}

#[allow(
    clippy::single_call_fn,
    reason = "linting a package resolves the workspace and its lint settings once"
)]
fn lint_all(
    options: Options,
    store: &LintStore,
    cli_overrides: Vec<CliOverride>,
    files: &[String],
    fix: bool,
    format: OutputFormat,
    _no_cache: bool,
) -> ExitCode {
    if let Some(extra) = files.first() {
        eprintln!(
            "aevia lint: `--all` lints every module of the package, so it takes no file (found `{extra}`)"
        );
        return ExitCode::from(2);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let package = match Package::find(&cwd) {
        | Ok(Some(pkg)) => pkg,
        | Ok(None) => {
            eprintln!(
                "aevia lint: `--all` needs an `aevia.toml` in `{}` or any parent",
                cwd.display()
            );
            return ExitCode::from(2);
        },
        | Err(err) => {
            eprintln!("aevia lint: {}", err.render());
            return ExitCode::from(1);
        },
    };
    let paths = package::sources_under(&package.source_dir());
    if paths.is_empty() {
        eprintln!(
            "aevia lint: `{}` holds no `.aev` files",
            package.source_dir().display()
        );
        return ExitCode::from(1);
    }
    let roots = match package.roots() {
        | Ok(r) => r,
        | Err(err) => {
            eprintln!("aevia lint: {}", err.render());
            return ExitCode::from(1);
        },
    };
    let config = LintConfig {
        manifest_lints: package.manifest.lints,
        cli_overrides,
        run_extra_lints: true,
    };

    let (program, _) = driver::load_all_with_roots(
        &paths,
        roots.clone(),
        &aevia_types::check::CheckOptions::default(),
    );
    let root_ids = collect_root_ids(&program, &paths);
    let mut report = Driver::run_program(&program, store, &config, Some(&root_ids));

    if fix {
        match FixEngine::apply(&report.lint_diagnostics, &program.sources) {
            | Ok(summary) if summary.applied > 0 => {
                eprintln!(
                    "aevia lint: applied {} fix(es) in {} file(s)",
                    summary.applied,
                    summary.files_modified.len()
                );
                let (reloaded, _) = driver::load_all_with_roots(
                    &paths,
                    roots,
                    &aevia_types::check::CheckOptions::default(),
                );
                let root_ids = collect_root_ids(&reloaded, &paths);
                report = Driver::run_program(&reloaded, store, &config, Some(&root_ids));
                print_report(&reloaded.sources, &report, store, format, options);
                return if report.has_errors() {
                    ExitCode::from(1)
                } else {
                    ExitCode::SUCCESS
                };
            },
            | Ok(_) => {},
            | Err(err) => {
                eprintln!("aevia lint: {err}");
                return ExitCode::from(1);
            },
        }
    }

    print_report(&program.sources, &report, store, format, options);
    if report.has_errors() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn collect_root_ids(
    program: &driver::Program,
    paths: &[PathBuf],
) -> Vec<FileId> {
    let canonical_roots: std::collections::HashSet<String> = paths
        .iter()
        .map(|p| aevia_core::source::canonical_path(p).display().to_string())
        .collect();
    let mut ids = Vec::new();
    let mut file: FileId = 0;
    while let Some(source) = program.sources.file(file) {
        if canonical_roots.contains(&source.name) {
            ids.push(file);
        }
        file = file.saturating_add(1);
    }
    ids
}

fn print_report(
    sources: &aevia_syntax::SourceMap,
    report: &aevia_lint::LintReport,
    store: &LintStore,
    format: OutputFormat,
    options: Options,
) {
    let rendered = aevia_lint::render(report, sources, store, format, style(options));
    match format {
        | OutputFormat::Human => eprint!("{rendered}"),
        | OutputFormat::Json | OutputFormat::Sarif => print_structured(&rendered),
    }
}

fn print_structured(rendered: &str) {
    let _ = std::io::stdout().lock().write_all(rendered.as_bytes());
}

#[allow(
    clippy::single_call_fn,
    reason = "checks for the user's uncommitted changes before applying edits"
)]
fn is_git_dirty(dir: &Path) -> bool {
    Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output()
        .is_ok_and(|out| out.status.success() && !out.stdout.is_empty())
}
