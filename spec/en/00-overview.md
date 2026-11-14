# 00 — Overview: what Aevia is and why it is built this way

## 0.1 Definition

**Aevia is a pure functional language for scientific computing. Dimensions are part of the
type, units are compile-time scales, formulas are first-class values, and a numeric result
is a deliberate request for a different phase.**

It targets one kind of program: start from a physical law, a set of measurements or a
differential equation; derive symbolically, check dimensions, discretise numerically,
visualise; end with a reproducible result. Such programs are usually assembled from Python,
NumPy, SymPy and Pint: dimension checking lives in a library, symbolic and numeric work use
two APIs, and unit errors tend to surface only at run time. Aevia puts these in one language
and lets the compiler reject dimension errors at the cheapest moment — type checking.

```aevia
module Kinematics

open Units.SI
import Math { sqrt }

let g = 9.80665 m/s^2

fn fall_time(h: Float[L]): Float[T] =
    sqrt(2 * h / g)

fn main(): IO Unit = do
    let h = 20.0 m
    println("fall time from {h} = {fall_time(h)}")
```

There are no casts, unit strings or `Quantity` wrappers. `h : Float[L]` and
`g : Real[L/T^2]`; dimension algebra gives `2 * h / g` the dimension `T^2`, and `sqrt` halves
the exponent, yielding `Float[T]`. Write `h + g` and the program does not run.

## 0.2 Goals

In priority order:

1. **Dimensional correctness by construction.** Addition, subtraction and comparison require
   equal dimensions; multiplication, division and powers combine them by dimension algebra;
   transcendental functions require dimensionless arguments. A dimension error is a type
   error, not a run-time warning.
2. **Symbolic and numeric work are two phases of one language.** The same expression can be
   simplified to closed form or evaluated at a point.
3. **Code that reads like mathematics.** Unit literals such as `9.81 m/s^2`, Unicode
   identifiers such as `Δt`, infix operators, the pipe `|>`, pattern matching.
4. **A pure core with explicit effects.** No mutable state, no implicit IO. Effects are
   confined to `IO`, so pure functions can be parallelised, cached, differentiated and
   substituted freely.
5. **Predictable performance.** Strict evaluation; a tensor's rank is in its type and its
   shape is known at run time; numeric hot spots go down to rssn's kernels and JIT; symbolic
   search budgets are explicit.
6. **A small core and a thick standard library.** The language has functions, algebraic data
   types, records, type classes, modules and `IO`; mathematics, units, linear algebra,
   statistics, ODE/PDE and plotting live in the library.
7. **Errors that teach.** A dimension error names the dimensions on both sides and suggests a
   likely fix (appendix B).

## 0.3 Non-goals

- **Not a systems language.** No manual memory management, no `unsafe`, no pointer
  arithmetic; low-level performance comes from rssn's kernels.
- **Not a proof assistant.** Dimension types are equations over an abelian group, not full
  dependent types. Array shapes enter the type as refinements, and type-level `Nat` supports
  linear arithmetic only (§13.9.1).
- **No attempt at a Python-sized ecosystem.** Data is exchanged through CSV/NPY/JSON, the C
  ABI, plugins and the registry; Python is not embedded.
- **No user-defined syntax, macros or effect handlers.** The extension points are RSSN
  operators, plugins, `Rssn` dynamic calls and the package registry, which keeps tooling,
  formatting and error messages uniform (§13.9.6, §13.9.7).
- **No automatic parallelism.** Parallelism must be requested explicitly with primitives such
  as `par_map`; devices, GPUs and distribution are run-time targets, not type parameters
  (§13.9.4).

## 0.4 Design principles

### 1. Dimensions are types; units are not

- A **dimension** lives in the type: `L`, `T`, `M*L^2/T^2`. It decides which operations a
  value may take part in.
- A **unit** is a compile-time constant on the value side: `m`, `km`, `degC`. It supplies a
  scale (and an affine offset) and is folded away during compilation.

So `3 km` and `3000 m` have the **same type**, `Real[L]`, and are the same value at run time;
`to(km, x)` changes only the display unit. Type equality is therefore dimension equality,
there is no combinatorial explosion from "metres and centimetres are different types", and
the unit table can grow without touching the type checker. The price is that the type system
cannot tell a length in metres from a length in feet. This is deliberate: errors in scientific
code are almost always dimension errors, not unit choices within one dimension.

### 2. Exact and approximate are two types, not two modes

| Type | Meaning |
|---|---|
| `Int` | arbitrary-precision integer, dimensionless |
| `Real[d]` | exact real or symbolic expression of dimension `d`: `1/3`, `sqrt(2)`, `x^2 + 1` |
| `Float[d]` | IEEE-754 binary64 of dimension `d`: `1.5` |
| `Complex[d]` | complex scalar |

Integer literals are `Int`, decimal literals are `Float`, and `1/10` is an exact `Real`. In
mixed arithmetic **`Real` absorbs `Float`** (exact wins) with warning `W0302`: `1/3 + 0.5` is
`5/6`. The language never converts `Real` to `Float` where you did not write `Float`, so
`sin(1)` stays symbolic as `sin(1)` while `sin(1.0)` yields `0.8414709848078965`. The author
must say whether they are deriving exactly or computing numerically (rationale: §12.8.9).

### 3. Pure, but allowed to look imperative

There are no mutable variables, no `for` loops and no in-place updates. Iteration is
recursion with guaranteed tail calls; array updates are copies semantically and happen in
place at run time when ownership is unique. `do` blocks serve `IO` only; they are not general
monad syntax.

### 4. Symbolic computation is a value, not a macro

There is no `quote`/`unquote`. `symbol("x")` returns a `Real`, and everything you can do to
it — add, multiply, differentiate, substitute — is what you can do to any `Real`.
`diff(fn x => x^2)(3 m)` has type `Real[L]`, because differentiation divides by the
dimension of the variable. "Derive the formula, then substitute data" is the default workflow.

### 5. Yield on convenience, never on soundness

Aevia uses Hindley–Milner inference with type classes and dimension algebra rather than full
dependent types. Users need not annotate `mean(xs)`, yet can know its dimension without
running anything; dimension equations are linear over an abelian group, so inference is
decidable and linear. Where static types cannot express a program (say `x^n` with a run-time
integer `n`), the language does not invent an unsound type: it offers the explicit dynamic
escape `Dyn`, deferring the check to run time. The static path stays sound; the dynamic path
stays usable.

### 6. One registry, reflection first

Aevia does not hand-write RSSN operator bindings or keep a second unit table. `OpTable` is the
only operator registry and `rssn::dim` the only dimension/unit layer; Aevia reads the Kernel
Manifest and generates the `Rssn` module (typed plus `dynamic`). New operators, units and
plugins need no compiler change.

## 0.5 A ten-minute tour

Every example below runs; the outputs were checked against the implementation.

### Dimensions and units

```aevia
let d = 100 m                 # Real[L]
let t = 9.58 s                # Real[T]
let v = d / t                 # Real[L/T]
let E = 70 kg * v^2 / 2       # Real[M*L^2/T^2]

let bad = d + t               # error[E0401]: dimension mismatch
```

Names such as `Energy` are only print aliases for `M*L^2/T^2`, not new types.

### Dimension polymorphism

```aevia
fn mean<d>(xs: Tensor<1, Real[d]>): Real[d] =
    sum(xs) / length(xs)

fn variance<d>(xs: Tensor<1, Real[d]>): Real[d^2] =
    let m = mean(xs)
    sum((xs - m)^2) / length(xs)
```

`mean` works for any dimension, and the dimension of `variance` is the square of the input's.
Dimensions are erased at run time; no code is generated per dimension.

### Symbolic and numeric

```aevia
let x = symbol("x")                   # Real, dimensionless by default
let df = diff(fn t => t^3 - 3*t + 1)  # the derivative, as a function
let slope = df(x)                     # 3*x^2 - 3
let y = approx(df(2))                 # Ok(9.0)
```

`diff` takes a function and returns its derivative: internally it creates a symbol of the
same dimension, applies the function, differentiates and substitutes back, so it works with
higher-order functions, partial application and user-defined functions alike.

### Tensors

```aevia
let M = [[1.0, 2.0], [3.0, 4.0]]
let b = [5.0, 6.0]
let x = match linsolve(M, b) with
        | Ok(v) -> v
        | Err(_) -> [0.0, 0.0]        # illustration; handle the error in real code
let y = M @ x                         # ≈ b

let R = [[1.0, 2.0], [3.0, 4.0]] * ohm
let I = [0.1, 0.2] * A
let V = R @ I                         # [0.5 V, 1.1 V]
```

`@` is matrix multiplication and `*` is element-wise (scalars broadcast), as in NumPy.

### Variants and pattern matching

```aevia
type Shape =
    | Circle(Real[L])
    | Rect(Real[L], Real[L])

fn area(s: Shape): Real[L^2] =
    match s with
    | Circle(r) -> pi * r^2
    | Rect(w, h) -> w * h
```

### A pure core and IO

```aevia
fn main(): IO Unit = do
    r <- read_csv<{ time: Float[T], velocity: Float[L/T] }>("data.csv")
    match r with
    | Err(e) -> println("cannot read: {e}")
    | Ok(xs) ->
        match (column(xs, "time"), column(xs, "velocity")) with
        | (Ok(t), Ok(v)) ->
            println("mean speed = {mean(v)}")
            plot(combine([label(line(t, v), "v(t)")]))
        | _ -> println("column type mismatch")
```

`read_csv`, `plot` and `println` are `IO` actions; `mean` is a pure function; `main` is the
only entry point the runtime calls.

## 0.6 Language and backend layers

```
┌──────────────────────────────────────────────────────────────┐
│ Aevia source (.aev)                                          │
│  lex / layout → syntax tree → name resolution →              │
│  type + dimension checking → core IR                         │
└───────────────────────────┬──────────────────────────────────┘
                            │ pure values, closures, tensors, symbolic scalars
┌───────────────────────────▼──────────────────────────────────┐
│ Aevia runtime                                                │
│  interpreter · tensor runtime · symbolic bridge ·            │
│  budget/cache · REPL · LSP                                   │
└───────────────────────────┬──────────────────────────────────┘
                            │ Term / Config / compute / kernels
┌───────────────────────────▼──────────────────────────────────┐
│ rssn                                                         │
│  e-graph rewriting · dimension inference · numeric kernels · │
│  JIT · simulation scenarios                                  │
└──────────────────────────────────────────────────────────────┘
```

Aevia owns syntax, type and dimension checking, closures and records, tensor shapes, modules,
IO and the orchestration of the standard library. rssn owns the representation and rewriting
of symbolic expressions, the run-time part of dimension algebra, numeric algorithms, linear
algebra, FFT, ODE/PDE, special functions, the JIT and physical simulation. The boundary is the
**Term**: a `Real` or `Complex` scalar that needs symbolic power holds an rssn term, while
`Float` scalars and numeric tensors take a native fast path and never enter the term graph.

The language's dimension check is **compile-time**; rssn's dimension layer is **run-time and
term-level**. They share one set of base dimensions and one exponent algebra: the former
rejects bad programs, the latter keeps simplification correct and reports diagnostics. For
what rssn provides and how it is called, see chapters 10 and 13.

## 0.7 Conventions

- Blocks tagged `aevia` are Aevia source; blocks tagged `text` are grammar, derivations or
  output.
- Typing judgements are written `e : τ`; dimensions are `d`, `e`; the type environment is
  `Γ`. In `Real[d]`, `d` is a dimension expression; `<d>` declares a dimension parameter.
- The words *must / must not / should / may* carry the force of RFC 2119 MUST / MUST NOT /
  SHOULD / MAY (see the [README](README.md)).
- `...` in an example means elision, not valid syntax.

The next chapter starts at the bottom: the characters, tokens and layout rules that make up
an Aevia source file.
