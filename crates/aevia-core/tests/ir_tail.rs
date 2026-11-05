//! The observable reason the Core IR exists: a tail call is a jump.
//!
//! The AST evaluator recurses for a call in tail position, so a
//! self-tail-recursive loop exhausts the 256 MiB thread the CLI gives it. The
//! IR returns a `Tail` to its loop instead, so the same program runs in constant
//! stack — demonstrated here on a thread with **2 MiB**, which is a thousandth
//! of what a million frames would need and a hundred and twenty-eighth of the
//! stack the AST path is given. The stack is the same for a hundred thousand
//! iterations and for a million, which is what "constant" means.
//!
//! The AST path is deliberately left alone: it still bounds recursion by calls
//! (`E0805`), which is the semantic reference, and the second test shows the two
//! paths differ exactly where they should.

#![allow(clippy::result_large_err)]

use aevia_core::{compile, Interp, Mode};

/// Runs `main` on a thread with `stack` bytes and returns its output or the
/// runtime error's code.
///
/// The whole pipeline runs inside the thread: a checked module is not `Send`
/// (its values are `Rc`-backed), and the stack the interpreter is judged by is
/// the point of the test.
fn run_with_stack(source: &str, mode: Mode, stack: usize) -> Result<String, String> {
    let source = source.to_string();
    std::thread::Builder::new()
        .stack_size(stack)
        .spawn(move || {
            let compilation = compile("tail.aev", &source);
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
            let module = compilation.module.expect("module");
            let mut interp = Interp::new(&compilation.checked)
                .with_buffer()
                .with_mode(mode);
            let diagnostics = interp.load(&module);
            assert!(
                !diagnostics.has_errors(),
                "load errors: {:?}",
                diagnostics
                    .items
                    .iter()
                    .map(|item| (&item.code, &item.message))
                    .collect::<Vec<_>>()
            );
            match interp.run_main() {
                Ok(_) => Ok(interp.captured()),
                Err(error) => Err(error.code.to_string()),
            }
        })
        .expect("spawn")
        .join()
        .expect("the test body panicked")
}

const COUNT: &str = r#"fn count(n: Int, acc: Int): Int =
    if n == 0 then acc else count(n - 1, acc + 1)

fn main(): IO Unit = do
    println(count(1000000, 0))
"#;

#[test]
fn a_million_tail_calls_run_in_constant_stack() {
    // 2 MiB. A recursive implementation needs at least a few hundred bytes per
    // call, so a million calls would need hundreds of megabytes; the trampoline
    // needs one frame, so this stack holds a hundred thousand iterations and a
    // million alike.
    let output = run_with_stack(COUNT, Mode::Ir, 2 * 1024 * 1024).expect("the loop terminates");
    assert_eq!(output, "1000000\n");
}

#[test]
fn the_ast_path_still_bounds_the_same_loop_by_calls() {
    // The reference evaluator has no trampoline, so the same program is
    // `E0805` once the call depth passes the limit. That is the difference the
    // IR is for, and it is why the AST path stays selectable.
    let error = run_with_stack(
        r#"fn count(n: Int, acc: Int): Int =
    if n == 0 then acc else count(n - 1, acc + 1)

fn main(): IO Unit = do
    println(count(5000, 0))
"#,
        Mode::Ast,
        // The AST path needs a frame per call, so the depth limit alone costs
        // tens of megabytes. This is the stack the CLI gives it.
        256 * 1024 * 1024,
    )
    .expect_err("the AST path must report the depth limit");
    assert_eq!(error, "E0805");

    let output = run_with_stack(
        r#"fn count(n: Int, acc: Int): Int =
    if n == 0 then acc else count(n - 1, acc + 1)

fn main(): IO Unit = do
    println(count(5000, 0))
"#,
        Mode::Ir,
        2 * 1024 * 1024,
    )
    .expect("the IR path has no depth limit to hit");
    assert_eq!(output, "5000\n");
}

#[test]
fn mutual_tail_recursion_is_also_constant_stack() {
    let output = run_with_stack(
        r#"fn even(n: Int): Bool = if n == 0 then true else odd(n - 1)
fn odd(n: Int): Bool = if n == 0 then false else even(n - 1)

fn main(): IO Unit = do
    println(even(200000))
    println(odd(200000))
"#,
        Mode::Ir,
        2 * 1024 * 1024,
    )
    .expect("mutual tail recursion terminates");
    assert_eq!(output, "true\nfalse\n");
}

#[test]
fn a_tail_call_through_a_match_is_trampolined() {
    // The tail position is inside a `match` arm, which is the shape a written
    // loop usually takes.
    let output = run_with_stack(
        r#"fn step(state: Int, fuel: Int): Int =
    match fuel with
    | 0 -> state
    | f -> step(state + 1, f - 1)

fn main(): IO Unit = do
    println(step(0, 400000))
"#,
        Mode::Ir,
        2 * 1024 * 1024,
    )
    .expect("the loop terminates");
    assert_eq!(output, "400000\n");
}

/// Measurement harness (ignored by default): the wall-clock cost of one
/// trampoline step, Int and Float.
///
/// Run with:
/// `cargo test -p aevia-core --test ir_tail -- --ignored --nocapture`.
///
/// These are debug-build numbers in the same unit as the performance
/// research's baselines (30.7 µs/step Int, 22.2 µs/step Float before the
/// unconditional stack clone and the per-evaluation literal parse were
/// removed), so a regression or a win is visible. The test asserts nothing: a
/// timing assertion would be flaky on a shared build machine.
#[test]
#[ignore = "measurement harness"]
fn bench_tail_call_step() {
    const STEPS: u64 = 100_000;
    let cases = [
        (
            "int",
            format!(
                "fn count(n: Int, acc: Int): Int =\n    if n == 0 then acc else count(n - 1, acc + 1)\n\nfn main(): IO Unit = do\n    println(count({STEPS}, 0))\n"
            ),
        ),
        (
            "float",
            format!(
                "fn count(n: Int, acc: Float): Float =\n    if n == 0 then acc else count(n - 1, acc + 0.5)\n\nfn main(): IO Unit = do\n    println(show(count({STEPS}, 0.0)))\n"
            ),
        ),
    ];
    for (label, source) in cases {
        // One warm-up run so the reported number is not the first-run cost.
        run_with_stack(&source, Mode::Ir, 2 * 1024 * 1024).expect("warm-up runs");
        let start = std::time::Instant::now();
        run_with_stack(&source, Mode::Ir, 2 * 1024 * 1024).expect("the loop runs");
        let elapsed = start.elapsed().as_secs_f64();
        println!(
            "{label}: {:.2} µs/step over {STEPS} steps ({elapsed:.3} s)",
            elapsed * 1e6 / STEPS as f64
        );
    }
}
