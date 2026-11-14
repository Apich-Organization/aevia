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
use rustfft::num_complex::Complex64;

/// Kernel FFT normalization conventions.
pub use rssn::kernels::fft::Norm;

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
    from_internal(rssn::kernels::fft::fft(&to_internal(input)))
}

/// The inverse transform, scaled by `1/N`.
#[must_use]
pub fn ifft(input: &[Complex]) -> Vec<Complex> {
    from_internal(rssn::kernels::fft::ifft(&to_internal(input)))
}

/// The forward transform under a chosen normalisation.
///
/// `Backward` is the plain forward transform, `Forward` scales it by `1/N`
/// (so the inverse needs none) and `Unitary` by `1/sqrt(N)`.
#[must_use]
pub fn fft_norm(
    input: &[Complex],
    norm: Norm,
) -> Vec<Complex> {
    from_internal(rssn::kernels::fft::fft_norm(&to_internal(input), norm))
}

/// The inverse transform under a chosen normalisation: `Backward` scales by
/// `1/N`, `Forward` not at all, `Unitary` by `1/sqrt(N)`.
#[must_use]
pub fn ifft_norm(
    input: &[Complex],
    norm: Norm,
) -> Vec<Complex> {
    from_internal(rssn::kernels::fft::ifft_norm(&to_internal(input), norm))
}

/// The transform over `axes` of a row-major tensor of the given `shape`,
/// computed one axis at a time (row-column decomposition) with the 1-D
/// transform above. An empty `axes` is the identity.
///
/// # Errors
/// An axis out of range or repeated, or a `shape` that disagrees with the
/// number of elements.
pub fn fft_axes(
    input: &[Complex],
    shape: &[usize],
    axes: &[usize],
    inverse: bool,
    norm: Norm,
) -> Result<Vec<Complex>, String> {
    rssn::kernels::fft::fft_axes(&to_internal(input), shape, axes, inverse, norm).map(from_internal)
}

/// The half-spectrum transform of a real tensor over `axes`: the last listed
/// axis keeps `n/2 + 1` bins. Returns the data and its shape.
///
/// # Errors
/// As [`fft_axes`], and an empty `axes`.
pub fn rfft_axes(
    input: &[f64],
    shape: &[usize],
    axes: &[usize],
) -> Result<(Vec<Complex>, Vec<usize>), String> {
    rssn::kernels::fft::rfft_axes(input, shape, axes)
        .map(|(data, shape)| (from_internal(data), shape))
}

/// Rebuilds a real tensor from the half spectrum of [`rfft_axes`]. `shape` is
/// the half-spectrum shape; `n_last` the length of the last listed axis in the
/// result (`2 * (m - 1)` when the original was even).
///
/// # Errors
/// As [`fft_axes`], and an `n_last` the half spectrum cannot belong to.
pub fn irfft_axes(
    input: &[Complex],
    shape: &[usize],
    axes: &[usize],
    n_last: usize,
) -> Result<(Vec<f64>, Vec<usize>), String> {
    rssn::kernels::fft::irfft_axes(&to_internal(input), shape, axes, n_last)
}

/// Where each element goes under `fftshift` (`inverse = false`) or
/// `ifftshift` of a row-major tensor over `axes`: result element `i` is input
/// element `permutation[i]`.
///
/// # Errors
/// An axis out of range or repeated.
pub fn shift_permutation(
    shape: &[usize],
    axes: &[usize],
    inverse: bool,
) -> Result<Vec<usize>, String> {
    rssn::kernels::fft::shift_permutation(shape, axes, inverse)
}

/// The non-negative sample frequencies of a length-`n` real transform,
/// `n/2 + 1` of them.
#[must_use]
pub fn rfftfreq(
    n: usize,
    d: f64,
) -> Vec<f64> {
    rssn::kernels::fft::rfftfreq(n, d)
}

/// The transform of a real signal: the non-redundant half of the spectrum,
/// `n/2 + 1` bins.
#[must_use]
pub fn rfft(input: &[f64]) -> Vec<Complex> {
    from_internal(rssn::kernels::fft::rfft(input))
}

/// Rebuilds a real signal of length `n` from the half spectrum.
#[must_use]
pub fn irfft(
    input: &[Complex],
    n: usize,
) -> Vec<f64> {
    rssn::kernels::fft::irfft(&to_internal(input), n)
}

/// The sample frequencies of a length-`n` transform with spacing `d`.
#[must_use]
pub fn fftfreq(
    n: usize,
    d: f64,
) -> Vec<f64> {
    rssn::kernels::fft::fftfreq(n, d)
}

/// 2D forward FFT of a row-major `rows × cols` complex array.
///
/// # Errors
/// When `input.len() != rows * cols`.
pub fn fft2(
    input: &[Complex],
    rows: usize,
    cols: usize,
) -> Result<Vec<Complex>, String> {
    rssn::kernels::fft::fft2(&to_internal(input), rows, cols).map(from_internal)
}

/// 2D inverse FFT of a row-major `rows × cols` complex array.
///
/// # Errors
/// When `input.len() != rows * cols`.
pub fn ifft2(
    input: &[Complex],
    rows: usize,
    cols: usize,
) -> Result<Vec<Complex>, String> {
    rssn::kernels::fft::ifft2(&to_internal(input), rows, cols).map(from_internal)
}

/// N-D forward FFT over all axes of a row-major tensor of `shape`.
///
/// # Errors
/// When `shape.iter().product() != input.len()`.
pub fn fftn(
    input: &[Complex],
    shape: &[usize],
) -> Result<Vec<Complex>, String> {
    rssn::kernels::fft::fftn(&to_internal(input), shape).map(from_internal)
}

/// Shifts zero-frequency component to the center of a 1D slice.
#[must_use]
pub fn fftshift(input: &[Complex]) -> Vec<Complex> {
    from_internal(rssn::kernels::fft::fftshift(&to_internal(input)))
}

/// Inverse of [`fftshift`] on a 1D slice.
#[must_use]
pub fn ifftshift(input: &[Complex]) -> Vec<Complex> {
    from_internal(rssn::kernels::fft::ifftshift(&to_internal(input)))
}

/// Orthonormal Type-II Discrete Cosine Transform (`rssn::kernels::signal::dct`).
#[must_use]
pub fn dct(x: &[f64]) -> Vec<f64> {
    rssn::kernels::signal::dct(x)
}

/// Inverse of [`dct`] (`rssn::kernels::signal::idct`).
#[must_use]
pub fn idct(x: &[f64]) -> Vec<f64> {
    rssn::kernels::signal::idct(x)
}

/// Orthonormal Type-II Discrete Sine Transform (`rssn::kernels::signal::dst`).
#[must_use]
pub fn dst(x: &[f64]) -> Vec<f64> {
    rssn::kernels::signal::dst(x)
}

/// Inverse of [`dst`] (`rssn::kernels::signal::idst`).
#[must_use]
pub fn idst(x: &[f64]) -> Vec<f64> {
    rssn::kernels::signal::idst(x)
}

/// Short-Time Fourier Transform (`rssn::kernels::signal::stft`).
#[must_use]
pub fn stft(
    x: &[f64],
    window: &[f64],
    hop: usize,
) -> Vec<Vec<Complex>> {
    rssn::kernels::signal::stft(x, window, hop)
        .into_iter()
        .map(from_internal)
        .collect()
}

/// Power spectrogram `|STFT|^2` (`rssn::kernels::signal::spectrogram`).
#[must_use]
pub fn spectrogram(
    x: &[f64],
    window: &[f64],
    hop: usize,
) -> Vec<Vec<f64>> {
    rssn::kernels::signal::spectrogram(x, window, hop)
}

/// Hann window of length `n` (`rssn::kernels::signal::hann_window`).
#[must_use]
pub fn hann_window(n: usize) -> Vec<f64> {
    rssn::kernels::signal::hann_window(n)
}

/// Hamming window of length `n` (`rssn::kernels::signal::hamming_window`).
#[must_use]
pub fn hamming_window(n: usize) -> Vec<f64> {
    rssn::kernels::signal::hamming_window(n)
}

/// Blackman window of length `n` (`rssn::kernels::signal::blackman_window`).
#[must_use]
pub fn blackman_window(n: usize) -> Vec<f64> {
    rssn::kernels::signal::blackman_window(n)
}

/// Kaiser window of length `n` with parameter `beta` (`rssn::kernels::signal::kaiser_window`).
#[must_use]
pub fn kaiser_window(
    n: usize,
    beta: f64,
) -> Vec<f64> {
    rssn::kernels::signal::kaiser_window(n, beta)
}

/// 1D linear convolution (`rssn::kernels::signal::convolve`).
#[must_use]
pub fn conv(
    a: &[f64],
    b: &[f64],
) -> Vec<f64> {
    rssn::kernels::signal::convolve(a, b)
}

/// 1D cross-correlation (`rssn::kernels::signal::cross_correlation`).
#[must_use]
pub fn xcorr(
    a: &[f64],
    b: &[f64],
) -> Vec<f64> {
    rssn::kernels::signal::cross_correlation(a, b)
}

/// Linear FIR/IIR filter (`rssn::kernels::signal::lfilter`).
#[must_use]
pub fn lfilter(
    b: &[f64],
    a: &[f64],
    x: &[f64],
) -> Vec<f64> {
    rssn::kernels::signal::lfilter(b, a, x)
}

/// Digital low-pass Butterworth filter `(b, a)` (`rssn::kernels::signal::butterworth_lowpass`).
#[must_use]
pub fn butterworth_lowpass(
    order: usize,
    cutoff: f64,
) -> (Vec<f64>, Vec<f64>) {
    rssn::kernels::signal::butterworth_lowpass(order, cutoff)
}

/// Savitzky–Golay smoothing filter (`rssn::kernels::signal::savgol_filter`).
#[must_use]
pub fn savgol_filter(
    x: &[f64],
    window_len: usize,
    poly_order: usize,
) -> Vec<f64> {
    rssn::kernels::signal::savgol_filter(x, window_len, poly_order)
}

/// Single-level 1D discrete wavelet transform (`rssn::kernels::wavelets::dwt`).
///
/// # Errors
/// When the input is too short for the chosen wavelet.
pub fn dwt(
    x: &[f64],
    wavelet: rssn::kernels::wavelets::Wavelet,
) -> Result<(Vec<f64>, Vec<f64>), String> {
    rssn::kernels::wavelets::dwt(x, wavelet).map_err(|e| format!("{e:?}"))
}

/// Single-level 1D inverse discrete wavelet transform (`rssn::kernels::wavelets::idwt`).
///
/// # Errors
/// When the coefficient lengths are incompatible.
pub fn idwt(
    approx: &[f64],
    detail: &[f64],
    wavelet: rssn::kernels::wavelets::Wavelet,
) -> Result<Vec<f64>, String> {
    rssn::kernels::wavelets::idwt(approx, detail, wavelet).map_err(|e| format!("{e:?}"))
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

    fn complexes(values: &[f64]) -> Vec<Complex> {
        values.iter().map(|v| Complex::new(*v, 0.0)).collect()
    }

    /// The textbook 2-D DFT, as the reference.
    fn naive_dft2(
        input: &[Complex],
        rows: usize,
        cols: usize,
    ) -> Vec<Complex> {
        let mut out = Vec::new();
        for k in 0..rows {
            for l in 0..cols {
                let (mut re, mut im) = (0.0, 0.0);
                for m in 0..rows {
                    for n in 0..cols {
                        let angle = -2.0
                            * std::f64::consts::PI
                            * ((k * m) as f64 / rows as f64 + (l * n) as f64 / cols as f64);
                        let x = input[m * cols + n];
                        re += x.re * angle.cos() - x.im * angle.sin();
                        im += x.re * angle.sin() + x.im * angle.cos();
                    }
                }
                out.push(Complex::new(re, im));
            }
        }
        out
    }

    #[test]
    fn the_row_column_transform_matches_the_textbook_2d_dft() {
        let input = complexes(&[
            1.0, 2.0, 0.5, -1.0, 3.0, 4.0, 0.0, 2.5, -2.0, 1.0, 1.5, 0.25,
        ]);
        let fast = fft_axes(&input, &[3, 4], &[0, 1], false, Norm::Backward).unwrap();
        let slow = naive_dft2(&input, 3, 4);
        for (a, b) in fast.iter().zip(slow.iter()) {
            assert!(close(a.re, b.re) && close(a.im, b.im), "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn the_nd_inverse_undoes_the_forward_on_every_axis_subset() {
        let data: Vec<f64> = (0..24).map(|i| ((i * 7) % 11) as f64 - 3.0).collect();
        let input = complexes(&data);
        for axes in [vec![0], vec![2], vec![0, 2], vec![2, 0, 1], vec![]] {
            let there = fft_axes(&input, &[2, 3, 4], &axes, false, Norm::Backward).unwrap();
            let back = fft_axes(&there, &[2, 3, 4], &axes, true, Norm::Backward).unwrap();
            for (a, b) in input.iter().zip(back.iter()) {
                assert!(close(a.re, b.re) && close(a.im, b.im), "axes {axes:?}");
            }
        }
        assert!(fft_axes(&input, &[2, 3, 4], &[3], false, Norm::Backward).is_err());
        assert!(fft_axes(&input, &[2, 3, 4], &[1, 1], false, Norm::Backward).is_err());
    }

    #[test]
    fn forward_normalisation_divides_by_n_and_unitary_by_root_n() {
        let input = complexes(&[2.0, 2.0, 2.0, 2.0]);
        assert!(close(fft_norm(&input, Norm::Forward)[0].re, 2.0));
        assert!(close(fft_norm(&input, Norm::Unitary)[0].re, 4.0));
        let back = ifft_norm(&fft_norm(&input, Norm::Forward), Norm::Forward);
        assert!(close(back[3].re, 2.0));
    }

    #[test]
    fn the_real_nd_transform_round_trips_odd_and_even_extents() {
        for shape in [[3usize, 4], [4, 5], [2, 7]] {
            let total = shape[0] * shape[1];
            let data: Vec<f64> = (0..total).map(|i| ((i * 5) % 9) as f64 - 2.0).collect();
            let (spectrum, half) = rfft_axes(&data, &shape, &[0, 1]).unwrap();
            assert_eq!(half, vec![shape[0], shape[1] / 2 + 1]);
            let (back, full) = irfft_axes(&spectrum, &half, &[0, 1], shape[1]).unwrap();
            assert_eq!(full, shape.to_vec());
            for (a, b) in data.iter().zip(back.iter()) {
                assert!(close(*a, *b), "{shape:?}: {a} vs {b}");
            }
        }
    }

    #[test]
    fn shifting_moves_zero_frequency_to_the_middle_and_back() {
        let even = shift_permutation(&[4], &[0], false).unwrap();
        assert_eq!(even, vec![2, 3, 0, 1]);
        let odd = shift_permutation(&[5], &[0], false).unwrap();
        assert_eq!(odd, vec![3, 4, 0, 1, 2]);
        let undo = shift_permutation(&[5], &[0], true).unwrap();
        let composed: Vec<usize> = odd.iter().map(|i| undo[*i]).collect();
        assert_eq!(composed, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn real_frequencies_are_the_non_negative_half() {
        let frequencies = rfftfreq(5, 0.5);
        assert_eq!(frequencies.len(), 3);
        assert!(close(frequencies[2], 0.8));
    }

    #[test]
    fn frequencies_wrap_around_the_nyquist_bin() {
        let frequencies = fftfreq(4, 1.0);
        assert!(close(frequencies[0], 0.0));
        assert!(close(frequencies[1], 0.25));
        assert!(close(frequencies[2], -0.5));
        assert!(close(frequencies[3], -0.25));
    }

    #[test]
    fn extended_signal_and_wavelet_transforms_round_trip() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let c = dct(&x);
        let back = idct(&c);
        for (a, b) in x.iter().zip(&back) {
            assert!(close(*a, *b));
        }
        let s = dst(&x);
        let s_back = idst(&s);
        for (a, b) in x.iter().zip(&s_back) {
            assert!(close(*a, *b));
        }

        let w = hann_window(4);
        assert_eq!(w.len(), 4);
        assert_eq!(kaiser_window(5, 3.0).len(), 5);
        let spec = spectrogram(&x, &w, 2);
        assert_eq!(spec.len(), 3);

        let (b, a) = butterworth_lowpass(2, 0.4);
        let filt = lfilter(&b, &a, &x);
        assert_eq!(filt.len(), x.len());
        let sg = savgol_filter(&x, 5, 2);
        assert_eq!(sg.len(), x.len());

        let (ca, cd) = dwt(&x, rssn::kernels::wavelets::Wavelet::Haar).unwrap();
        let rec = idwt(&ca, &cd, rssn::kernels::wavelets::Wavelet::Haar).unwrap();
        for (u, v) in x.iter().zip(&rec) {
            assert!(close(*u, *v));
        }
    }
    #[test]
    fn empty_transforms_and_overflowing_shapes_are_handled() {
        assert!(fft(&[]).is_empty());
        assert!(ifft(&[]).is_empty());
        assert!(fft_norm(&[], Norm::Forward).is_empty());
        assert!(ifft_norm(&[], Norm::Unitary).is_empty());
        assert!(fft_axes(&[], &[usize::MAX, 2], &[0], false, Norm::Backward).is_err());
        assert!(shift_permutation(&[usize::MAX, 2], &[0], false).is_err());
        assert!(rfft_axes(&[], &[0], &[0]).is_err());
        assert!(
            fft_axes(&[], &[0, usize::MAX, 2], &[0], false, Norm::Backward)
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn savitzky_golay_coefficients_preserve_polynomial_moments() {
        let coefficients = rssn::kernels::signal::savgol_coeffs(21, 8);
        assert_eq!(coefficients.len(), 21);
        for degree in 0..=8 {
            let moment: f64 = coefficients
                .iter()
                .enumerate()
                .map(|(i, c)| c * ((i as f64 - 10.0) / 10.0).powi(degree))
                .sum();
            let expected = if degree == 0 { 1.0 } else { 0.0 };
            assert!(
                (moment - expected).abs() < 1e-10,
                "degree {degree}: {moment}"
            );
        }
    }
}
