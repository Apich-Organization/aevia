# `Optimize`

Source: `stdlib/Optimize.aev`

The `Optimize` module of §8.20.

## `let least_squares`

```aevia
let least_squares = least_squares
```

A least-squares solution of `f(x) ≈ 0`, by Levenberg–Marquardt.

**Signature:** `least_squares : ((Tensor<1, Real[d]> -> Tensor<1, Real[e]>), Tensor<1, Real[d]>) -> Result<Tensor<1, Real[d]>, MathError>`

**Errors**

[`NumericError::NotConverged`] when the iteration does not settle.

*Source: Rust documentation of the `aevia-std` function.*
## `let maximize`

```aevia
let maximize = maximize
```

A local maximum of `f`.

**Signature:** `maximize : (Tensor<1, Real[d]> -> Real[e], Tensor<1, Real[d]>) -> Result<Tensor<1, Real[d]>, MathError>`

**Errors**

As [`minimize`].

*Source: Rust documentation of the `aevia-std` function.*
## `let minimize`

```aevia
let minimize = minimize
```

A local minimum of `f`, by the Nelder–Mead simplex method.

**Signature:** `minimize : (Tensor<1, Real[d]> -> Real[e], Tensor<1, Real[d]>) -> Result<Tensor<1, Real[d]>, MathError>`

Derivative-free, which is why it is the default: `f` is a closure and its
gradient is only available numerically.

**Errors**

[`NumericError::Empty`] when the start point is empty, and
[`NumericError::NotConverged`] when the simplex does not shrink.

*Source: Rust documentation of the `aevia-std` function.*
## `let minimize_f`

```aevia
let minimize_f = minimize_f
```

**Signature:** `minimize_f : (Tensor<1, Float[d]> -> Float[e], Tensor<1, Float[d]>) -> Result<Tensor<1, Float[d]>, MathError>`
