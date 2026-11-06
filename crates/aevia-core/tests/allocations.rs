//! Allocation gate for the interpreter's hot paths (round-2 performance work).
//!
//! A custom global allocator counts `GlobalAlloc::alloc` calls; the measured
//! workload is run warm (compilation and module loading are outside the
//! measurement) and the count is divided by the number of elements/steps. The
//! assertions are the *deterministic* part of the performance work: wall clock
//! is noisy on a shared machine, an allocation count is not.
//!
//! One test function only: the counter is process-global, so the measurements
//! must not run concurrently with each other.

// The package lint denies `unsafe`; a `#[global_allocator]` is necessarily an
// unsafe impl. This file is the single exception, and every method forwards to
// `System` unchanged except for the counter.
#![allow(unsafe_code)]

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::NumVal;
use aevia_core::value::Value;
use aevia_syntax::Span;
use std::alloc::GlobalAlloc;
use std::alloc::Layout;
use std::alloc::System;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every method forwards to `System`, which is a valid allocator; the
// only addition is a relaxed atomic increment.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(
        &self,
        layout: Layout,
    ) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(
        &self,
        pointer: *mut u8,
        layout: Layout,
    ) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(
        &self,
        pointer: *mut u8,
        layout: Layout,
        new_size: usize,
    ) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Runs `body` and returns how many allocations it made.
fn allocations<T>(body: impl FnOnce() -> T) -> (usize, T) {
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let value = body();
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    (after - before, value)
}

fn interpreter(source: &str) -> Interp {
    let compilation = compile("allocations.aev", source);
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

const TAIL: &str = r#"fn count(n: Int, acc: Int): Int =
    if n == 0 then acc else count(n - 1, acc + 1)

fn main(): IO Unit = do
    println(count(1000, 0))
"#;

const HOT: &str = r#"let xs = zeros(2000)
let f = fn x => x * 2.0

fn main(): IO Unit = do
    println(show(xs))
"#;

fn main_program(interp: &mut Interp) {
    interp.run_main().expect("main runs");
}

/// The gate: one measurement per hot path, with the count reported so a
/// regression is visible as a number rather than as a slow test.
#[test]
fn the_hot_paths_allocate_within_their_budget() {
    // --- tail loop: a trampoline step should not allocate per step ----------
    let mut tail = interpreter(TAIL);
    main_program(&mut tail); // warm up
    let (tail_allocations, _) = allocations(|| main_program(&mut tail));
    let tail_per_step = tail_allocations as f64 / 1_000.0;
    eprintln!("allocations: tail loop {tail_allocations} total, {tail_per_step:.3}/step");
    // One `Vec<Value>` per tail call (the evaluated argument list) is the only
    // remaining per-step allocation; the frame buffer itself is reused. The
    // gate pins that: a regression that clones frames or names per step shows up
    // immediately.
    assert!(
        tail_per_step < 1.05,
        "a tail step allocates only its argument list (was {tail_per_step:.3})"
    );

    // --- map: a closure call per element ------------------------------------
    let mut hot = interpreter(HOT);
    let function = hot.eval_toplevel("f").expect("f");
    let xs = hot.eval_toplevel("xs").expect("xs");
    hot.call_named("map", vec![function.clone(), xs.clone()], Span::default())
        .expect("warm-up map");
    let (map_allocations, mapped) = allocations(|| {
        hot.call_named("map", vec![function.clone(), xs.clone()], Span::default())
            .expect("map")
    });
    let elements = match &mapped {
        | Value::Tensor(tensor) => tensor.storage.len(),
        | other => panic!("map answers a tensor, found {}", other.type_name()),
    };
    let map_per_element = map_allocations as f64 / elements as f64;
    eprintln!(
        "allocations: map {map_allocations} for {elements} elements, {map_per_element:.3}/element"
    );
    assert!(
        map_per_element < 12.0,
        "map of an IR closure must stay near a few allocations per element (was {map_per_element:.3})"
    );

    // --- fold: the same closure call, with an accumulator -------------------
    let identity = Value::Native(std::rc::Rc::new(aevia_core::value::NativeFn(Box::new(
        |_interp: &mut Interp, args: Vec<Value>, _span: Span| {
            let (Some(Value::Num(a)), Some(Value::Num(b))) = (args.first(), args.get(1)) else {
                return Ok(Value::float(0.0));
            };
            let a = a.to_f64().unwrap_or(0.0);
            let b = b.to_f64().unwrap_or(0.0);
            Ok(Value::Num(NumVal::float(a + b, aevia_dim::Dim::one())))
        },
    ))));
    let zero = Value::Num(NumVal::float(0.0, aevia_dim::Dim::one()));
    hot.call_named(
        "fold",
        vec![identity.clone(), zero.clone(), xs.clone()],
        Span::default(),
    )
    .expect("warm-up fold");
    let (fold_allocations, _) = allocations(|| {
        hot.call_named(
            "fold",
            vec![identity.clone(), zero.clone(), xs.clone()],
            Span::default(),
        )
        .expect("fold")
    });
    let fold_per_element = fold_allocations as f64 / elements as f64;
    eprintln!(
        "allocations: fold {fold_allocations} for {elements} elements, {fold_per_element:.3}/element"
    );
    assert!(
        fold_per_element < 2.0,
        "fold of a native function should not allocate per element (was {fold_per_element:.3})"
    );

    // --- dense sum: one buffer walk, no boxing ------------------------------
    hot.call_named("sum", vec![xs.clone()], Span::default())
        .expect("warm-up sum");
    let (sum_allocations, _) = allocations(|| {
        hot.call_named("sum", vec![xs], Span::default())
            .expect("sum")
    });
    eprintln!("allocations: dense sum {sum_allocations} for {elements} elements");
    assert!(
        sum_allocations < 16,
        "a dense sum is O(1) allocations, found {sum_allocations}"
    );

    // --- dense axis reduction: allocations scale with the output, not input --
    //
    // 512×512 summed along axis 0 produces 512 outputs; the fast path walks the
    // buffer with no `Value` per input element, so the count is O(output).
    let grid_source = "let grid = zeros_of<Tensor<[512, 512], Float>>()\n";
    let mut grid_interp = interpreter(grid_source);
    let grid = grid_interp.eval_toplevel("grid").expect("grid");
    grid_interp
        .call_named("sum", vec![grid.clone(), Value::int(0)], Span::default())
        .expect("warm-up axis sum");
    let (axis_allocations, reduced) = allocations(|| {
        grid_interp
            .call_named("sum", vec![grid, Value::int(0)], Span::default())
            .expect("axis sum")
    });
    let Value::Tensor(reduced) = &reduced else {
        panic!("axis sum answers a tensor");
    };
    assert_eq!(reduced.shape, vec![1, 512]);
    eprintln!(
        "allocations: dense sum(axis=0) {axis_allocations} for 512 outputs over 262144 elements"
    );
    assert!(
        axis_allocations < 256,
        "the dense axis path allocates per output, not per element (found {axis_allocations})"
    );

    // The old axis path (still the one a non-`f64` class takes) boxes a `Value`
    // per input element: measure it on the same 512×512 data as the "before".
    let boxed = Value::Tensor(std::rc::Rc::new(aevia_core::value::TensorData::new(
        vec![512, 512],
        aevia_core::value::Storage::Boxed((0..512 * 512).map(|_| Value::float(0.5)).collect()),
        aevia_dim::Dim::one(),
        None,
    )));
    let (boxed_allocations, _) = allocations(|| {
        grid_interp
            .call_named("sum", vec![boxed, Value::int(0)], Span::default())
            .expect("boxed axis sum")
    });
    eprintln!(
        "allocations: boxed sum(axis=0) {boxed_allocations} for 262144 elements (before-proxy)"
    );
    assert!(
        axis_allocations * 1_000 < boxed_allocations,
        "the dense path must be orders of magnitude below per-element boxing: \
         {axis_allocations} vs {boxed_allocations}"
    );

    // --- dense whole-tensor min/max: one scan, no per-element boxing --------
    let grid_again = grid_interp.eval_toplevel("grid").expect("grid");
    let (min_allocations, _) = allocations(|| {
        grid_interp
            .call_named("min_all", vec![grid_again], Span::default())
            .expect("min_all")
    });
    eprintln!("allocations: dense min_all {min_allocations} over 262144 elements");
    assert!(
        min_allocations <= 2,
        "a dense min_all is O(1) allocations, found {min_allocations}"
    );
    let boxed_min = Value::Tensor(std::rc::Rc::new(aevia_core::value::TensorData::new(
        vec![512, 512],
        aevia_core::value::Storage::Boxed((0..512 * 512).map(|_| Value::float(0.5)).collect()),
        aevia_dim::Dim::one(),
        None,
    )));
    let (boxed_min_allocations, _) = allocations(|| {
        grid_interp
            .call_named("min_all", vec![boxed_min], Span::default())
            .expect("boxed min_all")
    });
    eprintln!(
        "allocations: boxed min_all {boxed_min_allocations} over 262144 elements (before-proxy)"
    );
    // The element-wise path still materialises a `Value` per element (a large
    // transient allocation); the fast path scans the buffer and allocates
    // nothing per element. The count difference is small because `Value` f64
    // elements are inline — the win is the scan, shown by the timing in
    // `tests/reductions_minmax.rs` — but the fast path must not grow with n.
    assert!(
        boxed_min_allocations > min_allocations,
        "the dense min_all should allocate less than the element-wise path: \
         {min_allocations} vs {boxed_min_allocations}"
    );

    // --- symbolic term construction: linear, not quadratic ------------------
    //
    // `Term::add` groups like terms. Keying them by `format!("{term:?}")` cost
    // one `String` per operand and a linear scan per group, so building an
    // n-operand sum allocated O(n²) times. The structural-hash key is alloc-free,
    // so the allocation count must grow roughly with n.
    let build = |terms: usize| -> usize {
        let (allocs, _) = allocations(|| {
            let mut sum = aevia_core::sym::Term::int(0);
            for index in 0..terms {
                sum = aevia_core::sym::Term::add(
                    sum,
                    aevia_core::sym::Term::Sym(format!("x{index}")),
                );
            }
        });
        allocs
    };
    let small = build(100);
    let medium = build(200);
    let large = build(400);
    eprintln!("allocations: symbolic build 100={small} 200={medium} 400={large}");
    assert!(
        large <= small * 8,
        "symbolic construction allocations grew super-linearly: 100 terms {small}, 400 terms {large}"
    );
}
