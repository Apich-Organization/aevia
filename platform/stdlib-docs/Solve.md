# `Solve`

Source: `stdlib/Solve.aev`

The `Solve` module of §8.20.

## `let linsolve`

```aevia
let linsolve = linsolve
```

Solves A x = b for x, returning a Result; a singular or non-square system gives Err(LinAlgError).

**Signature:** `linsolve : (Tensor<2, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Result<Tensor<1, Scalar<div_rep(ρ),e/d>>, LinAlgError>`

**Kernel signature** (derived):
- `linsolve : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `linsolve : (Tensor<2, Int>, Tensor<1, Int>) -> Tensor<1, Dyn<term>>`
- `linsolve : (Tensor<2, Real>, Tensor<1, Real>) -> Tensor<1, Real>`
- `linsolve : (Tensor<2, Dyn<term>>, Tensor<1, Dyn<term>>) -> Tensor<1, Dyn<term>>`

**Dimensions:** kernel rule `Custom("")`

The solution has the dimension of b divided by that of A. For over-determined systems use lstsq.

**Examples:**
```text
linsolve([[2.0, 0.0], [0.0, 4.0]], [2.0, 4.0])
```

*Source: Aevia standard library guide.*
## `let nsolve`

```aevia
let nsolve = nsolve
```

nsolve(f, x0) finds a root of f numerically, starting the search at x0.

**Signature:** `nsolve : (Real[d] -> Real[e], Real[d]) -> Result<Real[d], MathError>`

**Kernel signature** (overlay):
- `nsolve : (Dyn<term>, Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** the result has the dimension of argument 2

It returns Ok(root) or Err(MathError) when the iteration does not converge, so check the Result and try a different start. root is a synonym.

**Examples:**
```text
nsolve(fn x => x^2 - 2, 1)
```

*Source: Aevia standard library guide.*
## `let nsolve_f`

```aevia
let nsolve_f = nsolve_f
```

Float version of nsolve: finds a root of a Float function from a Float starting point.

**Signature:** `nsolve_f : (Float[d] -> Float[e], Float[d]) -> Result<Float[d], MathError>`

Returns Err(MathError) when it does not converge.

**Examples:**
```text
nsolve_f(fn x => x * x - 2.0, 1.0)
```

*Source: Aevia standard library guide.*
## `let root`

```aevia
let root = root
```

A synonym for nsolve: finds a root of f near the starting point x0, returning a Result.

**Signature:** `root : 同上`

**Examples:**
```text
root(fn x => x * x - 2.0, 1.0)
```

*Source: Aevia standard library guide.*
## `let solve`

```aevia
let solve = solve
```

solve(f) finds the real roots of f(x) = 0 symbolically.

**Signature:** `solve : (Real[d] -> Real[e]) -> Result<Solutions<Real,d>, MathError>`

**Kernel signature** (overlay):
- `solve : (Dyn<term>, Symbol) -> Dyn<term>`
- `solve : (Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** the result has the dimension of argument 2

It returns a Result holding a record with the roots, their multiplicities, any conditions they depend on (such as a != 0) and a complete flag that is true when the list is exhaustive. For numerical root finding use nsolve.

**Examples:**
```text
solve(fn x => x^2 - 4)
```

*Source: Aevia standard library guide.*
## `let solve_complex`

```aevia
let solve_complex = solve_complex
```

Like solve, but for a function that returns a Complex value: it finds all roots of f(x) = 0, including non-real ones, as a Result holding the same record as solve.

**Signature:** `solve_complex : (Real[d] -> Complex[e]) -> Result<Solutions<Complex,d>, MathError>`

**Kernel signature** (overlay):
- `solve_complex : (Dyn<term>, Symbol) -> Tensor<1, Dyn<term>>`
- `solve_complex : (Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** the result has the dimension of argument 2

**Examples:**
```text
solve_complex(fn x => complex(x^2 + 1, 0))
```

*Source: Aevia standard library guide.*
## `let solve_system`

```aevia
let solve_system = solve_system
```

solve_system(f, x0) solves a system of equations f(x) = 0, where f maps a vector to a vector of the same length, starting from x0.

**Signature:** `solve_system : (Tensor<1, Real[d]> -> Tensor<1, Real[e]>, Tensor<1, Real[d]>) -> Result<Tensor<1, Real[d]>, MathError>`

Returns Ok(solution) or Err(MathError).

**Examples:**
```text
solve_system(fn v => [v[0] + v[1] - 3, v[0] - v[1] - 1], [1, 1])
```

*Source: Aevia standard library guide.*
