# 06 — Evaluation semantics

This chapter defines run-time behaviour: what values are, the order of evaluation, how
symbolic and numeric values coexist, how tensors are represented, how IO executes and how
errors are reported.

## 6.1 Evaluation strategy

Aevia is **strict, call-by-value and left-to-right**:

- Function arguments are evaluated before the call; a binary operator evaluates its left
  operand first; the right-hand side of a `let` is evaluated immediately and then bound.
- Record fields are evaluated in written order; tensor elements in row-major order.
- `if` and `match` evaluate only the selected branch; `&&` and `||` short-circuit when the
  condition is decidable.

Pure functions have no side effects, so the order of evaluation affects only resource use,
not the result.

## 6.2 Values

```
v ::= () | true | false
    | n : Int | r : Real | f : Float | c : Complex | s : String
    | tensor(shape, storage) | sparse(format, shape, storage) | table(schema, columns)
    | device(cpu|gpu|distributed) | quantity(value, unit)
    | value(dynamic)                 # RSSN dynamic boundary
    | plugin(handle)
    | { l1 = v1, ..., ln = vn } | C(v1, ..., vn)
    | closure(env, params, body) | io(action) | set(elements)
    | Ok(v) | Err(v) | None | Some(v)
```

### 6.2.1 Scalar representation

```
Real    ::= Exact(Number) | Sym(TermId)
Float   ::= F64(f64)                     # strictly f64, never symbolic
Complex ::= C(Real, Real) | C64(Complex64) | Sym(TermId)
Int     ::= BigInt
```

- `Number` is rssn's exact number (arbitrary-precision integer or rational); `TermId` is a
  handle to a term in the symbolic graph, which may contain free symbols.
- `Float` has **only** the `F64` form; a floating-point expression containing symbols is a
  `Real`. This keeps `Float`'s IEEE semantics, `Ord` and JIT clean.
- `C(re, im)` is an exact or symbolic complex; `C64` comes from numeric kernels such as `eig`
  and `fft`; `Sym` is a symbolic complex term. The type `Complex[d]` does not distinguish
  exact from approximate; inspect with `re`/`im` when that matters.
- A display unit is scalar metadata `Option<UnitRef>` and takes no part in equality.

### 6.2.2 Closures and IO actions

A closure captures its lexical environment. A recursive function is realised by a
self-reference in the environment; semantically it is a value, not a special form.
`IO a` is an action description: a thunk that takes a world token and returns `a`. The token
is not exposed; the runtime supplies it when executing `main`. Actions can be composed, passed
and stored, but only the runtime executes them.

## 6.3 Exact, floating-point and symbolic

### 6.3.1 The three phases

| Phase | Representation | Example |
|---|---|---|
| exact | `Exact(Number)`, rationals only | `1/3`, `2` |
| symbolic | `Sym(TermId)`, free symbols or unsimplified operators | `x^2 + 1`, `sin(1)`, `sqrt(2)` |
| floating | `F64` | `0.8414709848078965` |

A `Real` moves between exact and symbolic; a `Float` is always f64. `approx` turns a `Real`
without free symbols into an f64.

### 6.3.2 Arithmetic dispatch

```
Exact(a) op Exact(b):  op closed on rationals, no domain error -> Exact(a op b); else Sym(build(op, a, b))
Sym(_) op _ or _ op Sym(_):  Sym(build(op, ...)), folded immediately when possible
F64 op F64, or F64 with Int:  Int is promoted to F64, computed under IEEE
F64 with Exact/Sym:           Real — the F64 is converted exactly to a rational, then Real rules apply
```

- Mixing `Real` and `Float` yields `Real` (exact wins, warning `W0302`): `1/3 + 0.5` is `5/6`
  and symbolic information is kept.
- `Int` with `Float` yields `Float`; an unannotated `1.0 + 2` is `Float` (the `join` of
  §3.4.1). For an exact reading, give a `Real` expected type (§3.10.3).
- `Int / Int` and negative integer powers yield an exact rational, even when the value is an
  integer; a rational with denominator 1 is not an `Int`.
- `0 / 0` and `1 / 0` in the exact phase are `E0801` (reported at compile time for literals);
  in the floating phase they give NaN/Inf under IEEE.

### 6.3.3 Building symbolic terms

`build(op, args)` calls rssn's operator constructor (`Session::call_value`) and does **not**
run the global rewrite engine, so `x + 1` is an O(1) term construction. Only these boundaries
trigger `compute`:

- explicit `simplify`/`eval`;
- heavy requests: `diff`, `integrate`, `solve`, `limit`, `sum`, `product`, `series`, `dsolve`;
- `approx`.

### 6.3.4 Domains and type invariants

- Symbolic functions of `Real` are **formal**: `sqrt(-1)`, `ln(-1)` and `asin(2)` stay as
  symbolic terms and do not turn into complex numbers; `approx` reports `E0806` for a result
  outside the domain. For complex results use `complex_sqrt` or `Complex`.
- `ln(0) = -oo`, `ln(oo) = oo`, `exp(oo) = oo`, `exp(-oo) = 0`.
- The bridge MUST disable rssn rules that simplify real-domain expressions to complex ones, or
  check the range before returning; breaking the range of `Real` is the internal error `E0809`.
- **Extended-real arithmetic in the exact phase is partial.** Apart from the boundary values
  above, `+ - * / ^` with `oo` stay symbolic terms (the undefined forms `oo - oo`, `oo*0`,
  `oo/oo`, `oo ^ 0` are kept too, and the sign of `-oo` is not lost); they are neither
  silently folded into a finite number nor reported as errors. For a finite answer compute in
  the floating phase (`inf`/`nan`, `nan != nan`, `-0.0 == 0.0`). An infinite exponent
  (`0 ^ oo`, `oo ^ oo`) requires a finite rational exponent and is `E0402`.

### 6.3.5 `eval`, `simplify`, `approx`

```
eval      : Real[d] -> Real[d]          # symbolic simplification, may stay symbolic
simplify  : Real[d] -> Real[d]          # alias of eval
approx    : Real[d] -> Result<Float[d], EvalError>
approx    : Float[d] -> Result<Float[d], EvalError>
```

- `eval` runs rssn with `Target::Symbolic` and returns the simplest closed form or the
  original; it need not terminate and is bounded by the budget (§6.9.1).
- `approx` runs with `Target::Numeric` and requires no free symbols, otherwise `E0803`. The
  default tolerance is `1e-12`; write `approx(x, tol)` to change it.

### 6.3.6 Symbol identity and substitution

```aevia
let x = symbol("x")       # a fresh symbol, display name x
let y = symbol("x")       # another symbol, same display name
```

- Each `symbol("x")` creates a **fresh** symbol: the internal name is unique, the display name
  is shared; `repr` shows the internal id.
- `subst(x, v, e)` substitutes by symbol identity (`x` MUST be a symbol value);
  `subst_all([(x, 1), (y, 2)], e)` substitutes in bulk.
- Symbolic equality `x == y` is a `Prop`, not a `Bool`, and cannot drive `if`; use
  `decide(x == y) : Option<Bool>` or `assuming`.

## 6.4 Implementation semantics of calculus and equations

### 6.4.1 `diff`

```
diff     : (Real[d] -> Real[e]) -> (Real[d] -> Real[e/d])
diff_at  : ((Real[d] -> Real[e]), Real[d]) -> Real[e/d]      # = diff(f)(x)
numeric_deriv : (Float[d] -> Float[e]) -> (Float[d] -> Float[e/d]) # numeric/AD, not symbolic
```

Evaluating `diff(f)`: create a fresh symbol `ξ : Real[d]`; apply `f` to `ξ` to get a symbolic
term `t`; compute `diff(t, ξ)` with `Target::Symbolic` to get `t'`; return the closure
`fn (x) => subst(ξ, x, t')`.

If `f` does something non-symbolic with the symbolic value (such as `if ξ > 0`: `ξ > 0` is a
`Prop` and `if` requires `Bool`), the error is `E0302`. Differentiate a piecewise function by
splitting it explicitly (`by_cases`/`assuming`, §8.2).

### 6.4.2 `antiderivative` and `defint`

```
antiderivative : (Real[d] -> Real[e]) -> (Real[d] -> Real[e*d])
defint         : (Real[d] -> Real[e]) -> Real[d] -> Real[d] -> Result<Real[e*d], MathError>
```

`antiderivative(f)` likewise creates `ξ`, computes `integral(t, ξ)` and returns a closure;
the constant of integration is taken to be 0. `defint(f,a,b)` returns `Err` on failure.

### 6.4.3 `limit`, `series`, `sum`, `product`

- `limit(f, a)`; `series(f, a, n)` returns an expression with `O(...)`.
- `sum(f, a, b)` uses an index symbol `κ : Real[1]` and builds `sum(f(κ), κ, a, b)`.
- `product(f, a, b)` requires a dimensionless `f`; otherwise use `product_dyn`, which returns
  `Dyn<ρ>`.

### 6.4.4 `solve` and `nsolve`

```
solve    : (Real[d] -> Real[e]) -> Result<Solutions<Real,d>, MathError>
nsolve   : (Real[d] -> Real[e]) -> Real[d] -> Result<Real[d], MathError>
nsolve_f : (Float[d] -> Float[e]) -> Float[d] -> Result<Float[d], MathError>
```

`solve(f)` hands `f(ξ) = 0` to rssn and converts the result to `Solutions<Real,d>` (roots,
multiplicities, parameter conditions) rather than a `Set`, because symbolic roots have no
total order. No solution gives an empty `Solutions`; failure to solve gives `Err`. `nsolve` is
numeric root finding and `nsolve_f` uses the floating-point kernel.

### 6.4.5 `dsolve` and `SymFun`

```aevia
let y : SymFun<T, L> = symfun("y")
let eq = diff(y)(t) + y(t) / tau == 0 m/s   # eq : Prop, both sides L/T
let sol = dsolve(eq, y, t)           # Result<(Real[T] -> Real[L]), MathError>, tau : Real[T]
```

- `SymFun<d,e>` is an unknown-function symbol; applying it to a `Real[d]` builds a term (the
  kernel's undefined-function node `apply(y, t)`, whose dimension is the codomain dimension
  the checker solved for that call site).
- `dsolve(eq, y, t[, conditions])`: conditions such as `[y(0) == 1]` fix the integration
  constants; undetermined constants are written `C1`, `C2`.
- If the answer is explicit, `y(t) = F(t)`, the result is the function `s ↦ F(s)`; implicit or
  parametric solutions, and equations with no closed form, give `Err` (carrying the kernel's
  rendering); the implementation does not guess. For numeric ODEs use `odeint` (§8.9).

## 6.5 Pattern matching

- Arms are tried in order; the first match wins. Constructor patterns match by tag, record
  patterns by field (open), tuple patterns by position.
- The tensor pattern `[a, b, ..rest]` matches only rank 1 with length at least 2; `rest`
  binds the remaining slice.
- Literal patterns: `Int`, `Bool`, `String`; a floating-point literal is not a valid pattern.
- A guard is evaluated after the pattern binds and MUST be a concrete `Bool`; a `Prop` is
  `E0308`.
- A non-exhaustive match is `E0309` at compile time, and a match never falls through at run
  time; an arm that can never be selected warns `W0201`.

Status: exhaustiveness and redundancy analysis is bounded recursion with a **pattern nesting
limit of 24**; beyond it a pattern is treated as covered, which can only suppress a
diagnostic, never produce a false one. Or-patterns and empty enums cannot be expressed in the
current grammar; a match with zero arms on a type with no constructors counts as exhaustive.

## 6.6 Tensor runtime

### 6.6.1 Representation

```
TensorData {
    shape: Vec<usize>,          # row-major
    storage: F64(Vec<f64>) | C128(Vec<Complex64>) | Exact(Vec<Number>)
           | Boxed(Vec<Value>) | Bool(Vec<bool>),
    display: Option<UnitRef>,
}
```

- `Float` uses `F64`, `Complex` uses `C128`, `Int`/`Real` use `Exact` or `Boxed`, `Bool` uses
  `Bool`; mixed elements are promoted to the widest storage at construction (`[1, 2.0]` is
  `[1.0, 2.0]`).
- The rank is the length of `shape` and equals `n` in the static type `Tensor<n, τ>` (checked
  by a run-time assertion).
- A display unit applies to the whole tensor.
- Construction: a literal is checked recursively for rectangularity, its element types are
  unified and it is flattened; `zeros`/`ones`/`fill` allocate contiguous storage;
  `range`/`linspace` produce `F64` or `Exact`; CSV/JSON input is parsed as `Float` and the
  caller annotates the dimension.

### 6.6.2 Broadcasting

Same-rank broadcasting: for each axis, equal sizes `d1 == d2` give `d`, a size of 1 takes the
other size, anything else is `E0802`. A scalar broadcasts to every element; mixed ranks are a
compile error (§3.5.3) unless you `reshape` explicitly.

### 6.6.3 Indexing and slicing

- A negative index `-1` is the last element; out of range is `E0802`.
- `a[i]` returns a sub-tensor view of rank one less; `a[i..j]` returns a view. Views share the
  buffer (safe under immutable semantics). `reshape` returns a view on contiguous memory and
  copies otherwise.
- There are no writes: every "modification" returns a new tensor, done in place when the
  reference is unique. `set(a, idx, v)` is the functional update.
- `at(a, i)` is rank-polymorphic indexing `Tensor<n,τ> -> Tensor<n-1,τ>` (`n>=1`).
- `a @ b` dispatches on rank: 2×2 `matmul`, 2×1 `matvec`, 1×2 `vecmat`, 1×1 `dot` (§3.5.3.1).

#### 6.6.3.1 Strides

In `a[i:j:s]`, `s` is an **integer** expression (not a `Float`, and not dimensioned); a
non-integer stride is `E0302`.

- An omitted `s` is 1; `a[i:j]` and `a[i..j]` are the same node.
- Omitted endpoints follow the direction of the stride: for `s > 0`, `i` defaults to 0 and `j`
  to the axis length; for `s < 0`, `i` defaults to the last index and `j` to **before** the
  first index. So `xs[::-1]` is `reverse(xs)` and `xs[::-2]` is `xs[4], xs[2], xs[0]`.
- A negative explicit endpoint counts from the end and is clamped to the valid range; the end
  is half-open: `xs[4:0:-1]` is `xs[4]…xs[1]`.

The length is exact: `ceil((b - a) / s)` for `s > 0` and `ceil((a - b) / |s|)` for `s < 0`.
A slice whose first step already passes `b` is **empty** (`xs[5:2:1]`, `xs[0:3:-1]`) and does
not panic. `s == 0` never advances: it is a static `E0802` when the stride is a literal and a
run-time `E0802` otherwise.

A negative stride yields a **view**: same buffer, different offset, that axis reversed
(`View { strides, reversed }`); a positive stride multiplies that axis's stride by `s`. So
`xs[::-1]` and `reverse(xs)` share one buffer, and `m[:, ::-1]` flips only the second axis.

### 6.6.4 Linear algebra

- `matmul`, `solve`, `inv`, `eig`, `svd`, `qr`, `lu` on `F64`/`C128` use rssn's numeric
  kernels; small `Exact`/`Boxed` matrices use symbolic linear-algebra rules, and a large matrix
  that cannot be numericised returns `Err` or stays symbolic.
- `det` accepts only dimensionless input; a dimensioned matrix uses `det_unit`, which returns
  `Dyn<ρ>` (`E0410`).
- `solve`/`inv` of a singular matrix return `Err(SingularMatrix)`; `eig` always returns
  `Complex`.

### 6.6.5 Reductions

- Empty tensor: `sum` = 0, `prod` = 1; `mean` is `NaN` for `Float` and `E0802` for `Real`;
  `min`/`max` are `E0802`.
- `sum(a, axis)` keeps the rank (that axis has size 1); `sum(a)` returns a scalar. `F64` uses
  pairwise/Kahan summation; `Exact` sums exactly.
- `var` is the sample variance by default (divide by `n-1`); `var(xs, population=true)` divides
  by `n`; `std = sqrt(var)`; `norm` is the 2-norm by default and `norm(xs, p)` supports 1, 2, ∞.

## 6.7 Records, variants and collections

- Records are stored sorted by field name; an update yields a new record and unchanged fields
  are shared. A variant is `(tag, payload)`.
- `Set<a>` and `Map<a,b>` are persistent ordered containers requiring `TotalOrd a`: elements
  are ordered by `total_compare`, so iteration is deterministic, and `Set`'s `==` is set
  equality. `Float` is not `TotalOrd`, so `Set<Float>` is invalid; wrap in `TotalFloat`.
- `Solutions<ρ,d>` = `{ roots: Tensor<1, Scalar<ρ,d>>, multiplicities: Tensor<1, Int>,
  conditions: Tensor<1, Condition> }`; it needs no `Ord`.

## 6.8 IO

### 6.8.1 Execution model

The runtime executes `main : IO Unit`: initialise the standard library and modules (pure
evaluation) → build `main`'s action → execute it (`print` writes standard output, `read_file`
reads a file, `plot` writes HTML) → the action returns `()`. An uncaught `panic` terminates
and prints the call stack.

A `do` block saves its program and lexical environment when built and does not run the pure
expressions inside it. On execution each statement is evaluated in source order: the next
statement, including a following `let`, action arguments and string interpolation, is
evaluated only after the previous IO action has completed; inserting an unused `let` does not
change the order of effects. The same action can be replayed, and each run has its own
bindings and execution position. The block's result is that of the last action; if the last
statement is a `let` or `<-` binding, the block returns `Unit`.

`?` applies only to `Result<T,E>` or `Option<T>`. A failure in a pure function returns from
that function; a failure inside `do` returns from the current IO program; a nested function
has its own return boundary. The target MUST be the same container family and the `Result`
error types MUST match; there is no implicit error conversion (the same applies to a function
without a return annotation). So the `do` of an `IO (Result<U,E>)` can propagate a
`Result<T,E>` while an `IO Unit` cannot. An early return skips the rest of the block, and the
enclosing IO can still bind and handle the failure value.

### 6.8.2 Common actions

```
print    : Show a => a -> IO Unit
println  : Show a => a -> IO Unit
eprintln : Show a => a -> IO Unit
read_line: () -> IO String
read_file: String -> IO (Result<String, IOError>)
write_file: String * String -> IO (Result<(), IOError>)
read_csv_dyn : String -> IO (Result<Table<Dynamic>, DataError>)
plot     : PlotSpec -> IO Unit
time_now : () -> IO Float[T]
random   : () -> IO Float[1]
```

`print` uses `show` and `println` appends a newline. `read_file` returns a `Result` and throws
no exception. `plot` writes `plot.html` in the current directory (or `--out`). `random` uses
the run-time RNG; for reproducibility use the pure API `rng(seed)`.

### 6.8.3 Purity

An `IO` action cannot appear in a pure function (`E0313`); a pure function may receive or
return an action as a value but cannot execute it. There is no `unsafePerformIO`. `main` is
the only entry point that runs automatically.

## 6.9 Budgets, errors and resources

### 6.9.1 Symbolic budget

rssn's `compute` takes a `Budget`. The actual defaults (`rssn::graph::schedule::Budget::default`)
are `max_iterations = 24`, `max_nodes = 20_000`, `match_limit = 1_000`, `reduce_rounds = 32`,
`patience = 3`, `time_limit = None`. Exhaustion is `E0808`; heavy requests
(`integrate`/`solve`) return `Err`.

Status: there is no language-level `with_budget(seconds, rewrites, thunk)` yet. The CLI's
`--budget` is the **tail-call budget** (§9.3.4), not the symbolic budget; the symbolic budget
takes the kernel defaults and is adjusted through the kernel `Config`/`Budget` API (embedding
layer).

### 6.9.2 Error classes

| Code | Situation | Recoverable |
|---|---|---|
| `E0801` | exact division or modulo by zero | no (floating point follows IEEE) |
| `E0802` | shape or index error | no |
| `E0803` | `approx` met a free symbol | yes (`Result`) |
| `E0804` | symbolic condition cannot be decided, or the assumption of `assuming` is false | no |
| `E0805` | stack overflow (non-tail recursion too deep) | no |
| `E0806` | domain error | yes (`Result`) |
| `E0807` | numerical method did not converge | yes (`Result`) |
| `E0808` | budget exhausted | yes (`Result`/`panic`) |
| `E0809` | type invariant broken by a backend | no (internal error) |
| `E0810` | IO error | yes (`Result`) |
| `E0811` | task failure (§9.7.3) | yes |
| `E0899` | `panic` | no |

### 6.9.3 Stack and tail calls

- A self tail call uses a trampoline and constant stack.
- Non-tail recursion has two bounds: the library default call-depth limit of 2048
  (`Interp::max_depth`); and, under the `aevia` CLI, which runs the interpreter on a thread
  stack of `--max-stack <MiB>` (default 1024), a depth limit raised to 10⁷ and replaced in
  practice by the **remaining stack** (an eighth of the stack, 8–64 MiB, is kept in reserve).
  Exceeding either is `E0805` with a call stack.
- A **non-terminating** tail loop is bounded by the consecutive-tail-call budget: default
  100,000,000 (`Interp::max_tail_calls`, changed by the CLI's `--budget`); exceeding it is
  `E0805`. This is runaway protection, not a semantic limit (§9.3.4).
- Expression nesting depth is also bounded (default 16,384 nodes) and exceeding it is `E0805`.
  A `panic` prints the call stack.

## 6.10 Determinism and parallelism

- Pure evaluation is fully deterministic (floating-point NaNs are canonicalised).
- `par_map(f, xs)` preserves input order and the thread count does not affect the result; it
  requires `Parallel a` (only `Int`/`Float` and their tensors), and symbolic `Real` does not
  run in parallel. When a value cannot cross threads it falls back to sequential evaluation
  with the same result (§9.7.2). The parallelism of `compute_all` affects speed only.
- `Set`/`Map` iteration order follows `TotalOrd`, not a hash seed. `random()` is IO; `rng(seed)`
  is pure and deterministic.

## 6.11 Module initialisation

- Top-level `let`s are evaluated **eagerly** in dependency order; mutually recursive top-level
  functions bind their names first and then evaluate their closures.
- `symbol`s are created in source order, so internal ids are deterministic. Unit, type and
  class declarations produce no run-time value (units excepted).
- `main` runs after all modules are initialised.

## 6.12 Equality, ordering and display

### 6.12.1 Equality

- Concrete `Int`/`Float` values compare numerically; `Float` follows IEEE (`NaN != NaN`,
  `-0.0 == 0.0`).
- `==` on `Real` returns `Prop`; for a concrete boolean use `decide(x == y)`, for approximate
  equality use `approx_eq`.
- Tensors: equal shape and element-wise equality (a `Real` tensor gives `Prop`); records and
  variants compare structurally; functions have no `Eq`; `Set` compares as a set.

### 6.12.2 Ordering

- `Int` is totally ordered; `String` by Unicode code point; records and tuples
  lexicographically by field.
- `Float`'s `compare` returns `Option<Ordering>` (`None` for NaN; `<`/`>` are `false` for NaN).
- `TotalOrd`'s `total_compare` sorts NaN last and serves `Set`/`sort`; `Float` itself is not
  `TotalOrd`.
- `Real` and tensors have no `Ord`; for tensors use `lexicographic` or `norm`.

### 6.12.3 Display

- `show(x)` is human-readable and includes the display unit; `repr(x)` parses back to the same
  value and shows internal ids of symbols; `to_latex` and `to_typst` typeset mathematics
  (single-letter symbols in italics, Greek names as letters, products with negative powers as
  `\frac`, elementary functions as upright operator names, `exp(u)` as `e^{u}`).
- A float prints in its shortest round-trip form and **always keeps its phase**: an
  integer-valued `Float` has a decimal point (`10.0`, `-0.0`) and large or small magnitudes use
  an exponent (`1e20`, `1e-9`); the exact `10` and the float `10.0` look different. An exact
  `Real` prints as a fraction or radical; a `Complex` prints as `a + b*i` with both parts
  following the `Float` rule.
- IEEE infinities print as `inf`/`-inf` and NaN as `nan`; `oo`/`-oo` denote only the exact
  infinities of the `Real` phase. `repr` writes a float infinity as `(1.0 / 0.0)`.
- A string shown on its own is its text; inside a container it is quoted: `show(Err("not
  found"))` is `Err("not found")` and `show(["a, b"])` is `["a, b"]`.
- Converting a `Float` with a display unit rounds only once (`(x - offset) / scale` is computed
  on exact rationals, then the nearest `f64` is taken): `to(°C, point(300.0, K))` displays as
  `26.85 °C`. The value itself is unchanged.
- Sums print in descending degree (`x^3 - 3*x + 1`) and `b^(1/2)` prints as `sqrt(b)`; terms
  returned by the kernel use Aevia's spelling (`e`, not `E`).

## 6.13 A worked example

```aevia
let f = fn t => t^3 - 3*t + 1
let df = diff(f)
let a = df(2)          # 9, exact
let b = approx(a)      # Ok(9.0)
```

`diff(f)` creates `ξ`, applies `f` to get `ξ^3 - 3ξ + 1`, rssn simplifies the derivative to
`3ξ^2 - 3` and a closure is returned; `df(2)` substitutes `ξ=2` and computes `9` exactly;
`approx(9)` gives `Ok(9.0)`. If `f` were `fn t => if t > 0 then t^2 else -t^2`, `t > 0` is a
`Prop` and the error is `E0302`; for a symbolic piecewise function use `by_cases`, which
builds an `ite` term, or use a numeric derivative at concrete points.

The next chapter defines modules, packages and visibility.
