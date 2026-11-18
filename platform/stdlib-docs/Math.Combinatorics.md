# `Math.Combinatorics`

Source: `stdlib/Math/Combinatorics.aev`

The `Math/Combinatorics` module of §8.20.

## `let binomial`

```aevia
let binomial = binomial
```

Binomial coefficient `C(n, k) = n! / (k! (n - k)!)`.

**Signature:** `binomial : (Int, Int) -> Int`

**Kernel signature** (derived):
- `binomial : (Real, Real) -> Real`
- `binomial : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `binomial : (Int, Int) -> Int`

**Dimensions:** the result is dimensionless

**Examples:**
- `binomial(0, 0)` returns a `Int`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `combinatorics.binomial`.*
## `let permutations`

```aevia
let permutations = permutations
```

Number of `k`-permutations of `n` items, `n! / (n - k)!`.

**Signature:** `permutations : Int -> Int`

**Kernel signature** (derived):
- `permutations : (Real, Real) -> Real`
- `permutations : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `permutations : (Int, Int) -> Int`

**Dimensions:** the result is dimensionless

Computes the number of permutations `P(n, k) = n! / (n-k)!`.

**Arguments**
* `n` - The total number of items.
* `k` - The number of items to choose.

**Returns**
The number of permutations as an `f64`. Returns `0.0` if `k > n`.

**Examples:**
- `permutations(10, 3)` returns a `Int`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `combinatorics.permutations`; rustdoc of `crate::kernels::combinatorics::permutations` (by name).*
