//! End-to-end checks of the type and dimension system against the examples in
//! the specification.
//!
//! Sources are written as raw strings so their indentation is exactly what the
//! layout algorithm sees.

use aevia_syntax::lex;
use aevia_syntax::parse_module;
use aevia_types::Capabilities;
use aevia_types::CheckOptions;
use aevia_types::CheckedModule;
use aevia_types::DynPrimitive;
use aevia_types::Export;
use aevia_types::HostRegistrationKind;
use aevia_types::Rep;
use aevia_types::Shape;
use aevia_types::Stability;
use aevia_types::Type;
use aevia_types::check::check_module;

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

fn expect_error(
    source: &str,
    code: &str,
) {
    let (codes, _) = check(source);
    assert!(
        codes.iter().any(|found| found == code),
        "expected {code}, got {codes:?}"
    );
}

fn expect_warning(
    source: &str,
    code: &str,
) {
    let lexed = lex(0, source).expect("the source lexes");
    let (module, parse_diagnostics) = parse_module(0, source, &lexed);
    assert!(
        !parse_diagnostics.has_errors(),
        "parse errors: {parse_diagnostics:?}"
    );
    let result = check_module(0, &module);
    let codes: Vec<String> = result
        .diagnostics
        .items
        .iter()
        .map(|item| item.code.clone())
        .collect();
    assert!(
        codes.iter().any(|found| found == code),
        "expected {code}, got {codes:?}"
    );
}

fn expect_any_error(
    source: &str,
    code: &str,
) {
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

fn scheme_of(
    module: &CheckedModule,
    name: &str,
) -> aevia_types::Scheme {
    match module.exports.get(name) {
        | Some(Export::Value { scheme, .. }) => scheme.clone(),
        | other => panic!("`{name}` is not an exported value: {other:?}"),
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
    assert_eq!(
        scheme_of(&module, "half").ty,
        Type::real(aevia_dim::Dim::one())
    );
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
    expect_error("fn f(x: Int): Int = x\nlet same: Bool = f == f\n", "E0310");
    expect_error("fn f(x: Int): Int = x\nlet s: String = show(f)\n", "E0310");
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
    expect_error(
        "let f = fn (n) => if n == 0 then 0 else f(n - 1)\n",
        "E0301",
    );
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
    expect_error("let t: Float[Θ] = 10 K\nlet c = value_in(°C, t)\n", "E0510");
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
    // §3.2.4: ids are allocated, not looked up by name, so two
    // modules that both declare `type Shape` do not share one nominal type.
    expect_error("type A = | A\ntype A = | A\n", "E0316");
    let first = expect_ok("type Shape = | Circle\n");
    let second = expect_ok("type Shape = | Circle\n");
    let id = |module: &CheckedModule| {
        match module.exports.get("Shape") {
            | Some(Export::Type { adt }) => *adt,
            | other => panic!("`Shape` is not a type export: {other:?}"),
        }
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
    expect_error("fn f(a: Tensor<2, Real[L]>): Real[L] = det(a)\n", "E0410");
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
    expect_error("type F = | F(Int -> Int)\nderive Eq for F\n", "E0317");
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
    // Two *values* of the two phases mix; a decimal literal next to an exact
    // one is read exactly instead (§3.10.1), so a literal pair does not warn.
    assert!(
        warnings("let r: Real[L] = 1 m\nlet a: Float[L] = 1.0 m\nlet x = r + a\n")
            .contains(&"W0302".to_string()),
        "mixing Real and Float must warn"
    );
    assert!(!warnings("let x = 1 m + 1.0 m\n").contains(&"W0302".to_string()));
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
    let module = expect_ok("fn f(m: Tensor<[2, 3], Float>): Tensor<[3], Float> = m[0]\n");
    assert_eq!(
        scheme_of(&module, "f").ty,
        Type::func(
            vec![Type::tensor(
                Shape::refined([Some(2), Some(3)]),
                Type::float(aevia_dim::Dim::one())
            )],
            Type::tensor(
                Shape::refined([Some(3)]),
                Type::float(aevia_dim::Dim::one())
            ),
        )
    );
}

// --- `Rssn.<op>` resolution ------------------------------------------------

#[test]
fn a_rssn_call_is_resolved_and_pinned() {
    // The checker picks the overload and records its key, so the
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
        Type::Dyn(Rep::Real),
        "a derived overload pins dispatch but does not claim an exact result dimension"
    );
}

#[test]
fn an_overloaded_rssn_name_is_not_a_value() {
    // `Rssn.<op>` outside call position is `E0303` unless every
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
fn offset_of(
    source: &str,
    needle: &str,
    nth: usize,
) -> usize {
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
fn expr_type(
    module: &CheckedModule,
    source: &str,
    text: &str,
    nth: usize,
) -> Type {
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
    // §3.4.1/§3.10.2: `join(Int, Float) = Float`; the closed tree's value is
    // folded exactly (§3.10.4), a value property rather than its type. The
    // entry is the expression's, not the binding's: `x` is recorded
    // separately at its own span.
    assert_eq!(
        expr_type(&module, source, "1.0 + 2", 1),
        Type::float(aevia_dim::Dim::one())
    );
    assert_eq!(
        expr_type(&module, source, "1.0", 1),
        Type::float(aevia_dim::Dim::one())
    );
    assert_eq!(
        expr_type(&module, source, "x", 1),
        Type::float(aevia_dim::Dim::one())
    );
    assert!(
        module
            .constant_regions
            .values()
            .any(|rep| *rep == aevia_types::Rep::Float),
        "the closed tree is a Float constant region"
    );
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
    let source = "fn f(x) = x + 1\nlet a = f(1)\nlet b = f(2.0)\n";
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
    let body = expr_type(&module, source, "x + 1", 1);
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
fn a_mixed_literal_sum_is_float_on_either_side() {
    // §3.4.1/§3.10.2: an integer literal and a decimal one meet at
    // `join(Int, Float) = Float`, whichever side each is on. `1.5 + 1` used to
    // be `Int` and `1 + 1.5` `Float` — the same sum typed two ways, one of
    // which cannot hold the value. Exactness is kept by constant-region
    // folding (§3.10.4), not by changing the type.
    let source = "let a = 1.5 + 1\nlet b = 1 + 1.5\nlet c = 1.0 / 2\n";
    let module = expect_ok(source);
    for name in ["a", "b", "c"] {
        assert_eq!(
            scheme_of(&module, name).ty,
            Type::float(aevia_dim::Dim::one()),
            "`{name}` must be Float"
        );
    }
    // A `Real` context still reads the same tree exactly (§3.10.3).
    let module = expect_ok("let d: Real = 1.5 + 1\n");
    assert_eq!(
        scheme_of(&module, "d").ty,
        Type::real(aevia_dim::Dim::one())
    );
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

    // ... and it is a `Float` (§3.10.2), so a `Real` annotation on the
    // *variable* is a mismatch; the literal tree itself still reads exactly
    // where `Real` is expected (§3.10.3).
    let (codes, _) = check(
        r#"let x = 1.0 + 2
let y = x * 3
let z: Real = 1.0 + 2
let w: Float = x
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

// --- §3.5.3 tensor arithmetic with units and elementwise -------------------

#[test]
fn a_unit_scales_a_tensor_without_changing_its_representation() {
    // §3.5.3: `Tensor<shape, Scalar<ρ,d>> * Unit[e]` is
    // `Tensor<shape, Scalar<ρ,d*e>>`. This used to fail with "cannot scale a
    // tensor of t3 by a unit" because the literal's element type was still a
    // variable when the unit met it.
    //
    // `Resistance` is the derived dimension `L^2*M/(I^2*T^3)`.
    let us = |name: &str| aevia_dim::Dim::base(name);
    let resistance = us("Length")
        .mul(&us("Length"))
        .mul(&us("Mass"))
        .div(&us("Current").mul(&us("Current")))
        .div(&us("Time").mul(&us("Time")).mul(&us("Time")));
    let ohms = Type::tensor(
        Shape::refined([Some(2), Some(2)]),
        Type::float(resistance.clone()),
    );
    // Lower-case binders are variables; a capitalised `let R` is a nullary
    // constructor pattern (§A.4).
    let module = expect_ok("let r = [[1.0, 2.0], [3.0, 4.0]] * ohm\n");
    assert_eq!(scheme_of(&module, "r").ty, ohms);
    // The annotated form from spec/11 must agree.
    expect_ok("let a: Tensor<2, Float[Resistance]> = [[1.0, 2.0], [3.0, 4.0]] * ohm\n");
    // `Unit[e] * Tensor` is the same product written the other way (§3.5.3).
    let module = expect_ok("let r = ohm * [[1.0, 2.0], [3.0, 4.0]]\n");
    assert_eq!(scheme_of(&module, "r").ty, ohms);
    // `Tensor / Unit[e]` divides the dimension and keeps ρ (§3.5.3).
    let module = expect_ok("let v = [[1.0, 2.0], [3.0, 4.0]] / s\n");
    assert_eq!(
        scheme_of(&module, "v").ty,
        Type::tensor(
            Shape::refined([Some(2), Some(2)]),
            Type::float(aevia_dim::Dim::base("Time").inverse()),
        )
    );
    // The dimensions still have to agree for `+`.
    expect_error("let bad = [1.0, 2.0] * ohm + [1.0, 2.0] * s\n", "E0401");
}

#[test]
fn tensors_add_multiply_and_divide_elementwise() {
    // §3.5.3: same-rank tensors combine elementwise with the scalar rules.
    let module = expect_ok(
        r#"let a = [1.0, 2.0] * [3.0, 4.0]
let b = [1.0, 2.0] + [3.0, 4.0]
let c = [1.0, 2.0] / [3.0, 4.0]
"#,
    );
    let vector = Type::tensor(
        Shape::refined([Some(2)]),
        Type::float(aevia_dim::Dim::one()),
    );
    for name in ["a", "b", "c"] {
        assert_eq!(scheme_of(&module, name).ty, vector, "`{name}`");
    }
    // The element's dimensions multiply / divide, per the table.
    let module = expect_ok(
        r#"let prod = [1.0 m, 2.0 m] * [3.0 s, 4.0 s]
let quot = [1.0 m, 2.0 m] / [3.0 s, 4.0 s]
"#,
    );
    let product = Type::tensor(
        Shape::refined([Some(2)]),
        Type::float(aevia_dim::Dim::base("Length").mul(&aevia_dim::Dim::base("Time"))),
    );
    assert_eq!(scheme_of(&module, "prod").ty, product);
    let time = aevia_dim::Dim::base("Time");
    let quotient = Type::tensor(
        Shape::refined([Some(2)]),
        Type::float(aevia_dim::Dim::base("Length").div(&time)),
    );
    assert_eq!(scheme_of(&module, "quot").ty, quotient);
    // Different ranks are rejected (§3.5.3 forbids cross-rank broadcasting).
    expect_error(
        "fn f(a: Tensor<1, Float>, b: Tensor<2, Float>): Float = 1.0\nlet bad = [1.0, 2.0] * [[1.0], [2.0]]\n",
        "E0302",
    );
}

// --- §8.9 ODE: RateOf ------------------------------------------------------

#[test]
fn odeint_types_the_right_hand_side_through_rate_of() {
    // §8.9: `odeint : ((Float[T], s) -> RateOf<s>) -> s -> Tensor<1, Float[T]> -> Tensor<1, s>`
    // and `RateOf<{x: Float[L], v: Float[L/T]}> = {x: Float[L/T], v: Float[L/T^2]}`.
    let module = expect_ok(
        r#"type State = { x: Float[L], v: Float[L/T] }
type Rate = { x: Float[L/T], v: Float[L/T^2] }
fn rhs(_t: Float[T], s: State): Rate = { x = s.v, v = -9.8 m/s^2 }
fn traj(): Tensor<1, State> =
    odeint(rhs, { x = 0.0 m, v = 1.0 m/s }, [0.0 s, 1.0 s])
"#,
    );
    let state = Type::record(vec![
        (
            "v".to_string(),
            Type::float(aevia_dim::Dim::base("Length").div(&aevia_dim::Dim::base("Time"))),
        ),
        ("x".to_string(), Type::float(aevia_dim::Dim::base("Length"))),
    ]);
    assert_eq!(
        scheme_of(&module, "traj").ty,
        Type::func(Vec::new(), Type::tensor(Shape::Rank(1), state),)
    );

    // A scalar state: `RateOf<Float[L]> = Float[L/T]`, so returning `Float[L]`
    // from the right-hand side is a dimension error.
    expect_error(
        "fn rhs(_t: Float[T], s: Float[L]): Float[L] = s\nlet y = odeint(rhs, 1.0 m, [0.0 s])\n",
        "E0401",
    );
    // A `Bool` state field cannot be flattened into the numeric kernel.
    expect_error(
        "fn rhs(_t: Float[T], s: Bool): Bool = s\nlet y = odeint(rhs, true, [0.0 s])\n",
        "E0302",
    );
    // `RateOf<Int>` is not a rate the solver can integrate.
    expect_error(
        "fn rhs(_t: Float[T], s: Int): RateOf<Int> = 1\nlet y = odeint(rhs, 1, [0.0 s])\n",
        "E0302",
    );
    // The right-hand side must be a two-argument function.
    expect_error(
        "fn rhs(t: Float[T]): Float[T] = t\nlet y = odeint(rhs, 1.0 s, [0.0 s])\n",
        "E0302",
    );
}

#[test]
fn rssn_extension_entry_points_are_typed() {
    // §8.19: `Rssn.typed`/`call_ho`/`register`/`register_plugin` are host
    // extension entry points, not operators, so they are first-class values
    // with fixed signatures rather than `E0301`/`E0303`.
    expect_ok(
        r#"let register = Rssn.register
let register_plugin = Rssn.register_plugin
let typed = Rssn.typed
let call_ho = Rssn.call_ho
"#,
    );
    // A source annotation can name the opaque host types.
    expect_ok("fn reg(p: Plugin) = Rssn.register(p)\n");
    expect_ok("fn go(rep: TypeRep) = Rssn.typed(\"sin\", [], rep)\n");
}

#[test]
fn odeint_with_returns_the_time_series_it_reached() {
    // §8.9: `odeint_with` returns `{ t: Tensor<1, Float[T]>, y: Tensor<1, s> }`.
    let module = expect_ok(
        r#"fn rhs(_t: Float[T], s: Float[L]): Float[L/T] = 1.0 m/s
fn traj(): { t: Tensor<1, Float[T]>, y: Tensor<1, Float[L]> } =
    odeint_with(rhs, 0.0 m, [0.0 s, 1.0 s], { rtol = 1e-8, atol = 1e-10 })
"#,
    );
    let time = aevia_dim::Dim::base("Time");
    let length = aevia_dim::Dim::base("Length");
    let expected = Type::record(vec![
        (
            "t".to_string(),
            Type::tensor(Shape::Rank(1), Type::float(time)),
        ),
        (
            "y".to_string(),
            Type::tensor(Shape::Rank(1), Type::float(length)),
        ),
    ]);
    assert_eq!(
        scheme_of(&module, "traj").ty,
        Type::func(Vec::new(), expected)
    );
}

// --- §3.11 / §4.9 dynamic dimension `Dyn` ----------------------------------

#[test]
fn dyn_wraps_a_scalar_and_keeps_its_representation() {
    // §3.11: `dyn : Scalar<ρ,d> -> Dyn<ρ>`; the payload's representation is
    // kept (`Dyn<Int>`, `Dyn<Real>`, `Dyn<Float>` are different types).
    let module = expect_ok(
        r#"let a = dyn(3.0 m)
let b = dyn(2)
"#,
    );
    assert_eq!(scheme_of(&module, "a").ty, Type::Dyn(Rep::Float));
    assert_eq!(scheme_of(&module, "b").ty, Type::Dyn(Rep::Int));
    // §4.9.4: the call site materialises `d`; the record lets the runtime do
    // the same comparison `check` needs (E0412).
    let length = aevia_dim::Dim::base("Length");
    assert_eq!(module.dyn_calls.len(), 2);
    assert!(
        module
            .dyn_calls
            .iter()
            .any(|call| call.primitive == DynPrimitive::Dyn && call.dim == length)
    );
    assert!(
        module
            .dyn_calls
            .iter()
            .any(|call| call.primitive == DynPrimitive::Dyn && call.dim == aevia_dim::Dim::one())
    );
}

#[test]
fn dyn_requires_a_concrete_dimension() {
    // §4.9.4: `dyn` is a type-directed primitive — `d` must not contain a free
    // dimension variable. Generic code uses `dyn_with(x, dim)`.
    expect_error("fn f<d>(x: Real[d]): Dyn<Real> = dyn(x)\n", "E0304");
    expect_ok("fn f<d>(x: Real[d], dim: DimValue): Dyn<Real> = dyn_with(x, dim)\n");
    // `dyn` of a non-scalar is a type error, not an inference failure.
    expect_error("let t = dyn([1.0, 2.0])\n", "E0302");
}

#[test]
fn dyn_arity_and_value_are_checked() {
    // §4.9.3: `Dyn` is a value: `dim_of`, `value_of` and `pow_dyn` take one.
    let module = expect_ok(
        r#"let d = dyn(3.0 m)
let dim = dim_of(d)
let v = value_of(d)
let p = pow_dyn(d, 2)
"#,
    );
    assert_eq!(scheme_of(&module, "p").ty, Type::Dyn(Rep::Float));
    assert_eq!(
        scheme_of(&module, "v").ty,
        Type::float(aevia_dim::Dim::one())
    );
    // `DimValue` is a runtime type, distinct from the kind `Dim` and from
    // `Dyn` itself.
    assert_eq!(scheme_of(&module, "dim").ty.to_string(), "DimValue");
    expect_error("let d = dyn(3.0 m)\nlet x = dim_of(3.0)\n", "E0302");
}

#[test]
fn check_materialises_the_dimension_it_checks_against() {
    // §3.11: `check : Dyn<ρ> -> Result<Scalar<ρ,d>, DimError>` and §4.9.4:
    // `check<d>(x)` names `d`; the result is the value at the *static* `d`.
    let length = aevia_dim::Dim::base("Length");
    let time = aevia_dim::Dim::base("Time");
    let (codes, module) = check(
        r#"fn single(x: Real[L]): Result<Real[L], DimError> = check<L>(dyn(x))
fn wrong_static(x: Real[L]): Result<Real[T], DimError> = check<T>(dyn(x))
let r: Result<Real[L], DimError> = check(dyn(3.0 m))
"#,
    );
    assert!(codes.is_empty(), "unexpected errors: {codes:?}");
    // The `check<L>` site materialised `Length`; the `check<T>` site `Time`.
    let checks: Vec<_> = module
        .dyn_calls
        .iter()
        .filter(|call| call.primitive == DynPrimitive::Check)
        .map(|call| call.dim.clone())
        .collect();
    assert!(checks.contains(&length), "found {checks:?}");
    assert!(checks.contains(&time), "found {checks:?}");
    // `check` needs `d` concrete: an unconstrained result has no static answer.
    expect_error("let r = check(dyn(3.0 m))\n", "E0304");
    // Its argument must be a `Dyn`, and its type argument a dimension.
    expect_error("let r = check(3.0 m)\n", "E0302");
    expect_error(
        "let r: Result<Real, DimError> = check<Float>(dyn(3.0 m))\n",
        "E0407",
    );
}

#[test]
fn dyn_arithmetic_checks_at_run_time() {
    // §4.9.3: `+`/`-` require equal dimensions (runtime `E0412`), `*` adds and
    // `/` subtracts them, `^ Int` multiplies the exponent, `sqrt` halves it.
    let module = expect_ok(
        r#"let a = dyn(1.0 m) + dyn(2.0 m)
let b = dyn(1.0 m) - dyn(2.0 m)
let c = dyn(1.0 m) * dyn(2.0 s)
let d = dyn(1.0 m) / dyn(2.0 s)
let e = dyn(1.0 m) ^ 2
fn f(n: Int): Dyn<Float> = dyn(1.0 m) ^ n
let g = sqrt(dyn(4.0 m^2))
let h = -dyn(1.0 m)
"#,
    );
    for name in ["a", "b", "c", "d", "e", "g", "h"] {
        assert_eq!(
            scheme_of(&module, name).ty,
            Type::Dyn(Rep::Float),
            "`{name}` must stay `Dyn<Float>`"
        );
    }
    // §4.9.4: `Dyn` is not a `Scalar`; mixing them needs `check` first.
    expect_error("let x = dyn(1.0 m) + 1.0 m\n", "E0302");
    expect_error("let x = 2.0 * dyn(1.0 m)\n", "E0302");
    // The runtime exponent of `^` is an `Int`, and `sqrt` of a `Dyn` is one.
    expect_error("let x = dyn(1.0 m) ^ 0.5\n", "E0302");
}

#[test]
fn check_with_is_the_generic_form() {
    // §4.9.4: generic code passes the runtime dimension, and `check_with`
    // answers with the *dimensionless* scalar.
    let module = expect_ok(
        r#"fn f<d>(dv: DimValue, x: Real[d]): Result<Real, DimError> =
    check_with(dv, dyn_with(x, dv))
"#,
    );
    let scheme = scheme_of(&module, "f");
    // `f : ∀d. (DimValue, Real[d]) -> Result<Real, DimError>` — the second
    // argument's dimension is the quantified parameter, so compare against
    // the scheme's own variable rather than assuming its id.
    assert_eq!(scheme.dims.len(), 1);
    let dim_value = aevia_types::builtins::dim_value_type();
    let dim_error = Type::Adt(
        aevia_types::adt::intern(aevia_types::adt::Adt {
            name: "DimError".to_string(),
            params: Vec::new(),
            ctors: Vec::new(),
        }),
        Vec::new(),
    );
    let expected = Type::func(
        vec![dim_value, Type::real(aevia_dim::Dim::var(scheme.dims[0]))],
        Type::result(Type::real(aevia_dim::Dim::one()), dim_error),
    );
    assert_eq!(scheme.ty, expected);
}

// --- unit/value namespace, §8.4 and §6.10 builtins -------------------------

#[test]
fn a_unit_name_does_not_shadow_a_builtin_in_call_position() {
    // §1.8.2: the unit namespace is separate from the value namespace, so `at`
    // is the unit (technical atmosphere) in `3 at` and the §8.4 indexing
    // builtin in `at(t, 0)`. The unit used to win and `at(t, 0)` was
    // `E0305 Unit[...] is not a function`.
    let module = expect_ok(
        r#"let pressure = 3 at
let t = [[1.0, 2.0], [3.0, 4.0]]
let row = at(t, 0)
let v = at([1.0, 2.0], 1)
"#,
    );
    assert_eq!(
        scheme_of(&module, "row").ty,
        // The rank-2 overload returns a rank-1 tensor; a builtin scheme cannot
        // carry the refined remainder of `Tensor<n-1, τ>`.
        Type::tensor(Shape::Rank(1), Type::float(aevia_dim::Dim::one()))
    );
    assert_eq!(
        scheme_of(&module, "v").ty,
        Type::float(aevia_dim::Dim::one())
    );
}

#[test]
fn set_and_axis_sum_are_rank_polymorphic() {
    // §3.5.5/§8.4: `set : (Tensor<n,a>, Tensor<1,Int>, a) -> Tensor<n,a>` and
    // `sum(a, axis)` keeps the rank (keepdims).
    let module = expect_ok(
        r#"let m = set([[1.0, 2.0], [3.0, 4.0]], [0, 1], 9.0)
let v = set([1.0, 2.0], [0], 9.0)
fn rows(m: Tensor<2, Float>): Tensor<2, Float> = sum(m, 1)
fn total(x: Tensor<1, Float>): Float = sum(x)
"#,
    );
    assert_eq!(
        scheme_of(&module, "m").ty,
        Type::tensor(Shape::Rank(2), Type::float(aevia_dim::Dim::one()))
    );
    assert_eq!(
        scheme_of(&module, "v").ty,
        Type::tensor(Shape::Rank(1), Type::float(aevia_dim::Dim::one()))
    );
    assert_eq!(
        scheme_of(&module, "rows").ty,
        Type::func(
            vec![Type::tensor(
                Shape::Rank(2),
                Type::float(aevia_dim::Dim::one())
            )],
            Type::tensor(Shape::Rank(2), Type::float(aevia_dim::Dim::one()))
        )
    );
}

#[test]
fn par_map_requires_a_parallel_element_type() {
    // §6.10/§3.8.4: `par_map(f, xs)` needs `Parallel a`; `Int`/`Float` and
    // tensors of them have the instance, symbolic `Real` does not.
    let module = expect_ok(
        r#"fn go(xs: Tensor<1, Float>): Tensor<1, Float> = par_map(fn (x) => x * 2.0, xs)
fn up(xs: Tensor<1, Int>): Tensor<1, Int> = par_map(fn (x) => x + 1, xs)
"#,
    );
    let float = Type::float(aevia_dim::Dim::one());
    assert_eq!(
        scheme_of(&module, "go").ty,
        Type::func(
            vec![Type::tensor(Shape::Rank(1), float.clone())],
            Type::tensor(Shape::Rank(1), float.clone())
        )
    );
    expect_error(
        "fn no(xs: Tensor<1, Real>): Tensor<1, Real> = par_map(fn (x) => x, xs)\n",
        "E0310",
    );
    // `rng(seed)` is the pure generator; the value is the next-number function.
    let module = expect_ok("let next = rng(42)\nlet x: Float = next()\n");
    assert_eq!(
        scheme_of(&module, "next").ty,
        Type::func(vec![Type::Unit], Type::float(aevia_dim::Dim::one()))
    );
    // And the no-argument call convention still accepts the unit explicitly.
    expect_ok("let next = rng(42)\nlet x: Float = next(())\n");
}

// --- §9.6.1 @jit, §13.8.1 stability, §13.5.2 @op/@rule ---------------------

fn check_with_capabilities(
    source: &str,
    capabilities: Capabilities,
) -> (Vec<String>, Vec<String>, CheckedModule) {
    use aevia_syntax::Severity;
    let lexed = lex(0, source).expect("the source lexes");
    let (module, parse_diagnostics) = parse_module(0, source, &lexed);
    assert!(
        !parse_diagnostics.has_errors(),
        "parse errors: {parse_diagnostics:?}"
    );
    let result = aevia_types::check::check_module_with_options(
        0,
        &module,
        &[],
        &CheckOptions { capabilities },
    );
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for item in &result.diagnostics.items {
        match item.severity {
            | Severity::Error => errors.push(item.code.clone()),
            | Severity::Warning => warnings.push(item.code.clone()),
            | Severity::Note => {},
        }
    }
    (errors, warnings, result.module)
}

#[test]
fn jit_needs_the_core_jit_capability() {
    // Check the missing-capability case independent of Cargo feature unification.
    let (errors, _, _) = check_with_capabilities(
        "@jit\nfn hot(x: Float): Float = x * 2.0\n",
        Capabilities::from_names(std::iter::empty::<&str>()),
    );
    assert!(errors.iter().any(|code| code == "E1001"), "{errors:?}");
    // With the capability present it is accepted (the lowering hook is the
    // IR's job, §9.6.2).
    let (errors, _, module) = check_with_capabilities(
        "@jit\nfn hot(x: Float): Float = x * 2.0\n",
        Capabilities::all(),
    );
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    assert!(module.exports.contains_key("hot"));
    // The gate is a capability, not a keyword: `E1001` names it.
    let lexed = lex(0, "@jit\nfn hot(x: Float): Float = x * 2.0\n").expect("lexes");
    let (module, _) = parse_module(0, "@jit\nfn hot(x: Float): Float = x * 2.0\n", &lexed);
    let result = aevia_types::check::check_module_with_options(
        0,
        &module,
        &[],
        &CheckOptions {
            capabilities: Capabilities::from_names(std::iter::empty::<&str>()),
        },
    );
    let detail = result
        .diagnostics
        .items
        .iter()
        .find(|item| item.code == "E1001")
        .expect("E1001");
    assert!(
        detail.message.contains("core.jit"),
        "the diagnostic must name the capability: {}",
        detail.message
    );
    // Placement and arguments are `E0210` (appendix B: an attribute's
    // arguments are checked).
    expect_any_error("@jit\nlet x = 1\n", "E0210");
    expect_any_error("@jit(true)\nfn f(x: Float): Float = x\n", "E0210");
}

#[test]
fn stability_attributes_are_recorded_and_deprecation_warns() {
    // §13.8.1: `@stable`/`@experimental`/`@deprecated` are markers, and a
    // deprecated name has a warning period — using it warns (`W0602`).
    let source = r#"@deprecated("use new_api")
fn old_api(x: Int): Int = x
@stable
fn new_api(x: Int): Int = x
@experimental
fn trial(x: Int): Int = x
fn caller(): Int = old_api(1)
"#;
    let (errors, warnings, module) = check_with_capabilities(source, Capabilities::all());
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    assert!(
        warnings.iter().any(|code| code == "W0602"),
        "expected a deprecation warning, got {warnings:?}"
    );
    assert_eq!(
        module.stability.get("old_api"),
        Some(&Stability::Deprecated("use new_api".to_string()))
    );
    assert_eq!(module.stability.get("new_api"), Some(&Stability::Stable));
    assert_eq!(
        module.stability.get("trial"),
        Some(&Stability::Experimental)
    );
    // An invalid argument is `E0210`, per appendix B's "check the name/args".
    expect_any_error("@deprecated(1)\nfn f(x: Int): Int = x\n", "E0210");
    expect_any_error("@stable(\"x\")\nfn f(x: Int): Int = x\n", "E0210");
}

#[test]
fn op_and_rule_report_the_missing_plugin_abi_or_register() {
    // §13.5.2: `@op` publishes an Aevia function as a kernel definition and
    // `@rule` as a rewrite. The manifest's `core.plugin.source` row says
    // whether the build can; without it the attribute is `E1001` naming it,
    // never a silent no-op.
    let source = "@op(\"mydomain/softplus\")\nfn softplus(x: Float): Float = x\n";
    let (errors, _, _) = check_with_capabilities(source, Capabilities::from_names(["core.jit"]));
    assert!(
        errors.iter().any(|error| error == "E1001"),
        "the diagnostic must name the capability: {errors:?}"
    );
    // This build has in-process plugins, so the default options accept it.
    let lexed = lex(0, source).expect("lexes");
    let (module, _) = parse_module(0, source, &lexed);
    let result = check_module(0, &module);
    assert!(
        result
            .diagnostics
            .items
            .iter()
            .all(|item| item.code != "E1001"),
        "{:?}",
        result.diagnostics.items
    );
    let (errors, _, module) = check_with_capabilities(
        r#"@op("mydomain/softplus")
fn softplus(x: Float): Float = x
@rule("simplify_softplus")
fn simplify_softplus(x: Float): Float = x
"#,
        Capabilities::all(),
    );
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    assert_eq!(module.registrations.len(), 2);
    assert!(module.registrations.iter().any(|registration| {
        registration.kind == HostRegistrationKind::Op
            && registration.name == "mydomain/softplus"
            && registration.function == "softplus"
    }));
    assert!(
        module
            .registrations
            .iter()
            .any(|registration| registration.kind == HostRegistrationKind::Rule)
    );
    // The argument must be a compile-time string.
    expect_any_error("@op(1)\nfn f(x: Float): Float = x\n", "E0210");
}

// --- §3.5.1 / §13.9.1 shape type arguments ---------------------------------

#[test]
fn reshape_reads_its_shape_type_argument() {
    // §3.5.1: `reshape<[n1, ..., nk]>(a)` always returns a `Result`, and the
    // target shape is static. This used to be a parse error because the
    // expression's `<…>` list was parsed as types only.
    let shape_error = Type::Adt(
        aevia_types::adt::intern(aevia_types::adt::Adt {
            name: "ShapeError".to_string(),
            params: Vec::new(),
            ctors: Vec::new(),
        }),
        Vec::new(),
    );
    let float = Type::float(aevia_dim::Dim::one());
    let module = expect_ok("let r = reshape<[4]>([[1.0, 2.0], [3.0, 4.0]])\n");
    assert_eq!(
        scheme_of(&module, "r").ty,
        Type::result(
            Type::tensor(Shape::refined([Some(4)]), float.clone()),
            shape_error.clone()
        )
    );
    // §13.9.1: `zeros_of<[3, 4]>()` names the shape it builds.
    let module = expect_ok("let z = zeros_of<[3, 4]>()\n");
    assert_eq!(
        scheme_of(&module, "z").ty,
        Type::tensor(Shape::refined([Some(3), Some(4)]), float.clone())
    );
    // A statically-known element count must be preserved (§3.5.1): 4 elements
    // cannot fill a `[3, 2]` tensor, so this is `E0802` at check time.
    expect_error(
        "let r = reshape<[3, 2]>([[1.0, 2.0], [3.0, 4.0]])\n",
        "E0802",
    );
    // A rank-only source has no static count, so the runtime checks it and the
    // declaration is accepted.
    expect_ok(
        "fn f(a: Tensor<2, Float>): Result<Tensor<[3, 2], Float>, ShapeError> = reshape<[3, 2]>(a)\n",
    );
    expect_error("let r = reshape<[3, 2]>(1.0)\n", "E0302");
    // The spelled-out tensor type still names the shape (the spelling the
    // runtime tests use).
    expect_ok("let r = reshape<Tensor<[2, 3], Float>>([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]])\n");
}

// --- §3.10 literal representations agree with the static type -------------

#[test]
fn a_literal_is_exported_at_the_representation_its_type_names() {
    // §3.10.1/§3.10.3: a literal is representation-polymorphic and takes the
    // representation its static type names. The checker exports that choice in
    // `literal_reps` so the runtime can materialise the literal at it — without
    // that, `fn f(x: Float): Float = x + 1` type-checks but `f(2.0)` answers
    // the exact `Real` `3`, and a statically `Float` value holds an exact one.
    let source = r#"fn f(x: Float): Float = x + 1
let y: Float = 1
let z: Real = 1
let w = 1
"#;
    let (codes, module) = check(source);
    assert!(codes.is_empty(), "unexpected errors: {codes:?}");
    let mut reps: Vec<Rep> = module.literal_reps.values().copied().collect();
    reps.sort_by_key(|rep| rep.name());
    // `x + 1` and `y`: Float. `z`: Real. `w`: Int.
    assert!(reps.contains(&Rep::Float), "found {reps:?}");
    assert!(reps.contains(&Rep::Real), "found {reps:?}");
    assert!(reps.contains(&Rep::Int), "found {reps:?}");
    assert_eq!(
        module.literal_reps.len(),
        4,
        "every literal has an entry: {reps:?}"
    );
    // The no-coercion side: `Real` does not become `Float` (§3.4.1). A
    // `Float`-annotated literal is `Float`, so the static and runtime
    // representations are the same choice.
    let (codes, _) = check("let a: Float = 1.5 + 1.0\nlet b: Real = 1.5 + 1\n");
    assert!(codes.is_empty(), "unexpected errors: {codes:?}");
}

#[test]
fn the_simulate_boundary_is_explicit_json() {
    // §8.17: `simulate : (String, Json) -> Result<Json, SimError>`; a typed
    // wrapper encodes with `to_json` and decodes with `from_json`, so no
    // `∀a.a` value is needed at the boundary.
    let module = expect_ok(
        r#"type P = { n: Int, beta: Float }
type R = { energy: Float }
fn run(p: P): Result<R, SimError> =
    match from_json(to_json(p)) with
    | Ok(j) -> match simulate("ising", j) with
        | Ok(out) -> from_json(out)
        | Err(e) -> Err(e)
    | Err(e) -> Err(e)
"#,
    );
    assert!(module.exports.contains_key("run"));
    // The boundary type can be named in a signature.
    expect_ok("fn encode(p: Int): Json = to_json(p)\n");
}

#[test]
fn a_builtin_scheme_is_instantiated_freshly_each_time() {
    // `on` and `odeint_with` used `var(1)` while quantifying only `var(0)`, so
    // the variable was shared between instantiations and the second declaration
    // that used the builtin was a spurious E0303 (bridge-fixer's repro). The
    // `scheme` helper now asserts full quantification in test builds.
    expect_ok("let first = on\nlet sin = sin\nlet second = on\n");
    expect_ok("let first = odeint_with\nlet sin = sin\nlet second = odeint_with\n");
    expect_ok("let a = solve_ivp\nlet b = solve_ivp\n");
}

// --- §3.5.1 type-level Nat: literals, `+`, `*`, linear equations -----------

#[test]
fn a_linear_nat_equation_solves_the_extent() {
    // `n + 1 ~ 2` ⇒ `n = 1`: the parameter's extent is `2` afterwards, and the
    // solution is concrete, so the scheme has no Nat variables left.
    let module = expect_ok("fn f<n>(a: Tensor<[n + 1], Float>): Tensor<[2], Float> = a\n");
    let scheme = scheme_of(&module, "f");
    assert!(scheme.nats.is_empty(), "n is solved: {scheme:?}");
    let two = Type::tensor(
        Shape::refined([Some(2)]),
        Type::float(aevia_dim::Dim::one()),
    );
    assert_eq!(scheme.ty, Type::func(vec![two.clone()], two));
}

#[test]
fn a_scaled_nat_equation_solves_the_extent() {
    // `2 * n ~ 6` ⇒ `n = 3`; the coefficient divides exactly.
    let module = expect_ok("fn f<n>(a: Tensor<[2 * n], Float>): Tensor<[6], Float> = a\n");
    let scheme = scheme_of(&module, "f");
    assert!(scheme.nats.is_empty(), "n is solved: {scheme:?}");
    let six = Type::tensor(
        Shape::refined([Some(6)]),
        Type::float(aevia_dim::Dim::one()),
    );
    assert_eq!(scheme.ty, Type::func(vec![six.clone()], six));
}

#[test]
fn an_inconsistent_nat_equation_is_e0302() {
    // `n ~ n + 1` has no solution; the residual is the constant `1`.
    expect_error(
        "fn f<n>(a: Tensor<[n], Float>): Tensor<[n + 1], Float> = a\n",
        "E0302",
    );
    // `2 * n ~ 3` has no integer solution.
    expect_error(
        "fn f<n>(a: Tensor<[2 * n], Float>): Tensor<[3], Float> = a\n",
        "E0302",
    );
}

#[test]
fn a_non_linear_nat_equation_is_e0304() {
    // §3.5.1 only guarantees linear equations; `n * m` is not linear.
    expect_error(
        "fn f<n, m>(a: Tensor<[n * m], Float>): Tensor<[2], Float> = a\n",
        "E0304",
    );
}

#[test]
fn nat_variables_generalise_and_instantiate_freshly() {
    // `id_t : ∀n. Tensor<[n], Float> -> Tensor<[n], Float>`; two calls with
    // different shapes must not share the variable.
    let module = expect_ok(
        r#"fn id_t<n>(x: Tensor<[n], Float>): Tensor<[n], Float> = x
let a = id_t([1.0, 2.0])
let b = id_t([1.0, 2.0, 3.0])
"#,
    );
    let scheme = scheme_of(&module, "id_t");
    assert_eq!(scheme.nats.len(), 1, "n is quantified: {scheme:?}");
    let nat = aevia_types::Nat::var(scheme.nats[0]);
    assert_eq!(
        scheme.ty,
        Type::func(
            vec![Type::tensor(
                Shape::Refined(vec![Some(nat.clone())]),
                Type::float(aevia_dim::Dim::one())
            )],
            Type::tensor(
                Shape::Refined(vec![Some(nat)]),
                Type::float(aevia_dim::Dim::one())
            )
        )
    );
}

#[test]
fn reshape_and_zeros_of_propagate_nat_extents() {
    // §3.5.1: `reshape`'s element count is a Nat equation. Here `2 * n = 6`
    // solves `n = 3`, so the source shape is `[6]` and the result matches.
    let module = expect_ok(
        "fn f<n>(a: Tensor<[2 * n], Float>): Result<Tensor<[6], Float>, ShapeError> =\n    reshape<[6]>(a)\n",
    );
    assert!(scheme_of(&module, "f").nats.is_empty(), "n solved");
    // `zeros_of<[2 * 3]>()` folds to `[6]`.
    let module = expect_ok("let z = zeros_of<[2 * 3]>()\n");
    assert_eq!(
        scheme_of(&module, "z").ty,
        Type::tensor(
            Shape::refined([Some(6)]),
            Type::float(aevia_dim::Dim::one())
        )
    );
    // A polymorphic target keeps its variable: `zeros<n>` has one Nat parameter.
    let module = expect_ok(
        "fn zeros<n>(m: Tensor<[n], Float>): Tensor<[2 * n], Float> = zeros_of<[2 * n]>()\n",
    );
    assert_eq!(scheme_of(&module, "zeros").nats.len(), 1);
}

#[test]
fn matmul_unifies_its_shared_extent() {
    // §3.5.3.1 with refined shapes and Nat variables: `[a, b] @ [b, c]`.
    let module = expect_ok(
        "fn mm<a, b, c>(x: Tensor<[a, b], Float>, y: Tensor<[b, c], Float>): Tensor<[a, c], Float> = x @ y\n",
    );
    let scheme = scheme_of(&module, "mm");
    assert_eq!(
        scheme.nats.len(),
        3,
        "a, b and c are quantified: {scheme:?}"
    );
    // A concrete mismatch is `E0802` (shape error), as before.
    expect_error(
        "fn bad(a: Tensor<[2, 3], Float>, b: Tensor<[4, 5], Float>): Tensor<[2, 5], Float> = a @ b\n",
        "E0802",
    );
}

#[test]
fn an_undeclared_shape_variable_is_e0304() {
    // A shape variable needs a binder: `<n>` or an enclosing `fn` parameter.
    expect_error("let x: Tensor<[n], Float> = [1.0]\n", "E0304");
}

#[test]
fn an_instance_head_cannot_quantify_a_nat_variable() {
    // §3.5.1: there is no per-instance Nat binder, so `Tensor<[n], Float]`
    // cannot name an instance head (it would also make the runtime witness
    // ambiguous). Rank-only heads stay legal.
    expect_error(
        "class Sized_ a where\n    size_of(x: a): Int\ninstance Sized_ for Tensor<[n], Float> where\n    size_of(x) = 0\n",
        "E0304",
    );
    expect_ok(
        "class Sized_ a where\n    size_of(x: a): Int\ninstance Sized_ for Tensor<2, Float> where\n    size_of(x) = 0\n",
    );
}

// --- §8.9.1 SDE: DiffusionOf, sdeint_*, wiener, sde_order ----------------

#[test]
fn diffusion_of_divides_the_dimension_by_sqrt_time() {
    // `DiffusionOf<Float[L]>` is `Float[L / T^(1/2)]`: `sqrt(s / t)` has
    // exactly that dimension, and the annotation is a built-in type function.
    expect_ok("fn noise(t: Float[T], s: Float[L]): DiffusionOf<Float[L]> = sqrt(s * s / t)\n");
    // The Brownian scaling is on the *state*, not an arbitrary choice: plain
    // `sqrt(s)` is `L^(1/2)`, not `L / T^(1/2)`.
    expect_error(
        "fn noise(t: Float[T], s: Float[L]): DiffusionOf<Float[L]> = sqrt(s)\n",
        "E0401",
    );
    // It recurses through records, exactly like `RateOf`.
    expect_ok("fn g(d: DiffusionOf<{ p: Float[L] }>): { p: Float[L / T^(1/2)] } = d\n");
    // A non-numeric state field is rejected by the type function.
    expect_error(
        "fn bad(t: Float[T], n: Int): DiffusionOf<Int> = 0\n",
        "E0302",
    );
}

#[test]
fn sde_entry_points_are_typed_through_rate_of_and_diffusion_of() {
    let source = r#"fn drift(t: Float[T], s: Float[L]): RateOf<Float[L]> = 1.0 m/s
fn noise(t: Float[T], s: Float[L]): DiffusionOf<Float[L]> = sqrt(s * s / t)
let p = sdeint_diag(drift, noise, 1.0 m, { method = "Milstein" })
let path: Result<{ t: Tensor<1, Float[T]>, y: Tensor<1, Float[L]>, steps: Int, rejected: Int, nfev: Int, seed: Int, noise: Int, method: SdeMethod }, SdeError> = p
"#;
    let module = expect_ok(source);
    let scheme = scheme_of(&module, "path");
    assert!(
        format!("{}", scheme.ty).contains("SdeError"),
        "the result names SdeError: {}",
        scheme.ty
    );
    // The drift's return type is checked through RateOf, not by the HM scheme.
    expect_error(
        "fn drift(t: Float[T], s: Float[L]): Float[L] = 1.0 m\nfn noise(t: Float[T], s: Float[L]): DiffusionOf<Float[L]> = sqrt(s * s / t)\nlet p = sdeint_diag(drift, noise, 1.0 m, { method = \"Milstein\" })\n",
        "E0401",
    );
    // `sdeint_general` takes a record state too: its diffusion is a record of the same shape
    // with a noise axis on every leaf (`GeneralDiffusionOf<s>`), not the diagonal
    // `DiffusionOf<s>`.
    expect_ok(
        "fn drift(t: Float[T], s: { p: Float[L] }): RateOf<{ p: Float[L] }> = { p = 1.0 m/s }
fn noise(t: Float[T], s: { p: Float[L] }): { p: Tensor<1, Float[L / T^(1/2)]> } = { p = [sqrt(1.0 m^2 / t), sqrt(2.0 m^2 / t)] }
let p = sdeint_general(drift, noise, { p = 1.0 m }, { method = \"Milstein\" })
",
    );
    expect_error(
        "fn drift(t: Float[T], s: { p: Float[L] }): RateOf<{ p: Float[L] }> = { p = 1.0 m/s }
fn noise(t: Float[T], s: { p: Float[L] }): DiffusionOf<{ p: Float[L] }> = { p = sqrt(1.0 m^2 / t) }
let p = sdeint_general(drift, noise, { p = 1.0 m }, { method = \"Milstein\" })
",
        "E0302",
    );
    // An `Int` state field is still `E0302`.
    expect_error(
        "fn drift(t: Float[T], s: { n: Int }): Int = 0
fn noise(t: Float[T], s: { n: Int }): Int = 0
let p = sdeint_general(drift, noise, { n = 1 }, { method = \"Milstein\" })
",
        "E0302",
    );
}

#[test]
fn sde_helper_names_have_their_specified_types() {
    let module = expect_ok(
        r#"let order = sde_order("Milstein")
let inc = wiener(1, 2, 10, 0.1 s)
let seed = sde_ensemble_seed(1, 2)
"#,
    );
    let order = scheme_of(&module, "order");
    assert!(
        format!("{}", order.ty).contains("strong"),
        "sde_order returns strong/weak: {}",
        order.ty
    );
    let half = num_rational::BigRational::new(1.into(), 2.into());
    let sqrt_time = aevia_dim::Dim::base("Time").pow(&half);
    assert_eq!(
        scheme_of(&module, "inc").ty,
        Type::tensor(Shape::rank_only(2), Type::float(sqrt_time))
    );
    assert_eq!(scheme_of(&module, "seed").ty, Type::int());
}

// --- §11.2/§11.4 conformance regressions (task-16) ------------------------

#[test]
fn rate_of_recurses_through_records_and_tensors() {
    // §8.9: `RateOf<State>` divides every leaf's dimension by `T`, through
    // records and tensors. A two-field position/velocity state is the §11.2
    // shape; getting the direction wrong (multiplying, or double-dividing)
    // reports E0401 at the `rhs` argument.
    expect_ok(
        r#"type State = { x: Float[L], v: Float[L/T] }
type Rate = { x: Float[L/T], v: Float[L/T^2] }
fn rhs(_t: Float[T], s: State): Rate = { x = s.v, v = -9.80665 m/s^2 }
fn go(s0: State): Tensor<1, State> = odeint(rhs, s0, [0.0 s, 1.0 s])
"#,
    );
    // Nested tensor + record-in-record.
    expect_ok(
        r#"type Nested = { pos: Tensor<1, Float[L]>, tail: { speed: Float[L/T] } }
type NRate = { pos: Tensor<1, Float[L/T]>, tail: { speed: Float[L/T^2] } }
fn nrhs(_t: Float[T], s: Nested): NRate =
    { pos = s.pos / (1.0 s), tail = { speed = 1.0 m/s^2 } }
fn ngo(s0: Nested): Tensor<1, Nested> = odeint(nrhs, s0, [0.0 s, 1.0 s])
"#,
    );
    // Returning the state's own dimensions (forgetting the time derivative)
    // must stay E0401: the check exists to prevent a silently wrong integral.
    expect_error(
        r#"type State = { x: Float[L], v: Float[L/T] }
fn wrong(_t: Float[T], s: State): State = s
fn go(s0: State): Tensor<1, State> = odeint(wrong, s0, [0.0 s, 1.0 s])
"#,
        "E0401",
    );
}

#[test]
fn an_imported_odeint_keeps_the_rate_of_typing() {
    // §11.2 imports `odeint` from the `Ode` module, which re-exports the
    // builtin with its loose HM scheme. The type-function typing must still
    // apply: an imported entry point is not a *local* shadow.
    use aevia_types::builtins;
    use aevia_types::check::Export;
    use aevia_types::check::check_module_with_imports;
    let source = r#"type State = { x: Float[L], v: Float[L/T] }
type Rate = { x: Float[L/T], v: Float[L/T^2] }
fn rhs(_t: Float[T], s: State): Rate = { x = s.v, v = -9.80665 m/s^2 }
fn go(s0: State): Tensor<1, State> = odeint(rhs, s0, [0.0 s, 1.0 s])
"#;
    let mut exports = indexmap::IndexMap::new();
    exports.insert(
        "odeint".to_string(),
        Export::Value {
            scheme: builtins::schemes("odeint").expect("odeint is a builtin")[0].clone(),
            decl: 0,
            arity: 3,
        },
    );
    let imports = vec![("Ode".to_string(), exports)];
    let lexed = lex(0, source).expect("lexes");
    let (module, parse_diags) = parse_module(0, source, &lexed);
    assert!(parse_diags.items.is_empty(), "{:#?}", parse_diags.items);
    let checked = check_module_with_imports(0, &module, &imports);
    assert!(
        checked.diagnostics.items.is_empty(),
        "{:#?}",
        checked
            .diagnostics
            .items
            .iter()
            .map(|item| (item.code.clone(), item.message.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_refined_shape_unifies_with_the_same_rank() {
    // §3.5.1/§3.5.3/§11.4: `v * i` mixes a refinated extent and a rank-only
    // tensor of the same rank. Same-rank is enough; the extent is unknown on
    // one side. Both operand orders must work.
    expect_ok(
        r#"let a: Tensor<[2], Float[L]> = [1.0 m, 2.0 m]
let b: Tensor<1, Float[T]> = [0.1 s, 0.2 s]
let p = a * b
let q = b * a
let r: Tensor<[2, 2], Float[L]> = [[1.0, 2.0], [3.0, 4.0]] * m
let u: Tensor<1, Float[T]> = [0.1 s, 0.2 s]
let v = r @ u
"#,
    );
    // Different known extents are still a mismatch (§11.4 regression guard).
    expect_error(
        r#"let a: Tensor<[2], Float> = [1.0, 2.0]
let b: Tensor<[3], Float> = [1.0, 2.0, 3.0]
let c = a * b
"#,
        "E0302",
    );
    // Rank mismatch is still a mismatch.
    expect_error(
        r#"let a: Tensor<1, Float> = [1.0, 2.0]
let b: Tensor<2, Float> = [[1.0, 2.0], [3.0, 4.0]]
let c = a * b
"#,
        "E0302",
    );
}

// --- §2/task-20: kind annotations and inference on generic declarations -----

#[test]
fn a_dimension_kinded_alias_is_inferred_from_its_fields() {
    // §2/§8.7.1: the kind annotation may be omitted; `Real[d]` binds `d` as a
    // dimension. This is the `Normal<d>` shape of the standard library.
    expect_ok(
        r#"type Normal<d> = { mu: Real[d], sigma: Real[d] }
let n: Normal<L> = { mu = 1.0 m, sigma = 0.5 m }
fn spread(n: Normal<L>): Real[L] = n.sigma
"#,
    );
    // An explicit annotation is accepted too.
    expect_ok(
        r#"type Shift<d: Dim> = { x: Float[d] }
let s: Shift[L] = { x = 2.0 m }
"#,
    );
}

#[test]
fn a_reference_kinded_alias_records_are_well_formed_11_2_style() {
    // §11.2 needed exactly this: a record alias with dimension-kinded
    // parameters inside `RateOf`-style states.
    expect_ok(
        r#"type State2<d> = { x: Float[d], v: Float[d/T] }
fn step<d>(s: State2<d>): State2<d> = s
"#,
    );
}

#[test]
fn a_nat_kinded_alias_parameter_names_a_shape_extent() {
    // `<n: Nat>` with `Tensor<[n], Float>`; the argument is a natural number.
    expect_ok(
        r#"type RowOf<n: Nat> = { data: Tensor<[n], Float> }
let r: RowOf<3> = { data = [1.0, 2.0, 3.0] }
"#,
    );
}

#[test]
fn kind_inference_tracks_point_and_tensor_extents() {
    // `Point[d]` and `Tensor<1, Float[d]>` both bind `d` as a dimension.
    expect_ok(
        r#"type Affine<d> = { p: Point[d] }
let a: Affine[Θ] = { p = point(300.0, K) }
type PlaneElt<d> = { v: Tensor<1, Float[d]> }
let v: PlaneElt[L] = { v = [1.0 m, 2.0 m] }
"#,
    );
}

#[test]
fn a_row_kinded_alias_parameter_is_a_record_tail() {
    expect_ok(
        r#"type With<r: Row> = { x: Int, ...r }
type Point2 = { y: Int }
let w: With<Point2> = { x = 1, y = 2 }
"#,
    );
}

#[test]
fn conflicting_kind_uses_are_rejected() {
    // The same parameter cannot be a dimension in one field and a shape extent
    // in another; the user must annotate the intended kind.
    expect_error(
        "type Bad<d> = { a: Float[d], b: Tensor<[d], Float> }\n",
        "E0317",
    );
}

#[test]
fn ordinary_type_parameters_still_work_on_aliases() {
    expect_ok(
        r#"type Pair<a, b> = { f: a, s: b }
let p: Pair<Int, String> = { f = 1, s = "x" }
type Box<a> = { v: a }
"#,
    );
    // Deriving on an instantiated parameterised record alias still behaves.
    expect_ok("type Box<a> = { v: a }\nderive Eq, Show for Box<Int>\n");
    // An unused type parameter is legal (`type Phantom<a> = Int` is the
    // standard phantom-parameter idiom); no diagnostic is invented for it.
    expect_ok("type Phantom<a> = { x: Int }\n");
}

#[test]
fn a_dimension_kinded_variant_type_carries_its_dimension() {
    // §8.9.1's `SdeOpts<T> = | Fixed(SdeFixedOpts<T>) | …`: a variant type may
    // be parameterised by a dimension; construction and matching unify the
    // dimension through the encoded `UnitOf(d)` argument.
    expect_ok(
        r#"type FixedOpts<d> = { t0: Float[d] }
type Opts<d> = | Fixed(FixedOpts<d>) | Adaptive(Int)
let f = Fixed({ t0 = 1.0 s })
let g: Opts[T] = f
fn which<d>(o: Opts<d>): Int = match o with
    | Fixed(_) -> 1
    | Adaptive(n) -> n
"#,
    );
}

// --- §2.2.7: attribute arguments are compile-time constants ----------------

#[test]
fn attribute_arguments_must_be_compile_time_constants() {
    // Literals, constant arithmetic, quantity literals and bare names (which
    // may denote a `let` constant) are accepted.
    expect_ok("@bench(1 + 2)\nfn timed(): Int = 1\n");
    expect_ok("@bench(2 m)\nfn timed(): Int = 1\n");
    expect_ok("let iterations = 8\n@bench(iterations)\nfn timed(): Int = 1\n");
    // A call, an interpolated string or a field access is not constant.
    expect_error("@bench(measure())\nfn timed(): Int = 1\n", "E0210");
    expect_error("@bench(\"{iterations}\")\nfn timed(): Int = 1\n", "E0210");
    expect_error("@bench(1 + measure())\nfn timed(): Int = 1\n", "E0210");
}

// --- nullary constructors in patterns (§3.6.3 / runtime E0309 regression) --

#[test]
fn nullary_constructors_type_check_in_patterns() {
    expect_ok(
        r#"type Color = | Red | Blue
fn pick(c: Color): Int = match c with
    | Red -> 1
    | Blue -> 2
let x = pick(Red)
let stored: Option<Color> = Some(Blue)
fn nested(o: Option<Color>): Int = match o with
    | Some(Red) -> 1
    | Some(Blue) -> 2
    | None -> 0
"#,
    );
}

// --- §6.6 exhaustiveness/usefulness hardening ------------------------------

#[test]
fn nested_payload_patterns_are_checked_for_exhaustiveness() {
    // Every `Option<Option<Color>>` shape is covered: exhaustive.
    expect_ok(
        r#"type Color = | Red | Blue
fn f(x: Option<Option<Color>>): Int = match x with
    | Some(Some(Red)) -> 1
    | Some(Some(Blue)) -> 2
    | Some(None) -> 3
    | None -> 4
"#,
    );
    // `Some(None)` is missed at the nested level.
    expect_error(
        r#"type Color = | Red | Blue
fn f(x: Option<Option<Color>>): Int = match x with
    | Some(Some(Red)) -> 1
    | Some(Some(Blue)) -> 2
    | None -> 4
"#,
        "E0309",
    );
    // A payload constructor used at the wrong depth does not count.
    expect_error(
        r#"type Color = | Red | Blue
fn f(x: Option<Color>): Int = match x with
    | Some(Red) -> 1
"#,
        "E0309",
    );
}

#[test]
fn bool_and_literal_patterns_are_analysed() {
    expect_ok("fn f(b: Bool): Int = match b with\n    | true -> 1\n    | false -> 2\n");
    // An integer literal does not exhaust an infinite domain.
    expect_error("fn f(n: Int): Int = match n with\n    | 0 -> 1\n", "E0309");
    expect_ok("fn f(n: Int): Int = match n with\n    | 0 -> 1\n    | _ -> 2\n");
    expect_error(
        "fn f(b: Bool): Int = match b with\n    | true -> 1\n",
        "E0309",
    );
    // A guard does not count as coverage.
    expect_error(
        "fn f(b: Bool): Int = match b with\n    | true if b -> 1\n    | false -> 2\n",
        "E0309",
    );
}

#[test]
fn record_patterns_are_analysed_by_field() {
    // Closed record: `x` is always read, `y` must be covered both ways.
    expect_ok(
        "fn f(r: { x: Int, y: Bool }): Int = match r with\n    | { x = 0, y = true } -> 1\n    | { x = 0, y = false } -> 2\n    | _ -> 3\n",
    );
    expect_error(
        "fn f(r: { x: Int, y: Bool }): Int = match r with\n    | { x = 0, y = true } -> 1\n    | { x = 0, y = false } -> 2\n",
        "E0309",
    );
}

#[test]
fn unreachable_match_arms_warn() {
    // A duplicate constructor arm.
    expect_warning(
        "type Color = | Red | Blue\nfn f(c: Color): Int = match c with\n    | Red -> 1\n    | Red -> 2\n    | Blue -> 3\n",
        "W0201",
    );
    // A wildcard after an exhaustive set.
    expect_warning(
        "fn f(b: Bool): Int = match b with\n    | true -> 1\n    | false -> 2\n    | _ -> 3\n",
        "W0201",
    );
    // A duplicate literal.
    expect_warning(
        "fn f(n: Int): Int = match n with\n    | 1 -> 1\n    | 1 -> 2\n    | _ -> 0\n",
        "W0201",
    );
}

// --- §3.5.1 Nat solver edges ----------------------------------------------

#[test]
fn a_repeated_nat_variable_gathers_its_coefficient() {
    // `n + n ~ 4` is `2n = 4`, so `n = 2`.
    let module = expect_ok("fn f<n>(a: Tensor<[n + n], Float>): Tensor<[4], Float> = a\n");
    assert!(scheme_of(&module, "f").nats.is_empty(), "n solved");
    assert_eq!(
        scheme_of(&module, "f").ty,
        Type::func(
            vec![Type::tensor(
                Shape::refined([Some(4)]),
                Type::float(aevia_dim::Dim::one())
            )],
            Type::tensor(
                Shape::refined([Some(4)]),
                Type::float(aevia_dim::Dim::one())
            )
        )
    );
}

#[test]
fn a_scaled_nat_variable_solves_on_either_side() {
    expect_ok("fn f<n>(a: Tensor<[2 * n], Float>): Tensor<[6], Float> = a\n");
    expect_ok("fn f<n>(a: Tensor<[n * 2], Float>): Tensor<[6], Float> = a\n");
}

#[test]
fn a_negative_nat_solution_is_rejected() {
    // `n + 2 ~ 1` would need `n = -1`.
    expect_error(
        "fn f<n>(a: Tensor<[n + 2], Float>): Tensor<[1], Float> = a\n",
        "E0302",
    );
}

#[test]
fn very_large_nat_literals_still_solve() {
    // Larger than `i64::MAX`: the linear form is `i128`, so this is a normal
    // equation rather than a spurious "non-linear" E0304.
    expect_ok("fn f<n>(a: Tensor<[n], Float>): Tensor<[15000000000000000000], Float> = a\n");
}

#[test]
fn a_pending_equation_solved_later_is_not_e0304() {
    // `n + m ~ 7` stays pending until `n` is bound by the second `let`, then
    // `m = 4`; the declaration must not report E0304.
    expect_ok(
        "fn f<n, m>(a: Tensor<[n + m], Float>, c: Tensor<[n], Float>): Tensor<[7], Float> =\n    let total: Tensor<[7], Float> = a\n    let three: Tensor<[3], Float> = c\n    total\n",
    );
}

#[test]
fn an_ambiguous_pending_equation_is_e0304() {
    // `n + m ~ 3` has no unique solution and cannot be represented in the
    // scheme: the declaration reports E0304 (§3.10.2 rule 6).
    expect_error(
        "fn f<n, m>(a: Tensor<[n + m], Float>): Tensor<[3], Float> =\n    let total: Tensor<[3], Float> = a\n    total\n",
        "E0304",
    );
}

#[test]
fn an_equation_that_becomes_inconsistent_later_is_e0302() {
    // `n + m ~ 3` is pending; binding `m = 4` leaves `n = -1`, which is
    // impossible. The error is reported where the later binding happens.
    expect_error(
        "fn f<n, m>(a: Tensor<[n + m], Float>, b: Tensor<[m], Float>): Tensor<[3], Float> =\n    let three: Tensor<[3], Float> = a\n    let four: Tensor<[4], Float> = b\n    three\n",
        "E0302",
    );
}

// --- §3.5.3 shape-refinement preservation --------------------------------

#[test]
fn matmul_collapses_to_the_surviving_refined_extent() {
    // Matrix @ vector keeps the row count; vector @ matrix keeps the column
    // count; when the surviving extent is unknown the result is rank-only.
    expect_ok(
        "fn f(a: Tensor<[2, 3], Float>, b: Tensor<[3], Float>): Tensor<[2], Float> = a @ b\n",
    );
    expect_ok(
        "fn f(a: Tensor<[3], Float>, b: Tensor<[3, 4], Float>): Tensor<[4], Float> = a @ b\n",
    );
    expect_ok("fn f(a: Tensor<2, Float>, b: Tensor<1, Float>): Tensor<1, Float> = a @ b\n");
}

#[test]
fn indexing_and_literal_slices_keep_the_refined_remainder() {
    // `[3, 4]` indexed at axis 0 is `[4]`.
    expect_ok("fn f(a: Tensor<[3, 4], Float>): Tensor<[4], Float> = a[1]\n");
    // A literal unit-step slice computes its extent: `[6][1..4]` is `[3]`.
    expect_ok("fn f(a: Tensor<[6], Float>): Tensor<[3], Float> = a[1..4]\n");
    // An out-of-range literal index is still E0802 (static check).
    expect_error("fn f(a: Tensor<[3], Float>): Float = a[7]\n", "E0802");
}

#[test]
fn transpose_swaps_refined_extents() {
    expect_ok("fn f(a: Tensor<[2, 3], Float>): Tensor<[3, 2], Float> = transpose(a)\n");
    expect_error(
        "fn f(a: Tensor<[2, 3], Float>): Tensor<[2, 3], Float> = transpose(a)\n",
        "E0302",
    );
    expect_error("fn f(n: Int): Int = transpose(n)\n", "E0302");
}

// --- task-28: chapter-8 scheme conformance --------------------------------

#[test]
fn division_shaped_builtins_refine_the_result_representation() {
    // §3.4.2/§8.1/§8.4/§8.7: mean/var/std/cov/corr/inv/linsolve/lstsq divide, so
    // an `Int` argument yields the divided (`Real`) representation.
    let module = expect_ok("let m = mean([1, 2, 3])\n");
    assert_eq!(
        scheme_of(&module, "m").ty,
        Type::real(aevia_dim::Dim::one()),
        "mean of Ints is Real"
    );
    expect_ok("fn f(a: Tensor<2, Int>): Result<Tensor<2, Real>, LinAlgError> = inv(a)\n");
    expect_ok("fn f(xs: Tensor<1, Int>): Real = std(xs)\n");
    expect_ok("fn f(xs: Tensor<1, Int>, ys: Tensor<1, Int>): Real = corr(xs, ys)\n");
}

#[test]
fn complex_sqrt_halves_the_dimension() {
    expect_ok("fn f(z: Complex[L^2]): Complex[L] = complex_sqrt(z)\n");
    expect_ok("fn f(z: Complex[L]): Complex[L^(1/2)] = complex_sqrt(z)\n");
}

#[test]
fn rank_and_shape_require_a_tensor() {
    expect_ok("fn f(a: Tensor<2, Float>): Int = rank(a)\n");
    expect_ok("fn f(a: Tensor<1, Float>): Tensor<[1], Int> = shape(a)\n");
    expect_error("fn f(n: Int): Int = rank(n)\n", "E0302");
    expect_error("fn f(n: Int): Tensor<1, Int> = shape(n)\n", "E0302");
}

#[test]
fn measurement_arithmetic_is_typed() {
    // Same dimension add/sub; scaling by a plain number; dimension mismatch is
    // E0401; adding a bare number to a measurement is E0302.
    expect_ok(
        "let a = measure(1.0, 0.1)\nlet b = measure(2.0, 0.2)\nlet s = a + b\nlet d = a - b\nlet k = a * 2.0\nlet q = 2.0 * b\nlet r = a / 2.0\n",
    );
    expect_ok(
        "let p = measure(1.0 m, 0.1 m) * measure(2.0 s, 0.2 s)\nlet v = measure(1.0 m, 0.1 m) / measure(2.0 s, 0.2 s)\n",
    );
    expect_error(
        "let a = measure(1.0 m, 0.1 m)\nlet b = measure(2.0 s, 0.2 s)\nlet s = a + b\n",
        "E0401",
    );
    expect_error("let a = measure(1.0, 0.1)\nlet s = a + 2.0\n", "E0302");
}

#[test]
fn decide_accepts_symbolic_propositions() {
    // §8.1/§8.12: a symbolic `Real` comparison is a `Prop`, and `decide` turns
    // a proposition into `Option<Bool>`.
    expect_ok("let d = decide(1 == 1)\n");
    expect_ok("let d = decide(sin(0)^2 + cos(0)^2 == 1)\n");
    expect_any_error("let d = decide(1 m == 1 s)\n", "E0401");
}

#[test]
fn npy_io_accepts_every_tensor_rank_it_handles() {
    // The writer accepts dimensionless Float tensors of any concrete rank;
    // the reader must name its rank explicitly.
    expect_ok("let r: IO (Result<(), DataError>) = write_npy(\"x.npy\", [1.0, 2.0])\n");
    expect_ok(
        "let r: IO (Result<(), DataError>) = write_npy(\"x.npy\", [[1.0, 2.0], [3.0, 4.0]])\n",
    );
    expect_error(
        "let r: IO (Result<(), DataError>) = write_npy(\"x.npy\", 1.0)\n",
        "E0302",
    );
    expect_ok("let r: IO (Result<Tensor<2, Float>, DataError>) = read_npy<2>(\"x.npy\")\n");
    expect_error(
        "let r: IO (Result<Tensor<2, Float>, DataError>) = read_npy<2>(1)\n",
        "E0302",
    );
}

#[test]
fn a_dimension_variable_sqrt_renders_its_half_exponent() {
    // §8.15 display: `Complex[d^(1/2)]`, not `Complex[d0^1]`.
    let module = expect_ok("fn f<d>(z: Complex[d]): Complex[d^(1/2)] = complex_sqrt(z)\n");
    let scheme = scheme_of(&module, "f");
    assert!(
        format!("{}", scheme.ty).contains("^(1/2)"),
        "sqrt dimension displays its half exponent: {}",
        scheme.ty
    );
}

// --- §13.2.2 literal-key result typing for `Rssn.invoke` -------------------

fn pinned_hash(
    op: &str,
    index: usize,
) -> Option<String> {
    let entry = aevia_rssn::registry().op(op)?;
    Some(
        entry
            .keys
            .get(index)?
            .signature_hash
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}

#[test]
fn a_literal_pinned_invoke_gets_the_manifest_result() {
    let Some(hash) = pinned_hash("det", 0) else {
        return; // this kernel build has no `det`
    };
    // The generated façade declares `Dyn<term>` for a `Dyn` result.
    expect_ok(&format!(
        "fn f(a: Tensor<2, Float>): Real = Rssn.invoke(\"det\", \"{hash}\", [to_value(a)])\n"
    ));
    // The precise result rejects a wrong annotation.
    expect_error(
        &format!(
            "fn f(a: Tensor<2, Float>): Bool = Rssn.invoke(\"det\", \"{hash}\", [to_value(a)])\n"
        ),
        "E0302",
    );
    // A wrong/unknown literal hash deliberately falls back to `Value`: the
    // runtime's E1005 is the diagnostic of record.
    expect_ok("let x = Rssn.invoke(\"det\", \"00000000000000000000000000000000\", [])\n");
    // Non-literal keys keep the dynamic typing.
    expect_error(
        "fn f(name: String, hash: String): Bool = Rssn.invoke(name, hash, [])\n",
        "E0302",
    );
}

#[test]
fn sde_invoke_preserves_the_dimension_relationship() {
    let Some(hash) = pinned_hash("sde_wiener", 0) else {
        return; // this kernel build has no `sde_wiener`
    };
    // `Real[T]` in, `T^(1/2)` out (twice, at independent call sites): the
    // fresh dimension variable per call is what makes this type-check.
    // Concrete dimensions pin the `T^(1/2)` relationship exactly: `L^2` in,
    // `L` out; `Time^2` in, `Time` out (two independent call sites).
    expect_ok(&format!(
        "fn a(seed: Int, m: Int, steps: Int, h: Float[L^2]): Tensor<2, Real[L]> =\n    Rssn.invoke(\"sde_wiener\", \"{hash}\", [to_value(seed), to_value(m), to_value(steps), to_value(h)])\nfn b(seed: Int, m: Int, steps: Int, h: Float[Time^2]): Tensor<2, Real[Time]> =\n    Rssn.invoke(\"sde_wiener\", \"{hash}\", [to_value(seed), to_value(m), to_value(steps), to_value(h)])\n"
    ));
    // The result class is precise: a Bool annotation is rejected. A wrong
    // *dimension* is not statically observable here — the arguments go through
    // `to_value`, so the fresh dimension variable could equal any expression;
    // the runtime E1005/E0802 path owns that check.
    expect_error(
        &format!(
            "fn c(seed: Int, m: Int, steps: Int, h: Float[L^2]): Bool =\n    Rssn.invoke(\"sde_wiener\", \"{hash}\", [to_value(seed), to_value(m), to_value(steps), to_value(h)])\n"
        ),
        "E0302",
    );
}

#[test]
fn a_literal_invoke_of_an_unrepresentable_result_stays_dynamic() {
    // `sde_levy_areas` returns an opaque/kernel type; the mapping falls back to
    // `Dyn<term>`, and the emitted façade is still well typed.
    if let Some(hash) = pinned_hash("sde_levy_areas", 0) {
        expect_ok(&format!(
            "let x = Rssn.invoke(\"sde_levy_areas\", \"{hash}\", [])\n"
        ));
    }
}

// --- task-36: whole-tensor `min_all`/`max_all` accept any rank -------------

#[test]
fn min_all_and_max_all_reduce_tensors_of_any_rank() {
    // §8.1: `min_all`/`max_all` reduce the whole tensor, any rank; the runtime
    // (task-35) has the dense fast path. The rank-1-only scheme rejected the
    // 3×3 case with a shape mismatch.
    let module = expect_ok(
        "let m = [[1.0, 5.0, 9.0], [2.0, 6.0, 10.0], [3.0, 7.0, 11.0]]\nlet t = transpose(m)\nlet lo = min_all(t)\nlet hi = max_all(t)\n",
    );
    let one = aevia_dim::Dim::one();
    assert_eq!(
        scheme_of(&module, "lo").ty,
        Type::float(one.clone()),
        "a whole-tensor reduction returns the scalar element type"
    );
    assert_eq!(scheme_of(&module, "hi").ty, Type::float(one));
    // Rank-1 and rank-only tensors still work.
    expect_ok("let x = min_all([1.0, 2.0, 3.0])\n");
    expect_ok("fn f(a: Tensor<3, Float>): Float = max_all(a)\n");
    expect_ok("fn f(a: Tensor<2, Real[L]>): Real[L] = min_all(a)\n");
    expect_ok("fn f(a: Tensor<[2, 3], Float>): Float = max_all(a)\n");
    // The result dimension is the element's dimension.
    expect_ok("fn f(a: Tensor<2, Real[L]>): Real[L] = min_all(a)\n");
    // Non-tensors are rejected.
    expect_error("let x = min_all(3.0)\n", "E0302");
    expect_error("let x = max_all(\"s\")\n", "E0302");
    // The binary scalar functions are unchanged.
    expect_ok(
        "fn f(a: Real[L], b: Real[L]): Real[L] = min(a, b)\nfn g(a: Float, b: Float): Float = max(a, b)\n",
    );
}

#[test]
fn decimal_body_arithmetic_promotes_an_int_input_without_changing_literal_domain() {
    let source = "fn f(x) = x + 1.0\nlet result = f(1)\n";
    let module = expect_ok(source);
    assert_eq!(
        expr_type(&module, source, "f(1)", 1),
        Type::float(aevia_dim::Dim::one())
    );
    expect_error("fn f(x) = x + 1.0\nlet impossible: Int = f(1)\n", "E0302");
}
