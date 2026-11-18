# `Prelude`

Source: `stdlib/Prelude.aev`

The `Prelude` module of §8.20.

## `let abs`

```aevia
let abs = abs
```

Absolute value (or complex modulus) `|x|`.

**Signature:** `abs : Scalar<ρ,d> -> Scalar<ρ,d>`

**Kernel signature** (derived):
- `abs : (Real) -> Real`
- `abs : (Dyn<term>) -> Dyn<term>`
- `abs : (Int) -> Int`

**Dimensions:** the result has the dimension of argument 1

**Examples:**
- `abs(0)` returns a `Int`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `elementary.abs`.*
## `let all`

```aevia
let all = all
```

**Signature:** `all : ((Tensor<n-1, a> -> Bool), Tensor<n, a>) -> Bool`

**Signature:** `all : ((a -> Bool), Tensor<1, a>) -> Bool`
## `let and_then`

```aevia
let and_then = and_then
```

**Signature:** `and_then : ((a -> Option<b>), Option<a>) -> Option<b>`

**Signature:** `and_then : ((a -> Result<b, e>), Result<a, e>) -> Result<b, e>`
## `let any`

```aevia
let any = any
```

**Signature:** `any : ((Tensor<n-1, a> -> Bool), Tensor<n, a>) -> Bool`

**Signature:** `any : ((a -> Bool), Tensor<1, a>) -> Bool`
## `let apply`

```aevia
let apply = apply
```

**Signature:** `apply : ((a -> b), a) -> b`
## `let argmax`

```aevia
let argmax = argmax
```

**Signature:** `argmax : Tensor<1, Scalar<ρ,d>> -> Int`
## `let argmin`

```aevia
let argmin = argmin
```

**Signature:** `argmin : Tensor<1, Scalar<ρ,d>> -> Int`
## `let argsort`

```aevia
let argsort = argsort
```

**Signature:** `argsort : TotalOrd a => Tensor<n, a> -> Tensor<1, Int>`
## `let assuming`

```aevia
let assuming = assuming
```

**Signature:** `assuming : (Prop, () -> a) -> a`
## `let by_cases`

```aevia
let by_cases = by_cases
```

**Signature:** `by_cases : (Prop, () -> Real[d], () -> Real[d]) -> Real[d]`
## `let ceil`

```aevia
let ceil = ceil
```

Smallest integer greater than or equal to `x`.

**Signature:** `ceil : Scalar<ρ,d> -> Scalar<ρ,d>`

**Kernel signature** (derived):
- `ceil : (Real) -> Real`
- `ceil : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result has the dimension of argument 1

**Examples:**
- `ceil(7/2)` returns a `Int`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `special.ceil`.*
## `let clamp`

```aevia
let clamp = clamp
```

**Signature:** `clamp : (Scalar<ρ,d>, Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,d>`
## `let compare`

```aevia
let compare = compare
```

**Signature:** `compare : Ord a => (a, a) -> Option<Ordering>`
## `let compose`

```aevia
let compose = compose
```

**Signature:** `compose : ((b -> c), (a -> b)) -> (a -> c)`
## `let concat`

```aevia
let concat = concat
```

**Signature:** `concat : (String, String) -> String`

**Signature:** `concat : (Tensor<n, a>, Tensor<n, a>) -> Tensor<n, a>`

**Signature:** `concat : (Tensor<1, a>, Tensor<1, a>) -> Tensor<1, a>`
## `let const`

```aevia
let const = const
```

**Signature:** `const : (a, b) -> a`
## `let contains`

```aevia
let contains = contains
```

**Signature:** `contains : (String, String) -> Bool`
## `let cumprod`

```aevia
let cumprod = cumprod
```

**Signature:** `cumprod : Tensor<1, Scalar<ρ,1>> -> Tensor<1, Scalar<ρ,1>>`
## `let cumsum`

```aevia
let cumsum = cumsum
```

**Signature:** `cumsum : Tensor<1, Scalar<ρ,d>> -> Tensor<1, Scalar<ρ,d>>`
## `let curry`

```aevia
let curry = curry
```

**Signature:** `curry : ((a, b) -> c) -> (a -> (b -> c))`
## `let decide`

```aevia
let decide = decide
```

**Signature:** `decide : Prop -> Option<Bool>`
## `let div`

```aevia
let div = div
```

**Signature:** `div : (Int, Int) -> Int`
## `let drop`

```aevia
let drop = drop
```

**Signature:** `drop : (Int, Tensor<1, a>) -> Tensor<1, a>`

**Signature:** `drop : (Int, Tensor<n, a>) -> Tensor<n, a>`
## `let ends_with`

```aevia
let ends_with = ends_with
```

**Signature:** `ends_with : (String, String) -> Bool`
## `let enumerate`

```aevia
let enumerate = enumerate
```

**Signature:** `enumerate : Tensor<1, a> -> Tensor<1, (Int, a)>`

**Signature:** `enumerate : Tensor<n, a> -> Tensor<1, (Int, Tensor<n-1, a>)>`
## `let eprintln`

```aevia
let eprintln = eprintln
```

**Signature:** `eprintln : Show a => a -> IO Unit`
## `let factorial`

```aevia
let factorial = factorial
```

Factorial `n! = 1 * 2 * ...

**Signature:** `factorial : Int -> Int`

**Kernel signature** (derived):
- `factorial : (Real) -> Real`
- `factorial : (Dyn<term>) -> Dyn<term>`
- `factorial : (Int) -> Int`

**Dimensions:** the result is dimensionless

* n` for non-negative integer `n`.

Computes the factorial of `n` (`n!`) as an `f64`.

**Arguments**
* `n` - The non-negative integer for which to compute the factorial.

**Returns**
The factorial of `n` as an `f64`. Returns `f64::INFINITY` if `n` is too large to fit in `f64`.

**Examples:**
- `factorial(0)` returns a `Int`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `combinatorics.factorial`; rustdoc of `crate::kernels::combinatorics::factorial` (by name).*
## `let filter`

```aevia
let filter = filter
```

**Signature:** `filter : ((a -> Bool), Tensor<1, a>) -> Tensor<1, a>`

**Signature:** `filter : ((Tensor<n-1, a> -> Bool), Tensor<n, a>) -> Tensor<n, a>`

**Signature:** `filter : ((ρ -> Bool), Table<ρ>) -> Table<ρ>`
## `let find`

```aevia
let find = find
```

**Signature:** `find : ((Tensor<n-1, a> -> Bool), Tensor<n, a>) -> Option<Tensor<n-1, a>>`

**Signature:** `find : ((a -> Bool), Tensor<1, a>) -> Option<a>`
## `let first`

```aevia
let first = first
```

**Signature:** `first : Tensor<n, a> -> Option<Tensor<n-1, a>>`

**Signature:** `first : Tensor<1, a> -> Option<a>`
## `let flatten`

```aevia
let flatten = flatten
```

**Signature:** `flatten : Tensor<n, Tensor<m, a>> -> Tensor<n+m, a>`

**Signature:** `flatten : Tensor<n, a> -> Tensor<1, a>`
## `let flip`

```aevia
let flip = flip
```

**Signature:** `flip : ((a, b) -> c) -> ((b, a) -> c)`
## `let floor`

```aevia
let floor = floor
```

Greatest integer less than or equal to `x`.

**Signature:** `floor : Scalar<ρ,d> -> Scalar<ρ,d>`

**Kernel signature** (derived):
- `floor : (Real) -> Real`
- `floor : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result has the dimension of argument 1

**Examples:**
- `floor(7/2)` returns a `Int`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `special.floor`.*
## `let fold`

```aevia
let fold = fold
```

**Signature:** `fold : ((b, a) -> b, b, Tensor<1, a>) -> b`

**Signature:** `fold : ((b, Tensor<n-1, a>) -> b, b, Tensor<n, a>) -> b`
## `let fold_left`

```aevia
let fold_left = fold_left
```

**Signature:** `fold_left : 同上`
## `let format`

```aevia
let format = format
```

**Signature:** `format : (String, Tensor<1, String>) -> String`
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
## `let id`

```aevia
let id = id
```

**Signature:** `id : a -> a`
## `let is_err`

```aevia
let is_err = is_err
```

**Signature:** `is_err : Result<a, e> -> Bool`
## `let is_finite`

```aevia
let is_finite = is_finite
```

**Signature:** `is_finite : Scalar<ρ,d> -> Bool`
## `let is_infinite`

```aevia
let is_infinite = is_infinite
```

**Signature:** `is_infinite : Scalar<ρ,d> -> Bool`
## `let is_nan`

```aevia
let is_nan = is_nan
```

**Signature:** `is_nan : Scalar<ρ,d> -> Bool`
## `let is_none`

```aevia
let is_none = is_none
```

**Signature:** `is_none : Option<a> -> Bool`
## `let is_ok`

```aevia
let is_ok = is_ok
```

**Signature:** `is_ok : Result<a, e> -> Bool`
## `let is_some`

```aevia
let is_some = is_some
```

**Signature:** `is_some : Option<a> -> Bool`
## `let join`

```aevia
let join = join
```

**Signature:** `join : (String, Tensor<1, String>) -> String`

**Signature:** `join : (Table<ρ>, Table<σ>, String) -> Table<ρ + σ>`
## `let last`

```aevia
let last = last
```

**Signature:** `last : Tensor<n, a> -> Option<Tensor<n-1, a>>`

**Signature:** `last : Tensor<1, a> -> Option<a>`
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
## `let length`

```aevia
let length = length
```

**Signature:** `length : String -> Int`

**Signature:** `length : Tensor<n, a> -> Int`

**Signature:** `length : Set<a> -> Int`
## `let lower`

```aevia
let lower = lower
```

**Signature:** `lower : String -> String`
## `let map`

```aevia
let map = map
```

**Signature:** `map : ((a -> b), Tensor<n, a>) -> Tensor<n, b>`

**Signature:** `map : ((a -> b), Option<a>) -> Option<b>`

**Signature:** `map : ((a -> b), Result<a, e>) -> Result<b, e>`
## `let map2`

```aevia
let map2 = map2
```

**Signature:** `map2 : ((a, b) -> c, Tensor<n, a>, Tensor<n, b>) -> Tensor<n, c>`
## `let map_err`

```aevia
let map_err = map_err
```

**Signature:** `map_err : ((e -> f), Result<a, e>) -> Result<a, f>`
## `let max`

```aevia
let max = max
```

**Signature:** `max : (Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,d>`
## `let mean`

```aevia
let mean = mean
```

Arithmetic mean of list `xs`.

**Signature:** `mean : Tensor<n, Scalar<ρ,d>> -> Scalar<div_rep(ρ),d>`

**Kernel signature** (derived):
- `mean : (Dyn<term>) -> Dyn<term>`
- `mean : (Tensor<1, Int>) -> Real`

**Dimensions:** the result has the dimension of argument 1

Computes the mean of a slice of data.

**Arguments**
* `data` - A unmutable slice of `f64` data points.

**Returns**
The mean of the data as an `f64`.

**Examples:**
- `mean(list(1, 2, 4))` returns a `Rat`
- `mean(list(5))` returns a `Int`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `stats.mean`; rustdoc of `crate::kernels::stats::mean` (by name).*
## `let min`

```aevia
let min = min
```

**Signature:** `min : (Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,d>`
## `let mod`

```aevia
let mod = mod
```

Least non-negative residue of `x` modulo `m > 0`.

**Signature:** `mod : (Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,d>`

**Kernel signature** (derived):
- `mod : (Real, Real) -> Real`
- `mod : (Dyn<term>, Dyn<term>) -> Dyn<term>`
- `mod : (Int, Int) -> Int`

**Dimensions:** the result is dimensionless

**Examples:**
- `mod(17, 5)` returns a `Int`
- `mod(sqrtmod(2, 1000000007)^2, 1000000007)` returns a `Int`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `number_theory.mod`.*
## `let not`

```aevia
let not = not
```

Logical negation of `p`.

**Signature:** `not : Bool -> Bool`

**Kernel signature** (overlay):
- `not : (Bool) -> Bool`

**Dimensions:** kernel rule `NonMetric`

*Source: rssn operator `logic.not`.*
## `let ok`

```aevia
let ok = ok
```

**Signature:** `ok : Result<a, e> -> Option<a>`
## `let ok_or`

```aevia
let ok_or = ok_or
```

**Signature:** `ok_or : (Option<a>, e) -> Result<a, e>`
## `let par_map`

```aevia
let par_map = par_map
```

**Signature:** `par_map : Parallel a => ((a -> b), Tensor<1, a>) -> Tensor<1, b>`
## `let parse_float`

```aevia
let parse_float = parse_float
```

**Signature:** `parse_float : String -> Result<Float, ParseError>`
## `let parse_int`

```aevia
let parse_int = parse_int
```

**Signature:** `parse_int : String -> Result<Int, ParseError>`
## `let partial_compare`

```aevia
let partial_compare = partial_compare
```

**Signature:** `partial_compare : (Float, Float) -> Option<Ordering>`
## `let print`

```aevia
let print = print
```

**Signature:** `print : Show a => a -> IO Unit`
## `let println`

```aevia
let println = println
```

**Signature:** `println : Show a => a -> IO Unit`
## `let prod`

```aevia
let prod = prod
```

**Signature:** `prod : Tensor<n, Scalar<ρ,1>> -> Scalar<ρ,1>`
## `let quot`

```aevia
let quot = quot
```

**Signature:** `quot : (Int, Int) -> Int`
## `let range`

```aevia
let range = range
```

**Signature:** `range : (Int, Int) -> Tensor<1, Int>`

**Signature:** `range : (Int, Int, Int) -> Tensor<1, Int>`
## `let rank`

```aevia
let rank = rank
```

**Signature:** `rank : Tensor<n, a> -> Int`

**Signature:** `rank : Tensor<2, a> -> Int`
## `let rem`

```aevia
let rem = rem
```

**Signature:** `rem : (Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,d>`
## `let replace`

```aevia
let replace = replace
```

**Signature:** `replace : (String, String, String) -> String`
## `let repr`

```aevia
let repr = repr
```

**Signature:** `repr : Show a => a -> String`
## `let reverse`

```aevia
let reverse = reverse
```

**Signature:** `reverse : Tensor<1, a> -> Tensor<1, a>`

**Signature:** `reverse : Tensor<n, a> -> Tensor<n, a>`
## `let reverse_axis`

```aevia
let reverse_axis = reverse_axis
```

**Signature:** `reverse_axis : (Tensor<n, a>, Int) -> Tensor<n, a>`
## `let rng`

```aevia
let rng = rng
```

**Signature:** `rng : Int -> (() -> Float[1])`
## `let round`

```aevia
let round = round
```

Nearest integer to `x`, rounding half away from zero.

**Signature:** `round : Scalar<ρ,d> -> Scalar<ρ,d>`

**Signature:** `round : (Scalar<ρ,d>, Int) -> Scalar<ρ,d>`

**Kernel signature** (derived):
- `round : (Real) -> Real`
- `round : (Dyn<term>) -> Dyn<term>`

**Dimensions:** the result has the dimension of argument 1

**Examples:**
- `round(7/2)` returns a `Int`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `special.round`.*
## `let shape`

```aevia
let shape = shape
```

**Signature:** `shape : Tensor<n, a> -> Tensor<1, Int>`
## `let show`

```aevia
let show = show
```

**Signature:** `show : Show a => a -> String`
## `let sign`

```aevia
let sign = sign
```

Signum function (`-1` for `x < 0`, `0` at `0`, `1` for `x > 0`).

**Signature:** `sign : Scalar<ρ,d> -> Int`

**Kernel signature** (derived):
- `sign : (Real) -> Real`
- `sign : (Dyn<term>) -> Dyn<term>`
- `sign : (Int) -> Int`

**Dimensions:** kernel rule `Custom("")`

**Examples:**
- `sign(0)` returns a `Int`
- `sign(-7/3)` returns a `Int`
(recorded kernel calls; the result is shown as its sort)

*Source: rssn operator `special.sign`.*
## `let sort`

```aevia
let sort = sort
```

**Signature:** `sort : TotalOrd a => Tensor<1, a> -> Tensor<1, a>`

**Signature:** `sort : TotalOrd a => Tensor<n, a> -> Tensor<n, a>`
## `let sort_axis`

```aevia
let sort_axis = sort_axis
```

**Signature:** `sort_axis : TotalOrd a => (Tensor<n, a>, Int) -> Tensor<n, a>`
## `let sort_by`

```aevia
let sort_by = sort_by
```

**Signature:** `sort_by : ((a, a) -> Ordering, Tensor<1, a>) -> Tensor<1, a>`

**Signature:** `sort_by : ((Tensor<n-1, a>, Tensor<n-1, a>) -> Ordering, Tensor<n, a>) -> Tensor<n, a>`
## `let sort_by_key`

```aevia
let sort_by_key = sort_by_key
```

**Signature:** `sort_by_key : TotalOrd k => ((a -> k), Tensor<1, a>) -> Tensor<1, a>`

**Signature:** `sort_by_key : TotalOrd k => ((Tensor<n-1, a> -> k), Tensor<n, a>) -> Tensor<n, a>`
## `let sort_float`

```aevia
let sort_float = sort_float
```

**Signature:** `sort_float : (Tensor<1, Float>, Bool) -> Tensor<1, Float>`

**Signature:** `sort_float : (Tensor<n, Float>, Bool) -> Tensor<n, Float>`
## `let split`

```aevia
let split = split
```

**Signature:** `split : (String, String) -> Tensor<1, String>`
## `let starts_with`

```aevia
let starts_with = starts_with
```

**Signature:** `starts_with : (String, String) -> Bool`
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
## `let sym_eq`

```aevia
let sym_eq = sym_eq
```

**Signature:** `sym_eq : (Real[d], Real[d]) -> Prop`
## `let sym_lt`

```aevia
let sym_lt = sym_lt
```

**Signature:** `sym_lt : (Real[d], Real[d]) -> Prop`
## `let sym_ne`

```aevia
let sym_ne = sym_ne
```

**Signature:** `sym_ne : (Real[d], Real[d]) -> Prop`
## `let take`

```aevia
let take = take
```

**Signature:** `take : (Int, Tensor<1, a>) -> Tensor<1, a>`

**Signature:** `take : (Int, Tensor<n, a>) -> Tensor<n, a>`
## `let to_float`

```aevia
let to_float = to_float
```

**Signature:** `to_float : Scalar<ρ,d> -> Result<Float[d], EvalError>`
## `let to_int`

```aevia
let to_int = to_int
```

**Signature:** `to_int : Scalar<ρ,1> -> Int`
## `let to_latex`

```aevia
let to_latex = to_latex
```

**Signature:** `to_latex : Show a => a -> String`
## `let to_pretty`

```aevia
let to_pretty = to_pretty
```

**Signature:** `to_pretty : Show a => a -> String`
## `let to_real`

```aevia
let to_real = to_real
```

**Signature:** `to_real : Scalar<ρ,d> -> Real[d]`
## `let to_typst`

```aevia
let to_typst = to_typst
```

**Signature:** `to_typst : Show a => a -> String`
## `let total_compare`

```aevia
let total_compare = total_compare
```

**Signature:** `total_compare : TotalOrd a => (a, a) -> Ordering`
## `let trim`

```aevia
let trim = trim
```

**Signature:** `trim : String -> String`
## `let trunc`

```aevia
let trunc = trunc
```

**Signature:** `trunc : Scalar<ρ,d> -> Scalar<ρ,d>`
## `let uncurry`

```aevia
let uncurry = uncurry
```

**Signature:** `uncurry : ((a -> (b -> c))) -> ((a, b) -> c)`

**Signature:** `uncurry : (a -> (b -> c)) -> ((a, b) -> c)`
## `let unwrap_or`

```aevia
let unwrap_or = unwrap_or
```

**Signature:** `unwrap_or : (Option<a>, a) -> a`

**Signature:** `unwrap_or : (Result<a, e>, a) -> a`
## `let upper`

```aevia
let upper = upper
```

**Signature:** `upper : String -> String`
## `let xor`

```aevia
let xor = xor
```

Variadic exclusive-or (parity), true when an odd number of operands are true.

**Signature:** `xor : (Bool, Bool) -> Bool`

**Kernel signature** (overlay):
- `xor : (Bool, Bool) -> Bool with 0... x Bool`

**Dimensions:** kernel rule `NonMetric`

*Source: rssn operator `logic.xor`.*
## `let zero`

```aevia
let zero = zero
```

**Signature:** `zero : Scalar<ρ,d>`
## `let zip`

```aevia
let zip = zip
```

**Signature:** `zip : (Tensor<1, a>, Tensor<1, b>) -> Tensor<1, (a, b)>`

**Signature:** `zip : (Tensor<n, a>, Tensor<m, b>) -> Tensor<1, (Tensor<n-1, a>, Tensor<m-1, b>)>`
## `let zip_with`

```aevia
let zip_with = zip_with
```

**Signature:** `zip_with : ((a, b) -> c, Tensor<n, a>, Tensor<n, b>) -> Tensor<n, c>`
