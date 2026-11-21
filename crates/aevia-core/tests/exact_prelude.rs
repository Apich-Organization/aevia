//! Arbitrary-precision scalar operations through both execution backends.
use aevia_core::Interp;
use aevia_core::Mode;
use aevia_core::compile;

#[test]
fn integer_division_remainder_and_conversion_stay_exact() {
    let source = r#"let below: Int = -9223372036854775808
let above: Int = 9223372036854775808
let value: Int = 100000000000000000001
let remainder: Int = mod(-value, -7)
let signed: Int = rem(-value, -7)
let fraction: Real = rem(-13 / 6, 3 / 2)
let positive_fraction: Real = mod(-13 / 6, -3 / 2)
fn main(): IO Unit = do
    println("{div(below, -1)} {quot(below, -1)} {div(-9, -4)} {quot(-9, -4)}")
    println("{remainder} {signed} {mod(below, -1)} {gcd(below, 0)} {lcm(value, 2)} {lcm(0, 0)}")
    println("{fraction} {positive_fraction} {to_int(value)} {to_int(-value / 3)} {to_int(2.0^80)} {gcd(above, above)}")
"#;
    let compilation = compile("exact_prelude.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked)
            .with_mode(mode)
            .with_buffer();
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        interp.run_main().unwrap();
        assert_eq!(
            interp.captured(),
            concat!(
                "9223372036854775808 9223372036854775808 3 2\n",
                "4 -3 0 9223372036854775808 200000000000000000002 0\n",
                "-2/3 5/6 100000000000000000001 -33333333333333333333 1208925819614629174706176 9223372036854775808\n",
            ),
            "{mode:?}",
        );
    }
}

#[test]
fn nonfinite_conversion_and_unrepresentable_machine_arguments_report_errors() {
    use aevia_core::builtins::call_simple;
    use aevia_core::value::NumVal;
    use aevia_core::value::Value;
    use num_bigint::BigInt;

    let invalid = call_simple("to_int", vec![Value::float(f64::INFINITY)]).unwrap_err();
    assert_eq!(invalid.code, "E0806");
    let oversized: BigInt = BigInt::from(i64::MAX) + 1;
    let error = call_simple("zeros", vec![Value::Num(NumVal::int(oversized))]).unwrap_err();
    assert_eq!(error.code, "E0802");
}

#[test]
fn ranges_size_from_exact_bounds_before_materializing() {
    use aevia_core::builtins::call_simple;
    use aevia_core::value::NumVal;
    use aevia_core::value::Value;
    use aevia_core::value::show;
    use num_bigint::BigInt;

    let start = Value::int(i64::MAX.saturating_sub(1));
    let end = Value::int(i64::MAX);
    let near_boundary = call_simple("range", vec![start, end, Value::int(7)]).unwrap();
    assert_eq!(show(&near_boundary), "[9223372036854775806]");

    let offset = BigInt::from(i64::MIN) * BigInt::from(4);
    let from_big = |delta: i32| Value::Num(NumVal::int(&offset + BigInt::from(delta)));
    let descending = call_simple("range", vec![from_big(5), from_big(0), Value::int(-2)]).unwrap();
    assert_eq!(
        show(&descending),
        "[-36893488147419103227, -36893488147419103229, -36893488147419103231]"
    );

    let oversized =
        call_simple("range", vec![Value::int(i64::MIN), Value::int(i64::MAX)]).unwrap_err();
    assert_eq!(oversized.code, "E0802");
}

#[test]
fn range_keeps_machine_endpoints_exclusive_and_crosses_into_wider_integers() {
    use aevia_core::value::ExactRep;
    use aevia_core::value::Storage;
    use aevia_core::value::Value;
    use aevia_core::value::repr;

    let source = r#"let wide: Int = 1361129467683753853853498429727072845824
let last = range(9223372036854775806, 9223372036854775807, wide)
let above = range(9223372036854775806, 9223372036854775812, 2)
let below = range(-9223372036854775806, -9223372036854775812, -2)
let min_step = range(9223372036854775807, -9223372036854775808, -9223372036854775808)
let crossing_zero = range(-wide, wide + 1, wide)
let away = range(0, wide, -1)
let equal_bounds = range(wide, wide)
"#;
    let compilation = compile("range_edges.aev", source);
    assert!(
        !compilation.diagnostics.has_errors(),
        "{:?}",
        compilation.diagnostics.items
    );
    for mode in [Mode::Ast, Mode::Ir] {
        let mut interp = Interp::new(&compilation.checked).with_mode(mode);
        let diagnostics = interp.load(compilation.module.as_ref().unwrap());
        assert!(
            !diagnostics.has_errors(),
            "{mode:?}: {:?}",
            diagnostics.items
        );
        for (binding, size, expected) in [
            ("last", 1, "[9223372036854775806]"),
            (
                "above",
                3,
                "[9223372036854775806, 9223372036854775808, 9223372036854775810]",
            ),
            (
                "below",
                3,
                "[-9223372036854775806, -9223372036854775808, -9223372036854775810]",
            ),
            ("min_step", 2, "[9223372036854775807, -1]"),
            (
                "crossing_zero",
                3,
                "[-1361129467683753853853498429727072845824, 0, 1361129467683753853853498429727072845824]",
            ),
            ("away", 0, "[]"),
            ("equal_bounds", 0, "[]"),
        ] {
            let value = interp.eval_toplevel(binding).unwrap();
            let Value::Tensor(data) = &value else {
                panic!("{mode:?} {binding}: expected an Int tensor");
            };
            assert_eq!(data.shape, [size], "{mode:?} {binding}");
            assert!(matches!(
                data.storage,
                Storage::TypedExact(_, ExactRep::Int)
            ));
            assert_eq!(repr(&value), expected, "{mode:?} {binding}");
        }
        let zero_step = interp
            .call_named(
                "range",
                vec![Value::int(0), Value::int(0), Value::int(0)],
                aevia_syntax::Span::default(),
            )
            .unwrap_err();
        assert_eq!(zero_step.code, "E0802");
        assert_eq!(zero_step.message, "range step must not be zero");
    }
}
