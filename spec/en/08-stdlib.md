# 08 — Standard library

The standard library is part of the language: it defines the Prelude, mathematical functions, units, linear algebra, calculus, statistics, data, plotting and simulation. This chapter gives the **normative signatures**. An implementation MAY add functions but MUST NOT change the type or dimension rule of an existing one.

In signatures, `ρ` is a numeric representation (`Int`, `Real`, `Float`, `Complex`), `d` and `e` are dimensions, `n` is a rank, and `a` and `b` are types. `Scalar<ρ,d>` is written `Real[d]` etc. in surface syntax.

## 8.1 Prelude

The Prelude is imported automatically; no `open` is needed.

### 8.1.1 Function composition

```
id       : a -> a
const    : (a, b) -> a
compose  : ((b -> c), (a -> b)) -> (a -> c)
flip     : ((a, b) -> c) -> ((b, a) -> c)
curry    : ((a, b) -> c) -> (a -> (b -> c))
uncurry  : ((a -> (b -> c))) -> ((a, b) -> c)
apply    : ((a -> b), a) -> b
```

`compose(f, g)(x) = f(g(x))`. `|>` and `<|` are syntax, not functions.

### 8.1.2 Booleans and comparison

```
not      : Bool -> Bool
xor      : (Bool, Bool) -> Bool
(&&)     : (Bool, Bool) -> Bool
(||)     : (Bool, Bool) -> Bool
(==)     : Eq a => (a, a) -> Bool
(!=)     : Eq a => (a, a) -> Bool
(<)      : Ord a => (a, a) -> Bool
(<=)     : Ord a => (a, a) -> Bool
(>)      : Ord a => (a, a) -> Bool
(>=)     : Ord a => (a, a) -> Bool
compare  : Ord a => (a, a) -> Option<Ordering>
partial_compare : (Float, Float) -> Option<Ordering>   # NaN -> None
total_compare   : TotalOrd a => (a, a) -> Ordering

sym_eq   : (Real[d], Real[d]) -> Prop
sym_ne   : (Real[d], Real[d]) -> Prop
sym_lt   : (Real[d], Real[d]) -> Prop
decide   : Prop -> Option<Bool>
assuming : (Prop, () -> a) -> a
by_cases : (Prop, () -> Real[d], () -> Real[d]) -> Real[d]
```

`by_cases(p, yes, no)` selects a branch when `p` is decidable; otherwise it builds a symbolic `ite` term (for symbolic work on piecewise functions). `assuming` supports unary predicates only (`x > 0`, `x >= 0`, `x != 0`, `x ∈ ℤ`).

`Real` has no `Eq`/`Ord` instances; its comparisons yield `Prop`. `Int`, `Float` and `String` comparisons yield `Bool`. `TotalOrd` is used by `Set` and `sort`; `Float` needs the `TotalFloat` wrapper.

### 8.1.3 Numbers

```
abs      : Scalar<ρ,d> -> Scalar<ρ,d>        # ρ ≠ Complex
sign     : Scalar<ρ,d> -> Int                 # ρ ≠ Complex
min      : (Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,d>   # ρ ≠ Complex
max      : (Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,d>   # ρ ≠ Complex
clamp    : (Scalar<ρ,d>, Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,d>
floor    : Scalar<ρ,d> -> Scalar<ρ,d>         # ρ ≠ Complex
ceil     : Scalar<ρ,d> -> Scalar<ρ,d>         # ρ ≠ Complex
round    : Scalar<ρ,d> -> Scalar<ρ,d>         # ρ ≠ Complex
round    : (Scalar<ρ,d>, Int) -> Scalar<ρ,d>   # keep n decimals; negative n rounds to tens, hundreds, ...
is_nan   : Scalar<ρ,d> -> Bool
is_finite : Scalar<ρ,d> -> Bool
is_infinite : Scalar<ρ,d> -> Bool
trunc    : Scalar<ρ,d> -> Scalar<ρ,d>         # ρ ≠ Complex
mod      : (Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,d>   # ρ ≠ Complex
rem      : (Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,d>   # ρ ≠ Complex
div      : (Int, Int) -> Int
quot     : (Int, Int) -> Int
gcd      : (Int, Int) -> Int
lcm      : (Int, Int) -> Int
factorial: Int -> Int
to_float : Scalar<ρ,d> -> Result<Float[d], EvalError>   # fails for symbolic / free-symbol values
to_real  : Scalar<ρ,d> -> Real[d]
to_int   : Scalar<ρ,1> -> Int
zero     : Scalar<ρ,d>                 # zero of the representation/dimension, for fold/sum
rng      : Int -> (() -> Float[1])     # pure, deterministic; returns a "next number" function
```

`min` and `max` are binary; tensor reductions are `min_all` and `max_all`. `zero` is the start of `fold((+), zero, xs)`; `sum([])` internally uses the zero of the corresponding representation and dimension.

`rng(seed)` is a **pure** pseudo-random generator: the same seed gives the same sequence in a program, independent of evaluation order and thread count. It returns a nullary function that yields the next `Float[1]` on each call. `random()` (§6.8.2) is the `IO` version and is not reproducible.

### 8.1.4 Strings

```
length   : String -> Int
concat   : (String, String) -> String
join     : (String, Tensor<1, String>) -> String
split    : (String, String) -> Tensor<1, String>
replace  : (String, String, String) -> String
trim     : String -> String
upper    : String -> String
lower    : String -> String
contains : (String, String) -> Bool
starts_with : (String, String) -> Bool
ends_with   : (String, String) -> Bool
parse_int   : String -> Result<Int, ParseError>
parse_float : String -> Result<Float, ParseError>
format      : (String, Tensor<1, String>) -> String
```

String indexing counts grapheme clusters. `length(s)`, `chars(s)`, `s[i]` and `s[i..j]`/`s[i:j:k]` all work on grapheme clusters (`"e\u{301}"` is one position); `s[i]` and slices are `String`, negative indices count from the end, and out-of-range access is `E0802`. `chars(s)` returns `Tensor<1, String>`.

The placeholders of `format(template, args)` are `{0}`, `{1}` (positional) and `{}` (next argument); other brace content and placeholders beyond the argument count are kept as written. In an ordinary string literal `{…}` is interpolation, so templates are usually raw strings (`r"{} m"`) or read from a file; an empty interpolation `"{}"` is `E0104` and the diagnostic suggests both forms.

### 8.1.5 Containers

```
map      : ((a -> b), Tensor<n, a>) -> Tensor<n, b>
map2     : ((a, b) -> c, Tensor<n, a>, Tensor<n, b>) -> Tensor<n, c>
par_map  : Parallel a => ((a -> b), Tensor<1, a>) -> Tensor<1, b>
filter   : ((a -> Bool), Tensor<1, a>) -> Tensor<1, a>
fold     : ((b, a) -> b, b, Tensor<1, a>) -> b
fold_left: same as fold
zip      : (Tensor<1, a>, Tensor<1, b>) -> Tensor<1, (a, b)>
zip_with : ((a, b) -> c, Tensor<n, a>, Tensor<n, b>) -> Tensor<n, c>
enumerate: Tensor<1, a> -> Tensor<1, (Int, a)>
range    : (Int, Int) -> Tensor<1, Int>
range    : (Int, Int, Int) -> Tensor<1, Int>
take     : (Int, Tensor<1, a>) -> Tensor<1, a>
drop     : (Int, Tensor<1, a>) -> Tensor<1, a>
reverse  : Tensor<1, a> -> Tensor<1, a>
reverse  : Tensor<n, a> -> Tensor<n, a>                  # n ≥ 2: reverses the first axis
reverse_axis : (Tensor<n, a>, Int) -> Tensor<n, a>       # n ≥ 1: reverses the given axis
filter   : ((Tensor<n-1, a> -> Bool), Tensor<n, a>) -> Tensor<n, a>   # n ≥ 2: the predicate sees one row
take     : (Int, Tensor<n, a>) -> Tensor<n, a>           # n ≥ 2: takes the first rows
drop     : (Int, Tensor<n, a>) -> Tensor<n, a>
concat   : (Tensor<n, a>, Tensor<n, a>) -> Tensor<n, a>  # n ≥ 2: stacks along the first axis
any      : ((Tensor<n-1, a> -> Bool), Tensor<n, a>) -> Bool
all      : ((Tensor<n-1, a> -> Bool), Tensor<n, a>) -> Bool
find     : ((Tensor<n-1, a> -> Bool), Tensor<n, a>) -> Option<Tensor<n-1, a>>
first    : Tensor<n, a> -> Option<Tensor<n-1, a>>
last     : Tensor<n, a> -> Option<Tensor<n-1, a>>
sort     : TotalOrd a => Tensor<1, a> -> Tensor<1, a>
sort     : TotalOrd a => Tensor<n, a> -> Tensor<n, a>      # n ≥ 2: rows in lexicographic order
sort_by  : ((a, a) -> Ordering, Tensor<1, a>) -> Tensor<1, a>
sort_by  : ((Tensor<n-1, a>, Tensor<n-1, a>) -> Ordering, Tensor<n, a>) -> Tensor<n, a>
sort_by_key : TotalOrd k => ((a -> k), Tensor<1, a>) -> Tensor<1, a>
sort_by_key : TotalOrd k => ((Tensor<n-1, a> -> k), Tensor<n, a>) -> Tensor<n, a>
sort_float : (Tensor<1, Float>, Bool) -> Tensor<1, Float>   # NaN last/first
sort_float : (Tensor<n, Float>, Bool) -> Tensor<n, Float>   # n ≥ 2: rows in lexicographic order
sort_axis : TotalOrd a => (Tensor<n, a>, Int) -> Tensor<n, a>   # sorts every 1-D slice along one axis
argsort  : TotalOrd a => Tensor<n, a> -> Tensor<1, Int>        # permutation of a stable sort (row indices)
fold     : ((b, Tensor<n-1, a>) -> b, b, Tensor<n, a>) -> b    # n ≥ 2: the accumulator sees one row
zip      : (Tensor<n, a>, Tensor<m, b>) -> Tensor<1, (Tensor<n-1, a>, Tensor<m-1, b>)>   # pairs row by row
enumerate : Tensor<n, a> -> Tensor<1, (Int, Tensor<n-1, a>)>    # n ≥ 2: index and row
concat   : (Tensor<1, a>, Tensor<1, a>) -> Tensor<1, a>
flatten  : Tensor<n, Tensor<m, a>> -> Tensor<n+m, a>   # regular nesting only
any      : ((a -> Bool), Tensor<1, a>) -> Bool
all      : ((a -> Bool), Tensor<1, a>) -> Bool
find     : ((a -> Bool), Tensor<1, a>) -> Option<a>
sum      : Tensor<n, Scalar<ρ,d>> -> Scalar<ρ,d>
prod     : Tensor<n, Scalar<ρ,1>> -> Scalar<ρ,1>
mean     : Tensor<n, Scalar<ρ,d>> -> Scalar<div_rep(ρ),d>
length   : Tensor<n, a> -> Int
shape    : Tensor<n, a> -> Tensor<1, Int>
rank     : Tensor<n, a> -> Int
first    : Tensor<1, a> -> Option<a>
last     : Tensor<1, a> -> Option<a>
cumsum   : Tensor<1, Scalar<ρ,d>> -> Tensor<1, Scalar<ρ,d>>
cumprod  : Tensor<1, Scalar<ρ,1>> -> Tensor<1, Scalar<ρ,1>>
argmax   : Tensor<1, Scalar<ρ,d>> -> Int      # index of the first maximum
argmin   : Tensor<1, Scalar<ρ,d>> -> Int
```

**Any rank.** Operations written `Tensor<n, …>` are typed for **every rank** `n`: `sum`, `prod`, `mean`, `var`, `std`, `norm`, `normalize`, `map`, `map2`/`zip_with` (same shape), `flatten` (rank ≥ 2), `transpose`, the keepdims form of `sum(a, axis)`, the whole FFT family of §8.8, and the sequence operations below. Rank is not a first-class type parameter, so these signatures are **templates over the rank**: the type table lists instances for ranks 1 to 4 (`builtins::schemes`, used for hovers and first-class use), and the checker instantiates the template at the rank of the actual argument. An argument whose rank matches no instance is `E0302`, with expected and actual types. `first`/`last` give `None` on an empty tensor; `argmax`/`argmin` on an empty tensor are `E0802`; NaN never wins, and ties take the lowest index.

**Sequence operations act on the first axis.** A tensor of rank `n ≥ 2` is a sequence of rank-`n−1` slices along the first axis ("rows"); `Tensor<0, a>` is `a`, so a "row" of a rank-1 tensor is an element.

- `reverse` reverses the first axis and keeps the shape (to flatten, write `reverse(flatten(m))`). `reverse_axis(t, k)` reverses axis `k` (negative counts from the end, out of range `E0802`). On `f64` tensors both are views and do not copy the buffer (§9.5).
- `filter(p, m)`: `p` takes a row (rank `n−1`); the result keeps rank `n`; rejecting everything gives a tensor of the same rank with first-axis length 0. `any`/`all`/`find` test rows one by one; `find`, `first` and `last` return `Option<Tensor<n-1, a>>`.
- `take(k, m)`/`drop(k, m)` select rows; `concat(a, b)` stacks along the first axis and the other axes MUST agree (otherwise `E0802`).
- `fold(f, init, m)`/`fold_left`: `f(acc, row)` runs from the first row to the last.
- `zip(a, b)` pairs rows, truncating to the shorter, giving `Tensor<1, (row-or-element, row-or-element)>` (the operands may have different ranks); `enumerate(m)` gives a sequence of records with fields `index` and `value`.
- `sort(m)` sorts rows lexicographically and stably, keeping the shape; the element type needs `TotalOrd` (use `sort_float` for `Float`). The comparison of `sort_by(cmp, m)` takes two rows; `sort_by_key(f, m)` sorts stably by the `TotalOrd` key `f(row)` (for rank 1, `f` takes an element); `sort_float(m, nan_last)` is `sort` with NaN first or last; `argsort(m)` returns the stable-sort permutation as `Tensor<1, Int>` (element indices for rank 1, row indices for rank ≥ 2); `sort_axis(t, k)` sorts every 1-D slice along axis `k`, keeping the shape.
- **Element-wise operations are unchanged:** `map`/`map2`/`zip_with` act element by element and keep the shape; `sum`/`prod`/`mean`/`var`/`std`/`norm`/`normalize` reduce over all elements; `cumsum`/`cumprod`/`argmax`/`argmin` are defined for rank 1 only. For element-wise `fold`/`sort`/`zip`, `flatten` first.

`length` is overloaded on `String` and `Tensor` and resolved by the type class `Sized`.

`par_map(f, xs)` maps in parallel; the result order matches the input (the thread count never changes the result). It requires `Parallel a`: `Int`, `Float[d]` and their tensors can run in parallel; symbolic `Real` cannot (§3.8.4, §6.10). The element type is fixed at the call site, so the constraint is checked there rather than as a global instance. `par_tabulate`, `par_reduce` and `rng` are in the same family; `par_reduce` block boundaries depend only on the length (§9.7.2).

### 8.1.5.1 Sets

```
set_of       : TotalOrd a => Tensor<1, a> -> Set<a>   # deduplicated, ordered by total_compare
elements     : Set<a> -> Tensor<1, a>                 # ascending
length       : Set<a> -> Int                          # third overload of length
member       : (a, Set<a>) -> Bool                    # O(log n)
set_insert   : (Set<a>, a) -> Set<a>
set_remove   : (Set<a>, a) -> Set<a>
union        : (Set<a>, Set<a>) -> Set<a>
intersection : (Set<a>, Set<a>) -> Set<a>
difference   : (Set<a>, Set<a>) -> Set<a>
is_subset    : (Set<a>, Set<a>) -> Bool
```

`Set<a>` (§3.2, §6.6) is a persistent ordered set and requires `TotalOrd a` (`Set<Float>` is `E0310`). The representation is canonical — elements ascending by `total_compare` and unique — so `==` on sets is set equality regardless of insertion order; `show` prints `{1, 2, 3}` and `repr` prints `set_of([1, 2, 3])`. Set operations are linear merges and `member` is a binary search. Nothing is updated in place: `set_insert` returns a new set and the old value is unchanged (§0.4, principle 3).

### 8.1.6 Option / Result

```
Some     : a -> Option<a>
None     : Option<a>
Ok       : a -> Result<a, e>
Err      : e -> Result<a, e>
map      : ((a -> b), Option<a>) -> Option<b>
and_then : ((a -> Option<b>), Option<a>) -> Option<b>
unwrap_or: (Option<a>, a) -> a
is_some  : Option<a> -> Bool
is_ok    : Result<a, e> -> Bool
is_err   : Result<a, e> -> Bool
ok       : Result<a, e> -> Option<a>
ok_or    : (Option<a>, e) -> Result<a, e>
map      : ((a -> b), Result<a, e>) -> Result<b, e>
and_then : ((a -> Result<b, e>), Result<a, e>) -> Result<b, e>
unwrap_or: (Result<a, e>, a) -> a
is_none  : Option<a> -> Bool
map_err  : ((e -> f), Result<a, e>) -> Result<a, f>
```

`?` is syntax, not a function.

### 8.1.7 Display

```
show     : Show a => a -> String
repr     : Show a => a -> String
to_latex : Show a => a -> String
to_typst : Show a => a -> String
print    : Show a => a -> IO Unit
println  : Show a => a -> IO Unit
eprintln : Show a => a -> IO Unit
```

### 8.1.8 Program arguments

```
args : () -> Tensor<1, String>
```

`args()` returns the argument vector the host passes to the program, **containing only the arguments after `--` in `aevia run file -- a b`** (`--` itself is not an argument). With no arguments it is the empty tensor (`length(args()) == 0`, printed `[]`). Arguments keep their original strings in order, with no shell parsing (quoting, spaces and globbing are the shell's business); the value does not change during the process, and repeated calls return the same vector. The type is `Tensor<1, String>`, so it can be mapped, indexed and measured.

## 8.2 Math

```
pi       : Real[1]
e        : Real[1]
i        : Complex[1]
oo       : Real[1]
nan      : Float[1]
```

`oo` is `Real[1]`, not dimension-polymorphic; `oo m` is `Real[L]`. `-oo` comes from unary minus. `nan` is `Float[1]`. **The semantics of infinity are in §6.3.4**: which boundary values fold (`ln(0)=-oo`, `ln(oo)=oo`, `exp(oo)=oo`, `exp(-oo)=0`), which `+ - * / ^` expressions containing `oo` stay symbolic (the exact phase invents no NaN), and how the `Float` phase gives `inf`/`nan` per IEEE. This chapter declares only the types and dimensions of `oo` and `nan`.

### 8.2.1 Elementary functions

```
sqrt     : Scalar<ρ,d> -> Scalar<ρ,d^(1/2)>        # ρ ∈ {Real,Float}
cbrt     : Scalar<ρ,d> -> Scalar<ρ,d^(1/3)>        # ρ ∈ {Real,Float}
exp      : Scalar<ρ,1> -> Scalar<ρ,1>
ln       : Scalar<ρ,1> -> Scalar<ρ,1>
log      : Scalar<ρ,1> -> Scalar<ρ,1>              # natural logarithm
log2     : Scalar<ρ,1> -> Scalar<ρ,1>
log10    : Scalar<ρ,1> -> Scalar<ρ,1>
sin cos tan          : Scalar<ρ,1> -> Scalar<ρ,1>
asin acos atan        : Scalar<ρ,1> -> Scalar<ρ,1>
atan2                : (Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,1>
sinh cosh tanh        : Scalar<ρ,1> -> Scalar<ρ,1>
asinh acosh atanh     : Scalar<ρ,1> -> Scalar<ρ,1>
hypot                : (Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,d>
```

### 8.2.2 Special functions

```
gamma        : Scalar<ρ,1> -> Scalar<ρ,1>
lgamma       : Scalar<ρ,1> -> Scalar<ρ,1>
beta         : (Scalar<ρ,1>, Scalar<ρ,1>) -> Scalar<ρ,1>
erf erfc     : Scalar<ρ,1> -> Scalar<ρ,1>
fresnel_s fresnel_c : Scalar<ρ,1> -> Scalar<ρ,1>   # S(x)=∫₀ˣ sin(πt²/2)dt, C likewise (DLMF 7.2.7)
zeta         : Scalar<ρ,1> -> Scalar<ρ,1>
bessel_j0 j1 : Scalar<ρ,1> -> Scalar<ρ,1>
bessel_jn    : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>
bessel_y0 y1 : Scalar<ρ,1> -> Scalar<ρ,1>
bessel_yn    : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>
legendre_p   : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>
hermite_h    : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>
laguerre_l   : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>
chebyshev_t  : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>
chebyshev_u  : (Int, Scalar<ρ,1>) -> Scalar<ρ,1>
```

Arguments of special functions MUST be dimensionless; results are dimensionless.

## 8.3 Function utilities

```
curry    : ((a, b) -> c) -> (a -> (b -> c))
uncurry  : (a -> (b -> c)) -> ((a, b) -> c)
partial  : ((a, b) -> c, a) -> (b -> c)
```

`partial(f, a)` is the function form of `f(a, _)`.

## 8.4 LinearAlgebra

```
dot      : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Scalar<ρ,d*e>
matmul   : (Tensor<2, Scalar<ρ,d>>, Tensor<2, Scalar<ρ,e>>) -> Tensor<2, Scalar<ρ,d*e>>
matvec   : (Tensor<2, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Tensor<1, Scalar<ρ,d*e>>
vecmat   : (Tensor<1, Scalar<ρ,d>>, Tensor<2, Scalar<ρ,e>>) -> Tensor<1, Scalar<ρ,d*e>>
outer    : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Tensor<2, Scalar<ρ,d*e>>
transpose: Tensor<n, a> -> Tensor<n, a>          # reverses the axis order
trace    : Tensor<2, Scalar<ρ,d>> -> Scalar<ρ,d>
det      : Tensor<2, Scalar<ρ,1>> -> Scalar<ρ,1>
inv      : Tensor<2, Scalar<ρ,d>> -> Result<Tensor<2, Scalar<div_rep(ρ),1/d>>, LinAlgError>
linsolve : (Tensor<2, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>)
         -> Result<Tensor<1, Scalar<div_rep(ρ),e/d>>, LinAlgError>
lstsq    : (Tensor<2, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>)
         -> Result<Tensor<1, Scalar<div_rep(ρ),e/d>>, LinAlgError>
eig      : Tensor<2, Scalar<ρ,d>> -> (Tensor<1, Complex[d]>, Tensor<2, Complex<1>>)
eigvals  : Tensor<2, Scalar<ρ,d>> -> Tensor<1, Complex[d]>
svd      : Tensor<2, Scalar<ρ,d>> -> (Tensor<2, Scalar<ρ,1>>, Tensor<1, Scalar<ρ,d>>, Tensor<2, Scalar<ρ,1>>)
qr       : Tensor<2, Scalar<ρ,d>> -> (Tensor<2, Scalar<ρ,1>>, Tensor<2, Scalar<ρ,d>>)
lu       : Tensor<2, Scalar<ρ,d>> -> (Tensor<2, Scalar<ρ,1>>, Tensor<2, Scalar<ρ,1>>, Tensor<2, Scalar<ρ,d>>)
cholesky : Tensor<2, Scalar<ρ,d>> -> Result<Tensor<2, Scalar<ρ,d^(1/2)>>, LinAlgError>
norm     : Tensor<n, Scalar<ρ,d>> -> Scalar<ρ,d>
norm     : (Tensor<n, Scalar<ρ,d>>, Int) -> Scalar<ρ,d>
normalize: Tensor<n, Scalar<ρ,d>> -> Tensor<n, Scalar<ρ,1>>
rank     : Tensor<2, a> -> Int
diag     : Tensor<1, a> -> Tensor<2, a>
diag     : Tensor<2, a> -> Tensor<1, a>
tril     : Tensor<2, a> -> Tensor<2, a>
triu     : Tensor<2, a> -> Tensor<2, a>
eye      : Int -> Tensor<2, Float>
zeros    : (Int, ..., Int) -> Tensor<n, Float>       # n = number of arguments
zeros_of<shape>() : Tensor<shape, Float>              # refined-shape constructors
ones_of<shape>()  : Tensor<shape, Float>
ones     : (Int, ..., Int) -> Tensor<n, Float>
fill     : (Scalar<ρ,d>, Int, ..., Int) -> Tensor<n, Scalar<ρ,d>>
linspace : (Scalar<ρ,d>, Scalar<ρ,d>, Int) -> Tensor<1, Scalar<ρ,d>>
reshape<shape> : Tensor<shape0, τ> -> Result<Tensor<shape, τ>, ShapeError>
reshape_dyn : (Tensor<n, τ>, Tensor<1, Int>) -> Result<Tensor<n, τ>, ShapeError>
refine_shape<shape> : Tensor<n, τ> -> Option<Tensor<shape, τ>>
flatten  : Tensor<n, a> -> Tensor<1, a>
at       : (Tensor<n, a>, Int) -> Tensor<n-1, a>     # requires n >= 1
set      : (Tensor<n, a>, Tensor<1, Int>, a) -> Tensor<n, a>
map_indexed : ((Int, a) -> b, Tensor<1, a>) -> Tensor<1, b>
approx_map : ((Real[d] -> Real[e]), Tensor<1, Real[d]>)
           -> Result<Tensor<1, Float[e]>, EvalError>
sum      : Tensor<n, Scalar<ρ,d>> -> Scalar<ρ,d>
sum      : (Tensor<n, Scalar<ρ,d>>, Int) -> Tensor<n, Scalar<ρ,d>>   # keepdims
mean     : Tensor<n, Scalar<ρ,d>> -> Scalar<div_rep(ρ),d>
var      : Tensor<n, Scalar<ρ,d>> -> Scalar<div_rep(ρ),d^2>
std      : Tensor<n, Scalar<ρ,d>> -> Scalar<div_rep(ρ),d>
```

On tensors of rank ≥ 2, `transpose` reverses the axis order (`[a, b, c]` of rank 3 becomes `[c, b, a]`, and refined shapes reverse likewise); on rank 1 it is `E0302` (a vector has no second axis, and the hint says so). On rank ≥ 2, `norm` is the norm over all elements (`p = 2` is the Frobenius norm), and `var`/`std`/`mean`/`sum`/`prod` reduce over all elements (§8.1.5).

In `zeros<Float>(3, 4)`, `n=2`; `zeros<Int>` fixes the element type. `zeros`/`ones`/`fill` are **built-in variadic**: `n` is the number of `Int` arguments. They can be imported from `Tensor` or `LinearAlgebra` and re-exported through value aliases; the type argument may be given before or at the call (`let z = zeros<Int>; z(2, 3)`). A local definition with the same name shadows the built-in as usual, and constructors passed through a generic identity function keep their identity. `zeros` and `ones` of the same element type can be branches of one conditional expression (the operation is chosen at run time by the actual value); `fill` is a separate signature family and different specialisations cannot be mixed. A higher-order parameter with a fixed signature cannot take a variadic constructor directly; write a fixed-arity lambda such as `fn (n) => zeros(n)`. In `reshape<[n1, ..., nk]>(a)` the target shape is explicit; the element count is checked at compile time when shapes are known, otherwise at run time. `reshape_dyn(a, shape)` takes a run-time shape vector and returns a `Result`.

**Shape branching: `refine_shape`.** `refine_shape<[3, ?]>(a)` checks at run time whether each extent of `a` equals the literal written in the type argument (`?` matches any extent); on a match it returns `Some(b)` (`b : Tensor<[3, ?], τ>`), otherwise `None`. The rank is checked at compile time (a mismatch is `E0302`). So shape-dependent branching is a `match` on the `Option`, and in each branch `b` already carries concrete extents in its **type**, which later matrix products, indexing and `reshape` check statically:

```
match refine_shape<[3, 3]>(m) with
| Some(sq) -> sq @ sq            # sq : Tensor<[3, 3], Float>, the multiplication shapes are statically known
| None -> m
```

An extent MUST be a literal or `?`: type-level variables have no run-time value, so `refine_shape<[n]>(a)` is `E0304` (refining a variable extent can only be solved from equations in a signature, not by a run-time test).

`@` dispatches on rank: `A @ B` (2×2), `A @ x` (2×1), `x @ A` (1×2), `x @ y` (1×1 dot product). `matmul` is 2×2 only; `matvec`/`vecmat`/`dot` are the explicit forms.

`det` accepts only dimensionless matrices; use `det_unit` for dimensioned ones:

```
det_unit : (Tensor<2, Scalar<ρ,d>>, Unit[d]) -> Dyn<ρ>

type LinAlgError = | Singular | NotConverged | NotPositiveDefinite | ShapeMismatch
```

### 8.4.1 Sparse

```
type Sparse<F, shape, τ>

from_dense<F> : Tensor<shape, τ> -> Sparse<F, shape, τ>
to_dense      : Sparse<F, shape, τ> -> Tensor<shape, τ>
convert<G>    : Sparse<F, shape, τ> -> Sparse<G, shape, τ>
to_bcsr       : (Sparse<F, [m,n], τ>, Int, Int) -> Sparse<Bcsr, [m,n], τ>
block_size    : Sparse<F, shape, τ> -> (Int, Int)
nnz           : Sparse<F, shape, τ> -> Int
density       : Sparse<F, shape, τ> -> Float
format_of     : Sparse<F, shape, τ> -> String
sparse_coo    : (Tensor<1, Int>, Tensor<2, Int>, Tensor<1, Scalar<ρ,d>>)
              -> Sparse<Coo, shape, Scalar<ρ,d>>
sparse_csr    : (Int, Int, Tensor<1, Int>, Tensor<1, Int>, Tensor<1, Scalar<ρ,d>>)
              -> Sparse<Csr, [m,n], Scalar<ρ,d>>
speye<F>      : Int -> Sparse<F, [n,n], Float>
spdiag<F>     : Tensor<1, Scalar<ρ,d>> -> Sparse<F, [n,n], Scalar<ρ,d>>
transpose     : Sparse<F, [m,n], τ> -> Sparse<F, [n,m], τ>
matmul        : (Sparse<F, [m,k], Scalar<ρ,d>>, Sparse<G, [k,n], Scalar<ρ,e>>)
              -> Sparse<Csr, [m,n], Scalar<ρ,d*e>>
kron          : (Sparse<F, [m,n], Scalar<ρ,d>>, Sparse<G, [p,q], Scalar<ρ,e>>)
              -> Sparse<Csr, [m*p,n*q], Scalar<ρ,d*e>>
spsolve       : (Sparse<F, [n,n], Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>)
              -> Tensor<1, Scalar<div_rep(ρ),e/d>>
cg            : (Sparse<F, [n,n], Float[d]>, Tensor<1, Float[e]>, Float, Int)
              -> { x: Tensor<1, Float[e/d]>, iterations: Int, residual: Float, converged: Bool }
type Precond = | Identity | Jacobi | BlockJacobi | Ilu0 | Ic0 | Auto
gmres_with    : (Sparse<F, [n,n], Float[d]>, Tensor<1, Float[e]>, { tol: Float, maxiter: Int, restart: Int, precond: Precond })
              -> { x: Tensor<1, Float[e/d]>, iterations: Int, residual: Float, converged: Bool, precond: String }
bicgstab_with : (Sparse<F, [n,n], Float[d]>, Tensor<1, Float[e]>, { tol: Float, maxiter: Int, precond: Precond })
              -> { x: Tensor<1, Float[e/d]>, iterations: Int, residual: Float, converged: Bool, precond: String }
cg_with       : (Sparse<F, [n,n], Float[d]>, Tensor<1, Float[e]>, { tol: Float, maxiter: Int, precond: Precond })
              -> { x: Tensor<1, Float[e/d]>, iterations: Int, residual: Float, converged: Bool, precond: String }
contract      : (Sparse<F, shape, τ>, Sparse<G, shape, τ>, [Int], [Int]) -> Sparse<Coo, shape, τ>
map_nonzero   : ((Scalar<ρ,d>) -> Scalar<σ,e>, Sparse<F, shape, Scalar<ρ,d>>)
              -> Sparse<F, shape, Scalar<σ,e>>
nonzeros      : Sparse<F, shape, Scalar<ρ,d>> -> Tensor<1, (Tensor<1, Int>, Scalar<ρ,d>)>
```

**Non-symmetric systems: `gmres_with` and `bicgstab_with`.** `cg`/`cg_with` require symmetric positive definite matrices; for general square matrices use `gmres_with(A, b, { tol, maxiter, restart, precond })` (restarted GMRES, right-preconditioned, monotonically non-increasing residual norm, memory `O(restart·n)`) or `bicgstab_with(A, b, { tol, maxiter, precond })` (short recurrence, constant memory, possibly irregular residual history). Both come from rssn's Krylov kernels, start from the zero vector, and return a record shaped like `cg_with`'s (`x`, `iterations`, `residual`, `converged`, `precond`); missing the tolerance is `converged = false`, not an error, and `restart = 0` is `E0802`. For non-symmetric matrices `Ic0` and `Auto` both switch to `Ilu0` (`Auto` tries block ILU(0) first on `Bcsr`); a failed factorisation falls back to `Jacobi`, and the `precond` field reports the one actually used. Elements MUST be `Float` (use `spsolve` for exact elements); the result dimension is `e/d`.

**Preconditioned conjugate gradients.** `cg_with(A, b, { tol, maxiter, precond })` is `cg` with a preconditioner (`cg` itself is unchanged). `Precond` is a standard-library variant (`import LinearAlgebra.Sparse { Precond, Jacobi, cg_with }`): `Identity` (no preconditioner; not called `None` so as not to clash with `Option`'s `None`), `Jacobi` (diagonal scaling), `BlockJacobi` (inverted diagonal blocks), `Ilu0` (incomplete LU keeping the sparsity pattern), `Ic0` (incomplete Cholesky), and `Auto` (the first of IC(0) → ILU(0) → Jacobi that can be built; on `Bcsr`, block ILU(0) then block Jacobi). Only `Float`, symmetric positive definite. When a factorisation fails (zero pivot, indefinite), it falls back to Jacobi, and the result's `precond` field (`"Identity"`, `"Jacobi"`, `"BlockJacobi"`, `"BlockIlu0"`, `"Ilu0"`, `"Ic0"`) reports the one actually used. The direct method is `spsolve`: for `Float` elements it calls `faer`'s sparse LU (COLAMD ordering, no densification; a tridiagonal system of order 10^5 takes milliseconds); for symmetric positive definite matrices (strict symmetry and a positive diagonal are checked first) it tries sparse Cholesky and falls back to LU; exact elements (`Int`/`Real`/`Complex`/mixed) use sparse Gaussian elimination and stay exact; a singular matrix is `E0806`. `cg(A, b, tol, maxiter)` is the unpreconditioned version and returns `{ x, iterations, residual, converged }`.

**Parallelism and determinism.** `sparse @ dense` (including the `Bcsr` block kernel) runs in parallel by row blocks when the multiply-add count is at least 2^17: the block size is constant (256 rows, one block row for `Bcsr`), and each output row is accumulated by exactly one work item in column order, so the result is independent of the thread count and bit-identical to the sequential kernel. Krylov iteration is itself sequential (only `A @ x` is parallel), so the iteration sequence is also independent of the thread count.

**Slicing and overloads.** Sparse values support slicing: `A[i, :]`, `A[:, j]`, `A[r0..r1, c0..c1]`, `A[::2, 1..]` (same syntax as dense tensors). An integer index drops the axis; a range keeps the axis and renumbers it, so `A[i, :]` is a rank-1 `Sparse<Coo, [n], τ>`; a result whose rank is not 2 is always `Coo`. Slicing only filters and re-addresses stored entries — it does **not densify** — and costs `O(nnz)`; an out-of-range integer index is `E0802`, and the number of indices MUST equal the rank. Three existing names are also overloaded on `Sparse`: `sum(A)`, `sum(A, axis)` (the result is a dense tensor) and `get(A, i, j, …)` (also written `A[i, j, …]`; an unstored position returns the **implicit zero**).

**Types and formats.** `Csr`/`Csc`/`Coo`/`Block`/`Bcsr` are built-in `Format` tags (§3.1), not value constructors. `Sparse` is a core type constructor and `F` has kind `Format`; it is **not** erased to `Tensor`: there is no implicit conversion either way, and the format is part of the type (a `Sparse<Csr, …>` cannot be passed as `Sparse<Coo, …>`; use `convert<G>`). The `F` of `from_dense<F>`/`speye<F>`/`spdiag<F>` defaults to `Csr`; `convert<G>` MUST name the target format. `Csr`/`Csc` are two-dimensional formats; `Coo` is a sparse tensor of any rank. `Block` is another spelling of `Bcsr` — the same format tag — and `format_of` always reports `Bcsr`. The refined shape of `Sparse<F, [m,n], τ>` is checked and propagated (the inner dimension of `A @ B`, `A ± B`, `spsolve`, `transpose`, `[m*p, n*q]` for `kron`, `to_dense`/`from_dense`); a mismatch is a compile error.

**Block compressed sparse row (`Bcsr`).** A matrix is cut into dense `br × bc` blocks; only blocks containing a non-zero are stored, and blocks are compressed like CSR: `ptr` is the block-row pointer (length `m/br + 1`), `idx` the block-column index of each stored block (strictly increasing within a block row), and the value buffer holds `br·bc` slots per block (row-major within the block). The block shape MUST satisfy `br | m` and `bc | n`, and is read back with `block_size(A)` (`(1, 1)` for other formats). It suits systems with several unknowns per node (elasticity, Navier–Stokes, coupled reaction–diffusion).

- **Padding.** The invariant "a stored entry is never the implicit zero" is relaxed only inside a block in `Bcsr`: a stored **block** has at least one non-zero, and its other slots may be zero (padding). Padding is not a logical entry: `nnz(A)` counts logical non-zeros, `A[i, j]` answers the implicit zero at padding, and `nonzeros`, `==`, `show`/`repr`, `to_dense` and the kernel boundary see only logical non-zeros. All other operations go through the canonical term form in lexicographic order, which skips padding, so `Bcsr` behaves exactly like `Csr`; results stay `Bcsr` (the original block shape — `(bc, br)` after a transpose — is kept if it still divides the result shape, otherwise a shape is re-chosen as below). The padding of boxed elements is the zero of the element class; exact values are never rounded.
- **Block shape.** `to_bcsr(A, br, bc)` gives it explicitly; zero `br`/`bc`, a shape that does not divide `A`'s, or a rank other than 2 is `E0802`. `convert<Bcsr>`, `from_dense<Bcsr>`, `speye<Bcsr>`, `spdiag<Bcsr>` and results that need a new block shape choose by **fill ratio**: the candidates are the blocks of `{4, 3, 2, 1}²` that divide the shape (square first); fill ratio = stored slots / logical non-zeros; take the largest-area candidate whose ratio does not exceed `1.5`, ties to the lower ratio, then to the earlier candidate; if none qualifies, `(1, 1)`. The rssn kernel's `kernels::bsr::choose_block_size` uses the same rule.
- **Complexity.** With `B = br·bc` and `nb` stored blocks: storage is `nb·B + nb + m/br` words; `A @ x` (`k` columns) is `O(nb·B·k)`, multiplies only non-zero slots, and is bit-identical to the `Csr` kernel on the same matrix; conversion to canonical form is `O(nb·B)`; construction is `O(nnz + nb·B)` (automatic choice makes at most 15 `O(nnz)` passes). A fill ratio near 1 is best; blocks that are too large waste space — use smaller blocks or `Csr`.

**Run-time representation.** `Value::Sparse` is a first-class run-time value. It holds the shape, the element dimension, the format, and the stored entries in lexicographic order without duplicates (compressed pointers for CSR/CSC, flat coordinates for COO, block pointers and block-column indices for `Bcsr`); a stored entry is never the implicit zero (except `Bcsr` in-block padding; `SparseData::validate` checks this in debug builds). Element storage follows the dense-tensor rule: an `f64` buffer if and only if every stored element is `Float` (the implicit zero is then `0.0`), otherwise boxed `Int`/`Real`/`Complex`/symbolic scalars, with every operation going through the language's own scalar arithmetic — exact values are **never** rounded. The implicit zero is the zero of the element representation and dimension.

**Construction and validation.** The `indices` of `sparse_coo(shape, indices, values)` may be an `Int` matrix of shape `[nnz, rank]` or a list of `Int` tuples, and for rank 1 also a list of `Int`; out-of-range or negative indices and length mismatches are `E0802`. **Duplicate coordinates are summed** (finite-element assembly semantics) and entries that sum to zero are dropped. `sparse_csr(rows, cols, row_ptr, col_indices, values)` requires `row_ptr` of length `rows+1`, starting at `0`, monotone and ending at `nnz`, with strictly increasing column indices below `cols` in each row, otherwise `E0802`. `from_dense` drops zero elements and `to_dense` fills the implicit zeros back; a dense result with more elements than the implementation limit (10^8) is `E0802`, so an accidental densification is rejected instead of exhausting memory.

**Arithmetic and layout dispatch** (element dimensions follow exactly the dense-tensor rules: sums need equal dimensions `E0401`, products multiply, `spsolve` divides):

| Operation | Result |
|---|---|
| `sparse ± sparse` | `Sparse`, format of the left operand |
| `sparse ± dense`, `dense ± sparse` | dense `Tensor` |
| `sparse * s`, `s * sparse`, `sparse / s` (`s` a scalar or unit, including dimensioned scalars) | `Sparse` |
| `sparse * sparse`, `sparse * dense`, `dense * sparse` (Hadamard product) | `Sparse`; only stored entries are visited and the dense side's values at unstored positions are ignored |
| `sparse @ dense` (vector or matrix), `dense @ sparse` | dense `Tensor` |
| `sparse @ sparse`, `matmul`, `kron` | `Sparse<Csr, …>` |
| `-A`, `transpose(A)` | `Sparse`, same format |

`sparse / sparse`, scalar divided by sparse and `sparse + scalar` are undefined (they would densify or produce infinite zeros) and give `E0302`; `to_dense` first if needed. `==`/`!=` compare the **logical content** of two sparse values (same shape and same stored entries, with numbers compared by the dense-tensor rules so `1` equals `1.0`), **independent of format**. `show` writes `sparse(Csr, [3, 3], { (0, 0) = 4.0, … })`; `repr` is an expression that rebuilds the same value, `convert<Csr>(sparse_coo([3, 3], [(0, 0), …], [4.0, …]))`, and for `Bcsr` it includes the block shape: `to_bcsr(sparse_coo([4, 4], […], […]), 2, 2)`.

**Rank N and contraction.** `contract(A, B, axes_a, axes_b)` is pairwise tensor contraction (`tensordot`): `axes_a`/`axes_b` are **list literals** (so the result rank is static), and the result axes are the free axes of `A` followed by those of `B`. Two sparse operands give `Sparse<Coo, …>`; one sparse and one dense give a dense `Tensor`; contracting all axes gives a scalar; two dense operands are a type error. `map_nonzero(f, A)` calls `f` only on **stored entries**, so `f(0) = 0` is required (entries returning zero are dropped); `nonzeros(A)` returns a list of `(index, value)` tuples.

**Boundary with the kernel.** rssn's `TypeExpr::Sparse` has only a `rank`, so `Sparse<F, shape, τ>` is lowered at the boundary to rank plus a run-time format; `aevia-rssn`'s `Val::Sparse` corresponds one-to-one to rssn's `Value::Sparse`, **without densifying**. rssn's `term_of_value` does not accept `Value::Sparse` into the term graph (`in_term_graph = false` in `manifest.value_kinds`), so a sparse value cannot be fed directly to general rewriting operators; sparse operations are carried by Aevia's own layout kernels, with `faer` for sparse LU. User-defined formats wait until rssn opens `Format` (§13.9.5).

> **Status**: all of the above is implemented (tests `aevia-core/tests/sparse.rs`, `sparse_block.rs`, `sparse_followup.rs`, `shape_refinement.rs`), and the capability `storage.sparse` is enabled. **Remaining gaps**: sparse values cannot enter rssn's term graph; `gmres_with`/`bicgstab_with` do not accept exact elements; there are no sparse eigenvalues or SVD.

## 8.5 Calculus

```
diff          : (Real[d] -> Real[e]) -> (Real[d] -> Real[e/d])
diff_at       : ((Real[d] -> Real[e]), Real[d]) -> Real[e/d]
numeric_deriv : (Float[d] -> Float[e]) -> (Float[d] -> Float[e/d])   # numeric / AD
antiderivative: (Real[d] -> Real[e]) -> (Real[d] -> Real[e*d])
defint        : (Real[d] -> Real[e], Real[d], Real[d]) -> Result<Real[e*d], MathError>
quadrature    : (Float[d] -> Float[e], Float[d], Float[d]) -> Result<Float[e*d], MathError>
limit         : (Real[d] -> Real[e], Real[d]) -> Result<Real[e], MathError>
series        : (Real[d] -> Real[e], Real[d], Int) -> Real[e]
sum           : (Real[1] -> Real[d], Real[1], Real[1]) -> Real[d]
sum_f         : (Float[1] -> Float[d], Float[1], Float[1]) -> Float[d]
product       : (Real[1] -> Real[1], Real[1], Real[1]) -> Real[1]
product_dyn   : (Real[1] -> Real[d], Real[1], Real[1]) -> Dyn<Real>
grad          : ((Tensor<1, Real[d]> -> Real[e]), Tensor<1, Real[d]>)
              -> Tensor<1, Real[e/d]>
grad_f        : ((Tensor<1, Float[d]> -> Float[e]), Tensor<1, Float[d]>)
              -> Tensor<1, Float[e/d]>
jacobian      : ((Tensor<1, Real[d]> -> Tensor<1, Real[e]>), Tensor<1, Real[d]>)
              -> Tensor<2, Real[e/d]>
hessian       : ((Tensor<1, Real[d]> -> Real[e]), Tensor<1, Real[d]>)
              -> Tensor<2, Real[e/d^2]>
divergence    : ((Tensor<1, Real[d]> -> Tensor<1, Real[e]>), Tensor<1, Real[d]>)
              -> Real[e/d]
curl          : ((Tensor<1, Real[L]> -> Tensor<1, Real[d]>), Tensor<1, Real[L]>)
              -> Tensor<1, Real[d/L]>
laplacian     : ((Tensor<1, Real[d]> -> Real[e]), Tensor<1, Real[d]>)
              -> Real[e/d^2]
```

`diff`/`antiderivative`/`defint`/`limit`/`series`/`grad` run in the symbolic phase and take `Real` arguments. Numeric data uses `numeric_deriv`/`quadrature`/`grad_f`/`sum_f`, which run on `Float` kernels (finite differences, automatic differentiation, quadrature) and never convert `Float` data to `Real`.

The index of `sum` is a dimensionless `Real`, so `k` may be symbolic; `product` supports dimensionless values only — use `product_dyn` (returning `Dyn<Real>`) for dimensioned ones.

Both bounds of `sum(f, a, b)`/`product(f, a, b)` MUST be integers (an exact non-integer bound is `E0302`). A concrete range of at most 10^4 terms is summed exactly term by term; an infinite bound (`oo`), a symbolic bound or a longer range asks the kernel for a closed form (`sum(fn k => 1/k^2, 1, oo) = pi^2/6`, `sum(fn k => k, 1, n) = n^2/2 + n/2`, the geometric series `sum(fn k => 2^(-k), 0, oo) = 2`). When the kernel finds no closed form the result is `E0808`; an unevaluated `sum(…)` is never returned as if it were an answer.

### 8.5.1 Algebraic forms

```
expand   : Real[d] -> Real[d]
factor   : Real[d] -> Real[d]            # factorisation over the rationals; the Int overload is in §8.16
cancel   : Real[d] -> Real[d]            # cancels common factors of a rational function: p/q, gcd(p, q) = 1
together : Real[d] -> Real[d]            # puts over a common denominator
apart    : (Real[d], Real[e]) -> Real[d] # partial fractions in the variable
collect  : (Real[d], Real[e]) -> Real[d] # collects like terms by powers of the variable
degree   : (Real[d], Real[e]) -> Int     # polynomial degree in the variable
```

These functions change the **form** of an exact expression without changing its value, so the result has the dimension of the argument. They complement `simplify`: `simplify` lets the kernel choose the "best" equivalent form (usually the shortest), while users often need a specific one — expand to read coefficients, factor to read roots, split into partial fractions to integrate term by term. Each calls the same-named operator of the kernel's `poly` rule set directly instead of issuing a `simplify` request (which would turn `factor(x^4 - 1)` back into the shorter `x^4 - 1`). If the kernel does not produce the form the result is `E0808`; an unevaluated `factor(…)` is never returned. For an `Int` argument `factor` remains the integer factorisation of §8.16: `factor(360)` is `[(2, 3), (3, 2), (5, 1)]`, whereas `factor(x^4 - 1)` is `(x - 1)*(x + 1)*(x^2 + 1)`.

## 8.6 Solve / Optimize

```
solve        : (Real[d] -> Real[e]) -> Result<Solutions<Real,d>, MathError>
solve_complex: (Real[d] -> Complex[e]) -> Result<Solutions<Complex,d>, MathError>
nsolve       : (Real[d] -> Real[e], Real[d]) -> Result<Real[d], MathError>
nsolve_f     : (Float[d] -> Float[e], Float[d]) -> Result<Float[d], MathError>
root         : same as nsolve
solve_system : (Tensor<1, Real[d]> -> Tensor<1, Real[e]>, Tensor<1, Real[d]>)
             -> Result<Tensor<1, Real[d]>, MathError>
minimize     : (Tensor<1, Real[d]> -> Real[e], Tensor<1, Real[d]>)
             -> Result<Tensor<1, Real[d]>, MathError>
minimize_f   : (Tensor<1, Float[d]> -> Float[e], Tensor<1, Float[d]>)
             -> Result<Tensor<1, Float[d]>, MathError>
maximize     : (Tensor<1, Real[d]> -> Real[e], Tensor<1, Real[d]>)
             -> Result<Tensor<1, Real[d]>, MathError>    # same shape as minimize (Real form), not minimize_f
least_squares: ((Tensor<1, Real[d]> -> Tensor<1, Real[e]>), Tensor<1, Real[d]>)
             -> Result<Tensor<1, Real[d]>, MathError>
linsolve     : (Tensor<2, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>)
             -> Result<Tensor<1, Scalar<div_rep(ρ),e/d>>, LinAlgError>
```

`Solutions<ρ,d>` is `{ variable: Option<String>, roots: Tensor<1, Scalar<ρ,d>>, multiplicities: Tensor<1, Int>, conditions: Tensor<1, Condition>, complete: Bool }`; it keeps multiplicities and parameter conditions and is not a `Set`.

The record is rssn's `Solutions` record (`Solutions::to_value`): field names, order and meaning correspond one-to-one and the bridge does not reinterpret them, so "multiplicities not reported" from the kernel is an empty table and "incomplete" is `false`. The only reshaping is `variable` — a symbol on the rssn side (the empty string when there is no unknown), an `Option<String>` on the Aevia side.

- `multiplicities` is the solver's multiplicity table; empty means **not reported**. rssn's `multiplicity()` accessor returns 1 for an empty table, but that is a default answer when there is no information, not a declaration; Aevia keeps the empty meaning, does not treat it as "all 1", and does not infer multiplicities from repeated roots. The kernel fills it when it **can prove** it: when the equation is a univariate rational polynomial and every root is an exact number, an exact square-free decomposition gives the multiplicities (`(x-1)^2*(x+2)` → `[1, 2]`); for irrational or symbolic roots the table stays empty (matching symbolic roots to factors numerically would be a guess).
- `conditions` holds only conditions the solver can declare **by construction**, e.g. the root `-b/a` of `solve(a*x + b = 0, x)` carries `a != 0`; reality and branch conditions are not filled. When `complete = false`, both `multiplicities` and `conditions` stay empty: a single root found along the way is not enough to declare multiplicity or validity.
- `complete` is the solver's declaration: `true` means it returned an exhaustive form (rssn's `list(...)`); for a single value rssn gives `false`. The bridge passes this value through unchanged and never writes a constant. Symbolic roots inside `list(...)` are expanded one by one into elements of `roots`, so "the solver gave an exhaustive list" and "the bridge got a single root" are distinguishable. **`complete` is not a mathematical proof**, only "the solver did not declare incompleteness".

`Condition` is `{ kind: String, text: String, expression: Option<Value> }`: `kind` is the lower-case name of rssn's `ConditionKind` (`nonzero`, `positive`, `real`, …), `text` is the human-readable predicate, and `expression` is the structured condition the solver kept (a `Value` on the rssn side), absent when the solver kept none. The language has no type named like `ConditionKind`, so the element type is the opaque `Condition` and fields are read by value.

`solve` returns real solutions; `solve_complex` returns complex ones. A failed numeric solve returns `Err`. `linsolve` solves linear systems, and is named to avoid clashing with the symbolic `solve`.

## 8.7 Stats

```
mean      : Tensor<n, Scalar<ρ,d>> -> Scalar<div_rep(ρ),d>
median    : Tensor<1, Scalar<ρ,d>> -> Scalar<ρ,d>
centroid  : Tensor<1, Point[d]> -> Point[d]
quantile  : (Tensor<1, Scalar<ρ,d>>, Scalar<ρ,1>) -> Scalar<ρ,d>
var       : Tensor<n, Scalar<ρ,d>> -> Scalar<div_rep(ρ),d^2>
var       : (Tensor<n, Scalar<ρ,d>>, Bool) -> Scalar<div_rep(ρ),d^2>   # population
std       : Tensor<n, Scalar<ρ,d>> -> Scalar<div_rep(ρ),d>
cov       : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Scalar<div_rep(ρ),d*e>
corr      : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Scalar<div_rep(ρ),1>
skewness  : Tensor<1, Scalar<ρ,d>> -> Scalar<ρ,1>
kurtosis  : Tensor<1, Scalar<ρ,d>> -> Scalar<ρ,1>
histogram : (Tensor<1, Scalar<ρ,d>>, Int) -> { counts: Tensor<1, Int>, edges: Tensor<1, Scalar<ρ,d>> }
mode      : Tensor<1, Scalar<ρ,d>> -> Scalar<ρ,d>
```

### 8.7.1 Distributions

Distributions are **nominal types**, not a multi-parameter type class (the language has no multi-parameter classes, §3.8.1):

```
type Distribution<d> =
    | Normal(Normal<d>)
    | Uniform(Uniform<d>)
    | Exponential(Exponential<d>)
    | Gamma(Gamma<d>)
    | Beta(Beta)
    | ChiSquared(ChiSquared)
    | StudentT(StudentT)
    | Poisson(Poisson)
    | Binomial(Binomial)
    | LogNormal(LogNormal)
    | Weibull(Weibull<d>)
    | Cauchy(Cauchy<d>)
    | F(F)
    | NegativeBinomial(NegativeBinomial)
    | Geometric(Geometric)

type Normal<d>      = { mu: Real[d], sigma: Real[d] }
type Uniform<d>     = { lo: Real[d], hi: Real[d] }
type Exponential<d> = { rate: Real[1/d] }
type Gamma<d>       = { shape: Real[1], rate: Real[1/d] }
type Beta           = { alpha: Real[1], beta: Real[1] }
type ChiSquared     = { df: Real[1] }
type StudentT       = { df: Real[1] }
type Poisson        = { lambda: Real[1] }
type Binomial       = { n: Int, p: Real[1] }
type LogNormal       = { mu: Real[1], sigma: Real[1] }
type Weibull<d>      = { shape: Real[1], scale: Real[d] }
type Cauchy<d>       = { loc: Real[d], scale: Real[d] }
type F               = { df1: Real[1], df2: Real[1] }
type NegativeBinomial = { r: Real[1], p: Real[1] }
type Geometric       = { p: Real[1] }

```

These declarations are the source of the `Stats.Distributions` module (after `open Stats.Distributions`, `Normal({ mu = 1.7 m, sigma = 0.1 m })` is a `Distribution<L>` value). `pdf`, `cdf`, `sample` and `sample_n` are **built-in overloads** over this closed ADT:

```
pdf      : (Distribution<d>, Real[d]) -> Real[1/d]
cdf      : (Distribution<d>, Real[d]) -> Real[1]
sample   : Distribution<d> -> IO Real[d]
sample_n : (Distribution<d>, Int, Option<Int>) -> Tensor<1, Real[d]>
```

The dimension of `pdf` for `Normal<L>` is `1/L`; `cdf` is dimensionless; `sample` returns `Real[L]`. `Weibull` uses its `scale` dimension and `Cauchy` its `loc`/`scale` dimension; `LogNormal` parameters describe the dimensionless logarithm; `NegativeBinomial` and `Geometric` count failures before successes, with support `0, 1, ...`. All fifteen constructors share the same `pdf`, `cdf`, `sample` and `sample_n` built-ins.

**User-defined distributions are not a language extension point.** Multi-parameter classes were removed (§3.8.1), and the map "distribution parameters → dimension" needs a type function (like `RateOf`), which is a separate design. User-defined distributions lie outside the extension boundary of §12.8.13; the standard library may add built-in constructors for new distribution families, but that is a closed library set, not a language extension point.

## 8.8 FFT / Interpolate

```
fft      : Tensor<1, X[d]> -> Tensor<1, Complex[d]>       # unnormalised; X ∈ {Complex, Float, Int}
ifft     : Tensor<1, X[d]> -> Tensor<1, Complex[d]>       # 1/N
fft_norm : (Tensor<1, X[d]>, Norm) -> Tensor<1, Complex[d]>
ifft_norm : (Tensor<1, X[d]>, Norm) -> Tensor<1, Complex[d]>
fft_axis : (Tensor<n, X[d]>, Int) -> Tensor<n, Complex[d]>      # along one axis
ifft_axis : (Tensor<n, X[d]>, Int) -> Tensor<n, Complex[d]>
fft2     : Tensor<n, X[d]> -> Tensor<n, Complex[d]>             # last two axes
ifft2    : Tensor<n, X[d]> -> Tensor<n, Complex[d]>
fftn     : Tensor<n, X[d]> -> Tensor<n, Complex[d]>             # all axes
ifftn    : Tensor<n, X[d]> -> Tensor<n, Complex[d]>
fftn_axes : (Tensor<n, X[d]>, Tensor<1, Int>) -> Tensor<n, Complex[d]>   # arbitrary list of axes
ifftn_axes : (Tensor<n, X[d]>, Tensor<1, Int>) -> Tensor<n, Complex[d]>
fftn_norm : (Tensor<n, X[d]>, Norm) -> Tensor<n, Complex[d]>
ifftn_norm : (Tensor<n, X[d]>, Norm) -> Tensor<n, Complex[d]>
rfft     : Tensor<1, Float[d]> -> Tensor<1, Complex[d]>
irfft    : Tensor<1, Complex[d]> -> Tensor<1, Float[d]>
irfft    : (Tensor<1, Complex[d]>, Int) -> Tensor<1, Float[d]>  # explicit length (odd lengths)
rfft2    : Tensor<n, Float[d]> -> Tensor<n, Complex[d]>         # last two axes
irfft2   : Tensor<n, Complex[d]> -> Tensor<n, Float[d]>
irfft2   : (Tensor<n, Complex[d]>, Int) -> Tensor<n, Float[d]>
rfftn    : Tensor<n, Float[d]> -> Tensor<n, Complex[d]>         # all axes
irfftn   : Tensor<n, Complex[d]> -> Tensor<n, Float[d]>
irfftn   : (Tensor<n, Complex[d]>, Int) -> Tensor<n, Float[d]>
rfftn_axes : (Tensor<n, Float[d]>, Tensor<1, Int>) -> Tensor<n, Complex[d]>   # half spectrum on the last listed axis
irfftn_axes : (Tensor<n, Complex[d]>, Tensor<1, Int>) -> Tensor<n, Float[d]>
irfftn_axes : (Tensor<n, Complex[d]>, Tensor<1, Int>, Int) -> Tensor<n, Float[d]>   # explicit length (odd lengths)
fftshift : Tensor<n, a> -> Tensor<n, a>                         # all axes
ifftshift : Tensor<n, a> -> Tensor<n, a>
fftshift_axis : (Tensor<n, a>, Int) -> Tensor<n, a>
ifftshift_axis : (Tensor<n, a>, Int) -> Tensor<n, a>
fftfreq  : (Int, Float[T]) -> Tensor<1, Float[1/T]>
rfftfreq : (Int, Float[T]) -> Tensor<1, Float[1/T]>             # n/2 + 1 non-negative frequencies

interp1  : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Scalar<ρ,d>) -> Scalar<ρ,e>
interp2  : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Tensor<2, Scalar<ρ,f>>,
            Scalar<ρ,d>, Scalar<ρ,e>) -> Scalar<ρ,f>
spline   : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Scalar<ρ,d>) -> Scalar<ρ,e>
polyfit  : (Tensor<1, Scalar<ρ,1>>, Tensor<1, Scalar<ρ,e>>, Int) -> Tensor<1, Scalar<ρ,e>>
polyval  : (Tensor<1, Scalar<ρ,e>>, Scalar<ρ,1>) -> Scalar<ρ,e>
chebyshev_fit : (Tensor<1, Scalar<ρ,1>>, Tensor<1, Scalar<ρ,e>>, Int) -> Tensor<1, Scalar<ρ,e>>
```

`fft` is unnormalised and `ifft` divides by `N`, so `ifft(fft(x)) == x`. For `1/√N` or other conventions use `fft_norm(x, Unitary)` (`import FFT { Unitary }`, or write `FFT.Unitary`).

**The transform family.** All forward and inverse transforms accept complex, `Float` or `Int` tensors (real and integer samples are promoted with imaginary part 0); the result is always complex with the same dimension as the samples. A frequency-axis dimension appears only in the results of `fftfreq`/`rfftfreq`, which take the sampling interval `Float[T]` and return `Float[1/T]` (`fftfreq(4, 0.5 s)` is in `Hz`). Multi-dimensional transforms are implemented by row–column decomposition, applying the 1-D transform along each transformed axis in turn; every rank is typed (§8.1.5).

- `fft2`/`ifft2` transform the last two axes; `fftn`/`ifftn` all axes; `fft_axis(t, k)` one axis; `fftn_axes(t, axes)` an arbitrary list. Axes count from 0 and negative values count from the end; an out-of-range or duplicate axis is `E0802`.
- Normalisation uses the total number of transformed points `N` (the product of the transformed axis lengths): `Backward` is 1 forward and `1/N` inverse; `Forward` is `1/N` forward and 1 inverse; `Unitary` is `1/√N` both ways.
- `rfft`/`rfft2`/`rfftn` take real input and keep only the `n/2 + 1` non-redundant frequencies along the last transformed axis; `irfft`/`irfft2`/`irfftn` rebuild the real tensor from the half spectrum, with the original length defaulting to `2(m − 1)` (even); odd lengths are given explicitly by the form taking an `Int`; a length incompatible with `m` is `E0802`.
- `rfftn_axes(t, axes)`/`irfftn_axes(s, axes)`/`irfftn_axes(s, axes, n)` are real transforms over any subset of axes (matching numpy's `rfftn(a, axes=…)`): the **last** axis in the list is the half-spectrum axis and the others get full complex transforms; `irfftn_axes` does the Hermitian reconstruction on the last listed axis, with `n` the original length of that axis (default `2(m − 1)`). An empty list, or an out-of-range or duplicate axis, is `E0802`.
- `fftshift`/`ifftshift` move the zero frequency to the middle (each shifted axis rotates cyclically by `⌊n/2⌋`, the inverse the other way) and work for any element type; the `*_axis` forms shift a single axis.

```
type Norm = | Backward | Forward | Unitary
type AxisScale = | Linear | Log
```

`polyfit` requires a dimensionless independent variable; for a dimensioned one, divide by a unit first (`x/u`); the coefficients keep the dimension of `y`. This is a dimension-system constraint on the API: the coefficient `a_i` of a polynomial has dimension `e/d^i`, different for each power, and cannot sit in a homogeneous tensor.

## 8.9 ODE

```
type OdeMethod =
    | Rk4 | Dopri5 | Radau | Bdf | Rosenbrock23 | AutoSwitch
    | StormerVerlet | Ruth4 | GaussLegendre4 | GaussLegendre6
type OdeOpts<s> = {
    rtol: Float[1],
    atol: Float[1],
    method: OdeMethod,
    steps: Option<Int>,                 # Rk4 / geometric integrators: fixed sub-steps
    events: Option<(Float[T], s) -> Float[1]>,
}
```

```
odeint   : (((Float[T], s) -> RateOf<s>), s, Tensor<1, Float[T]>)
         -> Tensor<1, s>
odeint_with : (((Float[T], s) -> RateOf<s>), s, Tensor<1, Float[T]>, OdeOpts<s>)
           -> { t: Tensor<1, Float[T]>, y: Tensor<1, s> }
solve_ivp: (((Float[T], s) -> RateOf<s>), s, (Float[T], Float[T]), OdeOpts<s>)
         -> { t: Tensor<1, Float[T]>, y: Tensor<1, s> }
```

`RateOf<s>` is a built-in type function, defined recursively:

```
RateOf<Scalar<ρ,d>>        = Scalar<ρ,d/T>
RateOf<Tensor<n, τ>>       = Tensor<n, RateOf<τ>>
RateOf<{ l1: τ1, ..., ln: τn }> = { l1: RateOf<τ1>, ..., ln: RateOf<τn> }
```

If `s = { x: Float[L], v: Float[L/T] }`, then `RateOf<s> = { x: Float[L/T], v: Float[L/T^2] }`. The `RateOf` of the scalar state `Float[1]` is `Float[1/T]`. A state containing `Int`, `Bool` or function fields is invalid (`E0302`); capture constant parameters in the closure of `rhs`. The compiler checks field by field; users write no instances and there is no `OdeState` class. An ODE right-hand side (such as `rhs`) MUST take two parameters `(t, s)`; write `_` for an unused time parameter.

`events` stops and interpolates when a `Float[1]` changes sign. `Dopri5`, `Radau`, `Bdf`, `Rosenbrock23` and the fixed-step `Rk4` all support events (rssn's `solve_fixed_events`); the difference is that the fixed-step method needs `steps`, and event location accuracy also depends on the step. `odeint_with` returns the `t`/`y` actually reached. `odeint` is shorthand for the defaults (`rtol=1e-8`, `atol=1e-10`, `Dopri5`).

`StormerVerlet` and `Ruth4` are second- and fourth-order fixed-step symplectic integrators; the flattened state MUST have even length, with the first half the positions `q` and the second half the momenta `p`, and the right-hand side a separable canonical Hamiltonian system. `GaussLegendre4` and `GaussLegendre6` are fourth- and sixth-order implicit Gauss–Legendre RK methods and support general states. These four methods take their number of fixed sub-steps from `steps`, and an unconverged nonlinear stage is an error.

**Flattening contract.** The bridge flattens `s` into a `Vec<f64>` in **record field declaration order**, depth first, tensors row-major, and restores it in the same order after solving. Nested records flatten recursively; `Tensor<n, Float[d]>` flattens row-major. Only `Float` scalar and tensor fields are supported; a state with `Int`, `Bool` or function fields is `E0302` (capture constants in the `rhs` closure). `Tensor<1, s>` uses `Boxed` storage; numeric kernels see only flat `f64`, and Aevia boxes and unboxes.

**8.9.0 Solution objects, dense output, multiple events, higher-order equations, boundary-value problems**

`odeint`/`solve_ivp` give only discrete output and one terminal event. For values at **arbitrary times**, several events (with direction, terminal or not), higher-order equations or boundary-value problems, use the entry points below. They share `OdeOpts<s>`, `RateOf<s>` and the flattening contract with `odeint_with`, and have the same shape as the SDE event and dense-output API of §8.9.1 (`{ g, terminal, direction }`, `{ index, t, y }`, `at`).

```
type OdeEvent<s> = {
    g: (Float[T], s) -> Float[1],   # a sign change is an event; must return a dimensionless number
    terminal: Bool,                 # true: integration stops at the event
    direction: Int,                 # -1 falling crossings only, 0 both directions, 1 rising crossings only
}
type OdeHit<s> = { index: Int, t: Float[T], y: s }
type OdeSolution<s> = {
    t: Tensor<1, Float[T]>, y: Tensor<1, s>,        # accepted step nodes
    at: (Float[T]) -> s,                            # continuous extension: value at any t
    events: Tensor<1, OdeHit<s>>,                   # events sorted by time
    nfev: Int, rejected: Int, method: String,
}

ode_solve : (((Float[T], s) -> RateOf<s>), s, (Float[T], Float[T]), OdeOpts<s>, Tensor<1, OdeEvent<s>>)
          -> OdeSolution<s>
odeint_order : (((Float[T], y0, y1, ..., yk) -> RateOf^(k+1)<y0>), (y0, y1, ..., yk), Tensor<1, Float[T]>)
          -> { t: Tensor<1, Float[T]>, y: Tensor<1, y0>, d: Tensor<1, (y0, y1, ..., yk)> }
odeint_order_with : (rhs, (y0, y1, ..., yk), Tensor<1, Float[T]>, OdeOpts<y0>)
          -> { t: Tensor<1, Float[T]>, y: Tensor<1, y0>, d: Tensor<1, (y0, y1, ..., yk)> }
ode_bvp : (((Float[T], s) -> RateOf<s>), s, (Float[T], Float[T]), ((s, s) -> Tensor<1, Float[d]>), OdeOpts<s>)
          -> OdeSolution<s>
```

**Continuous output.** `ode_solve(rhs, y0, (t0, t1), opts, events)` integrates over the whole interval (`t1 ≥ t0`; backward integration is not supported) and keeps the method's own continuous extension; `sol.at(t)` evaluates at any time in `[t0, t_end]` (outside the interval is `E0802`, a dimension other than `T` is `E0401`). `Dopri5` uses rssn's 4th-order continuous extension; `Radau`/`Bdf`/`Rosenbrock23` use their collocation polynomials / backward-difference interpolation; `Rk4` has no stage data and uses cubic Hermite interpolation on the nodes (interpolation error `O(h^4)`, fourth-order convergence in the step). Here the `steps` of `Rk4` is the **total number of steps over the whole interval** (in `odeint_with` it is the number of sub-steps per output interval). `at` is a function value (not data), so a solution containing `at` cannot be returned across threads; to return one from `par_map`, take only `t`/`y`.

**Events.** `events` is a list of `OdeEvent` and may be empty (`[]`). The `g` of each event is located by root finding on the continuous extension (`Rk4`: bisection on re-integration) when it changes sign between accepted steps (`g0 < 0 ≤ g1` rising, `g0 > 0 ≥ g1` falling, filtered by `direction`); a hit is recorded in `events` (`index` is the list position); an event with `terminal = true` ends the integration at that point, so the last node of `t`/`y` is the hit and the valid interval of `at` is truncated accordingly. Several hits in one step are sorted by time and hits after a terminal event are discarded. A `g` returning a dimensioned value is `E0302`; a direction other than -1/0/1 is `E0802`.

**Stiff solver selection.** `AutoSwitch` starts with `Dopri5` and gives it only 20000 attempted steps; when the budget is exhausted (a stiff problem limited by stability, needing far more steps than accuracy requires) it re-integrates the whole interval with `Radau`, and `sol.method` reports the method finally used (`"Dopri5"` or `"Radau"`). The decision is by **step budget**, not an eigenvalue estimate: a moderately stiff problem (needing tens of thousands of steps) wastes one explicit pass; choose `Radau`/`Bdf`/`Rosenbrock23` directly when stiffness is known. `AutoSwitch` also works in `odeint_with`, `solve_ivp` and event integration (same rule).

**Higher-order equations.** `odeint_order(f, (y0, y1, …, y_{n-1}), times)` integrates `y⁽ⁿ⁾ = f(t, y, y′, …, y⁽ⁿ⁻¹⁾)` (`n` is the parameter count of `f` minus one, `n ≥ 1`), internally reduced to a first-order system on the stacked state `z = (y, y′, …, y⁽ⁿ⁻¹⁾)` in which every component except `y⁽ⁿ⁾ = f` is trivial (`zₖ′ = zₖ₊₁`). **Dimensions are tracked order by order**: the type of the `k`-th derivative is `RateOf` applied `k` times, so for `y : Float[L]`, `y′ : Float[L/T]`, `y″ : Float[L/T^2]` and `f` MUST return `L/T^n` (`E0401`/`E0302` at check time; at run time the dimension and structure of every initial value is checked again, `E0401` on a mismatch). `y` may be a scalar, tensor or record, with the same structure for all derivatives. The returned `d` is the history of derivative tuples and `y` is order 0. `odeint_order_with` takes an extra `OdeOpts<y0>` (method, tolerances; `events` MUST be `None` — for events, reduce the order by hand and use `ode_solve`). Example (spring `x″ = -(k/m) x`):

```
let sol = odeint_order(fn (_t, x, v) => -(k / m) * x, (1.0 m, 0.0 m/s), times)
# x : Float[L], v : Float[L/T], returns Float[L/T^2]
```

**Boundary-value problems (single shooting).** `ode_bvp(rhs, guess, (t0, t1), bc, opts)` solves `y′ = rhs(t, y)`, `bc(y(t0), y(t1)) = 0`, where `bc` returns a residual tensor with as many entries as state components. The algorithm is Newton iteration on the initial state (forward-difference Jacobian, one integration per unknown; backtracking line search); `guess` is the Newton starting point, convergence is `‖r‖∞ ≤ 1e-9 (1 + ‖y0‖∞)` with at most 60 iterations; it returns the `OdeSolution` of the last integration (so `at` is available). For second-order problems, write the state `(y, y′)` as in the order reduction of `odeint_order`. A wrong number of residuals is `E0802`; a singular Jacobian or non-convergence is `NotConverged`. **Limitation**: this is single shooting and suits only problems where the sensitivity of `y(t1)` to `y(t0)` does not blow up; exponentially unstable (long-interval, stiff) boundary-value problems need multiple shooting or collocation, which are **not** provided.

Imported `odeint`/`odeint_with`/`solve_ivp` (and `sdeint_*` of §8.9.1, e.g. `import Ode { odeint }`) **keep the type-function semantics**: the `RateOf<s>`/`DiffusionOf<s>` checks are determined by the entry point itself, the standard-library module merely re-exports and does not change the signature, and only a local redefinition of the same name shadows it.

## 8.9.1 SDE

SDEs share `RateOf` and the flattening contract with ODEs (§8.9) but add a noise structure and a stochastic-integration convention. The method name itself encodes the convention: a name containing `stratonovich` converges to the Stratonovich solution and the rest are Itô; there is no `calculus` option orthogonal to the method (a scheme whose name does not state its convention is almost always misused).

```
type SdeMethod =
    | EulerMaruyama | Milstein | MilsteinStratonovich
    | HeunStratonovich | SrkPlaten | MilsteinNoncommutative
    | MilsteinNoncommutativeStratonovich
    | DriftImplicitEuler | ThetaMaruyama | DriftImplicitMilstein

type SdeStructure =
    | Diagonal                     # one noise channel per state component, m = n
    | General(Int)                 # (m)；the caller declares the noise commutative
    | GeneralNoncommutative(Int)   # (m)；non-commutative noise: the method name dispatches to the Lévy-area kernel

type SdeFixedOpts<T> = {
    structure: SdeStructure,
    method: SdeMethod,
    seed: Int,
    t0: Float[T], dt: Float[T], steps: Int,
    terms: Int,                    # Fourier modes of the Lévy area; 0 = default clamp(ceil(1/√h), 16, 4096), h the step
}

type SdeAdaptiveOpts<T> = {
    structure: SdeStructure,
    method: SdeMethod,
    seed: Int,
    t0: Float[T], t1: Float[T],
    rtol: Float[1], atol: Float[1],
    max_steps: Int,
    cells: Int,                    # Brownian grid cells for non-commutative adaptive runs; 0 = default 4096, ignored for commutative noise
    terms: Int,                    # as above; h is the cell width (t1 - t0)/cells
}

type SdeOpts<T> = | Fixed(SdeFixedOpts<T>) | Adaptive(SdeAdaptiveOpts<T>)

type SdePath<s> = {
    t: Tensor<1, Float[T]>, y: Tensor<1, s>,
    steps: Int, rejected: Int, nfev: Int,
    seed: Int, noise: Int, method: SdeMethod,
}

sdeint_diag : (((Float[T], s) -> RateOf<s>),
               ((Float[T], s) -> DiffusionOf<s>),
               s, SdeOpts<T>) -> Result<SdePath<s>, SdeError>

sdeint_general : (((Float[T], s) -> RateOf<s>),
                  ((Float[T], s) -> GeneralDiffusionOf<s>),
                  s, SdeOpts<T>) -> Result<SdePath<s>, SdeError>
                  # s = Float[d]       : GeneralDiffusionOf<s> = Tensor<2, Scalar<ρ, d/T^(1/2)>>  (1×m)
                  # s = Tensor<1, τ>   : Tensor<2, DiffusionOf<τ>>                               (n×m)
                  # s = { l: τ, … }    : { l: GeneralDiffusionOf<τ>, … }(record state, see below)

wiener           : (Int, Int, Int, Float[T]) -> Tensor<2, Float[T^(1/2)]>
                   # (seed, m, steps, dt)；same order as the manifest's sde_wiener(seed, m, steps, h)
sde_order        : SdeMethod -> { strong: Float[1], weak: Float[1] }
                   # order under commutative noise
sde_order_with   : (SdeMethod, SdeStructure) -> { strong: Float[1], weak: Float[1] }
                   # order by structure: HeunStratonovich has only strong order 1/2 on non-commutative noise
sde_ensemble_seed: (Int, Int) -> Int     # (seed, index), pure function

type SdeEvent<s> = {
    g: (Float[T], s) -> Float[1],  # a sign change is an event; dimensionless
    terminal: Bool,                # true: the path is cut at the first such event
    direction: Int,                # -1 falling, 0 both directions, 1 rising
}
type SdeHit<s> = { index: Int, t: Float[T], y: s }

sde_at       : (SdePath<s>, Float[T]) -> s
sde_resample : (SdePath<s>, Tensor<1, Float[T]>) -> Tensor<1, s>
sde_interp   : SdePath<s> -> ((Float[T]) -> s)
sde_events   : (SdePath<s>, Tensor<1, SdeEvent<s>>) -> { path: SdePath<s>, hits: Tensor<1, SdeHit<s>> }
sde_ensemble_diag    : (((Float[T], s) -> RateOf<s>), ((Float[T], s) -> DiffusionOf<s>), s, SdeOpts<T>, Int)
                       -> Tensor<1, Result<SdePath<s>, SdeError>>
sde_ensemble_general : (((Float[T], s) -> RateOf<s>), ((Float[T], s) -> GeneralDiffusionOf<s>), s, SdeOpts<T>, Int)
                       -> Tensor<1, Result<SdePath<s>, SdeError>>
```

**Continuous output (`saveat`, paths callable at any `t`).** A numeric SDE path is known only at its nodes. `sde_at(path, t)` gives the state at `t`, `sde_resample(path, ts)` is `saveat` (the value at each time, giving `Tensor<1, s>`), and `sde_interp(path)` returns a callable `Float[T] -> s`. Interpolation is a **linear chord** between nodes: continuous, deterministic, exact at the nodes, and of the same order as the scheme itself (strong order at most 1; a Brownian path is only Hölder-1/2 between nodes anyway); it does **not** restore the Brownian-bridge variance within a step, so statistics between nodes are lower than the true process by `O(dt)` — use `y` directly when exact node distribution statistics matter. Outside the interval is `E0802`, and a `t` whose dimension is not the path's time dimension is `E0401`. The path itself remains **pure data** (no closure fields), so it can be returned across threads; the callable is built separately by `sde_interp`.

**Events.** `sde_events(path, events)` detects sign changes of `g` on the discrete path (at nodes, `g0 < 0 ≤ g1` rising, `g0 > 0 ≥ g1` falling, filtered by `direction`; the same convention as ODE events); the crossing time is the root of `g` on the chord (bisection) and the state is the chord interpolation. It returns `{ path, hits }`: `hits` is sorted by time; if there is a `terminal` event, `path` is cut at the first terminal hit (the hit point becomes the last node and `steps` is updated) and later hits are discarded, otherwise `path` is returned as is. Because only the discrete path is observed, an event of the continuous process that crosses and returns between two nodes is invisible (true of any discrete scheme). A terminal event is a **post-hoc** cut: on the same random stream it gives the same path as stopping at the event, but does not save the work of the discarded part. A `g` returning a dimensioned value is `E0302`, and an out-of-range `direction` is `E0802`.

**Ensembles (parallel, deterministic).** `sde_ensemble_diag/general(drift, diffusion, y0, opts, n)` integrates `n` paths, path `i` using the seed `sde_ensemble_seed(seed, i)` and otherwise the same options; it returns a `Tensor<1, Result<SdePath<s>, SdeError>>` of length `n` (`n ≤ 0` is `E0802`; a failure of an individual member is that member's `Err`). Member `i` depends only on `(seed, i)`, so **the result is independent of execution order and thread count**. Members run in parallel through the share-nothing replicas of §9.7.2 (when there are at least 32 members, drift/diffusion can cross threads and `--jobs 1` is not set), otherwise sequentially on the current thread; both give bit-identical values (`AEVIA_PARALLEL_TRACE=1` reports the fallback). The equivalent hand-written form is `par_tabulate(n, fn i => sdeint_diag(drift, diffusion, y0, <opts with the seed replaced by sde_ensemble_seed(seed, i)>))`.

**Dispatch rule**: **the method name selects the scheme family and integration convention; the structure selects the commutative or non-commutative kernel.** Every method name is therefore usable on every structure:

| method | calculus | commutative (`Diagonal`, `General`) | non-commutative (`GeneralNoncommutative`) | strong/weak |
|---|---|---|---|---|
| `EulerMaruyama` | Itô | Euler–Maruyama | Euler–Maruyama | 1/2, 1 |
| `Milstein` | Itô | Milstein | Milstein + Lévy area (`milstein_noncommutative`) | 1, 1 |
| `MilsteinStratonovich` | Stratonovich | Milstein (Strat.) | Milstein (Strat.) + Lévy area (`milstein_noncommutative_stratonovich`) | 1, 1 |
| `HeunStratonovich` | Stratonovich | Heun | Heun | 1, 1; **1/2, 1 when non-commutative** |
| `SrkPlaten` | Itô | Platen derivative-free SRK | Kloeden–Platen SRK (11.1.7, `srk_noncommutative`) + Lévy area | 1, 1 |
| `MilsteinNoncommutative` | Itô | as the next column (explicit alias, any structure) | Milstein + Lévy area | 1, 1 |
| `MilsteinNoncommutativeStratonovich` | Stratonovich | as the next column (explicit alias, any structure) | Strat. Milstein + Lévy area | 1, 1 |

The kernel documentation states there is no scheme stronger than first order, and the Milstein family is only weak order one, so no weak order two is claimed. `sde_order(method)` gives the orders under commutative noise; `sde_order_with(method, structure)` gives them by structure (`HeunStratonovich` has no Lévy-area term and drops to strong 1/2 on non-commutative noise — the call is allowed but the order is reported honestly). Iterated integrals: Itô `I_jl = ½ΔW_jΔW_l − ½δ_jl h + A_jl`, Stratonovich `J_jl = I_jl + ½δ_jl h = ½ΔW_jΔW_l + A_jl` (`A` is the Lévy area). The SRK support values are `Υ^j = Y + a h + b^j √h` (one per noise channel, `m` extra diffusion evaluations per step), and `Y' = Y + a h + Σ_j b^j ΔW_j + h^(-1/2) Σ_{j,l} (b^l(Υ^j) − b^l(Y)) I_jl` (the Stratonovich version uses `J_jl` and the Stratonovich drift).

**Default `terms` for the Lévy area.** Paths that need the area (`Milstein`/`MilsteinStratonovich`/`SrkPlaten` on non-commutative structure, and the two `*Noncommutative*` methods) take `clamp(ceil(1/√h), 16, 4096)` when `terms == 0`, with `h` the step (the Brownian cell width `(t1 − t0)/cells` for adaptive runs); an explicit `terms > 0` is used as is. `terms` is no longer a required value.

For `General`/`GeneralNoncommutative`, `m` is the number of noise channels; `sdeint_general` checks at run time that the diffusion matrix has shape `n × m` (`E0802`), and `SdePath.noise` reports the actual `m` (`n` for diagonal).

**Record state with general noise**: `sdeint_general` accepts a record state. The diffusion returns a **record of the same shape** in which each scalar field is a `Tensor<1, Float[d/T^(1/2)]>` of `m` coefficients and a field of rank `r` becomes a tensor of rank `r+1` (one extra trailing noise axis `[.., m]`): `GeneralDiffusionOf<{ l: τ, … }> = { l: GeneralDiffusionOf<τ>, … }`. At run time the fields are taken in the **field order of the state record**, depth first and row-major, which is exactly the kernel's `n × m` row-major matrix (a scalar field is one row). Example: for `{ x: Float, v: Tensor<1, Float> }` (`n = 1 + k`) with `General(2)`, the diffusion returns `{ x: [σx1, σx2], v: Tensor<2> [k×2] }`.

**Step policy**: `Fixed` uses a constant step `dt` for `steps` steps (time grid `t0 + k·dt`); `Adaptive` controls the local error with `rtol`/`atol` on `[t0, t1]`, with Brownian-bridge rejection and resampling (in the kernel). **Every method supports adaptive stepping**:

- Itô commutative (`EulerMaruyama`/`Milstein`/`SrkPlaten`): the kernel's adaptive controller returns the Milstein solution (its error estimate is EM-versus-Milstein), so the three give the same controlled path and the method name keeps the requested value;
- **Stratonovich adaptive = exact Itô conversion**: the drift is replaced by the Itô-equivalent drift `ã_i = a_i + ½ Σ_j Σ_k b_kj ∂_k b_ij` (the correction uses central differences along the diffusion columns, with the step scaled like the Milstein-term differences), and the Itô controller then runs on `(ã, b)`. The returned path is the **controlled Itô–Milstein solution of the equivalent Itô SDE**, which converges to the Stratonovich solution of the original coefficients (for `dX = σX∘dW` it converges to `X0·exp(σ W_T)`);
- non-commutative noise (and the two `*Noncommutative*` methods): a fine cell grid plus a controller using the Chen relation (`cells`, 0 = default 4096); Stratonovich methods again convert to Itô first. A single cell that is still rejected is `E0807` and needs more `cells`.

**Determinism**: `seed` is required and has no default. Increments of fixed-step commutative paths come from the language RNG stream (the same LCG as `rng(seed)`, normals by Box–Muller), so `wiener(seed, m, steps, dt)` and a fixed-step path with the same seed consume **the same numbers**; reproducible within a build, with no promise of bitwise equality across platforms. Fixed-step non-commutative paths and all adaptive paths need the kernel's own xoshiro stream (bridge rejection and Lévy areas are sampled during the solve), seeded from `seed`, again reproducible only within a build. `sde_ensemble_seed` depends only on `(seed, index)`, so ensemble members are independent of execution order and thread count; `sde_ensemble_diag/general` run members in parallel at the language level (see above).

**Error table**: `E0802` shape/argument (including a `General` `m` that disagrees with the diffusion matrix, non-uniform grids, missing required fields); `E0806` non-finite state; `E0807` adaptive step collapse or exhausted step budget; `E1001` is reserved for genuinely unimplemented capabilities (`core.sde`, `sde.events`), and no combination of method, structure and step policy produces it today. `SdeError` is an opaque error type (like `SimError`) and the language exposes no constructors.

**Not provided**: user-defined noise structures; explicit non-uniform grids (the fixed-step kernel assumes a uniform step and reports `E0802`; use `Adaptive` for variable steps); solver manifest operators (solvers need callbacks, which cannot cross the manifest ABI); Brownian-bridge dense output (`sde_at` is a linear chord); terminal events that stop **during** integration (`sde_events` cuts after the fact).

> **Status**: all of the above is implemented (`aevia-std::sde`; the `sde_runtime` and `sde_language` tests in `aevia-core`; strong-convergence slopes are measured by `rssn/tests/sde_noncommutative.rs` on a two-dimensional non-commutative linear SDE: > 0.8 for the Milstein family, about 0.5 for Euler–Maruyama). Every method works on every structure; `sdeint_diag`/`sdeint_general`/`wiener`/`sde_order`/`sde_ensemble_seed` are Prelude built-ins re-exported by the `Sde` module, which also declares the option ADTs. In source, write `Sde.Fixed({ structure = Sde.Diagonal, method = Sde.EulerMaruyama, … })` (or `import Sde { Fixed, Diagonal, EulerMaruyama }` and use short names). The option record is read field by field at run time; a missing or ill-typed field gives an `SdeError` with `E0802` and no defaults are filled in (`terms` and `cells` MUST be present, with `0` meaning default). The manifest's `sde_scheme_info(name: String) -> Tensor<1, Real>` returns `[strong_order, ito, commutative_only, adaptive, levy_areas]` (the last four are 1/0 flags); it is a manifest-only metadata query, not a solver path, and gives only the strong order — use `sde_order` for the weak order. `Float[T^(1/2)]` elements of `wiener` print as `s^(1/2)` (§5.4.3).

## 8.10 PDE

PDE solvers take records containing the grid, coefficients and boundary conditions. The signatures are long; here are the entry points:

```
solve_poisson : PoissonSpec -> Result<Field, PdeError>
solve_heat    : HeatSpec -> Result<Tensor<1, Field>, PdeError>
solve_wave    : WaveSpec -> Result<Tensor<1, Field>, PdeError>
solve_advection : AdvectionSpec -> Result<Tensor<1, Field>, PdeError>
```

`Field` is `{ grid: Grid, values: Tensor<n, Float[d]> }`. The dimensions of coefficients and boundary conditions are annotated field by field in the `Spec` records; the solver checks at run time that the grid and coefficient dimensions agree. The full typing rules of PDEs are in the `Pde` module documentation; PDE specifications are not part of the core type system.

## 8.11 Data

```
type Table<ρ>                  # row-polymorphic static schema; ρ is a record type
type Column

read_csv<ρ>   : String -> IO (Result<Table<ρ>, DataError>)
read_csv_dyn  : String -> IO (Result<Table<Dynamic>, DataError>)
write_csv     : (String, Table<ρ>) -> IO (Result<(), DataError>)
read_npy<n>   : String -> IO (Result<Tensor<n, Float>, DataError>)
write_npy     : (String, Tensor<n, Float>) -> IO (Result<(), DataError>)
read_json     : String -> IO (Result<Json, DataError>)
write_json    : (String, Json) -> IO (Result<(), DataError>)

column        : (Table<ρ>, String) -> Result<Tensor<1, ρ[name]>, DataError>
column_dyn    : (Table<ρ>, String) -> Result<Tensor<1, Value>, DataError>
row           : (Table<ρ>, Int) -> Result<ρ, DataError>
select<σ>     : (Table<ρ>, Tensor<1, String>) -> Table<σ>   # σ ⊆ ρ
filter        : ((ρ -> Bool), Table<ρ>) -> Table<ρ>
join          : (Table<ρ>, Table<σ>, String) -> Table<ρ + σ>
schema        : Table<ρ> -> Tensor<1, ColumnInfo>
to_table<ρ>   : Tensor<1, ρ> -> Result<Table<ρ>, DataError>
to_dynamic    : Table<ρ> -> Table<Dynamic>

type ColumnInfo = { name: String, dtype: String, unit: Option<String> }
```

`Table<ρ>` is a **row-polymorphic static schema**: the field names, types and dimensions of `ρ` are known at compile time. `read_csv<ρ>` infers field types from header units; when it cannot, it reports `DataError.Schema` and the user may supply an explicit `ρ`. `read_csv_dyn` returns `Table<Dynamic>` for unknown schemas; `to_table<ρ>` validates and converts at run time.

The field name of `column` MUST be a **string literal**; the type checker lifts the literal to a type-level label, looks up `ρ[name]` in `ρ` and returns its element type. A missing field is `E0306`; a dimension mismatch is `E0401`. For a non-literal use `column_dyn` (returning `Tensor<1, Value>`), which does not use the static schema. `select`/`filter`/`join` stay row-polymorphic; the result row of `join` is `ρ + σ` (`+` is type-level row union; a conflicting field is `E0208`).

The rank of `read_npy` cannot be inferred statically from the file and MUST be written `read_npy<2>(path)`; omitting it is `E0314`. The type argument currently MUST be a concrete natural number; if the file's actual rank differs, the IO action returns `Err(DataError)` and never hands a tensor of the wrong rank to the program.

CSV headers may carry units: `time [s]`, `velocity [m/s]`. `read_csv<ρ>` parses the header units; `column(t, "time")` is checked at compile time against the field types of `ρ` and validated defensively again at run time. Without header units, columns are dimensionless; an explicit `ρ` can override this, but a run-time validation failure is `DataError.DimensionMismatch`.

> **Status**: `read_csv`/`write_csv`/`read_json`/`write_json` and Table/column/select/filter/join are implemented. NPY uses a shared local codec: NPY 1.0/2.0/3.0, little- and big-endian float32/float64, C and Fortran order (Fortran input is converted to row-major storage; output is little-endian float64, C order); `read_npy<n>` stores and verifies a concrete rank, `write_npy` accepts dimensionless `Float` tensors of any rank; an unresolved generic rank is `E0304`. IO error codes: missing file `E0901`, invalid argument `E0902`, corrupt/truncated/rank mismatch/size overflow `E0903`, unsupported format, dtype or extension `E0904`, failed plot write `E0905`; NPY errors are returned as `Err(DataError)` after the IO action runs (`aevia-core/tests/io_taxonomy.rs`).

## 8.12 Plot

```
type PlotSpec
type Series

line      : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Series
line_with : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, PlotOpts) -> Series
scatter   : same as line (default options)
scatter_with : same as line_with (with PlotOpts)
errorbar  : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>,
             Tensor<1, Scalar<ρ,e>>) -> Series
errorbar_with : same as errorbar + PlotOpts
hist      : (Tensor<1, Scalar<ρ,d>>, Int) -> Series
hist_with : (Tensor<1, Scalar<ρ,d>>, Int, PlotOpts) -> Series
heatmap   : (Tensor<2, Scalar<ρ,d>>) -> Series
heatmap_with : (Tensor<2, Scalar<ρ,d>>, PlotOpts) -> Series
label     : (Series, String) -> Series
combine   : Tensor<1, Series> -> PlotSpec
plot      : PlotSpec -> IO Unit
save      : (String, PlotSpec) -> IO Unit

default_plot_opts : PlotOpts
```

`PlotOpts` is a full record: `{ label: Option<String>, color: Option<String>, xunit: Option<String>, yunit: Option<String>, xscale: AxisScale, yscale: AxisScale, xlim: Option<(Float, Float)>, ylim: Option<(Float, Float)> }`. `line(x, y)` uses the defaults and generates axis labels automatically from the dimensions of `x` and `y` (`t / s`, `v / (m/s)`); `line_with` takes a full `PlotOpts`; `label(series, "v(t)")` is the shortcut for setting a label. `plot` writes `plot.html` by default; `save` gives the file name.

> **Status**: there is one renderer, HTML with inline SVG: `plot` and `save` produce the same HTML (containing an `<svg>` element), and `save` does **not** switch format by extension (writing a `.svg` file still produces an HTML document). A write failure is `E0905`; `plot`/`save` remain fatal IO actions (failure exits with a diagnostic and is not a `Result`). Standalone `.svg` output and PNG/JPEG are not implemented.

## 8.13 Test

```
assert      : Bool -> IO Unit
assert_eq   : (Eq a, Show a) => (a, a) -> IO Unit
approx_eq   : (Scalar<ρ,d>, Scalar<ρ,d>, Scalar<ρ,d>) -> Bool
forall      : Arbitrary a => (a -> Prop) -> Bool
compile_fail: String -> Bool
```

`approx_eq(a, b, tol)` requires all three to have the same dimension; `tol` is an absolute tolerance. `forall` generates random samples of an `Arbitrary` type and `decide`s the proposition on each; any `Some(false)` fails, all `Some(true)` passes, and a `None` is `E0804`. `compile_fail` takes a string of source and compiles it in an **isolated** environment (no imports, no calling-module declarations, Prelude only), returning `true` if at least one error results; it does not check the error code — to assert a code, use the doctest `compile_fail EXXXX` marker (§7.5.5/§9.12).

## 8.14 Measurement

```
type Measurement<d> = { value: Real[d], uncertainty: Real[d] }

measure   : (Real[d], Real[d]) -> Measurement<d>
value     : Measurement<d> -> Real[d]
uncertainty : Measurement<d> -> Real[d]
(+) (-)   : (Measurement<d>, Measurement<d>) -> Measurement<d>
(*)       : (Measurement<d>, Measurement<e>) -> Measurement<d*e>
(/)       : (Measurement<d>, Measurement<e>) -> Measurement<d/e>
```

Uncertainty propagates as independent errors (root of the sum of squares). For `a = (v₁, u₁)` and `b = (v₂, u₂)`:

- `a + b` / `a - b`: value `v₁ ± v₂`, uncertainty `√(u₁² + u₂²)`; both sides MUST have the same dimension, otherwise `E0401`.
- `a * b`: value `v₁·v₂`, uncertainty `√((u₁·v₂)² + (u₂·v₁)²)` (dimension `d*e`).
- `a / b`: value `v₁/v₂`, uncertainty `√((u₁/v₂)² + (u₂·v₁/v₂²)²)` (dimension `d/e`).

Correlated errors need a covariance matrix, which the core library does not provide. `Measurement` is a library type: the checker recognises it **structurally** as the plain record `{ value, uncertainty }` (not a new nominal type), so the field names and order are part of the boundary. The entries above are **operators** (overloads), not names, so they are not in the name-coverage audit. The checker (`binary_type`) and the run time (`apply_binary`) both implement them (`aevia-core/tests/measurement.rs`). The implementation also supports unary `-` and "measurement ×/÷ scalar" (the uncertainty is multiplied by `|scalar|`); `Measurement + number` (or `-`) is still `E0302`, and `number / measurement` is also rejected (the hint says to divide by the measurement's value).

## 8.15 Complex

```
complex   : (Real[1], Real[1]) -> Complex[1]
re im     : Complex[d] -> Real[d]
conj      : Complex[d] -> Complex[d]
magnitude : Complex[d] -> Real[d]
arg       : Complex[d] -> Real[1]
complex_sqrt : Complex[d] -> Complex[d^(1/2)]
complex_exp  : Complex[1] -> Complex[1]
complex_ln   : Complex[1] -> Complex[1]
complex_sin cos : Complex[1] -> Complex[1]
```

`abs` is for real numbers only; the modulus of a complex number is `magnitude`, which avoids a clash between `abs : Complex[d] -> Real[d]` and `abs : Scalar<ρ,d> -> Scalar<ρ,d>`. The result dimension of `complex_sqrt` is `d^(1/2)`; in type display and diagnostics, the exponent of a dimension variable keeps its subscript (`:type complex_sqrt` → `Complex[d0] -> Complex[d0^(1/2)]`, never collapsing to `d0^1`).

## 8.16 NumberTheory / Combinatorics / Geometry

```
factor      : Int -> Tensor<1, (Int, Int)>       # (prime, exponent)
is_prime    : Int -> Bool
primes      : Int -> Tensor<1, Int>
binomial    : (Int, Int) -> Int
permutations: Int -> Int
gcd lcm     : (Int, Int) -> Int
polygon_area: Tensor<2, Real[L]> -> Real[L^2]
distance    : (Tensor<1, Real[L]>, Tensor<1, Real[L]>) -> Real[L]
```

These modules map directly onto rssn's rule sets; the full list is in `aevia doc Math.NumberTheory`.

## 8.17 Physics / Simulate

```
simulate : (String, Json) -> Result<Json, SimError>

to_json   : a -> Json                              # dynamic boundary: encodes a representable value as JSON
from_json : Json -> Result<a, SimError>            # dynamic boundary: decodes; a is erased at run time
```

`Json` is an opaque dynamic value type; `to_json`/`from_json` are its only in-language construction and narrowing channels, with the same semantics as `Rssn.typed` + `from_value`: `to_json` does not check `a`, and `from_json` decodes at run time and reports failure as `SimError`. Typed wrappers use them to send record parameters into `simulate` and read results back (rather than making `simulate` itself polymorphic).

Typed wrappers (each is a separate module with top-level declarations and no indentation):

| Module | Parameters | Result | Entry point |
|---|---|---|---|
| `Physics.Ising` | `{ width: Int, height: Int, temperature: Float[1], mc_steps: Int, seed: Option<Int> }` | `{ spins: Tensor<2, Int>, magnetization: Float[1] }` | `run : Params -> Result<IsingResult, SimError>` |
| `Physics.Geodesic` | `{ black_hole_mass: Float[M], initial_state: Tensor<1, Float[1]>, proper_time_end: Float[T], initial_dt: Float[T] }` | `{ orbit: Tensor<2, Float[1]> }` | `run : Params -> Result<GeodesicResult, SimError>` |

The real source looks like `src/Physics/Ising.aev`:

```aevia
module Physics.Ising

type Params = {
    width: Int,
    height: Int,
    temperature: Float[1],
    mc_steps: Int,
    seed: Option<Int>,
}

type IsingResult = {
    spins: Tensor<2, Int>,
    magnetization: Float[1],
}

fn run(p: Params): Result<IsingResult, SimError> = ...
```

`simulate` is the dynamic entry point and takes an rssn scenario name and JSON; the typed wrappers are in `Physics.*` and annotate dimensions field by field. Scenario names follow rssn's live `SCENARIOS` list (29 at the time of writing): `ising`, `ising_wolff`, `geodesic`, `gpe`, `schrodinger`, `fdtd`, `elasticity`, `lid_driven_cavity`, `channel_flow`, `sod_shock_tube`, `kepler_symplectic`, `poisson_multigrid`, `heat_nd`, `wave_nd`, `euler_fv`, `dg_conservation`, `acoustics_pml`, `yee_fdtd`, `navier_stokes_mac`, `split_step`, `fem_simplex`, `elasticity_fem`, `bem_laplace`, `sph`, `dpd`, `md_lj`, `spin_model`, `nbody_gravity`, `kerr_geodesic`. A host MUST NOT keep a second hand-written scenario list; the run time reads `rssn::sim::scenario::SCENARIOS`.

> **Generated simulation modules**: `import Rssn_Sim_<Name>` (e.g. `Rssn_Sim_PoissonMultigrid`, `Rssn_Sim_HeatNd`) generates a typed module on import from rssn's scenario catalogue (`rssn::sim::schema`): the `Params`/`Output` records, one variant type per method-name field (`P.W`, `S.Metropolis`), `defaults()` and `run(p)`. Parameter types and defaults are inferred from the scenario's own request type, the result type comes from a witness run, and an annotation table supplies what inference cannot see; no hand-written code is needed when rssn adds scenarios, parameters or methods. All 29 scenarios can be generated (reference test `tests/sim_catalog.aev`).
>
> **Status**: `simulate`, `to_json`/`from_json` and the `Physics.Ising`/`Physics.Geodesic` wrappers (Aevia source modules, `crates/aevia-std/stdlib/Physics/`) are implemented; `import Physics.Ising` works directly through the embedded module loader.

## 8.18 Device

```
type Device = | Cpu | Gpu(Int) | Distributed(Cluster)
type Cluster                      # opaque run-time topology
type Mesh                         # opaque sharding topology
to_device : (Tensor<n,τ>, Device) -> IO (Tensor<n,τ>)
on        : (Device, () -> a) -> IO a
device_of : Tensor<n,τ> -> Device
```

A device is a run-time value, not a tensor type parameter; without `core.gpu`, `to_device`/`on` report `E1001`. Distributed tensors use the library type `Sharded<Mesh, Tensor<n,τ>>`.

## 8.19 Rssn dynamic interface

```
Rssn.Version       : String
Rssn.ManifestHash  : String
Rssn.Capabilities  : Tensor<1, String>          # names of the capabilities enabled in this build
Rssn.Units         : Tensor<1, String>          # unit names; unit values are exported by the generated Units module
Rssn.Constants     : Tensor<1, String>          # constant symbols; values are exported by the generated Constants module
Rssn.ValueKinds    : Tensor<1, String>          # names of the 17 Value variants

Rssn.call          : (String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.dynamic       : (String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.typed         : (String, Tensor<1, Value>, TypeRep) -> Result<Value, OpError>
Rssn.call_key      : (String, String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.resolve_overload : (String, Tensor<1, Value>) -> Result<String, OpError>
Rssn.overloads     : String -> Tensor<1, String>
to_value           : a -> Value
from_value         : Value -> Result<a, OpError>
type_rep<a>()      : TypeRep
to_callback        : a checked concrete function -> Callback   # compiler intrinsic; any number of function parameters
Rssn.call_ho       : (String, Tensor<1, Value>, Tensor<1, Callback>)
                   -> Result<Value, OpError>
Rssn.call_ho_key   : (String, String, Tensor<1, Value>, Tensor<1, Callback>)
                   -> Result<Value, OpError>
Rssn.register        : Plugin -> Result<PluginId, PluginError>
Rssn.register_plugin : Plugin -> Result<PluginId, PluginError>   # alias
```

`OpKey` is `(rule_set, name, signature_hash)` on the Aevia side: `call_key` takes a name plus a `signature_hash` (32 hex characters; all zeros means the operator has no declared overloads); on success `resolve_overload` returns `"rule_set.name@signature_hash"`; `overloads` returns the rendered signature text. `Rssn.call` is an alias of `Rssn.dynamic` (both take a short name and a `Value` array).

The members of `Rssn` are generated from the Kernel Manifest (rules in §13.2.2): operators with `Declared | Overlay | Derived` signatures get typed declarations (one per overload) that pin the `signature_hash` at generation time; `None` (leaf / non-callable) and plugin operators get `dynamic`; an alias generates `Rssn.<alias>` and on a clash `Rssn.<rule_set>.<name>` is used (a namespace, not a run-time key). Generated declarations call the internal form `Rssn.invoke(name, signature_hash, args)` (it pins the overload too, but the error channel differs: a generated `fn` MUST return its declared type, so a kernel failure raises instead of returning `Err`; it is not a fixed entry point). The size of the manifest is a **measurement**, not a constant: `aevia rssn` gives live numbers (`rssn 3.0.0-rc.1`: 1620 operators, 99 declared + 103 overlay + 1418 derived, 82 units, 30 constants). `Derived` is a conservative signature inferred from registration data that does not distinguish `Tensor`/`Quantity`/`Int`, and IDEs mark it "conservatively inferred" (§13.2.2). The two SDE noise primitives `sde_wiener`/`sde_levy_areas` and the scheme metadata `sde_scheme_info` (`(String) -> Tensor<1, Real>`, giving only the strong order and flags) are typed declarations with exact overlay signatures; the SDE **solve loop** needs host callbacks, is not a manifest operator and does not appear in `Rssn` (§8.9.1).

Reachability and limits of the fixed entry points:

- `type_rep<T>()` makes an unforgeable `TypeRep` from a concrete checked type; an unresolved generic cannot be reflected without a run-time type dictionary. `Rssn.typed` performs the dynamic call and checks the result against that witness; a mismatch or insufficient evidence returns `Err(OpError)`, and a successful result is still an opaque `Value` to be validated and converted with `from_value<T>`.
- `to_callback(f)` checks the concrete function type of a checked source closure in the current evaluator session and builds an opaque `Callback` (aliases and concretised generic functions keep this evidence; wrap built-in functions in a source lambda first). The callback bridge supports concrete numbers, `Bool`, `String`, and tensors and closed records built recursively from them; concrete dimensions, ranks and fully known shapes are checked, and an empty structured tensor needs independent element-type and dimension evidence. Each operator may restrict signatures further; for example the numeric solvers use `Float` scalars and vectors. The call runs synchronously on the original thread and the handle expires when the outer call returns; calls across threads, re-entry in the same scope and callback errors all return errors; an empty callback list is equivalent to a dynamic call.
- `Rssn.register`/`Rssn.register_plugin` accept a host `Plugin`; Aevia source does not hold such values (run-time `E0302`, with a note pointing to the source mechanism). Source plugins are published by declaration (§13.5.2): `@op`/`@rule` functions are registered at module load, and `load_plugin(path) : String -> IO (Result<Int, String>)` loads a plugin file at run time.
- `core.callback` marks the restricted callback bridge as available, and `core.plugin.source` marks source plugins as available. Generated higher-order bindings keep the explicit `(values, callbacks)` dynamic interface; for a single overload with an exact declaration whose callbacks and results can be certified, a typed alias pinned to the signature is also generated, which certifies the concrete callback signature at call time and validates the result against the instantiated type; signatures whose type or dimension cannot be proved offer only the dynamic entry. For example the `num_least_squares` alias of `Rssn_numerical_callback`: parameters and residuals have dimensions `D` and `E`, the residual tolerance uses `E`, the other controls are dimensionless, and the result carries `converged` and `status` (residual/gradient convergence, budget exhausted, stagnation).

`Rssn.dynamic` is a **first-class capability**, not a degraded path: plugin operators, leaf operators and experimental entries all use it. With several registrations rssn chooses at run time by `Exact > Widened > Shaped > Generic > No` and reports `E1002` on ties; an argument accepted by no overload is `OpError::Type` (`check_args`). Each name in the standard set has one registration, so this path does no registration-level scoring, but it **does** validate at the signature level. `Rssn.call_key` does not resolve at all and runs the overload named by name + `signature_hash` directly.

### 8.19.1 The numeric-domain module `Rssn_Numeric`

rssn's `numeric` rule set turns a batch of numeric kernels into operators (table-driven, evaluated only on literal numbers; symbolic arguments, ragged lists, rejected arguments and non-convergence all leave the call as a request, and the wrapper reports `Err(NotReduced(..))`). Aevia files them under `Rssn_Numeric`, where every operator has a hand-written typed signature (`sig` in `domains.overlay`, always `Result<_, OpError>`; rules in §10.3.8). After `import Rssn_Numeric as N`:

| Group | Operators (Aevia signatures) |
|---|---|
| Robust statistics | `mad`, `mad_sigma(xs: Tensor<1, Float>): Float`, `trimmed_mean`, `winsorized_mean(xs, proportion)`, `huber_location(xs, k)`, `biweight_location(xs, c)`, `ecdf(xs, x)`, `kde(xs, x, bandwidth)`, `bandwidth_silverman`, `bandwidth_scott(xs)`, `quantile_hf(xs, p, kind: Int)` (Hyndman–Fan 1–9), `bootstrap_ci(xs, statistic, alpha, resamples, seed): Tensor<1, Float>` (fixed seed, reproducible) |
| Windows and spectra | `hann_window`, `hamming_window`, `blackman_window(n: Int)`, `cross_correlation`, `fftconv(xs, ys)`, `fft_abs`, `fft_re`, `fft_im(xs)`, `ifft_re`, `ifft_im(re, im)`, `frft_re`, `frft_im(re, im, order)` (fractional Fourier), `interpolate(xs, ys, x)` |
| Wavelets | `dwt_approx`, `dwt_detail(xs, wavelet)`, `idwt(approx, detail, wavelet)`, `wavedec(xs, wavelet, levels): Tensor<1, Tensor<1, Float>>`, `waverec(coefficients, wavelet)`, `dwt2(m, wavelet): Tensor<3, Float>` (`[LL, LH, HL, HH]`), `idwt2(ll, lh, hl, hh, wavelet)` |
| Krylov, low rank, ordering | `minres_solve`, `fgmres_solve`, `bicgstab_l_solve(a, b, tol)`, `sparse_solve`, `amg_solve(entries, b, tol, method)` (triples `(i, j, v)`, 0-based), `randomized_singular_values(a, k)`, `lobpcg_eigenvalues(a, k, tol)`, `qr_pivoted_rank`, `qr_pivoted_permutation`, `qr_pivoted_diagonal`, `nested_dissection_order(adjacency)`, `polynomial_system_roots(system): Tensor<3, Float>` (homotopy continuation) |
| Sampled functions | `function_l1_norm`, `function_l2_norm`, `function_inf_norm(points: Tensor<2, Float>)`, `function_inner`, `function_project(f, g)`, `function_normalize(f)`; a function is a table of `(x, y)` rows in ascending order |
| Complex special functions | `lgamma_complex`, `gamma_complex`, `digamma_complex(re, im)`, `lambert_w_complex(re, im, branch: Int)`, `bessel_j_complex`, `bessel_i_complex(order, re, im)`; the result is `[re, im]` |
| Expression operators | `oscillatory_integral` (Levin collocation), `anderson_fixed_point`, `newton_cg_minimize` (symbolic gradient and Hessian), initial-value problems `ode_solve` (`dopri5`, `rosenbrock23`, `radau5`, `bdf`, `sdirk3`, `exprb32`), `ode_symplectic` (Verlet and Yoshida compositions), `ode_imex`, `ode_etdrk4`; the right-hand side and variable are `Real` symbolic terms and the other arguments are numbers |

Conventions and limits:

- **Results are `Float`.** The kernel encodes only floating-point numbers for these operators, so counts and 0-based indices (`qr_pivoted_rank`, `qr_pivoted_permutation`, `nested_dissection_order`) are also returned as `Float`; `from_value<Int>` does not accept them. `Real` (exact) is not a result type.
- **Methods, wavelets and statistics are named by symbols**, which are `Real` in Aevia: `let haar = symbol("haar")`, then `N.dwt_approx(xs, haar)`. **Every** `symbol("haar")` creates a new symbol (`haar#2`, `haar#3`, …) that the kernel does not recognise, so bind each name once with a top-level `let` and reuse it.
- **Dimension-independent, named by dimension count / rank.** `heat_fd`, `wave_fd`, `poisson_fd`, `advect_fd` do not distinguish dimensions in the kernel: the spatial dimension is the nesting depth of the field data (a list is 1-D, a list of lists 2-D, three levels 3-D; the domain is the unit interval/square/cube). An Aevia signature has exactly one literal rank, so one kernel operator carries **several signatures, each with its own name**: the unsuffixed name is the 1-D form (`poisson_fd(f: Tensor<1, Float>, boundary: Float)`), and `_2d`/`_3d` take `Tensor<2, Float>`/`Tensor<3, Float>` fields (`heat_fd_2d`, `wave_fd_3d`, `advect_fd_3d` …) with answers of the same shape as the field; the advection velocity is still a `Tensor<1, Float>` with one component per spatial dimension. Rank-polymorphic tensor operators are named by rank likewise: `tensor_norm_<r>`, `tensor_inner_<r>` (r = 1..4) and `tensordot_<ra>_<rb>_<k>` (contracts k axes, `axes_a`/`axes_b: Tensor<1, Int>` 0-based, result rank ra + rb − 2k, `Float` for 0; result rank at most 4). The decompositions `hosvd_<r>`, `hooi_<r>`, `cp_als_<r>`, `tt_svd_<r>`, `tt_dense_<r>` (r = 2, 3, 4) return the records `Hosvd<r>`, `Hooi<r>`, `CpFit`, `TensorTrain` (factor tables are `Tensor<1, Tensor<2, Float>>`), and mixed lists are taken apart by the list accessor `from_value<Tensor<1, Value>>` and decoded part by part (§10.3.8). Symbolic PDEs have a single name: `solve_heat_equation`, `solve_wave_equation`, `solve_laplace_equation`, `solve_poisson_equation` (the dimension comes from the equation's variables; the old `_1d`/`_2d`/`_3d` are aliases); the affine transforms `translation`, `scaling`, `shear`, `reflection`, `rotation` are the same, filed under `Rssn_Graphics`.
- Ragged inputs (adjacency lists, polynomial systems with different term counts) have no literal syntax; build a `Tensor<1, Tensor<1, Float>>` with `map(f, range(0, n))`.

End-to-end examples and closed-form checks: `tests/domain_numeric.aev` (`mad([1,2,3,4,100]) = 1`, `ifft(fft(x)) = x`, wavelet round trips, `poisson_fd` of `-u'' = 1` giving `x(1-x)/2`, the `exp(-π²t)` decay of `heat_fd`, `tensordot_2_2_1` against a hand-computed matrix product, reconstruction by `hosvd_3`/`tt_svd_3`, `ode_solve` of `y' = -2y` giving `e^-2`, `lambert_w_complex(1,0,0) = 0.5671…`, and others), run with `cargo test -p aevia-rssn --test aev_suites`.

## 8.20 Library organisation

```
Prelude
Math
  Special
  Complex
  NumberTheory
  Combinatorics
  Geometry
Units
  SI
  Imperial
  Astronomy
  CGS
  Natural
  Constants
Tensor
LinearAlgebra
  Sparse
Calculus
Solve
Optimize
Stats
  Distributions
FFT
Interpolate
Ode
Sde
Pde
Data
  Csv
  Npy
  Json
  Table
Plot
Test
Measurement
Rssn
  Dynamic
  Plugin
Device
Physics
  Ising
  Geodesic
  Schrodinger
  ...
```

The standard-library modules live in the `aevia-std` package; the Prelude is imported automatically and the rest on demand with `import`. A module name is its path: `import A.B` resolves `A/B.aev` (§7), `import Math { sqrt }` brings in only `sqrt`, and `open Units.SI` puts the exports of `Units.SI` into the current scope.

> **Status**: the module tree is in place (`crates/aevia-std/stdlib/**.aev`, 41 module files, embedded with `include_str!`; `aevia_std::stdlib` exposes `source`/`is_module_name`/`module_paths`). When the loader cannot resolve a module in the ordinary roots it falls back to the embedded modules, so `import Math { sqrt }`, `open Units.SI`, `import Data.Csv { read_csv, column, Table }` and `import Physics.Ising` all work directly; local files take precedence over embedded modules. Standard-library names are also directly available as Prelude built-ins. `Units/*` are documentation modules: unit names are already ambient `Unit[d]` values, and exporting quantity bindings would shadow them and change the semantics of `Tensor / Unit`, so they are not exported (§5.3.4). The `Sde` module re-exports the five entry points of §8.9.1 and declares the option ADTs; `Physics.Ising`/`Physics.Geodesic` provide the typed wrappers of §8.17. Function names, the nominal variant constructors of `Sde` and transparent type aliases (`type X = { … }`, e.g. `Params` of `Physics.Ising`) can all be brought in with `import { X }`/`open`.
>
> **Conformance audit**: every name declared in this chapter has an implementation (the `aevia-core` test `chapter_eight_has_no_capability_gaps` in `stdlib_coverage`); the `aevia-std` test `spec_signatures` compares each declaration with the checker scheme (presence, arity and return class; parameter types and order are not compared literally because the spec and the schemes spell them differently, and the recorded differences are kept on file). Passing on names does not mean signatures match word for word. All nine complete programs of chapter 11 type-check (`spec_programs`).
>
> **Generated modules and manifest sync**: `Rssn`/`Units`/`Constants` are generated views of the Kernel Manifest. The embedded `Units/Constants.aev` comes from the live manifest; after the manifest changes it MUST be regenerated with `cargo run -p aevia-stdlib-gen -- --write`, otherwise the test `units_constants_is_the_live_manifest` fails.

`Rssn` is a **generated module**, not a hand-written library: at build time `Rssn.<op>` typed declarations and the `Rssn.dynamic` dynamic entry are generated from the Kernel Manifest (§13.2). The content of `Rssn` varies with the rssn version, and Aevia source hard-codes no operator list. `Sparse` provides the format tags `Csr`/`Csc`/`Coo`/`Block`/`Bcsr` and all the sparse built-ins of §8.4.1 (`nnz`/`to_dense`/`from_dense`/`convert`, sparse `matmul`/`transpose`/`kron`, layout dispatch of `@`, `spsolve`/`cg`); `Layout` remains a set of built-in functions rather than an instantiable type class (§13.9.5); `Device` provides `Cpu`/`Gpu`/`Distributed` and `on`/`to_device` (§13.9.4, `E1001` without `core.gpu`).

The next chapter defines the run time: from source to values, from values to rssn terms, from the REPL to the JIT.
