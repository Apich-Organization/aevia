//! §6.4.5: `symfun` builds undetermined-function terms and `dsolve` solves an
//! ODE for one, returning the solution as an Aevia function.

fn run(source: &str) -> Vec<String> {
    let output = aevia_core::run(source, "dsolve.aev").expect("runs");
    output.lines().map(str::to_string).collect()
}

#[test]
fn a_symfun_application_is_a_term() {
    let lines = run(r#"fn main(): IO Unit = do
    let y = symfun("y")
    let t = symbol("t")
    println(y(t))
    println(diff(fn s: Real[1] => y(s))(t))
"#);
    assert!(lines[0].contains("y(t)"), "{lines:?}");
    assert!(lines[1].contains("y"), "{lines:?}");
}

#[test]
fn dsolve_returns_the_solution_function() {
    let lines = run(r#"fn main(): IO Unit = do
    let y = symfun("y")
    let t = symbol("t")
    let eq = diff(fn s: Real[1] => y(s))(t) == y(t)
    match dsolve(eq, y, t) with
    | Ok(f) -> println(f(t))
    | Err(e) -> println("failed: {e}")
"#);
    assert!(lines[0].contains("exp(t)"), "{lines:?}");
    assert!(!lines[0].starts_with("failed"), "{lines:?}");
}

#[test]
fn dsolve_uses_initial_conditions() {
    let lines = run(r#"fn main(): IO Unit = do
    let y = symfun("y")
    let t = symbol("t")
    let eq = diff(fn s: Real[1] => y(s))(t) == -2 * y(t)
    match dsolve(eq, y, t, [y(0) == 3]) with
    | Ok(f) -> do
        println(f(t))
        println(f(0))
    | Err(e) -> println("failed: {e}")
"#);
    assert!(!lines[0].starts_with("failed"), "{lines:?}");
    assert!(lines[0].contains("exp"), "{lines:?}");
    assert_eq!(lines[1], "3", "{lines:?}");
}

#[test]
fn dsolve_rejects_a_non_equation() {
    let compilation = aevia_core::compile(
        "bad.aev",
        r#"fn main(): IO Unit = do
    let y = symfun("y")
    let t = symbol("t")
    match dsolve(y(t) > 0, y, t) with
    | Ok(_) -> println("solved")
    | Err(e) -> println("failed")
"#,
    );
    // `y(t) > 0` is a `Prop`, so the program checks; the runtime refuses it.
    let errors = compilation.diagnostics.error_count();
    if errors == 0 {
        let output = aevia_core::run(
            r#"fn main(): IO Unit = do
    let y = symfun("y")
    let t = symbol("t")
    match dsolve(y(t) > 0, y, t) with
    | Ok(_) -> println("solved")
    | Err(e) -> println("failed")
"#,
            "bad.aev",
        );
        assert!(output.is_err() || output.unwrap().contains("failed"));
    }
}
