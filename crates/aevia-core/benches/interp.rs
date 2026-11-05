//! Offline interpreter benchmarks (round-2 performance work).
//!
//! Run with `cargo bench -p aevia-core --bench interp` (or
//! `cargo bench -p aevia-core --bench interp -- --quick` for one round).
//!
//! Protocol: **release-ish** build (`cargo bench` uses the `bench` profile, which
//! inherits `release`), one process, warm up once per workload, then report the
//! median of `ROUNDS` runs with min/max. The measured path is the Core IR
//! interpreter (`Mode::Ir`) — the production path; the AST evaluator is the
//! semantic reference and is intentionally slower. Numbers are comparable
//! between commits on the same machine only; the allocation counts printed by
//! `crates/aevia-core/tests/allocations.rs` are the portable gate.
//!
//! Workloads: a 10 000-step tail loop, `map` over 2 000 elements with an IR
//! closure, `fold` over the same tensor with a native function, dense `sum`, and
//! symbolic-term construction (100/200/400 operands).

use aevia_core::{compile, Interp, Mode};
use aevia_syntax::Span;
use std::time::{Duration, Instant};

const ROUNDS: usize = 7;

fn interpreter(source: &str) -> Interp {
    let compilation = compile("bench.aev", source);
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
    let module = compilation.module.expect("a parsed module");
    let mut interpreter = Interp::new(&compilation.checked)
        .with_buffer()
        .with_mode(Mode::Ir);
    let diagnostics = interpreter.load(&module);
    assert!(
        !diagnostics.has_errors(),
        "load errors: {:?}",
        diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    interpreter
}

fn median(mut samples: Vec<Duration>) -> (Duration, Duration, Duration) {
    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    (median, samples[0], samples[samples.len() - 1])
}

fn report(label: &str, samples: Vec<Duration>, units: f64) {
    let (median, min, max) = median(samples);
    let unit = median.as_secs_f64() / units;
    println!(
        "{label:<34} median {:>10.3} ms  (min {:>8.3}, max {:>8.3})  {unit:>10.3} µs/unit",
        median.as_secs_f64() * 1e3,
        min.as_secs_f64() * 1e3,
        max.as_secs_f64() * 1e3,
    );
}

fn main() {
    println!("aevia-core interpreter benchmarks (IR path, median of {ROUNDS})");

    // --- tail loop ---------------------------------------------------------
    const TAIL: &str = r#"fn count(n: Int, acc: Int): Int =
    if n == 0 then acc else count(n - 1, acc + 1)

fn main(): IO Unit = do
    println(count(10000, 0))
"#;
    let mut tail = interpreter(TAIL);
    let mut samples = Vec::with_capacity(ROUNDS);
    tail.run_main().expect("warm-up");
    for _ in 0..ROUNDS {
        let start = Instant::now();
        tail.run_main().expect("tail loop");
        samples.push(start.elapsed());
    }
    report("tail loop (10k steps)", samples, 10_000.0);

    // --- map / fold / sum over one 2 000-element tensor --------------------
    const HOT: &str = r#"let xs = zeros(2000)
let f = fn x => x * 2.0

fn main(): IO Unit = do
    println(show(xs))
"#;
    let mut hot = interpreter(HOT);
    let function = hot.eval_toplevel("f").expect("f");
    let xs = hot.eval_toplevel("xs").expect("xs");
    hot.call_named("map", vec![function.clone(), xs.clone()], Span::default())
        .expect("warm-up map");
    let mut samples = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let start = Instant::now();
        hot.call_named("map", vec![function.clone(), xs.clone()], Span::default())
            .expect("map");
        samples.push(start.elapsed());
    }
    report("map (2 000 elements)", samples, 2_000.0);

    let identity = aevia_core::value::Value::Native(std::rc::Rc::new(
        aevia_core::value::NativeFn(Box::new(
            |_interp: &mut Interp, args: Vec<aevia_core::value::Value>, _span: Span| {
                let (Some(aevia_core::value::Value::Num(a)), Some(aevia_core::value::Value::Num(b))) =
                    (args.first(), args.get(1))
                else {
                    return Ok(aevia_core::value::Value::float(0.0));
                };
                let a = a.to_f64().unwrap_or(0.0);
                let b = b.to_f64().unwrap_or(0.0);
                Ok(aevia_core::value::Value::Num(
                    aevia_core::value::NumVal::float(a + b, aevia_dim::Dim::one()),
                ))
            },
        )),
    ));
    let zero = aevia_core::value::Value::float(0.0);
    hot.call_named(
        "fold",
        vec![identity.clone(), zero.clone(), xs.clone()],
        Span::default(),
    )
    .expect("warm-up fold");
    let mut samples = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let start = Instant::now();
        hot.call_named(
            "fold",
            vec![identity.clone(), zero.clone(), xs.clone()],
            Span::default(),
        )
        .expect("fold");
        samples.push(start.elapsed());
    }
    report("fold (2 000 elements)", samples, 2_000.0);

    hot.call_named("sum", vec![xs.clone()], Span::default())
        .expect("warm-up sum");
    let mut samples = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let start = Instant::now();
        hot.call_named("sum", vec![xs.clone()], Span::default())
            .expect("sum");
        samples.push(start.elapsed());
    }
    report("dense sum (2 000 elements)", samples, 2_000.0);

    // --- dense axis reduction (512×512) -----------------------------------
    let mut grid = interpreter("let grid = zeros_of<Tensor<[512, 512], Float>>()\n");
    let grid_value = grid.eval_toplevel("grid").expect("grid");
    grid.call_named(
        "sum",
        vec![grid_value.clone(), aevia_core::value::Value::int(0)],
        Span::default(),
    )
    .expect("warm-up axis sum");
    let mut samples = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let start = Instant::now();
        grid.call_named(
            "sum",
            vec![grid_value.clone(), aevia_core::value::Value::int(0)],
            Span::default(),
        )
        .expect("axis sum");
        samples.push(start.elapsed());
    }
    report("dense sum(axis=0) (512×512)", samples, 512.0 * 512.0);

    // --- dense whole-tensor min (512×512) ---------------------------------
    grid.call_named("min_all", vec![grid_value.clone()], Span::default())
        .expect("warm-up min_all");
    let mut samples = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let start = Instant::now();
        grid.call_named("min_all", vec![grid_value.clone()], Span::default())
            .expect("min_all");
        samples.push(start.elapsed());
    }
    report("dense min_all (512×512)", samples, 512.0 * 512.0);

    // --- symbolic term construction ---------------------------------------
    for terms in [100usize, 200, 400] {
        let mut samples = Vec::with_capacity(ROUNDS);
        for _ in 0..ROUNDS {
            let start = Instant::now();
            let mut sum = aevia_core::sym::Term::int(0);
            for index in 0..terms {
                sum = aevia_core::sym::Term::add(
                    sum,
                    aevia_core::sym::Term::Sym(format!("x{index}")),
                );
            }
            let _ = sum;
            samples.push(start.elapsed());
        }
        report(&format!("symbolic build ({terms} terms)"), samples, terms as f64);
    }
}
