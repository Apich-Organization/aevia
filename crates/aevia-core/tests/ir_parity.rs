//! Differential parity between the AST evaluator and the Core IR evaluator.
//!
//! This is the gate for the IR: it is only allowed to be the default
//! because every program the project has produces the *same observable result*
//! on both paths. The comparison is on values, output and error codes — never
//! on a rendered substring that a wording change could make agree by accident.
//!
//! The corpus is the project's own material, not a hand-picked set:
//!
//! * every `examples/**/*.aev`, run through the module loader so imports work;
//! * every `tests/*.aev`;
//! * every raw string literal in the `aevia-core` integration tests, which is
//!   where the semantic assertions live;
//! * a curated corpus for the constructs those files exercise least (slices,
//!   record update, guards, tensor patterns, block-local recursion, instance
//!   dispatch, `?` in a `do` block, the error codes).
//!
//! A program whose two AST runs differ is skipped as nondeterministic
//! (`time_now`), and a program that does not compile is skipped: the two paths
//! share the front end, so a compile error is not evidence about either
//! evaluator.

#![allow(clippy::result_large_err)]

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::driver;
use aevia_core::value;
use std::path::Path;
use std::path::PathBuf;

/// What one run of one program produced.
#[derive(Clone, Debug, PartialEq)]
struct Outcome {
    output: String,
    error: Option<String>,
    values: Vec<(String, String)>,
}

impl Outcome {
    fn describe(&self) -> String {
        let values = self
            .values
            .iter()
            .map(|(name, text)| format!("{name} = {text}"))
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "output {:?}, error {:?}, values {{{values}}}",
            self.output, self.error
        )
    }
}

/// Why a program was not compared.
#[derive(Clone, Debug, PartialEq)]
enum Skipped {
    /// The front end rejected it, so neither evaluator ran.
    Compile(String),
    /// Two AST runs disagree (`time_now`).
    Nondeterministic,
}

/// The result of comparing one program.
#[derive(Clone, Debug)]
enum Comparison {
    Agree,
    Skip(Skipped),
    Differ { ast: String, ir: String },
}

fn run_source(
    source: &str,
    mode: Mode,
) -> Result<Outcome, String> {
    let compilation = compile("parity.aev", source);
    if compilation.diagnostics.has_errors() {
        return Err(compilation
            .diagnostics
            .items
            .iter()
            .map(|item| item.code.to_string())
            .collect::<Vec<_>>()
            .join(","));
    }
    let Some(module) = compilation.module.clone() else {
        return Err("E0608".to_string());
    };
    let mut interp = Interp::new(&compilation.checked)
        .with_buffer()
        .with_mode(mode);
    // Both paths get the same small budget. The corpus contains a deliberate
    // runaway loop (`fn down(n) = down(n + 1)`), and the point of comparing is
    // the *outcome*, not how long the resource guard takes to fire: with the
    // production budget the IR side would burn a hundred million trampolines
    // per run. The AST path stops on depth, the IR on this budget, and both
    // report E0805 — which is what the comparison is asserting.
    interp.max_tail_calls = 10_000;
    let loaded = interp.load(&module);
    let error = if loaded.has_errors() {
        loaded.items.first().map(|item| item.code.to_string())
    } else {
        match interp.run_main() {
            | Ok(_) => None,
            | Err(failure) => Some(failure.code.to_string()),
        }
    };
    let mut values = Vec::new();
    for name in aevia_core::top_level_names(&module) {
        if let Ok(bound) = interp.eval_toplevel(&name) {
            values.push((name, value::repr(&bound)));
        }
    }
    Ok(Outcome {
        output: interp.captured(),
        error,
        values,
    })
}

fn run_file(
    path: &Path,
    mode: Mode,
) -> Result<Outcome, String> {
    let program = driver::load(path);
    if program.diagnostics.has_errors() {
        return Err(program
            .diagnostics
            .items
            .iter()
            .map(|item| item.code.to_string())
            .collect::<Vec<_>>()
            .join(","));
    }
    let Some(entry) = program.entry() else {
        return Err("E0608".to_string());
    };
    let checked = entry.checked.clone();
    let names = aevia_core::top_level_names(&entry.module);
    let mut interp = Interp::new(&checked).with_buffer().with_mode(mode);
    let loaded = driver::run_modules(&mut interp, &program);
    let error = if loaded.has_errors() {
        loaded.items.first().map(|item| item.code.to_string())
    } else {
        match interp.run_main() {
            | Ok(_) => None,
            | Err(failure) => Some(failure.code.to_string()),
        }
    };
    let mut values = Vec::new();
    for name in names {
        if let Ok(bound) = interp.eval_toplevel(&name) {
            values.push((name, value::repr(&bound)));
        }
    }
    Ok(Outcome {
        output: interp.captured(),
        error,
        values,
    })
}

fn trace_parity(message: std::fmt::Arguments<'_>) {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ENABLED.get_or_init(|| std::env::var_os("AEVIA_PARITY_TRACE").is_some()) {
        // Direct stderr survives a process abort; the test harness's captured
        // buffer cannot identify a worker that overflows before returning.
        use std::io::Write;
        let _ = writeln!(std::io::stderr().lock(), "parity: {message}");
    }
}

fn compare(run: impl Fn(Mode) -> Result<Outcome, String>) -> Comparison {
    trace_parity(format_args!("AST first"));
    let first = match run(Mode::Ast) {
        | Ok(outcome) => outcome,
        | Err(codes) => return Comparison::Skip(Skipped::Compile(codes)),
    };
    trace_parity(format_args!("AST repeat"));
    let second = match run(Mode::Ast) {
        | Ok(outcome) => outcome,
        | Err(codes) => return Comparison::Skip(Skipped::Compile(codes)),
    };
    if first != second {
        return Comparison::Skip(Skipped::Nondeterministic);
    }
    trace_parity(format_args!("IR first"));
    let ir = match run(Mode::Ir) {
        | Ok(outcome) => outcome,
        | Err(codes) => return Comparison::Skip(Skipped::Compile(codes)),
    };
    // The IR is run twice as well: a nondeterministic IR (an iteration order
    // leaking into `repr`, say) would otherwise be invisible, because only the
    // AST side was ever checked for stability.
    trace_parity(format_args!("IR repeat"));
    let ir_again = match run(Mode::Ir) {
        | Ok(outcome) => outcome,
        | Err(codes) => return Comparison::Skip(Skipped::Compile(codes)),
    };
    if ir != ir_again {
        return Comparison::Skip(Skipped::Nondeterministic);
    }
    if first == ir {
        Comparison::Agree
    } else {
        Comparison::Differ {
            ast: first.describe(),
            ir: ir.describe(),
        }
    }
}

/// Every raw string literal in a file, which is how the semantic tests write
/// their programs.
fn raw_strings(path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find("r#\"") {
        let after = &rest[start + 3..];
        let Some(end) = after.find("\"#") else {
            break;
        };
        found.push(after[..end].to_string());
        rest = &after[end + 2..];
    }
    found
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate is inside the workspace")
        .to_path_buf()
}

/// The programs that exercise the constructs the extracted corpus touches
/// least. Each is a whole program, and each is also a semantic claim the IR
/// must reproduce exactly.
const CURATED: &[&str] = &[
    // A closure must see the binding that was in scope when it was *created*,
    // not a later `let` of the same name. The AST path used to share one
    // mutable scope per block, so it read the later binding (2, not 1) while
    // the IR was lexically correct — and no program in the extracted corpus
    // had this shape, so the gate never looked.
    "fn f(): Int =
    let x = 1
    let g = fn () => x
    let x = 2
    g()

fn main(): IO Unit = do
    println(\"{f()}\")
",
    // The same defect through a deferred `do` thunk, where it is visible as
    // program output rather than as a value.
    "fn main(): IO Unit = do
    let x = 1
    let act = do
        let y = x
        println(\"{y}\")
    let x = 2
    act
",
    // Shadowing a *parameter* inside the function that captures it.
    "fn outer(n: Int): Int =
    let g = fn () => n
    let n = n + 1
    g()

fn main(): IO Unit = do
    println(\"{outer(1)}\")
",
    // Exactness and promotion.
    "let a = 1.0 + 2
let b = 1/3 + 0.5
let c = sqrt(2)
let d = sqrt(2)^2
",
    "let a = 2^10
let b = 2^-1
let c = (2/3) * (3/4)
",
    // Comparison and structure.
    "let a = Some(1) == Some(2)
let b = { x = 1, y = 2 } == { y = 2, x = 1 }
",
    // Records: literal, update, field access, nesting.
    "let p = { x = 1, y = 2 }
let q = { p with x = 10 }
let r = q.y
",
    "let p = { a = { b = 3 } }
let c = p.a.b
",
    // Tuples.
    "let t = (1, \"two\", true)
let (a, b, c) = t
",
    // Tensors: literal, nested, index, slice, matmul, dot, broadcast.
    "let a = [[1, 2], [3, 4]]
let b = a[0, 1]
let c = a[1]
",
    "let a = [[1, 2, 3], [4, 5, 6]]
let b = a[0..2, 1..3]
let c = a[.., 1]
",
    "let a = [[1, 2], [3, 4]]
let b = [[5, 6], [7, 8]]
let c = a @ b
",
    "let v = [1, 2, 3]
let w = [4, 5, 6]
let d = v @ w
",
    "let a = [1, 2, 3]
let b = a + 1
let c = 2 * a
",
    "let a = [1, 2, 3]
let b = map(fn (x: Int) => x * 2, a)
let c = filter(fn (x: Int) => x > 2, b)
",
    // Patterns: constructors, literals, guards, as, tensor, record.
    "type Shape = | Circle(Real) | Rect(Real, Real)
fn area(s: Shape): Real = match s with
    | Circle(r) -> 3 * r * r
    | Rect(w, h) -> w * h
let a = area(Circle(2.0))
",
    "fn f(n: Int): String = match n with
    | 0 -> \"zero\"
    | m if m > 0 -> \"positive\"
    | _ -> \"negative\"
let a = f(0)
let b = f(3)
let c = f(-1)
",
    "fn f(xs: Tensor<1, Int>): Int = match xs with
    | [a, b, ..rest] -> a + b + length(rest)
    | _ -> 0
let a = f([1, 2, 3, 4])
",
    "let p = (1, 2)
let q = match p with
    | (a, b) as whole -> a + b
let r = { x = 1, y = 2 }
let s = match r with
    | { x, y } -> x + y
",
    // Closures, higher order, placeholders, pipes, currying.
    "let add = fn (a: Int, b: Int) => a + b
let inc = add(1, _)
let a = inc(41)
let b = 2 |> inc
",
    "fn twice(f: (Int) -> Int, x: Int): Int = f(f(x))
let a = twice(fn (n: Int) => n + 3, 1)
",
    "let f = fn (x: Int) => fn (y: Int) => x * 10 + y
let a = f(3)(4)
",
    // Mutual recursion through top-level forward references (§7.1.2).
    "fn even(n: Int): Bool = if n == 0 then true else odd(n - 1)
fn odd(n: Int): Bool = if n == 0 then false else even(n - 1)
let a = even(10)
let b = odd(7)
",
    "fn fact(n: Int): Int = if n == 0 then 1 else n * fact(n - 1)
let a = fact(10)
",
    // `do` blocks: let, bind-free sequencing, nesting.
    "fn main(): IO Unit = do
    let x = 1 + 2
    let y = x * 2
    println(\"{y}\")
",
    "fn main(): IO Unit = do
    let f = fn (x: Int) => x + 1
    println(\"{f(1)}\")
    println(\"done\")
",
    "fn main(): IO Unit = do
    let xs = [1, 2, 3]
    println(\"{sum(xs)}\")
    println(\"{length(xs)}\")
",
    // `<-` binds the result of an action; the continuation reads it as a slot.
    "fn main(): IO Unit = do
    u <- assert(true)
    println(\"ok\")
",
    "fn main(): IO Unit = do
    r <- read_file(\"/definitely/not/here\")
    match r with
    | Ok(s) -> println(\"read {s}\")
    | Err(e) -> println(\"failed\")
",
    // `?` returns from the enclosing function.
    "fn safe(n: Int): Result<Int, String> = if n < 0 then Err(\"negative\") else Ok(n)
fn use(n: Int): Result<Int, String> =
    let a = safe(n)?
    Ok(a + 1)
let a = use(1)
let b = use(-1)
",
    // Strings and interpolation.
    "let s = \"a\tb\"
let t = \"x = {1 + 1}\"
let u = \"\"\"
line
\"\"\"
",
    // Quantities and units.
    "let d = 1000 m
let t = 2 s
let v = d / t
let e = 3 kg * 2 m / 1 s^2
",
    "let t = 25 °C
let k = 0 K
",
    // Classes and instances.
    "type Vec2 = { x: Float, y: Float }
instance Show for Vec2 where
    show(v: Vec2): String = \"<{v.x},{v.y}>\"
let a = show({ x = 1.0, y = 2.0 })
",
    // Rssn operators, pinned by span.
    "let a = Rssn.sin(0.0)
let b = Rssn.sqrt(4.0)
",
    // Short-circuit evaluation.
    "fn boom(n: Int): Bool = n == 0
let a = false && boom(1)
let b = true || boom(1)
let c = true && boom(0)
",
    // Empty tensors.
    "let a: Tensor<1, Int> = []
let b = filter(fn (x: Int) => x > 10, [1, 2, 3])
let c = b + 1
",
    // Runtime failures: the code must agree, not just the fact of failing.
    "fn f(a: Float): Float = 10.0 / a
let a = f(0.0)
",
    "fn f(i: Int): Int = [1, 2, 3][i]
let a = f(5)
",
    "let Some(x) = None
let a = x
",
    "fn f(n: Int): Int = 1 + f(n + 1)
let a = f(0)
",
    // Nested if / let chains and shadowing.
    "fn f(a: Int, b: Int): Int =
    if a > b then
        let d = a - b
        if d > 10 then 10 else d
    else 0
let a = f(20, 1)
",
    "let x = 1
fn f(x: Int): Int =
    let x = x + 1
    let x = x * 2
    x
let a = f(3)
let b = x
",
    // A `do` block whose statements depend on earlier bindings.
    "fn main(): IO Unit = do
    let a = 1
    let b = a + 1
    let c = b + 1
    println(\"{a} {b} {c}\")
",
];

/// Runs the corpus on the stack the CLI gives the interpreter (§9.9: 256 MiB).
///
/// The AST path recurses for a legal deep program and for a runaway one up to
/// the depth guard, and the harness's default stack is far smaller than the
/// stack a program is judged by.
fn with_cli_stack<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(body)
        .expect("spawn")
        .join()
        .expect("the test body panicked")
}

#[test]
fn the_two_evaluators_agree_on_every_program_we_have() {
    with_cli_stack(compare_corpus);
}

fn compare_corpus() {
    let root = workspace();
    let mut agreed = 0usize;
    let mut skipped = Vec::new();
    let mut differed = Vec::new();
    let mut programs = 0usize;

    // Files: examples (with their imports) and the `.aev` test programs.
    let mut files: Vec<PathBuf> = Vec::new();
    for directory in ["examples", "tests"] {
        let base = root.join(directory);
        collect(&base, &mut files);
    }
    files.sort();
    for path in &files {
        programs += 1;
        trace_parity(format_args!("file {}", path.display()));
        match compare(|mode| run_file(path, mode)) {
            | Comparison::Agree => agreed += 1,
            | Comparison::Skip(reason) => {
                skipped.push((path.display().to_string(), reason));
            },
            | Comparison::Differ { ast, ir } => {
                differed.push((path.display().to_string(), ast, ir));
            },
        }
    }

    // Raw strings from the semantic tests. The `ir_*` files are excluded on
    // purpose: they are *about* the two evaluators, and `ir_tail.rs`
    // deliberately demonstrates the one program where the paths must differ
    // (a million tail calls are `E0805` on the AST path and a result on the
    // IR path), so including them would be asking the gate to prove the thing
    // it is measuring.
    let tests = root.join("crates/aevia-core/tests");
    let mut sources = Vec::new();
    for entry in std::fs::read_dir(&tests).expect("the test directory") {
        let path = entry.expect("a directory entry").path();
        let excluded = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("ir_"));
        if path.extension().is_some_and(|ext| ext == "rs") && !excluded {
            sources.extend(
                raw_strings(&path)
                    .into_iter()
                    .enumerate()
                    .map(|(index, source)| (path.clone(), index, source)),
            );
        }
    }
    sources.sort_by(|left, right| (&left.0, left.1).cmp(&(&right.0, right.1)));
    for (path, index, source) in &sources {
        programs += 1;
        trace_parity(format_args!(
            "source {} #{index}: {:?}",
            path.display(),
            source.lines().next().unwrap_or("")
        ));
        match compare(|mode| run_source(source, mode)) {
            | Comparison::Agree => agreed += 1,
            | Comparison::Skip(reason) => {
                skipped.push((
                    format!(
                        "<test source {}>",
                        source.chars().take(40).collect::<String>()
                    ),
                    reason,
                ));
            },
            | Comparison::Differ { ast, ir } => {
                differed.push((source.clone(), ast, ir));
            },
        }
    }

    // The curated corpus.
    for source in CURATED {
        programs += 1;
        trace_parity(format_args!(
            "curated {:?}",
            source.lines().next().unwrap_or("")
        ));
        match compare(|mode| run_source(source, mode)) {
            | Comparison::Agree => agreed += 1,
            | Comparison::Skip(reason) => skipped.push(((*source).to_string(), reason)),
            | Comparison::Differ { ast, ir } => differed.push(((*source).to_string(), ast, ir)),
        }
    }

    eprintln!(
        "parity: {programs} programs, {agreed} agreed, {} skipped, {} differed",
        skipped.len(),
        differed.len()
    );
    for (name, reason) in &skipped {
        eprintln!("  skipped {name}: {reason:?}");
    }
    for (name, ast, ir) in &differed {
        eprintln!("  DIFFERS {name}\n    ast: {ast}\n    ir:  {ir}");
    }
    assert!(
        differed.is_empty(),
        "{} program(s) disagree between the AST and IR evaluators",
        differed.len()
    );
    // A soft threshold lets the corpus rot: dropping thirty programs still
    // passed at `agreed >= 60`. The number is pinned instead, so a change that
    // silently stops extracting programs fails here.
    assert!(
        agreed >= 98,
        "the corpus agreed on only {agreed} programs (was 98); a program was \
         dropped rather than compared"
    );
    assert!(
        programs >= 100,
        "the corpus shrank to {programs} programs (was 100)"
    );
}

fn collect(
    directory: &Path,
    out: &mut Vec<PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "aev") {
            out.push(path);
        }
    }
}

/// Parses the declarations of a source snippet, the way the REPL does.
fn declarations(source: &str) -> Vec<aevia_syntax::Decl> {
    let mut sources = aevia_syntax::SourceMap::new();
    let file = sources.add("repl.aev", source);
    let lexed = aevia_syntax::lex(file, source).expect("the snippet lexes");
    let (module, diagnostics) = aevia_syntax::parse_module(file, source, &lexed);
    assert!(
        !diagnostics.has_errors(),
        "parse errors: {:?}",
        diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    module.decls
}

#[test]
fn an_incremental_session_agrees_on_both_paths() {
    // The REPL appends declarations to a running interpreter (§9.8), which is a
    // different entry point from `load`: it lowers one declaration at a time.
    let lines = [
        "let a = 1\n",
        "let b = a + 1\n",
        "fn f(x: Int): Int = x * 2\n",
        "let c = f(b)\n",
        "let g = fn (x: Int) => x + a\n",
        "let d = g(c)\n",
    ];
    let mut seen = Vec::new();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&aevia_types::CheckedModule::default())
            .with_buffer()
            .with_mode(mode);
        for line in lines {
            for decl in declarations(line) {
                interp.load_decl(&decl).expect("the declaration loads");
            }
        }
        let c = interp.eval_toplevel("c").expect("c is bound");
        let d = interp.eval_toplevel("d").expect("d is bound");
        seen.push((value::repr(&c), value::repr(&d)));
    }
    assert_eq!(
        seen[0], seen[1],
        "the two paths disagree on an incremental session"
    );
    assert_eq!(seen[0], ("4".to_string(), "5".to_string()));
}
