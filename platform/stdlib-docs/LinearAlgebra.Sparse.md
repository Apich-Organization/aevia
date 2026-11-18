# `LinearAlgebra.Sparse`

Source: `stdlib/LinearAlgebra/Sparse.aev`

The `LinearAlgebra/Sparse` module of §8.20.

## `type Precond`

```aevia
type Precond
```
## `let bicgstab_with`

```aevia
let bicgstab_with = bicgstab_with
```

**Signature:** `bicgstab_with : (Sparse<F, [n,n], Float[d]>, Tensor<1, Float[e]>, { tol: Float, maxiter: Int, precond: Precond }) -> { x: Tensor<1, Float[e/d]>, iterations: Int, residual: Float, converged: Bool, precond: String }`
## `let block_size`

```aevia
let block_size = block_size
```

**Signature:** `block_size : Sparse<F, shape, τ> -> (Int, Int)`
## `let cg`

```aevia
let cg = cg
```

RSSN `cg` adapter with the established Aevia result layout.

**Signature:** `cg : (Sparse<F, [n,n], Float[d]>, Tensor<1, Float[e]>, Float, Int) -> { x: Tensor<1, Float[e/d]>, iterations: Int, residual: Float, converged: Bool }`

**Errors**
Invalid dimensions, sparse data, controls, or numerical factorization failure.

*Source: Rust documentation of the `aevia-std` function.*
## `let cg_with`

```aevia
let cg_with = cg_with
```

**Signature:** `cg_with : (Sparse<F, [n,n], Float[d]>, Tensor<1, Float[e]>, { tol: Float, maxiter: Int, precond: Precond }) -> { x: Tensor<1, Float[e/d]>, iterations: Int, residual: Float, converged: Bool, precond: String }`
## `let contract`

```aevia
let contract = contract
```

**Signature:** `contract : (Sparse<F, shape, τ>, Sparse<G, shape, τ>, [Int], [Int]) -> Sparse<Coo, shape, τ>`
## `let convert`

```aevia
let convert = convert
```

Converts quantity `qty` to compatible `target_unit`.

**Signature:** `convert : Sparse<F, shape, τ> -> Sparse<G, shape, τ>`

**Kernel signature** (overlay):
- `convert : (Dyn<term>, Unit[_]) -> Dyn<term>`
- `convert : (Dyn<term>, Symbol) -> Dyn<term>`
- `convert : (Dyn<term>, Dyn<term>) -> Dyn<term>`

**Dimensions:** the result has the dimension of argument 1

*Source: rssn operator `units.convert`.*
## `let density`

```aevia
let density = density
```

**Signature:** `density : Sparse<F, shape, τ> -> Float`
## `let format_of`

```aevia
let format_of = format_of
```

**Signature:** `format_of : Sparse<F, shape, τ> -> String`
## `let from_dense`

```aevia
let from_dense = from_dense
```

**Signature:** `from_dense : Tensor<shape, τ> -> Sparse<F, shape, τ>`
## `let gmres_with`

```aevia
let gmres_with = gmres_with
```

**Signature:** `gmres_with : (Sparse<F, [n,n], Float[d]>, Tensor<1, Float[e]>, { tol: Float, maxiter: Int, restart: Int, precond: Precond }) -> { x: Tensor<1, Float[e/d]>, iterations: Int, residual: Float, converged: Bool, precond: String }`
## `let kron`

```aevia
let kron = kron
```

Kronecker product `A otimes B` of matrices `A` and `B`.

**Signature:** `kron : (Sparse<F, [m,n], Scalar<ρ,d>>, Sparse<G, [p,q], Scalar<ρ,e>>) -> Sparse<Csr, [m*p,n*q], Scalar<ρ,d*e>>`

**Kernel signature** (derived):
- `kron : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `kron : (Tensor<2, Int>, Tensor<2, Int>) -> Tensor<2, Int>`

**Dimensions:** the result is the product of the argument dimensions

**Examples:**
- `kron(list(list(1, 2), list(3, 4)), list(list(0, 1), list(1, 0)))` returns a `[[Int]]`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `linalg.kron`.*
## `let map_nonzero`

```aevia
let map_nonzero = map_nonzero
```

**Signature:** `map_nonzero : ((Scalar<ρ,d>) -> Scalar<σ,e>, Sparse<F, shape, Scalar<ρ,d>>) -> Sparse<F, shape, Scalar<σ,e>>`
## `let matmul`

```aevia
let matmul = matmul
```

Matrix product of two or more matrices or vectors.

**Signature:** `matmul : (Tensor<2, Scalar<ρ,d>>, Tensor<2, Scalar<ρ,e>>) -> Tensor<2, Scalar<ρ,d*e>>`

**Signature:** `matmul : (Sparse<F, [m,k], Scalar<ρ,d>>, Sparse<G, [k,n], Scalar<ρ,e>>) -> Sparse<Csr, [m,n], Scalar<ρ,d*e>>`

**Kernel signature** (derived):
- `matmul : () -> Dyn<term> with 0... x Dyn<term>`
- `matmul : () -> Tensor<2, Dyn<term>> with 0... x Tensor<2, Dyn<term>>`
- `matmul : () -> Tensor<2, Int> with 0... x Tensor<2, Int>`
- `matmul : () -> Tensor<2, Real> with 0... x Tensor<2, Real>`

**Dimensions:** the result is the product of the argument dimensions

**Examples:**
- `matmul(dirac_gamma(0), dirac_gamma(2))` returns a `[[Expr]]`
- `matmul(dirac_gamma(2), dirac_gamma(0))` returns a `[[Expr]]`
- `matmul(pauli_x, pauli_y)` returns a `[[Expr]]`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `linalg.matmul`.*
## `let nnz`

```aevia
let nnz = nnz
```

**Signature:** `nnz : Sparse<F, shape, τ> -> Int`
## `let nonzeros`

```aevia
let nonzeros = nonzeros
```

**Signature:** `nonzeros : Sparse<F, shape, Scalar<ρ,d>> -> Tensor<1, (Tensor<1, Int>, Scalar<ρ,d>)>`
## `let sparse_coo`

```aevia
let sparse_coo = sparse_coo
```

**Signature:** `sparse_coo : (Tensor<1, Int>, Tensor<2, Int>, Tensor<1, Scalar<ρ,d>>) -> Sparse<Coo, shape, Scalar<ρ,d>>`
## `let sparse_csr`

```aevia
let sparse_csr = sparse_csr
```

**Signature:** `sparse_csr : (Int, Int, Tensor<1, Int>, Tensor<1, Int>, Tensor<1, Scalar<ρ,d>>) -> Sparse<Csr, [m,n], Scalar<ρ,d>>`
## `let spdiag`

```aevia
let spdiag = spdiag
```

**Signature:** `spdiag : Tensor<1, Scalar<ρ,d>> -> Sparse<F, [n,n], Scalar<ρ,d>>`
## `let speye`

```aevia
let speye = speye
```

**Signature:** `speye : Int -> Sparse<F, [n,n], Float>`
## `let spsolve`

```aevia
let spsolve = spsolve
```

**Signature:** `spsolve : (Sparse<F, [n,n], Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Tensor<1, Scalar<div_rep(ρ),e/d>>`
## `let to_bcsr`

```aevia
let to_bcsr = to_bcsr
```

**Signature:** `to_bcsr : (Sparse<F, [m,n], τ>, Int, Int) -> Sparse<Bcsr, [m,n], τ>`
## `let to_dense`

```aevia
let to_dense = to_dense
```

**Signature:** `to_dense : Sparse<F, shape, τ> -> Tensor<shape, τ>`
## `let transpose`

```aevia
let transpose = transpose
```

Transpose `A^T` of matrix `A`.

**Signature:** `transpose : Tensor<n, a> -> Tensor<n, a>`

**Signature:** `transpose : Sparse<F, [m,n], τ> -> Sparse<F, [n,m], τ>`

**Kernel signature** (overlay):
- `transpose : (Tensor<_, Real>) -> Tensor<_, Real>`
- `transpose : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result has the dimension of argument 1

*Source: rssn operator `linalg.transpose`.*
