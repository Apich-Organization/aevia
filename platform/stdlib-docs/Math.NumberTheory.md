# `Math.NumberTheory`

Source: `stdlib/Math/NumberTheory.aev`

The `Math/NumberTheory` module of §8.20.

## `let factor`

```aevia
let factor = factor
```

Factors a polynomial or rational expression into irreducible factors over the rationals.

**Signature:** `factor : Real[d] -> Real[d]`

**Signature:** `factor : Int -> Tensor<1, (Int, Int)>`

**Kernel signature** (derived):
- `factor : (Real) -> Real`
- `factor : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result has the dimension of argument 1

*Source: rssn operator `poly.factor`.*
## `let gcd`

```aevia
let gcd = gcd
```

Greatest common divisor of integers `a` and `b`.

**Signature:** `gcd : (Int, Int) -> Int`

**Kernel signature** (derived):
- `gcd : (Real, Real) -> Real`
- `gcd : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `gcd : (Int, Int) -> Int`

**Dimensions:** the result is dimensionless

Computes the greatest common divisor (GCD) of two numbers using the Euclidean algorithm.

**Arguments**
* `a` - The first number.
* `b` - The second number.

**Returns**
The greatest common divisor of `a` and `b`.

**Examples:**
- `gcd(12, 18)` returns a `Int`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `number_theory.gcd`; rustdoc of `crate::kernels::number_theory::gcd` (by name).*
## `let is_prime`

```aevia
let is_prime = is_prime
```

**Signature:** `is_prime : Int -> Bool`
## `let lcm`

```aevia
let lcm = lcm
```

Least common multiple of integers `a` and `b`.

**Signature:** `lcm : (Int, Int) -> Int`

**Kernel signature** (derived):
- `lcm : (Real, Real) -> Real`
- `lcm : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `lcm : (Int, Int) -> Int`

**Dimensions:** the result is dimensionless

Computes the least common multiple (LCM) of two numbers.

Returns `0` when either argument is `0`, and `u64::MAX` when the true
value does not fit (use [`checked_lcm`] to tell the two apart).

**Examples:**
- `lcm(4, 6)` returns a `Int`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `number_theory.lcm`; rustdoc of `crate::kernels::number_theory::lcm` (by name).*
## `let primes`

```aevia
let primes = primes
```

List of all prime numbers `<= n`.

**Signature:** `primes : Int -> Tensor<1, Int>`

**Kernel signature** (derived):
- `primes : (Dyn<term>) -> Dyn<term>`
- `primes : (Int) -> Tensor<1, Int>`

**Dimensions:** kernel rule `List`

Prime numbers strictly below `limit`; negative limits yield an empty list.

**Examples:**
- `primes(30)` returns a `[Int]`
- `primes(1)` returns a `[Never]`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `number_theory.primes`; rustdoc of `crate::kernels::number_exact::primes` (by name).*
