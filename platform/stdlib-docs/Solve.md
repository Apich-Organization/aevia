# `Solve`

Source: `stdlib/Solve.aev`

The `Solve` module of §8.20.

## `let linsolve`

```aevia
let linsolve = linsolve
```

Solution `x` to the linear system `A x = b`.

**Signature:** `linsolve : (Tensor<2, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Result<Tensor<1, Scalar<div_rep(ρ),e/d>>, LinAlgError>`

**Kernel signature** (derived):
- `linsolve : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `linsolve : (Tensor<2, Int>, Tensor<1, Int>) -> Tensor<1, Dyn<term>>`
- `linsolve : (Tensor<2, Real>, Tensor<1, Real>) -> Tensor<1, Real>`
- `linsolve : (Tensor<2, Dyn<term>>, Tensor<1, Dyn<term>>) -> Tensor<1, Dyn<term>>`

**Dimensions:** kernel rule `Custom("")`

**Examples:**
- `linsolve(list(list(1, 1, 1)), list(3))` returns a `[Expr]`
- `linsolve(list(list(1, 1), list(1, 1)), list(1, 2))` returns a `[Never]`
- `linsolve(list(list(1, 1/2), list(1/2, 1/3)), list(1/3, 1/4))` returns a `[Rat]`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `linalg.linsolve`.*
## `let nsolve`

```aevia
let nsolve = nsolve
```

Numerically finds a root of `eq_or_system` starting from `guess`.

**Signature:** `nsolve : (Real[d] -> Real[e], Real[d]) -> Result<Real[d], MathError>`

**Kernel signature** (derived):
- `nsolve : (Dyn<term>, Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** the result has the dimension of argument 2

*Source: rssn operator `solve.nsolve`.*
## `let nsolve_f`

```aevia
let nsolve_f = nsolve_f
```

**Signature:** `nsolve_f : (Float[d] -> Float[e], Float[d]) -> Result<Float[d], MathError>`
## `let root`

```aevia
let root = root
```

A root of `f` near `guess`, by Newton's method with a bisection fallback.

**Signature:** `root : 同上`

**Errors**

[`NumericError::NotConverged`] when neither method finds a sign change or a
stationary point.

*Source: Rust documentation of the `aevia-std` function.*
## `let solve`

```aevia
let solve = solve
```

Symbolically solves an equation, inequality, or system for `var_or_vars`.

**Signature:** `solve : (Real[d] -> Real[e]) -> Result<Solutions<Real,d>, MathError>`

**Kernel signature** (overlay):
- `solve : (Dyn<term>, Symbol) -> Dyn<term>`
- `solve : (Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** the result has the dimension of argument 2

*Source: rssn operator `solve.solve`.*
## `let solve_complex`

```aevia
let solve_complex = solve_complex
```

Finds all complex roots of polynomial `poly` in `var`.

**Signature:** `solve_complex : (Real[d] -> Complex[e]) -> Result<Solutions<Complex,d>, MathError>`

**Kernel signature** (derived):
- `solve_complex : (Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** the result has the dimension of argument 2

**Examples:**
- `solve_complex(x^2 + 1, x)` returns a `[Expr]`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `solve.solve_complex`.*
## `let solve_system`

```aevia
let solve_system = solve_system
```

A solution of the square system `f(x) = 0`, by Newton's method.

**Signature:** `solve_system : (Tensor<1, Real[d]> -> Tensor<1, Real[e]>, Tensor<1, Real[d]>) -> Result<Tensor<1, Real[d]>, MathError>`

**Errors**

[`NumericError::NotConverged`] when the iteration does not settle, and
[`NumericError::Singular`] when the Jacobian vanishes.

*Source: Rust documentation of the `aevia-std` function.*
