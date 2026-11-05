//! Complex arithmetic (§8.15).
//!
//! `Complex<d>` is a scalar representation at run time, so the runtime keeps the
//! value and calls these helpers. `abs` is deliberately absent: §8.15 says the
//! modulus of a complex number is `magnitude`, because `abs : Complex[d] ->
//! Real[d]` would collide with `abs : Scalar<ρ,d> -> Scalar<ρ,d>`.

use crate::Complex;

/// `re + im i`.
#[must_use]
pub fn complex(
    re: f64,
    im: f64,
) -> Complex {
    Complex::new(re, im)
}

/// The modulus, `|z|`.
#[must_use]
pub fn magnitude(value: Complex) -> f64 {
    value.abs()
}

/// The argument, `arg z`, in `(-π, π]`.
#[must_use]
pub fn arg(value: Complex) -> f64 {
    value.arg()
}

/// `z * conj(z)` is real; this returns that real part, `|z|^2`.
#[must_use]
pub fn norm_squared(value: Complex) -> f64 {
    value.re * value.re + value.im * value.im
}

/// `z^n` for an integer exponent, by repeated squaring.
#[must_use]
pub fn pow(
    value: Complex,
    exponent: i64,
) -> Complex {
    if exponent == 0 {
        return Complex::new(1.0, 0.0);
    }
    let mut result = Complex::new(1.0, 0.0);
    let mut base = if exponent < 0 {
        Complex::new(1.0, 0.0).div(value)
    } else {
        value
    };
    let mut remaining = exponent.unsigned_abs();
    while remaining > 0 {
        if remaining & 1 == 1 {
            result = result.mul(base);
        }
        base = base.mul(base);
        remaining >>= 1;
    }
    result
}

/// `sin z`.
#[must_use]
pub fn sin(value: Complex) -> Complex {
    Complex::new(
        value.re.sin() * value.im.cosh(),
        value.re.cos() * value.im.sinh(),
    )
}

/// `cos z`.
#[must_use]
pub fn cos(value: Complex) -> Complex {
    Complex::new(
        value.re.cos() * value.im.cosh(),
        -value.re.sin() * value.im.sinh(),
    )
}

/// `z^w = exp(w ln z)`.
#[must_use]
pub fn powc(
    base: Complex,
    exponent: Complex,
) -> Complex {
    exponent.mul(base.ln()).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(
        a: f64,
        b: f64,
    ) -> bool {
        (a - b).abs() < 1e-12
    }

    #[test]
    fn the_square_root_of_minus_one_is_i() {
        let root = Complex::new(-1.0, 0.0).sqrt();
        assert!(close(root.re, 0.0));
        assert!(close(root.im, 1.0));
    }

    #[test]
    fn the_exponential_of_i_pi_is_minus_one() {
        let value = Complex::new(0.0, std::f64::consts::PI).exp();
        assert!(close(value.re, -1.0));
        assert!(close(value.im, 0.0));
    }

    #[test]
    fn the_logarithm_inverts_the_exponential() {
        let original = Complex::new(1.5, -0.75);
        let round_trip = original.ln().exp();
        assert!(close(round_trip.re, original.re));
        assert!(close(round_trip.im, original.im));
    }

    #[test]
    fn integer_powers_are_repeated_products() {
        let value = Complex::new(0.0, 1.0);
        let squared = pow(value, 2);
        assert!(close(squared.re, -1.0));
        assert!(close(squared.im, 0.0));
        let inverse = pow(value, -1);
        assert!(close(inverse.re, 0.0));
        assert!(close(inverse.im, -1.0));
    }

    #[test]
    fn magnitude_and_argument_describe_the_plane() {
        let value = Complex::new(3.0, 4.0);
        assert!(close(magnitude(value), 5.0));
        assert!(close(arg(value), 0.927_295_218_001_612_2));
        assert!(close(norm_squared(value), 25.0));
    }
}
