//! Complex arithmetic (§8.15).
//!
//! `Complex<d>` is a scalar representation at run time, so the runtime keeps the
//! value and calls these helpers. `abs` is deliberately absent: §8.15 says the
//! modulus of a complex number is `magnitude`, because `abs : Complex[d] ->
//! Real[d]` would collide with `abs : Scalar<ρ,d> -> Scalar<ρ,d>`.
//! The principal square root uses the shared [`crate::Complex::sqrt`] method.

pub use rssn::kernels::complex_scalar::arg;
pub use rssn::kernels::complex_scalar::complex;
pub use rssn::kernels::complex_scalar::cos;
pub use rssn::kernels::complex_scalar::magnitude;
pub use rssn::kernels::complex_scalar::norm_squared;
pub use rssn::kernels::complex_scalar::pow;
pub use rssn::kernels::complex_scalar::powc;
pub use rssn::kernels::complex_scalar::sin;

/// Real part of `z`.
#[must_use]
pub const fn re(z: crate::Complex) -> f64 {
    z.re
}

/// Imaginary part of `z`.
#[must_use]
pub const fn im(z: crate::Complex) -> f64 {
    z.im
}

/// Complex conjugate of `z`.
#[must_use]
pub fn conj(z: crate::Complex) -> crate::Complex {
    z.conj()
}

/// Reciprocal `1 / z`.
#[must_use]
pub fn recip(z: crate::Complex) -> crate::Complex {
    z.recip()
}

/// Principal complex exponential `exp(z)`.
#[must_use]
pub fn exp(z: crate::Complex) -> crate::Complex {
    z.exp()
}

/// Principal complex logarithm `ln(z)`.
#[must_use]
pub fn ln(z: crate::Complex) -> crate::Complex {
    z.ln()
}

/// Principal complex square root `sqrt(z)`.
#[must_use]
pub fn sqrt(z: crate::Complex) -> crate::Complex {
    z.sqrt()
}

/// Complex tangent `tan(z) = sin(z) / cos(z)`.
#[must_use]
pub fn tan(z: crate::Complex) -> crate::Complex {
    sin(z).div(cos(z))
}

/// Complex hyperbolic sine `sinh(z) = (exp(z) - exp(-z)) / 2`.
#[must_use]
pub fn sinh(z: crate::Complex) -> crate::Complex {
    let ep = z.exp();
    let em = crate::Complex::new(-z.re, -z.im).exp();
    crate::Complex::new(0.5 * (ep.re - em.re), 0.5 * (ep.im - em.im))
}

/// Complex hyperbolic cosine `cosh(z) = (exp(z) + exp(-z)) / 2`.
#[must_use]
pub fn cosh(z: crate::Complex) -> crate::Complex {
    let ep = z.exp();
    let em = crate::Complex::new(-z.re, -z.im).exp();
    crate::Complex::new(f64::midpoint(ep.re, em.re), f64::midpoint(ep.im, em.im))
}

/// Complex hyperbolic tangent `tanh(z) = sinh(z) / cosh(z)`.
#[must_use]
pub fn tanh(z: crate::Complex) -> crate::Complex {
    sinh(z).div(cosh(z))
}

/// Constructs a complex number from polar coordinates `(r, theta)`.
#[must_use]
pub fn from_polar(
    r: f64,
    theta: f64,
) -> crate::Complex {
    crate::Complex::new(r * theta.cos(), r * theta.sin())
}

#[cfg(test)]
use crate::Complex;

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
    fn the_principal_square_root_keeps_tiny_components_on_either_side() {
        let tiny = 5e-151_f64;
        for sign in [1.0, -1.0] {
            let positive = Complex::new(1e300, sign).sqrt();
            assert!((positive.re / 1e150 - 1.0).abs() <= f64::EPSILON);
            assert!((positive.im / (sign * tiny) - 1.0).abs() <= f64::EPSILON);
            let negative = Complex::new(-1e300, sign).sqrt();
            assert!((negative.re / tiny - 1.0).abs() <= f64::EPSILON);
            assert!((negative.im / (sign * 1e150) - 1.0).abs() <= f64::EPSILON);
        }
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

    #[test]
    fn elementary_trig_hyperbolic_and_polar_helpers_agree() {
        let z = complex(0.4, -0.3);
        assert!(close(re(z), 0.4));
        assert!(close(im(z), -0.3));
        assert!(close(conj(z).im, 0.3));
        assert!(close(z.mul(recip(z)).re, 1.0));
        assert!(close(z.mul(recip(z)).im, 0.0));

        let t = tan(z);
        let expected_t = sin(z).div(cos(z));
        assert!(close(t.re, expected_t.re) && close(t.im, expected_t.im));

        let sh = sinh(z);
        let ch = cosh(z);
        let ch2_minus_sh2 = ch.mul(ch).re - sh.mul(sh).re;
        assert!(close(ch2_minus_sh2, 1.0));

        let th = tanh(z);
        let expected_th = sh.div(ch);
        assert!(close(th.re, expected_th.re) && close(th.im, expected_th.im));

        let p = from_polar(2.0, std::f64::consts::FRAC_PI_4);
        assert!(close(magnitude(p), 2.0));
        assert!(close(arg(p), std::f64::consts::FRAC_PI_4));
        assert!(close(exp(ln(z)).re, z.re) && close(exp(ln(z)).im, z.im));
        assert!(close(sqrt(complex(0.0, 2.0)).re, 1.0));
        assert!(close(sqrt(complex(0.0, 2.0)).im, 1.0));
    }
}
