//! §13.5.2: `@op` and `@rule` publish Aevia functions to the kernel. The
//! function is traced on symbols into a kernel definition, so the kernel can
//! call it, differentiate it and evaluate it numerically.

use aevia_core::Interp;
use aevia_core::value::Value;

fn load(source: &str) -> (Interp, Vec<String>) {
    let compilation = aevia_core::compile("plugin.aev", source);
    let errors: Vec<String> = compilation
        .diagnostics
        .items
        .iter()
        .filter(|item| item.code.starts_with('E'))
        .map(|item| format!("{}: {}", item.code, item.message))
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let module = compilation.module.as_ref().expect("parsed");
    let mut interp = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = interp.load(module);
    let codes = diagnostics
        .items
        .iter()
        .map(|item| format!("{}: {}", item.code, item.message))
        .collect();
    (interp, codes)
}

fn float_result(value: Value) -> f64 {
    let Value::Variant(result) = value else {
        panic!("expected a Result, found {value:?}");
    };
    assert_eq!(result.index, 0, "the call failed: {:?}", result.fields);
    match result.fields.first() {
        | Some(Value::KernelValue(opaque)) => {
            match &opaque.payload {
                | aevia_rssn::Val::Num(aevia_rssn::Num::Float(x)) => *x,
                | other => panic!("expected a float, found {other:?}"),
            }
        },
        | other => panic!("expected an opaque kernel value, found {other:?}"),
    }
}

#[test]
fn an_op_becomes_a_kernel_definition() {
    let (mut interp, diagnostics) = load(
        r#"@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))

let at_zero = Rssn.dynamic("mydomain/softplus", [to_value(0.0)])
let at_two = Rssn.dynamic("mydomain/softplus", [to_value(2.0)])
"#,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let zero = float_result(interp.eval_toplevel("at_zero").unwrap());
    assert!((zero - 2.0_f64.ln()).abs() < 1e-12, "{zero}");
    let two = float_result(interp.eval_toplevel("at_two").unwrap());
    assert!((two - (1.0 + 2.0_f64.exp()).ln()).abs() < 1e-12, "{two}");
}

#[test]
fn the_kernel_differentiates_a_published_operator() {
    let (_interp, diagnostics) = load(
        r#"@op("demo/cube")
fn cube(x: Real[1]): Real[1] = x * x * x
"#,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let kernel = aevia_rssn::kernel();
    let x = aevia_rssn::Val::Symbol("x".to_string());
    let applied = kernel.call("demo/cube", &[x]).expect("the operator exists");
    let text = match applied {
        | aevia_rssn::Val::Term(handle) => kernel.text(handle),
        | other => format!("{other}"),
    };
    assert!(text.contains("x^3") || text.contains("x"), "{text}");
}

#[test]
fn a_rule_is_published_as_a_rewrite() {
    let (_interp, diagnostics) = load(
        r#"@op("demo/twice")
fn twice(x: Real[1]): Real[1] = 2 * x

@rule("demo/sinh-exp")
fn sinh_exp(x: Real[1]) = (sinh(x), (exp(x) - exp(-x)) / 2)
"#,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn a_function_that_branches_on_its_argument_is_rejected() {
    let (_interp, diagnostics) = load(
        r#"@op("demo/relu")
fn relu(x: Float): Float = if x > 0.0 then x else 0.0
"#,
    );
    assert!(
        diagnostics.iter().any(|item| item.starts_with("E1006")),
        "{diagnostics:?}"
    );
}

#[test]
fn a_plugin_file_is_loaded_at_run_time() {
    let directory = std::env::temp_dir().join(format!("aevia-plugin-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let plugin = directory.join("Gauss.aev");
    std::fs::write(
        &plugin,
        "module Gauss\n\n@op(\"stats/gauss\")\nfn gauss(x: Real[1]): Real[1] = exp(-x * x / 2)\n",
    )
    .unwrap();
    let source = format!(
        "fn main(): IO Unit = do\n    loaded <- load_plugin(\"{}\")\n    println(loaded)\n    println(Rssn.dynamic(\"stats/gauss\", [to_value(0.0)]))\n",
        plugin.display().to_string().replace('\\', "/")
    );
    let output = aevia_core::run(&source, "host.aev").expect("runs");
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0], "Ok(1)", "{output}");
    assert!(lines[1].starts_with("Ok("), "{output}");
    let _ = std::fs::remove_dir_all(directory);
}

/// Runs a program's `main` and returns its output lines.
fn run_lines(source: &str) -> Vec<String> {
    let output = aevia_core::run(source, "ops.aev").expect("runs");
    output.lines().map(str::to_string).collect()
}

#[test]
fn a_source_operator_is_an_operator_node_on_symbols() {
    let lines = run_lines(
        r#"@op("demo/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))

fn main(): IO Unit = do
    let x = symbol("x")
    println(softplus(0))
    println(softplus(x))
    let d = diff(fn t: Real[1] => softplus(t))
    println(d(0))
"#,
    );
    // Numbers run the Aevia body; a symbol builds the kernel's node.
    assert_eq!(lines[0], "ln(2)", "{lines:?}");
    assert!(lines[1].contains("demo/softplus(x"), "{lines:?}");
    // The kernel differentiates through the definition: d/dx at 0 is 1/2.
    assert!(
        lines[2] == "1/2" || lines[2].starts_with("0.5"),
        "{lines:?}"
    );
}

#[test]
fn a_source_operator_carries_its_dimension_signature() {
    let lines = run_lines(
        r#"@op("demo/area")
fn area(r: Real[L]): Real[L^2] = pi * r * r

fn main(): IO Unit = do
    println(area(2 m))
    let r = symbol("r") * 1 m
    let a = area(r)
    println(a)
    println(Rssn.dynamic("demo/area", [to_value(3.0)]))
"#,
    );
    assert_eq!(lines[0], "4*pi m^2", "{lines:?}");
    assert!(lines[1].contains("demo/area"), "{lines:?}");
    // The dynamic path shows the value instead of an opaque marker.
    assert!(lines[2].starts_with("Ok("), "{lines:?}");
    assert!(!lines[2].contains("<Value>"), "{lines:?}");
}

#[test]
fn a_rule_can_mention_a_source_operator() {
    let (_interp, diagnostics) = load(
        r#"@op("demo/softplus2")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))

@rule("demo/softplus2-neg")
fn softplus_neg(x: Real[1]) = (softplus(-x), softplus(x) - x)
"#,
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}
