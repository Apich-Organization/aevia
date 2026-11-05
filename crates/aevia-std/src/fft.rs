//! The discrete Fourier transform (§8.8).
//!
//! `fft` does not normalise and `ifft` divides by `N`, so `ifft(fft(x)) == x`
//! (§8.8). The other conventions are reachable through [`fft_norm`], which is
//! what `fft_norm(x, Norm.Unitary)` needs.
//!
//! The transform itself is `rustfft`'s: it handles every length (including
//! primes) through Bluestein's algorithm, and it was already in the dependency
//! graph.

use crate::Complex;
use rustfft::FftPlanner;
use rustfft::num_complex::Complex64;

/// The normalisation convention of [`fft_norm`] (§8.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Norm {
    /// `1/N` on the forward transform, none on the inverse (the default).
    Backward,
    /// `1/N` on the inverse transform, none on the forward one.
    Forward,
    /// `1/sqrt(N)` on both.
    Unitary,
}

fn to_internal(input: &[Complex]) -> Vec<Complex64> {
    input
        .iter()
        .map(|value| Complex64::new(value.re, value.im))
        .collect()
}

fn from_internal(input: Vec<Complex64>) -> Vec<Complex> {
    input
        .into_iter()
        .map(|value| Complex::new(value.re, value.im))
        .collect()
}

/// The unnormalised forward transform.
#[must_use]
pub fn fft(input: &[Complex]) -> Vec<Complex> {
    let mut buffer = to_internal(input);
    let mut planner = FftPlanner::new();
    let transform = planner.plan_fft_forward(buffer.len());
    transform.process(&mut buffer);
    from_internal(buffer)
}

/// The inverse transform, scaled by `1/N`.
#[must_use]
pub fn ifft(input: &[Complex]) -> Vec<Complex> {
    let mut buffer = to_internal(input);
    let len = buffer.len();
    let mut planner = FftPlanner::new();
    let transform = planner.plan_fft_inverse(buffer.len());
    transform.process(&mut buffer);
    let scale = 1.0 / len.max(1) as f64;
    from_internal(buffer.into_iter().map(|value| value * scale).collect())
}

/// The forward transform under a chosen normalisation.
#[must_use]
pub fn fft_norm(
    input: &[Complex],
    norm: Norm,
) -> Vec<Complex> {
    let len = input.len().max(1) as f64;
    let scale = match norm {
        | Norm::Backward => 1.0,
        | Norm::Forward | Norm::Unitary => 1.0 / len.sqrt(),
    };
    fft(input)
        .into_iter()
        .map(|value| Complex::new(value.re * scale, value.im * scale))
        .collect()
}

/// The transform of a real signal: the non-redundant half of the spectrum,
/// `n/2 + 1` bins.
#[must_use]
pub fn rfft(input: &[f64]) -> Vec<Complex> {
    let spectrum = fft(&input
        .iter()
        .map(|value| Complex::new(*value, 0.0))
        .collect::<Vec<_>>());
    spectrum.into_iter().take(input.len() / 2 + 1).collect()
}

/// Rebuilds a real signal of length `n` from the half spectrum.
#[must_use]
pub fn irfft(
    input: &[Complex],
    n: usize,
) -> Vec<f64> {
    let mut spectrum = vec![Complex::new(0.0, 0.0); n];
    for (index, value) in input.iter().enumerate() {
        if index < n {
            spectrum[index] = *value;
            if index != 0 && index != n - index && n - index < n {
                spectrum[n - index] = value.conj();
            }
        }
    }
    ifft(&spectrum).into_iter().map(|value| value.re).collect()
}

/// The sample frequencies of a length-`n` transform with spacing `d`.
#[must_use]
pub fn fftfreq(
    n: usize,
    d: f64,
) -> Vec<f64> {
    let mut out = Vec::with_capacity(n);
    if n == 0 || d == 0.0 {
        return out;
    }
    let half = n.div_ceil(2);
    for index in 0..n {
        if index < half {
            out.push(index as f64 / (n as f64 * d));
        } else {
            out.push((index as f64 - n as f64) / (n as f64 * d));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(
        a: f64,
        b: f64,
    ) -> bool {
        (a - b).abs() < 1e-10
    }

    #[test]
    fn the_transform_of_a_constant_is_a_single_spike() {
        let input = vec![Complex::new(1.0, 0.0); 4];
        let spectrum = fft(&input);
        assert!(close(spectrum[0].re, 4.0));
        for bin in &spectrum[1..] {
            assert!(close(bin.re, 0.0) && close(bin.im, 0.0));
        }
    }

    #[test]
    fn the_inverse_undoes_the_forward() {
        let input = vec![
            Complex::new(1.0, -1.0),
            Complex::new(2.0, 0.5),
            Complex::new(-3.0, 2.0),
            Complex::new(0.0, 0.0),
            Complex::new(0.5, 0.25),
        ];
        let round_trip = ifft(&fft(&input));
        for (a, b) in input.iter().zip(round_trip.iter()) {
            assert!(close(a.re, b.re) && close(a.im, b.im));
        }
    }

    #[test]
    fn a_sine_has_its_energy_in_two_bins() {
        let n = 16;
        let signal: Vec<f64> = (0..n)
            .map(|index| (2.0 * std::f64::consts::PI * 2.0 * index as f64 / n as f64).sin())
            .collect();
        let spectrum = rfft(&signal);
        assert!(spectrum[2].abs() > 7.0);
        assert!(spectrum[1].abs() < 1e-9);
        assert!(spectrum[3].abs() < 1e-9);
    }

    #[test]
    fn the_real_inverse_undoes_the_real_forward() {
        let signal = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
        let spectrum = rfft(&signal);
        let rebuilt = irfft(&spectrum, signal.len());
        for (a, b) in signal.iter().zip(rebuilt.iter()) {
            assert!(close(*a, *b), "{a} vs {b}");
        }
    }

    #[test]
    fn the_unitary_normalisation_preserves_energy() {
        let input = vec![Complex::new(3.0, 0.0), Complex::new(0.0, 4.0)];
        let spectrum = fft_norm(&input, Norm::Unitary);
        let energy: f64 = spectrum
            .iter()
            .map(|value| value.re * value.re + value.im * value.im)
            .sum();
        assert!(close(energy, 25.0));
    }

    #[test]
    fn frequencies_wrap_around_the_nyquist_bin() {
        let frequencies = fftfreq(4, 1.0);
        assert!(close(frequencies[0], 0.0));
        assert!(close(frequencies[1], 0.25));
        assert!(close(frequencies[2], -0.5));
        assert!(close(frequencies[3], -0.25));
    }
}
