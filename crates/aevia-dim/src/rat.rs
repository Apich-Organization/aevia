//! Exact rational helpers shared by the dimension and unit layers.
//!
//! Aevia never stores a dimension exponent as a float: `L^(1/2)` must be the
//! same value as `L^(2/4)`, and `0.3048 m` must convert to feet exactly. All
//! arithmetic in this module is therefore on [`BigRational`].

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::str::FromStr;

/// Builds a rational from a numerator/denominator pair.
///
/// # Panics
///
/// Panics when `den` is zero; callers in this crate pass literal constants.
#[must_use]
pub fn ratio(num: i64, den: i64) -> BigRational {
    BigRational::new(BigInt::from(num), BigInt::from(den))
}

/// Parses a decimal literal exactly.
///
/// `"1.5"` becomes `3/2`, `"0.3048"` becomes `381/1250`, and `"1e-10"`
/// becomes `1/10000000000`. Underscores used as digit separators are ignored.
/// Returns `None` for anything that is not a plain decimal number.
#[must_use]
pub fn parse_decimal(text: &str) -> Option<BigRational> {
    let cleaned: String = text.chars().filter(|c| *c != '_').collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return None;
    }
    let (negative, rest) = match cleaned.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, cleaned.strip_prefix('+').unwrap_or(cleaned)),
    };
    let (mantissa, exponent) = match rest.find(['e', 'E']) {
        Some(at) => {
            let exp = i64::from_str(&rest[at + 1..]).ok()?;
            (&rest[..at], exp)
        }
        None => (rest, 0),
    };
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((int_part, frac_part)) => (int_part, frac_part),
        None => (mantissa, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    let digits: String = format!("{int_part}{frac_part}");
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut value = BigInt::from_str(&digits).ok()?;
    if negative {
        value = -value;
    }
    let scale = exponent - i64::try_from(frac_part.len()).ok()?;
    let ten = BigInt::from(10);
    let result = if scale >= 0 {
        BigRational::from_integer(value * ten.pow(u32::try_from(scale).ok()?))
    } else {
        BigRational::new(value, ten.pow(u32::try_from(-scale).ok()?))
    };
    Some(result)
}

/// Raises a rational to an integer power.
#[must_use]
pub fn pow_int(value: &BigRational, exponent: i32) -> BigRational {
    let magnitude = exponent.unsigned_abs();
    let powered = value.pow(i32::try_from(magnitude).unwrap_or(i32::MAX));
    if exponent < 0 {
        powered.recip()
    } else {
        powered
    }
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
pub fn format_rational_decimal(value: &BigRational) -> String {
    if value.denom().is_one() {
        return value.numer().to_string();
    }
    let denom = value.denom().clone();
    let mut reduced = denom;
    let two = BigInt::from(2);
    let five = BigInt::from(5);
    let mut twos = 0u32;
    let mut fives = 0u32;
    while (&reduced % &two).is_zero() {
        reduced /= &two;
        twos += 1;
    }
    while (&reduced % &five).is_zero() {
        reduced /= &five;
        fives += 1;
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
    let body = if places == 0 {
        digits
    } else if digits.len() > places {
        let split = digits.len() - places;
        format!("{}.{}", &digits[..split], &digits[split..])
    } else {
        format!("0.{}{}", "0".repeat(places - digits.len()), digits)
    };
    let body = body.trim_end_matches('0').trim_end_matches('.').to_string();
    if negative { format!("-{body}") } else { body }
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
    }

    #[test]
    fn rational_powers_are_exact() {
        assert_eq!(pow_int(&ratio(2, 3), 2), ratio(4, 9));
        assert_eq!(pow_int(&ratio(2, 3), -1), ratio(3, 2));
    }

    #[test]
    fn terminating_decimals_render_as_decimals() {
        assert_eq!(format_rational_decimal(&ratio(1, 2)), "0.5");
        assert_eq!(format_rational_decimal(&ratio(3, 1)), "3");
        assert_eq!(format_rational_decimal(&ratio(1, 3)), "1/3");
        assert_eq!(format_rational_decimal(&ratio(-25, 4)), "-6.25");
    }
}
