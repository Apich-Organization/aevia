# `Math`

Source: `stdlib/Math.aev`

The `Math` module of §8.20.

## `let acos`

```aevia
let acos = acos
```

Principal inverse cosine of `x`, in `[0, pi]` for real `x` in `[-1, 1]`.

**Signature:** `acos : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `acos : (Real) -> Real`
- `acos : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

*Source: rssn operator `elementary.acos`.*
## `let acosh`

```aevia
let acosh = acosh
```

Principal inverse hyperbolic cosine `ln(x + sqrt(x^2 - 1))`.

**Signature:** `acosh : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `acosh : (Real) -> Real`
- `acosh : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

*Source: rssn operator `elementary.acosh`.*
## `let asin`

```aevia
let asin = asin
```

Principal inverse sine of `x`, in `[-pi/2, pi/2]` for real `x` in `[-1, 1]`.

**Signature:** `asin : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `asin : (Real) -> Real`
- `asin : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

*Source: rssn operator `elementary.asin`.*
## `let asinh`

```aevia
let asinh = asinh
```

Inverse hyperbolic sine `ln(x + sqrt(x^2 + 1))`.

**Signature:** `asinh : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `asinh : (Real) -> Real`
- `asinh : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

*Source: rssn operator `elementary.asinh`.*
## `let atan`

```aevia
let atan = atan
```

Principal inverse tangent of `x`, in `(-pi/2, pi/2)` for real `x`.

**Signature:** `atan : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `atan : (Real) -> Real`
- `atan : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

*Source: rssn operator `elementary.atan`.*
## `let atan2`

```aevia
let atan2 = atan2
```

Two-argument arctangent of `(y, x)`, giving the angle in `(-pi, pi]` from the positive x-axis.

**Signature:** `atan2 : (Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `atan2 : (Real, Real) -> Real`
- `atan2 : (Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is an angle (dimensionless)

*Source: rssn operator `elementary.atan2`.*
## `let atanh`

```aevia
let atanh = atanh
```

Inverse hyperbolic tangent `ln((1 + x) / (1 - x)) / 2`.

**Signature:** `atanh : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `atanh : (Real) -> Real`
- `atanh : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

*Source: rssn operator `elementary.atanh`.*
## `let cbrt`

```aevia
let cbrt = cbrt
```

**Signature:** `cbrt : Scalar<ρ,d> -> Scalar<ρ,d^(1/3)>`
## `let cos`

```aevia
let cos = cos
```

Cosine of angle `x` in radians.

**Signature:** `cos : Scalar<ρ,1> -> Scalar<ρ,1>`

**Signature:** `cos : Complex[1] -> Complex[1]`

**Kernel signature** (derived):
- `cos : (Real) -> Real`
- `cos : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

`cos z`.

*Source: rssn operator `elementary.cos`; rustdoc of `crate::kernels::complex_scalar::cos` (by name).*
## `let cosh`

```aevia
let cosh = cosh
```

Hyperbolic cosine `(e^x + e^-x) / 2`.

**Signature:** `cosh : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `cosh : (Real) -> Real`
- `cosh : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

*Source: rssn operator `elementary.cosh`.*
## `let curry`

```aevia
let curry = curry
```

**Signature:** `curry : ((a, b) -> c) -> (a -> (b -> c))`
## `let e`

```aevia
let e = e
```

**Signature:** `e : Real[1]`
## `let exp`

```aevia
let exp = exp
```

Natural exponential function `e^x`.

**Signature:** `exp : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `exp : (Real) -> Real`
- `exp : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

*Source: rssn operator `elementary.exp`.*
## `let hypot`

```aevia
let hypot = hypot
```

**Signature:** `hypot : (Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,d>`
## `let i`

```aevia
let i = i
```

**Signature:** `i : Complex[1]`
## `let ln`

```aevia
let ln = ln
```

Natural logarithm (base `e`) of `x`.

**Signature:** `ln : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `ln : (Real) -> Real`
- `ln : (Dyn<term>) -> Dyn<term>`
- `ln : (Float) -> Float`

**Dimensions:** the result is dimensionless

**Examples:**
- `ln(0.3)` returns a `Float`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `elementary.ln`.*
## `let log`

```aevia
let log = log
```

Logarithm of `x` to base `b`, equal to `ln(x) / ln(b)`.

**Signature:** `log : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `log : (Real, Real) -> Real`
- `log : (Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

*Source: rssn operator `elementary.log`.*
## `let log10`

```aevia
let log10 = log10
```

**Signature:** `log10 : Scalar<ρ,1> -> Scalar<ρ,1>`
## `let log2`

```aevia
let log2 = log2
```

**Signature:** `log2 : Scalar<ρ,1> -> Scalar<ρ,1>`
## `let nan`

```aevia
let nan = nan
```

**Signature:** `nan : Float[1]`
## `let oo`

```aevia
let oo = oo
```

`oo`: positive real infinity (`+infinity`).

**Signature:** `oo : Real[1]`

**Kernel signature** (derived):
- `oo : () -> Real`
- `oo : () -> Dyn<term>`

**Dimensions:** dimensionally polymorphic

*Source: rssn operator `calculus.oo`.*
## `let partial`

```aevia
let partial = partial
```

**Signature:** `partial : ((a, b) -> c, a) -> (b -> c)`
## `let pi`

```aevia
let pi = pi
```

`pi`: Archimedes' constant, the ratio of a circle's circumference to its diameter.

**Signature:** `pi : Real[1]`

**Kernel signature** (derived):
- `pi : () -> Real`
- `pi : () -> Dyn<term>`

**Dimensions:** the result is dimensionless

*Source: rssn operator `elementary.pi`.*
## `let sin`

```aevia
let sin = sin
```

Sine of angle `x` in radians.

**Signature:** `sin : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `sin : (Real) -> Real`
- `sin : (Dyn<term>) -> Dyn<term>`
- `sin : (Float) -> Float`

**Dimensions:** the result is dimensionless

`sin z`.

**Examples:**
- `sin(0.5)` returns a `Float`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `elementary.sin`; rustdoc of `crate::kernels::complex_scalar::sin` (by name).*
## `let sinh`

```aevia
let sinh = sinh
```

Hyperbolic sine `(e^x - e^-x) / 2`.

**Signature:** `sinh : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `sinh : (Real) -> Real`
- `sinh : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

*Source: rssn operator `elementary.sinh`.*
## `let sqrt`

```aevia
let sqrt = sqrt
```

Principal square root of `x`.

**Signature:** `sqrt : Scalar<ρ,d> -> Scalar<ρ,d^(1/2)>`

**Kernel signature** (derived):
- `sqrt : (Real) -> Real`
- `sqrt : (Dyn<term>) -> Dyn<term>`

**Dimensions:** kernel rule `Root(2)`

*Source: rssn operator `elementary.sqrt`.*
## `let tan`

```aevia
let tan = tan
```

Tangent `sin(x) / cos(x)` of angle `x` in radians.

**Signature:** `tan : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `tan : (Real) -> Real`
- `tan : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

*Source: rssn operator `elementary.tan`.*
## `let tanh`

```aevia
let tanh = tanh
```

Hyperbolic tangent `sinh(x) / cosh(x)`.

**Signature:** `tanh : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `tanh : (Real) -> Real`
- `tanh : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

*Source: rssn operator `elementary.tanh`.*
## `let uncurry`

```aevia
let uncurry = uncurry
```

**Signature:** `uncurry : ((a -> (b -> c))) -> ((a, b) -> c)`

**Signature:** `uncurry : (a -> (b -> c)) -> ((a, b) -> c)`
