//! Allocation gate for the interpreter's hot paths (round-2 performance work).
//!
//! A custom global allocator counts allocation and reallocation requests and
//! their requested sizes. Compilation, module loading and warm-up take place
//! outside the measurement. The budgets use counts for call overhead and
//! bytes for reductions where a single request could copy the whole input.
//!
//! One test function only: the counter is process-global, so the measurements
//! must not run concurrently with each other.
#![allow(clippy::result_large_err)]
// test helpers return the interpreter's own error type

// The package lint denies `unsafe`; a `#[global_allocator]` is necessarily an
// unsafe impl. This file is the single exception, and every method forwards to
// `System` unchanged except for the counter.
#![allow(unsafe_code)]

use aevia_core::Interp;
use aevia_core::JitPolicy;
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
static REQUESTED_BYTES: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every method forwards to `System`, which is a valid allocator; the
// only additions record requests and sizes with relaxed atomics.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(
        &self,
        layout: Layout,
    ) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        REQUESTED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        // SAFETY: The caller supplies the layout required by GlobalAlloc.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(
        &self,
        pointer: *mut u8,
        layout: Layout,
    ) {
        // SAFETY: The caller supplies a live allocation with its original layout.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(
        &self,
        pointer: *mut u8,
        layout: Layout,
        new_size: usize,
    ) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        REQUESTED_BYTES.fetch_add(new_size, Ordering::Relaxed);
        // SAFETY: The caller provides the allocation, old layout and valid new size.
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

struct AllocationTotals {
    calls: usize,
    bytes: usize,
}

fn allocation_totals<T>(body: impl FnOnce() -> T) -> (AllocationTotals, T) {
    let calls = ALLOCATIONS.load(Ordering::Relaxed);
    let bytes = REQUESTED_BYTES.load(Ordering::Relaxed);
    let value = body();
    (
        AllocationTotals {
            calls: ALLOCATIONS.load(Ordering::Relaxed).wrapping_sub(calls),
            bytes: REQUESTED_BYTES.load(Ordering::Relaxed).wrapping_sub(bytes),
        },
        value,
    )
}

fn allocations<T>(body: impl FnOnce() -> T) -> (usize, T) {
    let (totals, value) = allocation_totals(body);
    (totals.calls, value)
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

fn run_count(n: Int): Int = count(n, 0)
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
    // The trampoline reuses its frame and argument buffers. Warm the automatic
    // policy beyond the compilation threshold so the native compiler's one-time
    // work stays outside the steady-state allocation measurement.
    for (policy, label, warmup_runs) in [
        (JitPolicy::Annotated, "interpreted", 1),
        #[cfg(feature = "jit")]
        (JitPolicy::Automatic, "automatic JIT", 3),
    ] {
        let mut tail = interpreter(TAIL);
        tail.jit_policy = policy;
        let function = tail.eval_toplevel("run_count").expect("run_count exists");
        let mut run_count = || {
            tail.apply(function.clone(), vec![Value::int(1_000)], Span::default())
                .expect("tail loop runs")
        };
        for _ in 0..warmup_runs {
            assert_eq!(run_count().as_f64(), Some(1_000.0));
        }
        let (tail_allocations, result) = allocations(run_count);
        assert_eq!(result.as_f64(), Some(1_000.0));
        let tail_per_step = tail_allocations as f64 / 1_000.0;
        eprintln!(
            "allocations: {label} tail loop {tail_allocations} total, {tail_per_step:.3}/step"
        );
        assert!(
            tail_per_step < 1.05,
            "the {label} tail loop exceeded its argument allocation budget ({tail_per_step:.3}/step)"
        );
    }

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

    // --- dense axis reduction: allocate in proportion to the output --------
    const SIDE: usize = 512;
    const ELEMENTS: usize = SIDE * SIDE;
    const OUTPUT_BYTES: usize = SIDE * size_of::<f64>();
    let mut grid_interp = interpreter("let grid = zeros_of<Tensor<[512, 512], Float>>()\n");
    let dense = Value::Tensor(std::rc::Rc::new(aevia_core::value::TensorData::new(
        vec![SIDE, SIDE],
        aevia_core::value::Storage::Dense(std::rc::Rc::new(vec![0.5; ELEMENTS])),
        aevia_dim::Dim::one(),
        None,
    )));
    // Construct boxed storage explicitly to bypass automatic Float packing.
    let boxed = Value::Tensor(std::rc::Rc::new(aevia_core::value::TensorData::new(
        vec![SIDE, SIDE],
        aevia_core::value::Storage::Boxed((0..ELEMENTS).map(|_| Value::float(0.5)).collect()),
        aevia_dim::Dim::one(),
        None,
    )));
    let mut sum_axis = |input| {
        grid_interp
            .call_named("sum", vec![input, Value::int(0)], Span::default())
            .expect("axis sum")
    };
    sum_axis(dense.clone());
    sum_axis(boxed.clone());
    let (dense_totals, dense_result) = allocation_totals(|| sum_axis(dense));
    let (boxed_totals, boxed_result) = allocation_totals(|| sum_axis(boxed));
    for result in [dense_result, boxed_result] {
        let Value::Tensor(tensor) = result else {
            panic!("axis sum answers a tensor");
        };
        assert_eq!(tensor.shape, vec![1, SIDE]);
        let elements = tensor.elements();
        assert_eq!(elements.len(), SIDE);
        assert!(
            elements.iter().all(|value| value.as_f64() == Some(256.0)),
            "axis sum must add 512 copies of 0.5 in each column"
        );
    }
    eprintln!(
        "allocations: sum(axis=0) dense {} calls/{} bytes and boxed {} calls/{} bytes for {SIDE} outputs over {ELEMENTS} elements",
        dense_totals.calls, dense_totals.bytes, boxed_totals.calls, boxed_totals.bytes
    );
    // A single materialization of the entire input would pass a call-count
    // budget. The output-size byte budget also catches that regression.
    assert!(
        dense_totals.bytes <= 4 * OUTPUT_BYTES,
        "dense axis reduction must allocate in proportion to its {OUTPUT_BYTES}-byte output (requested {} bytes)",
        dense_totals.bytes
    );
    assert!(
        dense_totals.calls < 32,
        "dense axis reduction must use a few allocation requests (found {})",
        dense_totals.calls
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

    // --- checked constructors: a cons cell is O(1), not O(tail) ------------
    //
    // §9.2.3 trust boundary: a checked constructor application attaches its
    // type witness without walking the payload. Re-proving the payload made
    // `Cons(n, acc)` linear in `acc` — and allocating, since a witness tree was
    // built per call — so building a list was quadratic.
    let cons = |n: usize| -> usize {
        let source = format!(
            "type L = | Nil | Cons(Int, L)
fn mk(n: Int, acc: L): L = if n == 0 then acc else mk(n - 1, Cons(n, acc))
fn head(l: L): Int = match l with | Nil -> 0 | Cons(h, _) -> h
fn main(): IO Unit = do
    println(head(mk({n}, Nil)))
"
        );
        let mut interp = interpreter(&source);
        main_program(&mut interp);
        let (allocs, _) = allocations(|| main_program(&mut interp));
        allocs
    };
    let short_list = cons(500);
    let long_list = cons(2000);
    eprintln!("allocations: cons list 500={short_list} 2000={long_list}");
    assert!(
        long_list <= short_list * 6,
        "building a list must be linear: 500 cells {short_list}, 2000 cells {long_list}"
    );
    let per_cell = long_list as f64 / 2000.0;
    assert!(
        per_cell < 8.0,
        "a cons cell and its tail call should cost a few allocations (was {per_cell:.2})"
    );

    // --- a dimensioned record step: literals and dimensions are shared ------
    //
    // Units are compile-time scales (§0.4): `9.81 m/s^2` is resolved once per
    // site, and a dimension is a shared value whose products are memoized.
    let mut record = interpreter(
        "type P = { pos: Float[L], vel: Float[L/T] }
fn step(p: P, dt: Float[T]): P =
    { p with pos = p.pos + p.vel * dt, vel = p.vel - 9.81 m/s^2 * dt }
fn run(p: P, n: Int): P = if n == 0 then p else run(step(p, 0.001 s), n - 1)
fn main(): IO Unit = do
    println(run({ pos = 0.0 m, vel = 1.0 m/s }, 1000).pos)
",
    );
    main_program(&mut record);
    let (record_allocations, _) = allocations(|| main_program(&mut record));
    let per_step = record_allocations as f64 / 1000.0;
    eprintln!("allocations: dimensioned record step {per_step:.2}/step");
    assert!(
        per_step < 40.0,
        "a dimensioned record step must not rebuild units or dimension maps (was {per_step:.2})"
    );
}
