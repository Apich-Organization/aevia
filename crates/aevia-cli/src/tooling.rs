//! `aevia fmt`, `aevia doc`, `aevia bench` — the source-level tools.

#[path = "stdlib_locale.rs"]
mod stdlib_locale;

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
use std::fmt::Write as _;
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
#[allow(
    clippy::single_call_fn,
    reason = "the fmt command owns formatter errors and check-mode exit status"
)]
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
    enum FormatOutcome {
        Unchanged,
        Formatted(String),
        ReadError(String),
        FormatError(String),
    }
    let mut outcomes: Vec<(usize, FormatOutcome)> = (0..paths.len())
        .map(|index| (index, FormatOutcome::Unchanged))
        .collect();
    let mode = aevia_rssn::execution::default_execution_mode();
    let format_one = |index: usize| -> FormatOutcome {
        let Some(path) = paths.get(index) else {
            return FormatOutcome::Unchanged;
        };
        let source = match std::fs::read_to_string(path) {
            | Ok(source) => source,
            | Err(error) => return FormatOutcome::ReadError(error.to_string()),
        };
        match aevia_fmt::format_source(&source) {
            | Ok(formatted) if formatted == source => FormatOutcome::Unchanged,
            | Ok(formatted) => FormatOutcome::Formatted(formatted),
            | Err(error) => FormatOutcome::FormatError(error.to_string()),
        }
    };
    aevia_rssn::par_for_chunks_mut_in_mode(&mut outcomes, 1, 4, mode, |_, chunk| {
        for (index, outcome) in chunk.iter_mut() {
            *outcome = format_one(*index);
        }
    });
    let mut changed = 0usize;
    let mut failed = 0usize;
    for (index, outcome) in outcomes {
        let Some(path) = paths.get(index) else {
            continue;
        };
        match outcome {
            | FormatOutcome::Unchanged => {},
            | FormatOutcome::Formatted(formatted) => {
                changed = changed.saturating_add(1);
                if check {
                    println!("would reformat {}", path.display());
                } else {
                    match std::fs::write(path, &formatted) {
                        | Ok(()) => println!("formatted {}", path.display()),
                        | Err(error) => {
                            eprintln!("aevia fmt: cannot write `{}`: {error}", path.display());
                            failed = failed.saturating_add(1);
                        },
                    }
                }
            },
            | FormatOutcome::ReadError(error) => {
                eprintln!("aevia fmt: cannot read `{}`: {error}", path.display());
                failed = failed.saturating_add(1);
            },
            | FormatOutcome::FormatError(error) => {
                // The file is left exactly as it was; the message names the
                // construct that could not be printed.
                eprintln!("aevia fmt: {}: {error}", path.display());
                failed = failed.saturating_add(1);
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

/// `aevia doc [--out DIR] [--open] [--stdlib] [--json] [files…]`
#[allow(
    clippy::single_call_fn,
    reason = "the doc command chooses standard library or project generation"
)]
pub fn doc(args: &[String]) -> ExitCode {
    let mut out: Option<PathBuf> = None;
    let mut files: Vec<String> = Vec::new();
    let mut open = false;
    let mut stdlib = false;
    let mut json = false;
    let mut lang = None;
    let mut args = args.iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            | "--out" => {
                let Some(value) = args.next() else {
                    eprintln!("aevia doc: `--out` expects a directory");
                    return ExitCode::from(2);
                };
                out = Some(PathBuf::from(value));
            },
            | "--open" => open = true,
            | "--stdlib" => stdlib = true,
            | "--json" => json = true,
            | "--lang" => {
                lang = args.next().map(String::as_str);
                if !matches!(lang, Some("en" | "zh")) {
                    eprintln!("aevia doc: --lang expects en or zh");
                    return ExitCode::from(2);
                }
            },
            | other if other.starts_with('-') => {
                eprintln!("aevia doc: unknown option `{other}` (see `aevia doc --help`)");
                return ExitCode::from(2);
            },
            | other => files.push(other.to_string()),
        }
    }
    if lang.is_some() && !stdlib {
        eprintln!("aevia doc: --lang requires --stdlib");
        return ExitCode::from(2);
    }
    let package = aevia_core::manifest::Package::find(
        &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    )
    .ok()
    .flatten();
    let directory = out.unwrap_or_else(|| {
        match &package {
            | Some(package) => package.dir.join("docs"),
            | None => PathBuf::from("docs"),
        }
    });
    if stdlib {
        if !files.is_empty() {
            eprintln!("aevia doc: `--stdlib` does not accept file arguments");
            return ExitCode::from(2);
        }
        write_stdlib_docs(&directory, lang == Some("zh"), json, open)
    } else {
        write_project_docs(&directory, &files, package.as_ref(), json, open)
    }
    .err()
    .unwrap_or(ExitCode::SUCCESS)
}

fn write_doc_file(
    path: &Path,
    contents: impl AsRef<[u8]>,
) -> Result<(), ExitCode> {
    std::fs::write(path, contents).map_err(|error| {
        eprintln!("aevia doc: cannot write `{}`: {error}", path.display());
        ExitCode::from(1)
    })?;
    println!("wrote {}", path.display());
    Ok(())
}

fn write_doc_pages(
    directory: &Path,
    modules: &[aevia_doc::ModuleDoc],
    index: &str,
) -> Result<PathBuf, ExitCode> {
    std::fs::create_dir_all(directory).map_err(|error| {
        eprintln!(
            "aevia doc: cannot create `{}`: {error}",
            directory.display()
        );
        ExitCode::from(1)
    })?;
    for module in modules {
        write_doc_file(
            &directory.join(module.page()),
            aevia_doc::render_module(module),
        )?;
    }
    let index_path = directory.join("index.md");
    write_doc_file(&index_path, index)?;
    Ok(index_path)
}

fn write_doc_json(
    path: &Path,
    value: &(impl serde::Serialize + ?Sized),
    label: &str,
) -> Result<(), ExitCode> {
    let encoded = serde_json::to_string_pretty(value).map_err(|error| {
        eprintln!("aevia doc: cannot serialize {label}: {error}");
        ExitCode::from(1)
    })?;
    write_doc_file(path, encoded)
}

fn open_doc_index(
    open: bool,
    index_path: &Path,
) {
    use std::io::IsTerminal;
    if !open {
        return;
    }
    let path = match std::path::absolute(index_path) {
        | Ok(path) => path,
        | Err(error) => {
            eprintln!(
                "aevia doc: --open: cannot resolve `{}`: {error}",
                index_path.display()
            );
            return;
        },
    };
    if !std::io::stdout().is_terminal() {
        println!("the documentation is at {}", path.display());
        return;
    }
    match crate::terminal::open_default(path.as_os_str()) {
        | Ok(()) => println!("opened {}", path.display()),
        | Err(error) => {
            eprintln!(
                "aevia doc: --open: cannot start a viewer ({error}); the documentation is at {}",
                path.display()
            );
        },
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps translated standard library rendering and manifest output together"
)]
fn write_stdlib_docs(
    directory: &Path,
    chinese: bool,
    json: bool,
    open: bool,
) -> Result<(), ExitCode> {
    let sources = aevia_std::stdlib::modules()
        .iter()
        .map(|module| (module.path, module.source));
    let operators = aevia_rssn::registry()
        .ops()
        .map(|op| {
            aevia_doc::OperatorDoc {
                name: op.name.clone(),
                rule_set: op.rule_set.clone(),
                signatures: op.signatures.iter().map(aevia_rssn::Sig::render).collect(),
                stability: format!("{:?}", op.stability),
                doc: op.doc.clone(),
                examples: op.witnesses.clone(),
            }
        })
        .collect();
    let mut bundle = match aevia_doc::generate_stdlib_docs(sources, operators) {
        | Ok(bundle) => bundle,
        | Err(message) => {
            eprintln!("aevia doc: {message}");
            return Err(ExitCode::from(1));
        },
    };
    document_ambient_units(&mut bundle.modules, &aevia_rssn::registry().units);
    let zh_modules = match stdlib_locale::chinese_modules(&bundle.modules) {
        | Ok(modules) => modules,
        | Err(error) => {
            eprintln!("aevia doc: {error}");
            return Err(ExitCode::from(1));
        },
    };
    let modules = if chinese {
        &zh_modules
    } else {
        &bundle.modules
    };
    let index = if chinese {
        stdlib_locale::chinese_index(modules)
    } else {
        aevia_doc::render_index("Aevia Standard Library", modules)
    };
    let index_path = write_doc_pages(directory, modules, &index)?;
    let markdown = if chinese {
        std::iter::once(index)
            .chain(modules.iter().map(aevia_doc::render_module))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        bundle.to_markdown()
    };
    write_doc_file(&directory.join("stdlib.md"), markdown)?;
    if json {
        write_doc_json(
            &directory.join("stdlib-docs.json"),
            &stdlib_locale::BilingualBundle {
                original: &bundle,
                zh_modules: &zh_modules,
            },
            "stdlib docs",
        )?;
    }
    println!(
        "{} stdlib module(s) and {} RSSN operator(s)",
        bundle.modules.len(),
        bundle.operators.len()
    );
    open_doc_index(open, &index_path);
    Ok(())
}

#[allow(
    clippy::single_call_fn,
    reason = "checks all project sources before writing documentation pages"
)]
fn write_project_docs(
    directory: &Path,
    files: &[String],
    package: Option<&aevia_core::manifest::Package>,
    json: bool,
    open: bool,
) -> Result<(), ExitCode> {
    let paths = match inputs(files) {
        | Ok(paths) => paths,
        | Err(code) => return Err(code),
    };
    let name = package.map_or_else(
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
                failed = failed.saturating_add(1);
                continue;
            },
        };
        let fallback = path.file_stem().map_or_else(
            || "Module".to_string(),
            |stem| stem.to_string_lossy().to_string(),
        );
        // The page quotes the file relative to the package, so the generated
        // documentation is the same on every checkout.
        let shown = package
            .and_then(|package| path.strip_prefix(&package.dir).ok())
            .unwrap_or(path)
            .display()
            .to_string();
        match aevia_doc::module_doc(&source, &shown, &fallback) {
            | Ok(doc) => docs.push(doc),
            | Err(message) => {
                eprintln!("aevia doc: {}: {message}", path.display());
                failed = failed.saturating_add(1);
            },
        }
    }
    if failed > 0 {
        return Err(ExitCode::from(1));
    }
    docs.sort_by(|left, right| left.path.cmp(&right.path));
    let index_path = write_doc_pages(directory, &docs, &aevia_doc::render_index(&name, &docs))?;
    if json {
        write_doc_json(&directory.join("docs.json"), &docs, "docs")?;
    }
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
    open_doc_index(open, &index_path);
    Ok(())
}

/// Units resolve through the registry even though their source namespaces
/// do not declare bindings. Generate a reference from the same registry.
#[allow(
    clippy::single_call_fn,
    reason = "generates documentation for units provided by the kernel registry"
)]
fn document_ambient_units(
    modules: &mut [aevia_doc::ModuleDoc],
    units: &[aevia_rssn::registry::UnitInfo],
) {
    for module in modules {
        let (summary, names): (&str, &[&str]) = match module.path.as_str() {
            | "Units.SI" => {
                (
                    "The unit registry lists SI units and accepted non-SI spellings. Use a unit after a number, as in `3 km`.\n\n单位表包含 SI 单位及可用的非 SI 名称。把单位写在数字之后，例如 `3 km`。",
                    &[],
                )
            },
            | "Units.Imperial" => {
                (
                    "Use imperial and US customary units in quantities such as `6 ft`. `gal` is the US liquid gallon.\n\n英制与美制单位可以直接用于物理量，如 `6 ft`。`gal` 采用美制液量加仑。",
                    &[
                        "in", "ft", "yd", "mi", "lb", "oz", "gal", "psi", "mph", "acre", "lbf",
                        "hp", "degF",
                    ],
                )
            },
            | "Units.Astronomy" => {
                (
                    "Use these astronomical distances, masses and angles directly in quantities.\n\n这些天文距离、质量和角度单位可以直接用于物理量。",
                    &[
                        "au", "ly", "pc", "Rsun", "Msun", "Mearth", "jup", "arcmin", "arcsec",
                    ],
                )
            },
            | "Units.CGS" => {
                (
                    "Use these centimetre–gram–second units with the same dimension checking as SI.\n\n厘米–克–秒制单位与 SI 单位遵循相同的量纲检查。",
                    &["dyn", "erg", "Gal", "P", "St", "G", "Mx", "Ba", "Oe"],
                )
            },
            | _ => continue,
        };
        let mut text = format!(
            "{summary}\n\nThe coherent SI value is the magnitude times the scale, plus the offset.\n\nSI 相干单位下的值等于读数乘比例，再加偏置。\n\n"
        );
        text.push_str("| Unit 单位 | Dimension 量纲 | Scale 比例 | Offset 偏置 | Prefix 可加前缀 | Aliases 别名 |\n|---|---|---|---|---|---|\n");
        for unit in units
            .iter()
            .filter(|unit| names.is_empty() || names.contains(&unit.name.as_str()))
        {
            let offset = unit
                .offset
                .as_ref()
                .map_or_else(|| String::from("0"), ToString::to_string);
            let aliases = unit
                .aliases
                .iter()
                .map(|alias| format!("`{alias}`"))
                .collect::<Vec<_>>()
                .join(", ");
            let prefix = if unit.prefixable {
                "✓"
            } else {
                "—"
            };
            let _ = writeln!(
                text,
                "| `{}` | `{}` | `{}` | `{offset}` | {prefix} | {aliases} |",
                unit.name, unit.dim, unit.scale
            );
        }
        module.doc = Some(text);
    }
}

/// The absolute path of a written file, for a message a user can click.
/// `aevia bench [--filter PAT] [--iterations N] [--warmup N] [files…]`
#[allow(
    clippy::single_call_fn,
    reason = "the bench command coordinates reporting and baseline comparison"
)]
pub fn bench(
    options: Options,
    args: &[String],
) -> ExitCode {
    let args = match BenchArgs::parse(args) {
        | Ok(args) => args,
        | Err(code) => return code,
    };
    if cfg!(debug_assertions) {
        // A release build is 10-20× faster for interpreter loops; reporting
        // debug numbers without saying so invites a comparison that means
        // nothing. The runtime `--release` flag optimizes IR; it cannot rebuild
        // the Rust interpreter binary.
        eprintln!(
            "aevia bench: note: this is a debug build; the timings are not representative. \
             Build with `cargo build --release` for comparable numbers (§9.9's `--release` flag \
             does not change the build)."
        );
    }
    let (rows, failed) = match measure_benchmarks(options, &args) {
        | Ok(result) => result,
        | Err(code) => return code,
    };
    if rows.is_empty() {
        if args.filter.is_some() && !failed {
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
         with the same `--release` IR optimization setting, from the median of N iterations; timings include interpreter allocation but allocations \
         are not counted"
    );
    if let Some(path) = &args.save_baseline_path {
        match save_baseline(path, &rows, args.iterations, args.warmup, options.release) {
            | Ok(saved) => {
                println!(
                    "wrote baseline `{}` ({saved} workload(s), profile `{}`)",
                    path.display(),
                    profile()
                );
            },
            | Err(message) => {
                eprintln!("aevia bench: cannot save the baseline: {message}");
                return ExitCode::from(2);
            },
        }
    }
    if let Some(path) = &args.compare_baseline_path {
        match compare_baseline(path, &rows, args.threshold, options.release) {
            | Ok(comparison) => {
                let percent = args.threshold * 100.0;
                println!(
                    "{} benchmark(s) compared against `{}`, {} regression(s) over {percent:.1}% \
                     (profile `{}`)",
                    comparison.compared,
                    path.display(),
                    comparison.regressions,
                    profile()
                );
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
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

struct BenchArgs<'a> {
    filter: Option<&'a str>,
    iterations: usize,
    warmup: usize,
    threshold: f64,
    save_baseline_path: Option<PathBuf>,
    compare_baseline_path: Option<PathBuf>,
    files: Vec<String>,
}

impl<'a> BenchArgs<'a> {
    #[allow(
        clippy::single_call_fn,
        reason = "validates measurement and baseline options before loading workloads"
    )]
    fn parse(args: &'a [String]) -> Result<Self, ExitCode> {
        let mut filter = None;
        let mut iterations = DEFAULT_ITERATIONS;
        let mut warmup = DEFAULT_WARMUP;
        let mut threshold = DEFAULT_THRESHOLD;
        let mut save_baseline_path: Option<PathBuf> = None;
        let mut compare_baseline_path: Option<PathBuf> = None;
        let mut files: Vec<String> = Vec::new();
        let mut args = args.iter();
        while let Some(argument) = args.next() {
            match argument.as_str() {
                | "--filter" => {
                    let Some(value) = args.next() else {
                        eprintln!("aevia bench: `--filter` expects a pattern");
                        return Err(ExitCode::from(2));
                    };
                    filter = Some(value.as_str());
                },
                // A benchmark whose single call takes a second cannot be timed a
                // hundred times; the count is the user's, not a constant.
                | "--iterations" => {
                    let Some(value) = args.next().and_then(|value| value.parse().ok()) else {
                        eprintln!("aevia bench: `--iterations` expects a positive count");
                        return Err(ExitCode::from(2));
                    };
                    if value == 0 {
                        eprintln!("aevia bench: `--iterations` must be at least 1");
                        return Err(ExitCode::from(2));
                    }
                    iterations = value;
                },
                | "--warmup" => {
                    let Some(value) = args.next().and_then(|value| value.parse().ok()) else {
                        eprintln!("aevia bench: `--warmup` expects a count of untimed calls");
                        return Err(ExitCode::from(2));
                    };
                    warmup = value;
                },
                | "--save-baseline" => {
                    let Some(value) = args.next() else {
                        eprintln!("aevia bench: `--save-baseline` expects a file path");
                        return Err(ExitCode::from(2));
                    };
                    save_baseline_path = Some(PathBuf::from(value));
                },
                | "--compare-baseline" => {
                    let Some(value) = args.next() else {
                        eprintln!("aevia bench: `--compare-baseline` expects a file path");
                        return Err(ExitCode::from(2));
                    };
                    compare_baseline_path = Some(PathBuf::from(value));
                },
                // §9.12's 5 % gate; `0.05` and `5%` both mean the same thing.
                | "--threshold" => {
                    let Some(value) = args.next() else {
                        eprintln!(
                            "aevia bench: `--threshold` expects a ratio such as `0.05` or `5%`"
                        );
                        return Err(ExitCode::from(2));
                    };
                    match parse_threshold(value) {
                        | Ok(parsed) => threshold = parsed,
                        | Err(message) => {
                            eprintln!("aevia bench: {message}");
                            return Err(ExitCode::from(2));
                        },
                    }
                },
                | other if other.starts_with('-') => {
                    eprintln!(
                        "aevia bench: unknown option `{other}` (the options are `--filter PAT`, \
                         `--iterations N`, `--warmup N`, `--save-baseline FILE`, \
                         `--compare-baseline FILE` and `--threshold RATIO`)"
                    );
                    return Err(ExitCode::from(2));
                },
                | other => files.push(other.to_string()),
            }
        }
        if save_baseline_path.is_some() && compare_baseline_path.is_some() {
            eprintln!(
                "aevia bench: `--save-baseline` and `--compare-baseline` are different runs; \
                 pass one of them"
            );
            return Err(ExitCode::from(2));
        }
        Ok(Self {
            filter,
            iterations,
            warmup,
            threshold,
            save_baseline_path,
            compare_baseline_path,
            files,
        })
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "records benchmark timings using the ordinary interpreter"
)]
fn measure_benchmarks(
    options: Options,
    args: &BenchArgs<'_>,
) -> Result<(Vec<Row>, bool), ExitCode> {
    let paths = match inputs(&args.files) {
        | Ok(paths) => paths,
        | Err(code) => return Err(code),
    };
    let mut rows: Vec<Row> = Vec::new();
    let mut failed = false;
    for path in &paths {
        let program = match load_program(path, style(options)) {
            | Ok(program) => program,
            | Err(code) => return Err(code),
        };
        if report(&program.diagnostics, &program.sources, style(options)) {
            return Err(ExitCode::from(1));
        }
        let Some(entry) = program.entry() else {
            eprintln!("aevia bench: no module produced");
            return Err(ExitCode::from(1));
        };
        let mut interp = Interp::new(&entry.checked);
        apply_options(&mut interp, options);
        let load_diagnostics = driver::run_modules(&mut interp, &program);
        if report(&load_diagnostics, &program.sources, style(options)) {
            return Err(ExitCode::from(1));
        }
        let benches: Vec<&str> = entry
            .module
            .decls
            .iter()
            .filter(|decl| decl.has_attribute("bench"))
            .filter_map(|decl| {
                match &decl.kind {
                    | DeclKind::Fn { name, .. } => Some(name.as_str()),
                    | _ => None,
                }
            })
            .collect();
        if benches.is_empty() {
            eprintln!(
                "aevia bench: `{}` has no `@bench` functions, so nothing was measured",
                path.display()
            );
            failed = true;
            continue;
        }
        let shown = path.to_string_lossy();
        for name in benches {
            if args.filter.is_some_and(|pattern| !name.contains(pattern)) {
                continue;
            }
            let row = run_bench(
                &mut interp,
                entry,
                name,
                &shown,
                args.warmup,
                args.iterations,
            );
            if matches!(row.outcome, Outcome::Failed(_)) {
                failed = true;
            }
            rows.push(row);
        }
    }
    Ok((rows, failed))
}

/// The build profile a timing belongs to.
const fn profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

/// Parses `0.05` or `5%` into the ratio `0.05`.
#[allow(
    clippy::single_call_fn,
    reason = "parses the two accepted user formats for the regression threshold"
)]
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
    const fn measured(&self) -> Option<(Duration, Duration, Duration)> {
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
#[allow(
    clippy::single_call_fn,
    reason = "writes the benchmark results in the versioned baseline shape"
)]
fn save_baseline(
    path: &Path,
    rows: &[Row],
    iterations: usize,
    warmup: usize,
    optimized_ir: bool,
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
        "optimized_ir": optimized_ir,
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
#[allow(
    clippy::single_call_fn,
    clippy::too_many_lines,
    reason = "validates baseline settings and reports workload deltas"
)]
fn compare_baseline(
    path: &Path,
    rows: &[Row],
    threshold: f64,
    optimized_ir: bool,
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
    // Baselines predating the runtime optimizer were always unoptimized.
    let stored_optimized = document
        .get("optimized_ir")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if stored_optimized != optimized_ir {
        return Err((
            ExitCode::from(2),
            "the baseline and this run use different `--release` IR optimization settings; \
             use matching settings or re-save the baseline"
                .to_string(),
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
        #[allow(
            clippy::cast_precision_loss,
            reason = "timing ratios are approximate and displayed to one decimal place"
        )]
        let ratio = current_ns as f64 / baseline_ns as f64;
        let delta = ratio - 1.0;
        compared = compared.saturating_add(1);
        if delta > threshold {
            regressions = regressions.saturating_add(1);
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
#[allow(
    clippy::single_call_fn,
    reason = "times one declaration with bounded iteration and allocation"
)]
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
        .find_map(|decl| {
            match &decl.kind {
                | DeclKind::Fn {
                    name: declared, args, ..
                } if declared == name => Some(args.len()),
                | _ => None,
            }
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
    let total = samples
        .iter()
        .copied()
        .fold(Duration::ZERO, Duration::saturating_add);
    let count = u32::try_from(samples.len()).unwrap_or(u32::MAX).max(1);
    let mean = total.checked_div(count).unwrap_or_default();
    samples.sort_unstable();
    let middle = samples.len().div_euclid(2);
    let upper = samples.get(middle).copied().unwrap_or_default();
    let median = if samples.len().is_multiple_of(2) {
        let lower = middle
            .checked_sub(1)
            .and_then(|i| samples.get(i))
            .copied()
            .unwrap_or_default();
        lower
            .saturating_add(upper)
            .checked_div(2)
            .unwrap_or_default()
    } else {
        upper
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
            / f64::from(count.saturating_sub(1).max(1))
    } else {
        0.0
    };
    let half_width = 1.96 * variance.sqrt() / f64::from(count).sqrt();
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

#[allow(
    clippy::single_call_fn,
    reason = "formats measured durations and failed workloads in the same table"
)]
fn print_table(rows: &[Row]) {
    let width = rows
        .iter()
        .map(|row| row.name.chars().count())
        .chain(std::iter::once("benchmark".len()))
        .max()
        .unwrap_or(9)
        .max(9)
        .saturating_add(2);
    println!(
        "{:<width$}{:>10}  {:>12}  {:>12}  {:>12}",
        "benchmark",
        "iterations",
        "median",
        "mean",
        "95% CI",
        width = width
    );
    println!("{}", "-".repeat(width.saturating_add(52)));
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
                );
            },
            | Outcome::Failed(reason) => {
                println!(
                    "{:<width$}{:>10}  FAILED  {}",
                    row.name,
                    "-",
                    reason,
                    width = width
                );
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
pub fn inputs(files: &[String]) -> Result<Vec<PathBuf>, ExitCode> {
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
