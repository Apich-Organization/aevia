//! Special values, rational powers and `decide` (§6.3.4, §6.9.2).
//!
//! Each assertion quotes the specification statement it checks. The residual
//! cases the spec does not state are *observed and printed*, not asserted, so a
//! decision about them is made against the spec rather than against the current
//! implementation.

#![allow(clippy::result_large_err)]

use aevia_core::{compile, Interp};

fn eval_source(source: &str) -> Result<Interp, String> {
    let compilation = compile("edges.aev", source);
    if compilation.diagnostics.has_errors() {
        return Err(compilation
            .diagnostics
            .items
            .iter()
            .filter(|item| item.severity == aevia_syntax::Severity::Error)
            .map(|item| format!("{}: {}", item.code, item.message))
            .collect::<Vec<_>>()
            .join("; "));
    }
    let module = compilation.module.expect("a parsed module");
    let mut interp = Interp::new(&compilation.checked).with_buffer();
    let diagnostics = interp.load(&module);
    if diagnostics.has_errors() {
        return Err(diagnostics
            .items
            .iter()
            .filter(|item| item.severity == aevia_syntax::Severity::Error)
            .map(|item| format!("{}: {}", item.code, item.message))
            .collect::<Vec<_>>()
            .join("; "));
    }
    Ok(interp)
}

/// `Ok(rendered)` or `Err("code: message")` for a top-level binding.
fn observe(source: &str, name: &str) -> Result<String, String> {
    let mut interp = eval_source(source)?;
    match interp.eval_toplevel(name) {
        Ok(value) => Ok(aevia_core::value::show(&value)),
        Err(error) => Err(format!("{}: {}", error.code, error.message)),
    }
}

#[test]
fn special_values_follow_the_spec() {
    // §6.3.4: "`ln(0)` 返回 `-oo`（`Real` 的无穷）；`ln(-1)` 构造符号项".
    assert_eq!(observe("let v = ln(0)\n", "v").unwrap(), "-oo");
    assert_eq!(observe("let v = log(0)\n", "v").unwrap(), "-oo");
    // The Float phase is IEEE: `ln(0.0)` is `-Infinity`, spelled `-oo`.
    assert_eq!(observe("let v = ln(0.0)\n", "v").unwrap(), "-oo");
    // The extended-real limits the numeric tables state.
    assert_eq!(observe("let v = exp(-oo)\n", "v").unwrap(), "0");
    assert_eq!(observe("let v = ln(oo)\n", "v").unwrap(), "oo");
    assert_eq!(observe("let v = exp(oo)\n", "v").unwrap(), "oo");
    // `oo` and `-oo` read the way the spec writes them.
    assert_eq!(observe("let v = oo\n", "v").unwrap(), "oo");
    assert_eq!(observe("let v = -oo\n", "v").unwrap(), "-oo");
    // §6.3.4's formal-value rule: `ln(-1)` stays symbolic, never a complex.
    let ln_negative = observe("let v = ln(-1)\n", "v").unwrap();
    assert!(
        ln_negative.starts_with("ln("),
        "ln(-1) is formal, found {ln_negative}"
    );
}

#[test]
fn special_values_are_unchanged_by_phase_spelling() {
    // A Float zero and an exact zero both give `-oo`; a program must not depend
    // on which spelling reached the call.
    assert_eq!(observe("let v = ln(0 * 1)\n", "v").unwrap(), "-oo");
    assert_eq!(observe("let v = ln(0.0 + 0.0)\n", "v").unwrap(), "-oo");
    assert_eq!(observe("let v = exp(-1 * oo)\n", "v").unwrap(), "0");
}

#[test]
fn rational_powers_fold_when_the_root_is_exact() {
    // §6.3.1/§6.3.4: an exact answer stays exact.
    assert_eq!(observe("let v = 0 ^ 0\n", "v").unwrap(), "1");
    assert_eq!(observe("let v = (1/4) ^ (1/2)\n", "v").unwrap(), "0.5");
    // A formal square root is the `sqrt` term (§6.3.1).
    assert_eq!(observe("let v = 2 ^ (1/2)\n", "v").unwrap(), "sqrt(2)");
    // A dimensionful exact base takes the rational exponent exactly (§4.7).
    assert_eq!(observe("let v = (4 m^2) ^ (1/2)\n", "v").unwrap(), "2 m");
    // Exact division by zero is `E0801` (§6.9.2: "`0 / 0`、`1 / 0` 在精确相位
    // 是错误 E0801").
    for expression in ["0 / 0", "1 / 0", "0 ^ (-1)"] {
        let source = format!("let v = {expression}\n");
        let error = observe(&source, "v").expect_err("exact division by zero is E0801");
        assert!(
            error.starts_with("E0801:"),
            "`{expression}` is E0801, found {error}"
        );
    }
    // The odd exact root of a negative number is real, so it folds exactly.
    assert_eq!(observe("let v = 8 ^ (1/3)\n", "v").unwrap(), "2");
    assert_eq!(observe("let v = (-8) ^ (1/3)\n", "v").unwrap(), "-2");
}

#[test]
fn float_phase_follows_ieee() {
    // §6.9.2/§3.8: Float is IEEE: 0/0 is NaN, NaN != NaN, comparisons false.
    assert_eq!(observe("let v = 0.0 / 0.0\n", "v").unwrap(), "nan");
    assert_eq!(observe("let v = nan == nan\n", "v").unwrap(), "false");
    assert_eq!(observe("let v = nan + 1\n", "v").unwrap(), "nan");
    assert_eq!(observe("let v = nan < 1\n", "v").unwrap(), "false");
    assert_eq!(observe("let v = nan >= 1\n", "v").unwrap(), "false");
    assert_eq!(observe("let v = -0.0 == 0.0\n", "v").unwrap(), "true");
}

#[test]
fn exact_infinity_arithmetic_is_partial() {
    // §6.3.4's extended-real fields are only *partially* defined: an undefined
    // form must stay unevaluated and readable, never silently become a finite
    // number (`oo * 0` was `0` before this policy).
    for source in [
        "let v = oo * 0\n",
        "let v = 0 * oo\n",
        "let v = oo - oo\n",
        "let v = oo + (-oo)\n",
        "let v = oo / oo\n",
        "let v = oo ^ 0\n",
        "let v = -oo * 0\n",
    ] {
        let rendered = observe(source, "v").expect("the form is well typed");
        assert!(
            rendered.contains("oo"),
            "the undefined form stays readable, found {rendered:?} for {source:?}"
        );
        assert!(
            rendered != "0" && rendered != "1",
            "the undefined form must not collapse to a finite answer, found {rendered:?} for {source:?}"
        );
    }
    // The quotient/product spelling is preserved.
    assert_eq!(observe("let v = oo / oo\n", "v").unwrap(), "oo/oo");
    assert_eq!(observe("let v = oo * 0\n", "v").unwrap(), "oo*0");
    // The sign of a negative infinity survives the partial rule.
    assert!(observe("let v = -oo * 0\n", "v")
        .unwrap()
        .starts_with("-oo"));
    // `0 ^ oo` has no finite rational exponent, so it is `E0402` rather than an
    // invented value.
    let error = observe("let v = 0 ^ oo\n", "v").expect_err("a finite exponent is required");
    assert!(error.starts_with("E0402:"), "found {error}");
}

#[test]
fn float_phase_infinities_stay_ieee() {
    // The Float path is IEEE: `inf * 0` and `inf - inf` are NaN, not a partial
    // symbolic form.
    assert_eq!(observe("let v = (1.0/0.0) * 0.0\n", "v").unwrap(), "nan");
    assert_eq!(
        observe("let v = (1.0/0.0) - (1.0/0.0)\n", "v").unwrap(),
        "nan"
    );
    assert_eq!(observe("let v = 1.0/0.0\n", "v").unwrap(), "oo");
    assert_eq!(observe("let v = ln(0.0)\n", "v").unwrap(), "-oo");
}

#[test]
fn decide_bridges_prop_to_option_bool() {
    // §3.8.5: `decide : Prop -> Option<Bool>`; §6.3.3: symbolic equality is a
    // proposition, and `decide` is how a program turns it into a Bool.
    assert_eq!(
        observe("let v = decide(1/3 + 2/3 == 1)\n", "v").unwrap(),
        "Some(true)"
    );
    assert_eq!(observe("let v = show(1 == 1)\n", "v").unwrap(), "true");
    // A proposition the checker cannot decide yet is `E0302` or `None`; the
    // observation is recorded rather than invented.
    println!(
        "decide(sin(0)^2 + cos(0)^2 == 1) = {:?}",
        observe("let v = decide(sin(0)^2 + cos(0)^2 == 1)\n", "v")
    );
    for probe in [
        "let v = sin(0)^2\n",
        "let v = sin(0)^2 + cos(0)^2\n",
        "let v = sin(0)^2 + cos(0)^2 == 1\n",
        "let v = decide(sin(0) == sin(0))\n",
        "let v = decide(sin(0)^2 == sin(0)^2)\n",
        "let v = decide(cos(0)^2 + sin(0)^2 == 1)\n",
    ] {
        println!("probe {probe:?} = {:?}", observe(probe, "v"));
    }
    println!(
        "oo - oo = {:?}, oo * 0 = {:?}",
        observe("let v = oo - oo\n", "v"),
        observe("let v = oo * 0\n", "v")
    );
}
