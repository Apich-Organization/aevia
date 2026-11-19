# `Math.Special`

Source: `stdlib/Math/Special.aev`

The `Math/Special` module of §8.20.

## `let bessel_j0`

```aevia
let bessel_j0 = bessel_j0
```

Bessel function of the first kind of order 0, J0(x).

**Signature:** `bessel_j0 : Scalar<ρ,1> -> Scalar<ρ,1>`

**Examples:**
```text
bessel_j0(1.0)
```

*Source: Aevia standard library guide.*
## `let bessel_jn`

```aevia
let bessel_jn = bessel_jn
```

bessel_jn(n, x) is the Bessel function of the first kind of integer order n.

**Signature:** `bessel_jn : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>`

**Examples:**
```text
bessel_jn(2, 1.0)
```

*Source: Aevia standard library guide.*
## `let bessel_y0`

```aevia
let bessel_y0 = bessel_y0
```

Bessel function of the second kind of order 0, Y0(x), for x > 0.

**Signature:** `bessel_y0 : Scalar<ρ,1> -> Scalar<ρ,1>`

**Examples:**
```text
bessel_y0(1.0)
```

*Source: Aevia standard library guide.*
## `let bessel_yn`

```aevia
let bessel_yn = bessel_yn
```

bessel_yn(n, x) is the Bessel function of the second kind of integer order n, for x > 0.

**Signature:** `bessel_yn : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>`

**Examples:**
```text
bessel_yn(2, 1.0)
```

*Source: Aevia standard library guide.*
## `let beta`

```aevia
let beta = beta
```

The beta function B(a, b) = gamma(a) gamma(b) / gamma(a + b) of two dimensionless numbers.

**Signature:** `beta : (Scalar<ρ,1>, Scalar<ρ,1>) -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `beta : (Real, Real) -> Real`
- `beta : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `beta : (Int, Int) -> Real`
- `beta : (Int, Float) -> Float`
- ... and 1 more overloads

**Dimensions:** the result is dimensionless

**Examples:**
```text
beta(2.0, 3.0)
```

*Source: Aevia standard library guide.*
## `let chebyshev_t`

```aevia
let chebyshev_t = chebyshev_t
```

chebyshev_t(n, x) is the Chebyshev polynomial of the first kind T_n(x); on [-1, 1] it equals cos(n acos x).

**Signature:** `chebyshev_t : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `chebyshev_t : (Real, Real) -> Real`
- `chebyshev_t : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `chebyshev_t : (Int, Real) -> Real`
- `chebyshev_t : (Int, Int) -> Int`

**Dimensions:** the result is dimensionless

**Examples:**
```text
chebyshev_t(3, 0.5)
```

*Source: Aevia standard library guide.*
## `let chebyshev_u`

```aevia
let chebyshev_u = chebyshev_u
```

chebyshev_u(n, x) is the Chebyshev polynomial of the second kind U_n(x).

**Signature:** `chebyshev_u : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `chebyshev_u : (Real, Real) -> Real`
- `chebyshev_u : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `chebyshev_u : (Int, Real) -> Real`

**Dimensions:** the result is dimensionless

**Examples:**
```text
chebyshev_u(3, 0.5)
```

*Source: Aevia standard library guide.*
## `let erf`

```aevia
let erf = erf
```

The error function, 2/sqrt(pi) times the integral of exp(-t^2) from 0 to x.

**Signature:** `erf : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `erf : (Real) -> Real`
- `erf : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

It maps the reals onto (-1, 1) and is the building block of the normal CDF.

**Examples:**
```text
erf(1.0)
```

*Source: Aevia standard library guide.*
## `let erfc`

```aevia
let erfc = erfc
```

The complementary error function, 1 - erf(x).

**Signature:** `erfc : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `erfc : (Real) -> Real`
- `erfc : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

Use it for large x, where 1 - erf(x) loses all its digits to cancellation.

**Examples:**
```text
erfc(3.0)
```

*Source: Aevia standard library guide.*
## `let fresnel_c`

```aevia
let fresnel_c = fresnel_c
```

Fresnel cosine integral C(x), the integral of cos(pi t^2 / 2) from 0 to x.

**Signature:** `fresnel_c : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `fresnel_c : (Real) -> Real`
- `fresnel_c : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

Exact arguments stay symbolic; a Float argument gives a Float.

**Examples:**
```text
fresnel_c(1.0)
```

*Source: Aevia standard library guide.*
## `let fresnel_s`

```aevia
let fresnel_s = fresnel_s
```

Fresnel sine integral S(x), the integral of sin(pi t^2 / 2) from 0 to x.

**Signature:** `fresnel_s : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `fresnel_s : (Real) -> Real`
- `fresnel_s : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

Exact arguments stay symbolic so you can differentiate or integrate them; a Float argument gives a Float.

**Examples:**
```text
fresnel_s(1.0)
```

*Source: Aevia standard library guide.*
## `let gamma`

```aevia
let gamma = gamma
```

The gamma function, which extends factorial: gamma(n) == factorial(n - 1) for positive integers.

**Signature:** `gamma : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `gamma : (Real) -> Real`
- `gamma : (Dyn<term>) -> Dyn<term>`
- `gamma : (Int) -> Int`
- `gamma : (Float) -> Float`

**Dimensions:** the result is dimensionless

The argument must be dimensionless. At zero and the negative integers it has poles, where a Float result is an infinity or an enormous number.

**Examples:**
```text
gamma(5.0)
```

*Source: Aevia standard library guide.*
## `let hermite_h`

```aevia
let hermite_h = hermite_h
```

hermite_h(n, x) is the physicists' Hermite polynomial H_n(x), which appears in the quantum harmonic oscillator.

**Signature:** `hermite_h : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>`

**Examples:**
```text
hermite_h(3, 0.5)
```

*Source: Aevia standard library guide.*
## `let j1`

```aevia
let j1 = j1
```

Bessel function of the first kind of order 1; the same as bessel_j1.

**Signature:** `j1 : Scalar<ρ,1> -> Scalar<ρ,1>`

**Examples:**
```text
j1(1.0)
```

*Source: Aevia standard library guide.*
## `let laguerre_l`

```aevia
let laguerre_l = laguerre_l
```

laguerre_l(n, x) is the Laguerre polynomial L_n(x), which appears in the radial hydrogen wavefunctions.

**Signature:** `laguerre_l : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>`

**Examples:**
```text
laguerre_l(2, 0.5)
```

*Source: Aevia standard library guide.*
## `let legendre_p`

```aevia
let legendre_p = legendre_p
```

legendre_p(n, x) is the Legendre polynomial of degree n, orthogonal on [-1, 1].

**Signature:** `legendre_p : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>`

**Examples:**
```text
legendre_p(2, 0.5)
```

*Source: Aevia standard library guide.*
## `let lgamma`

```aevia
let lgamma = lgamma
```

Natural logarithm of the absolute value of gamma.

**Signature:** `lgamma : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `lgamma : (Real) -> Real`
- `lgamma : (Dyn<term>) -> Dyn<term>`
- `lgamma : (Float) -> Float`

**Dimensions:** the result is dimensionless

Prefer it to log(gamma(x)) for large x, where gamma itself overflows.

**Examples:**
```text
lgamma(100.0)
```

*Source: Aevia standard library guide.*
## `let y1`

```aevia
let y1 = y1
```

Bessel function of the second kind of order 1; the same as bessel_y1.

**Signature:** `y1 : Scalar<ρ,1> -> Scalar<ρ,1>`

**Examples:**
```text
y1(1.0)
```

*Source: Aevia standard library guide.*
## `let zeta`

```aevia
let zeta = zeta
```

The Riemann zeta function for a dimensionless argument.

**Signature:** `zeta : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `zeta : (Real) -> Real`
- `zeta : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

zeta(2) is pi^2/6. It has a pole at 1.

**Examples:**
```text
zeta(2.0)
```

*Source: Aevia standard library guide.*
