# `Math.Complex`

Source: `stdlib/Math/Complex.aev`

The `Math/Complex` module of §8.20.

## `let arg`

```aevia
let arg = arg
```

The phase angle of a complex number in radians, in (-pi, pi].

**Signature:** `arg : Complex[d] -> Real[1]`

**Kernel signature** (overlay):
- `arg : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is an angle (dimensionless)

**Examples:**
```text
arg(complex(0, 1))
```

*Source: Aevia standard library guide.*
## `let complex`

```aevia
let complex = complex
```

Builds a complex number from real and imaginary parts with a common dimension.

**Signature:** `complex : (Real[d], Real[d]) -> Complex[d]`

**Examples:**
```text
complex(3 m, 400 cm)
```

*Source: Aevia standard library guide.*
## `let complex_exp`

```aevia
let complex_exp = complex_exp
```

The complex exponential e^z for a dimensionless Complex z.

**Signature:** `complex_exp : Complex[1] -> Complex[1]`

**Examples:**
```text
complex_exp(complex(0, 1))
```

*Source: Aevia standard library guide.*
## `let complex_ln`

```aevia
let complex_ln = complex_ln
```

The principal natural logarithm of a dimensionless Complex number; the imaginary part lies in (-pi, pi].

**Signature:** `complex_ln : Complex[1] -> Complex[1]`

**Examples:**
```text
complex_ln(complex(1, 1))
```

*Source: Aevia standard library guide.*
## `let complex_sin`

```aevia
let complex_sin = complex_sin
```

The sine of a dimensionless Complex number.

**Signature:** `complex_sin : Complex[1] -> Complex[1]`

**Examples:**
```text
complex_sin(complex(0, 1))
```

*Source: Aevia standard library guide.*
## `let complex_sqrt`

```aevia
let complex_sqrt = complex_sqrt
```

Principal square root of a complex number.

**Signature:** `complex_sqrt : Complex[d] -> Complex[d^(1/2)]`

The dimension is raised to the power 1/2.

**Examples:**
```text
complex_sqrt(complex(-4, 0))
```

*Source: Aevia standard library guide.*
## `let conj`

```aevia
let conj = conj
```

The complex conjugate: the imaginary part changes sign.

**Signature:** `conj : Complex[d] -> Complex[d]`

**Kernel signature** (derived):
- `conj : (Dyn<term>) -> Dyn<term>`
- `conj : (Int) -> Int`
- `conj : (Real) -> Real`

**Dimensions:** the result has the dimension of argument 1

**Examples:**
```text
conj(complex(1, 2))
```

*Source: Aevia standard library guide.*
## `let cos`

```aevia
let cos = cos
```

Trigonometric function.

**Signature:** `cos : Scalar<ρ,1> -> Scalar<ρ,1>`

**Signature:** `cos : Complex[1] -> Complex[1]`

**Kernel signature** (overlay):
- `cos : (Real) -> Real`
- `cos : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

Its argument must be dimensionless; angle units such as deg carry an exact conversion scale.

**Examples:**
```text
sin(30 deg)
cos(0.0)
```

*Source: Aevia standard library guide.*
## `let im`

```aevia
let im = im
```

The imaginary part of a complex number.

**Signature:** `im : Complex[d] -> Real[d]`

**Kernel signature** (overlay):
- `im : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result has the dimension of argument 1

**Examples:**
```text
im(complex(1, 2))
```

*Source: Aevia standard library guide.*
## `let magnitude`

```aevia
let magnitude = magnitude
```

The modulus (absolute value) of a complex number, keeping its dimension.

**Signature:** `magnitude : Complex[d] -> Real[d]`

Use this instead of abs for Complex.

**Examples:**
```text
magnitude(complex(3, 4))
```

*Source: Aevia standard library guide.*
## `let re`

```aevia
let re = re
```

The real part of a complex number.

**Signature:** `re : Complex[d] -> Real[d]`

**Kernel signature** (overlay):
- `re : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result has the dimension of argument 1

**Examples:**
```text
re(complex(1, 2))
```

*Source: Aevia standard library guide.*
