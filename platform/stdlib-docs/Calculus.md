# `Calculus`

Source: `stdlib/Calculus.aev`

The `Calculus` module of §8.20.

## `let antiderivative`

```aevia
let antiderivative = antiderivative
```

**Signature:** `antiderivative : (Real[d] -> Real[e]) -> (Real[d] -> Real[e*d])`
## `let curl`

```aevia
let curl = curl
```

3D curl `nabla x F` of vector field `F`.

**Signature:** `curl : ((Tensor<1, Real[L]> -> Tensor<1, Real[d]>), Tensor<1, Real[L]>) -> Tensor<1, Real[d/L]>`

**Kernel signature** (derived):
- `curl : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `curl : (Tensor<1, Dyn<term>>, Tensor<1, Symbol>) -> Tensor<1, Dyn<term>>`

**Dimensions:** argument 1 divided by argument 2

Curl of a three-dimensional vector field. Returns `None` for wrong dimensions.

**Examples:**
- `curl(list(x*y*z, y*sin(x), x*exp(z)), list(x, y, z))` returns a `[Expr]`
- `curl(list(-y, x, 0), list(x, y, z))` returns a `[Int]`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `linalg.curl`; rustdoc of `crate::kernels::calculus::curl` (by name).*
## `let defint`

```aevia
let defint = defint
```

Definite integral of `f` with respect to bound variable `x` from `a` to `b`.

**Signature:** `defint : (Real[d] -> Real[e], Real[d], Real[d]) -> Result<Real[e*d], MathError>`

**Kernel signature** (derived):
- `defint : (Dyn<term>, Symbol, Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** argument 1 multiplied by argument 2

*Source: rssn operator `calculus.defint`.*
## `let diff`

```aevia
let diff = diff
```

Symbolic derivative of `f` with respect to `x`.

**Signature:** `diff : (Real[d] -> Real[e]) -> (Real[d] -> Real[e/d])`

**Kernel signature** (overlay):
- `diff : (Dyn<term>, Symbol) -> Dyn<term>`
- `diff : (Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** argument 1 divided by argument 2

The derivative of `f` with respect to the symbol `x`, as an unevaluated
request.

*Source: rssn operator `calculus.diff`; rustdoc of `crate::api::diff` (by name).*
## `let diff_at`

```aevia
let diff_at = diff_at
```

**Signature:** `diff_at : ((Real[d] -> Real[e]), Real[d]) -> Real[e/d]`
## `let divergence`

```aevia
let divergence = divergence
```

The divergence of a vector field at `x`.

**Signature:** `divergence : ((Tensor<1, Real[d]> -> Tensor<1, Real[e]>), Tensor<1, Real[d]>) -> Real[e/d]`

*Source: Rust documentation of the `aevia-std` function.*
## `let grad`

```aevia
let grad = grad
```

Gradient vector `[df/dx_1, ..., df/dx_n]` of scalar expression `f`.

**Signature:** `grad : ((Tensor<1, Real[d]> -> Real[e]), Tensor<1, Real[d]>) -> Tensor<1, Real[e/d]>`

**Kernel signature** (derived):
- `grad : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `grad : (Int, Tensor<1, Symbol>) -> Tensor<1, Dyn<term>>`
- `grad : (Dyn<term>, Tensor<1, Symbol>) -> Tensor<1, Dyn<term>>`

**Dimensions:** argument 1 divided by argument 2

**Examples:**
- `grad(0, list(x, y, z))` returns a `[Int]`
- `grad(x^2*y + z, list(x, y, z))` returns a `[Expr]`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `linalg.grad`.*
## `let grad_f`

```aevia
let grad_f = grad_f
```

**Signature:** `grad_f : ((Tensor<1, Float[d]> -> Float[e]), Tensor<1, Float[d]>) -> Tensor<1, Float[e/d]>`
## `let hessian`

```aevia
let hessian = hessian
```

Hessian matrix `H_{ij} = d^2f/(dx_i dx_j)` of scalar expression `f`.

**Signature:** `hessian : ((Tensor<1, Real[d]> -> Real[e]), Tensor<1, Real[d]>) -> Tensor<2, Real[e/d^2]>`

**Kernel signature** (derived):
- `hessian : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `hessian : (Dyn<term>, Tensor<1, Symbol>) -> Tensor<2, Dyn<term>>`

**Dimensions:** kernel rule `DiffN { f: 0, x: 1, n: 2 }`

**Examples:**
- `hessian(x^2*y, list(x, y))` returns a `[[Expr]]`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `linalg.hessian`.*
## `let jacobian`

```aevia
let jacobian = jacobian
```

Jacobian matrix `J_{ij} = dF_i/dx_j` of vector field `F`.

**Signature:** `jacobian : ((Tensor<1, Real[d]> -> Tensor<1, Real[e]>), Tensor<1, Real[d]>) -> Tensor<2, Real[e/d]>`

**Kernel signature** (derived):
- `jacobian : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `jacobian : (Tensor<1, Dyn<term>>, Tensor<1, Symbol>) -> Tensor<2, Dyn<term>>`

**Dimensions:** argument 1 divided by argument 2

**Examples:**
- `jacobian(list(x*y, x + y), list(x, y))` returns a `[[Expr]]`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `linalg.jacobian`.*
## `let laplacian`

```aevia
let laplacian = laplacian
```

Laplacian `sum_i d^2f/dx_i^2` of scalar expression `f`.

**Signature:** `laplacian : ((Tensor<1, Real[d]> -> Real[e]), Tensor<1, Real[d]>) -> Real[e/d^2]`

**Kernel signature** (derived):
- `laplacian : (Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** kernel rule `DiffN { f: 0, x: 1, n: 2 }`

Laplacian of a scalar field, using central differences.

*Source: rssn operator `linalg.laplacian`; rustdoc of `crate::kernels::calculus::laplacian` (by name).*
## `let limit`

```aevia
let limit = limit
```

Limit of `f` as bound variable `x` approaches `a` (optional direction `plus` or `minus`).

**Signature:** `limit : (Real[d] -> Real[e], Real[d]) -> Result<Real[e], MathError>`

**Kernel signature** (derived):
- `limit : () -> Dyn<term> with 0... x Dyn<term>`

**Dimensions:** the result has the dimension of argument 1

*Source: rssn operator `calculus.limit`.*
## `let numeric_deriv`

```aevia
let numeric_deriv = numeric_deriv
```

**Signature:** `numeric_deriv : (Float[d] -> Float[e]) -> (Float[d] -> Float[e/d])`
## `let product`

```aevia
let product = product
```

Symbolic/numeric product over bound index `k` from `lo` to `hi`.

**Signature:** `product : (Real[1] -> Real[1], Real[1], Real[1]) -> Real[1]`

**Kernel signature** (derived):
- `product : (Dyn<term>, Symbol, Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** kernel rule `ProductOver { f: 0, index: 1 }`

The triangulated product `K x L` (ordered product of simplices: the
vertex `(v, w)` is numbered `index(v) * |L| + index(w)`), or `None` when
it would exceed `limit` simplices.

*Source: rssn operator `calculus.product`; rustdoc of `crate::kernels::homology::product` (by name).*
## `let product_dyn`

```aevia
let product_dyn = product_dyn
```

**Signature:** `product_dyn : (Real[1] -> Real[d], Real[1], Real[1]) -> Dyn<Real>`
## `let quadrature`

```aevia
let quadrature = quadrature
```

Adaptive Gauss–Kronrod quadrature of `f` over `[a, b]`.

**Signature:** `quadrature : (Float[d] -> Float[e], Float[d], Float[d]) -> Result<Float[e*d], MathError>`

**Errors**

[`NumericError::NotConverged`] when the recursion limit is reached before
the tolerance is met, and [`NumericError::Domain`] when `b < a`.

*Source: Rust documentation of the `aevia-std` function.*
## `let series`

```aevia
let series = series
```

**Signature:** `series : (Real[d] -> Real[e], Real[d], Int) -> Real[e]`
## `let sum`

```aevia
let sum = sum
```

Symbolic/numeric summation over bound index `k` from `lo` to `hi`, or tensor reduction along `axis`.

**Signature:** `sum : Tensor<n, Scalar<ρ,d>> -> Scalar<ρ,d>`

**Signature:** `sum : (Tensor<n, Scalar<ρ,d>>, Int) -> Tensor<n, Scalar<ρ,d>>`

**Signature:** `sum : (Real[1] -> Real[d], Real[1], Real[1]) -> Real[d]`

**Kernel signature** (overlay):
- `sum : (Tensor<_, Real>) -> Real`
- `sum : (Tensor<_, Real>, Int) -> Tensor<_, Real>`
- `sum : (Dyn<term>, Symbol, Dyn<term>, Dyn<term>) -> Dyn<term>`
- `sum : (Dyn<term>) -> Dyn<term>`
- ... and 1 more overloads

**Dimensions:** kernel rule `SumOver { f: 0, index: 1 }`

*Source: rssn operator `calculus.sum`.*
## `let sum_f`

```aevia
let sum_f = sum_f
```

**Signature:** `sum_f : (Float[1] -> Float[d], Float[1], Float[1]) -> Float[d]`
