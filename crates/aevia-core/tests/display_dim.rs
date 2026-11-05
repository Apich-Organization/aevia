//! Dimension spelling in value display (§4.1, §8.9.1).
//!
//! The SI spelling lives in `aevia-dim` so a diagnostic and a value agree; the
//! fractional-exponent case is the one that used to be wrong (`s^1` for a
//! Brownian increment, whose unit is `s^(1/2)`).

use aevia_core::value::NumVal;
use aevia_core::value::Value;
use aevia_core::value::show;
use aevia_core::value::si_unit_text;
use aevia_dim::Dim;
use num_bigint::BigInt;
use num_rational::BigRational;

fn rational(
    numerator: i64,
    denominator: i64,
) -> BigRational {
    BigRational::new(BigInt::from(numerator), BigInt::from(denominator))
}

#[test]
fn si_symbols_keep_rational_exponents() {
    let time = Dim::base("Time");
    let length = Dim::base("Length");
    assert_eq!(si_unit_text(&Dim::one()), "");
    assert_eq!(si_unit_text(&time), "s");
    assert_eq!(si_unit_text(&time.pow(&rational(1, 2))), "s^(1/2)");
    assert_eq!(si_unit_text(&time.pow(&rational(3, 2))), "s^(3/2)");
    assert_eq!(si_unit_text(&time.pow(&rational(-1, 2))), "1/s^(1/2)");
    assert_eq!(
        si_unit_text(&length.div(&time.pow(&rational(2, 1)))),
        "m/s^2"
    );
}

/// A `wiener` increment carries `T^(1/2)`; its rendered value must say so.
#[test]
fn a_wiener_increment_displays_its_sqrt_time_unit() {
    let dt = Value::Num(NumVal::float(0.5, Dim::base("Time")));
    let increments = aevia_core::builtins::call_simple(
        "wiener",
        vec![Value::int(3), Value::int(1), Value::int(2), dt],
    )
    .expect("wiener");
    let rendered = show(&increments);
    assert!(
        rendered.contains("s^(1/2)"),
        "a Brownian increment displays its `s^(1/2)` unit, found {rendered}"
    );
    assert!(
        !rendered.contains("s^1"),
        "the old `s^1` spelling must be gone, found {rendered}"
    );
}
