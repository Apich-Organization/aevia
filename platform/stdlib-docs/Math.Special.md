# `Math.Special`

Source: `stdlib/Math/Special.aev`

The `Math/Special` module of §8.20.

## `let bessel_j0`

```aevia
let bessel_j0 = bessel_j0
```

The Bessel function of the first kind, order zero.

**Signature:** `bessel_j0 : Scalar<ρ,1> -> Scalar<ρ,1>`

*Source: Rust documentation of the `aevia-std` function.*
## `let bessel_jn`

```aevia
let bessel_jn = bessel_jn
```

The Bessel function of the first kind, order `n`.

**Signature:** `bessel_jn : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>`

Upward recurrence where it is stable (`n <= x`) and Miller's downward
recurrence where it is not, normalised against `J0`.

*Source: Rust documentation of the `aevia-std` function.*
## `let bessel_y0`

```aevia
let bessel_y0 = bessel_y0
```

The Bessel function of the second kind, order zero.

**Signature:** `bessel_y0 : Scalar<ρ,1> -> Scalar<ρ,1>`

*Source: Rust documentation of the `aevia-std` function.*
## `let bessel_yn`

```aevia
let bessel_yn = bessel_yn
```

The Bessel function of the second kind, order `n`.

**Signature:** `bessel_yn : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>`

*Source: Rust documentation of the `aevia-std` function.*
## `let beta`

```aevia
let beta = beta
```

Euler beta function `B(x, y) = Gamma(x) Gamma(y) / Gamma(x + y)`.

**Signature:** `beta : (Scalar<ρ,1>, Scalar<ρ,1>) -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `beta : (Real, Real) -> Real`
- `beta : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `beta : (Int, Int) -> Real`
- `beta : (Int, Float) -> Float`
- ... and 1 more overloads

**Dimensions:** the result is dimensionless

**Examples:**
- `beta(2, 3)` returns a `Rat`
- `beta(2, 3.5)` returns a `Float`
- `beta(2.5, 4.5)` returns a `Float`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `special.beta`.*
## `let chebyshev_t`

```aevia
let chebyshev_t = chebyshev_t
```

`chebyshevt(n, x)`: Chebyshev polynomial of the first kind `T_n(x)`.

**Signature:** `chebyshev_t : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `chebyshev_t : (Real, Real) -> Real`
- `chebyshev_t : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `chebyshev_t : (Int, Real) -> Real`
- `chebyshev_t : (Int, Int) -> Int`

**Dimensions:** the result is dimensionless

**Examples:**
- `chebyshevt(1, 1/3)` returns a `Rat`
- `chebyshevt(0, 1/3)` returns a `Int`
- `chebyshevt(7, 1)` returns a `Int`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `special.chebyshev_t`.*
## `let chebyshev_u`

```aevia
let chebyshev_u = chebyshev_u
```

`chebyshevu(n, x)`: Chebyshev polynomial of the second kind `U_n(x)`.

**Signature:** `chebyshev_u : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `chebyshev_u : (Real, Real) -> Real`
- `chebyshev_u : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `chebyshev_u : (Int, Real) -> Real`

**Dimensions:** the result is dimensionless

**Examples:**
- `chebyshevu(1, 1/3)` returns a `Rat`
- `chebyshevu(0, 1/3)` returns a `Int`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `special.chebyshev_u`.*
## `let erf`

```aevia
let erf = erf
```

Gauss error function `(2 / sqrt(pi)) * int_0^x exp(-t^2) dt`.

**Signature:** `erf : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `erf : (Real) -> Real`
- `erf : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

*Source: rssn operator `special.erf`.*
## `let erfc`

```aevia
let erfc = erfc
```

Complementary error function `1 - erf(x)`.

**Signature:** `erfc : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `erfc : (Real) -> Real`
- `erfc : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

*Source: rssn operator `special.erfc`.*
## `let fresnel_c`

```aevia
let fresnel_c = fresnel_c
```

`fresnelc(x)`: Fresnel cosine integral `int_0^x cos(pi * t^2 / 2) dt`.

**Signature:** `fresnel_c : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `fresnel_c : (Real) -> Real`
- `fresnel_c : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

*Source: rssn operator `special.fresnel_c`.*
## `let fresnel_s`

```aevia
let fresnel_s = fresnel_s
```

`fresnels(x)`: Fresnel sine integral `int_0^x sin(pi * t^2 / 2) dt`.

**Signature:** `fresnel_s : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `fresnel_s : (Real) -> Real`
- `fresnel_s : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

*Source: rssn operator `special.fresnel_s`.*
## `let gamma`

```aevia
let gamma = gamma
```

Euler gamma function `Gamma(z)`.

**Signature:** `gamma : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `gamma : (Real) -> Real`
- `gamma : (Dyn<term>) -> Dyn<term>`
- `gamma : (Int) -> Int`
- `gamma : (Float) -> Float`

**Dimensions:** the result is dimensionless

**Examples:**
- `gamma(1/1)` returns a `Int`
- `gamma(1.75)` returns a `Float`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `special.gamma`.*
## `let hermite_h`

```aevia
let hermite_h = hermite_h
```

The physicists' Hermite polynomial `H_n(x)`.

**Signature:** `hermite_h : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>`

*Source: Rust documentation of the `aevia-std` function.*
## `let j1`

```aevia
let j1 = j1
```

**Signature:** `j1 : Scalar<ρ,1> -> Scalar<ρ,1>`
## `let laguerre_l`

```aevia
let laguerre_l = laguerre_l
```

The Laguerre polynomial `L_n(x)`.

**Signature:** `laguerre_l : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>`

*Source: Rust documentation of the `aevia-std` function.*
## `let legendre_p`

```aevia
let legendre_p = legendre_p
```

The Legendre polynomial `P_n(x)`, by the three-term recurrence.

**Signature:** `legendre_p : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>`

*Source: Rust documentation of the `aevia-std` function.*
## `let lgamma`

```aevia
let lgamma = lgamma
```

Natural logarithm of `|Gamma(z)|`.

**Signature:** `lgamma : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `lgamma : (Real) -> Real`
- `lgamma : (Dyn<term>) -> Dyn<term>`
- `lgamma : (Float) -> Float`

**Dimensions:** the result is dimensionless

**Examples:**
- `lgamma(2.5)` returns a `Float`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `special.lgamma`.*
## `let y1`

```aevia
let y1 = y1
```

**Signature:** `y1 : Scalar<ρ,1> -> Scalar<ρ,1>`
## `let zeta`

```aevia
let zeta = zeta
```

Riemann zeta function `sum_{n=1}^infty n^{-s}`.

**Signature:** `zeta : Scalar<ρ,1> -> Scalar<ρ,1>`

**Kernel signature** (derived):
- `zeta : (Real) -> Real`
- `zeta : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result is dimensionless

*Source: rssn operator `special.zeta`.*
