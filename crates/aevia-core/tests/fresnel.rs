//! §8.2.2: the Fresnel integrals, numerically and through the kernel's
//! symbolic calculus.

fn run(source: &str) -> String {
    aevia_core::run(source, "fresnel.aev")
        .unwrap_or_else(|error| panic!("{}: {}", error.code, error.message))
}

#[test]
fn values_match_reference_tables() {
    let output = run(r#"fn main(): IO Unit = do
    println(fresnel_s(1.0))
    println(fresnel_c(1.0))
    println(fresnel_s(-2.0))
    println(fresnel_c(100.0))
"#);
    let values: Vec<f64> = output.lines().map(|line| line.parse().unwrap()).collect();
    let reference = [
        0.438_259_147_390_354_766,
        0.779_893_400_376_822_829,
        -0.343_415_678_363_698_242,
        0.499_999_898_678_817_9,
    ];
    for (got, want) in values.iter().zip(reference) {
        assert!((got - want).abs() < 1e-14, "{got} vs {want}");
    }
}

#[test]
fn the_kernel_differentiates_and_integrates_them() {
    let output = run(r#"fn main(): IO Unit = do
    let d = diff(fn (t) => fresnel_s(t))
    println(d(symbol("u")))
    let f = antiderivative(fn (t) => fresnel_c(t))
    println(f(symbol("v")))
    let g = antiderivative(fn (t) => sin(t^2))
    println(g(symbol("w")))
"#);
    let lines: Vec<&str> = output.lines().collect();
    assert!(
        lines[0].starts_with("sin(") && lines[0].contains("pi"),
        "{}",
        lines[0]
    );
    assert!(lines[1].contains("fresnelc"), "{}", lines[1]);
    assert!(lines[2].contains("fresnels"), "{}", lines[2]);
}
