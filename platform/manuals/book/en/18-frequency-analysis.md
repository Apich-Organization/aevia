# Frequency analysis of a vibration record

A position sensor reports one length every 125 ms. The movement has a fixed offset and a small periodic component. We want to report the baseline position, vibration frequency and displacement amplitude. A discrete Fourier transform separates the record into frequency bins. The sample spacing determines their frequencies, and the transform convention determines how to read their amplitudes.

## Make a reference signal

Start with eight samples. The values alternate through `1.0 m`, `1.2 m`, `1.0 m` and `0.8 m`, so one cycle occupies four sample intervals. This trace has a known displacement amplitude of `0.2 m`. The eight-point record contains two cycles, which puts the signal exactly on a Fourier bin.

Save the program as `vibration.aev`. `rfft` takes real samples and returns the nonnegative half of the complex spectrum. Keep the original shape and time step with the spectrum if you plan to reconstruct the signal. `rfftfreq` turns bin indices into `Float[1/Time]` where the input time step carries its physical unit.

```aevia,run
import FFT { rfft, irfft, rfftfreq }
import Math.Complex { magnitude }

let gap: Float[Time] = 0.125 s
let samples: Tensor<1, Float[Length]> = [1.0 m, 1.2 m, 1.0 m, 0.8 m, 1.0 m, 1.2 m, 1.0 m, 0.8 m]
let spectrum = rfft(samples)
let frequencies = rfftfreq(8, gap)
let baseline = magnitude(spectrum[0]) / 8
let amplitude = 2 * magnitude(spectrum[2]) / 8
let reconstructed = irfft(spectrum, 8)

fn close_length(actual: Real[Length], expected: Real[Length]): Bool =
    match decide(abs(actual - expected) < 0.000001 m) with
    | Some(yes) -> yes
    | None -> false

test "the second bin resolves a two-hertz vibration" = frequencies[2] == 2.0 Hz
test "FFT scaling recovers the offset and amplitude" = close_length(baseline, 1.0 m) && close_length(amplitude, 0.2 m)
test "inverse restores the measured series" = max_all(map(fn x => abs(x), reconstructed - samples)) < 0.000001 m

fn main(): IO Unit = do
    println("frequencies: {frequencies}")
    println("baseline: {baseline}")
    println("vibration: {amplitude} at {frequencies[2]}")
```

## Read the spectrum

The eight samples span a transform window of `8 × 0.125 s = 1 s`. Consecutive bins therefore differ by `1 Hz`. The second positive bin is the `2 Hz` vibration and the zero-frequency coefficient holds the sum of the samples. Dividing the latter by eight gives the baseline. A real signal has matching positive and negative frequency components with conjugate phases. Only the positive side is stored by `rfft`, so doubling its magnitude and dividing by eight recovers the displacement amplitude. This rule applies to interior bins. The DC bin and, for even lengths, the Nyquist bin each represent a single coefficient.

Both `magnitude` results have the same dimension as the input. `magnitude` returns an Aevia `Real`, whose comparisons yield propositions. `close_length` uses `decide` to turn a decidable comparison into a test result. The inverse transform in the third test uses `Float` samples and can compare the maximum absolute residual directly. The three tests check the frequency axis, scaling convention and reconstruction independently.

Run `aevia test vibration.aev`. Change the gap to `0.25 s` while leaving the samples unchanged. The recovered amplitude stays near `0.2 m`. The frequency now reads `1 Hz`, and the first test fails until you update its expected value. A sample file without its timing metadata is insufficient to identify physical frequencies.

## Reconstruct an odd-length record

For an even record of length `N`, the final stored bin is precisely the Nyquist bin when there are `N/2 + 1` values. An odd record of length five and an even record of length four both produce three coefficients, with different meanings for the final bin. Supply the original length explicitly to `irfft` to disambiguate. Try the five-point trace below, which uses the same units as the vibration record.

```aevia,run
import FFT { rfft, irfft, rfftfreq }

let samples: Tensor<1, Float[Length]> = [0.0 m, 0.1 m, 0.2 m, 0.1 m, 0.0 m]
let half = rfft(samples)
let recovered = irfft(half, 5)
let bins = rfftfreq(5, 0.25 s)

test "an odd-length signal keeps its last sample" = shape(recovered) == [5] && abs(recovered[4] - samples[4]) < 0.000001 m

fn main(): IO Unit = do
    println("nonnegative bins: {bins}")
    println("recovered samples: {recovered}")
```

## Investigate a frequency between bins

Replace the hand-written samples with values from `sin` at `2.5 Hz`. To form the sine argument, multiply frequency by elapsed time and by `2π`, so it is dimensionless. Use a longer record with the same sample spacing or apply a window before transforming. Compare the largest bins and the distribution across nearby frequencies. A record that ends during a cycle spreads energy over bins. Windowing changes that spread and the amplitude scale, so calibrate the amplitude against a known tone before using it on measurements.

Finish by writing down the maximum observable frequency at the chosen sample spacing. With `0.125 s` between samples, it is `4 Hz`. Frequencies above that can appear below it after sampling. A higher-resolution transform of the same samples leaves this sampling limit unchanged.
