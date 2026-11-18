# `Tensor`

Source: `stdlib/Tensor.aev`

The `Tensor` module of §8.20.

## `let fill`

```aevia
let fill = fill
```

**Signature:** `fill : (Scalar<ρ,d>, Int, ..., Int) -> Tensor<n, Scalar<ρ,d>>`
## `let ones`

```aevia
let ones = ones
```

**Signature:** `ones : (Int, ..., Int) -> Tensor<n, Float>`
## `let zeros`

```aevia
let zeros = zeros
```

`m x n` zero matrix.

**Signature:** `zeros : (Int, ..., Int) -> Tensor<n, Float>`

**Kernel signature** (derived):
- `zeros : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `zeros : (Int, Int) -> Tensor<2, Int>`

**Dimensions:** kernel rule `List`

The zero vector of length `n`.

**Examples:**
- `zeros(4, 4)` returns a `[[Int]]`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `linalg.zeros`; rustdoc of `crate::kernels::qlinalg::zeros` (by name).*
