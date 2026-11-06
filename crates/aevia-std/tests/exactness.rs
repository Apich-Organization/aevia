//! Exactness survives the `f64` backend.
//!
//! The `faer` kernels take `&[f64]`, so an exact `Real` matrix cannot reach
//! them — but "cannot" is a claim about the call graph, and a claim about the
//! call graph is worth exactly one test. This is that test: it runs an Aevia
//! program whose operands are exact, and asserts on the *storage phase* of each
//! result element, never on anything rendered.
//!
//! `matmul` (`@`), `det` and `inv` are the operations whose `f64` backend was
//! moved, and each is checked twice: once with integer operands, where the
//! answer must stay `Int`, and once with rational operands, where it must stay
//! `Exact`. A `Float` anywhere in a result is the bug this file exists to
//! catch.

use aevia_core::Interp;
use aevia_core::compile;
use aevia_core::value::NumKind;
use aevia_core::value::Value;
use num_bigint::BigInt;
use num_rational::BigRational;

/// Compile and load a program, failing on any diagnostic.
fn session(source: &str) -> Interp {
    let compilation = compile("exactness.aev", source);
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
    let mut interp = Interp::new(&compilation.checked).with_buffer();
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
    interp
}

/// Every element of a tensor, in row-major order.
fn elements(value: &Value) -> Vec<Value> {
    let Value::Tensor(tensor) = value else {
        panic!("expected a tensor, found {}", value.type_name());
    };
    (0..tensor.storage.len())
        .map(|index| tensor.get_flat(index).expect("element"))
        .collect()
}

/// The storage phase of a scalar: `"Int"`, `"Exact"` or `"Float"`.
///
/// `NumKind::type_name` answers the *language* type (`Int` or `Real`), which
/// cannot tell an exact rational from a float; the variant can.
fn phase(value: &Value) -> &'static str {
    match &value
        .as_num()
        .unwrap_or_else(|| panic!("expected a number, found {}", value.type_name()))
        .kind
    {
        | NumKind::Int(_) => "Int",
        | NumKind::Exact(_) => "Exact",
        | NumKind::Float(_) => "Float",
        | NumKind::Sym(_) => "Sym",
        | NumKind::Complex(..) => "Complex",
    }
}

fn phases(value: &Value) -> Vec<&'static str> {
    elements(value).iter().map(phase).collect()
}

fn as_rational(value: &Value) -> BigRational {
    value
        .as_num()
        .and_then(|number| number.to_rational())
        .unwrap_or_else(|| panic!("expected an exact number, found {}", value.type_name()))
}

fn integer(value: i64) -> BigRational {
    BigRational::from_integer(BigInt::from(value))
}

fn rational(
    numerator: i64,
    denominator: i64,
) -> BigRational {
    BigRational::new(BigInt::from(numerator), BigInt::from(denominator))
}

/// The payload of an `Ok(…)`, which is what `inv` answers with.
///
/// `inv` returns `Result<Matrix, LinAlgError>`, so the matrix is one
/// constructor deep; taking it out here keeps the assertions about the matrix.
fn ok_payload(value: &Value) -> Value {
    match value {
        | Value::Variant(variant) if variant.name == "Ok" => {
            variant
                .fields
                .first()
                .cloned()
                .unwrap_or_else(|| panic!("`Ok` without a payload"))
        },
        | other => panic!("expected `Ok(…)`, found {}", other.type_name()),
    }
}

#[test]
fn an_exact_matmul_is_still_exact() {
    let mut interp = session(
        "let a = [[1, 2], [3, 4]]\n\
         let b = [[5, 6], [7, 8]]\n\
         let p = a @ b\n\
         let h = [[0.5, 0], [0, 0.5]]\n\
         let q = a @ h\n",
    );

    // Integers in, exact values out — the product of two exact matrices is
    // not a place an `f64` kernel may appear. Int multiplication also keeps
    // the element representation promised by its checked type.
    let p = interp.eval_toplevel("p").expect("p");
    assert!(
        phases(&p).iter().all(|phase| *phase == "Int"),
        "an exact matmul produced {:?}",
        phases(&p)
    );
    let values: Vec<BigRational> = elements(&p).iter().map(as_rational).collect();
    assert_eq!(values, [integer(19), integer(22), integer(43), integer(50)]);

    // Rationals in, rationals out: `0.5` is the exact decimal `1/2`, so the
    // product is exact and must not be a float that merely rounds to it.
    let q = interp.eval_toplevel("q").expect("q");
    assert_eq!(phases(&q), ["Exact", "Exact", "Exact", "Exact"]);
    let exact: Vec<BigRational> = elements(&q).iter().map(as_rational).collect();
    assert_eq!(
        exact,
        [rational(1, 2), integer(1), rational(3, 2), integer(2),]
    );
}

#[test]
fn an_exact_determinant_and_inverse_are_still_exact() {
    let mut interp = session(
        "let s = det([[1, 2], [2, 4]])\n\
         let d = det([[1, 2], [3, 4]])\n\
         let i = inv([[1, 2], [3, 4]])\n",
    );

    // A singular exact matrix has determinant exactly `0` — not `-0.0`, which
    // is what an `f64` elimination produces and what §6.6.5 forbids.
    let singular = interp.eval_toplevel("s").expect("s");
    assert_eq!(phase(&singular), "Int");
    assert_eq!(as_rational(&singular), integer(0));

    let determinant = interp.eval_toplevel("d").expect("d");
    assert_eq!(phase(&determinant), "Int");
    assert_eq!(as_rational(&determinant), integer(-2));

    let inverse = ok_payload(&interp.eval_toplevel("i").expect("i"));
    assert_eq!(phases(&inverse), ["Exact", "Exact", "Exact", "Exact"]);
    let exact: Vec<BigRational> = elements(&inverse).iter().map(as_rational).collect();
    assert_eq!(
        exact,
        [integer(-2), integer(1), rational(3, 2), rational(-1, 2),]
    );
}
