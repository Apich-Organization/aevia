//! Exact word and arbitrary-precision number theory through AST and IR calls.

use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;
use aevia_core::driver;
use aevia_core::value::NumKind;
use aevia_core::value::Value;
use std::path::Path;

#[test]
fn modular_powers_and_common_multiples_use_exact_integer_arithmetic() {
    let source = r#"import Math.NumberTheory { gcd, lcm, mod_pow }
let negative_base = mod_pow(-3, 3, 7)
let zero_exponent = mod_pow(0, 0, 7)
let modulus_one = mod_pow(-3, 0, 1)
let unsigned_exponent = mod_pow(2, 18446744073709551615, 97)
let wide_exponent = mod_pow(-7, 340282366920938463463374607431768223801, 97)
let wide = mod_pow(-12554203470773361527671578846415332832204710888928069035668,
                   340282366920938463463374607431768223801,
                   170141183460469231731687303715884105727)
let wide_one = mod_pow(170141183460469231731687303715884105727, 5, 1)
let gcd_edge = gcd(-9223372036854775808, 0)
let lcm_edge = lcm(-9223372036854775808, 9223372036854775807)
let common = 1267650600228229401496703205376
let gcd_wide = gcd(-6 * common, 15 * common)
let lcm_wide = lcm(-6 * common, 15 * common)
let lcm_zero = lcm(0, common)
"#;
    let program = driver::load_with_source(Path::new("modular_power.aev"), source);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&program.entry().unwrap().checked).with_mode(mode);
        let loads = driver::run_modules(&mut interp, &program);
        assert!(!loads.has_errors(), "{mode:?} {loads:?}");
        for (name, expected) in [
            ("negative_base", "1"),
            ("zero_exponent", "1"),
            ("modulus_one", "0"),
            ("unsigned_exponent", "79"),
            ("wide_exponent", "57"),
            ("wide", "5156499469602237965128690280193276443"),
            ("wide_one", "0"),
            ("gcd_edge", "9223372036854775808"),
            ("lcm_edge", "85070591730234615856620279821087277056"),
            ("gcd_wide", "3802951800684688204490109616128"),
            ("lcm_wide", "38029518006846882044901096161280"),
            ("lcm_zero", "0"),
        ] {
            let Value::Num(number) = interp.eval_toplevel(name).unwrap() else {
                panic!("{mode:?} {name} must be an integer");
            };
            let NumKind::Int(result) = number.kind else {
                panic!("{mode:?} {name} lost its exact Int representation");
            };
            assert_eq!(number.dim, aevia_dim::Dim::one(), "{mode:?} {name}");
            assert_eq!(result.to_string(), expected, "{mode:?} {name}");
        }
    }
}

#[test]
fn modular_power_requires_dimensionless_integers_for_all_arguments() {
    for call in [
        "mod_pow(2 m, 3, 7)",
        "mod_pow(2, 3 m, 7)",
        "mod_pow(2, 3, 7 m)",
        "mod_pow(2.5, 3, 7)",
    ] {
        let source = format!("let invalid = {call}\n");
        let compilation = compile("typed_modular_power.aev", &source);
        assert!(compilation.diagnostics.has_errors(), "{call}");
    }
}

#[test]
fn modular_power_reports_invalid_exponents_and_moduli_as_domain_errors() {
    for (call, message) in [
        ("mod_pow(2, -1, 7)", "nonnegative exponent"),
        ("mod_pow(2, 0, 0)", "positive modulus"),
        ("mod_pow(2, 3, -7)", "positive modulus"),
        (
            "mod_pow(2, -340282366920938463463374607431768223801, 7)",
            "nonnegative exponent",
        ),
    ] {
        let source = format!("let invalid = {call}\n");
        let compilation = compile("invalid_modular_power.aev", &source);
        assert!(
            !compilation.diagnostics.has_errors(),
            "{:?}",
            compilation.diagnostics
        );
        for mode in [Mode::Ast, Mode::Ir] {
            let mut interp = Interp::new(&compilation.checked).with_mode(mode);
            let diagnostics = interp.load(compilation.module.as_ref().unwrap());
            assert!(
                diagnostics
                    .items
                    .iter()
                    .any(|d| d.code == "E0806" && d.message.contains(message)),
                "{mode:?} {call}: {diagnostics:?}",
            );
        }
    }
}
