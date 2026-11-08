//! §9.7: parallel evaluation and concurrent tasks. Parallelism must never
//! change a value: every result here is compared with the sequential one.

fn run_with_jobs(
    source: &str,
    jobs: usize,
) -> String {
    let compilation = aevia_core::compile("par.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let module = compilation.module.as_ref().expect("parsed");
    let mut interp = aevia_core::Interp::new(&compilation.checked).with_buffer();
    interp.set_jobs(jobs);
    let replica_source = source.to_string();
    interp.set_replica(std::sync::Arc::new(move || {
        let compilation = aevia_core::compile("par.aev", &replica_source);
        let module = compilation.module.as_ref().ok_or("no module")?;
        let mut replica = aevia_core::Interp::new(&compilation.checked).with_buffer();
        if replica.load(module).has_errors() {
            return Err("load failed".to_string());
        }
        Ok(replica)
    }));
    assert!(!interp.load(module).has_errors());
    interp.run_main().expect("runs");
    interp.captured()
}

const PROGRAM: &str = r#"fn heavy(x: Float): Float = sum(map(fn (k) => sin(x * k), linspace(0.0, 1.0, 50)))

fn collatz(n: Int, steps: Int): Int =
    if n == 1 then steps
    else if mod(n, 2) == 0 then collatz(quot(n, 2), steps + 1)
    else collatz(3 * n + 1, steps + 1)

fn greet(): IO Float = do
    println("from the task")
    pure(heavy(1.5))

fn main(): IO Unit = do
    let xs = linspace(0.0, 10.0, 300)
    let scale = 2.5
    let ys = par_map(fn (x) => scale * heavy(x), xs)
    println(ys[7])
    println(par_reduce(fn (a, b) => a + b, 0.0, ys))
    let steps = par_tabulate(500, fn (i) => collatz(i + 1, 0))
    println(max_all(steps))
    println(par_map(fn (n) => n * n, [1, 2, 3, 4, 5]))
    t <- spawn(greet)
    u <- spawn(fn () => pure(sum(ys)))
    println("before await")
    b <- await(u)
    a <- await(t)
    println("{a} {b}")
"#;

#[test]
fn parallel_results_equal_sequential_results() {
    let before = aevia_core::parallel::stats().0;
    let parallel = run_with_jobs(PROGRAM, 4);
    let dispatched = aevia_core::parallel::stats().0 - before;
    let sequential = run_with_jobs(PROGRAM, 1);
    assert_eq!(parallel, sequential);
    assert!(dispatched > 0, "no work reached a worker thread");
    // Task output is written when the task is awaited, in await order.
    let lines: Vec<&str> = parallel.lines().collect();
    assert_eq!(lines[4], "before await");
    assert_eq!(lines[5], "from the task");
    assert_eq!(lines[3], "[1, 4, 9, 16, 25]");
}

#[test]
fn a_reduction_is_independent_of_the_thread_count() {
    let source = r#"fn main(): IO Unit = do
    let xs = map(fn (i) => 1.0 / (i + 1.0), linspace(0.0, 4999.0, 5000))
    println(par_reduce(fn (a, b) => a + b, 0.0, xs))
"#;
    let one = run_with_jobs(source, 1);
    let two = run_with_jobs(source, 2);
    let eight = run_with_jobs(source, 8);
    assert_eq!(one, two);
    assert_eq!(one, eight);
}

#[test]
fn a_worker_error_is_reported_with_its_source_location() {
    let source = r#"fn main(): IO Unit = do
    let xs = [1, 2, 0, 4]
    println(par_map(fn (n) => 12 / n, xs))
"#;
    let compilation = aevia_core::compile("par.aev", source);
    let module = compilation.module.as_ref().expect("parsed");
    let mut interp = aevia_core::Interp::new(&compilation.checked).with_buffer();
    interp.set_jobs(4);
    assert!(!interp.load(module).has_errors());
    let error = interp.run_main().expect_err("division by zero");
    assert_eq!(error.code, "E0801");
    assert!(error.span.is_some());
}

#[test]
fn symbolic_values_fall_back_to_the_sequential_map() {
    // A symbolic term cannot cross threads; the map still runs.
    let source = r#"fn main(): IO Unit = do
    let x = symbol("x")
    let fs = par_map(fn (k) => k * x, [1, 2, 3])
    println(fs)
"#;
    let output = run_with_jobs(source, 4);
    assert!(output.contains('x'), "{output}");
}
