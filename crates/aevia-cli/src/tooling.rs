//! `aevia fmt`, `aevia doc`, `aevia bench` — the source-level tools.

use crate::{apply_options, load_program, report, style, Options};
use aevia_core::driver;
use aevia_core::Interp;
use aevia_syntax::{DeclKind, Expr, ExprKind, Span};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

/// How many untimed iterations a benchmark runs first.
const WARMUP: usize = 3;
/// How many iterations a benchmark is timed over.
const ITERATIONS: usize = 100;

/// `aevia fmt [--check] [files…]`
pub fn fmt(_options: Options, args: &[String]) -> ExitCode {
    let mut check = false;
    let mut files: Vec<String> = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--check" => check = true,
            other if other.starts_with('-') => {
                eprintln!("aevia fmt: unknown option `{other}` (the options are `--check`)");
                return ExitCode::from(2);
            }
            other => files.push(other.to_string()),
        }
    }
    let paths = match inputs(&files) {
        Ok(paths) => paths,
        Err(code) => return code,
    };
    let mut changed = 0usize;
    let mut failed = 0usize;
    for path in &paths {
        let source = match std::fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("aevia fmt: cannot read `{}`: {error}", path.display());
                failed += 1;
                continue;
            }
        };
        match aevia_fmt::format_source(&source) {
            Ok(formatted) => {
                if formatted == source {
                    continue;
                }
                changed += 1;
                if check {
                    println!("would reformat {}", path.display());
                } else {
                    match std::fs::write(path, &formatted) {
                        Ok(()) => println!("formatted {}", path.display()),
                        Err(error) => {
                            eprintln!("aevia fmt: cannot write `{}`: {error}", path.display());
                            failed += 1;
                        }
                    }
                }
            }
            Err(error) => {
                // The file is left exactly as it was; the message names the
                // construct that could not be printed.
                eprintln!("aevia fmt: {}: {error}", path.display());
                failed += 1;
            }
        }
    }
    if failed > 0 {
        return ExitCode::from(1);
    }
    if check && changed > 0 {
        return ExitCode::from(1);
    }
    if changed == 0 {
        println!("{} file(s) already formatted", paths.len());
    }
    ExitCode::SUCCESS
}

/// `aevia doc [--out DIR] [files…]`
pub fn doc(_options: Options, args: &[String]) -> ExitCode {
    let mut out: Option<PathBuf> = None;
    let mut files: Vec<String> = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--out" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    eprintln!("aevia doc: `--out` expects a directory");
                    return ExitCode::from(2);
                };
                out = Some(PathBuf::from(value));
            }
            other if other.starts_with('-') => {
                eprintln!("aevia doc: unknown option `{other}` (the option is `--out DIR`)");
                return ExitCode::from(2);
            }
            other => files.push(other.to_string()),
        }
        index += 1;
    }
    let paths = match inputs(&files) {
        Ok(paths) => paths,
        Err(code) => return code,
    };
    let package = aevia_core::manifest::Package::find(
        &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    )
    .ok()
    .flatten();
    let directory = out.unwrap_or_else(|| match &package {
        Some(package) => package.dir.join("docs"),
        None => PathBuf::from("docs"),
    });
    let name = package
        .as_ref()
        .map_or_else(|| "Aevia".to_string(), |package| package.manifest.name.clone());
    let mut docs = Vec::new();
    let mut failed = 0usize;
    for path in &paths {
        let source = match std::fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("aevia doc: cannot read `{}`: {error}", path.display());
                failed += 1;
                continue;
            }
        };
        let fallback = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| "Module".to_string());
        // The page quotes the file relative to the package, so the generated
        // documentation is the same on every checkout.
        let shown = package
            .as_ref()
            .and_then(|package| path.strip_prefix(&package.dir).ok())
            .unwrap_or(path)
            .display()
            .to_string();
        match aevia_doc::module_doc(&source, &shown, &fallback) {
            Ok(doc) => docs.push(doc),
            Err(message) => {
                eprintln!("aevia doc: {}: {message}", path.display());
                failed += 1;
            }
        }
    }
    if failed > 0 {
        return ExitCode::from(1);
    }
    docs.sort_by(|left, right| left.path.cmp(&right.path));
    if let Err(error) = std::fs::create_dir_all(&directory) {
        eprintln!("aevia doc: cannot create `{}`: {error}", directory.display());
        return ExitCode::from(1);
    }
    for doc in &docs {
        let path = directory.join(doc.page());
        if let Err(error) = std::fs::write(&path, aevia_doc::render_module(doc)) {
            eprintln!("aevia doc: cannot write `{}`: {error}", path.display());
            return ExitCode::from(1);
        }
        println!("wrote {}", path.display());
    }
    let index_path = directory.join("index.md");
    if let Err(error) = std::fs::write(&index_path, aevia_doc::render_index(&name, &docs)) {
        eprintln!("aevia doc: cannot write `{}`: {error}", index_path.display());
        return ExitCode::from(1);
    }
    println!("wrote {}", index_path.display());
    ExitCode::SUCCESS
}

/// `aevia bench [--filter PAT] [files…]`
pub fn bench(options: Options, args: &[String]) -> ExitCode {
    let mut filter: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--filter" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    eprintln!("aevia bench: `--filter` expects a pattern");
                    return ExitCode::from(2);
                };
                filter = Some(value.clone());
            }
            other if other.starts_with('-') => {
                eprintln!("aevia bench: unknown option `{other}` (the options are `--filter PAT`)");
                return ExitCode::from(2);
            }
            other => files.push(other.to_string()),
        }
        index += 1;
    }
    let paths = match inputs(&files) {
        Ok(paths) => paths,
        Err(code) => return code,
    };
    let mut rows: Vec<Row> = Vec::new();
    let mut failed = 0usize;
    for path in &paths {
        let program = match load_program(path, style(options)) {
            Ok(program) => program,
            Err(code) => return code,
        };
        if report(&program.diagnostics, &program.sources, style(options)) {
            return ExitCode::from(1);
        }
        let Some(entry) = program.entry() else {
            eprintln!("aevia bench: no module produced");
            return ExitCode::from(1);
        };
        let mut interp = Interp::new(&entry.checked);
        apply_options(&mut interp, options);
        let load_diagnostics = driver::run_modules(&mut interp, &program);
        if report(&load_diagnostics, &program.sources, style(options)) {
            return ExitCode::from(1);
        }
        let benches: Vec<(String, &Expr)> = entry
            .module
            .decls
            .iter()
            .filter(|decl| decl.has_attribute("bench"))
            .filter_map(|decl| match &decl.kind {
                DeclKind::Fn { name, body, .. } => Some((name.clone(), body)),
                _ => None,
            })
            .collect();
        if benches.is_empty() {
            eprintln!(
                "aevia bench: `{}` has no `@bench` functions, so nothing was measured",
                path.display()
            );
            failed += 1;
            continue;
        }
        for (name, _) in benches {
            if let Some(pattern) = &filter {
                if !name.contains(pattern.as_str()) {
                    continue;
                }
            }
            let row = run_bench(&mut interp, entry, &name, &path.to_string_lossy());
            if matches!(row.outcome, Outcome::Failed(_)) {
                failed += 1;
            }
            rows.push(row);
        }
    }
    if rows.is_empty() {
        if filter.is_some() && failed == 0 {
            eprintln!("aevia bench: the filter matched no benchmark");
        }
        return ExitCode::from(1);
    }
    print_table(&rows);
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

struct Row {
    name: String,
    iterations: usize,
    outcome: Outcome,
}

enum Outcome {
    Ok { total: Duration, mean: Duration },
    Failed(String),
}

/// Runs one benchmark function: warmup, then a fixed number of timed calls.
fn run_bench(
    interp: &mut Interp,
    entry: &driver::LoadedModule,
    name: &str,
    file: &str,
) -> Row {
    let arity = entry
        .module
        .decls
        .iter()
        .find_map(|decl| match &decl.kind {
            DeclKind::Fn {
                name: declared,
                args,
                ..
            } if declared == name => Some(args.len()),
            _ => None,
        })
        .unwrap_or(0);
    if arity > 0 {
        return Row {
            name: name.to_string(),
            iterations: 0,
            outcome: Outcome::Failed(format!(
                "takes {arity} parameter(s); a benchmark function takes none, so it cannot be called"
            )),
        };
    }
    let span = Span::default();
    let call = Expr::new(
        ExprKind::Call {
            callee: Box::new(Expr::new(ExprKind::Var(name.to_string()), span)),
            args: Vec::new(),
        },
        span,
    );
    let root = interp.root();
    for _ in 0..WARMUP {
        if let Err(error) = interp.eval(&call, &root) {
            return Row {
                name: name.to_string(),
                iterations: 0,
                outcome: Outcome::Failed(format!(
                    "{} while warming up (in {file})",
                    error.message
                )),
            };
        }
    }
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        if let Err(error) = interp.eval(&call, &root) {
            return Row {
                name: name.to_string(),
                iterations: 0,
                outcome: Outcome::Failed(format!("{} (in {file})", error.message)),
            };
        }
    }
    let total = start.elapsed();
    Row {
        name: name.to_string(),
        iterations: ITERATIONS,
        outcome: Outcome::Ok {
            total,
            mean: total / u32::try_from(ITERATIONS).unwrap_or(1),
        },
    }
}

fn print_table(rows: &[Row]) {
    let width = rows
        .iter()
        .map(|row| row.name.chars().count())
        .chain(std::iter::once("benchmark".len()))
        .max()
        .unwrap_or(9)
        .max(9)
        + 2;
    println!(
        "{:<width$}{:>10}  {:>12}  {:>12}",
        "benchmark",
        "iterations",
        "total",
        "mean",
        width = width
    );
    println!("{}", "-".repeat(width + 38));
    for row in rows {
        match &row.outcome {
            Outcome::Ok { total, mean } => println!(
                "{:<width$}{:>10}  {:>12}  {:>12}",
                row.name,
                row.iterations,
                duration(*total),
                duration(*mean),
                width = width
            ),
            Outcome::Failed(reason) => println!(
                "{:<width$}{:>10}  FAILED  {}",
                row.name,
                "-",
                reason,
                width = width
            ),
        }
    }
}

fn duration(value: Duration) -> String {
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

/// The files a command works on: the arguments, or the package's `src/` tree.
///
/// A directory argument is expanded to the `.aev` files under it, so
/// `aevia fmt examples` means what it looks like.
fn inputs(files: &[String]) -> Result<Vec<PathBuf>, ExitCode> {
    if !files.is_empty() {
        let mut paths = Vec::new();
        for name in files {
            let path = PathBuf::from(name);
            if path.is_dir() {
                paths.extend(crate::package::sources_under(&path));
            } else {
                paths.push(path);
            }
        }
        if paths.is_empty() {
            eprintln!("aevia: no `.aev` files in the given directories");
            return Err(ExitCode::from(1));
        }
        return Ok(paths);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let package = aevia_core::manifest::Package::find(&cwd)
        .ok()
        .flatten()
        .ok_or_else(|| {
            eprintln!(
                "aevia: no files given and no `aevia.toml` in `{}` or any parent",
                cwd.display()
            );
            ExitCode::from(2)
        })?;
    let paths = crate::package::sources_under(&package.source_dir());
    if paths.is_empty() {
        eprintln!(
            "aevia: `{}` holds no `.aev` files",
            package.source_dir().display()
        );
        return Err(ExitCode::from(1));
    }
    Ok(paths)
}
