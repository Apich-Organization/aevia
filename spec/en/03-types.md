# 03 — Type system

Aevia's type system is **HM inference + dimension algebra + type classes + row polymorphism**.
It has no full dependent types and no subtyping; flexibility comes from parametric,
dimension and row polymorphism. This chapter defines the structure of types, the inference
rules and the defaulting policy. Dimension algebra is chapter 04.

## 3.1 Kinds

| Kind | Meaning | Example |
|---|---|---|
| `Type` | value types | `Int`, `Real[L]`, `Tensor<[3,4], Float>` |
| `Dim` | dimension expressions | `L`, `L/T`, `d` |
| `Nat` | type-level naturals | `0`, `2`, `n` |
| `Row` | record rows | the `r` in `{ x: Real[L], ...r }` |
| `Rep` | numeric representation; closed: `Int`, `Real`, `Float`, `Complex` | `ρ` |
| `Format` | sparse format; closed: `Csr`, `Csc`, `Coo`, `Block`, `Bcsr` | `Csr` |
| `Shape` | type-level shapes | `[3,4]`, `[n,m]`, `[?]` |

Users cannot extend `Rep` or `Format`.

## 3.2 Types

### 3.2.1 Scalars `Scalar<ρ, δ>`

All numeric scalars share one form, `Scalar : Rep -> Dim -> Type`, with four surface aliases:

| Written | Internal form | Meaning |
|---|---|---|
| `Int` | `Scalar<Int, 1>` | arbitrary-precision integer, dimensionless |
| `Real[d]` | `Scalar<Real, d>` | exact real or formal symbolic expression |
| `Float[d]` | `Scalar<Float, d>` | IEEE-754 binary64, never symbolic |
| `Complex[d]` | `Scalar<Complex, d>` | complex number (exact/symbolic or `Complex64`) |

`Real`, `Float` and `Complex` without brackets mean `[1]`. An `Int` is always dimensionless: an
exact integer with a dimension is a `Real[L]` whose value happens to be an integer, so `Int`
is safe for indexing and counting. A `Float` is strictly an f64; expressions containing symbols
belong to `Real`, so `Real + Float` is `Real` (the `Float` is converted exactly to a rational
and symbolic information survives: `pi + 1.0` stays `pi + 1`). To stay on the floating path
write `to_float(x)` or make every operand a `Float`.

### 3.2.2 Other type constructors

```
()            : Type                         the unit value
Bool          : Type                         concrete boolean
Prop          : Type                         symbolic proposition (§3.8.6)
String        : Type
Tensor        : Shape -> Type -> Type        tensor (rank or refined shape)
Matrix        : Nat -> Nat -> Type -> Type   = Tensor<[n,m], τ>
Sparse        : Format -> Shape -> Type -> Type   sparse tensor (§13.9.5)
Point         : Dim -> Type                  affine point (Float representation; may be a tensor element)
Unit          : Dim -> Type                  unit value
Table         : Type -> Type                 row-polymorphic table; argument is a closed record type (§13.9.3)
Dynamic       : Type                         unknown schema (only Table<Dynamic>)
Device        : Type                         run-time device (§13.9.4)
Set Option    : Type -> Type
Result        : Type -> Type -> Type
IO            : Type -> Type
(->)          : multi-argument function type (τ1, …, τn) -> τ; () -> τ takes no arguments
SymFun        : Dim -> Dim -> Type           symbolic function (for dsolve)
Dyn           : Rep -> Type                  dynamically dimensioned scalar (§4.9)
RateOf        : Type -> Type                 time-derivative type of an ODE state (§8.9)
DiffusionOf   : Type -> Type                 diffusion-coefficient type of an SDE state (§8.9.1)
Solutions     : Rep -> Dim -> Type           symbolic solution set (§8.6)
```

- `Tensor<0, τ>` is by definition `τ`, so the typing rule for indexing is uniformly "rank
  minus one".
- `Point[d]` has only a `Float` representation; a symbolic temperature uses a `Real[Θ]`
  difference (§5.6).
- `Unit[d]` is the type of unit values (`m : Unit[L]`), not `()`.
- `DiffusionOf<s>` is isomorphic to `RateOf<s>` except that every leaf's dimension is further
  divided by `T^(1/2)` (the Brownian increment `dW` has dimension `T^(1/2)`). A state with
  `Int`, `Bool` or function fields is `E0302`.
- The Prelude and libraries also define `Ordering` (`Less | Equal | Greater`) and the opaque or
  error types `DimValue`, `DimError`, `EvalError`, `MathError`, `DataError`, `LinAlgError`,
  `Record`, `Json`, `Value`, `OpError`, `Plugin`, `PluginId`, `Callback`, `TypeRep` and
  `TermRef` (§4.9, §13.3, §13.5).

### 3.2.3 Records and rows

```
{ x: Real[L], y: Real[L] }              closed record
{ x: Real[L], ...r }                    open record, r : Row
```

A record type is `{ l1: τ1, …, ln: τn | ρ }` where `ρ` is a row variable or the empty row;
field order is irrelevant.

### 3.2.4 Variants

Variants are nominal: `type Shape = | Circle(Real[L]) | Rect(Real[L], Real[L])` is not equal to
any structurally identical record or variant. Constructors may take type parameters:
`type Option<a> = | None | Some(a)`.

### 3.2.5 Type schemes

`σ ::= ∀a:κ. σ | C => σ | τ`. `∀` quantifies type, dimension, natural, row and representation
variables. `C` includes class constraints (`Show a`) and numeric constraints: the
representation domain of a literal, `Join(ρ1, ρ2, ρout)`, `Divide(ρin, ρout)` and the
input-dependent private-literal reading rules. Constraints are stored with schemes, function
aliases and module interfaces and are rebuilt by the compiler; users never write them.
Instantiation renames the variables in constraints too. Intermediate representations produced
by operations are instantiated independently but do not become explicitly selectable `<…>`
parameters.

## 3.3 Environments and inference

### 3.3.1 Environment

`Γ ::= ∅ | Γ, x : σ | Γ, α:κ | Γ, C`. Top-level declarations are checked by first collecting
signatures and then checking bodies, so declarations in a module may refer to one another
(recursion included).

### 3.3.2 Bidirectional checking

- **Synthesis** `Γ ⊢ e ⇒ τ` infers a type from an expression; **checking** `Γ ⊢ e ⇐ τ` checks
  against a known expected type.
- A literal synthesises a fresh type with a literal constraint; a variable instantiates its
  scheme; a call synthesises the function type and unifies it with the arguments; a lambda
  checks its parameters against an expected type, or gives them fresh type variables; `let`
  infers the right-hand side and then generalises, `let rec` first binds a fresh type; `if`
  requires a `Bool` condition and unifies the branches; `match` checks patterns against the
  scrutinee type and unifies the arm bodies; record and tensor literals unify field by field
  and element by element; an annotation `(e : τ)` uses checking mode.

### 3.3.3 Generalisation and weak variables

At `let` and top-level declarations, type, dimension and row variables that do not occur free in
`Γ` are universally quantified together with their class constraints:

```
let id = fn (x) => x
# id : ∀a. a -> a
let mean = fn (xs: Tensor<1, Real[d]>) => sum(xs) / length(xs)
# mean : ∀d. Tensor<1, Real[d]> -> Real[d]
```

**Weak variables** are not generalised. They serve values whose name must have one dimension at
every use:

```
let x = symbol("x")        # x : Real[d], d is a weak variable
let a = x + 3 m            # d unifies with L
let b = x + 2 s            # E0401: x is already L
```

A weak variable still unresolved at the end of its declaration defaults to `1` with warning
`W0301`. Generalising to `∀d. Real[d]` would let one symbol be taken for different dimensions at
different places, so it is not done.

- `symbol : String -> Real[d]` and `symfun : String -> SymFun<d,e>` are built-in primitives.
  Each **syntactic call** allocates fresh weak variables; when a call runs repeatedly in a loop
  or function the runtime creates new symbols but the type remains the same weak variable.
- `let f = symbol` passes `symbol` as a value: every call of `f` then shares one dimension (a
  symbol factory has one dimension). Weak variables carry a level; generalisation excludes weak
  variables and those reachable from their substitutions, so they cannot escape polymorphically
  through a closure: in `let mk = fn s => symbol(s)` the `d` stays weak.
- An empty tensor `[]` introduces no weak variable and needs an expected type (`E0314`).
- Aevia's `Symbol` is `Real[δ]` with `δ` weak; there is no fifth kind of value. When the `Rssn`
  façade is generated, a `Symbol` parameter in a signature reads as a fresh weak `Real[δ]` and a
  `Real[d]` parameter as `Dim(Real, d)`. So the signature `(Term, Symbol) -> Term` of `diff`
  reads `(Dyn<term>, Real[δ]) -> Dyn<term>`, with the weak variable unified at the call site.

### 3.3.4 The value restriction

A pure language has no mutable references and needs no value restriction: any expression may be
generalised. Weak variables are not a value restriction; they enforce the identity of a name.

## 3.4 The numeric tower and promotion

### 3.4.1 Promotion lattice

```
Int < Float < Complex        Int < Real < Complex
```

`Float` and `Real` are incomparable, and `join` picks `Real` when they meet (exact wins). An
`Int` promotes to any other representation; a `Float` promotes exactly to `Real` (every f64 is a
rational, though numerator and denominator may be large) or to `Complex`; a `Real` promotes to
`Complex`; a `Complex` never demotes. `Real → Float` happens only through an explicit
`to_float` and fails if free symbols remain. Promotion happens only when an operation needs it.

```
join(Int, Int) = Int       join(Int, Real) = Real       join(Int, Float) = Float
join(Real, Real) = Real    join(Real, Float) = Real     join(Float, Float) = Float
join(_, Complex) = Complex
```

An unresolved representation keeps a finite arithmetic relation instead of being identified with
one input: `Join(ρ1, ρ2, ρout)` constrains the three by the table above and `Divide(ρin, ρout)`
constrains the promotion of integer division. An ordinary lattice bound cannot replace them.

```aevia
fn half<r: Rep>(x: r) = x / 2
let exact = half<Int>(3)        # Real, exactly 3/2
let floating = half<Float>(3.0) # Float
```

`half` has the single explicit parameter `r`; a caller cannot pick a result representation that
contradicts the division rule. The relations are kept both at check time and in the run-time
generic proof of a closure.

### 3.4.2 Arithmetic typing rules

Let `ρ = join(ρ1, ρ2)` and `ρ' = div_rep(ρ)` (`div_rep(Int) = Real`, otherwise unchanged):

| Expression | Operands | Result |
|---|---|---|
| `a + b`, `a - b`, `min(a,b)` | `Scalar<ρ1,d>`, `Scalar<ρ2,d>` | `Scalar<ρ,d>` |
| `a * b` | `Scalar<ρ1,d>`, `Scalar<ρ2,e>` | `Scalar<ρ,d*e>` |
| `a / b` | same | `Scalar<ρ',d/e>` |
| `a ^ n` | `Scalar<ρ,d>`, integer literal | `Scalar<ρ',d^n>` |
| `a ^ q` | `Scalar<ρ,d>`, rational literal | `Scalar<ρ,d^q>` |
| `-a`, `abs(a)` | `Scalar<ρ,d>` | `Scalar<ρ,d>` |
| `sign(a)` | `Scalar<ρ,d>` | `Int` |
| `clamp(x,lo,hi)` | all of dimension `d` | `Scalar<ρ,d>` |

Division, negative exponents, roots and inversion promote integers to exact rationals:
`Int / Int` is `Real[1]` (`7 / 2` is exactly `7/2` and cannot be annotated `Int`); integer
division is `div(a,b)` or `quot(a,b)`; `2^-1` is the `Real` `1/2`. `min`, `max`, `mod` and `rem`
require `ρ ≠ Complex`; the modulus of a complex number is `magnitude` (§8.15), not `abs`.

### 3.4.3 Static limits on powers

When the exponent is a compile-time integer or rational, the exponent is computed in the type:
`(3 m)^2 : Real[L^2]`, `sqrt(4 m^2) : Real[L]`, `(2 m)^(1/2) : Real[L^(1/2)]`. When the exponent
is a run-time `Int`:

- A dimensionless base (`Scalar<ρ,1>`) is allowed; the result is dimensionless.
- A base whose dimension `δ` is still an unsolved inference variable (such as `t` in
  `diff(fn t => t^t)`): a run-time exponent requires a dimensionless base, which is a constraint
  and not an error, so the checker sets `δ = 1` — the same rule by which `sin(t)` makes `t`
  dimensionless.
- A base whose dimension is known and is not `1`: `E0404`. Use `pow_dyn(a, n) : Dyn<ρ>`
  (§4.9), or write `exp(n * ln(a / unit))`. In a generic `fn f<d>(x: Real[d], n: Int)`, `x^n`
  is rejected because `d` is not constrained to `1`.

This is the edge of a static dimension system: the exponent of `d^n` is a run-time value the type
level cannot represent, and `Dyn` is the explicit exit.

### 3.4.4 Transcendental functions

| Function | Argument | Result |
|---|---|---|
| `sin cos tan asin acos atan`, `sinh … atanh`, `exp ln log log2 log10` | `Scalar<ρ,1>` | `Scalar<ρ,1>` |
| `atan2(y,x)` | same `d` | `Scalar<ρ,1>` |
| `hypot(x,y)` | same `d` | `Scalar<ρ,d>` |
| `sqrt(x)` / `cbrt(x)` | `Scalar<ρ,d>`, `ρ ∈ {Real,Float}` | `d^(1/2)` / `d^(1/3)` |
| `floor ceil round trunc`, `mod(a,b)` | `Scalar<ρ,d>` (`mod`: both `d`) | `Scalar<ρ,d>` |

`sin(3 m)` is `E0403`; `sin(30 deg)` is valid because `deg` is dimensionless (§5.5).

## 3.5 Tensor types

### 3.5.1 Rank and shape

```
Tensor<n, τ>                 # rank only; the shape is a run-time value
Tensor<[n1, …, nk], τ>       # refined shape
Matrix<n, m, τ>              # = Tensor<[n, m], τ>
```

- `Tensor<n, τ>` is sugar for `Tensor<[?, …, ?], τ>` (`n` question marks). A rank error is a
  compile-time error; a shape error is a compile-time error under a refined type and a run-time
  `E0802` otherwise. A rank `n` MUST be a concrete integer (`E0304`).
- The extents of a refined shape are type-level `Nat`s (literals, variables, `+`, multiplication
  by a literal, or `?`). Literals acquire a refined shape automatically, and constructors such as
  `zeros_of<[3,4]>()` can state one explicitly. The shape argument of `Tensor` is a special type
  form, not an ordinary type parameter.
- Refined → rank-only is a safe implicit conversion. The reverse uses
  `refine_shape(a) : Option<Tensor<[…],τ>>`, a run-time check; annotating directly is `E0302`.
  To branch on shape use `refine_shape<[3, ?]>(a)` (§8.4).
- `reshape<[n1, …, nk]>(a)` always returns `Result<Tensor<shape,τ>, ShapeError>`: when the shape
  is statically known the element count is checked at compile time and the result is always
  `Ok`; otherwise it is checked at run time. `reshape_dyn` always checks at run time.
- Indexing: a refined shape with a literal subscript is checked at compile time against
  `-n_i <= i < n_i`; a dynamic subscript is checked at run time.

**Nat equations.** Unification treats equations as polynomials:

- With one variable the equation is solved over non-negative integers: `n + 1 ~ 3` ⇒ `n = 2`;
  `2·n ~ 6` ⇒ `n = 3`; `n ~ 2·n` ⇒ `n = 0`. An equation in several variables is kept as a
  pending constraint and retried after later bindings; a non-zero constant residue is `E0302`.
- A product of variables (`n*m`) is an opaque monomial and cancels only against the same
  product (`n*m ~ m*n`); once one factor is solved it is substituted back into a linear
  equation. A product that stays unsolved is reported at the end of the declaration as `E0304`,
  with the equation. A shape variable without a binder, and a Nat variable in an instance head,
  are also `E0304`.
- Variables **declared** in `fn f<n, m>` are rigid: a literal or expression may solve them, but
  two distinct declared variables are never merged, and `n ~ m` is `E0302`. Variables created
  by instantiating a generic callee are inference variables and may cancel against any
  expression, so a generic function with symbolic extents may call another generic function.

**Broadcasting.** Element-wise operations act per axis: extents are equal, or one side is the
literal `1` (the result takes the other side). `[3,1] + [1,4]` is `[3,4]` and `[n,1] + [1,m]` is
`[n,m]`; two symbolic extents that are not `1` are unified; a rank-only side counts as unknown
on every axis. **Tensors of different rank do not broadcast** (`E0302`, with a hint to build the
larger shape explicitly with `map` or `outer`), so the result rank is always statically known
and silent errors such as `(3,1) + (1,4)` cannot occur.

Shape refinement catches `3×4` versus `4×3`, matrix-product dimensions and reshape element
counts without full dependent types, while `Tensor<n,τ>` remains the erased form for "shape
unknown / from a CSV". It has only linear `Nat` arithmetic, no general recursion and no proof
terms.

### 3.5.2 Literals

```
[1, 2, 3]                 : Tensor<[3], Int>
[[1.0, 2.0], [3.0, 4.0]]  : Tensor<[2, 2], Float>
[1, 2.0]                  : Tensor<[2], Float>      # promotion
[]                        : needs an annotation
```

Nesting depth fixes the rank, sibling lengths MUST be equal (`E0203`), element types are unified,
and an empty literal needs an expected type (`E0314`).

### 3.5.3 Element-wise operations

For `Tensor<n, Scalar<ρ1,d>>` and `Tensor<n, Scalar<ρ2,e>>`: `+` and `-` require `d = e` and give
`Scalar<join,d>`; `*` and `/` give `d*e` and `d/e`; `a ^ n` (literal `n`) gives `d^n`; unary `-`
and `abs` preserve the type. Mixing scalars or unit values with tensors:

```
Scalar<ρ1,d> * Tensor<n, Scalar<ρ2,e>> : Tensor<n, Scalar<join,d*e>>
Scalar<ρ1,d> + Tensor<n, Scalar<ρ2,e>> : Tensor<n, Scalar<join,d>>, requires d=e
Tensor<shape, Scalar<ρ,d>> * Unit[e]   : Tensor<shape, Scalar<ρ,d*e>>
Tensor<shape, Scalar<ρ,d>> / Unit[e]   : Tensor<shape, Scalar<ρ,d/e>>
```

A refined and a rank-only shape of the same rank unify in operations (refined → rank-only is an
implicit conversion).

Element rules for `Tensor<shape, Point[d]>`: `Point - Point : Float[d]`; `Point + Float[d] :
Point`; `Point + Point` is `E0405`. `mean` accepts numeric tensors only; average points with
`centroid : Tensor<1, Point[d]> -> Point[d]`.

### 3.5.3.1 Rank dispatch of `@`

`@` dispatches on rank (same precedence as `*`, left-associative):

| Left | Right | Result | Function |
|---|---|---|---|
| `Tensor<2, ·d>` | `Tensor<2, ·e>` | `Tensor<2, ·d*e>` | `matmul` |
| `Tensor<2, ·d>` | `Tensor<1, ·e>` | `Tensor<1, ·d*e>` | `matvec` |
| `Tensor<1, ·d>` | `Tensor<2, ·e>` | `Tensor<1, ·d*e>` | `vecmat` |
| `Tensor<1, ·d>` | `Tensor<1, ·e>` | `Scalar<ρ,d*e>` | `dot` |

Here `·d` stands for `Scalar<ρ,d>`. Under refined shapes `[m,k] @ [k,n] → [m,n]`,
`[m,k] @ [k] → [m]` and `[k] @ [k,n] → [n]`; `m`, `k` and `n` are checked at compile time, and
rank-only operands at run time. Sparse matrices use `Sparse<F, shape, τ>`; dense/sparse mixes
dispatch on layout (§13.9.5) and never convert implicitly.

### 3.5.4 Indexing

In `a : Tensor<n, τ>` the rank `n` MUST be a known concrete natural. Each **scalar subscript**
removes an axis and each **slice** (`..`, `i..j`, `i:j:s`) keeps one:

```
a[i]          : Tensor<n-1, τ>      requires n >= 1
a[i, j]       : Tensor<n-2, τ>      same as a[i][j]
a[i, ..]      : Tensor<n-1, τ>
a[.., j]      : Tensor<n-1, τ>
a[i..j, ..]   : Tensor<n, τ>
a[i..j, j]    : Tensor<n-1, τ>
```

Rank-polymorphic code uses `map`, `fold` and `sum` instead of subscripts. An out-of-bounds
subscript is a run-time `E0802`; literal subscripts may be checked statically.

### 3.5.5 Dimension rules for reductions and linear algebra

Full signatures are in §8.4; only the dimension and representation rules are listed here:

| Function | Result |
|---|---|
| `sum`, `min`, `max`, `norm`, `trace` | `Scalar<ρ,d>` |
| `sum(a, axis)` | same `d`, rank kept (keepdims) |
| `mean`, `std` | `Scalar<div_rep(ρ),d>` |
| `var` | `Scalar<div_rep(ρ),d^2>` |
| `normalize` | dimensionless elements |
| `dot`, `matmul`, `matvec`, `outer` | `d*e` |
| `det`, `prod` | accept **dimensionless** elements only |
| `inv` | `Result<Tensor<2, Scalar<div_rep(ρ),1/d>>, LinAlgError>` |
| `linsolve(A, b)` | `Result<Tensor<1, Scalar<div_rep(ρ),e/d>>, LinAlgError>` |
| `eig` | eigenvalues of dimension `d`, dimensionless eigenvectors |
| `svd`, `qr`, `lu` | singular values, `R` and `U` have dimension `d`; the other factors are dimensionless |

The dimension of `det` is `d^n` with `n` a run-time shape, so `det` accepts only dimensionless
matrices; a dimensioned determinant uses `det_unit(a, u) : Dyn<ρ>` (§4.9). `prod` is the same.

## 3.6 Records, rows and variants

### 3.6.1 Row unification

```
{ l: τ1 | ρ1 } ~ { l: τ2 | ρ2 }   ⇒  τ1 ~ τ2, ρ1 ~ ρ2
{ l: τ | ρ } ~ { l1: τ1, …, ln: τn | ρ' }
   ⇒ l matches some li, or l is absent and ρ absorbs the remaining fields
```

Row unification follows Rémy's algorithm and carries **lacks constraints**: when `ρ lacks l`, `ρ`
may absorb a field only if it does not contain `l`, otherwise duplicate labels would arise. A
missing field is `E0306`; a duplicate field is `E0208`. A record literal has a closed row, while
a record annotation on a function parameter may be open:

```aevia
fn norm(p: { x: Real[L], y: Real[L], ...r }): Real[L] =
    sqrt(p.x^2 + p.y^2)
```

`norm({ x = 1 m, y = 2 m, z = 3 m })` is valid and binds `r` to `{ z: Real[L] }`.

### 3.6.2 Record update

`{ p with x = 3.0 m }` requires `p` to have field `x`; the result has the type of `p` with `x`
replaced by the new type. If the field does not exist it is added, which requires
`row(p) lacks z` (otherwise `E0208`). Updates are pure and the old record is unchanged; the
runtime updates in place when the record is uniquely owned.

### 3.6.3 Variants and exhaustiveness

A constructor is a function or a value: `Circle : Real[L] -> Shape`, `None : Option<a>`,
`Some : a -> Option<a>`. A variant `match` MUST cover all constructors or end with `_`; record
patterns are open; a redundant arm warns `W0201`. A constructor pattern with the wrong arity is
`E0307`; a constructor call with the wrong number of arguments is `E0315`.

### 3.6.4 Positivity of recursive types

Recursive types are allowed (`type Tree<a> = | Leaf | Node(Tree<a>, a, Tree<a>)`), but a
recursive occurrence may not be to the left of a function arrow: `type Bad = | B(Bad -> Int)`
is `E0312`. Aevia proves nothing, but positivity keeps the construction of run-time values
well-founded.

## 3.7 Functions, closures and tail calls

### 3.7.1 Function types

`(τ1, …, τn) -> τ`: arguments are passed by value, functions are closures and may be passed and
returned. Function equality is undecidable, so `==` does not apply to functions (`E0310`: no
`Eq` instance).

### 3.7.2 Multiple arguments

Functions take several arguments and are not curried automatically:

```aevia
fn add(x: Int, y: Int): Int = x + y
add(1, 2)          # 3
add(1)             # E0315: wrong number of arguments
add(1, _)          # fn y => add(1, y)
```

Use `curry`/`uncurry` (§8.3) when currying is needed; the `_` placeholder is lightweight partial
application.

### 3.7.3 Tail calls

- A self-recursive tail call MUST run in constant stack space; tail calls in mutual recursion
  are optimised as well.
- Non-tail recursion is limited by the run-time stack; exceeding it is `E0805` (stack overflow),
  never a silent crash.

```aevia
fn sum_to(n: Int, acc: Int): Int =
    if n == 0 then acc else sum_to(n - 1, acc + n)
```

### 3.7.4 Recursion and effects

An `IO` action may be recursive, but only through a named function: a `do` block cannot declare
`fn` or `let rec` (§2.3.12).

## 3.8 Type classes

### 3.8.1 Declarations and instances

```aevia
class Show a where
    show(x: a): String

instance Show for Real[d] where
    show(x) = format_quantity(x)
```

- A class has **exactly one** type parameter; `class Convert a b` is `E0317`.
- Methods have signatures only; defaults come from `derive`, built-in structural
  implementations (Prelude classes) or plugins.
- An instance head is a type-constructor application; a bare type-variable head
  `instance C for a` is `E0317`. Variables in the head are implicitly universally quantified.
- An instance of a user class MUST define every method; the missing ones are listed in the
  `E0317`. Prelude classes are **hooks**: an instance may cover only some methods and the
  built-in structural implementation supplies the rest (such as printing records and tuples).
- Class and instance identity is symbolic: a class key is `(defining module path, class name)`
  and an instance key adds the canonical head type. An `AdtId` is only an in-process witness and
  never enters an interface or cache. A `private class` is visible only in its defining module;
  a `private instance` is not allowed (instances are exported with the module, §7.4).

### 3.8.2 Resolution

A constraint `C τ` is resolved as follows: when `τ` is a concrete type constructor, the unique
matching instance is looked up; when `τ` contains unresolved variables, resolution is deferred;
if it is still unresolved at the end of the declaration, the error is `E0310` (no instance) or
`E0304` (ambiguous). Instance matching is head matching without backtracking; overlapping
instances are `E0311`.

### 3.8.3 Coherence and the orphan rule

One class and one type head have at most one instance. An instance MUST be defined in the module
that owns the type or the module that owns the class (the orphan rule), so users cannot add
`Show` to `Int`. This makes the set of instances deterministic under separate compilation.

### 3.8.4 Built-in classes

| Class | Methods | Notes |
|---|---|---|
| `Eq a` | `==` `!=` | concrete structural equality; no instance for functions or `Real` |
| `Ord a` | `compare`, `<`, … | partial order; `compare : Option<Ordering>` (`None` for NaN) |
| `TotalOrd a` | `total_compare` | total order required by `Set`, `Map`, `sort`; none for `Float` |
| `Show a` | `show` | readable form; used by string interpolation |
| `Hash a` | `hash` | hashed containers |
| `Num a` `Fractional a` `Floating a` | arithmetic, `from_int`, `sqrt`, … | **dimensionless** user numeric types only |
| `Container f` | `map`, `fold` | tensors, `Option`, `Set` |
| `Sized a` | `length` | `String`, `Tensor` |
| `Arbitrary a` | `arbitrary` | random generator for property tests |
| `Parallel a` | `par_map` | only `Int`/`Float[d]` and their tensors; symbolic `Real` is not parallelised |

- `Num` and friends do not apply to dimensioned scalars, because `*` changes the dimension and
  so does not satisfy `a -> a -> a`; dimensioned arithmetic is a built-in language rule.
- `Real` is not `Eq`: `x == y` returns `Prop` (§3.8.6); use `decide(x == y)` or `approx_eq` for
  concrete equality. `==` on `Real` resolves to `sym_eq`, not through `Eq`, so generic code with
  an `Eq a` constraint is never instantiated at `Real`.
- `Float` is `Eq` (`NaN != NaN`) but not `TotalOrd`, so `Set<Float>` is invalid; sort with
  `sort_float` (NaN last). `Set<a>` and `Map<a,b>` require `TotalOrd a`.
- `solve` returns `Solutions<Real,d>` (§8.6), not a `Set` (symbolic roots have no total order).

Standard-library instance heads for `Show`, `Eq`, `Ord` and `Hash` MUST use the dimension-
polymorphic form (`Float[d]`), otherwise `20.0 m` could not be printed. `Show` for records,
tuples, `Tensor`, `Point`, `Solutions`, `Dyn` and unit values is a structural guarantee: the
built-in printer renders them when there is no user instance, and an instance a user writes for
a nominal type **they declared** (or `derive Show`) takes priority. Functions have no `Show`
instance and no structural fallback (`E0310`). The instance sets of built-in classes take part
in global coherence (§7.4): two instances MUST be distinguishable by their run-time witness
(§9.2.3), and equal witnesses are `E0311` — for example `Tensor<1, Int>` and `Tensor<1, Real>`
are both exact storage and cannot both have an instance of the same class.

### 3.8.5 Deriving

`derive Eq, Show, Hash for Vec2` is open to structural classes only: `Eq`, `Ord`, `TotalOrd`,
`Show`, `Hash`; deriving any other class is `E0317`. A derived instance is generated in the
module that owns the type (the orphan rule applies) and `Ord` is lexicographic in field
declaration order. Deriving fails for a function field, and the offending field type and class
name appear in the `E0317`.

### 3.8.6 Propositions `Prop`

`if`, `match` and `assert` accept only `Bool`. Comparing `Real` values yields a proposition
`Prop`:

```
(==) (!=) (<) (<=) (>) (>=) : Real[d] -> Real[d] -> Prop
decide   : Prop -> Option<Bool>
assuming : (Prop, () -> a) -> a
by_cases : (Prop, () -> Real[d], () -> Real[d]) -> Real[d]
```

- Comparisons of `Int`, `Float`, `String`, records and variants return `Bool`; comparisons of
  `Real` return `Prop`, so no boolean is "guessed" at run time. `Prop` supports `&&`, `||` and
  `!`, and the result is still a `Prop`.
- `decide(p)` returns `Some(true/false)` when it can prove or refute `p`, and `None` otherwise.
- `assuming(p, thunk)` adds `p` as an assumption to the symbolic computation inside `thunk`;
  it is `E0804` if `p` is decidably `false`. Only unary predicates are supported so far
  (`x > 0`, `x >= 0`, `x != 0`, `x ∈ ℤ`).
- `by_cases(p, yes, no)` picks a branch when `p` is decidable and otherwise builds a symbolic
  `ite` term (symbolic derivation of piecewise functions).
- A `Prop` in an `if` is `E0302` (symbolic proposition is not a `Bool`); `decide` first, or
  rewrite with `assuming`/`by_cases`.

This distinction prevents a symbolic Bool from masquerading as a `Bool`: exhaustiveness of
`Bool`, the `Eq` laws and the determinism of `if` all hold.

## 3.9 Effects: `IO`

### 3.9.1 Type

`IO : Type -> Type` and `pure : a -> IO a`; `do` desugars to a chain of `bind` (`bind` is not a
function users can call). `IO` is a single effect type. Capabilities (`fs`, `net`, `env`,
`rand`, `gpu`) are run-time permissions, not type parameters (§13.9.6), so standard-library
signatures are not infested with effect rows while plugins and sandboxes can still declare the
capabilities they need.

An `IO a` is an action **description**; effects occur only when the runtime executes `main`. A
pure function cannot run `IO`.

### 3.9.2 Typing of `do`

```
Γ ⊢ e1 : IO a     Γ, x: a ⊢ rest : IO b
----------------------------------------
Γ ⊢ (x <- e1; rest) : IO b

Γ ⊢ e : IO a     a is not ()
--------------------------------
Γ ⊢ (e; rest) : IO b      (the result of e is discarded, warning W0202)
```

A `let` inside `do` is a pure binding.

### 3.9.3 Errors

Recoverable errors use `Result<T, E>` and `?`; possible absence uses `Option<T>` (`?` works too);
IO failures come back as a `Result` (for example `read_file`). There is no `try/catch` and no
user-callable `panic`; an unrecoverable run-time error ends the program and prints the call
stack (§6.9).

### 3.9.4 The purity boundary

`IO` in a pure context is `E0313`. An `IO` action may capture pure values but not vice versa.
There is no `unsafePerformIO`. `main : IO Unit` is the only entry point; a library module may
export `IO` actions for the caller to run.

## 3.10 Literals and defaults

### 3.10.1 Literal types

A literal is **representation-polymorphic**: it carries a `Literal` constraint, is instantiated
where a type is expected, and is defaulted at the end of the declaration.

| Literal | Default type | May instantiate to |
|---|---|---|
| `123` | `Int` | `Int`, `Real`, `Float`, `Complex` |
| `1.5`, `1e3` | `Float[1]` | `Float[1]`, `Real[1]` (exact decimal) |
| `3 m` | `Real[L]` | `Real[L]`, `Float[L]`, `Complex[L]` |
| `3.0 m` | `Float[L]` | `Float[L]`, `Real[L]` |
| `25 °C` | `Point[Θ]` | `Point[Θ]` only (affine, fixed Float) |
| `oo` | `Real[1]` | `Real[1]`, `Float[1]`; with a dimension write `oo * 1 m` |
| `nan` | `Float[1]` | `Float[1]` |

- An integer literal converts automatically where `Real` or `Float` is expected (`2` in `df(2)`
  is a `Real`). A decimal literal converts to `Real` as an **exact decimal**: `1.5` is `3/2`,
  so `x + 1.5` (`x : Real`) stays exact.
- The representation of a unit literal follows its coefficient: an integer coefficient defaults
  to `Real` and a decimal one to `Float`; an expected type may override (`fall_time(20 m)`
  instantiates `20 m` as `Float[L]` where `h : Float[L]`).
- `0` is not dimension-polymorphic and defaults to `Int`; zero length is `0 m` and a floating
  zero is `0.0` (§4.7).

### 3.10.2 Defaulting

At the end of a declaration, variables still unresolved default in this order:

1. a `Rep` variable from an integer literal → `Int`; 2. from a decimal literal → `Float`;
   3. from a unit literal → `Real` (integer coefficient) or `Float` (decimal coefficient);
   4. any other `Rep` variable → `Real`; 5. a `Dim` variable → `1`, with warning `W0301`;
   6. a `Nat` variable is **not** defaulted: if constraints cannot solve it, `E0304`;
   7. a row variable → the empty row.

Defaulting touches only variables not quantified by a scheme or the lexical environment, and
propagates all numeric constraints first. A `Literal` constraint chooses a legal default and
never reads a decimal literal as `Int`. A private literal in a function body that depends on a
generic input is, once the inputs are fixed, read first according to the arithmetic `join` of
the inputs in the same numeric-constraint component, then by its literal default; unrelated
parameters do not take part and the caller's expected result cannot choose this reading. A
literal used directly as a generic parameter (`fn zero<r: Rep>(): r = 0`) keeps its own legal
domain. Data that is actually built and captured keeps one type instance, and function aliases
pass the construction requirement on, so a captured number is never read as both `Int` and
`Float`.

### 3.10.3 Annotations take priority

With an expected type, a literal is checked against it and defaulting does not run:

```aevia
let x: Float[L] = 0.0 m    # zero length, Float
let n: Int = 0
let y = 0                  # defaults to Int
let z: Real[L] = 0 m       # zero length, exact
```

A `+ - * /` expression made only of literals and signs accepts the same local context:
`let exact: Real = 0.1 + 2` is exactly `21/10`. The rule does not cross variables or calls: in
`fn f(x) = x + 1.0`, `f(1)` is a `Float` and the caller cannot annotate it `Int`.

### 3.10.4 Literal folding

Folding is compile-time evaluation. It MUST give the same value as run-time evaluation; it is an
optimisation, not a second arithmetic.

1. **A literal is rounded once.** Decimal text is an exact rational; in a `Float` position it
   becomes the nearest f64. A unit literal multiplies its scale in exact arithmetic before
   rounding: `1.1 km : Float[L]` is the f64 nearest `1100`, not `1.1 * 1000.0` rounded twice;
   a scale with π (`deg`) multiplies the correctly rounded rational part by the f64 `pi^k`.
2. **A closed tree of literal arithmetic is a compile-time constant.** An expression made only of
   literals, `+ - * /` and signs is evaluated once at its static representation; a dimensionless
   tree of `Float` literals is folded to one constant at IR lowering with the same IEEE
   operations. So `0.1 + 0.2 : Float` is `0.30000000000000004` whether written inline or via
   `let`.
3. **Exactness comes from types, not folding.** `1.0 + 2` with no expected type is
   `join(Int, Float) = Float`, value `3.0`; a decimal literal meeting an exact `Real` operand is
   read as an exact decimal: `1/3 + 0.1` is `13/30`.

## 3.11 Dynamic dimensions `Dyn` (summary)

Static dimensions cannot express a run-time exponent `x^n` (`n : Int`) or a shape-dependent
dimension (the `d^n` of `det(A)`). `Dyn<ρ>` is the explicit escape (representation kept):

```
dyn       : Scalar<ρ,d> -> Dyn<ρ>                       # d concrete
check     : Dyn<ρ> -> Result<Scalar<ρ,d>, DimError>     # d concrete
dyn_with  : (Scalar<ρ,d>, DimValue) -> Dyn<ρ>
check_with: (DimValue, Dyn<ρ>) -> Result<Scalar<ρ,1>, DimError>
dim_of    : Dyn<ρ> -> DimValue
```

`dyn` and `check` are type-directed primitives: `d` MUST be concrete at the call site and the
compiler materialises it as a `DimValue`; generic code passes a run-time dimension explicitly
with `dyn_with`/`check_with`. A `Dyn` never converts implicitly to a `Scalar`. The full rules
are in §4.9.

## 3.12 Type errors

| Code | Situation |
|---|---|
| `E0301` | unbound name |
| `E0302` | type mismatch |
| `E0303` | occurs check (infinite type) |
| `E0304` | cannot infer / ambiguous |
| `E0305` | not a function |
| `E0306` | record lacks a field |
| `E0307` | constructor pattern arity mismatch |
| `E0308` | pattern does not match the scrutinee, or a pattern binds a name twice |
| `E0309` | non-exhaustive match |
| `E0310` | missing type-class instance |
| `E0311` | overlapping instances |
| `E0312` | non-positive recursive type |
| `E0313` | `IO` in a pure context |
| `E0314` | tensor element type or rank cannot be inferred |
| `E0315` | wrong number of arguments in a call |
| `E0316` | duplicate definition |
| `E0317` | class, instance or derive error |

Dimension errors are `E04xx` (chapter 04) and unit errors `E05xx` (chapter 05).

## 3.13 Key inference rules

```
Γ(x) = σ    σ' = inst(σ)        Γ ⊢ e1 ⇒ τ1 -> τ2    Γ ⊢ e2 ⇐ τ1
-----------------------        --------------------------------
Γ ⊢ x ⇒ σ'                      Γ ⊢ e1(e2) ⇒ τ2

Γ, x: τ1 ⊢ e ⇐ τ2                Γ ⊢ e1 ⇐ Bool   Γ ⊢ e2 ⇐ τ   Γ ⊢ e3 ⇐ τ
--------------------------------  ------------------------------------------
Γ ⊢ (fn (x) => e) ⇐ τ1 -> τ2      Γ ⊢ if e1 then e2 else e3 ⇐ τ

Γ ⊢ e ⇒ { l: τ | ρ }             Γ ⊢ e1 ⇐ Scalar<ρ,d>   Γ ⊢ e2 ⇐ Scalar<ρ,d>
--------------------             ------------------------------------------
Γ ⊢ e.l ⇒ τ                      Γ ⊢ e1 + e2 ⇐ Scalar<ρ,d>

Γ ⊢ e1 ⇐ Scalar<ρ1,d>    Γ ⊢ e2 ⇐ Scalar<ρ2,e>
------------------------------------------------
Γ ⊢ e1 * e2 ⇐ Scalar<join(ρ1,ρ2), d*e>
```

The implementation order is: parse → name resolution → collect signatures → infer/check →
default → export the interface. The dimension unification rules are in the next chapter.
