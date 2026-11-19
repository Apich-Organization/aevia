# `Interpolate`

Source: `stdlib/Interpolate.aev`

The `Interpolate` module of §8.20.

## `let chebyshev_fit`

```aevia
let chebyshev_fit = chebyshev_fit
```

Least-squares fit of a Chebyshev series of the given degree to the points (xs, ys), which is better conditioned than polyfit for high degrees.

**Signature:** `chebyshev_fit : (Tensor<1, Scalar<ρ,1>>, Tensor<1, Scalar<ρ,e>>, Int) -> Tensor<1, Scalar<ρ,e>>`

Returns the coefficients.

**Examples:**
```text
chebyshev_fit([0.0, 0.5, 1.0], [1.0, 2.0, 5.0], 2)
```

*Source: Aevia standard library guide.*
## `let interp1`

```aevia
let interp1 = interp1
```

Linear interpolation: interp1(xs, ys, x) looks up x in the increasing sample points xs and interpolates the matching ys.

**Signature:** `interp1 : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Scalar<ρ,d>) -> Scalar<ρ,e>`

Outside the range of xs the first or last segment is extended as a straight line. The result has ys's dimension.

**Examples:**
```text
interp1([0.0, 1.0, 2.0], [0.0, 10.0, 40.0], 1.5)
```

*Source: Aevia standard library guide.*
## `let interp2`

```aevia
let interp2 = interp2
```

Bilinear interpolation on a grid: interp2(xs, ys, table, x, y) where table[i][j] is the value at (xs[i], ys[j]).

**Signature:** `interp2 : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Tensor<2, Scalar<ρ,f>>, Scalar<ρ,d>, Scalar<ρ,e>) -> Scalar<ρ,f>`

**Examples:**
```text
interp2([0.0, 1.0], [0.0, 1.0], [[0.0, 1.0], [2.0, 3.0]], 0.5, 0.5)
```

*Source: Aevia standard library guide.*
## `let polyfit`

```aevia
let polyfit = polyfit
```

polyfit(xs, ys, degree) fits a least-squares polynomial.

**Signature:** `polyfit : (Tensor<1, Scalar<ρ,1>>, Tensor<1, Scalar<ρ,e>>, Int) -> Tensor<1, Scalar<ρ,e>>`

The x values must be dimensionless; the returned coefficients start with the constant term and rise in power, and carry y's dimension. Evaluate them with polyval.

**Examples:**
```text
polyfit([0.0, 1.0, 2.0, 3.0], [1.0, 3.0, 7.0, 13.0], 2)
```

*Source: Aevia standard library guide.*
## `let polyval`

```aevia
let polyval = polyval
```

polyval(coefficients, x) evaluates a polynomial given its coefficients starting with the constant term, as returned by polyfit.

**Signature:** `polyval : (Tensor<1, Scalar<ρ,e>>, Scalar<ρ,1>) -> Scalar<ρ,e>`

**Examples:**
```text
polyval([1.0, 2.0, 3.0], 2.0)
```

*Source: Aevia standard library guide.*
## `let spline`

```aevia
let spline = spline
```

Cubic-spline interpolation: spline(xs, ys, x) fits a smooth curve through the sample points (xs increasing) and evaluates it at x.

**Signature:** `spline : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Scalar<ρ,d>) -> Scalar<ρ,e>`

Outside the range of xs the end polynomial is extended, which can drift quickly, so avoid extrapolating far.

**Examples:**
```text
spline([0.0, 1.0, 2.0, 3.0], [0.0, 1.0, 4.0, 9.0], 1.5)
```

*Source: Aevia standard library guide.*
