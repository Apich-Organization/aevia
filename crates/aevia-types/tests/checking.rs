//! End-to-end checks of the type and dimension system against the examples in
//! the specification.
//!
//! Sources are written as raw strings so their indentation is exactly what the
//! layout algorithm sees.

use aevia_syntax::{lex, parse_module};
use aevia_types::check::check_module;
use aevia_types::{CheckedModule, Export, Shape, Type};

fn check(source: &str) -> (Vec<String>, CheckedModule) {
    let lexed = lex(0, source).expect("the source lexes");
    let (module, parse_diagnostics) = parse_module(0, source, &lexed);
    assert!(
        !parse_diagnostics.has_errors(),
        "unexpected parse errors: {:?}",
        parse_diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    let result = check_module(0, &module);
    let codes = result
        .diagnostics
        .items
        .iter()
        .filter(|item| item.severity == aevia_syntax::Severity::Error)
        .map(|item| item.code.clone())
        .collect();
    (codes, result.module)
}

/// Checks a source that may be rejected by the parser as well as the checker.
///
/// A duplicate record field is `E0208` in both, so the code the user sees is
/// what the test asserts.
fn check_any(source: &str) -> Vec<String> {
    let lexed = lex(0, source).expect("the source lexes");
    let (module, parse_diagnostics) = parse_module(0, source, &lexed);
    let mut codes: Vec<String> = parse_diagnostics
        .items
        .iter()
        .filter(|item| item.severity == aevia_syntax::Severity::Error)
        .map(|item| item.code.clone())
        .collect();
    if !parse_diagnostics.has_errors() {
        let result = check_module(0, &module);
        codes.extend(
            result
                .diagnostics
                .items
                .iter()
                .filter(|item| item.severity == aevia_syntax::Severity::Error)
                .map(|item| item.code.clone()),
        );
    }
    codes
}

fn expect_ok(source: &str) -> CheckedModule {
    let (codes, module) = check(source);
    assert!(codes.is_empty(), "unexpected errors: {codes:?}");
    module
}

fn expect_error(source: &str, code: &str) {
    let (codes, _) = check(source);
    assert!(
        codes.iter().any(|found| found == code),
        "expected {code}, got {codes:?}"
    );
}

fn expect_any_error(source: &str, code: &str) {
    let codes = check_any(source);
    assert!(
        codes.iter().any(|found| found == code),
        "expected {code}, got {codes:?}"
    );
}

fn warnings(source: &str) -> Vec<String> {
    let lexed = lex(0, source).expect("the source lexes");
    let (module, _) = parse_module(0, source, &lexed);
    let result = check_module(0, &module);
    result
        .diagnostics
        .items
        .iter()
        .filter(|item| item.severity == aevia_syntax::Severity::Warning)
        .map(|item| item.code.clone())
        .collect()
}

fn scheme_of(module: &CheckedModule, name: &str) -> aevia_types::Scheme {
    match module.exports.get(name) {
        Some(Export::Value { scheme, .. }) => scheme.clone(),
        other => panic!("`{name}` is not an exported value: {other:?}"),
    }
}

#[test]
fn kinematics_example_type_checks() {
    expect_ok(
        r#"let g = 9.80665 m/s^2

fn fall_time(h: Float[L]): Float[T] =
    sqrt(2 * h / g)
"#,
    );
}

#[test]
fn adding_length_and_time_is_a_dimension_error() {
    expect_error("let bad = 3 m + 2 s\n", "E0401");
}

#[test]
fn adding_a_bare_number_to_a_length_is_a_dimension_error() {
    expect_error("let bad = 3 m + 2\n", "E0401");
}

#[test]
fn transcendentals_require_dimensionless_arguments() {
    expect_error("let bad = sin(3 m)\n", "E0403");
    expect_ok("let good = sin(30 deg)\n");
}

#[test]
fn dimension_polymorphic_mean_keeps_its_dimension_parameter() {
    // The old version of this test asserted only "no error", which is why the
    // erased scheme `Tensor<1, Real> -> Real` survived: defaulting ran before
    // generalisation. Assert the scheme itself.
    let module = expect_ok(
        r#"fn mean<d>(xs: Tensor<1, Real[d]>): Real[d] =
    sum(xs) / length(xs)
"#,
    );
    let scheme = scheme_of(&module, "mean");
    assert_eq!(
        scheme.dims.len(),
        1,
        "`mean` must quantify one dimension, got {:?}",
        scheme.dims
    );
    let dim = aevia_dim::Dim::var(scheme.dims[0]);
    let expected = Type::func(
        vec![Type::tensor(Shape::Rank(1), Type::real(dim.clone()))],
        Type::real(dim),
    );
    assert_eq!(scheme.ty, expected);
    // A polymorphic dimension is not unresolved, so it must not warn.
    assert!(
        !warnings(
            r#"fn mean<d>(xs: Tensor<1, Real[d]>): Real[d] =
    sum(xs) / length(xs)
"#
        )
        .contains(&"W0301".to_string()),
        "a generalised dimension must not emit W0301"
    );
}

#[test]
fn square_roots_halve_the_dimension() {
    expect_ok("fn f(x: Real[L^2]): Real[L] = sqrt(x)\n");
    expect_error("fn f(x: Real[L^2]): Real[T] = sqrt(x)\n", "E0401");
}

#[test]
fn multiplication_adds_dimensions() {
    expect_ok(
        r#"fn energy(m: Real[M], v: Real[L/T]): Real[M*L^2/T^2] =
    m * v^2 / 2
"#,
    );
}

#[test]
fn records_are_structural() {
    expect_ok(
        r#"type Vec2 = { x: Real[L], y: Real[L] }
fn norm(p: Vec2): Real[L] = sqrt(p.x^2 + p.y^2)
"#,
    );
}

#[test]
fn missing_record_field_is_reported() {
    expect_error(
        r#"type Vec2 = { x: Real[L], y: Real[L] }
fn bad(p: Vec2): Real[L] = p.z
"#,
        "E0306",
    );
}

#[test]
fn variants_and_exhaustiveness() {
    expect_ok(
        r#"type Shape =
    | Circle(Real[L])
    | Rect(Real[L], Real[L])

fn area(s: Shape): Real[L^2] =
    match s with
    | Circle(r) -> pi * r^2
    | Rect(w, h) -> w * h
"#,
    );
    expect_error(
        r#"type Shape =
    | Circle(Real[L])
    | Rect(Real[L], Real[L])

fn area(s: Shape): Real[L^2] =
    match s with
    | Circle(r) -> pi * r^2
"#,
        "E0309",
    );
}

#[test]
fn unbound_names_are_reported() {
    expect_error("let x = nope\n", "E0301");
}

#[test]
fn user_units_are_usable_after_declaration() {
    expect_ok(
        r#"dimension Currency
unit USD : Currency = 1
unit EUR : Currency = 1.08 USD
let price: Real[Currency] = 12 EUR
"#,
    );
}

#[test]
fn affine_temperature_literals_are_points() {
    expect_ok("let t: Point[Θ] = 25.0 °C\n");
    expect_error("let t: Float[Θ] = 25.0 °C\n", "E0302");
}

#[test]
fn temperature_differences_are_scalars() {
    expect_ok(
        r#"fn warming(t: Point[Θ]): Point[Θ] = t + 10.0 K
fn difference(a: Point[Θ], b: Point[Θ]): Float[Θ] = a - b
"#,
    );
    expect_error(
        "fn bad(a: Point[Θ], b: Point[Θ]): Point[Θ] = a + b\n",
        "E0405",
    );
}

#[test]
fn tensors_track_rank() {
    expect_ok(
        r#"fn dot3(a: Tensor<1, Float>, b: Tensor<1, Float>): Float = a @ b
fn mv(m: Tensor<2, Float>, v: Tensor<1, Float>): Tensor<1, Float> = m @ v
"#,
    );
}

#[test]
fn io_actions_are_separated_from_pure_values() {
    expect_ok(
        r#"fn main(): IO Unit = do
    println("hi")
"#,
    );
    // `E0313` (IO in a pure context) is the code the appendix documents for
    // this; the old test expected a generic `E0302`.
    expect_error(
        r#"fn bad(): Unit = do
    println("hi")
"#,
        "E0313",
    );
}

#[test]
fn literal_defaulting_keeps_declarations_separate() {
    let module = expect_ok("let n = 1\nlet x = 2.0 m\n");
    assert!(module.exports.contains_key("n"));
    assert!(module.exports.contains_key("x"));
}

#[test]
fn pipe_and_placeholder_are_desugared() {
    expect_ok("let y = 2.0 m |> sqrt\n");
    expect_ok(
        r#"fn add(a: Int, b: Int): Int = a + b
let inc = add(1, _)
"#,
    );
}

#[test]
fn string_interpolation_type_checks() {
    expect_ok(
        r#"let x = 1.0 m
let s = "x = {x}"
"#,
    );
}

#[test]
fn exponentiation_requires_literal_exponents_for_dimensions() {
    expect_ok("let a = (3.0 m)^2\n");
    expect_error("fn f(x: Real[L], n: Int): Real[L] = x^n\n", "E0404");
}

// --- regressions found by review -------------------------------------------

#[test]
fn let_bindings_are_generalised_before_use() {
    expect_ok(
        r#"let id = fn (x) => x
let a = id(1)
let b = id("hi")
"#,
    );
    expect_ok(
        r#"let id = fn (x) => x
let a = id({ x = 1 })
let b = id("hi")
"#,
    );
}

#[test]
fn open_record_annotations_absorb_extra_fields() {
    expect_ok(
        r#"fn norm(p: { x: Real[L], y: Real[L], ...r }): Real[L] =
    sqrt(p.x^2 + p.y^2)

let a = norm({ x = 1.0 m, y = 2.0 m, z = 3.0 m })
"#,
    );
}

#[test]
fn affine_points_reject_dimension_mismatches() {
    expect_error("let bad = 25.0 °C + 3.0 m\n", "E0401");
}

#[test]
fn tensor_plus_scalar_checks_the_dimension() {
    expect_error("let bad = 1.0 m + [1.0, 2.0, 3.0]\n", "E0401");
    expect_ok("let ok = [1, 2] + [3, 4]\n");
    expect_ok("let ok = 1.0 m + [1.0 m, 2.0 m]\n");
}

#[test]
fn boolean_matches_are_exhaustive_with_both_arms() {
    expect_ok(
        r#"fn f(b: Bool): Int =
    match b with
    | true -> 1
    | false -> 0
"#,
    );
    expect_error(
        r#"fn f(b: Bool): Int =
    match b with
    | true -> 1
"#,
        "E0309",
    );
    expect_error(
        r#"fn f(b: Bool): Int =
    match b with
    | true if true -> 1
    | false -> 0
"#,
        "E0309",
    );
}

#[test]
fn user_functions_shadow_builtins() {
    expect_ok(
        r#"fn norm(x: Real[L]): Real[L] = x

let a = norm(2.0 m)
"#,
    );
}

#[test]
fn unary_minus_applies_to_literals() {
    expect_ok("let a = -1\nlet b = -1.5\nlet c = -1.0 m\n");
}

// --- the soundness holes of audit/types.md §2 ------------------------------
//
// Each program below used to type-check and must not. The assertions are on
// the error code, never on message text.

#[test]
fn forward_reference_to_a_top_level_let_is_constrained() {
    // §2.1: pass B and pass C must share one variable.
    expect_error("fn f(): Int = g\nlet g = \"hello\"\n", "E0302");
    expect_ok("fn f(): Int = g\nlet g = 41\n");
}

#[test]
fn a_symbol_keeps_one_dimension() {
    // §2.2 / §3.3.3: `symbol` allocates a weak variable, so one symbol cannot
    // be a length here and a time there.
    expect_error(
        "let x = symbol(\"x\")\nlet a = x + 3 m\nlet b = x + 2 s\n",
        "E0401",
    );
    expect_ok("let x = symbol(\"x\")\nlet a = x + 3 m\nlet b = x * 2\n");
}

#[test]
fn an_integer_operand_rejects_a_decimal_literal() {
    // §2.3: `join(Int, Float) = Float`, so the annotation is wrong.
    expect_error("fn f(a: Int): Int = a + 2.0\n", "E0302");
    expect_ok("fn f(a: Int): Int = a + 2\n");
}

#[test]
fn a_negative_exponent_is_exact() {
    // §2.4: `2^-1` is `Real`, not `Int`.
    expect_error("let half: Int = 2^-1\n", "E0302");
    let module = expect_ok("let half = 2^-1\n");
    assert_eq!(scheme_of(&module, "half").ty, Type::real(aevia_dim::Dim::one()));
}

#[test]
fn an_index_must_be_an_int() {
    // §2.5: the index type was unified and the error discarded.
    expect_error("let v = [10, 20, 30]\nlet a: Int = v[\"nope\"]\n", "E0302");
}

#[test]
fn a_literal_index_is_bounds_checked() {
    // §2.5 / §3.5.4: a refined shape plus a literal index is a static check.
    expect_error("let v = [10, 20, 30]\nlet b: Int = v[99]\n", "E0802");
    expect_ok("let v = [10, 20, 30]\nlet b: Int = v[2]\n");
}

#[test]
fn a_dimensioned_exponent_is_rejected() {
    // §2.6 / §4.11: `2^(3 m)` is `E0402`.
    expect_error("let bad = 2^(3 m)\n", "E0402");
}

#[test]
fn try_outside_a_result_function_is_rejected() {
    // §2.7 / §3.9.3.
    expect_error("fn f(): Int = Ok(1)?\n", "E0702");
    expect_ok("fn f(): Result<Int, String> = Ok(Ok(1)?)\n");
}

#[test]
fn functions_are_neither_eq_nor_showable() {
    // §2.8 / §3.8.4: a function has no `Eq` and no `Show` instance.
    expect_error(
        "fn f(x: Int): Int = x\nlet same: Bool = f == f\n",
        "E0310",
    );
    expect_error(
        "fn f(x: Int): Int = x\nlet s: String = show(f)\n",
        "E0310",
    );
    // The instances that must exist do exist.
    expect_ok("let s: String = show(1)\nlet t: String = show(1.0 m)\n");
}

#[test]
fn duplicate_record_fields_are_rejected() {
    // §2.9 / §3.6.1. The parser reports it before the checker sees the
    // literal; the checker reports it for record types and updates.
    expect_any_error("let p = { x = 1, x = \"two\" }\n", "E0208");
    expect_any_error(
        "type P = { x: Int }\nfn f(p: P) = { p with x = 1, x = 2 }\n",
        "E0208",
    );
}

#[test]
fn a_rank_only_tensor_does_not_satisfy_a_refined_annotation() {
    // §2.10 / §3.5.1: rank-only -> refined needs `refine_shape`.
    expect_error("let a: Tensor<[3], Float> = zeros(4)\n", "E0302");
    // The safe direction still works.
    expect_ok("let a: Tensor<1, Float> = [1.0, 2.0, 3.0]\n");
}

#[test]
fn rssn_invoke_is_not_polymorphic() {
    // §2.11: `Rssn.invoke` used to be `∀a. a`.
    expect_error("let n: Int = Rssn.invoke(\"f\", \"h\", [])\n", "E0302");
    expect_error("let s: String = Rssn.invoke(\"f\", \"h\", [])\n", "E0302");
}

#[test]
fn a_tensor_of_floats_stays_float_when_scaled() {
    // §2.12: dividing by an `Int` literal must not leave the IEEE path.
    let module = expect_ok(
        "fn f(xs: Tensor<1, Float>): Tensor<1, Float> = xs / 2\nfn g(xs: Tensor<1, Float>): Tensor<1, Float> = xs * 2\n",
    );
    let float_tensor = Type::tensor(Shape::Rank(1), Type::float(aevia_dim::Dim::one()));
    let expected = Type::func(vec![float_tensor.clone()], float_tensor.clone());
    assert_eq!(scheme_of(&module, "f").ty, expected);
    assert_eq!(scheme_of(&module, "g").ty, expected);
}

#[test]
fn a_non_recursive_let_cannot_see_itself() {
    // §2.13 / §3.3.2 rule 5.
    expect_error("let x = x + 1\n", "E0301");
    expect_error("let f = fn (n) => if n == 0 then 0 else f(n - 1)\n", "E0301");
    // `let rec` binds first, so it is legal.
    expect_ok("let rec f = fn (n) => if n == 0 then 0 else f(n - 1)\n");
}

#[test]
fn an_orphan_instance_is_rejected() {
    // §2.14 / §3.8.3: the user cannot add `Show Int`.
    expect_error(
        "instance Show for Int where\n    show(x) = \"n\"\n",
        "E0604",
    );
    // A local type may have an instance of a prelude class.
    expect_ok(
        "type Vec2 = { x: Float, y: Float }\ninstance Show for Vec2 where\n    show(v) = \"v\"\n",
    );
}

#[test]
fn overlapping_instances_are_rejected() {
    // §3.8.2: head matching does not backtrack, so overlap is `E0311`.
    expect_error(
        "class Pretty a where\n    pretty(x: a): String\n\ntype V = { x: Float }\ninstance Pretty for V where\n    pretty(v) = \"v\"\ninstance Pretty for V where\n    pretty(v) = \"w\"\n",
        "E0311",
    );
}

#[test]
fn a_temperature_difference_cannot_use_an_affine_unit() {
    // §2.15 / §5.4.1.
    expect_error(
        "let t: Float[Θ] = 10 K\nlet c = value_in(°C, t)\n",
        "E0510",
    );
    expect_ok("let p: Point[Θ] = 25.0 °C\nlet c = value_in(°C, p)\n");
}

#[test]
fn a_display_unit_must_match_the_value_dimension() {
    // §5.4.1: `to`/`value_in` require the unit's dimension.
    expect_error("let d: Float[L] = 3.0 m\nlet c = value_in(s, d)\n", "E0509");
    expect_ok("let d: Float[L] = 3.0 m\nlet c = value_in(km, d)\n");
}

// --- rows, nominal identity, recursion -------------------------------------

#[test]
fn a_record_update_keeps_the_open_tail() {
    // §3.6.2: `{ p with z = … }` keeps `p`'s unknown fields.
    expect_ok(
        r#"type P = { x: Float, y: Float }
fn f(p: P): { x: Float, y: Float, z: Float } = { p with z = 1.0 }
fn g(p: { x: Float, ...r }): { x: Float, ...r } = { p with x = 2.0 }
"#,
    );
}

#[test]
fn a_recursive_type_is_legal_and_positivity_is_checked() {
    // §3.6.4.
    expect_ok("type Tree<a> = | Leaf | Node(Tree<a>, a, Tree<a>)\n");
    expect_error("type Bad = Bad -> Int\n", "E0312");
}

#[test]
fn a_type_declaration_gets_a_fresh_id_per_compilation_unit() {
    // §3.2.4 / PLAN D6: ids are allocated, not looked up by name, so two
    // modules that both declare `type Shape` do not share one nominal type.
    expect_error("type A = | A\ntype A = | A\n", "E0316");
    let first = expect_ok("type Shape = | Circle\n");
    let second = expect_ok("type Shape = | Circle\n");
    let id = |module: &CheckedModule| match module.exports.get("Shape") {
        Some(Export::Type { adt }) => *adt,
        other => panic!("`Shape` is not a type export: {other:?}"),
    };
    assert_ne!(id(&first), id(&second));
    assert_ne!(id(&first), aevia_types::adt::RESULT_ID);
}

// --- representation and diagnostics contract --------------------------------

#[test]
fn a_bare_zero_is_not_dimension_polymorphic() {
    // §4.7: `3 m + 0` is `E0406`, not a generic mismatch.
    expect_error("let bad = 3 m + 0\n", "E0406");
    expect_error("let bad = 3 m + 2\n", "E0401");
}

#[test]
fn dividing_an_affine_point_is_e0405() {
    // §5.6.3.
    expect_error("fn f(p: Point[Θ]): Point[Θ] = p / 2\n", "E0405");
}

#[test]
fn a_dimensioned_determinant_needs_dyn() {
    // §4.11 / §3.5.5.
    expect_error(
        "fn f(a: Tensor<2, Real[L]>): Real[L] = det(a)\n",
        "E0410",
    );
    // A dimensionless matrix still has a determinant.
    expect_ok("fn f(a: Tensor<2, Float>): Float = det(a)\n");
}

#[test]
fn a_duplicate_function_name_is_e0316() {
    expect_error("fn f(): Int = 1\nfn f(): String = \"s\"\n", "E0316");
}

#[test]
fn an_affine_offset_must_match_the_unit_dimension() {
    // §5.1.1.
    expect_error("unit bad : Length = 1 m offset 5 K\n", "E0504");
}

#[test]
fn deriving_over_a_function_field_is_rejected() {
    // §3.8.5.
    expect_error(
        "type F = | F(Int -> Int)\nderive Eq for F\n",
        "E0317",
    );
    expect_ok("type V = | V(Float, Float)\nderive Eq, Show for V\n");
}

#[test]
fn a_redundant_match_arm_warns() {
    // §3.6.3.
    assert!(
        warnings("fn f(b: Bool): Int = match b with\n    | _ -> 1\n    | true -> 2\n")
            .contains(&"W0201".to_string())
    );
}

#[test]
fn mixing_real_and_float_warns_about_the_exact_path() {
    // §3.4.1 / appendix B.6.
    assert!(
        warnings("let x = 1 m + 1.0 m\n").contains(&"W0302".to_string()),
        "mixing Real and Float must warn"
    );
}

#[test]
fn an_unresolved_symbol_warns_w0301() {
    assert!(
        warnings("let x = symbol(\"x\")\n").contains(&"W0301".to_string()),
        "a weak dimension defaults with W0301"
    );
}

#[test]
fn an_unused_symbol_warns_w0303() {
    assert!(
        warnings("let x = symbol(\"x\")\n").contains(&"W0303".to_string()),
        "a symbol nobody reads is dead weight"
    );
    assert!(
        !warnings("let x = symbol(\"x\")\nlet a = x + 3 m\n").contains(&"W0303".to_string()),
        "a used symbol is not warned about"
    );
}

// --- refinement survives indexing ------------------------------------------

#[test]
fn indexing_keeps_the_refinement() {
    let module = expect_ok(
        "fn f(m: Tensor<[2, 3], Float>): Tensor<[3], Float> = m[0]\n",
    );
    assert_eq!(
        scheme_of(&module, "f").ty,
        Type::func(
            vec![Type::tensor(
                Shape::Refined(vec![Some(2), Some(3)]),
                Type::float(aevia_dim::Dim::one())
            )],
            Type::tensor(
                Shape::Refined(vec![Some(3)]),
                Type::float(aevia_dim::Dim::one())
            ),
        )
    );
}

// --- `Rssn.<op>` resolution ------------------------------------------------

#[test]
fn a_rssn_call_is_resolved_and_pinned() {
    // PLAN D1: the checker picks the overload and records its key, so the
    // runtime cannot execute a different one.
    let module = expect_ok("let a = Rssn.sin(0.5)\n");
    assert_eq!(module.rssn_calls.len(), 1);
    let call = &module.rssn_calls[0];
    assert_eq!(call.name, "sin");
    let entry = aevia_rssn::registry()
        .op("sin")
        .expect("the manifest has `sin`");
    let expected = entry.key(call.overload).expect("the overload has a key");
    assert_eq!(call.key.signature_hash, expected.signature_hash);
    assert_eq!(
        scheme_of(&module, "a").ty,
        Type::real(aevia_dim::Dim::one()),
        "`sin(0.5)` picks the numeric overload"
    );
}

#[test]
fn an_overloaded_rssn_name_is_not_a_value() {
    // PLAN D1: `Rssn.<op>` outside call position is `E0303` unless every
    // overload has one type.
    expect_error("let a = Rssn.sin\n", "E0303");
    // A member with one fixed type is still a value.
    expect_ok("let v: String = Rssn.Version\n");
}

#[test]
fn an_unresolvable_rssn_call_is_e1003() {
    // The arity filter removes every overload: `sin` takes one argument.
    expect_error("let a = Rssn.sin(1, 2)\n", "E1003");
}

#[test]
fn an_ambiguous_rssn_call_is_e1002() {
    // `x` has no known type, so both `(Real) -> Real` and the symbolic
    // overload accept it equally well.
    expect_error("fn f(x) = Rssn.sin(x)\n", "E1002");
}

// --- classes ---------------------------------------------------------------

#[test]
fn a_class_method_resolves_through_the_instance_table() {
    let module = expect_ok(
        "class Pretty a where\n    pretty(x: a): String\n\ntype Vec2 = { x: Float, y: Float }\ninstance Pretty for Vec2 where\n    pretty(v) = \"v\"\nlet s = pretty({ x = 1.0, y = 2.0 })\n",
    );
    assert_eq!(module.dictionaries.len(), 1);
    let use_site = &module.dictionaries[0];
    assert_eq!(use_site.class, "Pretty");
    assert_eq!(use_site.method, "pretty");
    assert_eq!(
        use_site.head,
        Type::record([
            ("x".to_string(), Type::float(aevia_dim::Dim::one())),
            ("y".to_string(), Type::float(aevia_dim::Dim::one())),
        ])
    );
}

#[test]
fn an_instance_method_does_not_shadow_the_global_name() {
    // §2.14: installing `show : Int -> String` as a global used to hijack
    // `show` for every type in the module.
    expect_ok(
        "type V = { x: Float }\ninstance Show for V where\n    show(v) = \"v\"\nlet s: String = show(1.0 m)\n",
    );
}

#[test]
fn a_class_constraint_is_inferred_for_a_generic_function() {
    // §3.2.5: constraints are rebuilt when the signature omits them.
    let module = expect_ok("fn describe(x) = show(x)\n");
    let scheme = scheme_of(&module, "describe");
    assert_eq!(scheme.constraints.len(), 1);
    assert_eq!(scheme.constraints[0].class, "Show");
    // ...and it is solved at the call site.
    expect_ok("fn describe(x) = show(x)\nlet s = describe(1)\n");
}

#[test]
fn a_set_of_floats_is_rejected() {
    // §3.8.4: `Set<a>` needs `TotalOrd`, and `Float` has none.
    expect_error("type S = Set<Float>\n", "E0310");
    expect_ok("type S = Set<Int>\n");
    // `sort` needs the same instance.
    expect_error("let s = sort([1.0, 2.0])\n", "E0310");
    expect_ok("let s = sort([3, 1, 2])\n");
}

#[test]
fn a_function_declaration_is_generalised_before_later_uses() {
    // `fn ident(x) = x` used to be generalised only at the end of the module,
    // by which time the first call site had already unified the shared
    // variable: `ident(1)` followed by `ident("s")` was E0302.
    let (codes, module) = check(
        r#"fn ident(x) = x
let a = ident(1)
let b = ident("s")
"#,
    );
    assert!(codes.is_empty(), "expected no errors, got {codes:?}");
    let scheme = scheme_of(&module, "ident");
    assert_eq!(
        scheme.vars.len(),
        1,
        "`ident` must quantify its parameter, found {:?}",
        scheme.ty
    );
}

#[test]
fn a_function_with_a_class_constraint_quantifies_it() {
    let (codes, module) = check(
        r#"class Pretty a where
    pretty(x: a): String
instance Pretty for Int where
    pretty(n) = "n"
instance Pretty for String where
    pretty(s) = s
fn render(x) = pretty(x)
let a = render(1)
let b = render("s")
"#,
    );
    assert!(codes.is_empty(), "expected no errors, got {codes:?}");
    let scheme = scheme_of(&module, "render");
    assert!(
        !scheme.constraints.is_empty(),
        "`render` must carry a `Pretty` constraint, found {:?}",
        scheme
    );
    assert!(!scheme.vars.is_empty(), "`render` must be polymorphic");
}

/// The byte offset of the `nth` occurrence of `needle`, counting from one.
fn offset_of(source: &str, needle: &str, nth: usize) -> usize {
    let mut from = 0;
    let mut found = None;
    for _ in 0..nth.max(1) {
        let at = source
            .get(from..)
            .and_then(|rest| rest.find(needle))
            .map(|at| at + from)
            .unwrap_or_else(|| panic!("`{needle}` does not occur {nth} times in {source:?}"));
        found = Some(at);
        from = at + needle.len();
    }
    found.expect("at least one")
}

/// The type the checker recorded for the expression that is exactly `text`.
fn expr_type(module: &CheckedModule, source: &str, text: &str, nth: usize) -> Type {
    let start = offset_of(source, text, nth);
    let span = aevia_syntax::Span::new(0, start, start + text.len());
    module
        .expr_types
        .get(&span)
        .cloned()
        .unwrap_or_else(|| panic!("no recorded type for `{text}` at {start}"))
}

#[test]
fn an_expression_has_a_recorded_type() {
    let source = "let x = 1.0 + 2\n";
    let module = expect_ok(source);
    // §6.3.2: `Float ⊗ Exact → Real`. The entry is the expression's, not the
    // binding's: `x` is recorded separately at its own span.
    assert_eq!(
        expr_type(&module, source, "1.0 + 2", 1),
        Type::real(aevia_dim::Dim::one())
    );
    assert_eq!(
        expr_type(&module, source, "1.0", 1),
        Type::real(aevia_dim::Dim::one())
    );
    assert_eq!(expr_type(&module, source, "x", 1), Type::real(aevia_dim::Dim::one()));
}

#[test]
fn a_recorded_type_is_the_resolved_one_not_the_variable_it_started_as() {
    // `2` is synthesised as a bare literal variable and only becomes `Int`
    // when the argument is checked against the parameter. An entry frozen at
    // synthesis time would still say `t3`.
    let source = "fn f(x: Int): Int = x\nlet a = f(2)\n";
    let module = expect_ok(source);
    assert_eq!(expr_type(&module, source, "2", 1), Type::int());
}

#[test]
fn a_polymorphic_call_records_its_own_instantiation() {
    let source = "fn f(x) = x + 1.0\nlet a = f(1)\nlet b = f(2.0)\n";
    let module = expect_ok(source);
    // Each call site is its own span, so the second must not report the first's
    // instantiation.
    assert_eq!(expr_type(&module, source, "f(1)", 1), Type::int());
    assert_eq!(
        expr_type(&module, source, "f(2.0)", 1),
        Type::float(aevia_dim::Dim::one())
    );
    // The body is checked once and stays polymorphic: it is not frozen at the
    // first instantiation either.
    let body = expr_type(&module, source, "x + 1.0", 1);
    assert!(
        aevia_types::class::has_variables(&body),
        "the body must stay polymorphic, found {body}"
    );
}

#[test]
fn every_expression_has_at_most_one_entry_per_span() {
    let source = "fn f(x: Int): Int = if x > 0 then x + 1 else x - 1\nlet a = f(2)\n";
    let module = expect_ok(source);
    // The table is keyed by span, so a span the checker visits twice — an
    // argument that is both synthesised and checked — cannot produce two
    // answers.
    assert!(!module.expr_types.is_empty());
    assert!(
        module.expr_types.keys().all(|span| span.start < span.end),
        "a zero-width span was recorded"
    );
}

#[test]
fn a_mixed_literal_sum_is_real_on_either_side() {
    // §6.3.2: an exact literal and a decimal one meet on the exact `Real`
    // path. `1.5 + 1` used to be `Int` and `1 + 1.5` `Float` — the same sum
    // typed two ways, one of which cannot hold the value.
    let source = "let a = 1.5 + 1\nlet b = 1 + 1.5\nlet c = 1.0 / 2\n";
    let module = expect_ok(source);
    for name in ["a", "b", "c"] {
        assert_eq!(
            scheme_of(&module, name).ty,
            Type::real(aevia_dim::Dim::one()),
            "`{name}` must be Real"
        );
    }
}

#[test]
fn an_exact_literal_does_not_narrow_to_int_when_it_meets_one() {
    // `1.0 + 2` is exact, so the sum cannot be read as an `Int`. The literal
    // kinds are joined on their *stricter* side: `Int` is the only kind that
    // admits an `Int` reading, and re-marking an exact sum as `Int` when it
    // later met an integer literal typed `let x = 1.0 + 2` then `x * 3` as
    // `Int` — a representation that cannot hold the value.
    let (codes, _) = check(
        r#"let x = 1.0 + 2
let y = x * 3
let z: Int = x
"#,
    );
    assert_eq!(codes, ["E0302"], "the exact sum must not become an Int");

    // ... and it is still exact, so a `Real` annotation fits.
    let (codes, _) = check(
        r#"let x = 1.0 + 2
let y = x * 3
let z: Real = x
"#,
    );
    assert!(codes.is_empty(), "expected no errors, got {codes:?}");
}

#[test]
fn a_decimal_sum_still_takes_the_expected_type() {
    // §3.10.3: a literal takes the type it is checked against, so the same
    // expression is a `Float` where a `Float` is wanted and exact where a
    // `Real` is wanted. Making the join concrete would have broken this.
    let (codes, _) = check(
        r#"let a: Float = 1.0 + 2
let b: Real = 1.0 + 2
let c: Int = 1 + 2
"#,
    );
    assert!(codes.is_empty(), "expected no errors, got {codes:?}");
}
