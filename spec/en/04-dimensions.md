# 04 — Dimensions

This chapter is the heart of Aevia. Chapter 03 treated the `d` in `Real[d]` as a type
parameter; here we define what `d` is, how dimensions are equal, solved and inferred, and what
to do when the static system cannot express something.

## 4.1 Base dimensions

| Canonical name | Source / display alias | Meaning |
|---|---|---|
| `Length` | `L` | length |
| `Mass` | `M` | mass |
| `Time` | `T` | time |
| `Current` | `I` | electric current |
| `Temperature` | `Θ` | thermodynamic temperature |
| `Amount` | `Amt` | amount of substance |
| `Luminosity` | `Lum` | luminous intensity |

- A base dimension is a **generator**, not a type constructor; `1` is dimensionless (the empty
  product). Names are case-sensitive and the **full name is canonical**: the boundaries of FFI,
  manifests and `rssn::dim` always use full names, while the single letters are only source and
  display aliases.
- `dimension Currency` declares a new base dimension. Its identity is fixed by the canonical
  package/source owner of the defining module, the module path and the declared name; it is not
  a global bare name. Two packages that each declare `Currency` get different generators, while
  one declaration imported under different aliases is still equal. A prefix such as `A.Currency`
  is only a display or lookup hint and takes no part in equality, ordering, hashing or ADT
  schemas; an imported interface carries the resolved dimension. Instantiating a generic
  `d: Dim` replaces only dimension variables and keeps every nominal generator with its
  exponent.
- Source base dimensions and host (rssn) dimensions are kept apart: a plain host `Base("Currency")`
  is never automatically equal to any source `dimension Currency`. A nominal generator crossing
  the bridge uses a versioned canonical encoding containing the full owner and declared name
  (ordinary host names under the reserved prefix are escaped to avoid collisions); the carrier is
  still RSSN's `Base` string, and the signature hash and binary layout of SI are unchanged. A
  generated façade module cannot redeclare an external owner by display name: such operators use
  a dynamic path, and units and constants of an external nominal dimension are marked "cannot be
  generated" and skipped, so the user imports them from their defining module.
- A `private dimension` is visible only in its defining module. Public aliases, function
  signatures and type aliases may refer to it; the interface stores its resolved meaning, so
  users neither need nor can resolve the private name. An import is not a transitive export.

### 4.1.1 Derived aliases

Aliases are transparent and create no new type:

```
dimension Area          = L^2            dimension Volume       = L^3
dimension Velocity      = L/T            dimension Acceleration = L/T^2
dimension Frequency     = 1/T            dimension Force        = M*L/T^2
dimension Pressure      = M/(L*T^2)      dimension Energy       = M*L^2/T^2
dimension Power         = M*L^2/T^3      dimension Momentum     = M*L/T
dimension Density       = M/L^3          dimension Charge       = I*T
dimension Voltage       = M*L^2/(I*T^3)  dimension Resistance   = M*L^2/(I^2*T^3)
dimension Capacitance   = I^2*T^4/(M*L^2)  dimension Inductance = M*L^2/(I^2*T^2)
dimension MagneticFlux  = M*L^2/(I*T^2)  dimension MagneticFluxDensity = M/(I*T^2)
dimension Entropy       = M*L^2/(T^2*Θ)  dimension MolarMass    = M/Amt
dimension Conductance   = 1/Resistance   dimension Angle        = 1
dimension SolidAngle    = 1
```

`Angle = 1` is deliberate: in SI the radian is dimensionless, `sin` accepts a dimensionless
argument, and `deg` and `rad` are dimensionless units with scales (`π/180`, `1`) (§5.5). An
`Angle` base dimension would break `exp(i*θ)`, `sin(ω*t)` and many other formulas. Aliases
affect printing only: `Real[M*L^2/T^2]` and `Real[Energy]` are the same type, and printing
prefers the shortest known alias.

## 4.2 Dimension expressions

```
δ ::= 1 | b | d | δ * δ | δ / δ | δ ^ q
```

`b` is a base dimension, `d` a dimension variable (kind `Dim`) and `q` a rational literal (`2`,
`-1`, `(1/2)`, `(-3/4)`; the decimal `0.5` is invalid). **The exponent of a static dimension is
always a compile-time rational:** when `n` is a run-time value the type level cannot represent
`d^n`, and such cases use `Dyn` (§4.9). rssn's `graph::dim` supports a single symbolic exponent
on the term graph; Aevia's static types do not.

### 4.2.1 Canonical form

A dimension is normalised to a finite map `δ̂ : (Base ∪ DimVar) → Q` keeping only generators with
non-zero exponent: multiplication adds exponents, division subtracts, a power multiplies, zero
entries are dropped, and generators are sorted in a fixed order (SI order, user bases, then
variable names lexicographically). For example `L * T^-1 ⇒ {L:1, T:-1}`, `L / L ⇒ {}`,
`(L^2)^(1/2) ⇒ {L:1}`. **Two dimensions are equal if and only if their canonical forms are
equal:** `L/T` and `L*T^-1`, or `L^2` and `L^(4/2)`, are the same dimension.

Exponents are arbitrary-precision rationals, supporting at least `i64` numerators and
denominators; `1/2`, `1/3`, `-1` and `2` MUST be exact. `sqrt` yields `1/2` and `cbrt` yields
`1/3`.

### 4.2.2 Dimension variables

A dimension variable `d` stands for "some unknown dimension" ranging over the whole free abelian
group, and may appear in exponent positions: `Real[d]`, `Real[d^2]`, `Real[L/d]`,
`Real[d^(1/2)]`.

```aevia
fn square<d>(x: Real[d]): Real[d^2] = x^2
# square : ∀d. Real[d] -> Real[d^2]
```

## 4.3 Dimension unification

Dimension unification solves equations over an abelian group: given `δ1` and `δ2`, find a
substitution `σ` with `σ(δ1) = σ(δ2)`.

### 4.3.1 Algorithm

Let `q = normalize(δ1 / δ2)`, written as an exponent vector:

1. If `q = 1`, succeed.
2. Otherwise pick a dimension variable `v` with exponent `r ≠ 0` (in name order, for
   reproducibility), set `σ[v] = (q / v^r)^(-1/r)`, apply `σ` to `q` and return to step 1.
3. If there is no variable and `q ≠ 1`, the dimensions mismatch (`E0401`).

Each step eliminates one variable, so the algorithm terminates. The solution never contains `v`
itself, so an occurs-check failure cannot arise over an abelian group (`E0409` is reserved and
the present unifier never produces it). The most general unifier is not unique, but all choices
are equivalent.

**Examples**

```
unify(L, T):      q = L/T, no variable, q ≠ 1 ⇒ mismatch
unify(d, L/T):    q = d·T/L, pick d: σ[d] = L/T
unify(d^2, d^3):  q = d^-1, pick d: σ[d] = 1  ⇒ d = 1
unify(d*e, L):    pick d: σ[d] = L/e, e stays free
```

The third example shows that a function without declared dimension parameters,
`fn f(x: Real[d], y: Real[d^3]) = x^2 + y`, specialises `d` to `1`; the same function with a
declared `<d>` is rejected with `E0318` (§4.4.1).

### 4.3.2 Diagnostics and integration

- A dimension mismatch `E0401` MUST list the canonical forms of both sides and the difference
  (for instance "the left has an extra `T^-1`").
- Dimension unification runs when type unification meets `Scalar<ρ1,d1>` and `Scalar<ρ2,d2>`:
  `unify(Scalar<ρ1,d1>, Scalar<ρ2,d2>) = unify(ρ1, ρ2) ∧ unify(d1, d2)`. Unifying `ρ` is
  ordinary first-order unification; dimension variables and type variables are stored
  separately and share substitution and levels.

## 4.4 Dimension polymorphism

### 4.4.1 Generalisation and rigid parameters

At `let` and top-level declarations, free dimension variables that do not occur in the
environment are universally quantified and may appear anywhere in a type:

```aevia
fn mean<d>(xs: Tensor<1, Real[d]>): Real[d] = sum(xs) / length(xs)

fn map_quantity<d, e>(f: Real[d] -> Real[e], xs: Tensor<1, Real[d]>)
    : Tensor<1, Real[e]> = map(f, xs)
```

**Declared dimension parameters are rigid.** `fn f<d>` declares that `f` works for *any* `d`, and
the body cannot choose it for the caller. After checking the body, every declared dimension
parameter MUST still consist of free variables only (no base dimensions such as `L` or `T`), and
the exponent vectors of the declared parameters MUST be linearly independent (they can still be
chosen independently). A violation is `E0318`:

```aevia
fn h<d>(x: Real[d]): Real[d] = x + 1 m
# error[E0318]: `h` is less generic than its declared type parameters
#   = the body requires `d = L`

fn g<d, e>(x: Real[d], y: Real[e]): Real[d] = x + y
#   = the body determines `e` from `d`
```

The unifier may write one variable as a product of other free variables (in `linear_fit` of §11.8
`d` is solved as `d26*d28*d30/(d25*d29)`); that is still generic: the test is independence, not
"is it still a bare variable". A declared **type** parameter may be refined to a
representation-polymorphic numeric type (`fn number<a>(): a = 1` holds for all four
representations) and is `E0318` only when the body fixes it to a variable-free type; a declared
`Nat` parameter is solved by equations per §3.5.1 and is not subject to this rule.

### 4.4.2 Weak variables

The weak variables introduced by `symbol`, `symfun` and the empty tensor are not generalised
(§3.3.3):

```aevia
let x = symbol("x")       # x : Real[d], d weak
let f = fn (t) => x * t   # d unifies with the dimension of t
```

If `x` is still unconstrained at the end of the declaration it defaults to `1` with warning
`W0301`. Weak variables guarantee that one symbol has one dimension at every use.

### 4.4.3 Instances and erasure

`instance Show for Real[d]` covers every dimension; also defining `Show for Real[L]` would overlap
(`E0311`). Dimension parameters are erased at run time: `mean<L>` and `mean<T>` are the same code,
so no monomorphisation is needed and there is no run-time branching on a dimension; use `Dyn` when
a run-time dimension is needed.

## 4.5 Dimension rules for operators

### 4.5.1 Scalars

With `ρ = join(ρ1,ρ2)` (§3.4):

| Expression | Result dimension | Constraint |
|---|---|---|
| `a + b`, `a - b`, `min`, `max`, `mod`, `rem`, `hypot`, `clamp` | `d` | operands share `d` |
| `a * b` / `a / b` | `d1*d2` / `d1/d2` | — |
| `a ^ n` (`n` a literal) | `d^n` | `n ∈ Q` |
| `a ^ n` (`n : Int` at run time) | not statically expressible | `d = 1` or `Dyn` (§3.4.3) |
| `-a`, `abs`, `floor`, `ceil`, `round`, `trunc` | `d` | — |
| `sign(a)` | `1` | — |
| `atan2(y,x)` | `1` | same `d` |
| `sqrt(a)`, `cbrt(a)` | `d^(1/2)`, `d^(1/3)` | — |
| `exp ln log sin cos …` | `1` | argument has `d = 1` |

### 4.5.2 Tensors

The dimension rules for tensor reductions and linear algebra are in §3.5.5. `fft` and `ifft` keep
the element dimension. `prod` and `det` accept dimensionless elements only and use `Dyn` otherwise
(`E0410`). `mean` does not change the dimension, because its divisor is a dimensionless count.

### 4.5.3 Calculus

| Expression | Type |
|---|---|
| `diff(f)` | `(Real[d] -> Real[e]) -> (Real[d] -> Real[e/d])` |
| `antiderivative(f)` | `(Real[d] -> Real[e]) -> (Real[d] -> Real[e*d])` |
| `defint(f,a,b)` | `(Real[d] -> Real[e], Real[d], Real[d]) -> Result<Real[e*d], MathError>` |
| `limit(f,x)` | `(Real[d] -> Real[e], Real[d]) -> Result<Real[e], MathError>` |
| `sum(f,a,b)` | `(Real[1] -> Real[d], Real[1], Real[1]) -> Real[d]` |
| `product(f,a,b)` | only `d = 1`, or `Dyn<Real>` (the dimension is `d^n`) |
| `solve(f)` | `(Real[d] -> Real[e]) -> Result<Solutions<Real,d>, MathError>` |
| `nsolve(f,x0)` | `(Real[d] -> Real[e], Real[d]) -> Result<Real[d], MathError>` |
| `odeint(f,y0,ts)` | see §8.9 |

Differentiating `f : Real[L] -> Real[T]` gives `Real[T/L]` ("seconds per metre"), which prevents
the derivative from being used with the dimension of the original function.

### 4.5.4 Statistics

`mean`, `median` and `quantile` keep the element dimension `d`; `var` gives `d^2`; `std` gives
`d`; `cov(xs,ys)` gives `d*e`; `corr`, `skewness` and `kurtosis` are dimensionless;
`histogram(xs, n)` has dimensionless counts and edges of dimension `d`.

### 4.5.5 Containers

`Solutions<Real,d>`, `Option<Real[d]>`, `Result<Real[d],e>` and `IO (Real[d])` do not change the
inner dimension. The result dimension of `map` is decided by the function; `filter` does not
change it.

## 4.6 Dimensions of unit values

A unit is a value of type `Unit[d]`: `m : Unit[L]`, `km : Unit[L]`, `degC : Unit[Θ]` (affine,
§5.6).

```
u * v : Unit[d*e]       u / v : Unit[d/e]       u ^ n : Unit[d^n]   # n a literal
Scalar<ρ,d> * Unit[e] : Scalar<ρ,d*e>
Scalar<ρ,d> / Unit[e] : Scalar<ρ,d/e>
Scalar<ρ,d> / Unit[d] : Scalar<ρ,1>     # the number counted in that unit
```

Tensors work element-wise in the same way. `x / m` gives "the number of metres" (`Real[1]`) and
`x * m` gives a quantity. This is more basic than `to(m, x)`: `to` changes only the display unit,
whereas `/` really cancels the dimension.

## 4.7 Zero and infinity

`0` is **not** dimension-polymorphic. It is an ordinary integer literal and defaults to `Int`:

```
0      : Int           0.0    : Float[1]
0 m    : Real[L]       0.0 m  : Float[L]
```

- `0 + x` is `E0406` unless `x : Int`; to add a zero length write `0 m + x`.
- `0 * x` promotes the `Int` to the representation of `x` and has dimension `dim(x)`; `0 / x` has
  dimension `1/dim(x)`.
- `x / 0` is a run-time `E0801` for exact values and IEEE Inf/NaN for floats.
  `(0 m)^2 : Real[L^2]`.

This gives up the convenience of `0 + 3 m` in return for a simple, reliable type system; the
error message suggests writing `0 m`.

`oo` is the infinity of `Real[1]` and is not dimension-polymorphic; `nan` is `Float[1]`. A
dimensioned infinity is written `oo * 1 m`. `oo - oo` and `oo * 0` are `nan`; these are numeric
semantics and do not affect typing.

## 4.8 Dimension inference

Implementation has three steps: **constraint generation** (walk the AST; literals, variables,
operators, calls, records, tensors and patterns all produce constraints); **unification** (solve
type, dimension and row variables and class constraints, using §4.3 for dimensions); and
**defaulting and generalisation** (generalise at `let` boundaries, default what remains at the end
of a declaration, check class constraints, export the interface). Constraint forms: `τ1 ~ τ2`
(types), `δ1 ~ δ2` (dimensions, equivalent to `δ1/δ2 ~ 1`), `ρ1 ~ ρ2` (rows) and `C τ` (class).

### 4.8.1 A complete derivation

```aevia
fn fall_time(h: Float[L]): Float[T] =
    sqrt(2 * h / g)
```

`h : Float[L]` and `g : Float[L/T^2]`; `2 : Int` is promoted to `Float[1]`; `2 * h : Float[L]`;
`2 * h / g : Float[L / (L/T^2)] = Float[T^2]`; `sqrt` halves the exponent to give `Float[T]`,
which unifies with the return type. Written as `sqrt(2 * h * g)` the result is `Float[L/T]`, which
does not match `Float[T]`, and the error is `E0401`.

### 4.8.2 Error messages

A dimension error MUST contain the canonical forms of the expected and actual dimensions, the
difference (which base dimensions are extra or missing), the smallest offending subtree, and a fix
when one can be inferred from context:

```
error[E0401]: dimension mismatch
  --> kinematics.aev:7:13
   |
 7 |     sqrt(2 * h * g)
   |              ^^^^^ expected Real[T^2], found Real[L^2/T^2]
   |
   = left:  L^2/T^2
   = right: T^2
   = difference: L^2/T^4
help: did you mean `2 * h / g`?
```

## 4.9 Dynamic dimensions `Dyn`

### 4.9.1 Purpose

Static dimensions cannot express: `x^n` (`n : Int` at run time, `x` dimensioned), `det(A)` (order
`n` known at run time), `product(f, a, b)` (run-time term count), quantities read from CSV/JSON,
and user-entered formulas (dimension is data). `Dyn` gathers them into one explicit,
run-time-checked type.

### 4.9.2 Representation

```
Dyn        : Rep -> Type
DimValue   : Type        # run-time dimension: base dimensions with rational exponents
dyn        : Scalar<ρ,d> -> Dyn<ρ>                       # d must be concrete
check      : Dyn<ρ> -> Result<Scalar<ρ,d>, DimError>     # d must be concrete
dyn_with   : (Scalar<ρ,d>, DimValue) -> Dyn<ρ>
check_with : (DimValue, Dyn<ρ>) -> Result<Scalar<ρ,1>, DimError>
dim_of     : Dyn<ρ> -> DimValue
value_of   : Dyn<ρ> -> Scalar<ρ,1>                       # the number in coherent SI units
pow_dyn    : (Dyn<ρ>, Int) -> Dyn<ρ>
det_unit   : (Tensor<2, Scalar<ρ,d>>, Unit[d]) -> Dyn<ρ>
```

Internally a `Dyn<ρ>` is (a number, a `DimValue`, a display unit). It is not a `Scalar` and does
not convert implicitly to `Scalar<ρ,d>`; it must be `check`ed. A `DimValue` stores a canonical
dimension text that can be restored losslessly: custom host bases, nominal owners and variables all
have explicit encodings and SI text keeps its existing form. `dim_of` → `dyn_with`/`check_with` does
not recover identity from display names, and the readable aliases used in diagnostics and numeric
suffixes must not be taken as evidence of cross-module dimension identity.

### 4.9.3 Operations

`+` and `-` require equal run-time dimensions; `*` and `/` add and subtract the dimension
exponents; `^ Int` multiplies the exponents; `sqrt` halves them. **A dimension mismatch is the
run-time error `E0412`**: `Dyn` is already the escape layer, so the error ends evaluation and is
not a `Result`. For a recoverable check, use `check`/`check_with`, which return a `Result`.

### 4.9.4 The boundary with the static layer

- `dyn(x)` and `check(x)` are **type-directed primitives**: `d` MUST be a concrete dimension (no
  free variables) and the compiler materialises it as a `DimValue`. The result type of `check`
  must be fixed by an annotation or later use, otherwise it is `E0304`.
- In generic code `d` is a variable and cannot be passed to `dyn`/`check` directly; use
  `dyn_with(x, dim)`/`check_with(dim, x)` and pass the run-time dimension explicitly (from a CSV
  schema, user input, and so on).
- A `Dyn<ρ>` can be printed, compared (same dimension) and stored in containers, but it cannot take
  part in arithmetic that needs a `Scalar<ρ,d>`, and it is slower than `Real`/`Float`; do not use it
  in a hot loop.

### 4.9.5 Example

```aevia
open Units.SI

fn scale_law(x: Real[L], n: Int): Dyn<Real> =
    pow_dyn(dyn(x), n)

fn volume(a: Tensor<2, Float[L]>): Dyn<Float> =
    det_unit(a, m)

fn main(): IO Unit = do
    println(scale_law(2 m, 3))                                   # 8 m^3
    println(volume([[2.0 m, 0.0 m], [0.0 m, 3.0 m]]))            # 6.0 m^2
    let r: Result<Real[L^3], DimError> = check(scale_law(2 m, 3))
    println(r)                                                   # Ok(8 m^3)
```

`det_unit` embodies a pattern: factor the unit out, compute dimensionlessly, and use `Dyn` to put
the dimension back. This is more honest than letting `det` return an unknowable static dimension.

## 4.10 Relation to rssn's dimension layer

rssn's `graph::dim` is a **term-level** dimension inference: every scalar term has a dimension,
every operator has a `DimRule`, dimension is an e-class attribute, and it supports rational and
symbolic exponents, dimension variables and `Advisory`/`Strict` modes. Aevia's static layer shares
the same base dimensions and exponent algebra but **by default does not pass dimensions to rssn**:
the term graph of the static path is dimensionless, and the term-level layer is used only at
FFI/export (with `quantity`) or for optional diagnostics.

| | Aevia static layer | rssn term layer |
|---|---|---|
| When | compile time | run time / simplification |
| Subject | types of the source program | e-classes of the term graph |
| Exponents | rational constants only | rational + symbolic |
| Failure | rejects the program | diagnostic / error in strict mode |

Both sides use one dimension algebra (`aevia-dim`) with the same base names and canonical forms. At
start-up the bridge MUST check consistency item by item over the standard base dimensions, common
derived dimensions and the unit table; on any disagreement Aevia's table wins and a warning is
logged. rssn's `DimExpr::to_term` can intern `Length^n` as an atom, so the `Dyn` escape path does
not fail at the bridge. This does not change the language decision: a static type position accepts
rational exponents only, and a symbolic exponent can occur only in the run-time dimension of a
`Dyn`.

## 4.11 Dimension errors and warnings

| Code | Situation |
|---|---|
| `E0401` | dimension mismatch (addition, subtraction, comparison, function argument) |
| `E0402` | dimension used as an exponent (`2^(3 m)`) |
| `E0403` | transcendental function applied to a dimensioned argument (`sin(3 m)`) |
| `E0404` | run-time exponent on a dimensioned base (`x^n`, `n : Int`) |
| `E0405` | illegal affine-point operation (`Point + Point`) |
| `E0406` | a bare `0` where a dimension is needed; write `0 m` |
| `E0407` | unknown dimension or alias |
| `E0408` | reserved (unsatisfiable dimension constraint) |
| `E0409` | reserved (occurs check, §4.3.1) |
| `E0410` | dimensioned `det`/`prod`/`product` needs `Dyn` |
| `E0411` | incompatible element types in a tensor operation |
| `E0412` | `Dyn` run-time dimension check failed (run-time error) |
| `E0318` | a declared dimension parameter is more general than the body (§4.4.1) |

## 4.12 Design trade-offs

- **Dimensions are types, not run-time tags.** Run-time tags (Pint, Unitful) catch errors but leave
  them to testing or production; types catch them at compile time and guide inference (the
  dimension of `mean` follows its input). The cost is a more complex type system and the need to
  provide `Dyn` for what static types cannot express.
- **Units are not in types.** If `m` and `ft` were different types, `Real[L]` would become
  `Real[L, m]`, signatures would have to quantify over units, and type equality would get
  complicated. Reducing units to compile-time scales makes type equality equal dimension equality,
  lets the unit table grow independently and folds conversions at compile time; the price is that
  the type level does not distinguish different units of one dimension — and the real error in
  scientific computing is `m + s`.
- **No symbolic exponents.** `L^n` needs the type level to carry an expression in `n`, turning
  unification into polynomial equations that may be undecidable. rssn can return a "symbolic
  constraint" at the term level instead of rejecting; Aevia chooses static conservatism plus the
  `Dyn` escape: the static path is decidable and explainable, the dynamic path explicit and
  checkable.
- **`Angle = 1`.** In physical formulas the arguments of `sin(ω t)` and `exp(i θ)` are already
  dimensionless; making angles dimensionless with `rad`/`deg` for input and output is SI's choice
  and also the least obstructive.

The next chapter turns `Unit[d]` from a type into a usable unit table: prefixes, conversions,
affine points and constants.
