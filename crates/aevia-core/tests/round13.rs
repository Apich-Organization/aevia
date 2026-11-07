//! Round 13: probe-driven fixes. Each test pins a defect a real program hit:
//! scaled units in runtime arithmetic, the Float phase of `linsolve`/`sqrt`,
//! display-unit propagation (§5.4.2), symbolic powers and sums, derived
//! instances over records and generic types, distributions as the nominal
//! `Distribution<d>` (§8.7.1), the inline `match` form, quoted strings inside
//! containers, and the `with_cap` capability sandbox (§9.14).

#![allow(clippy::result_large_err)]

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::value::NumKind;
use aevia_core::value::Value;

fn errors(source: &str) -> Vec<String> {
    compile("round13.aev", source)
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
    let compilation = compile("round13.aev", source);
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

/// The error code a program fails with when its top-level bindings load
/// (they are eager, so a runtime failure in one is a load diagnostic).
fn failure(source: &str) -> String {
    let compilation = compile("round13.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "compile errors: {:?}",
        errors(source)
    );
    let module = compilation.module.expect("module");
    let mut interp = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = interp.load(&module);
    diagnostics
        .items
        .iter()
        .find(|item| item.severity == aevia_syntax::Severity::Error)
        .map(|item| item.code.to_string())
        .expect("expected a load failure")
}

/// Runs a file through the module driver (so `import`/`open` of standard
/// library modules resolve) and returns what `main` printed.
fn run_file(source: &str) -> String {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "aevia-round13-{}-{}",
        std::process::id(),
        source.len()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    let entry = dir.join("main.aev");
    std::fs::write(&entry, source).expect("write");
    let program = aevia_core::driver::load(&entry);
    assert!(
        !program.diagnostics.has_errors(),
        "diagnostics: {:?}",
        program
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    let checked = &program.entry().expect("entry module").checked;
    let mut interp = Interp::new(checked).with_buffer();
    let diagnostics = aevia_core::driver::run_modules(&mut interp, &program);
    assert!(!diagnostics.has_errors(), "load errors");
    interp.run_main().expect("main runs");
    let output = interp.captured();
    let _ = std::fs::remove_dir_all(&dir);
    output
}

/// What `main` prints, in both evaluators.
fn printed(source: &str) -> String {
    let mut results = Vec::new();
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = interp(source, mode);
        interp
            .run_main()
            .unwrap_or_else(|error| panic!("{mode:?}: {}: {}", error.code, error.message));
        results.push(interp.captured());
    }
    assert_eq!(results[0], results[1], "AST and IR disagree");
    results.remove(0)
}

fn main_error(source: &str) -> String {
    let mut interp = interp(source, Mode::Ir);
    match interp.run_main() {
        | Ok(_) => panic!("expected `main` to fail; printed {}", interp.captured()),
        | Err(error) => error.code.to_string(),
    }
}

// ----- units ---------------------------------------------------------------

#[test]
fn a_runtime_value_times_a_scaled_unit_is_scaled() {
    let source = "open Units.SI\nlet a = 3\nlet x = to(m, a * km)\nlet y = to(m, sum([1.0, 2.5] * km))\nlet z = (0.02 / km) * 1000.0 m\n";
    assert_eq!(shown(source, "x"), "3000 m");
    assert_eq!(shown(source, "y"), "3500.0 m");
    assert_eq!(shown(source, "z"), "0.02");
}

#[test]
fn the_spec_projectile_drag_constant_divides_by_a_unit() {
    // §11.2 writes `let k = 0.02 / m`; it used to stop with `E0302`.
    let source = "open Units.SI\nlet k = 0.02 / m\nlet drag = k * 3.0 m\n";
    assert_eq!(shown(source, "drag"), "0.06");
}

#[test]
fn display_units_propagate_as_section_5_4_2_says() {
    let source = "open Units.SI\nlet a = to(km, 3.0 km) + to(km, 200.0 m)\nlet b = 2.0 * to(km, 3.0 km)\nlet c = to(km, 3.0 km) + to(m, 1.0 m)\nlet t1 = 25.0 °C\nlet t3 = t1 + 10.0 K\n";
    assert_eq!(shown(source, "a"), "3.2 km");
    assert_eq!(shown(source, "b"), "6.0 km");
    assert_eq!(shown(source, "c"), "3001.0 m");
    // A point moved by a difference keeps the point's unit (§5.6.3, §11.5).
    assert_eq!(shown(source, "t3"), "35.0 °C");
}

#[test]
fn a_float_in_an_affine_display_unit_is_rounded_once() {
    let source = "open Units.SI\nlet t2 = point(300.0, K)\nlet c = to(°C, t2)\n";
    assert_eq!(shown(source, "c"), "26.85 °C");
}

#[test]
fn qualified_unit_names_are_unit_literals() {
    let source = "let d = 3 Units.SI.km\n";
    assert_eq!(shown(source, "d"), "3 km");
}

// ----- Float phase ------------------------------------------------------------

#[test]
fn linsolve_of_a_float_system_answers_floats() {
    let source = "import LinearAlgebra { linsolve }\nlet r = match linsolve([[1.0, 2.0], [3.0, 4.0]], [0.5, 1.1]) with\n    | Ok(v) -> v[0]\n    | Err(_) -> 0.0\nlet e = match linsolve([[1, 2], [3, 4]], [1, 0]) with\n    | Ok(v) -> v[1]\n    | Err(_) -> 0\n";
    let mut float = interp(source, Mode::Ir);
    let value = float.eval_toplevel("r").expect("r");
    assert!(
        matches!(&value, Value::Num(num) if matches!(num.kind, NumKind::Float(_))),
        "expected a Float, got {}",
        aevia_core::value::repr(&value)
    );
    // An exact system still has an exact solution: y = 3/2.
    assert_eq!(shown(source, "e"), "1.5");
    let mut exact = interp(source, Mode::Ir);
    let value = exact.eval_toplevel("e").expect("e");
    assert!(
        matches!(&value, Value::Num(num) if matches!(num.kind, NumKind::Exact(_))),
        "expected an exact value, got {}",
        aevia_core::value::repr(&value)
    );
}

#[test]
fn an_even_root_of_a_negative_float_is_a_domain_error() {
    assert_eq!(failure("let r = sqrt(-1.0)\n"), "E0806");
    assert_eq!(shown("let r = cbrt(-8.0)\n", "r"), "-2.0");
    // The Real phase keeps the formal term (§6.3.4).
    assert_eq!(shown("let r = sqrt(-1)\n", "r"), "sqrt(-1)");
}

#[test]
fn float_infinity_is_inf_and_exact_infinity_is_oo() {
    assert_eq!(shown("let v = 1.0 / 0.0\n", "v"), "inf");
    assert_eq!(shown("let v = oo\n", "v"), "oo");
}

// ----- symbolic -------------------------------------------------------------

#[test]
fn a_symbolic_exponent_builds_a_power() {
    let source = "let x = symbol(\"x\")\nlet p = 2^x\nlet d = diff(fn t => 2^t)(x)\nlet l = limit(fn t => (1 + 1/t)^t, oo)\n";
    assert_eq!(shown(source, "p"), "2^x");
    assert_eq!(shown(source, "d"), "ln(2)*2^x");
    assert_eq!(shown(source, "l"), "Ok(e)");
    assert!(errors("let x = symbol(\"x\")\nlet p: Real = 2^x\n").is_empty());
}

#[test]
fn sums_over_infinite_or_symbolic_ranges_ask_the_kernel() {
    let source = "let a = sum(fn k => 1 / k^2, 1, oo)\nlet b = sum(fn k => 1 / 2^k, 0, oo)\nlet c = sum(fn k => k, 1, 100000)\nlet n = symbol(\"n\")\nlet d = sum(fn k => k, 1, n)\n";
    assert_eq!(shown(source, "a"), "1/6*pi^2");
    assert_eq!(shown(source, "b"), "2");
    assert_eq!(shown(source, "c"), "5000050000");
    assert_eq!(shown(source, "d"), "1/2*n*(n + 1)");
    assert_eq!(failure("let s = sum(fn k => sin(k^2), 1, oo)\n"), "E0808");
}

#[test]
fn kernel_results_use_aevia_spelling() {
    let source = "let r = defint(fn t => exp(-t^2), -oo, oo)\n";
    assert_eq!(shown(source, "r"), "Ok(sqrt(pi))");
}

#[test]
fn latex_output_is_typeset() {
    let source = "let x = symbol(\"x\")\nlet y = symbol(\"y\")\nlet t = to_latex(x^2 / (y + 1))\n";
    assert_eq!(shown(source, "t"), r"\frac{x^{2}}{y + 1}");
}

// ----- classes --------------------------------------------------------------

#[test]
fn derive_accepts_record_fields_and_generic_types() {
    let source = "type Shape = | Tri({ a: Float, b: Float }) | Dot\nderive Eq, Show for Shape\ntype Tree<a> = | Leaf | Node(Tree<a>, a, Tree<a>)\nderive Eq, Show for Tree\nlet same = Tri({ a = 1.0, b = 2.0 }) == Tri({ a = 1.0, b = 2.0 })\nlet text = show(Node(Leaf, 3, Leaf))\n";
    assert_eq!(shown(source, "same"), "true");
    assert_eq!(shown(source, "text"), "Node(Leaf, 3, Leaf)");
}

// ----- syntax ---------------------------------------------------------------

#[test]
fn match_arms_can_share_the_with_line() {
    let source = "let o = Some(4)\nlet v = match o with | Some(n) -> n * 10 | None -> 0\nlet w = [match o with | Some(n) -> n | None -> 0, 7]\n";
    assert_eq!(shown(source, "v"), "40");
    assert_eq!(shown(source, "w"), "[4, 7]");
}

#[test]
fn strings_inside_containers_are_quoted() {
    let source = "let r = { name = \"x\", value = 4 }\nlet e: Result<Int, String> = Err(\"not found\")\nlet parts = split(\"a,b\", \",\")\nlet plain = \"top\"\n";
    assert_eq!(shown(source, "r"), "{ name = \"x\", value = 4 }");
    assert_eq!(shown(source, "e"), "Err(\"not found\")");
    assert_eq!(shown(source, "parts"), "[\"a\", \"b\"]");
    assert_eq!(shown(source, "plain"), "top");
}

// ----- library --------------------------------------------------------------

#[test]
fn distributions_are_the_nominal_type_of_section_8_7_1() {
    let source = "open Units.SI\nopen Stats.Distributions\n\nfn main(): IO Unit = do\n    let n = Normal({ mu = 0.0, sigma = 1.0 })\n    let h = Normal({ mu = 1.7 m, sigma = 0.1 m })\n    println(pdf(n, 0.0))\n    println(cdf(StudentT({ df = 3 }), 1.0))\n    println(cdf(ChiSquared({ df = 3 }), 1.0))\n    println(sample_n(h, 2, Some(7)))\n";
    let output = run_file(source);
    let lines: Vec<&str> = output.lines().collect();
    assert!(lines[0].starts_with("0.398942280401"), "{output}");
    assert!(lines[1].starts_with("0.80449889"), "{output}");
    assert!(lines[2].starts_with("0.19874804"), "{output}");
    assert!(lines[3].ends_with(" m]"), "{output}");
}

// ----- capabilities ---------------------------------------------------------

#[test]
fn with_cap_sandboxes_file_and_random_actions() {
    let source = "fn main(): IO Unit = do\n    r <- with_cap(\"\", fn () => read_file(\"definitely-missing.txt\"))\n    println(\"{r}\")\n    x <- with_cap(\"rand\", fn () => random())\n    println(\"{x >= 0.0}\")\n";
    let output = printed(source);
    assert!(
        output.contains("E0902") && output.contains("`fs`"),
        "{output}"
    );
    assert!(output.contains("true"), "{output}");
    let denied = "fn main(): IO Unit = do\n    x <- with_cap(\"fs\", fn () => random())\n    println(\"{x}\")\n";
    assert_eq!(main_error(denied), "E0902");
    // The sandbox ends with its block.
    let restored = "fn main(): IO Unit = do\n    _ <- with_cap(\"\", fn () => println(\"in\"))\n    x <- random()\n    println(\"{x >= 0.0}\")\n";
    assert!(printed(restored).ends_with("true\n"));
}

#[test]
fn an_unknown_capability_is_reported() {
    let source = "fn main(): IO Unit = do\n    _ <- with_cap(\"disk\", fn () => println(\"x\"))\n    println(\"y\")\n";
    assert_eq!(main_error(source), "E0302");
}

// ----- diagnostics ------------------------------------------------------------

#[test]
fn misspelt_names_get_a_suggestion() {
    let compilation = compile("round13.aev", "let lenght = 3\nlet x = lenhgt + 1\n");
    let unbound = compilation
        .diagnostics
        .items
        .iter()
        .find(|item| item.code == "E0301")
        .expect("E0301");
    assert!(
        unbound.helps.iter().any(|help| help.contains("`lenght`")),
        "{:?}",
        unbound.helps
    );
}

#[test]
fn strings_index_and_count_by_grapheme_cluster() {
    // §8.1.4: `e` + U+0301 is one position.
    let source = "let s = \"he\u{301}llo\"\nlet n = length(s)\nlet c = s[1]\nlet sub = s[1..3]\nlet last = s[-1]\nlet back = s[::-1]\nlet parts = length(chars(s))\n";
    assert_eq!(shown(source, "n"), "5");
    assert_eq!(shown(source, "c"), "e\u{301}");
    assert_eq!(shown(source, "sub"), "e\u{301}l");
    assert_eq!(shown(source, "last"), "o");
    assert_eq!(shown(source, "back"), "olle\u{301}h");
    assert_eq!(shown(source, "parts"), "5");
    assert!(errors("let s = \"ab\"\nlet c: String = s[0]\n").is_empty());
}

#[test]
fn format_templates_take_positional_and_sequential_placeholders() {
    let source = "let t = format(r\"{} + {1} = {0}\", [\"a\", \"b\"])\n";
    assert_eq!(shown(source, "t"), "a + b = a");
}

#[test]
fn a_c_style_remainder_and_an_empty_interpolation_are_explained() {
    let percent = compile("round13.aev", "let k = 7\nlet r = k % 2\n");
    let diagnostic = percent
        .diagnostics
        .items
        .iter()
        .find(|item| item.code == "E0202")
        .expect("E0202");
    assert!(
        diagnostic
            .helps
            .iter()
            .any(|help| help.contains("mod(a, b)"))
    );
    let empty = compile("round13.aev", "let s = \"{}\"\n");
    assert!(
        empty
            .diagnostics
            .items
            .iter()
            .any(|item| item.code == "E0104")
    );
}
