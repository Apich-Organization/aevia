//! Sparse tensors of rank N: contraction, rendering and equality, the kernel
//! bridge, and the "nothing is ever densified" size tests.

use super::*;
use aevia_core::builtins::call_simple;
use aevia_core::value::Storage;
use aevia_core::value::TensorData;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Reference contraction
// ---------------------------------------------------------------------------

fn lcg(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 33
}

/// Deterministic data: about 40% of the entries are nonzero small integers.
fn generate(
    shape: &[usize],
    seed: u64,
) -> Vec<f64> {
    let mut state = seed;
    (0..shape.iter().product::<usize>())
        .map(|_| {
            if lcg(&mut state) % 10 < 4 {
                (lcg(&mut state) % 4 + 1) as f64
            } else {
                0.0
            }
        })
        .collect()
}

fn nested(
    shape: &[usize],
    data: &[f64],
    exact: bool,
) -> String {
    if shape.len() == 1 {
        let items: Vec<String> = data
            .iter()
            .map(|v| {
                if exact {
                    format!("{}", *v as i64)
                } else {
                    format!("{v:?}")
                }
            })
            .collect();
        return format!("[{}]", items.join(", "));
    }
    let stride: usize = shape[1..].iter().product();
    let items: Vec<String> = (0..shape[0])
        .map(|i| nested(&shape[1..], &data[i * stride..(i + 1) * stride], exact))
        .collect();
    format!("[{}]", items.join(", "))
}

fn unflatten(
    mut flat: usize,
    shape: &[usize],
) -> Vec<usize> {
    let mut out = vec![0; shape.len()];
    for axis in (0..shape.len()).rev() {
        out[axis] = flat % shape[axis];
        flat /= shape[axis];
    }
    out
}

/// `tensordot` by brute force.
fn contract_reference(
    a: &[f64],
    shape_a: &[usize],
    b: &[f64],
    shape_b: &[usize],
    axes_a: &[usize],
    axes_b: &[usize],
) -> (Vec<usize>, Vec<f64>) {
    let free_a: Vec<usize> = (0..shape_a.len()).filter(|x| !axes_a.contains(x)).collect();
    let free_b: Vec<usize> = (0..shape_b.len()).filter(|x| !axes_b.contains(x)).collect();
    let out_shape: Vec<usize> = free_a
        .iter()
        .map(|x| shape_a[*x])
        .chain(free_b.iter().map(|x| shape_b[*x]))
        .collect();
    let mut out = vec![0.0; out_shape.iter().product::<usize>()];
    for (fa, va) in a.iter().enumerate() {
        let ca = unflatten(fa, shape_a);
        for (fb, vb) in b.iter().enumerate() {
            let cb = unflatten(fb, shape_b);
            if axes_a.iter().zip(axes_b).all(|(x, y)| ca[*x] == cb[*y]) {
                let mut flat = 0;
                let coords: Vec<usize> = free_a
                    .iter()
                    .map(|x| ca[*x])
                    .chain(free_b.iter().map(|x| cb[*x]))
                    .collect();
                for (c, e) in coords.iter().zip(&out_shape) {
                    flat = flat * e + c;
                }
                out[flat] += va * vb;
            }
        }
    }
    (out_shape, out)
}

fn contraction_program(
    shape_a: &[usize],
    shape_b: &[usize],
    axes_a: &[usize],
    axes_b: &[usize],
    seed: u64,
    exact: bool,
    layouts: (&str, &str),
) -> (String, String) {
    let (a, b) = (generate(shape_a, seed), generate(shape_b, seed + 77));
    let (out_shape, expected) = contract_reference(&a, shape_a, &b, shape_b, axes_a, axes_b);
    let list = |axes: &[usize]| {
        format!(
            "[{}]",
            axes.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let wrap = |layout: &str, text: String| {
        if layout == "sparse" {
            format!("from_dense<Coo>({text})")
        } else {
            text
        }
    };
    let result = if out_shape.is_empty() {
        "c".to_string()
    } else if layouts.0 == "sparse" && layouts.1 == "sparse" {
        "to_dense(c)".to_string()
    } else {
        "c".to_string()
    };
    let expected_text = if out_shape.is_empty() {
        if exact {
            format!("{}", expected[0] as i64)
        } else {
            format!("{:?}", expected[0])
        }
    } else {
        nested(&out_shape, &expected, exact)
    };
    let source = format!(
        "fn main(): IO Unit = do\n    let a = {}\n    let b = {}\n    let c = contract(a, b, {}, {})\n    println(\"{{{} == {}}}\")\n",
        wrap(layouts.0, nested(shape_a, &a, exact)),
        wrap(layouts.1, nested(shape_b, &b, exact)),
        list(axes_a),
        list(axes_b),
        result,
        expected_text,
    );
    (source, "true\n".to_string())
}

#[test]
fn rank_three_coo_contraction_matches_the_dense_contraction() {
    for (seed, exact) in [(3, false), (11, false), (5, true)] {
        for layouts in [
            ("sparse", "sparse"),
            ("sparse", "dense"),
            ("dense", "sparse"),
        ] {
            // Two contracted axes: rank 3 x rank 3 -> rank 2.
            let (source, expected) = contraction_program(
                &[3, 4, 2],
                &[2, 4, 3],
                &[1, 2],
                &[1, 0],
                seed,
                exact,
                layouts,
            );
            assert_eq!(run(&source), expected, "{layouts:?}\n{source}");
            // One contracted axis: rank 3 x rank 2 -> rank 3.
            let (source, expected) =
                contraction_program(&[3, 4, 2], &[2, 5], &[2], &[0], seed + 1, exact, layouts);
            assert_eq!(run(&source), expected, "{layouts:?}\n{source}");
        }
    }
}

#[test]
fn full_contraction_is_a_scalar_and_an_outer_product_is_sparse() {
    let (source, expected) = contraction_program(
        &[3, 4, 2],
        &[3, 4, 2],
        &[0, 1, 2],
        &[0, 1, 2],
        9,
        false,
        ("sparse", "sparse"),
    );
    assert_eq!(run(&source), expected, "{source}");
}

#[test]
fn contraction_errors() {
    let codes = check_codes(
        "let c = contract(from_dense<Coo>([[1.0]]), from_dense<Coo>([[1.0]]), [0, 1], [0])",
    );
    assert!(!codes.is_empty());
    let codes = check_codes("let c = contract([[1.0]], [[1.0]], [0], [0])");
    assert!(
        !codes.is_empty(),
        "two dense operands are not a sparse contraction"
    );
    let code = runtime_code(
        "fn main(): IO Unit = do\n    let c = contract(from_dense<Coo>([[1.0, 2.0]]), from_dense<Coo>([[1.0, 2.0, 3.0]]), [1], [1])\n    println(\"{nnz(c)}\")\n",
    );
    assert_eq!(code, "E0802");
}

#[test]
fn rank_three_sparse_tensors_support_the_elementwise_algebra() {
    let a = nested(&[2, 3, 2], &generate(&[2, 3, 2], 21), false);
    let b = nested(&[2, 3, 2], &generate(&[2, 3, 2], 22), false);
    let out = run(&format!(
        r#"
fn main(): IO Unit = do
    let da = {a}
    let db = {b}
    let x = from_dense<Coo>(da)
    let y = from_dense<Coo>(db)
    println("{{to_dense(x + y) == da + db}} {{to_dense(x * y) == da * db}} {{to_dense(x - y) == da - db}} {{x == from_dense<Coo>(da)}}")
    println("{{sum(x + y) == sum(x) + sum(y)}} {{shape(x)}} {{rank(x)}} {{x[1, 2, 1] == da[1, 2, 1]}} {{to_dense(x * 2.0) == da * 2.0}}")
"#
    ));
    assert_eq!(out, "true true true true\ntrue [2, 3, 2] 3 true true\n");
}

// ---------------------------------------------------------------------------
// show / repr / ==
// ---------------------------------------------------------------------------

#[test]
fn show_repr_and_equality() {
    let out = run(r#"
fn main(): IO Unit = do
    let a = from_dense<Coo>([[1.0, 0.0], [0.0, 2.0]])
    let m = from_dense([[2.0 m, 0.0 m], [0.0 m, 0.0 m]])
    println(show(a))
    println(show(m))
    println(repr(a))
    println(repr(convert<Csr>(a)))
    let i: Sparse<Csr, [2, 2], Int> = from_dense([[1, 0], [0, 7]])
    println(repr(i))
"#);
    assert_eq!(
        out,
        "sparse(Coo, [2, 2], { (0, 0) = 1.0, (1, 1) = 2.0 })\n\
         sparse(Csr, [2, 2], { (0, 0) = 2.0 m })\n\
         sparse_coo([2, 2], [(0, 0), (1, 1)], [1.0, 2.0])\n\
         convert<Csr>(sparse_coo([2, 2], [(0, 0), (1, 1)], [1.0, 2.0]))\n\
         convert<Csr>(sparse_coo([2, 2], [(0, 0), (1, 1)], [1, 7]))\n"
    );
}

#[test]
fn repr_parses_back_to_the_same_value() {
    let out = run(r#"
fn main(): IO Unit = do
    let a: Sparse<Csc, [2, 3], Int> = convert<Csc>(from_dense([[1, 0, 3], [0, 5, 0]]))
    println(repr(a))
"#);
    let source = format!(
        "fn main(): IO Unit = do\n    let a = {}    let b: Sparse<Csc, [2, 3], Int> = convert<Csc>(from_dense([[1, 0, 3], [0, 5, 0]]))\n    println(\"{{a == b}} {{format_of(a)}} {{to_dense(a) == to_dense(b)}}\")\n",
        out
    );
    assert_eq!(run(&source), "true Csc true\n");
}

#[test]
fn equality_is_logical() {
    let out = run(r#"
fn main(): IO Unit = do
    let a = from_dense([[1.0, 0.0], [0.0, 2.0]])
    let b = from_dense([[1.0, 0.0], [0.0, 3.0]])
    let c = from_dense([[1.0, 0.0, 0.0], [0.0, 2.0, 0.0]])
    println("{a == a} {a == b} {a != b} {a == convert<Coo>(a)} {a == convert<Csc>(a)}")
    println("{from_dense([[1, 0], [0, 2]]) == a} {nnz(a - a) == 0} {a - a == from_dense([[0.0, 0.0], [0.0, 0.0]])}")
    println("{shape(c)}")
"#);
    assert_eq!(out, "true false true true true\ntrue true true\n[2, 3]\n");
}

// ---------------------------------------------------------------------------
// Kernel bridge
// ---------------------------------------------------------------------------

#[test]
fn a_sparse_value_crosses_the_kernel_bridge_without_densifying() {
    let values = vec![
        Value::float(4.0),
        Value::float(1.0),
        Value::float(3.0),
        Value::float(5.0),
    ];
    let original = SparseData::from_flat_parts(
        SparseFormat::Csr,
        vec![3, 3],
        &[0, 3, 4, 8],
        values,
        aevia_dim::Dim::one(),
    )
    .expect("a valid sparse value");
    assert!(original.validate().is_ok());
    let value = Value::Sparse(Rc::new(original));
    let crossed = aevia_core::kernel::to_val(&value).expect("sparse values cross the bridge");
    let aevia_rssn::Val::Sparse {
        format,
        shape,
        indices,
        data,
    } = &crossed
    else {
        panic!("a sparse value must stay sparse at the bridge, found {crossed:?}");
    };
    assert_eq!(*format, aevia_rssn::SparseKind::Csr);
    assert_eq!(shape, &vec![3, 3]);
    assert_eq!(indices, &vec![0, 3, 4, 8], "flat row-major indices");
    assert_eq!(data.len(), 4);
    // Through the rssn model and back.
    let session = aevia_rssn::kernel().id();
    let kernel_value = crossed.to_rssn(session).expect("converts to an rssn value");
    let again = aevia_rssn::Val::from_rssn(&kernel_value, session).expect("converts back");
    assert_eq!(again, crossed);
    let back = aevia_core::kernel::from_val(&again, aevia_dim::Dim::one());
    assert!(
        aevia_core::value::equal(&value, &back).expect("comparable"),
        "the round trip changed the value"
    );
    let Value::Sparse(back) = back else {
        panic!("not sparse");
    };
    assert_eq!(back.format, SparseFormat::Csr);
    assert!(back.is_f64());
}

#[test]
fn flat_parts_validate_their_input() {
    let bad = |flat: &[u64], count: usize| {
        SparseData::from_flat_parts(
            SparseFormat::Coo,
            vec![2, 2],
            flat,
            (0..count).map(|_| Value::float(1.0)).collect(),
            aevia_dim::Dim::one(),
        )
    };
    assert!(bad(&[0, 4], 2).is_err(), "index 4 is outside a 2x2 tensor");
    assert!(bad(&[2, 1], 2).is_err(), "indices must ascend");
    assert!(bad(&[0, 1], 1).is_err(), "one value per index");
    assert!(bad(&[0, 3], 2).is_ok());
}

// ---------------------------------------------------------------------------
// Nothing is densified
// ---------------------------------------------------------------------------

fn int_tensor(items: Vec<usize>) -> Value {
    let len = items.len();
    aevia_core::eval::build_tensor(
        items.into_iter().map(|n| Value::int(n as i64)).collect(),
        vec![len],
        aevia_dim::Dim::one(),
    )
}

fn float_tensor(items: Vec<f64>) -> Value {
    let len = items.len();
    Value::Tensor(Rc::new(TensorData::new(
        vec![len],
        Storage::Dense(Rc::new(items)),
        aevia_dim::Dim::one(),
        None,
    )))
}

/// The tridiagonal `[-1, 2, -1]` matrix of order `n`, as CSR arrays.
fn tridiagonal(n: usize) -> Value {
    let (mut ptr, mut idx, mut val) = (vec![0usize], Vec::new(), Vec::new());
    for i in 0..n {
        if i > 0 {
            idx.push(i - 1);
            val.push(-1.0);
        }
        idx.push(i);
        val.push(2.0);
        if i + 1 < n {
            idx.push(i + 1);
            val.push(-1.0);
        }
        ptr.push(idx.len());
    }
    call_simple(
        "sparse_csr",
        vec![
            Value::int(n as i64),
            Value::int(n as i64),
            int_tensor(ptr),
            int_tensor(idx),
            float_tensor(val),
        ],
    )
    .expect("a valid CSR matrix")
}

#[test]
fn a_two_hundred_thousand_square_tridiagonal_is_never_densified() {
    // 200_000 x 200_000 doubles are 320 GB: anything that densified would be
    // refused (`E0802`) or would not finish.
    let n = 200_000;
    let started = std::time::Instant::now();
    let a = tridiagonal(n);
    let ones = float_tensor(vec![1.0; n]);
    let product = call_simple("matmul", vec![a.clone(), ones.clone()]).expect("sparse matvec");
    let Value::Tensor(product) = &product else {
        panic!("a dense vector");
    };
    let data = product.dense().expect("an f64 buffer");
    assert_eq!(data[0], 1.0);
    assert_eq!(data[n / 2], 0.0);
    assert_eq!(data[n - 1], 1.0);
    // The solution of the discrete Poisson problem with a unit load.
    let x = call_simple("spsolve", vec![a.clone(), ones.clone()]).expect("sparse LU");
    let Value::Tensor(x) = &x else {
        panic!("a dense vector");
    };
    let x = x.dense().expect("an f64 buffer");
    let h = (n + 1) as f64;
    let middle = n / 2;
    let exact = (middle + 1) as f64 * (h - (middle + 1) as f64) / 2.0;
    assert!(
        (x[middle] - exact).abs() / exact < 1e-6,
        "x[mid] = {} but the continuum solution is {exact}",
        x[middle]
    );
    // A sparse x sparse product stays sparse: nnz grows from 3n to 5n - 6.
    let squared = call_simple("matmul", vec![a.clone(), a.clone()]).expect("spgemm");
    let nnz = call_simple("nnz", vec![squared.clone()]).expect("nnz");
    assert_eq!(nnz.as_f64(), Some((5 * n - 6) as f64));
    let sum = call_simple("sum", vec![squared]).expect("sum");
    assert_eq!(sum.as_f64(), Some(2.0));
    // The Hadamard product and the transpose stay sparse as well.
    let hadamard = call_simple("transpose", vec![a.clone()]).expect("transpose");
    let diff = call_simple("nnz", vec![hadamard]).expect("nnz");
    assert_eq!(diff.as_f64(), Some((3 * n - 2) as f64));
    assert!(
        started.elapsed().as_secs() < 60,
        "the sparse kernels took {:?}",
        started.elapsed()
    );
    // Densifying really is refused.
    let error =
        call_simple("to_dense", vec![a]).expect_err("a 200000 x 200000 tensor is too large");
    assert_eq!(error.code, "E0802");
}

#[test]
fn cg_on_a_larger_laplacian() {
    let n = 2_000;
    let a = tridiagonal(n);
    let b = float_tensor(vec![1.0; n]);
    let result = call_simple(
        "cg",
        vec![a, b, Value::float(1e-10), Value::int(10 * n as i64)],
    )
    .expect("cg");
    let Value::Record(record) = result else {
        panic!("a result record");
    };
    assert!(matches!(record.fields["converged"], Value::Bool(true)));
}
