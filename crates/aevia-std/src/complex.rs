//! Complex arithmetic (§8.15).
//!
//! `Complex<d>` is a scalar representation at run time, so the runtime keeps the
//! value and calls these helpers. `abs` is deliberately absent: §8.15 says the
//! modulus of a complex number is `magnitude`, because `abs : Complex[d] ->
//! Real[d]` would collide with `abs : Scalar<ρ,d> -> Scalar<ρ,d>`.

pub use rssn::kernels::complex_scalar::arg;
pub use rssn::kernels::complex_scalar::complex;
pub use rssn::kernels::complex_scalar::cos;
pub use rssn::kernels::complex_scalar::magnitude;
pub use rssn::kernels::complex_scalar::norm_squared;
pub use rssn::kernels::complex_scalar::pow;
pub use rssn::kernels::complex_scalar::powc;
pub use rssn::kernels::complex_scalar::sin;

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
