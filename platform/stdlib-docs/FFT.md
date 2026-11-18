# `FFT`

Source: `stdlib/FFT.aev`

The `FFT` module of §8.20.

## `type Norm`

```aevia
type Norm
```
## `type AxisScale`

```aevia
type AxisScale
```
## `let fft`

```aevia
let fft = fft
```

The unnormalised forward transform.

**Signature:** `fft : Tensor<1, X[d]> -> Tensor<1, Complex[d]>`

*Source: Rust documentation of the `aevia-std` function.*
## `let fft2`

```aevia
let fft2 = fft2
```

2D forward FFT of a row-major `rows × cols` complex array.

**Signature:** `fft2 : Tensor<n, X[d]> -> Tensor<n, Complex[d]>`

**Errors**
When `input.len() != rows * cols`.

*Source: Rust documentation of the `aevia-std` function.*
## `let fft_axis`

```aevia
let fft_axis = fft_axis
```

**Signature:** `fft_axis : (Tensor<n, X[d]>, Int) -> Tensor<n, Complex[d]>`
## `let fft_norm`

```aevia
let fft_norm = fft_norm
```

The forward transform under a chosen normalisation.

**Signature:** `fft_norm : (Tensor<1, X[d]>, Norm) -> Tensor<1, Complex[d]>`

`Backward` is the plain forward transform, `Forward` scales it by `1/N`
(so the inverse needs none) and `Unitary` by `1/sqrt(N)`.

*Source: Rust documentation of the `aevia-std` function.*
## `let fftfreq`

```aevia
let fftfreq = fftfreq
```

The sample frequencies of a length-`n` transform with spacing `d`.

**Signature:** `fftfreq : (Int, Float[T]) -> Tensor<1, Float[1/T]>`

*Source: Rust documentation of the `aevia-std` function.*
## `let fftn`

```aevia
let fftn = fftn
```

N-D forward FFT over all axes of a row-major tensor of `shape`.

**Signature:** `fftn : Tensor<n, X[d]> -> Tensor<n, Complex[d]>`

**Errors**
When `shape.iter().product() != input.len()`.

*Source: Rust documentation of the `aevia-std` function.*
## `let fftn_axes`

```aevia
let fftn_axes = fftn_axes
```

**Signature:** `fftn_axes : (Tensor<n, X[d]>, Tensor<1, Int>) -> Tensor<n, Complex[d]>`
## `let fftn_norm`

```aevia
let fftn_norm = fftn_norm
```

**Signature:** `fftn_norm : (Tensor<n, X[d]>, Norm) -> Tensor<n, Complex[d]>`
## `let fftshift`

```aevia
let fftshift = fftshift
```

Shifts zero-frequency component to the center of a 1D slice.

**Signature:** `fftshift : Tensor<n, a> -> Tensor<n, a>`

*Source: Rust documentation of the `aevia-std` function.*
## `let fftshift_axis`

```aevia
let fftshift_axis = fftshift_axis
```

**Signature:** `fftshift_axis : (Tensor<n, a>, Int) -> Tensor<n, a>`
## `let ifft`

```aevia
let ifft = ifft
```

The inverse transform, scaled by `1/N`.

**Signature:** `ifft : Tensor<1, X[d]> -> Tensor<1, Complex[d]>`

*Source: Rust documentation of the `aevia-std` function.*
## `let ifft2`

```aevia
let ifft2 = ifft2
```

2D inverse FFT of a row-major `rows × cols` complex array.

**Signature:** `ifft2 : Tensor<n, X[d]> -> Tensor<n, Complex[d]>`

**Errors**
When `input.len() != rows * cols`.

*Source: Rust documentation of the `aevia-std` function.*
## `let ifft_axis`

```aevia
let ifft_axis = ifft_axis
```

**Signature:** `ifft_axis : (Tensor<n, X[d]>, Int) -> Tensor<n, Complex[d]>`
## `let ifft_norm`

```aevia
let ifft_norm = ifft_norm
```

The inverse transform under a chosen normalisation: `Backward` scales by `1/N`, `Forward` not at all, `Unitary` by `1/sqrt(N)`.

**Signature:** `ifft_norm : (Tensor<1, X[d]>, Norm) -> Tensor<1, Complex[d]>`

*Source: Rust documentation of the `aevia-std` function.*
## `let ifftn`

```aevia
let ifftn = ifftn
```

**Signature:** `ifftn : Tensor<n, X[d]> -> Tensor<n, Complex[d]>`
## `let ifftn_axes`

```aevia
let ifftn_axes = ifftn_axes
```

**Signature:** `ifftn_axes : (Tensor<n, X[d]>, Tensor<1, Int>) -> Tensor<n, Complex[d]>`
## `let ifftn_norm`

```aevia
let ifftn_norm = ifftn_norm
```

**Signature:** `ifftn_norm : (Tensor<n, X[d]>, Norm) -> Tensor<n, Complex[d]>`
## `let ifftshift`

```aevia
let ifftshift = ifftshift
```

Inverse of [`fftshift`] on a 1D slice.

**Signature:** `ifftshift : Tensor<n, a> -> Tensor<n, a>`

*Source: Rust documentation of the `aevia-std` function.*
## `let ifftshift_axis`

```aevia
let ifftshift_axis = ifftshift_axis
```

**Signature:** `ifftshift_axis : (Tensor<n, a>, Int) -> Tensor<n, a>`
## `let irfft`

```aevia
let irfft = irfft
```

Rebuilds a real signal of length `n` from the half spectrum.

**Signature:** `irfft : Tensor<1, Complex[d]> -> Tensor<1, Float[d]>`

**Signature:** `irfft : (Tensor<1, Complex[d]>, Int) -> Tensor<1, Float[d]>`

*Source: Rust documentation of the `aevia-std` function.*
## `let irfft2`

```aevia
let irfft2 = irfft2
```

**Signature:** `irfft2 : Tensor<n, Complex[d]> -> Tensor<n, Float[d]>`

**Signature:** `irfft2 : (Tensor<n, Complex[d]>, Int) -> Tensor<n, Float[d]>`
## `let irfftn`

```aevia
let irfftn = irfftn
```

**Signature:** `irfftn : Tensor<n, Complex[d]> -> Tensor<n, Float[d]>`

**Signature:** `irfftn : (Tensor<n, Complex[d]>, Int) -> Tensor<n, Float[d]>`
## `let irfftn_axes`

```aevia
let irfftn_axes = irfftn_axes
```

**Signature:** `irfftn_axes : (Tensor<n, Complex[d]>, Tensor<1, Int>) -> Tensor<n, Float[d]>`

**Signature:** `irfftn_axes : (Tensor<n, Complex[d]>, Tensor<1, Int>, Int) -> Tensor<n, Float[d]>`
## `let rfft`

```aevia
let rfft = rfft
```

The transform of a real signal: the non-redundant half of the spectrum, `n/2 + 1` bins.

**Signature:** `rfft : Tensor<1, Float[d]> -> Tensor<1, Complex[d]>`

*Source: Rust documentation of the `aevia-std` function.*
## `let rfft2`

```aevia
let rfft2 = rfft2
```

**Signature:** `rfft2 : Tensor<n, Float[d]> -> Tensor<n, Complex[d]>`
## `let rfftfreq`

```aevia
let rfftfreq = rfftfreq
```

The non-negative sample frequencies of a length-`n` real transform, `n/2 + 1` of them.

**Signature:** `rfftfreq : (Int, Float[T]) -> Tensor<1, Float[1/T]>`

*Source: Rust documentation of the `aevia-std` function.*
## `let rfftn`

```aevia
let rfftn = rfftn
```

**Signature:** `rfftn : Tensor<n, Float[d]> -> Tensor<n, Complex[d]>`
## `let rfftn_axes`

```aevia
let rfftn_axes = rfftn_axes
```

**Signature:** `rfftn_axes : (Tensor<n, Float[d]>, Tensor<1, Int>) -> Tensor<n, Complex[d]>`
