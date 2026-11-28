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

`range(start, stop, step)` produces exact integers from `start` up to the exclusive `stop`.
The default step is `1`. A negative step descends, and a step pointing away from `stop` produces
an empty tensor. A zero step or a result that exceeds the tensor size limit is `E0802`.

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
to_pretty : Show a => a -> String
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
linspace : (Scalar<ρ,d>, Scalar<ρ,d>, Int) -> Tensor<1, Float[d]>
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

`diag(v)` places a numeric or Boolean vector on the diagonal of a square matrix, filling the other entries with zero in the input representation or with `false`. `diag(A)` extracts the main diagonal of a matrix as a vector and keeps its element type. `eye(n)` returns an `n` by `n` floating-point identity matrix. Dense constructors check the result size before allocating. `linspace(start, end, n)` returns `n` floating-point samples with the endpoints’ dimension. For finite inputs it includes both endpoints when `n ≥ 2`, keeps only `start` when `n = 1`, and returns an empty tensor when `n ≤ 0`.

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

**Boundary with the kernel.** rssn's `TypeExpr::Sparse` has only a `rank`, so `Sparse<F, shape, τ>` is lowered at the boundary to rank plus a run-time format; `aevia-rssn`'s `Val::Sparse` corresponds one-to-one to rssn's `Value::Sparse`, **without densifying**. rssn's `term_of_value` does not accept `Value::Sparse` into the term graph (`in_term_graph = false` in `manifest.value_kinds`), so a sparse value cannot be fed directly to general rewriting operators; Aevia converts layouts; RSSN owns sparse algorithms and its `faer` sparse-LU backend. The sparse formats form a closed set (§13.9.5).

Note: `storage.sparse` enables sparse operations; `gmres_with`/`bicgstab_with` accept floating-point elements only; sparse eigenvalues and SVD are not provided.

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
quantiles : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,1>>) -> Tensor<1, Scalar<div_rep(ρ),d>>
var       : Tensor<n, Scalar<ρ,d>> -> Scalar<div_rep(ρ),d^2>
var       : (Tensor<n, Scalar<ρ,d>>, Bool) -> Scalar<div_rep(ρ),d^2>   # population
std       : Tensor<n, Scalar<ρ,d>> -> Scalar<div_rep(ρ),d>
cov       : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Scalar<div_rep(ρ),d*e>
corr      : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Scalar<div_rep(ρ),1>
skewness  : Tensor<1, Scalar<ρ,d>> -> Scalar<ρ,1>
kurtosis  : Tensor<1, Scalar<ρ,d>> -> Scalar<ρ,1>
histogram : (Tensor<1, Scalar<ρ,d>>, Int) -> { counts: Tensor<1, Int>, edges: Tensor<1, Float[d]> }
mode      : Tensor<1, Scalar<ρ,d>> -> Scalar<ρ,d>
```

`quantiles(xs, qs)` computes Hyndman–Fan type-7 quantiles in a batch. Every query in `qs` must be dimensionless and lie in `[0, 1]`. The result keeps the sample dimension, query order, and repeated queries. The batch shares one ordered sample. Exact inputs use rational interpolation. An empty query vector returns an empty result for a nonempty, finite sample.

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

These declarations are the source of the `Stats.Distributions` module (after `open Stats.Distributions`, `Normal({ mu = 1.7 m, sigma = 0.1 m })` is a `Distribution<L>` value). The distribution operations are **built-in overloads** over this closed ADT:

```
pdf       : (Distribution<d>, Real[d]) -> Real[1/d]
cdf       : (Distribution<d>, Real[d]) -> Real[1]
cdf_batch : (Distribution<d>, Tensor<1, Real[d]>) -> Tensor<1, Real[1]>
dist_quantile : (Distribution<d>, Real[1]) -> Real[d]
sample    : Distribution<d> -> IO Real[d]
sample_n  : (Distribution<d>, Int, Option<Int>) -> Tensor<1, Real[d]>
```

The dimension of `pdf` for `Normal<L>` is `1/L`; `cdf` is dimensionless; `sample` returns `Real[L]`. `Weibull` uses its `scale` dimension and `Cauchy` its `loc`/`scale` dimension; `LogNormal` parameters describe the dimensionless logarithm; `NegativeBinomial` and `Geometric` count failures before successes, with support `0, 1, ...`. All fifteen constructors support these operations.

`cdf_batch(dist, xs)` evaluates the same cumulative probability as `cdf` at each point. The result keeps the order and repetitions in `xs`. The points must have the distribution's dimension. A valid distribution and an empty point vector give an empty result. Invalid parameters or a NaN point raise a domain error.

`dist_quantile(dist, p)` returns the quantile at probability `p` in the distribution's dimension. For a discrete distribution and `0 < p < 1`, it returns the smallest support point whose cumulative probability reaches `p`. The probabilities `0` and `1` return the lower and upper support endpoints, which can be infinite. The probability is dimensionless and must be in `[0, 1]`. Invalid distribution parameters, out-of-range probabilities, and NaN probabilities raise `E0806`.

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
interp2Batch : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Tensor<2, Scalar<ρ,f>>,
                Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Tensor<1, Float[f]>
spline   : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Scalar<ρ,d>) -> Scalar<ρ,e>
polyfit  : (Tensor<1, Scalar<ρ,1>>, Tensor<1, Scalar<ρ,e>>, Int) -> Tensor<1, Scalar<ρ,e>>
polyval  : (Tensor<1, Scalar<ρ,e>>, Scalar<ρ,1>) -> Scalar<ρ,e>
chebyshev_fit : (Tensor<1, Scalar<ρ,1>>, Tensor<1, Scalar<ρ,e>>, Int) -> Tensor<1, Scalar<ρ,e>>
```

`fft` is unnormalised and `ifft` divides by `N`, so `ifft(fft(x)) == x`. For `1/√N` or other conventions use `fft_norm(x, Unitary)` (`import FFT { Unitary }`, or write `FFT.Unitary`).

**The transform family.** All forward and inverse transforms accept complex, `Float` or `Int` tensors (real and integer samples are promoted with imaginary part 0); the result is always complex with the same dimension as the samples. A frequency-axis dimension appears only in the results of `fftfreq`/`rfftfreq`, which take the sampling interval `Float[T]` and return `Float[1/T]` (`fftfreq(4, 0.5 s)` is in `Hz`). Multi-dimensional transforms are implemented by row–column decomposition, applying the 1-D transform along each transformed axis in turn; every rank is typed (§8.1.5).

- `fft2`/`ifft2` transform the last two axes; `fftn`/`ifftn` all axes; `fft_axis(t, k)` one axis; `fftn_axes(t, axes)` an arbitrary list. Axes count from 0 and negative values count from the end; an out-of-range or duplicate axis is `E0802`.
- Normalisation uses the total number of transformed points `N` (the product of the transformed axis lengths): `Backward` is 1 forward and `1/N` inverse; `Forward` is `1/N` forward and 1 inverse; `Unitary` is `1/√N` both ways.
- `rfft`/`rfft2`/`rfftn` take real input and keep only the `n/2 + 1` non-redundant frequencies along the last transformed axis. Their inverse forms rebuild the real tensor from its `m` bins along that axis. A single bin defaults to one sample. With more bins, the original length defaults to the even length `2(m − 1)`. Pass an explicit `Int` for an odd length. A length incompatible with `m` is `E0802`.
- `rfftn_axes(t, axes)`/`irfftn_axes(s, axes)`/`irfftn_axes(s, axes, n)` are real transforms over any subset of axes (matching numpy's `rfftn(a, axes=…)`). The **last** axis in the list is the half-spectrum axis and the others get full complex transforms. `irfftn_axes` does the Hermitian reconstruction on the last listed axis, with `n` the original length of that axis and the same default as `irfftn`. An empty list, or an out-of-range or duplicate axis, is `E0802`.
- `fftshift`/`ifftshift` move the zero frequency to the middle (each shifted axis rotates cyclically by `⌊n/2⌋`, the inverse the other way) and work for any element type; the `*_axis` forms shift a single axis.

```
type Norm = | Backward | Forward | Unitary
type AxisScale = | Linear | Log
```

`polyfit` requires a dimensionless independent variable; for a dimensioned one, divide by a unit first (`x/u`); the coefficients keep the dimension of `y`. This is a dimension-system constraint on the API: the coefficient `a_i` of a polynomial has dimension `e/d^i`, different for each power, and cannot sit in a homogeneous tensor.

`interp2` and `interp2Batch` use strictly increasing axes and clamp finite queries to
the grid edges. Store the sample at `(xs[i], ys[j])` in `table[j][i]`.
`interp2Batch(xs, ys, table, query_xs, query_ys)` prepares the grid once. The query
tensors must have equal lengths and match their respective axis dimensions. The
result has one Float per coordinate pair, in input order and with the table's
dimension. Two empty query tensors return an empty result. An empty axis, a grid
shape mismatch, or unequal query lengths give `E0802`. An axis with one knot or
non-increasing knots, or any non-finite input, gives `E0806`.

## 8.9 ODE

```
type OdeMethod =
    | Rk4 | Dopri5 | Radau | Bdf | Rosenbrock23 | AutoSwitch
    | StormerVerlet | Ruth4 | GaussLegendre4 | GaussLegendre6
    | ImplicitMidpoint | AdaptiveRk4 | AdaptiveStormerVerlet | AdaptiveRuth4
    | AdaptiveImplicitMidpoint | AdaptiveGaussLegendre4 | AdaptiveGaussLegendre6
type OdeOpts<s> = {
    rtol: Float[1],
    atol: Float[1],
    method: OdeMethod,
    steps: Option<Int>,                 # Rk4 / 几何积分器：定步长子步数
    events: Option<(Float[T], s) -> Float[1]>,
}
```

```
type OdeAdaptiveOpts<s> = {
    rtol: Float[1],
    atol: Float[1],
    method: OdeMethod,
    steps: Option<Int>,
    events: Option<(Float[T], s) -> Float[1]>,
    h0: Float[T],
    h_min: Float[T],
    h_max: Float[T],
    max_steps: Int,
    max_evaluations: Int,
}

type OdePhysicalOpts<s> = {
    rtol: Float[1],
    atol: s,
    method: OdeMethod,
    steps: Option<Int>,
    events: Option<(Float[T], s) -> Float[1]>,
    h0: Float[T],
    h_min: Float[T],
    h_max: Float[T],
    max_steps: Int,
    max_evaluations: Int,
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

`odeint_with`, `solve_ivp` and `ode_solve` also accept `OdeAdaptiveOpts<s>` or `OdePhysicalOpts<s>`.
`h0 = 0.0 s` selects startup automatically. `h_min`/`h_max` bound adaptive steps; the final clipped step may be smaller.
`max_steps` counts rejected attempts too; `max_evaluations` bounds actual RHS calls. Exhaustion is an error.
Physical `atol` matches every state leaf's structure, tensor shape and dimension, with finite positive components.
Components are normalized to implement `atol_i + rtol * abs(y_i)`; callbacks, events and results retain physical units.
Legacy dimensionless scalar `atol` applies to canonical component values.

Each `Adaptive*` geometric method compares its own full step with two half steps and checks the cubic Hermite
midpoint defect. It accepts the two half steps without switching methods or Richardson extrapolation.
State-dependent adaptive steps generally lose the long-term geometric properties of a fixed-step symplectic map.
Fixed methods use `steps`; implicit-stage failure is an error.

`RateOf<s>` is the built-in recursive type function:

```
RateOf<Scalar<ρ,d>>        = Scalar<ρ,d/T>
RateOf<Tensor<n, τ>>       = Tensor<n, RateOf<τ>>
RateOf<{ l1: τ1, ..., ln: τn }> = { l1: RateOf<τ1>, ..., ln: RateOf<τn> }
```

For `{ x: Float[L], v: Float[L/T] }`, the rate is `{ x: Float[L/T], v: Float[L/T^2] }`.
States contain only `Float` scalars, tensors or nested records; `Int`, `Bool` and function fields give `E0302`.
The RHS takes exactly `(t, s)`, even when time is unused. Imported entry points keep these type-function checks.
The bridge flattens records in declaration order, depth first, and tensors in row-major order, then restores the same
structure and leaf dimensions. A history of record states uses boxed storage.

`StormerVerlet` and `Ruth4` are second- and fourth-order partitioned symplectic methods: the flat state has even length,
with `q` first and `p` second, for a separable canonical Hamiltonian system. `ImplicitMidpoint`, `GaussLegendre4` and
`GaussLegendre6` are implicit Gauss–Legendre methods of orders two, four and six and support general states.
Physical position and momentum can use independently dimensioned record fields. One numeric tensor is homogeneous;
advanced `num_ode_*`, `num_dae_*` and `num_dde_*` tensor APIs require mixed physical components to be normalized.

`odeint` is shorthand for `Dopri5`, `rtol=1e-8`, `atol=1e-10`. `odeint_with` returns the times and states actually reached.
Its optional event stops at a dimensionless sign crossing; fixed-step event accuracy depends on `steps`.

**8.9.0 Solution objects, dense output, multiple events, higher-order equations, boundary-value problems**

Use the following entries for arbitrary-time queries, several directed events or boundary-value problems:

```
type OdeEvent<s> = {
    g: (Float[T], s) -> Float[1],   # 变号即事件；必须返回无量纲数
    terminal: Bool,                 # true：积分在该事件处停止
    direction: Int,                 # -1 只认下降穿越，0 两个方向，1 只认上升穿越
}
type OdeHit<s> = { index: Int, t: Float[T], y: s }
type OdeSolution<s> = {
    t: Tensor<1, Float[T]>, y: Tensor<1, s>,        # 接受的步节点
    at: (Float[T]) -> s,                            # 连续扩展：任意 t 处取值
    events: Tensor<1, OdeHit<s>>,                   # 按时间排序的事件
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

**Dense output.** Integration is forward (`t1 ≥ t0`). `at(t)` is valid on `[t0,t_end]`; an out-of-range query is `E0802`,
and a wrong time dimension is `E0401`. `Dopri5` uses its order-four extension; stiff solvers use their collocation or
backward-difference extensions. `Rk4` uses cubic Hermite interpolation with fourth-order convergence. Its `steps` is the
total over `ode_solve`'s span, versus substeps per output interval in `odeint_with`. An `at` closure cannot cross threads;
return only `t`/`y` from parallel work.

**Events.** `events` may be empty. Rising crossings satisfy `g0 < 0 ≤ g1`, falling crossings `g0 > 0 ≥ g1`;
`direction` is -1/0/1 (`E0802` otherwise). Roots are located on the continuous extension, or by reintegration for `Rk4`.
Hits are sorted by time; a terminal hit truncates nodes, the dense span and later hits. Dimensioned event values give `E0302`.

**Stiff selection.** `AutoSwitch` gives `Dopri5` at most `min(max_steps,20000)` attempts. On exhaustion, remaining attempt
budget runs `Radau` again from the interval start. Both attempts share the callback budget. `method` reports the final
solver. Selection uses step counts, not eigenvalue estimates; known stiff problems can select a stiff solver directly.

**Higher order.** `odeint_order` rewrites order `n ≥ 1` into the stacked state `(y,y',...,y^(n-1))`.
Every derivative keeps the same structure, with dimension divided by `T^k`; `f` returns `L/T^n` when `y` has `L`.
The checker and runtime validate structure/dimensions (`E0302`/`E0401`). Result `d` contains all derivative histories and
`y` the zeroth. `odeint_order_with` requires `events=None`; reduce manually and use `ode_solve` for events.

```
let sol = odeint_order(fn (_t, x, v) => -(k / m) * x, (1.0 m, 0.0 m/s), times)
# x : Float[L]，v : Float[L/T]，返回值 Float[L/T^2]
```

**Single shooting.** `ode_bvp` applies Newton to the initial state, with a forward-difference Jacobian and backtracking,
for at most 60 iterations and `norm_inf(r) ≤ 1e-9 * (1 + norm_inf(y0))`. `bc` returns one residual per component
(`E0802` otherwise). Singular or nonconverged solves give `NotConverged`. It returns the final dense solution.
Unstable sensitivities require multiple shooting or collocation, which this entry does not provide.

**Advanced callbacks.** The following `Rssn_numerical_callback` families use certified callbacks (§8.19).
Time, state and residual dimensions are independent; derivatives use state/time and absolute accuracy uses state units.

| Family | Method and limits |
|---|---|
| `num_ode_{rk4,verlet,ruth4,midpoint,gauss4,gauss6}_adaptive_typed` | Requested-method step doubling, endpoints, Hermite history and explicit work budgets |
| `num_dae_implicit_adaptive_typed` | Residual `F(t,y,y') = 0`; BDF1 startup, variable-step BDF2, Newton and step doubling; algebraic initial state must be consistent |
| `num_dae_index{1,2}_adaptive_typed` | Semi-explicit constraints; positive reference scales for time, state, algebraic variables and constraints |
| `num_dde_constant_adaptive_typed` | Positive-delay method of steps with Dormand–Prince; steps respect the minimum delay and reachable discontinuities |

These entries enforce step bounds and attempt/callback budgets, report rejections and fail instead of forcing acceptance.
DAE/DDE callback entries do not expose events. Pure `Rssn_numerical_vector.num_ode_history_eval` queries physical
state/rate histories within their span. Runnable assertions are in `examples/ode_adaptive_{geometric,dae,dde}.aev`.

## 8.9.1 SDE

SDEs share `RateOf` and the flattening contract with ODEs (§8.9) but add a noise structure and a stochastic-integration convention. The method name itself encodes the convention: a name containing `stratonovich` converges to the Stratonovich solution and the rest are Itô; there is no `calculus` option orthogonal to the method (a scheme whose name does not state its convention is almost always misused).

```
type SdeMethod =
    | EulerMaruyama | Milstein | MilsteinStratonovich
    | HeunStratonovich | SrkPlaten | MilsteinNoncommutative
    | MilsteinNoncommutativeStratonovich
    | DriftImplicitEuler | ThetaMaruyama | DriftImplicitMilstein

type SdeStructure =
    | Diagonal                     # 每个状态分量一个噪声通道，m = n
    | General(Int)                 # (m)；调用者声明噪声可交换
    | GeneralNoncommutative(Int)   # (m)；噪声不可交换：方法名分派到 Lévy 面积内核

type SdeFixedOpts<T> = {
    structure: SdeStructure,
    method: SdeMethod,
    seed: Int,
    t0: Float[T], dt: Float[T], steps: Int,
    terms: Int,                    # Lévy 面积的 Fourier 模数；0 = 默认 clamp(ceil(1/√h), 16, 4096)，h 为步长
}

type SdeAdaptiveOpts<T> = {
    structure: SdeStructure,
    method: SdeMethod,
    seed: Int,
    t0: Float[T], t1: Float[T],
    rtol: Float[1], atol: Float[1],
    max_steps: Int,
    cells: Int,                    # 所有自适应方法的最细噪声 cell；向上取二次幂，0 = 4096
    terms: Int,                    # 同上；h 取 cell 宽度 (t1 - t0)/cells
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
                  # s = { l: τ, … }    : { l: GeneralDiffusionOf<τ>, … }（记录状态，见下）

wiener           : (Int, Int, Int, Float[T]) -> Tensor<2, Float[T^(1/2)]>
                   # (seed, m, steps, dt)；与 manifest 的 sde_wiener(seed, m, steps, h) 同序
sde_order        : SdeMethod -> { strong: Float[1], weak: Float[1] }
                   # 可交换噪声下的阶
sde_order_with   : (SdeMethod, SdeStructure) -> { strong: Float[1], weak: Float[1] }
                   # 按结构给阶：HeunStratonovich 在不可交换噪声上只有强 1/2
sde_ensemble_seed: (Int, Int) -> Int     # (seed, index)，纯函数

type SdeEvent<s> = {
    g: (Float[T], s) -> Float[1],  # 变号即事件，无量纲
    terminal: Bool,                # true：路径在第一个这样的事件处截断
    direction: Int,                # -1 下降，0 两个方向，1 上升
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
| `DriftImplicitEuler` | Itô | Implicit drift, explicit diffusion | Same | 1/2, 1 |
| `ThetaMaruyama` | Itô | theta = 1/2 implicit drift, explicit diffusion | Same | 1/2, 1 |
| `DriftImplicitMilstein` | Itô | Implicit drift, Milstein cross terms | With Lévy areas | 1, 1 |

The kernel documentation states there is no scheme stronger than first order, and the Milstein family is only weak order one, so no weak order two is claimed. `sde_order(method)` gives the orders under commutative noise; `sde_order_with(method, structure)` gives them by structure (`HeunStratonovich` has no Lévy-area term and drops to strong 1/2 on non-commutative noise — the call is allowed but the order is reported honestly). Iterated integrals: Itô `I_jl = ½ΔW_jΔW_l − ½δ_jl h + A_jl`, Stratonovich `J_jl = I_jl + ½δ_jl h = ½ΔW_jΔW_l + A_jl` (`A` is the Lévy area). The SRK support values are `Υ^j = Y + a h + b^j √h` (one per noise channel, `m` extra diffusion evaluations per step), and `Y' = Y + a h + Σ_j b^j ΔW_j + h^(-1/2) Σ_{j,l} (b^l(Υ^j) − b^l(Y)) I_jl` (the Stratonovich version uses `J_jl` and the Stratonovich drift).

**Default `terms` for the Lévy area.** Paths that need the area (`Milstein`/`MilsteinStratonovich`/`SrkPlaten`/`DriftImplicitMilstein` on non-commutative structure, and the two `*Noncommutative*` methods) take `clamp(ceil(1/√h), 16, 4096)` when `terms == 0`, with `h` the step (the Brownian cell width `(t1 − t0)/cells` for adaptive runs); an explicit `terms > 0` is used as is. The `terms` field is required; 0 selects the default.

For `General`/`GeneralNoncommutative`, `m` is the number of noise channels; `sdeint_general` checks at run time that the diffusion matrix has shape `n × m` (`E0802`), and `SdePath.noise` reports the actual `m` (`n` for diagonal).

**Record state with general noise**: `sdeint_general` accepts a record state. The diffusion returns a **record of the same shape** in which each scalar field is a `Tensor<1, Float[d/T^(1/2)]>` of `m` coefficients and a field of rank `r` becomes a tensor of rank `r+1` (one extra trailing noise axis `[.., m]`): `GeneralDiffusionOf<{ l: τ, … }> = { l: GeneralDiffusionOf<τ>, … }`. At run time the fields are taken in the **field order of the state record**, depth first and row-major, which is exactly the kernel's `n × m` row-major matrix (a scalar field is one row). Example: for `{ x: Float, v: Tensor<1, Float> }` (`n = 1 + k`) with `General(2)`, the diffusion returns `{ x: [σx1, σx2], v: Tensor<2> [k×2] }`.

**Adaptive schedule.** `Fixed` keeps its uniform grid. `Adaptive` retains the requested scheme, including all three
implicit-drift schemes. One full step and two half steps use the same realized noise; accepted half-step states are saved.
Euler, SRK and Heun requests are not replaced with Milstein. Every adaptive method uses `cells`: 0 means 4096;
otherwise the count rounds up to a power of two, at least two. Coarse increments sum fixed independent finest increments;
iterated integrals obey Chen's relation. Rejection does not resample realized noise. Failure at the finest coupled interval
is `E0807`; acceptance is never forced.

Brownian defects use a `sqrt(dt/span)` local second-moment allocation; drift defects independently use `dt/span`.
Relative scales use the largest initial/current/trial component magnitude. These are local estimates, not global bounds.
`max_steps` bounds proposals and accepted steps. Allocation/work limits are 1048576 finest cells, 8388608 stored
coefficients/state values and 268435456 Lévy-area updates.

**Jump and stable noise.** Compound Poisson arrivals and marks are sampled once before rejection. Continuous evolution
stops at each arrival and records its post-jump state. Nonconstant compensation needs an expected-jump callback.
Stable coarse increments sum independent finest increments; local moment order is `q = 0.9 alpha`.
For `alpha < 2`, only Euler, drift-implicit Euler and theta apply; Brownian Milstein/Stratonovich corrections do not.
At `alpha = 2`, noise means `sqrt(2) W` and all Brownian schemes apply.

`Rssn_numerical_callback` exposes `num_sde_adaptive_{diag,general}_typed`,
`num_sde_jump_adaptive_{diag,general}_typed` and `num_sde_levy_adaptive_{diag,general}_typed`.
Controls include physical endpoints/reference time, state-unit absolute accuracy, relative accuracy, finest-grid,
proposal and callback budgets, seed and method code (`SdeMethod` declaration order 0–9). General noise adds channel
count and commutativity. Brownian coefficients use state/sqrt(time). Stable noise uses dimensionless
`L_(t/reference_time)`, so its coefficient has state units without floating-point dimensional powers.

**Determinism.** Seeds are explicit. Fixed/adaptive commutative Brownian paths use the language LCG/Box–Muller stream;
iterated-area paths use kernel xoshiro. Seed and finest resolution determine a realization unchanged by rejection.
Ensemble seeds depend only on `(seed,index)`. Reproducibility is within a build. Runnable assertions are in
`examples/sde_adaptive_*.aev`.

**Error table**: `E0802` shape/argument (including a `General` `m` that disagrees with the diffusion matrix, non-uniform grids, missing required fields); `E0806` non-finite state; `E0807` adaptive step collapse or exhausted step budget; `E1001` is reserved for unimplemented capabilities. `SdeError` is an opaque error type (like `SimError`) and the language exposes no constructors.

**Not provided**: user-defined noise structures, explicit non-uniform fixed grids, Brownian-bridge dense output,
or early stopping during standard `sde_events` integration (events truncate afterwards). Callback solvers use §8.19.
The `Sde` module declares option ADTs and re-exports these entries. `terms` and `cells` are required; 0 selects defaults.
`sde_scheme_info` queries metadata only; use `sde_order`/`sde_order_with` for weak order.

Native Rust API reachability is reported separately from manifest coverage (§8.19).

## 8.10 PDE

PDE solvers take records containing the grid, coefficients and boundary conditions. The signatures are long; here are the entry points:

```
solve_poisson : PoissonSpec -> Result<Field, PdeError>
solve_heat    : HeatSpec -> Result<Tensor<1, Field>, PdeError>
solve_wave    : WaveSpec -> Result<Tensor<1, Field>, PdeError>
solve_advection : AdvectionSpec -> Result<Tensor<1, Field>, PdeError>
```

`Field` is `{ grid: Grid, values: Tensor<n, Float[d]> }`. The dimensions of coefficients and boundary conditions are annotated field by field in the `Spec` records; the solver checks at run time that the grid and coefficient dimensions agree. The full typing rules of PDEs are in the `Pde` module documentation; PDE specifications are not part of the core type system.

The short 2D `solve_heat` reads `nx * ny` samples from `initial` in row-major order and holds its four initial edges fixed. It returns `steps + 1` snapshots, including the initial field and the final step. The optional `method` defaults to `"explicit"`, which requires a diffusion number `alpha * dt / h^2` of at most `1/4`. Set `method = "adi"` for alternating-direction implicit stepping at finite diffusion numbers beyond the explicit limit. The implicit step requires finite nonnegative diffusivity and a finite positive time step. Unknown methods and invalid implicit parameters return `Err(PdeError)`.

### 8.10.1 Temporal adaptivity through the manifest

`Rssn_numerical_callback.num_pde_*_adaptive` compares a full step with two half steps on a fixed spatial discretization.
Accepted states use the selected method's half steps without substitution or Richardson extrapolation. Error scaling uses
positive field-unit absolute accuracy and dimensionless relative accuracy. Rejected trials are discarded. Without a terminal
event, integration reaches the exact endpoint. Initial/minimum/maximum steps and accepted/rejected-step, step-kernel and
serial callback budgets are explicit; exhaustion or failure at the minimum step is an error.

| Spatial family | Time methods | Spatial dimensions | Boundaries |
|---|---|---|---|
| Regular differences: heat | Explicit Euler, full backward Euler, full Crank–Nicolson, Douglas ADI | 1–3 | Paired periodic, static Dirichlet/Neumann/Robin; corner data checked |
| Regular differences: wave | Velocity Verlet, the restartable variable-step leapfrog form | 1–3 | Same; displacement and velocity tolerances have distinct units |
| Regular differences: advection | First-order upwind | 1–3 | Same; outward flux and multidimensional CFL bound |
| Simplex FEM: heat/reaction | theta: backward Euler or Crank–Nicolson | 1–3 | Marker boundaries; ABI builds structured P1/P2 meshes; optional nodal source |
| Scalar FVM | Upwind/Rusanov, classical Lax–Friedrichs, MUSCL minmod/van Leer | 1 | Periodic or transmissive; flux/speed callbacks |
| Nodal DG: scalar advection | SSPRK3 | 1–2 | Periodic |
| Fourier pseudospectral | ETDRK4 | 1–3 | Periodic; nonlinear callback returns field/time |

Lax–Friedrichs spatial viscosity depends on the step size, so a fixed mesh may prevent tight temporal accuracy; the solver
fails rather than changing fluxes. FEM source callbacks evaluate `(time, coordinates) -> nodal field/time` serially at both
theta endpoints; their nodal interpolants use the consistent mass matrix. Sources and events share a callback budget.
Multidimensional DG accepts axis element counts, physical box lengths, polynomial degree and per-axis speeds.
Spatial convergence needs a separate mesh study; spatial refinement is not temporal adaptivity. Poisson, static
Helmholtz/FEM and elliptic Chebyshev solves have no time-stepping mode.

Coordinate, time and field dimensions are independent `L`, `T`, `U`. Diffusivity is `L^2/T`, advection/wave speed `L/T`,
reaction `1/T`, and wave velocity `U/T`. Boundary vector fields are `kind`, `value: U`, `flux: U/L`, `alpha: 1`, `beta: L`,
`gamma: U`. Kinds 0–3 mean Dirichlet, Neumann, Robin, periodic. One entry broadcasts to active faces; otherwise provide
two per axis, with paired periodic faces. Robin means `alpha*u + beta*du/dn = gamma`. FVM flux is `U*L/T`, speed `L/T`.
Results carry accepted `times`/`states`, physical `coordinates`, actual work counts, final error ratio and optional terminal
event time; wave results add `velocities`. Assertions are in `examples/pde_adaptive_{heat,wave_event,advanced}.aev`.

## 8.11 Data

```
type Table<ρ>                  # opaque table with a row type parameter

read_csv<ρ>   : String -> IO (Result<Table<ρ>, DataError>)
read_csv_dyn  : String -> IO (Result<Table<Dynamic>, DataError>)
write_csv     : (String, Table<ρ>) -> IO (Result<(), DataError>)
read_npy<n>   : String -> IO (Result<Tensor<n, Float>, DataError>)
write_npy     : (String, Tensor<n, Float>) -> IO (Result<(), DataError>)
read_json     : String -> IO (Result<Json, DataError>)
write_json    : (String, Json) -> IO (Result<(), DataError>)

column        : (Table<ρ>, String) -> Result<Tensor<1, a>, DataError>
column_dyn    : (Table<ρ>, String) -> Result<Tensor<1, Value>, DataError>
row           : (Table<ρ>, Int) -> Result<ρ, DataError>
select        : (Table<ρ>, Tensor<1, String>) -> Table<σ>
filter        : ((ρ -> Bool), Table<ρ>) -> Table<ρ>
join          : (Table<ρ>, Table<σ>, String) -> Table<τ>
schema        : Table<ρ> -> Tensor<1, ColumnInfo>
to_table<ρ>   : Tensor<1, ρ> -> Result<Table<ρ>, DataError>
to_dynamic    : Table<ρ> -> Table<Dynamic>

type ColumnInfo = { name: String, dtype: String, unit: Option<String> }
```

`read_csv_dyn` returns `Table<Dynamic>` for files whose columns are determined at run time. For `read_csv`, an explicit type argument or the surrounding expression supplies the row type parameter. Both readers accept `.csv` files with comma separators and `.tsv` files with tab separators. They validate quoting and row widths. `schema(t)` reports the run-time column names, detected cell types and optional units. A header such as `time [s]` records `s` as column metadata. Numeric cells are exposed as dimensionless `Float` values and other cells as `String` values.

`column` and `column_dyn` look up a string name at run time and return an error for a missing column. The result element type `a` in `column` comes from the surrounding type context independently of the table's row type. `row` returns a record built from the selected row. `filter` preserves the row type parameter. `select` and `join` determine their result columns at run time. For a join, both tables must contain the key column and their other column names must be distinct. `to_table` constructs a table from a vector of records, and `to_dynamic` describes the same table as `Table<Dynamic>`.

Supply a concrete natural number for the `read_npy` rank, as in `read_npy<2>(path)`. Omitting it is `E0314`. If the file has a different rank, the IO action returns `Err(DataError)`.

`read_json` accepts `.json` files. For `read_csv`, `read_csv_dyn` and `read_json`, an unsupported extension produces `Err(DataError)` with `E0904` when the IO action runs.

NPY supports NPY 1.0/2.0/3.0, little- and big-endian float32/float64, C and Fortran order (Fortran input is converted to row-major storage; output is little-endian float64, C order); `read_npy<n>` stores and verifies a concrete rank, `write_npy` accepts dimensionless `Float` tensors of any rank; an unresolved generic rank is `E0304`. IO error codes: missing file `E0901`, invalid argument `E0902`, corrupt/truncated/rank mismatch/size overflow `E0903`, unsupported format, dtype or extension `E0904`, failed plot write `E0905`; NPY errors are returned as `Err(DataError)` after the IO action runs.

The native toolchain enables RSSN's `npy` feature by default. The portable
Aevia `Data` reader and writer are also available in the minimal CLI. Both file
actions require `fs`. For a measured array, record the unit alongside the
array and apply it as an explicit conversion after reading the dimensionless
values.

## 8.12 Plot

```
type PlotSpec
type Series

line          : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Series
line_with     : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, PlotOpts) -> Series
scatter       : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Series
scatter_with  : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, PlotOpts) -> Series
errorbar      : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Tensor<1, Scalar<ρ,e>>) -> Series
errorbar_with : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Tensor<1, Scalar<ρ,e>>, PlotOpts) -> Series
hist          : (Tensor<1, Scalar<ρ,d>>, Int) -> Series
hist_with     : (Tensor<1, Scalar<ρ,d>>, Int, PlotOpts) -> Series
heatmap       : (Tensor<2, Scalar<ρ,d>>) -> Series
heatmap_with  : (Tensor<2, Scalar<ρ,d>>, PlotOpts) -> Series
label         : (Series, String) -> Series
combine       : Tensor<1, Series> -> PlotSpec
plot          : PlotSpec -> IO Unit
save          : (String, PlotSpec) -> IO Unit
plot_save_gif : (String, Tensor<1, PlotSpec>, Int) -> IO Result<Unit, IOError>
default_plot_opts : PlotOpts
```

`line`, `scatter` and `errorbar` take equal-length vectors whose coordinates can be evaluated as real numbers. A pair containing NaN or infinity is omitted. Lines join consecutive surviving pairs, and scatter draws individual markers. Error bars use nonnegative magnitudes with the dimension of `y`. If an error magnitude is non-finite, the central point is drawn without its whisker.

`hist(data, bins)` uses a positive number of equal-width bins and requires at least one finite observation. Non-finite observations are skipped. `heatmap` draws a cell per matrix entry with row zero at the top. Non-finite cells are left blank.

`PlotOpts` is a record with `label`, `color`, `xunit` and `yunit` of type `Option<String>`, `xscale` and `yscale` set to `Linear` or `Log`, and `xlim` and `ylim` of type `Option<(Float, Float)>`. Copy `default_plot_opts` with a record update to set these fields. Import `Linear` and `Log` from `FFT` to set the scales explicitly. Colors accept six-digit RGB hex strings and the names `red`, `blue`, `green`, `orange`, `purple` and `black`. The SVG includes the label as a tooltip and captions each axis with its unit. `label(series, text)` overrides the tooltip text.

Series in a frame share axes and must agree on their coordinate dimensions and any specified axis settings. The default is linear scaling in coherent units with automatic limits. Unit overrides name registered units, including valid SI prefixes. Limits are finite increasing pairs in the displayed units. `Log` is base ten, requires positive limits and omits points with non-positive coordinates on that axis.

Both SVG and GIF use an 800 × 400 canvas with a 720 × 340 plotting area. `plot` writes `plot.html` containing inline SVG. `save` writes standalone SVG for a case-insensitive `.svg` extension and HTML with inline SVG for other paths. Invalid plots and failed writes raise `E0905`.

`plot_save_gif` renders a nonempty vector of plot specifications in order and loops the animation continuously. `Plot.plot_save_gif(path, frames, delay_cs)` takes an explicit delay in hundredths of a second and accepts integers from 0 through 65535. The unqualified call `plot_save_gif(path, frames)` uses a delay of 10. Up to 312 frames fit the raster allocation limit. The IO action returns `Ok(())` or `Err(IOError)`. Invalid frames and delays report `E0902` in the error payload.

## 8.13 Test

```
assert      : Bool -> IO Unit
assert_eq   : (Eq a, Show a) => (a, a) -> IO Unit
approx_eq   : (Scalar<ρ,d>, Scalar<ρ,d>, Scalar<ρ,d>) -> Bool
forall      : (a -> Prop) -> Bool
compile_fail: String -> Bool
```

The arguments to `approx_eq(a, b, tol)` must share a dimension. The tolerance is absolute.

Use `forall` with a unary predicate on a dimensionless `Real`. The samples are `0`, `1`, `-1`, `2`, `-3`, and `1/2`, in that order. If every sample satisfies the predicate, the result is `true`. The first false verdict returns `false`. If there is no false verdict and a verdict is undecidable, the call raises `E0804`. The predicate must return `Prop`, which a `Real` comparison produces. The checker permits a generic parameter type, but the sampler always supplies this same sequence of dimensionless numeric values.

`compile_fail` takes a string of source and compiles it in an **isolated** environment (no imports, no calling-module declarations, Prelude only), returning `true` if at least one error results; it does not check the error code — to assert a code, use the doctest `compile_fail EXXXX` marker (§7.5.5/§9.12).

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
complex   : (Real[d], Real[d]) -> Complex[d]
re im     : Complex[d] -> Real[d]
conj      : Complex[d] -> Complex[d]
magnitude : Complex[d] -> Real[d]
arg       : Complex[d] -> Real[1]
complex_sqrt : Complex[d] -> Complex[d^(1/2)]
complex_exp  : Complex[1] -> Complex[1]
complex_ln   : Complex[1] -> Complex[1]
complex_sin cos : Complex[1] -> Complex[1]
```

`complex(re, im)` requires the real and imaginary parts to have the same dimension. The result keeps that dimension. For example, the magnitude of `complex(3 m, 400 cm)` is `5 m`.

`abs` is for real numbers only; the modulus of a complex number is `magnitude`, which avoids a clash between `abs : Complex[d] -> Real[d]` and `abs : Scalar<ρ,d> -> Scalar<ρ,d>`. The result dimension of `complex_sqrt` is `d^(1/2)`; in type display and diagnostics, the exponent of a dimension variable keeps its subscript (`:type complex_sqrt` → `Complex[d0] -> Complex[d0^(1/2)]`, never collapsing to `d0^1`).

## 8.16 NumberTheory / Combinatorics / Geometry

```
factor      : Int -> Tensor<1, (Int, Int)>       # (prime, exponent)
is_prime    : Int -> Bool
primes      : Int -> Tensor<1, Int>
binomial    : (Int, Int) -> Int
permutations: Int -> Int
gcd lcm     : (Int, Int) -> Int
mod_pow     : (Int, Int, Int) -> Int
polygon_area: Tensor<2, Real[L]> -> Real[L^2]
distance    : (Tensor<1, Real[L]>, Tensor<1, Real[L]>) -> Real[L]
tetrahedron_volume : Tensor<[4, 3], Real[d]> -> Real[d^3]
```

`mod_pow(base, exponent, modulus)` computes the exact residue in `[0, modulus)`
without expanding the full power. It accepts arbitrary-size integers, including
negative bases. Exponent zero yields `1 mod modulus`. A negative exponent or a
nonpositive modulus raises domain error `E0806`.

`polygon_area` takes vertices in boundary order, with two coordinates of the same dimension per row. It returns a nonnegative area in the square of that dimension. A valid matrix of zero, one, or two rows gives zero. A different column count raises shape error `E0802`.

`tetrahedron_volume` takes four rows of three coordinates in a common dimension `d`. It returns the nonnegative volume in `d^3`, independent of vertex order. Coplanar or repeated vertices give zero. A wrong shape raises a shape error. Non-finite coordinates or a non-finite computed volume raise a domain error.

Run `aevia doc Math.NumberTheory` for the module's operators.

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

> **Generated simulation modules**: `import Rssn_Sim_<Name>` (e.g. `Rssn_Sim_PoissonMultigrid`, `Rssn_Sim_HeatNd`) generates a typed module on import from rssn's scenario catalogue (`rssn::sim::schema`): the `Params`/`Output` records, one variant type per method-name field (`P.W`, `S.Metropolis`), `defaults()` and `run(p)`. Parameter types and defaults are inferred from the scenario's own request type, the result type comes from a witness run, and an annotation table supplies what inference cannot see; no hand-written code is needed when rssn adds scenarios, parameters or methods.

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

The members of `Rssn` are generated from the Kernel Manifest (rules in §13.2.2): overloads with exact `Declared`/`Overlay` signatures that source can express and the bridge can authenticate get typed declarations pinned to the generation-time `signature_hash`; `Derived`, `None` and unprovable signatures retain dynamic entry points; an alias generates `Rssn.<alias>` and on a clash `Rssn.<rule_set>.<name>` is used (a namespace, not a run-time key). Generated declarations call the internal form `Rssn.invoke(name, signature_hash, args)` (it pins the overload too, but the error channel differs: a generated `fn` MUST return its declared type, so a kernel failure raises instead of returning `Err`; it is not a fixed entry point). Manifest size is measured by `aevia rssn` (§10.8). `Derived` is a conservative signature inferred from registration data that does not distinguish `Tensor`/`Quantity`/`Int`, and IDEs mark it "conservatively inferred" (§13.2.2). The two SDE noise primitives `sde_wiener`/`sde_levy_areas` and the scheme metadata `sde_scheme_info` (`(String) -> Tensor<1, Real>`, giving only the strong order and flags) are typed declarations with exact overlay signatures; SDE callback solvers use the restricted higher-order ABI (§8.9.1); noise primitives and metadata do not replace solvers.

A public Rust API is not automatically a manifest operator. Concrete numerical functions are bound from source and reviewed
dimension policies; generic callbacks, stateful objects and opaque models need explicit adapters. Manifest reachability and
native source classifications are reported separately; manifest coverage does not establish complete native coverage.

The generator recursively maps a structural variant to `Option<T>` only when it contains exactly a payload-free `None` and a single-payload `Some`; other variants stay dynamic.
Payload types and dimensions are still checked, and an empty `None` supplies no payload-unit evidence. This mapping does not expand the callback bridge's accepted ADT schemas.

Reachability and limits of the fixed entry points:

- `type_rep<T>()` makes an unforgeable `TypeRep` from a concrete checked type; an unresolved generic cannot be reflected without a run-time type dictionary. `Rssn.typed` performs the dynamic call and checks the result against that witness; a mismatch or insufficient evidence returns `Err(OpError)`, and a successful result is still an opaque `Value` to be validated and converted with `from_value<T>`.
- `to_callback(f)` checks the concrete function type of a checked source closure in the current evaluator session and builds an opaque `Callback` (aliases and concretised generic functions keep this evidence; wrap built-in functions in a source lambda first). The callback bridge supports concrete numbers, `Bool`, `String`, the sealed host type `NativeAdVar`, and tensors and closed records built recursively from them; same-named source types and other opaque callback types remain rejected. Concrete dimensions, ranks and fully known shapes are checked, and an empty structured tensor needs independent element-type and dimension evidence. Each operator may restrict signatures further; for example the numeric solvers use `Float` scalars and vectors. The call runs synchronously on the original thread and the handle expires when the outer call returns; calls across threads, re-entry in the same scope and callback errors all return errors; an empty callback list is equivalent to a dynamic call.
  In `Rssn.call_ho` and `Rssn.call_ho_key`, supply ordinary values and callbacks in their respective order in the manifest signature. The bridge combines both lists at the declared parameter positions.
- The host `Plugin` interface is reserved for RSSN hosts. Calling `Rssn.register` or `Rssn.register_plugin` with Aevia values reports `E0302`. Aevia registers `@op` and `@rule` functions at module load. `load_plugin(path) : String -> IO (Result<Int, String>)` loads a local source file under the conditions in §13.5.2 and §13.5.3.
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

End-to-end examples and closed-form checks: `tests/domain_numeric.aev` (`mad([1,2,3,4,100]) = 1`, `ifft(fft(x)) = x`, wavelet round trips, `poisson_fd` of `-u'' = 1` giving `x(1-x)/2`, the `exp(-π²t)` decay of `heat_fd`, `tensordot_2_2_1` against a hand-computed matrix product, reconstruction by `hosvd_3`/`tt_svd_3`, `ode_solve` of `y' = -2y` giving `e^-2`, `lambert_w_complex(1,0,0) = 0.5671…`, and others), run with `bazel run //:aevia -- test --no-cache tests/domain_numeric.aev`.

### 8.19.2 Native numerical models

The following manifest-generated entries call RSSN associated methods or explicit generic specializations. Their numerical algorithms remain in RSSN.
`D` and `E` denote independent element dimensions; `X` denotes the coordinate dimension.

| Entry family | Value and dimension contract |
|---|---|
| `Rssn_numerical_matrix.num_matrix_{float,real}_*` | `Matrix<f64>` / exact rational specializations; records carry a format version, backend and entries. `real` rejects approximate `Float` and uses only the Native backend; `decompose_*` uses Float. `get_mut` and backend updates return new models. |
| `num_dense_lu_model_*`, `num_sparse_{lu,ilu,ic,auto}_model_*`, `num_csr_model_*` | Store defining matrices and factor kinds, reconstructing native factors on use; `A x = b` and preconditioner application return `E/D`. CSR indices, permutations and counts are `Int`, distinct from measured entries. |
| `Rssn_numerical_vector.num_polynomial_model_*` | Descending coefficients share one output dimension and are evaluated at `x/coordinate_scale`. Derivatives and integrals restore `D/X` and `D*X`; binary operations require identical physical coordinate scales. |
| `num_native_cubic_spline_*`, `num_native_pchip_*`, `num_native_cheb_*`, `num_native_aaa_*` | Coordinates and outputs have independent dimensions; a derivative of order `k` has `D/X^k`, an integral has `D*X`, and roots have coordinate units. |
| `Rssn_numerical_callback.num_native_cheb_{from_fn,adaptive}_typed` | Authenticated serial callbacks construct the same Chebyshev model, with explicit degree and evaluation budgets, finite-value checks and original callback errors. |
| `Rssn_numerical_matrix.num_backend_*` | Session-owned backend, compiled-program and scalar handles normalize each coordinate by a positive physical reference, then multiply by an output reference. Interpreter compilation has explicit work bounds; unsupported backends error. Changing the process default requires an explicit selection. |
| `num_ad_*` / `NativeAdVar` | Session-owned reverse AD tapes, variables and gradients use explicit reference scales for heterogeneous coordinates; when output and input dimensions are `E` and `D`, gradients have `E/D`. |

Models validate format/version, fields, shapes, indices, finite data and allocation bounds before invoking native methods. Portable definitions contain no process-local pointers; repeated queries may incur reconstruction cost.
Determinants return `normalized`, `entry_unit` and `order`, representing `normalized * entry_unit^order`. Packed LU stores mixed factors normalized to canonical input units; its whole packed array must not be interpreted as one physical dimension.
Session handles check kind, owning session, lifetime and quotas; release dependent handles before their parents, and do not interchange session handles with portable definitions. Runnable assertions are in `examples/native_algebra_models.aev`; generated manifest signatures and descriptions define the complete entry set and current limits.

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
  Planck
  Atomic
  Geometrized
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
Device
Physics
  Ising
  Geodesic
```

The standard-library modules live in the `aevia-std` package; the Prelude is imported automatically and the rest on demand with `import`. A module name is its path: `import A.B` resolves `A/B.aev` (§7), `import Math { sqrt }` brings in only `sqrt`, and `open Units.SI` puts the exports of `Units.SI` into the current scope. Local files take precedence over embedded standard-library modules. `Units/*` are documentation modules: unit names are already ambient `Unit[d]` values, and exporting quantity bindings would shadow them and change the semantics of `Tensor / Unit`, so they are not exported (§5.3.4). The `Sde` module re-exports the five entry points of §8.9.1 and declares the option ADTs; `Physics.Ising`/`Physics.Geodesic` provide the typed wrappers of §8.17. Function names, the nominal variant constructors of `Sde` and transparent type aliases (`type X = { … }`, e.g. `Params` of `Physics.Ising`) can all be brought in with `import { X }`/`open`.

`Rssn` is a **generated module**, not a hand-written library: at build time `Rssn.<op>` typed declarations and the `Rssn.dynamic` dynamic entry are generated from the Kernel Manifest (§13.2). The content of `Rssn` varies with the rssn version, and Aevia source hard-codes no operator list. `Sparse` provides the format tags `Csr`/`Csc`/`Coo`/`Block`/`Bcsr` and the sparse built-ins of §8.4.1. Layout dispatch is described in §13.9.5. `Device` provides `Cpu`/`Gpu`/`Distributed` and `on`/`to_device` (§13.9.4, `E1001` without `core.gpu`).

The next chapter defines the run time: from source to values, from values to rssn terms, from the REPL to the JIT.
