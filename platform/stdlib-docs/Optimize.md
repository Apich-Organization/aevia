# `Optimize`

Source: `stdlib/Optimize.aev`

The `Optimize` module of §8.20.

## `let least_squares`

```aevia
let least_squares = least_squares
```

least_squares(residuals, x0) finds the parameters x that minimise the sum of squares of the vector-valued residual function, starting from x0.

**Signature:** `least_squares : ((Tensor<1, Real[d]> -> Tensor<1, Real[e]>), Tensor<1, Real[d]>) -> Result<Tensor<1, Real[d]>, MathError>`

Use it to fit a model to data; for a linear model lstsq is simpler and exact.

**Examples:**
```text
least_squares(fn p => [p[0] - 1, p[1] - 2], [0, 0])
```

*Source: Aevia standard library guide.*
## `let maximize`

```aevia
let maximize = maximize
```

maximize(f, x0) searches for a local maximum of a scalar function of a vector, starting at x0, and returns the maximiser as a Result.

**Signature:** `maximize : (Tensor<1, Real[d]> -> Real[e], Tensor<1, Real[d]>) -> Result<Tensor<1, Real[d]>, MathError>`

**Examples:**
```text
maximize(fn v => -(v[0] - 1)^2, [0])
```

*Source: Aevia standard library guide.*
## `let minimize`

```aevia
let minimize = minimize
```

minimize(f, x0) searches for a local minimum of a scalar function of a vector, starting at x0, and returns the minimiser as a Result.

**Signature:** `minimize : (Tensor<1, Real[d]> -> Real[e], Tensor<1, Real[d]>) -> Result<Tensor<1, Real[d]>, MathError>`

The result is only as global as your starting point. Use maximize for a maximum or minimize_f for Float functions.

**Examples:**
```text
minimize(fn v => (v[0] - 1)^2 + (v[1] - 2)^2, [0, 0])
```

*Source: Aevia standard library guide.*
## `let minimize_f`

```aevia
let minimize_f = minimize_f
```

Float version of minimize: finds a local minimum of a Float function of a vector from the given starting vector, returning a Result.

**Signature:** `minimize_f : (Tensor<1, Float[d]> -> Float[e], Tensor<1, Float[d]>) -> Result<Tensor<1, Float[d]>, MathError>`

**Examples:**
```text
minimize_f(fn v => (v[0] - 1.0)^2 + (v[1] - 2.0)^2, [0.0, 0.0])
```

*Source: Aevia standard library guide.*
