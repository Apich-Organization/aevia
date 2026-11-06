//! `aevia fmt`, `aevia doc`, `aevia bench` — the source-level tools.

use crate::Options;
use crate::apply_options;
use crate::load_program;
use crate::report;
use crate::style;
use aevia_core::Interp;
use aevia_core::driver;
use aevia_syntax::DeclKind;
use aevia_syntax::Expr;
use aevia_syntax::ExprKind;
use aevia_syntax::Span;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use std::time::Instant;

/// How many untimed iterations a benchmark runs first, unless the user says.
const DEFAULT_WARMUP: usize = 3;
/// How many iterations a benchmark is timed over, unless the user says.
const DEFAULT_ITERATIONS: usize = 100;
/// §9.12's regression threshold: 5 % (a ratio, not a percentage point).
const DEFAULT_THRESHOLD: f64 = 0.05;

/// `aevia fmt [--check] [files…]`
pub fn fmt(
    _options: Options,
    args: &[String],
) -> ExitCode {
    let mut check = false;
    let mut files: Vec<String> = Vec::new();
    for arg in args {
        match arg.as_str() {
            | "--check" => check = true,
            | other if other.starts_with('-') => {
                eprintln!("aevia fmt: unknown option `{other}` (the options are `--check`)");
                return ExitCode::from(2);
            },
            | other => files.push(other.to_string()),
        }
    }
    let paths = match inputs(&files) {
        | Ok(paths) => paths,
        | Err(code) => return code,
    };
    let mut changed = 0usize;
    let mut failed = 0usize;
    for path in &paths {
        let source = match std::fs::read_to_string(path) {
            | Ok(source) => source,
            | Err(error) => {
                eprintln!("aevia fmt: cannot read `{}`: {error}", path.display());
                failed += 1;
                continue;
            },
        };
        match aevia_fmt::format_source(&source) {
            | Ok(formatted) => {
                if formatted == source {
                    continue;
                }
                changed += 1;
                if check {
                    println!("would reformat {}", path.display());
                } else {
                    match std::fs::write(path, &formatted) {
                        | Ok(()) => println!("formatted {}", path.display()),
                        | Err(error) => {
                            eprintln!("aevia fmt: cannot write `{}`: {error}", path.display());
                            failed += 1;
                        },
                    }
                }
            },
            | Err(error) => {
                // The file is left exactly as it was; the message names the
                // construct that could not be printed.
                eprintln!("aevia fmt: {}: {error}", path.display());
                failed += 1;
            },
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

/// `aevia doc [--out DIR] [--open] [files…]`
pub fn doc(
    _options: Options,
    args: &[String],
) -> ExitCode {
    let mut out: Option<PathBuf> = None;
    let mut files: Vec<String> = Vec::new();
    let mut open = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            | "--out" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    eprintln!("aevia doc: `--out` expects a directory");
                    return ExitCode::from(2);
                };
                out = Some(PathBuf::from(value));
            },
            | "--open" => open = true,
            | other if other.starts_with('-') => {
                eprintln!(
                    "aevia doc: unknown option `{other}` (the options are `--out DIR` and `--open`)"
                );
                return ExitCode::from(2);
            },
            | other => files.push(other.to_string()),
        }
        index += 1;
    }
    let paths = match inputs(&files) {
        | Ok(paths) => paths,
        | Err(code) => return code,
    };
    let package = aevia_core::manifest::Package::find(
        &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    )
    .ok()
    .flatten();
    let directory = out.unwrap_or_else(|| match &package {
        | Some(package) => package.dir.join("docs"),
        | None => PathBuf::from("docs"),
    });
    let name = package.as_ref().map_or_else(
        || "Aevia".to_string(),
        |package| package.manifest.name.clone(),
    );
    let mut docs = Vec::new();
    let mut failed = 0usize;
    for path in &paths {
        let source = match std::fs::read_to_string(path) {
            | Ok(source) => source,
            | Err(error) => {
                eprintln!("aevia doc: cannot read `{}`: {error}", path.display());
                failed += 1;
                continue;
            },
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
            | Ok(doc) => docs.push(doc),
            | Err(message) => {
                eprintln!("aevia doc: {}: {message}", path.display());
                failed += 1;
            },
        }
    }
    if failed > 0 {
        return ExitCode::from(1);
    }
    docs.sort_by(|left, right| left.path.cmp(&right.path));
    if let Err(error) = std::fs::create_dir_all(&directory) {
        eprintln!(
            "aevia doc: cannot create `{}`: {error}",
            directory.display()
        );
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
        eprintln!(
            "aevia doc: cannot write `{}`: {error}",
            index_path.display()
        );
        return ExitCode::from(1);
    }
    println!("wrote {}", index_path.display());
    let declarations: usize = docs.iter().map(|doc| doc.decls.len()).sum();
    let undocumented: usize = docs
        .iter()
        .map(aevia_doc::ModuleDoc::undocumented_count)
        .sum();
    println!(
        "{declarations} declaration(s) in {} module(s), {undocumented} without a `##` doc comment",
        docs.len()
    );
    // §7.5.5: the examples are compiled too, by `aevia test --doc`.
    println!("run `aevia test --doc` to compile the `aevia` examples in these pages");
    if open {
        match absolute(&index_path) {
            | Ok(path) => open_documentation(&path),
            | Err(message) => eprintln!("aevia doc: --open: {message}"),
        }
    }
    ExitCode::SUCCESS
}

/// The absolute path of a written file, for a message a user can click.
fn absolute(path: &std::path::Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .map_err(|error| format!("cannot resolve `{}`: {error}", path.display()))
}

/// Opens the generated index in the platform's viewer.
///
/// `--open` is a request to *show* the docs. When standard output is not a
/// terminal (a script, a pipe, a test) there is no one to show them to, so the
/// path is printed instead of spawning a viewer nobody asked for.
fn open_documentation(index: &std::path::Path) {
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        println!("the documentation is at {}", index.display());
        return;
    }
    #[cfg(target_os = "windows")]
    let command = std::process::Command::new("cmd")
        .args(["/C", "start", ""])
        .arg(index)
        .spawn();
    #[cfg(target_os = "macos")]
    let command = std::process::Command::new("open").arg(index).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let command = std::process::Command::new("xdg-open").arg(index).spawn();
    match command {
        | Ok(_) => println!("opened {}", index.display()),
        | Err(error) => {
            // Not being able to launch a viewer is not a documentation
            // failure: the files are written and the path is the answer.
            eprintln!(
                "aevia doc: --open: cannot start a viewer ({error}); the documentation is at {}",
                index.display()
            );
        },
    }
}

/// `aevia bench [--filter PAT] [--iterations N] [--warmup N] [files…]`
pub fn bench(
    options: Options,
    args: &[String],
) -> ExitCode {
    let mut filter: Option<String> = None;
    let mut iterations = DEFAULT_ITERATIONS;
    let mut warmup = DEFAULT_WARMUP;
    let mut threshold = DEFAULT_THRESHOLD;
    let mut save_baseline_path: Option<PathBuf> = None;
    let mut compare_baseline_path: Option<PathBuf> = None;
    let mut files: Vec<String> = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            | "--filter" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    eprintln!("aevia bench: `--filter` expects a pattern");
                    return ExitCode::from(2);
                };
                filter = Some(value.clone());
            },
            // A benchmark whose single call takes a second cannot be timed a
            // hundred times; the count is the user's, not a constant.
            | "--iterations" => {
                index += 1;
                let Some(value) = args.get(index).and_then(|value| value.parse().ok()) else {
                    eprintln!("aevia bench: `--iterations` expects a positive count");
                    return ExitCode::from(2);
                };
                if value == 0 {
                    eprintln!("aevia bench: `--iterations` must be at least 1");
                    return ExitCode::from(2);
                }
                iterations = value;
            },
            | "--warmup" => {
                index += 1;
                let Some(value) = args.get(index).and_then(|value| value.parse().ok()) else {
                    eprintln!("aevia bench: `--warmup` expects a count of untimed calls");
                    return ExitCode::from(2);
                };
                warmup = value;
            },
            | "--save-baseline" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    eprintln!("aevia bench: `--save-baseline` expects a file path");
                    return ExitCode::from(2);
                };
                save_baseline_path = Some(PathBuf::from(value));
            },
            | "--compare-baseline" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    eprintln!("aevia bench: `--compare-baseline` expects a file path");
                    return ExitCode::from(2);
                };
                compare_baseline_path = Some(PathBuf::from(value));
            },
            // §9.12's 5 % gate; `0.05` and `5%` both mean the same thing.
            | "--threshold" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    eprintln!("aevia bench: `--threshold` expects a ratio such as `0.05` or `5%`");
                    return ExitCode::from(2);
                };
                match parse_threshold(value) {
                    | Ok(parsed) => threshold = parsed,
                    | Err(message) => {
                        eprintln!("aevia bench: {message}");
                        return ExitCode::from(2);
                    },
                }
            },
            | other if other.starts_with('-') => {
                eprintln!(
                    "aevia bench: unknown option `{other}` (the options are `--filter PAT`, \
                     `--iterations N`, `--warmup N`, `--save-baseline FILE`, \
                     `--compare-baseline FILE` and `--threshold RATIO`)"
                );
                return ExitCode::from(2);
            },
            | other => files.push(other.to_string()),
        }
        index += 1;
    }
    if save_baseline_path.is_some() && compare_baseline_path.is_some() {
        eprintln!(
            "aevia bench: `--save-baseline` and `--compare-baseline` are different runs; \
             pass one of them"
        );
        return ExitCode::from(2);
    }
    if cfg!(debug_assertions) {
        // A release build is 10-20× faster for interpreter loops; reporting
        // debug numbers without saying so invites a comparison that means
        // nothing. The §9.9 `--release` flag is not this: it has no effect.
        eprintln!(
            "aevia bench: note: this is a debug build; the timings are not representative. \
             Build with `cargo build --release` for comparable numbers (§9.9's `--release` flag \
             does not change the build)."
        );
    }
    let paths = match inputs(&files) {
        | Ok(paths) => paths,
        | Err(code) => return code,
    };
    let mut rows: Vec<Row> = Vec::new();
    let mut failed = 0usize;
    for path in &paths {
        let program = match load_program(path, style(options)) {
            | Ok(program) => program,
            | Err(code) => return code,
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
                | DeclKind::Fn { name, body, .. } => Some((name.clone(), body)),
                | _ => None,
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
            let row = run_bench(
                &mut interp,
                entry,
                &name,
                &path.to_string_lossy(),
                warmup,
                iterations,
            );
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
    // Say what was measured, so a number is not mistaken for a different
    // measurement than it is (§9.12).
    println!();
    println!(
        "path: `Interp::eval`, the interpreter entry `aevia run` uses (module functions lowered \
         to the Core IR run on the IR evaluator); not a separate benchmark harness"
    );
    println!(
        "protocol: compare only release builds (`cargo build --release`), on the same machine, \
         from the median of N iterations; timings include interpreter allocation but allocations \
         are not counted"
    );
    let mut recorded = 0usize;
    if let Some(path) = &save_baseline_path {
        match save_baseline(path, &rows, iterations, warmup) {
            | Ok(saved) => {
                println!(
                    "wrote baseline `{}` ({saved} workload(s), profile `{}`)",
                    path.display(),
                    profile()
                );
                recorded += saved;
            },
            | Err(message) => {
                eprintln!("aevia bench: cannot save the baseline: {message}");
                return ExitCode::from(2);
            },
        }
    }
    if let Some(path) = &compare_baseline_path {
        match compare_baseline(path, &rows, threshold) {
            | Ok(comparison) => {
                let percent = threshold * 100.0;
                println!(
                    "{} benchmark(s) compared against `{}`, {} regression(s) over {percent:.1}% \
                     (profile `{}`)",
                    comparison.compared,
                    path.display(),
                    comparison.regressions,
                    profile()
                );
                recorded = comparison.compared;
                if comparison.regressions > 0 {
                    return ExitCode::from(1);
                }
            },
            | Err((code, message)) => {
                eprintln!("aevia bench: cannot compare with the baseline: {message}");
                return code;
            },
        }
    }
    let _ = recorded;
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// The build profile a timing belongs to.
fn profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

/// Parses `0.05` or `5%` into the ratio `0.05`.
fn parse_threshold(text: &str) -> Result<f64, String> {
    let text = text.trim();
    let (number, scale) = match text.strip_suffix('%') {
        | Some(number) => (number.trim(), 100.0),
        | None => (text, 1.0),
    };
    let value: f64 = number
        .parse()
        .map_err(|_| format!("`{text}` is not a ratio; write `0.05` or `5%`"))?;
    if !value.is_finite() || value < 0.0 {
        return Err(format!("`{text}` is not a non-negative ratio"));
    }
    Ok(value / scale)
}

/// The measured parts of one row, or `None` when it failed.
impl Row {
    fn measured(&self) -> Option<(Duration, Duration, Duration)> {
        match &self.outcome {
            | Outcome::Ok {
                median,
                mean,
                confidence,
            } => Some((*median, *mean, *confidence)),
            | Outcome::Failed(_) => None,
        }
    }
}

/// Writes the successful rows as a JSON baseline (§9.12).
///
/// The file records the profile, the iteration protocol and the tool version,
/// because a median without them is not comparable to anything: a debug build
/// or a different N is a different measurement.
fn save_baseline(
    path: &Path,
    rows: &[Row],
    iterations: usize,
    warmup: usize,
) -> Result<usize, String> {
    let mut benchmarks = serde_json::Map::new();
    for row in rows {
        let Some((median, mean, confidence)) = row.measured() else {
            continue;
        };
        benchmarks.insert(
            row.name.clone(),
            serde_json::json!({
                "median_ns": nanos(median),
                "mean_ns": nanos(mean),
                "ci_ns": nanos(confidence),
            }),
        );
    }
    if benchmarks.is_empty() {
        return Err("no benchmark ran successfully, so there is nothing to record".to_string());
    }
    let saved = benchmarks.len();
    let document = serde_json::json!({
        "format": 1,
        "tool": "aevia bench",
        "version": env!("CARGO_PKG_VERSION"),
        "profile": profile(),
        "iterations": iterations,
        "warmup": warmup,
        "benchmarks": benchmarks,
    });
    let text = serde_json::to_string_pretty(&document)
        .map_err(|error| format!("cannot serialise the baseline: {error}"))?;
    std::fs::write(path, format!("{text}\n"))
        .map_err(|error| format!("cannot write `{}`: {error}", path.display()))?;
    Ok(saved)
}

/// What a baseline comparison found.
struct Comparison {
    /// How many workloads were compared.
    compared: usize,
    /// How many exceeded the threshold.
    regressions: usize,
}

/// Compares the run's medians with a stored baseline (§9.12).
///
/// A workload regresses when its median is more than `threshold` above the
/// stored median. Workloads present on only one side are reported and do not
/// count as regressions: adding or removing a benchmark is a change to the
/// suite, not a slowdown.
fn compare_baseline(
    path: &Path,
    rows: &[Row],
    threshold: f64,
) -> Result<Comparison, (ExitCode, String)> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        (
            ExitCode::from(2),
            format!("cannot read `{}`: {error}", path.display()),
        )
    })?;
    let document: serde_json::Value = serde_json::from_str(&text).map_err(|error| {
        (
            ExitCode::from(2),
            format!(
                "`{}` is not a baseline JSON document: {error}",
                path.display()
            ),
        )
    })?;
    let stored_profile = document.get("profile").and_then(serde_json::Value::as_str);
    if stored_profile != Some(profile()) {
        return Err((
            ExitCode::from(2),
            format!(
                "the baseline was recorded from a `{}` build but this run is a `{}` build; \
                 timings from different profiles are not comparable — re-save the baseline \
                 with this binary",
                stored_profile.unwrap_or("unknown"),
                profile()
            ),
        ));
    }
    let benchmarks = document
        .get("benchmarks")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            (
                ExitCode::from(2),
                format!("`{}` has no `benchmarks` table", path.display()),
            )
        })?;
    let mut compared = 0usize;
    let mut regressions = 0usize;
    let mut current: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for row in rows {
        let Some((median, _, _)) = row.measured() else {
            continue;
        };
        current.insert(row.name.as_str());
        let Some(entry) = benchmarks.get(&row.name) else {
            println!(
                "{}: {} — no baseline entry (new benchmark), not compared",
                row.name,
                duration(median)
            );
            continue;
        };
        let Some(baseline_ns) = entry.get("median_ns").and_then(serde_json::Value::as_u64) else {
            println!(
                "{}: baseline entry has no `median_ns`, not compared",
                row.name
            );
            continue;
        };
        if baseline_ns == 0 {
            println!("{}: baseline median is zero, not compared", row.name);
            continue;
        }
        let current_ns = nanos(median);
        let ratio = current_ns as f64 / baseline_ns as f64;
        let delta = ratio - 1.0;
        compared += 1;
        if delta > threshold {
            regressions += 1;
            println!(
                "{}: {} vs baseline {} ({:+.1}%) REGRESSION over {:.1}%",
                row.name,
                duration(median),
                duration(Duration::from_nanos(baseline_ns)),
                delta * 100.0,
                threshold * 100.0
            );
        } else {
            println!(
                "{}: {} vs baseline {} ({:+.1}%) ok",
                row.name,
                duration(median),
                duration(Duration::from_nanos(baseline_ns)),
                delta * 100.0
            );
        }
    }
    for name in benchmarks.keys() {
        if !current.contains(name.as_str()) {
            println!("{name}: in the baseline but not measured in this run, not compared");
        }
    }
    Ok(Comparison {
        compared,
        regressions,
    })
}

/// A duration's nanoseconds, saturating at `u64::MAX`.
fn nanos(value: Duration) -> u64 {
    u64::try_from(value.as_nanos()).unwrap_or(u64::MAX)
}

struct Row {
    name: String,
    iterations: usize,
    outcome: Outcome,
}

enum Outcome {
    Ok {
        median: Duration,
        mean: Duration,
        /// Half-width of the 95 % confidence interval of the mean.
        confidence: Duration,
    },
    Failed(String),
}

/// Runs one benchmark function: `warmup` untimed calls, then `iterations`
/// timed calls.
///
/// Each call is timed separately so the row can report the **median** and a
/// 95 % confidence interval for the mean (§9.12), not just an average that a
/// single slow call can drag. The call goes through `Interp::eval` — the entry
/// `aevia run` uses, not a separate harness — so the number is the cost of the
/// interpreter path the user actually runs.
fn run_bench(
    interp: &mut Interp,
    entry: &driver::LoadedModule,
    name: &str,
    file: &str,
    warmup: usize,
    iterations: usize,
) -> Row {
    let arity = entry
        .module
        .decls
        .iter()
        .find_map(|decl| match &decl.kind {
            | DeclKind::Fn {
                name: declared, args, ..
            } if declared == name => Some(args.len()),
            | _ => None,
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
    for _ in 0..warmup {
        if let Err(error) = interp.eval(&call, &root) {
            return Row {
                name: name.to_string(),
                iterations: 0,
                outcome: Outcome::Failed(format!("{} while warming up (in {file})", error.message)),
            };
        }
    }
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = Instant::now();
        if let Err(error) = interp.eval(&call, &root) {
            return Row {
                name: name.to_string(),
                iterations: 0,
                outcome: Outcome::Failed(format!("{} (in {file})", error.message)),
            };
        }
        samples.push(start.elapsed());
    }
    let total: Duration = samples.iter().sum();
    let count = samples.len().max(1);
    let mean = total / u32::try_from(count).unwrap_or(1);
    samples.sort_unstable();
    let median = if samples.len() % 2 == 1 {
        samples[samples.len() / 2]
    } else {
        let middle = samples.len() / 2;
        (samples[middle - 1] + samples[middle]) / 2
    };
    // The 95 % confidence interval of the mean: 1.96 · s / √n, with s the
    // sample standard deviation in seconds.
    let mean_seconds = mean.as_secs_f64();
    let variance = if samples.len() > 1 {
        samples
            .iter()
            .map(|sample| {
                let delta = sample.as_secs_f64() - mean_seconds;
                delta * delta
            })
            .sum::<f64>()
            / f64::from(u32::try_from(samples.len() - 1).unwrap_or(1))
    } else {
        0.0
    };
    let half_width = 1.96 * variance.sqrt() / (samples.len() as f64).sqrt();
    Row {
        name: name.to_string(),
        iterations,
        outcome: Outcome::Ok {
            median,
            mean,
            confidence: Duration::from_secs_f64(half_width),
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
        "{:<width$}{:>10}  {:>12}  {:>12}  {:>12}",
        "benchmark",
        "iterations",
        "median",
        "mean",
        "95% CI",
        width = width
    );
    println!("{}", "-".repeat(width + 52));
    for row in rows {
        match &row.outcome {
            | Outcome::Ok {
                median,
                mean,
                confidence,
            } => {
                println!(
                    "{:<width$}{:>10}  {:>12}  {:>12}  {:>12}",
                    row.name,
                    row.iterations,
                    duration(*median),
                    duration(*mean),
                    format!("±{}", duration(*confidence)),
                    width = width
                )
            },
            | Outcome::Failed(reason) => {
                println!(
                    "{:<width$}{:>10}  FAILED  {}",
                    row.name,
                    "-",
                    reason,
                    width = width
                )
            },
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
/// `aevia fmt examples` means what it looks like; a `*`/`?` pattern is
/// expanded even when the shell did not expand it.
pub(crate) fn inputs(files: &[String]) -> Result<Vec<PathBuf>, ExitCode> {
    if !files.is_empty() {
        let mut paths = Vec::new();
        for path in crate::package::expand_globs(files) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_accept_ratios_and_percentages() {
        // §9.12's gate is a ratio; a user may write either spelling.
        assert_eq!(parse_threshold("0.05").expect("ratio"), 0.05);
        assert_eq!(parse_threshold("5%").expect("percent"), 0.05);
        assert_eq!(parse_threshold("0").expect("zero"), 0.0);
        assert!(parse_threshold("nope").is_err());
        assert!(parse_threshold("-1").is_err());
        assert!(parse_threshold("5%%").is_err());
    }

    #[test]
    fn the_baseline_duration_unit_is_nanoseconds() {
        assert_eq!(nanos(Duration::from_nanos(42)), 42);
        assert_eq!(nanos(Duration::from_micros(2)), 2_000);
    }
}
