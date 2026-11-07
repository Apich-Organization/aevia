//! Revision 13: design changes, each pinned by the behaviour it fixes.
//!
//! * a spaced `*`/`/` ends a unit literal (§1.8.1);
//! * declared type parameters are rigid (`E0318`, §3.5);
//! * a runtime exponent solves an inferred dimension to `1` (§3.4.3);
//! * `show` keeps the `Float` phase visible (§6.12.3);
//! * the algebraic normal forms of §8.5.1;
//! * the trust boundary of §9.2.3 does not change results (checked calls and
//!   constructors in both evaluators, a forged host value is still rejected).

#![allow(clippy::result_large_err)]

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;

fn errors(source: &str) -> Vec<String> {
    compile("revision13.aev", source)
        .diagnostics
        .items
        .iter()
        .filter(|item| item.severity == aevia_syntax::Severity::Error)
        .map(|item| format!("{}: {}", item.code, item.message))
        .collect()
}

fn interp(
    source: &str,
    mode: Mode,
) -> Interp {
    let compilation = compile("revision13.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "compile errors in\n{source}\n{:?}",
        errors(source)
    );
    let module = compilation.module.expect("module");
    let mut interp = Interp::new(&compilation.checked)
        .with_mode(mode)
        .with_buffer();
    let diagnostics = interp.load(&module);
    assert!(
        !diagnostics.has_errors(),
        "load errors: {:?}",
        diagnostics
            .items
            .iter()
            .map(|item| format!("{}: {}", item.code, item.message))
            .collect::<Vec<_>>()
    );
    interp
}

/// `show` of a top-level binding, in both evaluators (they must agree).
fn shown(
    source: &str,
    name: &str,
) -> String {
    let mut results = Vec::new();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = interp(source, mode);
        let value = interp
            .eval_toplevel(name)
            .unwrap_or_else(|error| panic!("{mode:?}: {}: {}", error.code, error.message));
        results.push(aevia_core::value::show(&value));
    }
    assert_eq!(results[0], results[1], "AST and IR disagree");
    results.remove(0)
}

/// The output of `main`, in both evaluators.
fn output(source: &str) -> String {
    let mut results = Vec::new();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = interp(source, mode);
        interp
            .run_main()
            .unwrap_or_else(|error| panic!("{mode:?}: {}: {}", error.code, error.message));
        results.push(interp.captured().to_string());
    }
    assert_eq!(results[0], results[1], "AST and IR disagree");
    results.remove(0)
}

// ----- §1.8.1: unit literals end at a spaced operator ------------------------

#[test]
fn a_quantity_times_a_field_needs_no_parentheses() {
    let source = "type P = { mass: Float[M], height: Float[L] }\n\
                  fn energy(p: P): Float[M*L^2/T^2] = p.mass * 9.81 m/s^2 * p.height\n\
                  let e = energy({ mass = 2.0 kg, height = 10.0 m })\n";
    assert_eq!(shown(source, "e"), "196.20000000000002 J");
}

#[test]
fn the_tour_formula_type_checks() {
    // §0.5 writes exactly this.
    let source = "let v = 3 m/s\nlet E = 70 kg * v^2 / 2\n";
    assert_eq!(shown(source, "E"), "315 J");
}

#[test]
fn a_tight_operator_still_belongs_to_the_unit() {
    assert_eq!(shown("let a = 9.80665 m/s^2\n", "a"), "9.80665 m/s^2");
    assert!(
        errors("let x = 2\nlet bad = 3 m*x\n")
            .iter()
            .any(|error| error.starts_with("E0501")),
        "`m*x` written tight is a unit expression"
    );
}

// ----- §3.5: declared binders are rigid --------------------------------------

#[test]
fn a_body_that_fixes_a_declared_dimension_is_e0318() {
    let found = errors("fn h<d>(x: Real[d]): Real[d] = x + 1 m\n");
    assert!(
        found.iter().any(|error| error.starts_with("E0318")),
        "{found:?}"
    );
}

#[test]
fn a_body_that_merges_two_declared_dimensions_is_e0318() {
    let found = errors("fn h<d, e>(x: Real[d], y: Real[e]): Real[d] = x + y\n");
    assert!(
        found.iter().any(|error| error.starts_with("E0318")),
        "{found:?}"
    );
}

#[test]
fn a_body_that_fixes_a_declared_type_is_e0318() {
    let found = errors("fn h<a>(x: a): a = concat(x, \"!\")\n");
    assert!(
        found.iter().any(|error| error.starts_with("E0318")),
        "{found:?}"
    );
}

#[test]
fn genuinely_generic_bodies_are_accepted() {
    for source in [
        "fn mean<d>(xs: Tensor<1, Float[d]>): Float[d] = sum(xs) / length(xs)\n",
        "fn sq<d>(x: Real[d]): Real[d^2] = x * x\n",
        "fn swap<a, b>(p: (a, b)): (b, a) = match p with | (x, y) -> (y, x)\n",
        "fn first<n>(xs: Tensor<[n], Float>): Float = xs[0]\n",
        // §11.8: unification solves `d` as a product of other free variables;
        // the binders are still independent, so this is generic.
        "fn linear_fit<d, e>(xs: Tensor<1, Real[d]>, ys: Tensor<1, Real[e]>): { slope: Real[e/d], intercept: Real[e] } =\n    let mx = mean(xs)\n    let my = mean(ys)\n    let slope = sum((xs - mx) * (ys - my)) / sum((xs - mx)^2)\n    { slope = slope, intercept = my - slope * mx }\n",
        // Representation polymorphism refines a type binder without fixing it.
        "fn bump<a>(x: a): a = x + 1\n",
        // Extent binders are solved by linear equations (§3.5.1).
        "fn f<n>(a: Tensor<[n + 1], Float>): Tensor<[2], Float> = a\n",
    ] {
        assert!(errors(source).is_empty(), "{source}: {:?}", errors(source));
    }
}

// ----- §3.4.3: a runtime exponent solves an inferred dimension ----------------

#[test]
fn a_runtime_exponent_makes_an_inferred_base_dimensionless() {
    let source = "let x = symbol(\"x\")\nlet d = diff(fn t => t^t)(x)\n";
    assert_eq!(shown(source, "d"), "ln(x)*x^x + x^x");
}

#[test]
fn a_runtime_exponent_on_a_declared_or_known_dimension_is_still_e0404() {
    for source in [
        "fn g(x: Real[L], n: Int) = x^n\n",
        "fn f<d>(x: Real[d], n: Int): Real[1] = x^n\n",
    ] {
        let found = errors(source);
        assert!(
            found.iter().any(|error| error.starts_with("E0404")),
            "{source}: {found:?}"
        );
    }
}

// ----- §6.12.3: the Float phase is visible -----------------------------------

#[test]
fn integral_floats_keep_their_decimal_point() {
    let source = "let a = 10.0\nlet b = 10\nlet c = -0.0\nlet d = [1.0, 2.0]\nlet e = 2.0 m\nlet f = 1.0e20\n";
    assert_eq!(shown(source, "a"), "10.0");
    assert_eq!(shown(source, "b"), "10");
    assert_eq!(shown(source, "c"), "-0.0");
    assert_eq!(shown(source, "d"), "[1.0, 2.0]");
    assert_eq!(shown(source, "e"), "2.0 m");
    assert_eq!(shown(source, "f"), "1e20");
}

// ----- §8.5.1: algebraic normal forms ----------------------------------------

#[test]
fn algebraic_forms_come_from_the_kernel_operators() {
    let source = "let x = symbol(\"x\")\n\
                  let a = expand((x + 1)^3)\n\
                  let b = factor(x^4 - 1)\n\
                  let c = cancel((x^2 - 1) / (x - 1))\n\
                  let n = degree(x^3 + x, x)\n\
                  let k = factor(360)\n";
    assert_eq!(shown(source, "a"), "x^3 + 3*x^2 + 3*x + 1");
    assert_eq!(shown(source, "b"), "(x - 1)*(x + 1)*(x^2 + 1)");
    assert_eq!(shown(source, "c"), "x + 1");
    assert_eq!(shown(source, "n"), "3");
    assert_eq!(shown(source, "k"), "[(2, 3), (3, 2), (5, 1)]");
}

#[test]
fn an_algebraic_form_keeps_the_dimension() {
    let source = "let t = symbol(\"t\")\nlet p = expand((t * 1 m + 2 m)^2)\n";
    let shown = shown(source, "p");
    assert!(shown.ends_with("m^2"), "{shown}");
}

// ----- §9.2.3: the trust boundary does not change results --------------------

#[test]
fn checked_constructors_and_calls_agree_in_both_evaluators() {
    let source = r#"type Tree<a> = | Leaf | Node(Tree<a>, a, Tree<a>)
derive Show for Tree
fn insert(t: Tree<Int>, x: Int): Tree<Int> =
    match t with
    | Leaf -> Node(Leaf, x, Leaf)
    | Node(l, v, r) -> if x < v then Node(insert(l, x), v, r) else Node(l, v, insert(r, x))
fn size<a>(t: Tree<a>): Int =
    match t with
    | Leaf -> 0
    | Node(l, _, r) -> size(l) + 1 + size(r)
fn build(n: Int, t: Tree<Int>): Tree<Int> =
    if n == 0 then t else build(n - 1, insert(t, mod(n * 7919, 10007)))
fn main(): IO Unit = do
    println(size(build(500, Leaf)))
    println(show(insert(Leaf, 3)))
"#;
    assert_eq!(output(source), "500\nNode(Leaf, 3, Leaf)\n");
}

#[test]
fn an_empty_generic_constructor_keeps_its_type_witness() {
    // `show` of an empty container dispatches through the witness the checked
    // constructor attached, so the witness must survive the IR fast path.
    let source = "let none: Option<Float> = None\nlet some = Some(1.0)\n";
    assert_eq!(shown(source, "none"), "None");
    assert_eq!(shown(source, "some"), "Some(1.0)");
}

// ----- §8.1.5.1: sets ---------------------------------------------------------

#[test]
fn sets_are_canonical_and_their_operations_merge() {
    let source = "let s = set_of([3, 1, 2, 3])\n\
                  let u = union(s, set_of([5, 1]))\n\
                  let i = intersection(s, set_of([2, 3, 9]))\n\
                  let d = difference(s, set_of([1]))\n\
                  let n = length(s)\n\
                  let has = member(2, s)\n\
                  let lacks = member(7, s)\n\
                  let same = set_insert(s, 0) == set_of([0, 1, 2, 3])\n\
                  let sub = is_subset(set_of([1, 2]), s)\n\
                  let back = elements(set_remove(s, 2))\n\
                  let r = repr(s)\n";
    assert_eq!(shown(source, "s"), "{1, 2, 3}");
    assert_eq!(shown(source, "u"), "{1, 2, 3, 5}");
    assert_eq!(shown(source, "i"), "{2, 3}");
    assert_eq!(shown(source, "d"), "{2, 3}");
    assert_eq!(shown(source, "n"), "3");
    assert_eq!(shown(source, "has"), "true");
    assert_eq!(shown(source, "lacks"), "false");
    assert_eq!(shown(source, "same"), "true");
    assert_eq!(shown(source, "sub"), "true");
    assert_eq!(shown(source, "back"), "[1, 3]");
    assert_eq!(shown(source, "r"), "set_of([1, 2, 3])");
}

#[test]
fn a_set_of_strings_orders_its_elements() {
    let source = "let s = set_of([\"b\", \"a\", \"b\"])\n";
    assert_eq!(shown(source, "s"), "{\"a\", \"b\"}");
}

#[test]
fn a_set_of_floats_is_rejected_for_lack_of_a_total_order() {
    let found = errors("let s = set_of([1.0, 2.0])\n");
    assert!(
        found.iter().any(|error| error.starts_with("E0310")),
        "{found:?}"
    );
}

// ----- §6.12.3: quotients read as quotients ----------------------------------

#[test]
fn negative_powers_print_as_a_denominator() {
    let source = "let x = symbol(\"x\")\nlet y = symbol(\"y\")\n\
                  let a = diff(fn t => ln(t^2 + 1))(x)\n\
                  let b = 1 / (x * y)\n\
                  let c = x / y^2\n";
    assert_eq!(shown(source, "a"), "2*x/(x^2 + 1)");
    assert_eq!(shown(source, "b"), "1/(x*y)");
    assert_eq!(shown(source, "c"), "x/y^2");
}

// ----- Result is Show and Eq like Option -------------------------------------

#[test]
fn results_have_show_and_eq_instances() {
    let source = "let x = symbol(\"x\")\n\
                  let r = repr(limit(fn t => (1 - cos(t)) / t^2, 0))\n\
                  let one: Result<Int, String> = Ok(1)\n\
                  let same = one == Ok(1)\n";
    // The limit is exact; `show` would print the terminating rational as a
    // decimal, `repr` keeps it a fraction.
    assert_eq!(shown(source, "r"), "Ok(1/2)");
    assert_eq!(shown(source, "same"), "true");
}
