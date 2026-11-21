//! Exact rational helpers shared by the dimension and unit layers.
//!
//! Aevia never stores a dimension exponent as a float: `L^(1/2)` must be the
//! same value as `L^(2/4)`, and `0.3048 m` must convert to feet exactly. All
//! arithmetic in this module is therefore on [`BigRational`].

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::One;
use num_traits::Signed;
use num_traits::ToPrimitive;
use num_traits::Zero;
use std::borrow::Cow;
use std::str::FromStr;

/// Builds a rational from a numerator/denominator pair.
///
/// # Panics
///
/// Panics when `den` is zero; callers in this crate pass literal constants.
#[must_use]
pub fn ratio(
    num: i64,
    den: i64,
) -> BigRational {
    BigRational::new(BigInt::from(num), BigInt::from(den))
}

/// Parses a decimal literal exactly.
///
/// `"1.5"` becomes `3/2`, `"0.3048"` becomes `381/1250`, and `"1e-10"`
/// becomes `1/10000000000`. Underscores used as digit separators are ignored.
/// Returns `None` for anything that is not a plain decimal number.
#[must_use]
#[allow(
    clippy::arithmetic_side_effects,
    reason = "the mantissa and decimal factor use arbitrary-precision integers"
)]
pub fn parse_decimal(text: &str) -> Option<BigRational> {
    let cleaned = if text.contains('_') {
        Cow::Owned(text.replace('_', ""))
    } else {
        Cow::Borrowed(text)
    };
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return None;
    }
    let (negative, rest) = match cleaned.strip_prefix('-') {
        | Some(rest) => (true, rest),
        | None => (false, cleaned.strip_prefix('+').unwrap_or(cleaned)),
    };
    let (mantissa, exponent) = match rest.split_once(['e', 'E']) {
        | Some((mantissa, exponent)) => (mantissa, i64::from_str(exponent).ok()?),
        | None => (rest, 0),
    };
    let (int_part, frac_part) = match mantissa.split_once('.') {
        | Some((int_part, frac_part)) => (int_part, frac_part),
        | None => (mantissa, ""),
    };
    let scale = exponent.checked_sub(i64::try_from(frac_part.len()).ok()?)?;
    if scale.unsigned_abs() > 10_000 {
        return None;
    }
    let digits = match (int_part, frac_part) {
        | ("", "") => return None,
        | (digits, "") | ("", digits) => Cow::Borrowed(digits),
        | _ => Cow::Owned([int_part, frac_part].concat()),
    };
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut value = BigInt::from_str(&digits).ok()?;
    if negative {
        value = -value;
    }
    if scale == 0 || value.is_zero() {
        return Some(BigRational::from_integer(value));
    }
    let factor = BigInt::from(10).pow(u32::try_from(scale.unsigned_abs()).ok()?);
    let result = if scale > 0 {
        BigRational::from_integer(value * factor)
    } else {
        BigRational::new(value, factor)
    };
    Some(result)
}

/// Raises a rational to an integer power.
#[must_use]
pub fn pow_int(
    value: &BigRational,
    exponent: i32,
) -> BigRational {
    if value.is_zero() && exponent < 0 {
        return BigRational::zero();
    }
    value.pow(exponent)
}

/// Renders a rational as `n` or `n/d`.
#[must_use]
pub fn format_rational(value: &BigRational) -> String {
    if value.denom().is_one() {
        value.numer().to_string()
    } else {
        format!("{}/{}", value.numer(), value.denom())
    }
}

/// Renders a rational as a decimal when it terminates, otherwise as `n/d`.
///
/// Used by `show` for exact values: `0.5` prints as `0.5`, `1/3` stays `1/3`.
#[must_use]
#[allow(
    clippy::arithmetic_side_effects,
    reason = "arbitrary-precision arithmetic with the nonzero denominator and fixed factors 2 and 5"
)]
pub fn format_rational_decimal(value: &BigRational) -> String {
    if value.denom().is_one() {
        return value.numer().to_string();
    }
    if value.denom().is_zero() {
        return format_rational(value);
    }
    let mut reduced = value.denom().abs();
    let two = BigInt::from(2);
    let five = BigInt::from(5);
    let mut twos = 0u32;
    let mut fives = 0u32;
    while (&reduced % &two).is_zero() {
        reduced /= &two;
        twos = twos.saturating_add(1);
    }
    while (&reduced % &five).is_zero() {
        reduced /= &five;
        fives = fives.saturating_add(1);
    }
    if !reduced.is_one() {
        return format_rational(value);
    }
    let places = twos.max(fives);
    let ten = BigInt::from(10);
    let scaled = value.numer() * ten.pow(places) / value.denom();
    let negative = scaled.is_negative();
    let digits = scaled.abs().to_string();
    let places = usize::try_from(places).unwrap_or(0);
    if places == 0 {
        return scaled.to_string();
    }
    let body = if let Some((whole, fractional)) =
        digits.split_at_checked(digits.len().saturating_sub(places))
        && !whole.is_empty()
    {
        format!("{whole}.{fractional}")
    } else {
        format!(
            "0.{}{}",
            "0".repeat(places.saturating_sub(digits.len())),
            digits
        )
    };
    let body = body.trim_end_matches('0').trim_end_matches('.').to_string();
    if negative {
        format!("-{body}")
    } else {
        body
    }
}

/// Converts a rational to the nearest `f64` (used only for display and kernels).
#[must_use]
pub fn to_f64(value: &BigRational) -> f64 {
    value.to_f64().unwrap_or(f64::NAN)
}

/// Returns true when the rational is an integer.
#[must_use]
pub fn is_integer(value: &BigRational) -> bool {
    value.denom().is_one()
}

/// Returns the integer value when the rational is an integer that fits in `i64`.
#[must_use]
pub fn as_i64(value: &BigRational) -> Option<i64> {
    if value.denom().is_one() {
        value.numer().to_i64()
    } else {
        None
    }
}

/// True when `value` is exactly zero.
#[must_use]
pub fn is_zero(value: &BigRational) -> bool {
    value.is_zero()
}

/// True when `value` is exactly one.
#[must_use]
pub fn is_one(value: &BigRational) -> bool {
    value.is_one()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_parsing_is_exact() {
        assert_eq!(parse_decimal("1.5"), Some(ratio(3, 2)));
        assert_eq!(parse_decimal("0.3048"), Some(ratio(381, 1250)));
        assert_eq!(parse_decimal("1e-10"), Some(ratio(1, 10_000_000_000)));
        assert_eq!(parse_decimal("-2.5e2"), Some(ratio(-250, 1)));
        assert_eq!(parse_decimal("1_000"), Some(ratio(1000, 1)));
        assert_eq!(parse_decimal("abc"), None);
        assert_eq!(parse_decimal("1e-9223372036854775808"), None);
        assert_eq!(parse_decimal("1.5e-9223372036854775808"), None);
    }

    #[test]
    fn decimal_fast_paths_preserve_literal_boundaries() {
        for (literal, value) in [
            ("5.", ratio(5, 1)),
            ("+.5", ratio(1, 2)),
            ("_  -2_5.e+_0_  _", ratio(-25, 1)),
            ("0.5e1", ratio(5, 1)),
            ("-0.00e10002", ratio(0, 1)),
            ("-0e-10000", ratio(0, 1)),
        ] {
            assert_eq!(parse_decimal(literal), Some(value), "{literal:?}");
        }
        for literal in ["_", ".", "1..", "1e", "1.２", "1.0e-10000", "0e10001"] {
            assert_eq!(parse_decimal(literal), None, "{literal:?}");
        }
    }

    #[test]
    fn rational_powers_are_exact() {
        assert_eq!(pow_int(&ratio(2, 3), 2), ratio(4, 9));
        assert_eq!(pow_int(&ratio(2, 3), -1), ratio(3, 2));
        assert_eq!(pow_int(&ratio(0, 1), -1), ratio(0, 1));
        assert_eq!(pow_int(&ratio(-1, 1), i32::MIN), ratio(1, 1));
        assert_eq!(parse_decimal("1e999999999"), None);
    }

    #[test]
    fn terminating_decimals_render_as_decimals() {
        assert_eq!(format_rational_decimal(&ratio(1, 2)), "0.5");
        assert_eq!(format_rational_decimal(&ratio(3, 1)), "3");
        assert_eq!(format_rational_decimal(&ratio(1, 3)), "1/3");
        assert_eq!(format_rational_decimal(&ratio(-25, 4)), "-6.25");
        let invalid = BigRational::new_raw(7.into(), 0.into());
        assert_eq!(format_rational_decimal(&invalid), "7/0");
    }
}
