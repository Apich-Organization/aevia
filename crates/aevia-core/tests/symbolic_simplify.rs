//! Exact simplification of symbolic `Real` terms: coefficient-aware like-term
//! collection, exact trigonometric values at rational multiples of `pi`,
//! square-factor extraction and exact display of rational coefficients.

#![allow(clippy::result_large_err)]

use aevia_core::Interp;
use aevia_core::compile;

fn observe(
    source: &str,
    name: &str,
) -> Result<String, String> {
    let compilation = compile("simplify.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    let module = compilation.module.expect("a parsed module");
    let mut interp = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = interp.load(&module);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    match interp.eval_toplevel(name) {
        | Ok(value) => Ok(aevia_core::value::show(&value)),
        | Err(error) => Err(format!("{}: {}", error.code, error.message)),
    }
}

fn show(expression: &str) -> String {
    let source = format!("let x = symbol(\"x\")\nlet v = {expression}\n");
    observe(&source, "v").unwrap_or_else(|error| format!("<{error}>"))
}

fn check(cases: &[(&str, &str)]) {
    for (expression, expected) in cases {
        assert_eq!(show(expression), *expected, "for `{expression}`");
    }
}

#[test]
fn like_terms_use_their_coefficients() {
    check(&[
        ("x + x", "2*x"),
        ("x - x", "0"),
        ("2*x + 3*x", "5*x"),
        ("x + 2*x - 3*x", "0"),
        ("(x + 1) - (x + 1)", "0"),
        ("3*x - 2*x", "x"),
        ("x/2 + x/2", "x"),
        ("x/2 + x/3", "5/6*x"),
        ("x^3 - 3*x + 1", "x^3 - 3*x + 1"),
        ("2*(x + 1) - 2*x", "2"),
    ]);
}

#[test]
fn trigonometric_values_at_rational_multiples_of_pi() {
    check(&[
        ("sin(pi/6)", "0.5"),
        ("cos(pi/3)", "0.5"),
        ("sin(pi)", "0"),
        ("cos(0)", "1"),
        ("sin(0)", "0"),
        ("tan(0)", "0"),
        ("tan(pi/4)", "1"),
        ("cos(pi)", "-1"),
        ("sin(pi/2)", "1"),
        ("cos(pi/2)", "0"),
        ("sin(-pi/6)", "-0.5"),
        ("sin(7*pi/6)", "-0.5"),
        ("cos(2*pi/3)", "-0.5"),
        ("cos(-pi/3)", "0.5"),
        ("sin(13*pi/6)", "0.5"),
        ("sin(pi/4)", "1/2*sqrt(2)"),
        ("cos(3*pi/4)", "-1/2*sqrt(2)"),
        ("sin(pi/3)", "1/2*sqrt(3)"),
        ("tan(pi/3)", "sqrt(3)"),
        ("tan(pi/6)", "1/3*sqrt(3)"),
        ("tan(3*pi/4)", "-1"),
        ("tan(-pi/3)", "-sqrt(3)"),
        // Undefined: left alone, not invented.
        ("tan(pi/2)", "tan(1/2*pi)"),
        ("tan(3*pi/2)", "tan(3/2*pi)"),
        // Other denominators are not evaluated.
        ("sin(pi/5)", "sin(1/5*pi)"),
        ("sin(x)", "sin(x)"),
    ]);
}

#[test]
fn exp_and_ln_are_inverse() {
    check(&[
        ("exp(0)", "1"),
        ("ln(1)", "0"),
        ("ln(exp(2))", "2"),
        ("ln(exp(x))", "x"),
        ("exp(ln(x))", "x"),
        ("exp(ln(x + 1))", "x + 1"),
    ]);
}

#[test]
fn square_roots_extract_square_factors() {
    check(&[
        ("sqrt(8)", "2*sqrt(2)"),
        ("sqrt(12)", "2*sqrt(3)"),
        ("sqrt(4)", "2"),
        ("sqrt(1/4)", "0.5"),
        ("sqrt(1/8)", "1/4*sqrt(2)"),
        ("sqrt(2)", "sqrt(2)"),
        ("sqrt(18)", "3*sqrt(2)"),
        ("sqrt(72)", "6*sqrt(2)"),
        ("8^(1/3)", "2"),
        ("sqrt(8)^2", "8"),
        ("sqrt(2)*sqrt(8)", "4"),
        ("3*sqrt(2)*sqrt(2)", "6"),
        ("sqrt(8) + sqrt(2)", "3*sqrt(2)"),
        ("sqrt(8) - 2*sqrt(2)", "0"),
    ]);
}

#[test]
fn big_integer_roots_are_exact() {
    // 2^53 + 1 squared: an f64 round trip cannot decide this.
    let n = "9007199254740993";
    assert_eq!(show(&format!("sqrt({n} * {n})")), n);
    assert_eq!(show(&format!("({n} * {n} * {n})^(1/3)")), n);
    // One more than a perfect square is not a perfect square.
    let v: num_bigint::BigInt = n.parse().unwrap();
    let next = (&v * &v + num_bigint::BigInt::from(1)).to_string();
    let shown = show(&format!("sqrt({n} * {n} + 1)"));
    assert!(shown.contains("sqrt("), "{shown} (radicand {next})");
    assert_ne!(shown, n);
}

#[test]
fn rational_coefficients_display_as_fractions() {
    check(&[
        ("pi/4", "1/4*pi"),
        ("pi/6", "1/6*pi"),
        ("x/4 + 1/2", "1/4*x + 1/2"),
        ("1/4", "0.25"),
    ]);
}

#[test]
fn calculus_still_simplifies() {
    let source = r#"let x = symbol("x")
let f = fn t => t^2 - t
let d = diff(f)(x)
let s = f(3)
let g = diff(fn t => t^3 - 3*t + 1)(x)
let h = diff(fn t => sin(t)^2)(x)
let q = diff(fn t => 2*t^2 + 3*t)(x)
"#;
    assert_eq!(observe(source, "d").unwrap(), "2*x - 1");
    assert_eq!(observe(source, "s").unwrap(), "6");
    assert_eq!(observe(source, "g").unwrap(), "3*x^2 - 3");
    assert_eq!(observe(source, "q").unwrap(), "4*x + 3");
    println!("{}", observe(source, "h").unwrap());
}
