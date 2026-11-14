# Aevia Language Specification

**Single-file edition · Revision 23**

> Generated from the chapters under `spec/en/` by `tools/bundle_spec.py`.

---

## Contents

1. [Aevia Language Specification](#ch-index)
2. [00 — Overview](#ch-00-overview)
3. [01 — Lexical structure](#ch-01-lexical)
4. [02 — Syntax](#ch-02-syntax)
5. [03 — Type system](#ch-03-types)
6. [04 — Dimensions](#ch-04-dimensions)
7. [05 — Units](#ch-05-units)
8. [06 — Evaluation semantics](#ch-06-semantics)
9. [07 — Modules, packages and visibility](#ch-07-modules)
10. [08 — Standard library](#ch-08-stdlib)
11. [09 — Runtime](#ch-09-runtime)
12. [10 — The rssn backend mapping](#ch-10-backend)
13. [11 — Complete examples](#ch-11-examples)
14. [12 — Design trade-offs, limits and roadmap](#ch-12-rationale)
15. [13 — Extensibility and the rssn integration](#ch-13-extensibility)
16. [Appendix A — Complete grammar](#ch-appendix-a-grammar)
17. [Appendix B — Diagnostic catalogue](#ch-appendix-b-diagnostics)
18. [Appendix C — Revision log and known implementation limits](#ch-appendix-c-changelog)

---

<a id="ch-index"></a>

# Aevia Language Specification

**Aevia — Design Standard**

Aevia is a pure functional language for scientific computing in which **dimensions are part of the type system**. A quantity is not a number with a run-time tag; it is a value whose type records its physical dimension. The compiler rejects `3 m + 2 s`, `sin(3 kg)` and `exp(1 m)` before any code runs.

The language is built on the `rssn` computer-algebra and scientific-computing engine. Symbolic and numeric evaluation are two phases of the same expression; units, dimensional inference, rewriting, numeric kernels, JIT compilation and physics simulation are provided by the backend. Aevia keeps no second operator list or unit table: it consumes rssn's **Kernel Manifest** and `rssn::dim` registry and generates its `Rssn` module from them. See [13 — Extensibility](#ch-13-extensibility) and rssn's `docs/KERNEL.md`.

> This document is the **normative design**: the grammar is complete, the typing rules are stated, the run-time architecture is fixed, and every backend dependency is identified. Examples and rationale are informative.

---

## Reading guide

| Chapter | Contents | Normative |
|---|---|---|
| [00 — Overview](#ch-00-overview) | Goals, non-goals, principles, ten-minute tour | Yes (goals) |
| [01 — Lexical structure](#ch-01-lexical) | Source encoding, literals, units, layout | Yes |
| [02 — Syntax](#ch-02-syntax) | Declarations, expressions, patterns, modules | Yes |
| [03 — Type system](#ch-03-types) | Kinds, types, inference, classes, effects | Yes |
| [04 — Dimensions](#ch-04-dimensions) | Dimension algebra, typing rules, polymorphism, `Dyn` | Yes |
| [05 — Units](#ch-05-units) | Unit declarations, prefixes, affine points, constants | Yes |
| [06 — Semantics](#ch-06-semantics) | Evaluation, values, symbolic/numeric phases, tensors | Yes |
| [07 — Modules, packages and visibility](#ch-07-modules) | Namespaces, visibility, packages, tests | Yes |
| [08 — Standard library](#ch-08-stdlib) | Prelude and library signatures | Mostly yes |
| [09 — Runtime](#ch-09-runtime) | Pipeline, interpreter, REPL, CLI, LSP, performance | Yes (architecture) |
| [10 — The rssn backend mapping](#ch-10-backend) | Mapping to rssn, bridge design, gaps and risks | Yes |
| [11 — Complete examples](#ch-11-examples) | Worked programs from kinematics to simulation | No |
| [12 — Design trade-offs, limits and roadmap](#ch-12-rationale) | Alternatives, trade-offs, design boundaries, implementation progress | No |
| [13 — Extensibility and the rssn integration](#ch-13-extensibility) | rssn reflection, generated `Rssn`, plugins, registry, shape/device/sparse decisions | Yes |
| [Appendix A — Grammar](#ch-appendix-a-grammar) | Complete EBNF | Yes |
| [Appendix B — Diagnostics](#ch-appendix-b-diagnostics) | Error codes and message format | Yes |
| [Appendix C — Changelog](#ch-appendix-c-changelog) | Revision history and known gaps | Informative |

The backend interface contract is in the rssn repository's `docs/KERNEL.md` (Kernel Manifest, dynamic-call ABI, plugins, units and dimensions) and `docs/DIMENSIONS.md`; it is a precondition of this specification, not an optional implementation detail. The division of the implementation into crates is in [ARCHITECTURE.md](https://github.com/Apich-Organization/aevia/blob/main/ARCHITECTURE.md) at the repository root. Every chapter also has a Chinese edition (the source text); authoring and terminology rules are in `spec/STYLE.md`.

## Conformance language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHOULD**, **SHOULD NOT** and **MAY** are to be interpreted as described in RFC 2119. A conforming implementation MUST accept every program in this specification's test corpus, MUST reject every program marked ill-typed, and MUST produce the specified diagnostic category (the exact wording is not normative).

## Design status

This is a **design standard**, not a frozen implementation. Chapters marked normative are intended to be implemented as written; [appendix C](#ch-appendix-c-changelog) records every revision and the known gaps. The current text is **Revision 23**, aligned with **`rssn 3.0.0-rc.1`** (`api_version = 2`). The size of the manifest is a measurement, not a constant; the live output of `aevia rssn` is authoritative (at that version: 1620 operators, 82 units, 30 constants). Dispatch and extension follow the boundary stated in chapter 13: no run-time multimethods, no multi-parameter classes, no per-type monomorphisation; instances are selected statically or by canonical witnesses.

There is **no v1/v2 language split**: every capability is designed in; a backend that does not yet implement one reports a capability diagnostic (`E1001`) instead of removing the language feature.

## A one-sentence summary

> Aevia makes the dimension of a quantity a type, the unit a compile-time scale, the formula a first-class value, and the numeric result a deliberate request.

## Quick taste

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

```
$ aevia run kinematics.aev
fall time from 20.0 m = 2.0196199771025523 s
```

The next chapter explains why this program type-checks, and what the language does when it does not.


<a id="ch-00-overview"></a>

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
  SHOULD / MAY (see the [README](#ch-index)).
- `...` in an example means elision, not valid syntax.

The next chapter starts at the bottom: the characters, tokens and layout rules that make up
an Aevia source file.


<a id="ch-01-lexical"></a>

# 01 — Lexical structure

This chapter defines how a source file is split into tokens. Syntax is chapter 02. The
semantics of unit literals is chapter 05; this chapter defines only how they are recognised.

## 1.1 Source files and encoding

- The extension is `.aev`; the encoding is UTF-8. A BOM is allowed and ignored.
- `\n`, `\r\n` and `\r` are all normalised to `\n` before tokenising.
- Identifiers (including keywords and unit names) are compared in Unicode NFC: a precomposed
  `é` and `e` plus a combining accent are the same name. Strings and comments are kept as
  written.
- The first line may be a shebang (`#!` to end of line); it is ignored.
- A tab MUST NOT appear in leading indentation (`E0101`). Tabs in strings and comments are
  unrestricted.

## 1.2 Tokens

```
token ::= IDENT | UIDENT | KEYWORD | INT | FLOAT | STRING
        | UNIT_NAME | SUPERSCRIPT | LT_APP | ELLIPSIS
        | OP | LPAREN | RPAREN | LBRACKET | RBRACKET | LBRACE | RBRACE
        | COMMA | SEMICOLON | COLON | DOT | DOTDOT | DOTDOTEQ
        | ARROW | FATARROW | LARROW | PIPE | EQUALS
        | NEWLINE | INDENT | DEDENT | EOF
```

Whitespace separates tokens, and newlines also take part in layout (§1.4). Apart from unit
literals there is no implicit multiplication by juxtaposition: `3 x` is not a product and
`f x` is not a call. Calls are written `f(x)` and products `3 * x`.

## 1.3 Comments

```aevia
# line comment
let x = 1  # trailing comment

#- block comment
   #- may nest -#
   still inside -#
```

- `#` starts a line comment. `#-` … `-#` is a nestable block comment; an unclosed one is
  `E0104`.
- `##` starts a doc comment (Markdown). It must begin a line (indentation allowed) and attaches
  to the next declaration.
- Comments produce no tokens.

## 1.4 Layout

Aevia uses the offside rule: blocks are delimited by indentation alone; there is no `end`.

### 1.4.1 INDENT / DEDENT

The lexer keeps an indentation stack, initially `[0]`. For each **logical line** (outside
brackets, not blank, not comment-only):

1. Count the leading spaces `n`.
2. `n >` top: push `n` and emit `INDENT`.
3. `n ==` top: emit nothing.
4. `n <` top: pop, emitting one `DEDENT` per pop, until top `== n`; if the stack empties
   first, report `E0102`.
5. Emit `NEWLINE` at the end of the line.

Blank and comment-only lines do not affect indentation and emit no `NEWLINE`.

### 1.4.2 Free layout inside brackets

Inside `(` `[` `{` no `NEWLINE`/`INDENT`/`DEDENT` is produced, and a trailing comma is
allowed:

```aevia
let A = [[1.0, 2.0],
         [3.0, 4.0]]

let B = [
    1.0, 2.0,
    3.0, 4.0,
]
```

### 1.4.3 Continuation

Outside brackets nothing suppresses the line end: every line ends with `NEWLINE`, and a
trailing `+` does not join the next line (`1 +` followed by a newline is `E0202`). A
multi-line expression MUST be parenthesised:

```aevia
let energy = (m * c^2
            + 1/2 * m * v^2
            - m * g * h)
```

When a line ends with a block-introducing token (`=`, `then`, `else`, `->`, `=>`, `do`,
`with`) and the next line is indented, the parser reads `NEWLINE INDENT expr DEDENT` as a
**layout group**. Layout groups are used for function and `let` bodies, `if` branches, `match`
arm bodies and `do` statement bodies. This keeps the lexer ignorant of which tokens are
operators.

### 1.4.4 Blocks

A block is also `NEWLINE INDENT … DEDENT`, containing a sequence of `let`/`fn` declarations and
a final expression; its value is the final expression's value, and a block of only `let`s is
`E0201`. After `NEWLINE INDENT` the parser chooses a block if the first token is `let` or `fn`,
and a layout group otherwise:

```aevia
fn mean<d>(xs: Tensor<1, Real[d]>): Real[d] =
    let n = length(xs)
    sum(xs) / n
```

A position that requires an indented block but has none is `E0103`. In a multi-line `if`,
`else` MUST be in the same column as `if`. The `|` arms of one `match` MUST share a column,
either the column of `match` or one level deeper (§2.3.4).

## 1.5 Identifiers and keywords

### 1.5.1 Identifiers

```
ident_start    ::= XID_Start | "_"
ident_continue ::= XID_Continue | "'" | "₀".."₉"
```

- Unicode letters (`Δt`, `λ`), primes (`f'`) and subscript digits (`x₁`) are allowed; names
  are case-sensitive.
- A name starting with an ASCII capital is a `UIDENT` (types, constructors, modules,
  dimensions); all others are `IDENT`. So `Δt` is a value name, not a constructor.
- A lone `_` is the wildcard. As a call argument it is a placeholder (§2.3.6). It is not a
  binding name.

### 1.5.2 Keywords

```
as       class     derive    dimension  do       else     false
fn       for       if        import     in       instance let
match    module    open      private    rec      test     then
true     type      unit      where      with
```

A keyword cannot be used as an identifier (`E0206`). `forall` is a contextual keyword: it is
a quantifier only at the start of a type, followed by type parameters and `.`, and it is also
a standard-library function name (§8.13). `Int`, `Real`, `Float`, `Bool` and the like are
treated specially only in type position; they are not keywords. `in` belongs to `let … in`,
`for` to `instance … for` and `derive … for`, and `rec` to `let rec`. `pure` is a Prelude
function, not a keyword (§9.7.3).

> **Status:** backtick identifiers (naming an external item that collides with a keyword) are
> not implemented; a backtick is `E0105`.

### 1.5.3 Naming conventions (informative)

Types, constructors, modules and dimensions use `UpperCamelCase`; functions, values and fields
use `snake_case`; type parameters are lower-case `a b`, dimension parameters `d e`; unit names
keep their international spelling (`m`, `kg`, `Pa`, `°C`).

## 1.6 Numeric literals

```
int_lit   ::= dec_int | "0x" hexdigit {["_"] hexdigit}
            | "0o" octdigit {["_"] octdigit} | "0b" bindigit {["_"] bindigit}
dec_int   ::= digit { ["_"] digit }
float_lit ::= dec_int "." dec_int [ exponent ] | dec_int exponent
exponent  ::= ("e" | "E") ["+" | "-"] dec_int
```

- Underscores only group digits; they cannot lead, trail or repeat (`E0106`). `1_000` is
  valid; `1__0` and `1_` are not.
- A decimal integer is an `Int`; a literal with a point or exponent is a `Float`. `0755` is
  decimal 755. There are no numeric suffixes, and `1.` and `.5` are invalid.
- `oo`, `-oo` and `nan` are predefined constants, not literals. `i` is the predefined imaginary
  unit; write a complex number as `3 + 4 * i`.
- `0` is not dimension-polymorphic: it defaults to `Int`. Write zero metres as `0 m` and a
  floating zero as `0.0` (§4.7).

## 1.7 String literals

```aevia
"hello"
"value = {x + 1}"
"line\nbreak"
r"C:\data\raw.csv"          # raw string: no escapes, no interpolation
"""
multi-line string
  keeps relative indentation
"""
```

- Strings are delimited by double quotes; a single quote is not a string delimiter (`E0105`).
- Escapes: `\n \t \r \\ \" \{ \} \u{1F600}`. An unknown escape is `E0105`.
- Interpolation `{expr}`: the expression's type MUST implement `Show`. An empty `{}` is
  `E0104`. A literal brace is written `\{`.
- A raw string `r"…"` processes neither backslashes nor braces and cannot contain `"`.
- A multi-line string `"""…"""` drops the leading newline and the common indentation of its
  lines; interior newlines are kept.
- A string is an immutable UTF-8 sequence; indexing counts grapheme clusters (§8.1.4).

## 1.8 Unit literals

```aevia
9.80665 m/s^2
3.0e8 m/s
1.5 kg*m^2/s^3
25 °C
50 %
2.5 µm
```

### 1.8.1 Recognition

**An identifier sequence immediately after a numeric literal is always a unit literal, never a
product.**

```aevia
3 m          # a quantity: 3 metres
3 * m        # the value 3 times the unit value m; display unit may differ
3 x          # a unit literal; E0501 if x is not a unit
```

The rule is context-free: neither lexer nor parser needs to know which names are units. Name
resolution then checks that `x` lives in the unit namespace, so unit declarations may come
later and the parser never backtracks.

```
quantity_lit ::= number unit_expr
unit_expr    ::= unit_factor { ("*" | "·" | "/") unit_factor }
unit_factor  ::= unit_atom [ "^" exponent | SUPERSCRIPT+ ]
unit_atom    ::= unit_name | "(" unit_expr ")" | "%"
unit_name    ::= UNIT_NAME | IDENT | qname
exponent     ::= ["-"] integer | "(" rational ")"
```

- `number` is an `INT` or `FLOAT` of §1.6. A unit literal starts when the numeric literal is
  immediately followed by an `IDENT`, a `UNIT_NAME` (`°C`), `%` or `(`.
- `*` and `·` are synonyms; `/` is left-associative: `kg*m^2/s^3` = `(kg*m^2)/s^3`.
- An exponent MUST be an integer literal or a parenthesised rational: `m^2`, `s^-1`,
  `m^(1/2)`. Variable exponents are not allowed (use `Dyn`, §4.9). Superscripts `m²`, `s⁻¹`,
  `m³·s⁻²` are equivalent to `^n` (`E0107`: invalid sequence).
- Juxtaposition inside a unit expression is not allowed: `kg m^2` is not `kg*m^2`.
- **Inside a unit expression, `*` `·` `/` MUST touch both neighbours.** A `*` or `/` with
  whitespace on either side ends the unit literal and is ordinary arithmetic:
  `9.81 m/s^2 * mass` = `(9.81 m/s^2) * mass`; `70 kg * v^2 / 2` = `((70 kg) * v^2) / 2`;
  `3 m * x` = `(3 m) * x`. The tight `3 m*x` is still a unit expression and is `E0501` when `x`
  is not a unit. The formatter always prints units tight and arithmetic operators spaced, so
  formatting never changes meaning.
- Numbers are not allowed inside a unit expression: `3 m/2 s` is `(3 m) / (2 s)`; `1/2 m` is
  `1 / (2 m)` with type `Real[1/L]`; write "half a metre" as `(1/2) m`.
- Parentheses are for units only: `3 (m/s)` is valid; `3 (2)` is `E0108`; a malformed unit in
  parentheses such as `3 ()` is `E0209`.

### 1.8.2 Unit names and qualification

- Unit names are resolved in the **unit namespace**, separate from values: `let s = 5` does not
  affect `3 s`; to multiply by the variable write `3 * s`.
- A name may carry a prefix (`km`, `MHz`, `µs`), matched longest-first (§5.3), and is
  case-sensitive (`Pa` ≠ `pa`).
- A name may be qualified: `3 Units.SI.km` is valid; `3 p.x` is not (`p` is not a unit).
- `%` is a unit, not a modulo operator: `50%` and `50 %` are both the `Real[1]` value 0.5;
  `50 % 3` is a syntax error. Use `mod(a, b)` or `rem(a, b)` for remainders.

### 1.8.3 Superscripts and special characters

The superscript characters `⁰¹²³⁴⁵⁶⁷⁸⁹ ⁻ ⁺` may only follow a unit name; `x²` is not an
expression — write `x^2`.

| Written | Meaning |
|---|---|
| `°C` `°F` | Celsius, Fahrenheit (affine, §5.6) |
| `°` alone | the angle unit `deg` |
| `Ω` / `Å` | `ohm` / `angstrom` |
| `µ` (U+00B5), `μ` (U+03BC) | both are the micro prefix, the same prefix |

### 1.8.4 Type and value (preview)

| Literal | Default type | Value |
|---|---|---|
| `3 m` | `Real[L]` (instantiates to `Float[L]` where one is expected) | 3 (coherent SI) |
| `3.0 m` | `Float[L]` (may be exact where `Real[L]` is expected) | 3.0 |
| `25 °C` | `Point[Θ]` | 298.15 K (§5.6) |
| `1.5 kg*m^2/s^3` | `Float[M*L^2/T^3]` | 1.5 |
| `50 %` | `Real[1]` (`Float[1]` where a `Float` is expected) | 0.5 |

The display unit (`m`, `km`, `°C`) is not part of the type. It is metadata on the value, and
`to(unit, x)` changes it (§5.7).

## 1.9 Operators and punctuation

Precedence and associativity are in §2.6.

| Token | Meaning |
|---|---|
| `+ - * /` `@` `^` | arithmetic, matrix product, power |
| `== != < <= > >=` | comparison |
| `&&` `\|\|` `!` | logical and, or, not; postfix `!` is the factorial (§2.7.5) |
| `\|>` `<\|` | pipe, reverse application |
| `::` | list cons |
| `..` `..=` `...` | half-open and closed ranges (index position only), record row variable |
| `->` `=>` `<-` `=` `:` | function type / match arm, lambda, `do` bind, definition, annotation |
| `?` `√` | postfix error propagation, prefix square root |

Synonyms are normalised to ASCII tokens in the lexer, keeping the source position for
diagnostics and formatting: `·` `×` = `*`, `÷` = `/`, `−` = `-`, `≤` `≥` `≠` = `<=` `>=`
`!=`. `%` appears only inside unit literals.

## 1.10 Lexical errors

| Code | Situation |
|---|---|
| `E0101` | tab in indentation |
| `E0102` | inconsistent indentation (no matching level for a DEDENT) |
| `E0103` | indented block required but absent |
| `E0104` | unclosed string, block comment or bracket; empty `{}` interpolation |
| `E0105` | invalid character or unknown escape |
| `E0106` | invalid numeric literal (underscore placement, no digits) |
| `E0107` | invalid superscript sequence |
| `E0108` | missing unit after a numeric literal |

A lexical error MUST report an exact line and column range and SHOULD suggest a fix.


<a id="ch-02-syntax"></a>

# 02 — Syntax

This chapter defines the syntax of declarations, expressions, patterns and types, and explains
how the ambiguous places are parsed. The full EBNF is in [appendix A](#ch-appendix-a-grammar)
and is not repeated here.

## 2.1 Structure of a source file

```aevia
module Kinematics            # optional; must be the first declaration

import Units.SI as SI        # qualified access
import Math { sqrt, pi }     # selective open
open Units.SI                # open units and constants

dimension Currency           # a user base dimension
unit EUR : Currency = 1      # a unit declaration

type Vec2 = { x: Real[L], y: Real[L] }

fn fall_time(h: Float[L]): Float[T] = sqrt(2 * h / g)
```

A file is one module. The order of top-level declarations does not affect name resolution
(unit names are resolved after all declarations are parsed). A top level may contain `module`
(at most once, and first), `import`/`open`, `dimension`/`unit`,
`type`/`class`/`instance`/`derive`, `fn`/`let` and `test`. `private` before `type`,
`dimension`, `unit`, `fn`, `let` or `class` keeps the declaration out of the export list;
everything else is exported. The operand of `open` is a module path, not an alias introduced
by `import … as` (§7.2).

## 2.2 Declarations

### 2.2.1 Functions

```aevia
fn kinetic_energy(m: Real[M], v: Real[L/T]): Real[M*L^2/T^2] =
    m * v^2 / 2

fn norm((x, y): (Real[L], Real[L])): Real[L] = sqrt(x^2 + y^2)
```

- Parameters may carry type annotations; when omitted they are inferred from the body and the
  call sites. A parameter may be a pattern.
- The return type goes after `)` and before `=`. The body is one expression; across several
  lines it is a block (§1.4.4).
- A function declaration is recursive: `fn f(x) = e` is sugar for
  `let rec f = fn (x) => e`.

### 2.2.2 Values

```aevia
let g = 9.80665 m/s^2
let (x, y) = (1.0 m, 2.0 m)
let rec fact = fn (n: Int) => if n == 0 then 1 else n * fact(n - 1)
```

- A plain `let` is not recursive: in `let x = x + 1` the right-hand `x` refers to an outer
  binding, and is `E0301` if there is none. `let rec` makes the name visible in its own
  definition.
- Top-level and local `let` behave the same. The pattern MUST match the type of the right-hand
  side (`E0308`); a constructor pattern binding (`let Some(x) = e`) is treated as irrefutable.

### 2.2.3 Types

```aevia
type Vec2 = { x: Real[L], y: Real[L] }

type Shape =
    | Circle(Real[L])
    | Rect(Real[L], Real[L])

type Result<a, e> = | Ok(a) | Err(e)
```

- `type Name = T` is a **transparent alias**.
- `type Name` (no `=`) is an **abstract type**; its constructors and operations come from the
  standard library or a plugin.
- `type Name = | C1(…) | C2(…)` is a **nominal variant type**; constructors are
  values/functions. A one-line variant list MUST start with `|` to distinguish it from an alias
  (`type T = A(Int) | B` is `E0202`); the indented form may omit the first `|`.
- A recursive occurrence must be to the right of a function arrow or inside a record field,
  tuple or constructor argument (positivity, §3.6.4).
- Constructor names are visible module-wide; qualify with `Module.Ctor` when two modules use
  the same name.

### 2.2.4 Classes and instances

```aevia
class Show a where
    show(x: a): String

instance Show for Real[d] where
    show(x) = format_quantity(x)

derive Eq, Show for Vec2
```

- A class has at least one type parameter. Methods have signatures only; default
  implementations come from `derive` or a plugin.
- An instance head is `instance Class for Type`; the `d` in `Real[d]` is implicitly
  universally quantified. Coherence and the orphan rule are in §7.4.
- `derive` generates the usual instances for records and variants.

### 2.2.5 Units and dimensions

```aevia
dimension Currency
unit USD : Currency = 1
unit EUR : Currency = 1.08 USD
unit m : L = 1 prefixable
unit degC : Θ = 1 K offset 273.15 K
```

Syntax and semantics are in chapter 05.

### 2.2.6 Tests

```aevia
test "fall time from 20 m" =
    approx_eq(fall_time(20.0 m), 2.019 s, 0.001 s)

test "energy is positive" =
    forall(fn (v: Real[L/T]) => kinetic_energy(1 kg, v) >= 0 J)
```

The name is a string and the body is a `Bool` (or `Prop`, §8.13) expression; `aevia test`
collects and runs them. `forall` is a standard-library function, not a keyword, and its
predicate takes one argument.

### 2.2.7 Attributes

```aevia
@stable
@deprecated("use new_api")
fn old_api(x: Real[1]): Real[1] = ...

@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))
```

- Known attributes: `@stable`, `@experimental`, `@deprecated` (stability, §13.8); `@jit`
  (request JIT; `E1001` if the backend lacks `core.jit`); `@op` and `@rule` (register an RSSN
  operator and a rewrite rule, §13.5); `@bench` (§9.12).
- An unknown attribute is `E0210`. Attribute arguments MUST be compile-time constants.

## 2.3 Expressions

### 2.3.1 Blocks

A block is a sequence of `let`/`fn` declarations followed by one final expression (§1.4.4). A
`let` is in scope to the end of the block.

```aevia
let y =
    let a = 1
    let b = 2
    a + b
```

### 2.3.2 `let … in`

```aevia
let x = let y = 2 in y^2 + 1
```

`let … in` is the non-recursive one-line form; for recursion use `let rec` or `fn` in a block.

### 2.3.3 `if`

```aevia
let s = if x > 0 then "positive" else "non-positive"
```

`if` is an expression and MUST have an `else`; `else if` chains. The two branches MUST have the
same type, dimension included. In a multi-line `if`, `else` is in the column of `if`
(§1.4.4).

### 2.3.4 `match`

```aevia
fn area(s: Shape): Real[L^2] =
    match s with
    | Circle(r) -> pi * r^2
    | Rect(w, h) -> w * h

fn sign(x: Real[1]): Int =
    match x with
    | 0 -> 0
    | n if n > 0 -> 1
    | _ -> -1
```

- Arms are tried in order. A guard `if` is evaluated after the pattern matches and MUST be
  `Bool`. An arm body may be a block.
- A non-exhaustive match is `E0309`; a redundant arm is warning `W0201`. Exhaustiveness
  analysis is in §2.4.
- Three layouts: arms indented below `match`; arms in the same column as `match`; or **inline**
  after `with`. Brackets contain no `NEWLINE`, so
  `println(match o with | Some(v) -> v | None -> 0)` can only be written inline. An inline arm
  body ends at the next `|` or where the enclosing expression ends; a nested inline `match`
  swallows the following arms, so parenthesise it. The formatter prints the inline form inside
  expressions and the block form at line level.

### 2.3.5 Lambdas

```aevia
let square = fn (x: Real[L]) => x^2
let add = fn (x, y) => x + y
let inc = fn x => x + 1
let f = fn (x: Real[L]) =>
    let y = x^2
    y + 1
```

Parameter and return types may be omitted. Closures capture lexically. `fn (x, y) => …` takes
two arguments; there is no automatic currying (`curry`/`uncurry`, §8.3).

### 2.3.6 Calls and placeholders

```aevia
f(x, y)
g()(x)                 # call the returned function
map(_, xs)             # fn f => map(f, xs)
map(f, _)              # fn ys => map(f, ys)
```

A call needs parentheses; arguments are comma-separated; there are no variadic functions; calls
associate to the left. `_` as an argument means "an argument is missing here": the whole call
becomes a lambda whose parameters follow the order of the `_`s. `_` anywhere else is `E0202`.

### 2.3.7 Operators and pipes

```aevia
let y = x |> square |> sqrt          # sqrt(square(x))
let z = sqrt <| square <| x          # same
let w = xs |> map(f, _) |> sum
```

- `a |> f` and `f <| a` both mean `f(a)`. The right-hand side is a function value with no
  implicit currying, so a multi-argument function uses a placeholder (`xs |> map(f, _)`). The
  two operators share one level and associate to the left in order of appearance; use one
  direction per expression.
- An operator section such as `(== 1)` or `(* 2)` is `E0205`; write `fn y => y == 1`.

> **Status:** a bare operator `(+)` parses but its name is not yet bound to a function
> (`E0301`); write a lambda for now.

### 2.3.8 Records

```aevia
let p = { x = 1.0 m, y = 2.0 m }
let q = { p with x = 3.0 m }
let r = { x, y }                      # field punning
let a = p.x
```

- Records are structural; field order does not affect type equality. A duplicate field is
  `E0208`. `{}` is the empty record.
- `{ p with x = … }` builds a new record: unlisted fields are unchanged, and new fields may be
  added.
- Field access is the highest-precedence postfix operator.

### 2.3.9 Variant construction

```aevia
let s = Circle(1.0 m)
let n = None
```

A constructor is an ordinary function or value: `Circle : Real[L] -> Shape`, and `None` is a
polymorphic value of `Option<a>`. Constructor names MUST start with an upper-case letter.

### 2.3.10 Tensors

```aevia
let v = [1.0, 2.0, 3.0]              # Tensor<[3], Float>
let m = [[1.0, 2.0], [3.0, 4.0]]     # Tensor<[2,2], Float>
let e: Tensor<1, Float> = []         # an empty tensor needs an annotation
let q = zeros<Int>(3, 4)             # explicit element type; default Float
```

- Nested literals build rank-n tensors and MUST be rectangular, otherwise `E0203`. The element
  type is unified across elements (`[1, 2.0]` is `Float`); an unannotated `[]` is `E0314`.
- Explicit type arguments are written `f<T>(…)` with `<` touching the function name; a
  comparison `<` has a space before it (§2.7.2).

### 2.3.11 Indexing and slicing

```aevia
let x = v[0]                         # element
let row = m[1]                       # rank 2 → rank 1
let a = m[0, 1]                      # element
let col = m[.., 1]                   # column
let block = m[0..2, 1..3]            # sub-matrix
let last = v[-1]                     # negative index: from the end
let ev = v[::2]                      # strided slice
```

- Indices start at 0 and negative ones count from the end. `..` is half-open and `..=` closed;
  an omitted end means the boundary. `a[i]` takes an element at rank 1 and the i-th slice
  (rank minus one) at rank > 1; `a[i, j]` needs rank ≥ 2.
- `a[i:j:s]` is a strided slice; `i`, `j` and `s` may each be omitted (`s` defaults to 1), and
  `a[i:j]` is the same node as `a[i..j]`.
- `::` is always cons (§2.6), so `xs[1::2]` is `xs[1 :: 2]`. With a start and a stride, write
  the end (`xs[1:5:2]`); when the end is omitted the start must be omitted too (`xs[::2]`).
  Inside `[...]` a `:` is the slice separator, so an annotated subscript needs parentheses:
  `xs[(i : Int)]`.
- An out-of-bounds index is run-time `E0802`; the compiler checks literal indices statically.
  A slice is semantically a copy, and the runtime may share the buffer; a negative-stride slice
  is always a view (§6.6.4.1).

### 2.3.12 `do` and IO

```aevia
fn main(): IO Unit = do
    let name = "data.csv"
    text <- read_file(name)
    let rows = parse_csv(text)
    println("rows = {length(rows)}")
    pure(())
```

- `x <- action` binds the result of an `IO` action; `let` is a pure binding; a bare `IO`
  expression runs in order and its result is discarded (warning `W0202` when the type is not
  `IO Unit`); a statement that is not `IO` is `E0701`.
- The value of `do` is its last `expr` statement. A `do` statement cannot declare `fn` or
  `let rec`; write a block expression when you need one.
- `pure : a -> IO a` is a Prelude function, not a keyword (§9.7.3). `do` accepts only `IO`.

### 2.3.13 Error propagation `?`

`e?` requires `e : Result<T, E>` in a function returning `Result<U, E>`: on success it yields
`T`, on failure it returns `Err(e)` early. `?` also works on `Option` in a function returning
`Option`.

### 2.3.14 Other expressions

```aevia
()                                   # unit
(1, 2)                               # tuple
(1)                                  # parentheses, not a tuple
x : Real[L]                          # type annotation
x!                                   # factorial (Int)
√x                                   # sqrt(x)
```

- There are no one-element tuples (`(1,)` is `E0202`); use the value or a record.
- A type annotation `e : T` has the lowest precedence and covers the whole expression.
- The rules for `x!` are in §2.7.5. `√x` is the prefix form of `sqrt(x)`; the operand's type
  MUST already be known at that point (`√x` works for an annotated `x`; `√4.0` cannot yet be
  inferred, so write `sqrt(4.0)`).

## 2.4 Patterns

```aevia
let (x, y) = p
let { x, y } = point
let [a, b, ..rest] = xs

match shape with
| Circle(r) -> …
| Rect(w, h) if w > h -> …
| _ -> …
```

The grammar is in A.4.

- A variable pattern binds a name; `_` binds nothing; `p as x` binds the whole value while
  matching `p`; `(p : T)` annotates.
- **An upper-case name in a binding position is a variable.** In `let`, a `let` inside `do`,
  `<-`, and function and lambda parameters, a bare `UIDENT` (`let A = …`,
  `fn step(M: Tensor<2, Float>)`) binds a variable, because scientific code habitually names
  matrices and constants in capitals. A constructor pattern in a binding position MUST carry an
  argument list (`let Some(x) = …`); a nullary constructor is matched as a constructor only in
  `match` arms. The bound variable shadows a value of the same name, units included, but does
  not affect unit literals such as `3 A`.
- A constructor pattern uses the nominal type and constructor index fixed by the checker;
  equal display names do not make equal constructors.
- A record pattern matches only the listed fields (open matching); `{ x, y }` binds the
  same-named fields and `{ x = p }` renames. A tuple pattern needs equal arity.
- Tensor patterns support rank 1 only: `[a, b, ..rest]`. Without `..rest` the length must be
  exactly equal; with it the length must be at least the prefix length, and `rest` keeps the
  element representation, units and nominal schema and shares the original storage.
- A binding name MUST be unique within one pattern (shorthand, `as` and rest included),
  otherwise `E0308`.
- A floating literal is not a valid pattern (`E0204`); use a guard.
- Exhaustiveness keeps correlations between fields: `(true,true)` and `(false,false)` do not
  cover `(true,false)`; `[]` and `[head, ..tail]` cover all lengths; guards do not contribute
  to coverage. The analysis has a fixed work and depth budget; when it cannot prove
  exhaustiveness it asks for a `_` arm and never assumes coverage.

## 2.5 Type syntax

```aevia
Real[L/T]
Float[1]
Tensor<2, Float[L]>
Tensor<[3, ?], Real[d]>
{ x: Real[L], ...r }
(a, b) -> c
forall d. Real[d] -> Real[d^2]
Point[Θ]
Result<T, E>
IO Unit
```

The grammar is in A.5. In brief:

- `Real`, `Float` and `Complex` without brackets mean `[1]`.
- In `Tensor<2, Float>` the `2` is the rank; `Tensor<[3,4], Float>` is a refined shape, and `?`
  marks an unknown extent.
- `Unit[d]` is the type of unit values (§5.2); bare `Unit` is the unit type `()`.
- A tuple type is `(A, B)`. `A * B` is not a type; `*` appears only in dimensions.
- The kind of a `type_param` (`Type`, `Dim`, `Nat`, `Row`, `Rep`, `Format`, `Shape`) may be
  omitted and is inferred from use. `<…>` on a declaration already quantifies universally, so
  `forall` is usually omitted.
- `IO` applies to one atomic type: `IO Unit`, `IO (Result<T, E>)`.

### 2.5.1 Dimension syntax

- A `dim` is factors joined by `*` and `/`; a factor may carry an exponent `^n` or `^(p/q)`,
  and `1` is dimensionless. `/` associates to the left: `L/T^2` = `L/(T^2)`.
- A capitalised name is a base dimension or alias; a lower-case name is a dimension variable and
  may appear only inside `[…]` (a bare `d` is a type variable).
- Dimension aliases (`Velocity`, `Force`) are expanded during checking and preferred when
  printing.

## 2.6 Operator precedence and associativity

From lowest to highest:

| Level | Operators | Associativity |
|---|---|---|
| 1 | `\|>` `<\|` | left (in order of appearance when mixed) |
| 2 | `\|\|` | right |
| 3 | `&&` | right |
| 4 | `==` `!=` `<` `<=` `>` `>=` | non-associative (chaining is `E0202`) |
| 5 | `::` | right |
| 6 | `+` `-` | left |
| 7 | `*` `/` `@` `·` `×` `÷` | left |
| 8 | prefix `-` `+` `!` `√` | prefix |
| 9 | `^` | right |
| 10 | call, index, `.field`, `?`, postfix `!` | postfix |

`..` and `..=` occur only in indices. `-x^2` = `-(x^2)`; `2^3^2` = `2^(3^2)`;
`a * b @ c` = `(a * b) @ c`, so parenthesise matrix products; write `x == y && y == z` for
`x == y == z`. The bodies of `if`, `match`, `fn` and `let` extend as far right as possible;
parenthesise to break ambiguity. `|` is used only for match arms and variant declarations.

## 2.7 Ambiguities and their resolution

### 2.7.1 An identifier after a number

`3 m` is a unit literal, and so is `3 x` (`E0501` when `x` is not a unit); see §1.8.1.

### 2.7.2 `<` for type arguments

In `zeros<Float>(3, 4)` the `<` touches `zeros` and is parsed as type arguments; in `a < b` the
`<` is preceded by a space and is a comparison. After `ident<` the parser tries type arguments
and falls back to a comparison on failure, so the formatter MUST keep the space before a
comparison `<`. An adjacent `<…>` is valid only when the callee is a function name (or its
qualified name); `3<Int>(2)` and `f()<Int>` are `E0207`, which suggests adding a space or
parenthesising the callee.

### 2.7.3 `{` is a record, not a block

Blocks use indentation only, and `{…}` is always a record: `if x then { y = 1 } else { y = 2 }`
is a record expression.

### 2.7.4 The roles of `|`

`|` appears in `match` arms (`| pattern -> expr`) and variant declarations (`| Ctor(T)`).
Logical or is the two-character `||`. There are no anonymous union types.

### 2.7.5 Prefix and postfix `!`

- Prefix `!x` is logical not; the `!=` of `x != y` is a single token.
- Postfix `x!` is the factorial and `x!!` the double factorial. The `!` MUST touch the end of
  its operand (no whitespace) and the operand MUST be an `Int`; `n !` is a syntax error. Postfix
  `!` binds tighter than prefix `-` and than `^`: `-3!` = `-6` and `2^3!` = `2^6`. For the
  factorial of a compound expression write `factorial(n + 1)`.

> **Status:** `(n + 1)!`, with `!` right after a closing parenthesis, is currently rejected by
> the parser (`E0202`); write `factorial(n + 1)`.

### 2.7.6 Unary `-`

Unary `-` binds more loosely than `^` and more tightly than `*`: `-2^2` = `-4` and `(-2)^2` =
`4`. `a - -b` is valid.

## 2.8 Desugaring

These forms serve type checking and the core IR (§9.2.2); they are not user syntax.

| Syntax | Desugars to |
|---|---|
| `fn f(x) = e` | `let rec f = fn (x) => e` |
| `a \|> f`, `f <\| a` | `f(a)` |
| `√x` | `sqrt(x)` |
| `x!`, `x!!` | `factorial(x)`, `double_factorial(x)` |
| `a @ b` | `matmul_rank(a, b)` (dispatches on rank to matmul/matvec/vecmat/dot) |
| `a * b` (tensors) | `mul_elementwise(a, b)` |
| `{ p with x = e }` | the record-update primitive |
| `f(_, x)` | `fn a => f(a, x)` |
| `do …` | a chain of `bind`/`pure` |
| `[a, b, c]` | `tensor([a, b, c])` |
| `a[i, j]`, `a[i..j]`, `a[i:j:s]` | `index(a, [i, j])`, `slice(a, i, j)`, `slice_step(a, i, j, s)` |

## 2.9 A complete example

```aevia
module Projectile

open Units.SI
import Math { sqrt, atan, sin, cos }

type State = {
    x:  Float[L],
    y:  Float[L],
    vx: Float[L/T],
    vy: Float[L/T],
}

type Rate = {
    x:  Float[L/T],
    y:  Float[L/T],
    vx: Float[L/T^2],
    vy: Float[L/T^2],
}

let g = 9.80665 m/s^2

fn rhs(_t: Float[T], s: State): Rate =
    let speed = sqrt(s.vx^2 + s.vy^2)
    let k = 0.02 / m
    {
        x  = s.vx,
        y  = s.vy,
        vx = -k * speed * s.vx,
        vy = -g - k * speed * s.vy,
    }

fn trajectory(v0: Float[L/T], angle: Float[1]): Tensor<1, State> =
    let s0 = { x = 0.0 m, y = 0.0 m,
               vx = v0 * cos(angle), vy = v0 * sin(angle) }
    let ts = linspace(0.0 s, 10.0 s, 1000)
    odeint(rhs, s0, ts)

fn main(): IO Unit = do
    let path = trajectory(50.0 m/s, 45 deg)
    println("range = {path[-1].x}")
```

The example uses records, field access, units, dimension inference, mixing `Float` and `Real`,
tensors, ODE integration, `do` and string interpolation (chapter 11 walks through it line by
line). Chapter 03 explains where the types of these expressions come from.


<a id="ch-03-types"></a>

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


<a id="ch-04-dimensions"></a>

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


<a id="ch-05-units"></a>

# 05 — Units

Chapter 04 made dimensions types; this chapter makes units **compile-time scales**. A unit is
not part of a type. It supplies:

1. a dimension `d`;
2. a scale `s` (an exact multiple of the coherent SI unit);
3. an affine offset `o : Real[d]` (affine units only).

The unit literal `3.2 km` folds at compile time to `3200.0` (the coherent SI value) with type
`Float[L]`; `km` does not stay in the value. To display a value in a unit, attach a display
unit with `to(km, x)` (§5.4).

## 5.1 Unit declarations

```aevia
dimension Currency                  # a user-defined base dimension
unit USD : Currency = 1             # base unit
unit EUR : Currency = 1.08 USD      # derived unit

unit m : Length = 1 prefixable      # base unit, SI prefixes allowed
unit km : L = 1000 m                # explicit derived unit
unit N : M*L/T^2 = 1 kg*m/s^2
unit degC : Θ = 1 K offset 273.15 K # affine unit
```

### 5.1.1 Syntax

```
dimension_decl ::= [ "private" ] "dimension" UIDENT [ "=" dim ]
unit_decl      ::= [ "private" ] "unit" unit_name ":" dim "=" unit_rhs [ "prefixable" ]
unit_rhs       ::= unit_scale [ unit_expr ] [ "offset" unit_scale unit_expr ]
unit_scale     ::= number [ "/" number ] | "pi" [ "/" number ]
```

- `dimension` and `unit` are **exported** by default; `private` limits them to the module.
  Exported items enter the manifest and the registry, so downstream packages can `import`
  them. `Units.*` is a generated view of `manifest.units`; user declarations need no change
  to the standard library.
- `unit_name` is an identifier or a special name containing `°` (`°C`, `°F`).
- `unit_scale` is a positive rational or a rational multiple of `pi`. Decimal literals are
  read as **exact decimals** (`0.3048` is `381/1250`), not as `Float`, so conversions such as
  `ft → m` are exact. Unit declarations are evaluated at compile time and admit no run-time
  expression; write a decimal literal of sufficient precision for an approximate scale.
- An omitted `unit_expr` (§5.2) declares a base unit: the scale MUST be 1 and the dimension
  MUST be a base dimension.
- `offset` is for affine units only; the offset's dimension MUST equal the unit's (`E0504`).
- `prefixable` generates the SI-prefixed variants (§5.1.4).

### 5.1.2 Base and derived units

- A unit name MUST be unique; a repeated declaration is `E0505`. A dimension may have several
  units of scale 1; they are equivalent.
- Units are visible in declaration order: using a unit before its declaration is `E0501`, so
  definitions cannot form a cycle.
- A derived scale is computed at compile time as an exact rational. If an exponent leaves the
  scale without an exact value (`3 ft^(1/2)`: the scale of `ft` is not a perfect square), the
  error is `E0506`.
- Unit declarations can be imported across modules; a `private unit` is not exported.

### 5.1.3 Unit namespace

Unit names and value names live in **separate namespaces**:

- In the literal `3 m`, `m` resolves in the unit namespace; `let m = 5` does not change
  `3 m`. To multiply by a variable, write `3 * m`.
- In an ordinary expression a unit name is the value `Unit[d]` (globally visible).
- On a clash, the longest match wins: `ms` is milliseconds, not `m*s`; write `m*s` for
  metre-seconds.
- A unit value cannot be accessed by a qualified name (`Units.SI.km` is invalid); units are globally visible from the kernel table, so use the short name.

Unit names and dimension names are also separate namespaces; one spelling may have a meaning
in each, and the parser decides by position:

| Spelling | As a unit | As a dimension |
|---|---|---|
| `L` | litre (alias of `liter`/`litre`, `prefixable`) | display alias of `Length` |
| `T` | tesla | display alias of `Time` |
| `G` | gauss (`1e-4 T`) | not the gravitational constant |
| `h` | hour | not Planck's constant |

Canonical names are always the full names (`Length`/`Mass`/`Time`/…); single letters are for
source and display only.

### 5.1.4 Prefixes

| Symbol | Factor | Symbol | Factor | Symbol | Factor | Symbol | Factor |
|---|---|---|---|---|---|---|---|
| `Q` quetta | 10^30 | `P` peta | 10^15 | `h` hecto | 10^2 | `µ` micro | 10^-6 |
| `R` ronna | 10^27 | `T` tera | 10^12 | `da` deca | 10^1 | `n` nano | 10^-9 |
| `Y` yotta | 10^24 | `G` giga | 10^9 | `d` deci | 10^-1 | `p` pico | 10^-12 |
| `Z` zetta | 10^21 | `M` mega | 10^6 | `c` centi | 10^-2 | `f` femto | 10^-15 |
| `E` exa | 10^18 | `k` kilo | 10^3 | `m` milli | 10^-3 | `a` atto | 10^-18 |
| | | | | | | `z`/`y`/`r`/`q` | 10^-21/-24/-27/-30 |

- A prefix applies only to a `prefixable` unit. `kg` is the SI base unit but prefixes attach
  to `g` (`1/1000 kg`, `prefixable`); an **exact name match wins over prefix decomposition**,
  so `kg` is the base unit, not `k`+`g`.
- Longest match: `dam` is `da`+`m`, not `d`+`am`.
- Prefixes do not stack: `kkm` is not a unit and is reported as `E0501`.
- Binary prefixes `Ki`, `Mi`, `Gi`, `Ti` apply only to information units (`bit`, `B`).

## 5.2 Unit expressions

```
unit_expr   ::= unit_factor { ("*" | "·" | "/") unit_factor }
unit_factor ::= unit_atom [ "^" exponent ]
unit_atom   ::= unit_name | "(" unit_expr ")" | "%"
```

- `*` and `·` are synonyms; `/` is left-associative. An exponent is an integer literal or a
  parenthesised rational: `m^2`, `s^-1`, `m^(1/2)`.
- Numbers (other than exponents) and juxtaposition by whitespace are not allowed inside a unit
  expression.
- An affine unit may appear only alone; in a product or power it is `E0508` (§5.6.4).

### 5.2.1 Unit values

At the value level a unit expression is a `Unit[d]` value:

```aevia
let speed_unit = km / h          # Unit[L/T], scale 1000/3600
let v = 100 * speed_unit         # Real[L/T], value 250/9
```

`100 speed_unit` is not a unit literal (`speed_unit` is not a unit name) and is `E0501`;
write `100 * speed_unit`. `Unit[d]` supports `u * v : Unit[d*e]`, `u / v : Unit[d/e]` and
`u ^ n : Unit[d^n]` (`n` a literal); units can be compared (same dimension), printed and
passed as arguments. A unit is not a number and cannot be added to a `Real`.

### 5.2.2 Evaluating a unit literal

```
value = number * scale(unit_expr) + offset(unit_expr)
type  = Scalar<rep(number), dim(unit_expr)>
```

- A linear unit has `offset = 0`: `3 km` → `3000`, `Real[L]`.
- An affine unit is legal only alone: `25 °C` → `25 + 273.15 = 298.15`, `Point[Θ]` (§5.6).
- An `Int` number gives `Real` (exact) by default and a `Float` number gives `Float`:
  `3 km : Real[L]`, `3.0 km : Float[L]`. The expected type overrides the representation:
  `let x: Real[L] = 3.0 m` is the exact `3`; in `fall_time(20 m)`, `20 m` is instantiated at
  `Float[L]`.
- A scale containing `pi` may give a symbolic result: `30 deg` → `pi/6 : Real[1]`, or
  `0.5235987755982988` when `Float` is expected.

## 5.3 Unit tables

The standard library `Units.SI` provides the following units (excerpt).

### 5.3.1 SI base units

| Unit | Dimension | Note |
|---|---|---|
| `m` | `L` | metre, prefixable |
| `kg` | `M` | kilogram (`g` is prefixable) |
| `s` | `T` | second, prefixable |
| `A` | `I` | ampere, prefixable |
| `K` | `Θ` | kelvin, prefixable |
| `mol` | `Amt` | mole, prefixable |
| `cd` | `Lum` | candela, prefixable |

### 5.3.2 SI derived units

| Unit | Dimension | Definition |
|---|---|---|
| `Hz` | `1/T` | `1/s` |
| `N` | `M*L/T^2` | `kg*m/s^2` |
| `Pa` | `M/(L*T^2)` | `N/m^2` |
| `J` | `M*L^2/T^2` | `N*m` |
| `W` | `M*L^2/T^3` | `J/s` |
| `C` | `I*T` | `A*s` |
| `V` | `M*L^2/(I*T^3)` | `W/A` |
| `F` | `I^2*T^4/(M*L^2)` | `C/V` |
| `Ω` / `ohm` | `M*L^2/(I^2*T^3)` | `V/A` |
| `S` | `I^2*T^3/(M*L^2)` | `1/Ω` |
| `Wb` | `M*L^2/(I*T^2)` | `V*s` |
| `T` | `M/(I*T^2)` | `Wb/m^2` |
| `H` | `M*L^2/(I^2*T^2)` | `Wb/A` |
| `lm` | `Lum` | `cd*sr` |
| `lx` | `Lum/L^2` | `lm/m^2` |
| `Bq` | `1/T` | `1/s` |
| `Gy`, `Sv` | `L^2/T^2` | `J/kg` |
| `kat` | `Amt/T` | `mol/s` |
| `rad`, `sr` | `1` | dimensionless, scale 1 |

### 5.3.3 Common non-SI units

| Unit | Dimension | Definition |
|---|---|---|
| `min` / `h` / `d` / `week` / `yr` | `T` | `60 s` / `3600 s` / `86400 s` / `604800 s` / `31557600 s` |
| `°` / `deg` | `1` | `pi/180` |
| `′` / `arcmin` | `1` | `pi/10800` |
| `″` / `arcsec` | `1` | `pi/648000` |
| `l` / `liter` | `L^3` | `1/1000 m^3` |
| `t` | `M` | `1000 kg` |
| `eV` | `Energy` | `1.602176634e-19 J` |
| `Da` / `u` | `M` | `1.66053906660e-27 kg` |
| `au` | `L` | `149597870700 m` |
| `pc` | `L` | `3.0856775814913673e16 m` |
| `ly` | `L` | `9460730472580800 m` |
| `Å` / `angstrom` | `L` | `1e-10 m` |
| `bar` / `atm` / `mmHg` / `psi` | `Pressure` | `100000 Pa` / `101325 Pa` / `133.322387415 Pa` / `6894.757293168 Pa` |
| `cal` | `Energy` | `4.184 J` |
| `Wh` | `Energy` | `3600 J` (`kWh` is the prefixed form) |
| `hp` | `Power` | `745.6998715822702 W` |
| `kn` / `mph` | `L/T` | `1852/3600 m/s` / `1609.344/3600 m/s` |
| `ft` / `in` / `yd` / `mi` / `nmi` | `L` | `0.3048` / `0.0254` / `0.9144` / `1609.344` / `1852` `m` |
| `acre` | `L^2` | `4046.8564224 m^2` |
| `gal` | `L^3` | `0.003785411784 m^3` |
| `oz` / `lb` | `M` | `0.028349523125 kg` / `0.45359237 kg` |
| `lbf` | `Force` | `4.4482216152605 N` |
| `°C` | `Θ` | affine: `1 K offset 273.15 K` |
| `°F` | `Θ` | affine: `5/9 K offset 255.3722222222222 K` |
| `bit` / `B` / `KiB` | `1` | information: `1` / `8 bit` / `1024 B` |

Information units are dimensionless because many formulas use bits as probability or entropy.
For a separate `Information` base dimension, declare `dimension Information` (the kernel's
`Config` must declare `base_dimension("Information")`).

### 5.3.4 Unit modules

| Module | Contents |
|---|---|
| `Units.SI` | base units, derived units, prefixes, common non-SI units |
| `Units.Imperial` | imperial length, mass, volume, pressure, temperature |
| `Units.Astronomy` | `au`, `pc`, `ly`, `Rsun`, `Msun`, `Mearth`, `jup` (`Mpc` is `pc` with a prefix) |
| `Units.CGS` | `cm`, `g`, `dyn`, `erg`, `Gal`, `P`, `St`, `G`, `Mx`, `Ba`, `Oe` |
| `Units.Constants` | physical constants (§5.8) |
| `Units.Natural`, `Units.Planck`, `Units.Atomic`, `Units.Geometrized` | constant modules under a unit system (§5.11.1) |

Status: all of these modules can be `open`ed. Their contents are generated from the kernel
manifest (`manifest.units`, `manifest.constants`, `manifest.unit_systems`); every `UnitValue`
carries `aliases`, `prefixable`, `category` and `since`. Units are **session-private**: a unit
added with `Session::register_unit` appears only in that session's manifest.

## 5.4 Conversion and display

### 5.4.1 Three operations

```aevia
let d = 3.2 km              # Float[L], value 3200.0
let e = to(m, d)            # value 3200.0, display unit m
let f = value_in(km, d)     # Float[1], value 3.2
let g = base(d)             # value 3200.0, display unit removed
```

- `to(u, x)`: `x : Scalar<ρ,d>` and `u : Unit[d]`; value and type are unchanged, only a
  display unit is attached.
- `value_in(u, x)` is `x / u`, a dimensionless number.
- `base(x)` removes the display unit and returns the coherent SI value.
- A unit whose dimension differs from the value's is `E0509`.
- **An affine unit accepts only `Point[d]`**: `value_in(°C, point(298.15, K))` is `25`. Using
  an affine unit on a temperature difference (`Scalar<ρ,Θ>`) is `E0510`, so
  `value_in(°C, 298.15 K)` is an error. Display a difference with `delta_to(°C, dT)`, which
  uses the scale and ignores the offset.

Status: `delta_to(°C, 10.0 K)` currently prints `-263.15 °C` (the offset is still applied),
which contradicts the semantics above. This is a known defect.

### 5.4.2 Display-unit propagation

At run time each scalar may carry an optional display unit:

- `x + y`, `x - y`: kept if both sides have the same display unit, otherwise dropped to SI.
- `x * y`, `x / y`: if one operand is dimensionless and carries a display unit, the other
  operand's display is kept; otherwise it is dropped.
- `to` sets it explicitly and `base` clears it; function calls drop it by default (shape-
  preserving functions such as `abs` keep it).

A display unit is **metadata**: it takes no part in equality, typing or serialisation.
`3.2 km == 3200 m` is `true`.

Status: the warning `W0501` (display unit dropped) has no emission site yet.

### 5.4.3 Printing

- With a display unit, the number is converted to that unit (`3.2 km`, `25 °C`).
- Without one, a "best unit" is chosen: an SI derived unit (`N`, `J`, `Pa`) if one fits,
  otherwise a product of base units (`kg*m/s^2`). Fractional exponents print correctly
  (`T^(1/2)` → `s^(1/2)`).
- `show` is guaranteed readable; `repr` is guaranteed to parse back to the same value.
- `Float` printing uses the shortest round-trip representation; an exact `Real` prints as a
  fraction or radical.

## 5.5 Angles and dimensionless units

- `rad` is a dimensionless unit of scale 1; `deg`/`°` has scale `pi/180`.
- `sin(30 deg)` and `sin(30)` (30 radians) are both legal, because `sin` only requires a
  dimensionless argument; the type system does not tell radians from plain numbers.
- `atan2(y, x)` returns dimensionless radians; display degrees with `to(deg, ...)`.

## 5.6 Affine units and `Point`

### 5.6.1 Problem and types

Temperature is used in two ways: an **absolute temperature** (a point of an affine space;
`25 °C` is 298.15 K) and a **temperature difference** (a vector that can be added and scaled).
Conflating them makes `20 °C + 30 °C` come out as a wrong 50 °C or 596.3 K.

```
Point[Θ]        absolute temperature (affine point)
Float[Θ]        temperature difference (vector)
```

- Only affine unit literals produce `Point[d]`: `25 °C`, `77 °F`. A linear unit literal
  produces a difference: `25 K` is a difference.
- Write an absolute temperature in kelvin with `point(298.15, K)`.
- `Point` has only the `Float` representation; a symbolic temperature is a `Real[Θ]`
  difference.
- A `Point` can be a tensor element (`Tensor<[nx,ny], Point[Θ]>` is a temperature field);
  element-wise operations follow the point rules. Average a field with `centroid`, not
  `mean` (§13.9.2).

### 5.6.2 Operations

```
p - q        : Float[d]        two points subtract to a difference
p + x, x + p : Point[d]        point plus difference
p - x        : Point[d]        point minus difference
lerp(p,q,t)  : Point[d]        affine combination, t : Float[1]
p + q, k * p, p / k, p / K   : error E0405
```

- `Point` is comparable (`p == q`, `p < q`); `min` and `max` return a `Point`;
  `centroid : Tensor<1,Point[d]> -> Point[d]`.
- In `p + x` and `p - x`, `x` MUST be `Float[Θ]`. An exact `Real[Θ]` variable does not
  qualify (`E0302`; use `to_float`), but a literal `10 K` is instantiated at the expected
  `Float`, so `25.0 °C + 10 K` is legal.
- `to(°C, p)` sets a display unit, `value_in(°C, p)` returns the Celsius number, and `base(p)`
  returns the kelvin value as `Float[Θ]`.

```
point     : (Float[1], Unit[d]) -> Point[d]
to        : (Unit[d], Scalar<ρ,d>) -> Scalar<ρ,d>      | (Unit[d], Point[d]) -> Point[d]
value_in  : (Unit[d], Scalar<ρ,d>) -> Scalar<ρ,1>      # E0510 for an affine unit
value_in  : (Unit[d], Point[d]) -> Float[1]
base      : Scalar<ρ,d> -> Scalar<ρ,d>                 | Point[d] -> Float[d]
delta_to  : (Unit[d], Scalar<ρ,d>) -> Scalar<ρ,d>      # scale only, no offset
unit_pow_dyn : (Unit[d], Int) -> Dyn<Real>             # run-time exponent
lerp      : (Point[d], Point[d], Float[1]) -> Point[d]
```

`point(25.0, °C)` is 298.15 K; the first argument is the numeric coefficient in that unit.

### 5.6.3 Why kelvin is linear

`K` is a linear unit (scale 1, offset 0), so `300 K` is a difference by type: the kelvin
scale serves both absolute temperatures and differences, and the unit alone cannot tell them
apart. Celsius and Fahrenheit are affine scales, so their literals are naturally points.

```aevia
let T1 = 25.0 °C            # Point[Θ]
let T2 = point(300.0, K)    # Point[Θ]
let dT = T2 - T1            # Float[Θ]
let T3 = T1 + 10.0 K        # Point[Θ]
```

### 5.6.4 Affine units in unit expressions

`°C/s` and `°C*m` are meaningless and the parser rejects them (`E0508`). `°C` may appear
only alone as a literal unit, or as an argument of `to`/`value_in`.

## 5.7 Implementation of conversion

- A scale is folded at compile time into an exact rational (or a symbolic value containing
  `pi`); `3 km` directly yields `3000`.
- At run time only the display unit's name and conversion data remain: `to(u, x)` sets the
  display unit and printing uses `x / u.scale`. Affine conversion is
  `value_in(°C, p) = (p - 273.15 K) / 1 K`.
- Unit equality: same dimension and same scale (and, for affine units, same offset). `m` and
  `100 cm` are different unit values of the same dimension; `to` converts between them.

## 5.8 Physical constants

`Units.Constants` is generated from `manifest.constants`; symbols are identical to RSSN's:

| Kind | Constants (SI unit) |
|---|---|
| exact | `c` (`m/s`), `g_n` (`m/s^2`), `h`, `hbar` (`J*s`), `k_B` (`J/K`), `N_A` (`1/mol`), `e` (`C`), `F` (`C/mol`), `R` (`J/(mol*K)`), `M_u` (`kg/mol`), `T_tp` (`K`), `atm` (`Pa`), `G_0` (`S`), `K_J` (`Hz/V`), `R_K` (`ohm`), `Phi_0` (`Wb`), `K_cd` (`lm/W`), `dnu_Cs` (`Hz`) |
| measured | `G` (`m^3/(kg*s^2)`), `alpha` (`1`), `R_inf` (`1/m`), `a_0` (`m`), `epsilon_0` (`F/m`), `mu_0` (`N/A^2`), `mu_B` (`J/T`), `m_e`, `m_p`, `m_n`, `m_u` (`kg`), `sigma_SB` (`W/(m^2*K^4)`) |

- An exact constant is a defining value or an exact derivation of one and has no
  `uncertainty`; a measured constant is a CODATA 2022 value with a standard uncertainty. Both
  are exact rationals in the manifest (`ScaleValue`); `exact` means "defining value", not a
  storage type.
- A constant is not a unit: it has a dimension and takes part in arithmetic, but cannot appear
  in a unit position. The core language carries no uncertainty; use `Measurement` (§8.14) when
  needed.
- `pi`, `e` (Euler's number) and `i` live in `Math`. `Units.Constants.e` is the **elementary
  charge** and shares its name with `Math.e`; qualify it when both are open.

Metadata of each entry:

```
type ConstantInfo = {
    name: String,              # "speed of light in vacuum"
    symbol: String,            # "c"
    value: Scalar<Real, d>,    # coherent SI value
    dimension: Dim,
    unit: String,              # "m/s"
    exact: Bool,
    uncertainty: Option<Scalar<Float, d>>,
    source: String,
}
```

## 5.9 Unit errors

| Code | Situation |
|---|---|
| `E0501` | unknown unit name (including a stacked prefix such as `kkm`, or use before declaration) |
| `E0502` | unit not imported/opened (reserved) |
| `E0503` | unit cannot take a prefix (reserved) |
| `E0504` | affine offset dimension mismatch |
| `E0505` | duplicate unit name |
| `E0506` | scale has no exact value for the exponent |
| `E0507` | prefixed unit conflicts with an explicit unit (reserved) |
| `E0508` | affine unit in a product or power |
| `E0509` | unit dimension of `to`/`value_in` does not match the value |
| `E0510` | affine display of a temperature difference |
| `E0511`/`E0512` | unit-system errors (§5.11) |

Status: `E0502`, `E0503` and `E0507` are reserved and currently unreachable. The unit table is
the whole kernel table plus local declarations and has no module-level unit namespace, so the
states "not imported", "prefix forbidden" and "prefix/explicit conflict" do not exist; the
compiler does not substitute another error for them.

## 5.10 Relation to rssn's unit layer

1. **Single source of truth.** `Units.*` is a generated view of `manifest.units`; user
   `dimension`/`unit` declarations register in the same registry and flow back to the manifest.
   `UnitRegistry` serves only run-time services such as `resolve` and `format`.
2. **The term graph carries no units.** `Real`/`Float` values are coherent SI numbers on the
   host side. Only for rssn typesetting or export is a value wrapped with
   `quantity(v, "<coherent SI unit>")`; it never enters `compute`.
3. **Affine units are resolved in the bridge.** A `Point` becomes a kelvin `Float` before
   reaching rssn; `to(°C, ...)` happens only in the display layer.
4. **Registry round trip.** Only for purely rational exponents is the exact round trip
   `DimTerm → Dimension → DimTerm` via `Dimension::exact_factors` supported; symbolic
   exponents do not round-trip and `Dimension.factors` text MUST NOT be parsed.
   `UnitDef → scale/offset → UnitDef` MUST round-trip exactly.
5. **`DimensionMode::Strict` is for FFI/export only.** The static path's term graph is
   dimensionless and uses `Off`; the dimension of a `Dyn<ρ>` is held by the host as a
   `DimTerm`.

## 5.11 Unit systems

Physicists often set some constants to 1 (`c = 1`, `ħ = 1`). Aevia does this as a
**module-level dimension reduction**: dimensions *written* in the module are simplified by the
unit system, while every value remains an honest SI quantity.

### 5.11.1 Selecting a unit system

The module header selects one by identifier:

```aevia
module Hep with units natural
```

The kernel provides four systems (`manifest.unit_systems`); any other name in the header is
`E0512`, and the diagnostic lists the available names.

| Name | Quantities set to 1 | Generator dimensions left after reduction |
|---|---|---|
| `natural` | `c`, `hbar`, `k_B`, `epsilon_0` (Heaviside–Lorentz) | energy, amount of substance, luminous intensity |
| `planck` | `c`, `hbar`, `G`, `k_B`, `4 pi epsilon_0` | amount of substance, luminous intensity |
| `atomic` | `hbar`, `m_e`, `e`, `4 pi epsilon_0` | temperature, amount of substance, luminous intensity |
| `geometrized` | `c`, `G` | length, current, temperature, amount of substance, luminous intensity |

The constant modules (`Units.Natural` and so on) declare `with units <system>` themselves;
`c`, `hbar` and `k_B` are exactly `1` in `Units.Natural` as a result of the reduction.

### 5.11.2 Reduction

Let the dimensions of the quantities set to 1 be `q₁ … q_k` and the generator dimensions
`g₁ … g_m`; together they form a rational basis of the seven SI base dimensions. Every SI
dimension `d` decomposes uniquely as

```
d = Π qᵢ^λᵢ · Π gⱼ^μⱼ
```

`R(d) = Π gⱼ^μⱼ` is the **representative** of `d`. The **system value** of a quantity `x` of
dimension `d` is `x / Π Qᵢ^λᵢ` (`Qᵢ` the exact SI value), an SI quantity of dimension `R(d)`.
Example: in `natural`, `R(L) = R(T) = Energy^-1`, and the system value of `1 m` is
`1 m / (ħ c) ≈ 5.07e24 J⁻¹`. User base dimensions and dimension variables lie outside the space
spanned by the SI bases and are left unchanged.

In a module with a unit system, **the dimension annotations and unit literals written in that
module** are reduced before checking, so the parameters of `fn f(x: Real[L])` and
`fn f(x: Real[T])` have the same type in `natural`.

### 5.11.3 Values are not reinterpreted

Reduction changes only the dimensions *written*, never a value: every quantity is still an SI
quantity whose type is written with its representative dimension. Values cross the boundary of
a module with a unit system without conversion; **an imported binding keeps the SI type it has
in its home module**.

### 5.11.4 Units without an exact scale

Reducing a unit literal needs an exact scale. If some exponent is not an integer the scale is
irrational; the unit has no exact scale in that system and the error is `E0511` (typically `C`
in `natural`, which needs `(ε₀ħc)^(1/2)`). Write the number in system units, or convert an SI
value with `to_system` (§5.11.5).

### 5.11.5 Explicit conversion

```aevia
to_system("natural", x)           # SI quantity x → system value of dimension R(d)
from_system<L>("natural", y)      # system value y → quantity of SI dimension L
```

- The system name MUST be a string literal, otherwise `E0302`; an unknown name is `E0512`; an
  argument count other than 2 is `E0315`.
- `to_system` requires the argument's dimension to be known at check time, otherwise `E0304`;
  the result has the same representation and dimension `R(d)`.
- `from_system<D>` MUST name the target SI dimension `D` (reduction forgets it: `L` and `T` are
  both `Energy^-1`), otherwise `E0304`; `D` is always read as SI. The argument's dimension MUST
  equal `R(D)`, otherwise it is reported as an ordinary dimension mismatch.
- The result keeps the argument's representation: `Float`/`Complex` multiply by a floating
  factor; exact `Int`/`Real` stay exact rationals when the factor is an exact scale without
  `π`, and otherwise become a symbolic term; they never silently become floats.

### 5.11.6 Display

In a module with a unit system, a quantity with no display unit of its own is shown in the
system's generator dimensions: an inverse energy in `natural` shows as `eV^-1`, not
`s^2/(m^2*kg)`. This selects the display unit only; the value is unchanged. Records, variants
and tuples are displayed field by field.

The next chapter defines evaluation: when a `Real` is an exact number, a symbolic term or a
`Float`, and how tensors and IO run.


<a id="ch-06-semantics"></a>

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


<a id="ch-07-modules"></a>

# 07 — Modules, packages and visibility

The module system is **file-level, functor-free and free of implicit global state**: a file is
a module, and a package is a set of modules plus a manifest. Name resolution is complete at
compile time; there is no dynamic import at run time.

## 7.1 Modules

### 7.1.1 A file is a module

```
src/
  kinematics.aev          -> module Kinematics
  thermo/
    ideal_gas.aev         -> module Thermo.IdealGas
    cycles.aev            -> module Thermo.Cycles
```

- A module name is determined by the path relative to `src/`, in `UpperCamelCase` segments; it
  is case-sensitive, and a file holds exactly one module.
- A file may write `module Thermo.IdealGas`. When imported, the declared name MUST match the
  source path, otherwise `E0601`. A package dependency's import alias belongs to the importer's
  namespace and does not rename the module at its definition (`src/Model.aev` of package
  `Library` may declare `module Model` or `module Library.Model`; a user importing it as
  `Renamed.Model` need not change it). An entry file run directly has no import-path context
  and is not checked.
- Module lookup and execution respect the defining package's scope: `import Helper` inside a
  dependency finds that dependency's `src/Helper.aev` and is never overridden by an
  application module of the same name. Load nodes are de-duplicated by (canonical file, owning
  package); a cycle is reported on the full node as `E0602`.
- `import Pkg` uses the `entry` of the dependency's own manifest.

### 7.1.2 Declaration order

Top-level declarations may refer to one another, including forward references and recursion:
the compiler first collects all top-level names and signatures and then checks the bodies.
Unit and dimension declarations may also come later; units from other modules are supplied by
`import`.

```aevia
fn f(x: Int): Int = g(x) + 1
fn g(x: Int): Int = x * 2
```

### 7.1.3 Exports

All top-level declarations are exported by default; `private` hides one:

```aevia
private let internal_scale = 1.0e-3
private fn helper(x: Real[L]): Real[L] = x * internal_scale

fn public_api(x: Real[L]): Real[L] = helper(x)
```

- `private` is allowed only on `let`, `fn`, `type`, `dimension`, `unit` and `class`, otherwise
  `E0202`; a private declaration is still visible inside its module.
- The constructors of a variant type are exported with the type and cannot be made `private`
  individually; the constructors of a `private type` do not enter the public interface.
- Instances are always exported with the module (separate compilation would otherwise lose
  them); there is no `private instance` (`E0202`).

## 7.2 Import and open

### 7.2.1 `import`

```aevia
open Units.SI
import Math as M
import Data.Csv { read_csv, Table }
```

- `import M` binds `M` as a qualified name, used as `M.f`; `import M as N` aliases it.
- `import M { f, g }` is a selective open and equals `open M { f, g }`.
- Imports are idempotent; repeating one is not an error.
- A module that cannot be found is `E0607`, and the diagnostic gives the expected file path.

### 7.2.2 `open`

- `open M` brings every exported name of `M` into scope unqualified; `open M { f, g }` brings
  only the listed names.
- `open` affects only the current module and is not transitive (there is no re-export).
- A local definition shadows an opened name. If two modules `open` the same name **with
  different types**, the error is `E0603`; the same type (one module opened twice) is not a
  clash.
- `import` and `open` can bring in values (functions, constants, unit values), nominal variant
  constructors and **transparent type aliases** (after `import A { Pair }` or
  `open Physics.Ising` you can write `Params`); an alias is exported with the interface and
  expanded in the importer.

**Units do not depend on imports.** The kernel unit table is global: with no `import` or
`open`, both `3 km` and `to(m, x)` work (there is no module-level unit namespace, §5.9). A unit
a user declares with `unit` is exported with its module. A unit value cannot be reached by the
qualified name `Units.SI.km`.

### 7.2.3 Qualified names

`Math.sqrt`, `Thermo.IdealGas.pressure`: a qualified name starts at a module root; there are no
relative imports. It is a lookup path and introduces no binding. Types, constructors and values
can all be qualified, provided the module is imported first.

### 7.2.4 Name resolution order

An unqualified name `x` is looked up in:

1. local bindings (parameters, `let`, patterns);
2. the current module's top-level declarations;
3. names the current module `open`s;
4. the Prelude;
5. otherwise `E0301`.

Earlier steps shadow later ones; a clash is `E0603` only inside step 3.

## 7.3 Dependencies, interfaces and identity

- The module dependency graph MUST be acyclic; a cyclic import is `E0602`. Type, class and
  instance dependencies are ordered too, and instance resolution needs the relevant modules
  already checked.
- Each module exports an **interface**: types and constructors, value signatures, units, class
  declarations, instances and alias definitions. An alias MUST carry its expansion, or the
  importer cannot expand `type Vec2 = { x: Float, y: Float }` (exporting only the name gives
  `E0301`).
- A module merges only the interfaces in its own dependency closure; it does not gain another
  module's interface because that module happened to be checked earlier. There are no implicit
  global instances: an instance is visible only from imported modules, and the standard
  library's common instances are imported by the Prelude.
- An interface can be cached as a `.aevi` file; the invalidation key is the source hash, the
  dependency-interface hashes, the capability set and the compiler build (§9.11.3).
- **Nominal identity is symbolic.** A class key is `(defining module path, class name)`; an
  instance key adds the canonical head type (aliases expanded first); an ADT's identity is the
  canonical source directory of the defining package (with its name and version) plus the
  source path inside the package, and the readable `module` name is kept separately. Two local
  packages of the same name and version at different source directories therefore have
  different type and instance keys; two import aliases of one package share an identity.
  `AdtId` is only an in-process witness and does not enter interfaces, caches or cross-process
  identity.
- Stand-alone sources and virtual sessions (the REPL, unsaved buffers) also have a stable
  source identity; identity uses no `FileId`, whole-file content hash or random nonce.
- An ADT's declaration schema is the canonical encoding of the nominal type graph reachable
  from it (constructor names and order, expanded field types, parameter kinds,
  alpha-normalised dimension bindings); an unrelated source edit does not change it.
- A private class and its local instances keep an exact run-time key but do not enter the
  public interface; a class method passed as a value or captured by a closure keeps its full
  `ClassKey`.
- A public transparent alias carries its resolved template; a private helper alias is expanded
  at its definition, so consumers never see it.

## 7.4 Type-class instances and modules

- Importing a module imports its classes, instances and aliases. Imported instances take part
  in **global coherence** checking (overlap is `E0311`, and the message names both modules)
  but are not re-checked for orphans.
- Orphan rule: an instance MUST be defined in the module of the type or of the class,
  otherwise `E0604`; instances generated by `derive` follow the same rule.
- A `private class` is visible only in its defining module, and its instances do not enter the
  interface.
- At run time dispatch uses witnesses at **quantified call sites** only (§9.2.3); the checker
  MUST be able to prove that the witnesses of candidate instances are pairwise disjoint,
  otherwise it reports `E0311`/`E0310` at check time. It never guesses from run-time shapes.

## 7.5 Packages

### 7.5.1 Manifest

`aevia.toml`:

```toml
[package]
name = "kinematics"
version = "0.1.0"
entry = "src/main.aev"      # optional; default src/main.aev

[dependencies]
Units = "^1.2"                                        # official scope: aevia/units
Plot = { package = "apich/plot", version = "~2.1" }   # registry package
Kinematics = { path = "../kinematics" }               # local path package
Dev = { path = "../dev", package = "apich/dev", version = "^0.3" }  # path for local work, version once published
```

Status: the manifest reader reads **only** `[package]` (keys `name`, `version`, `entry`) and
`[dependencies]`. Other tables (`[compat]`, `[capabilities]`, `[build]`, `[profile.*]`) and
other `[package]` keys (`edition`, `authors`, `description`) are `E0605`. Description, licence,
keywords and repository are supplied as options to `aevia publish` (§9.13). Capability
declarations have no run-time permission check; only `--caps` exists (§9.14). There is no
`git` dependency (`E0605`).

- `name` is the package name; `version` is SemVer 2.0 `major.minor.patch`, optionally with
  `-pre-release`; a published version MUST NOT carry `+build` (two versions differing only in
  build metadata have equal precedence and the registry cannot tell them apart).
- A dependency has one of three sources: registry, path, or both together. The registry is
  content-addressed and signed.
- The `entry` MUST define `main : IO Unit` for `aevia run`. Without `main` the CLI reports
  `this program has no main` (no diagnostic code, exit code 1); a library package is only
  `import`ed.

#### 7.5.1.1 Dependency declarations and version requirements

A key of `[dependencies]` is an **import root** (`UpperCamelCase`, no `-`; otherwise
`E0605`). Values take these forms:

| Form | Meaning |
|---|---|
| `Units = "^1.2"` | an official-scope package `aevia/units`. The package name is the key in `snake_case` (`UnitsExtra` → `aevia/units_extra`); an acronym key (`FFT`) needs an explicit `package` |
| `Plot = { package = "apich/plot", version = "~2.1" }` | a registry package named `owner/name` |
| `Lib = { path = "../lib" }` | a local path package |
| `Dev = { path = "../dev", package = "apich/dev", version = "^0.3" }` | both: `path` when building this package from source; only `package`/`version` when it is used as a registry package (the `path` directory does not exist). **A published manifest MUST NOT contain a path-only dependency**; the server rejects it at publish time (`E_MANIFEST`) |

`version` is a **version requirement** (Cargo syntax and meaning):

| Form | Meaning |
|---|---|
| `1.2.3`, `^1.2.3` | `>=1.2.3, <2.0.0` (a bare version is `^`) |
| `^0.2.3` / `^0.0.3` | `>=0.2.3, <0.3.0` / exactly `0.0.3` |
| `~1.2.3`, `~1.2`, `~1` | `>=1.2.3, <1.3.0`, `<1.3.0`, `<2.0.0` |
| `1.*`, `1.2.x` | wildcard |
| `>=1.2, <2`, `=1.2.3`, `>1`, `<=1.2` | comparators; a comma means conjunction |
| `*` | any **release** version |

A pre-release (`1.0.0-rc.1`) satisfies a requirement only if some comparator names a
pre-release of the **same** `major.minor.patch`; `^1` and `*` never select a pre-release, so
`aevia update` does not silently adopt a release candidate.

#### 7.5.1.2 Resolution: at most one version per compatibility series

The resolver is `aevia-resolve` (a pure crate with no I/O, shared by the toolchain and the
registry):

1. It satisfies every requirement, the dependencies of each selected version are satisfied
   too, and there is **no dependency cycle** (a cycle is an error and is listed).
2. **At most one version per compatibility series of each package.** The series is the major
   version for `>=1.0` (`1.x`), `0.y` for `0.y`, and `0.0.z` for `0.0.z`. So `1.4` and `1.9`
   never appear together, while `1.x` and `2.x` can coexist with distinct nominal types
   (§7.3). Coexistence needs the two edges to use **different import roots**; the
   program-wide import-root table still maps one root name to one package, otherwise `E0606`
   (the message names the two contending versions).
3. A **locked** version is preferred; otherwise the **highest** compatible version is chosen. A
   `yanked` version can be selected only if the lock file already contains it.
4. The result does not depend on index iteration order; when unsatisfiable, the explanation is
   derivation-style: which requirement could not be met, which candidates were tried and why
   each failed.
5. The implementation is a backtracking solver with conflict-directed backjumping (not PubGrub,
   because one requirement can be satisfied by several series).

The index `deps` of a registry package records only `package`, `req` and, when it differs from
the default import root, `import`.

### 7.5.2 Lock file

`aevia.lock` records each dependency's exact version, source and digest. A build MUST use the
lock file; `aevia update` updates it, and under `--locked` any change is an error. Format
(`version = 1`, fixed fields and order, packages sorted by `(source, import)`, so an unchanged
resolution yields a byte-identical file):

```toml
[[package]]
name = "apich/plot"          # registry package: registry identity owner/name
version = "2.1.3"
import = "Plot"              # import root (one entry per alias, as for path packages)
source = "registry+aevia"    # the registry's logical name, not a URL: a mirror change does not touch the lock
checksum = "sha256:…"        # archive digest from the index (the signature covers it)

[[package]]
name = "dep"                 # path package: [package] name
version = "0.2.0"
import = "Dep"
source = "path+../dep"
hash = "fnv1a64:…"           # digest of manifest + source tree + entry
```

- Paths are relative to the root package; the digest of a path dependency covers the canonical
  manifest, `src/**/*.aev` and the entry file, so changing `entry` or a dependency edge also
  invalidates `--locked`; a symlink cycle in a source directory is `E0607`.
- Index: `.aevia/registry/index/<owner>/<name>` (shareable through `$AEVIA_REGISTRY_DIR`) is
  the verbatim body of `GET /api/v1/index/{owner}/{name}`. Resolution reads only these files;
  when one is missing, `aevia add/update/tree` call `aevia-pkg index` to download it
  (forbidden by `--offline`). **The compiler itself never touches the network.**
- Source: `aevia fetch` (= `aevia-pkg vendor`) downloads archives, verifies the digest and the
  OpenPGP/SSH signature, unpacks into `.aevia/registry/src/<owner>/<name>/<version>/` and
  writes `.aevia-pkg-checksum`. A build MUST read only these directories and rejects a
  directory without the marker or with a marker that differs from the index digest (`E0607`),
  and a directory whose manifest dependencies disagree with the index record (`E0605`).
- `aevia update` without arguments ignores the lock and takes the newest versions;
  `aevia update owner/name…` moves only the named packages. `aevia tree` prints the dependency
  tree. `aevia clean` keeps `.aevia/registry`; only `aevia clean --registry` removes it.
- `aevia add` resolves the whole candidate graph before staging the manifest and lock; an
  ordinary I/O failure restores the original manifest, and a failed resolution rewrites
  nothing.

Status: the resolver lets incompatible series of one package coexist, but giving each package
its own dependency namespace (a per-package `Roots` lookup) is not implemented, so coexistence
requires distinct import roots.

### 7.5.3 Directory layout

```
kinematics/
  aevia.toml
  aevia.lock
  src/
    main.aev
    kinematics.aev
    thermo/ideal_gas.aev
  tests/fall_time.aev
  examples/projectile.aev
  docs/
```

- Every `.aev` file under `src/` is a module; `main.aev` is the default entry. `aevia test`
  compiles and runs `tests/`.
- `aevia run --example name` resolves `examples/<name>.aev` by name (a name is not a path; one
  with a path separator is rejected) and lists the available examples when it finds none;
  `--example` and a file argument are mutually exclusive.
- `aevia doc` writes Markdown API documentation, by default into `docs/`.

### 7.5.4 Tests

```aevia
# tests/fall_time.aev
open Kinematics

test "fall time from 20 m" =
    approx_eq(fall_time(20.0 m), 2.019 s, 0.001 s)
```

- A `test` declaration is a `Bool` expression; `aevia test` runs them all. `approx_eq(a, b, tol)`
  requires `a`, `b` and `tol` to have the same dimension.
- To test a compile-time error use `compile_fail("<source>")` (§8.13): it hands a source
  fragment to the compiler and requires that **compilation fail**; only fragments without
  imports are supported at present. `@compile_fail` is not an attribute (an unknown attribute
  is `E0210`). Property tests use `forall` (§8.13).

### 7.5.5 Documentation

- A `##` doc comment attaches to the next declaration and contains Markdown; `aevia doc`
  extracts type signatures, units and examples, and reports undocumented declarations in the
  index.
- Code blocks tagged `aevia` in doc comments are compile-checked: `aevia test --doc [FILE…]`
  extracts the fenced blocks and compiles each one; the first line of a block may be
  `compile_fail` or `compile_fail EXXXX`. A snippet compiles as its own synthetic module: the
  declaring module's `import`/`open` lines are copied in, but the module's own declarations
  do not enter the snippet implicitly; import the declaring module explicitly when needed. A
  failing dependency or inherited import does not satisfy `compile_fail`.

## 7.6 Versions and compatibility

- `edition` selects syntax and semantics (the manifest does not read it yet, §7.5.1); libraries
  follow SemVer: a breaking change raises major, a new API raises minor, a fix raises patch.
- A change to a type or dimension signature, to a unit's scale or offset, or adding or removing
  an instance is breaking; adding a unit is minor.

## 7.7 Module errors

| Code | Situation |
|---|---|
| `E0601` | module name does not match its path |
| `E0602` | cyclic import |
| `E0603` | two modules open the same name with different types |
| `E0604` | orphan instance |
| `E0605` | invalid package manifest (unknown table or key, non-SemVer version, `git` dependency, dependency key not a module name, lock and index disagree) |
| `E0606` | dependency version conflict (unsatisfiable requirement, dependency cycle, import-root contention) |
| `E0607` | module or package not found (including registry index not cached, package not fetched, checksum marker mismatch, symlink cycle) |
| `E0608` | the embedding API `aevia_core::run` got no runnable module (the CLI prints only a message when `main` is missing) |

The next chapter gives the standard library's signatures.


<a id="ch-08-stdlib"></a>

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


<a id="ch-09-runtime"></a>

# 09 — Runtime

The runtime turns source into values, values into rssn terms and terms into results. Its goals
are **fast start-up, fast interaction, fast numeric hot spots and controllable symbolic
work**. This chapter defines the components, the data flow, the threading model and the
performance boundaries.

## 9.1 Components

| Crate | Responsibility |
|---|---|
| `aevia-dim` | dimension algebra, unit table, unit systems; no dependency on the parser or the runtime |
| `aevia-syntax` | lexing, layout, parsing, AST, diagnostic rendering |
| `aevia-types` | kinds, types, dimension unification, inference, type classes, exhaustiveness, `.aevi` persistence |
| `aevia-ir` | Core IR and lowering from the checked AST; data only, no dependency on `aevia-core` |
| `aevia-rssn` | the rssn bridge: `Session`, conversion between the two value models, Kernel Manifest, `Rssn` module generation, JIT pass-through |
| `aevia-std` / `aevia-stdlib-model` | the standard library (Aevia source plus Rust numeric kernels) / the source of truth for its generation |
| `aevia-core` | values, tensors, closures, IO, the interpreters, built-ins, the symbolic layer, the module loader (`driver`/`manifest`), the disk cache, parallelism, the optional JIT |
| `aevia-fmt` / `aevia-doc` | the formatter / `##` documentation and doctest extraction |
| `aevia-analysis` / `aevia-lsp` | editor-independent semantic queries / the LSP adapter (the only crate that knows JSON-RPC) |
| `aevia-resolve` | version requirements and dependency resolution (§7.5.1.2), a pure crate |
| `aevia-cli` | the `aevia` executable |

Dependency direction: `dim → syntax → rssn → types → ir → core`; `cli` and `lsp` depend on
everything above them. The dimension algebra in `types` is pure Rust and needs no rssn run
time; `core` uses the backend through `aevia-rssn`.

## 9.2 Compilation pipeline

```
source .aev → lexing + layout → token stream (with INDENT/DEDENT) → parsing → AST
  → name resolution (module paths, unit resolution) → type/dimension checking, class resolution, exhaustiveness → checked AST
  → lowering → Core IR (ANF) → interpretation / optional JIT → values
```

### 9.2.1 AST

The AST keeps source positions for the formatter and the LSP; desugaring (`|>`, `√`, `!`, `@`,
the `_` placeholder, `do`, record update, tensor literals, indexing) happens before lowering.
Unit literals are resolved during checking: name → unit table → scale, dimension and affine
flag.

### 9.2.2 Core IR

```
core ::= let x = core in core | lambda(params, core) | app(core, [core])
       | if(core, core, core) | match(core, [arm]) | prim(op, [core])
       | construct(tag, [core]) | project(core, field) | update(core, [(field, core)])
       | tensor_lit([core]) | index(core, [index]) | slice(core, [slice])
       | do([stmt]) | return(core) | literal(value) | var(x)
```

Core IR is in ANF: every intermediate result is bound to a name, which helps closure
conversion, the JIT and debugging. Pattern matching is kept, and the interpreter matches by
tag and field.

### 9.2.3 Type erasure and the trust boundary

Dimensions and types are erased in Core IR, but **run-time witnesses** are kept: the optional
display unit of a `Real`/`Float`, the `is_point` flag of a `Point`, the run-time dimension of a
`Dyn`, and the shape and storage type of a tensor. Erasure lets `mean<L>` and `mean<T>` share
code; witnesses are used only for display, `Dyn` and FFI checks.

**Trust boundary (normative).** The runtime executes the checker's proofs; it does not
re-prove them:

1. **A checked source call does not re-validate its arguments.** If the callee's activation
   evidence is *closed* (the signature and all Type/Rep/Dim/Nat/Row projections are
   variable-free), the runtime uses it directly, without traversing arguments or unifying the
   signature again. If it is not closed, the projections the checker recorded at that call site
   are substituted first; only if it is still open are the remaining binders solved from the
   arguments' run-time witnesses (for example a tensor length known only at run time). Data is
   used only to solve unknowns the static evidence left open, never to re-check a closed
   signature.
2. **A checked constructor application does not traverse its payload.** A saturated
   `Cons(x, acc)` is the primitive `Construct` in Core IR; nominal identity is fixed at
   lowering and the type witness is the checked type at that position (substituted by the
   activation evidence, memoised per position).
3. **Full validation at boundaries.** Where a value's origin is unknown it is still validated:
   arguments of an Aevia closure called directly by a host (an embedder, a kernel callback,
   the `Rssn` dynamic ABI), the error payload propagated by `?`, and results of
   `from_value` and IO decoding. A built-in such as `map` or `fold` calling a callback counts
   as a host call and takes the "not closed" path of rule 1.
4. **Literals are constants.** The value of a numeric or unit literal depends only on its
   position and the activation evidence and is memoised by (position, activation evidence,
   module); a unit expression is not re-resolved on each evaluation.

These rules change no program's result, only the amount of work: a 10^5-step simulation over a
dimensioned record dropped from 22.7 s to 0.9 s, and building and traversing a 2×10^4-element
list from over 100 s to 0.3 s (optimised build).

A checked closure keeps the `ModuleEvidence` of its check through lexical scope (expression
types, literal representations, dimension checks, RSSN call keys, dictionaries, constructor
references, the unit table); a later check of the same source does not rewrite an older
closure's evidence, and a checked closure from a different interpreter session (including REPL
`:reset`) is refused with `E0809`. An explicit generic selection carries the origin of the
actual declaration and its Type/Rep/Dim/Nat/Row projections in declaration order (phantom
parameters are kept); higher-order value flows that cannot be proved stay conservative, and
this is not a provenance analysis for arbitrary rank-N polymorphism. A native self tail call
first verifies that the global still points at the same canonical closure and compares the
five concrete projections one by one; if that fails it returns to the interpreter trampoline.

Dynamic-ABI containers: a checked `Tensor<1, Value>` keeps one-dimensional boxed elements and
is not stacked when elements happen to be tensors; `[to_value(a), to_value(b)]` always carries
two ABI arguments.

**Witness table.** Type-class dispatch reads only these witnesses; a type without a witness
cannot be used for quantified run-time dispatch (rejected at check time, §3.8.4/§7.4):

| Type shape | Run-time witness |
|---|---|
| `Int` / `Real` / `Float` / `Complex` scalar | `NumKind` (integer / exact / float / complex) |
| `Tensor<n, τ>` | rank + storage class (`Exact`/`Float`/`Complex`) |
| nominal type `T<...>` | `(defining module, name)` + recursive argument witnesses |
| record | canonical label set (sorted) |
| `Bool` / `String` / `()` / `Prop` | their own tags |
| `Point` / `Unit` value / function / `Dyn` | **no witness** |

- **Concrete head type**: the checker records the exact instance key (class + canonical head);
  Core IR carries the key and the interpreter runs that instance body directly.
- **Quantified head type**: run-time selection by witness is allowed only if the candidate
  instances' witnesses are pairwise disjoint; otherwise the check reports `E0311`/`E0310`.
  Witnesses are not total: `Tensor<1, Int>` and `Tensor<1, Real>` have the same storage class,
  so two such instances cannot coexist.
- A record witness is **structural**: record types with the same label set are
  indistinguishable and cannot each have an instance; nominal types are told apart by module
  and name.
- There is no run-time multiple dispatch and no per-type or per-dimension monomorphisation:
  `mean<L>` and `mean<T>` MUST be the same code (§4.4.4, §12.8.13). Representation
  specialisation is a per-operation, tag-guarded fast path.

## 9.3 Interpreter

### 9.3.1 Environments

A local environment is an immutable persistent frame (`Rc`); a closure captures its
environment frame; the global environment holds module exports, the Prelude, the unit table
and class-instance dictionaries. Recursive closures are resolved through scope bindings and use
`Rc` environments; there is **no** session-level cycle collector. The parent chain of an
exclusively owned Scope is released iteratively; reclaiming arbitrary closure cycles is not
promised.

### 9.3.2 Calls

- Arguments are evaluated left to right; a built-in is a Rust function pointer plus an arity,
  and a user function is a closure.
- A self tail call uses a trampoline (`app` in tail position returns `TailCall` and the main
  loop replaces the current frame), so the Rust stack does not grow.
- The bounds on non-tail recursion are in §6.9.3 (`E0805`).

IO dispatch uses an explicit work stack; a Core IR `do` is one shared slot frame plus a
sequential step table, and no closure is created per suffix. Flat statement sequences and IO
tail recursion in the last action do not accumulate dispatch continuations in proportion to
statement count. `Interp::max_io_steps` (default 100,000,000) bounds dispatch steps and
`max_io_depth` (library default 2048, relaxed to 10⁶ by the CLI) bounds pending continuations;
exceeding either is `E0805`, and each `run_io` resets the counters. The CLI's `--budget` sets
both the tail-call and the IO-step budget; it is not a time or memory limit.

The control-flow marker of `?` carries the real function or IO boundary the checker certified;
failure propagation rebuilds `Err`/`None` at the target's success type, and an ordinary
run-time error is never mistaken for a success at a function boundary. A deferred action keeps
the check evidence of its definition; running an old action after a session change or reset
reports `E0809`. Built-in abstract errors of the Prelude such as `ParseError` and `IOError`
still use a text payload; `?` can still propagate them, but they are safely refused when
round-tripped through an opaque `Value`; a same-named ADT written in source is not covered by
this compatibility.

### 9.3.3 Pattern matching

A constructor pattern compares the tag and matches the payload recursively; a record pattern
looks fields up by name and is open; a rank-1 tensor pattern matches by length and elements, and
`..rest` produces a `SharedSlice` (every storage representation can be shared, and consecutive
tail operations point at the same original tensor with no chain of nested views); a literal is
compared exactly; a guard is evaluated after matching and MUST be a concrete `Bool`.

The AST and IR paths share an explicit work stack and a staged binding table: bindings are
committed only after all structural checks succeed, and a failure does not modify an existing
scope. Constructor evidence is kept with the lexical module revision and validates the
declaration schema, ordinal, name and field count. Storage work of element-by-element tail
recursion over a tensor is linear. The static exhaustiveness matrix handles at most 8192
states and 128 levels of expansion; when the budget runs out it conservatively reports `E0309`
and suggests `_`.

### 9.3.4 Errors and the stack

Every Core IR node carries a source position. The error object:

```
EvalError {
    code: E08xx,
    message: String,
    span: Span,
    stack: Vec<Frame>,   # function name + call site
    notes: Vec<String>,
}
```

`panic` prints the stack; a `Result` error is not printed and is propagated by `?`. Stack and
tail-call budgets are in §6.9.3.

## 9.4 The symbolic bridge

### 9.4.1 Session and Term

rssn's `Session` owns a term graph, and `Term<'s>` is a handle with a lifetime; `Session` is
built on `RefCell` and is not `Sync`. Policy:

- **One `Session` per thread** (`thread_local!` or the worker pool); terms kept across calls are
  stored by `u32` id, and `TermId` carries a `session_id` to prevent cross-session misuse.
- `compute` clones the template graph, which is acceptable for heavy requests and unacceptable
  for every `+`; so arithmetic only builds terms (`Session::call_value`) and never calls
  `compute` (§6.3.3).

### 9.4.2 Symbol table

```
SymbolicCtx {
    session: Session,
    symbols: HashMap<SymbolKey, TermId>,   # user symbol -> term
    display: HashMap<TermId, String>,      # display names
    config_cache: HashMap<ConfigKey, Config>,
    term_cache: LruCache<TermHash, TermId>,
}
```

- `symbol("x")` produces an internally unique name `x#n`. Symbols on the static path are
  dimensionless; only FFI and export pass the full name to `Config::dimension`, and the
  dimension of a `Dyn<ρ>` is kept by the host.
- `Config` is cached by (rule set, assumptions, dimension mode, budget); `term_cache`
  de-duplicates by structural hash.

### 9.4.3 Request construction

| Aevia | rssn request |
|---|---|
| `diff(f)` | `diff(term, ξ)` |
| `antiderivative(f)` | `integral(term, ξ)` |
| `defint(f,a,b)` | `defint(term, ξ, a, b)` |
| `limit(f,a)` | `limit(term, ξ, a)` |
| `solve(f)` | `solve(term, ξ)` |
| `sum(f,a,b)` / `product` | `sum(term, κ, a, b)` / `product(term, κ, a, b)` |
| `series` | `series(term, ξ, a, n)` |
| `dsolve` | `dsolve(eq, y(t))` |
| `simplify` | `compute(term, Config::symbolic())` |
| `approx` | `compute(term, Config::numeric(tol).bind(...))` |

`ξ` and `κ` are fresh symbols; `a` and `b` are evaluated terms or numbers. `compute` returns an
`Answer`; `Answer.term` is the answer term and `Answer.dimension` is the dimension rssn
inferred.

### 9.4.4 Dimension validation

After each `compute`:

1. Static path: the term graph is dimensionless, the checker already knows the answer's
   dimension, and no run-time comparison is made.
2. Numeric path: `Answer.dimension` is authoritative (do **not** re-infer it from
   `Answer.term`: the term of a Numeric target is a dimensionless floating literal); if it is
   `None` (`DimensionMode::Off`) fall back to the static dimension.
3. If the optional diagnostic is on and `Answer.dimension` disagrees with the static
   expectation, report `E0809` and print the request, the answer and both dimensions.
4. Convert `Answer.term` back to an Aevia value.

For `Dyn<ρ>` no static dimension is compared; the host-held `DimTerm` is the run-time
dimension.

### 9.4.5 Numeric path

- `Float` scalar arithmetic goes straight to f64 without entering rssn; `sin`, `exp` and the
  like use Rust `libm` or rssn kernels.
- `approx` of a `Real` uses `Target::Numeric`; the result carries an error ball and Aevia takes
  the midpoint.
- `F64` tensor operations use ndarray/faer; `Exact`/`Boxed` use the element-wise scalar path.

## 9.5 Tensor runtime

- Storage: `Arc<TensorData>`; a slice is a view (offset + strides) and `reshape` is a view when
  contiguous and a copy otherwise. There are no writes: `update`/`map` produce new tensors,
  reusing storage in place when the reference is unique. Semantics are in §6.6.
- Element-wise `F64` loops are unrolled 4/8-way; `sum` uses pairwise/Kahan summation.
  `matmul`/`solve`/`eig`/`svd`/`qr`/`lu` call rssn's numeric kernels (faer); `fft` calls rustfft;
  `interp`/`spline` call rssn kernels.
- Shape checks run before each operation and errors name both shapes and the operation;
  refined shapes are checked at compile time, rank-only shapes at run time.
- Devices: a tensor carries `device` metadata; `to_device` and `on(device, ...)` transfer and
  scope execution. CPU is the default; GPU and distributed are run-time targets, not type
  parameters.
- Sparse: `Sparse<F,shape,τ>` shares `@` dispatch with dense tensors; the `Layout` class
  provides `nnz`, `to_dense`, `from_dense`, `matmul` and `transpose`.
- Plugins: `Rssn.register` (alias `register_plugin`) registers operators, rewrites and units into
  the current RSSN session; Aevia sees them through manifest reflection with no recompilation.

## 9.6 JIT

### 9.6.1 Triggers and gates

The manifest's `Features` are read first:

- `features.jit = false`: `@jit` is `E1001("core.jit")` and the function is only interpreted;
- `features.jit_branch = false`: a function containing `if`/`match` is not JIT-compiled;
- `features.jit_index = false`: a function containing tensor indexing is not JIT-compiled.

Past the gates the function must also: contain only `Float` scalars, arithmetic and elementary
functions (with `if`/indexing as the flags allow); be called more than a threshold (default
1000) times or carry an explicit `@jit`; and have fixed input and output types (no symbols,
no `Dyn`, no records or variants).

### 9.6.2 Procedure

1. Lower Core IR to rssn's LIR (`rssn::backend::lir`): a typed register machine with basic
   blocks, block parameters, direct (possibly recursive) calls, checked integer operations,
   bounds-checked slices and *bail* exits. Each activation (argument kinds + result kind +
   literal specialisation) produces two LIR functions: the entry form returns
   `(status, result, next arguments, remaining fuel)` and the value form returns only the
   result, for recursive calls in value position.
2. Only when `features.jit` is true is it compiled by Cranelift (`rssn::backend::lir_jit`).
3. Cache key: function identity + argument kinds + result kind + literal specialisation.
4. Before entering machine code, check that every self-call site still points at this
   activation and that the built-in names it uses still resolve to built-ins (not shadowed).
   On a bail the interpreter re-executes from the same arguments (the machine code is pure, so
   redoing is unobservable).

### 9.6.3 Semantics and limits

- Compilable: capture-free pure functions whose parameters and result are dimensionless
  `Float`, `Bool` or 64-bit `Int`; `if`/`match` (literals, wildcards, bindings, guards) really
  branch; self tail calls become jumps with fuel checks, handing the trampoline back to the
  interpreter when fuel runs out; self calls in value position become native recursive calls
  (depth limit 512, beyond which it bails).
- `Int` `+ - *` are overflow-checked; on overflow the code bails and the interpreter completes
  with arbitrary precision. Mixing `Int` and `Float` promotes as in §6.3.2.
- Elementary functions (`sqrt`, `cbrt`, `exp`, `ln`/`log`, `log2`, `log10`, trigonometric and
  hyperbolic functions and their inverses, `√`) are guarded: any case in which the interpreter
  would not simply return `f(x)` (domain errors, the extended-real special values of §6.3.4,
  `±oo`, `ln(0)`) bails. `sqrt` and `cbrt` use `powf(1/n)` like the interpreter and agree bit
  for bit.
- Symbolic values, records, variants, closures, tensors, exact numbers and units do not enter
  the JIT.
- JIT results MUST agree bit for bit with the interpreter; a case that cannot guarantee this
  MUST bail, and a disagreement is a bug.
- `map_floats` (the device seam) still lowers to a straight-line tape, because a GPU needs a
  program without control flow.

Optional build: `cargo build -p aevia-cli --features jit` enables RSSN's Cranelift backend
(Rust 1.97 or newer). It is off by default, and `@jit` is then `E1001`. Reference test:
`tests/jit_native.aev`.

### 9.6.4 Device targets

`@jit` stays a scalar, CPU, machine-code path. Data-parallel execution on `Cpu`, `CpuParallel`
and `Gpu(n)` is a separate mechanism (rssn `Device`, §10.6.5); a function that qualifies for
`@jit` also qualifies for the device batch path.

- `@jit(target = Cpu)` equals `@jit`; `target = Gpu(n)` selects the **batch** device path, and
  without `core.gpu` the check reports `E1001("core.gpu")`.
- The device path and the JIT share one lowering (Core IR → SSA) and one gate (§9.6.1); they
  differ only in backend: the JIT compiles a one-point function, the device launches one
  kernel over `rows` rows.
- `CpuParallel` agrees bit for bit with `Cpu` and the thread count changes no result (§9.7.2);
  `Gpu(n)` is `f32` and agrees only within rounding error, so it does not enter pure
  functions or bit-deterministic primitives.
- If the device is unavailable, the function cannot be lowered or the device fails, evaluation
  falls back to the interpreter row by row and the values are unchanged;
  `AEVIA_PARALLEL_TRACE=1` prints the reason.

## 9.7 Memory and concurrency

### 9.7.1 Memory

Tensors are `Arc` + copy-on-write, slices share, and storage is reused in place when the
reference is unique; closures are `Rc` (no cycle collector, §9.3.1); symbolic terms are rssn's
hash-consed graph and term ids do not cross sessions; a REPL session arena grows and `:reset`
releases it.

### 9.7.2 Parallel evaluation: shared-nothing replicas

- Pure functions may run in parallel; the result order of `par_map(f, xs)` is fixed and **the
  thread count changes no result**.
- **Shared-nothing replica design.** The interpreter is built on `Rc`, and converting the
  runtime to `Arc` would make every program pay for the few parallel ones. So each worker
  thread holds a **replica**: a host-installed `Replica` factory rebuilds the interpreter from
  the same source on that thread (once per thread, then reused). Threads exchange **portable
  values** (`Portable`): numbers with dimension and display unit, strings, tensors, records,
  variants, constructors and IR closures.
- **Closures cross threads as code identity plus captured data.** A lambda is "the function's
  source position plus its captured values"; the position is a **module identity** (logical
  source path + owning package) plus a byte offset (`FileId` is assigned per load and does not
  cross threads), and the replica resolves it back to the same function through the
  same-named module.
- **Guarantee: parallelism never changes a value.** When a value cannot cross threads (a
  symbolic term may hold a handle of the thread's session, a kernel handle, an IO action, a
  closure with activation-time generic evidence), when there is no replica factory, under
  `--jobs 1`, or when a worker fails, the primitive falls back to sequential evaluation, and the
  sequential recomputation gives the value or error the program would have given (with
  position and call stack). `AEVIA_PARALLEL_TRACE=1` prints the reason for each fallback.
- **Determinism.** Chunk boundaries depend only on the input length, not on the thread count.
  `par_reduce(f, zero, xs)` requires `f` to be associative and `zero` an identity; it folds
  fixed chunks of 1024 elements and merges the chunk results in order, so a floating-point
  reduction is **bit-identical** on any number of cores and in the sequential fallback.
- Primitives: `par_map`, `par_tabulate(n, f)`, `par_reduce`. `--jobs N` (or `AEVIA_JOBS`) sets
  the worker count, defaulting to the available parallelism; replicas do not nest parallelism.
  Each worker thread has one kernel `Session`.
- **Parallel numeric kernels.** `sparse @ dense` (including `Bcsr`) runs in fixed 256-row blocks,
  each output row accumulated sequentially by one work item, bit-equal to the sequential
  kernel; `sde_ensemble_diag/general` runs through `par_map` replicas, and the member seed
  `ensemble_seed(seed, i)` depends only on `(seed, i)`. Deliberately not parallel: the time
  steps of a single ODE/SDE path, Krylov iterations and `tabulate`. A value with a closure field
  (the `at` of `ode_solve`) cannot cross threads and falls back to sequential.

### 9.7.3 Concurrent tasks

```
spawn : (() -> IO a) -> IO (Task a)
await : Task a -> IO a
pure  : a -> IO a
```

- `spawn(f)` runs `f()` on a worker thread and returns a `Task`; `await(t)` waits and fetches
  the result.
- **A task's console output is buffered and written at `await`, in await order**, so program
  output is deterministic however tasks interleave; other effects such as files happen directly
  on the task's thread.
- If the thunk cannot cross threads the task runs on this thread at `spawn` (also buffered), so
  `spawn` is always available. A run-time error on a worker is reported at `await` (not
  re-run); a task result MUST be portable data, otherwise `await` reports `E0811`; a task can
  be awaited only once (the second `await` is `E0811`).
- `do` itself still runs sequentially; concurrency appears only explicitly through tasks.

## 9.8 REPL

```
$ aevia repl
Aevia — dimensions are types. Type `:help` for commands, `:quit` to exit.
> let g = 9.80665 m/s^2
> sqrt(2 * 20.0 m / g)
> :type sqrt
sqrt : forall t0, d0. Scalar<ρ0, d0> -> Scalar<ρ0, d0^(1/2)>
```

| Command | Effect |
|---|---|
| `:type e` | show the type |
| `:dims` | list the base dimensions (`Length`, `Mass`, `Time`, `Current`, `Temperature`, `Amount`, `Luminosity`) |
| `:units [d]` | list units, optionally for one dimension |
| `:ast e` / `:core e` | show the AST / Core IR |
| `:time e` | evaluate and report the time taken |
| `:latex e` | render the value as LaTeX |
| `:load f` | load declarations from a file |
| `:reset` | clear the session (starts a new session identity, §7.3) |
| `:quit` | exit |

Multi-line input (indented blocks) and Ctrl-D to exit are supported; declarations accumulate in
input order and a repeated definition shadows the old one. After re-checking the accumulated
source the REPL installs the complete check evidence, keeps a cursor over already executed
declarations and initialises only the new ones; constructors of a newly added type use the
exact witnesses of that check.

Status: cancelling the current evaluation with Ctrl-C is **not implemented** (the default
console handling terminates the process; cooperative cancellation needs an interrupt flag in the
evaluator).

## 9.9 CLI

```
aevia run [--example NAME] [file.aev] [-- args…]   run the package entry, a file or an example (program arguments follow `--`)
aevia check [--all] [file.aev…]                    type/dimension check without running
aevia watch [file] [--once | --runs N]             check repeatedly, re-checking only changed modules (§9.11.1)
aevia test [--filter PAT] [file.aev]               run `test` declarations
aevia test --doc [--filter PAT] [files…]           compile the aevia blocks of `##` docs (§9.12)
aevia repl                                         interactive session (§9.8)
aevia fmt [--check] [files…]                       format source, the package's src/ by default
aevia doc [--open] [--out DIR] [files…]            write Markdown API docs, default docs/
aevia bench [--filter PAT] [--iterations N] [--warmup N] [--save-baseline FILE]
            [--compare-baseline FILE] [--threshold RATIO] [files…]
aevia init <name>                                  create a package: aevia.toml, src/main.aev, aevia.lock
aevia add <path|owner/name[@req]> [--as NAME] [--offline]   add a path or registry dependency and re-resolve the lock
aevia update [owner/name…] [--offline]             re-resolve aevia.lock to the newest compatible versions
aevia fetch [--offline] [--force]                  download and verify the registry packages aevia.lock names
aevia tree [--offline]                             print the dependency tree
aevia clean [--registry]                           remove `.aevia/` (`.aevia/registry` stays unless asked)
aevia lsp [--stdio]                                language server (§9.10)
aevia rssn [--audit] [--emit [--out DIR] [--namespaces]]   audit a Kernel Manifest / generate the Rssn module (chapter 13)
aevia login                                        bind a signing key to your registry account (browser)
aevia publish [--no-verify] [--dry-run] [options…] check, format-check, sign and upload this package
aevia search <query>      aevia info <owner/name>  search the registry / show a package page
aevia yank <owner/name@version> [--undo]           withdraw or restore a version
aevia verify <owner/name@version>                  check a download against its signature
aevia docs [owner/name[@version]] [--open]         print (or open) the address of a package's documentation site
aevia pkg <command>                                every registry-client command (`key`, `status`, `lock`, …), forwarded to aevia-pkg
aevia --version
```

Global flags (allowed anywhere on the command line):

| Flag | Effect |
|---|---|
| `--max-stack MiB` | the interpreter thread's stack (default 1024) |
| `--budget STEPS` | tail-call/IO step budget before `E0805` (default 100,000,000; not seconds, §9.3.4) |
| `--caps LIST` | grant only these capabilities: `fs,net,env,rand,gpu` (empty list = sandbox everything; default all, §9.14) |
| `--jobs N` | worker threads for parallel evaluation (§9.7.2) |
| `--no-cache` | `check`/`run` neither read nor write the disk caches (§9.11) |
| `--out PATH` | output path (`rssn --emit`, `doc`) |
| `--color auto\|always\|never` | diagnostic colouring |
| `--locked` | refuse to rewrite an out-of-date `aevia.lock` (`update`/`add`/…) |
| `--release` | accepted but reported as having no effect (the interpreter has no optimisation profile; JIT: §9.6) |

- **Program arguments**: `aevia run [file] -- args…` hands only the strings **after** `--` to the
  program, which reads them with `args()` (§8.1.8, a `Tensor<1, String>`; `[]` when there are
  none). `check`, `test` and `bench` do not forward arguments.
- **Registry commands**: `login`, `search`, `info`, `yank`, `verify`, `key`, `status`, `lock` and
  `index` are forwarded to `aevia-pkg`; `add`, `update`, `fetch` and `tree` are native to the
  toolchain (they edit `aevia.toml` and `aevia.lock` and call the client only for the network
  part); `publish` is native too: it first runs `aevia check --all` and `aevia fmt --check` with
  this binary and, on failure, refuses before a key is touched (`--no-verify` skips this), then
  hands over to `aevia-pkg publish`. If `aevia-pkg` is not on `PATH`, point `AEVIA_PKG` at a
  built binary or run `cargo install --path platform/crates/aevia-pkg`.
- **Options of `aevia-pkg publish`**: `--key FINGERPRINT`, `--description TEXT`, `--license SPDX`,
  `--keyword K` (up to five), `--repository URL`, `--dry-run` (print the statement and the
  checksum, sign nothing) and `--allow-dirty`. There are no API tokens: every authenticated
  action is a detached signature, made with the publisher's own OpenPGP or SSH key, over a
  short-lived server-issued statement that names one archive, carries a single-use nonce and is
  valid for five minutes.
- `aevia docs` only prints the documentation address
  (`<registry>/docs[/<owner>/<name>[/<version>]]`; the registry comes from `--registry`,
  `AEVIA_REGISTRY` or the default `https://aevia.apich.org`); `--open` opens a browser when
  stdout is a terminal.

Status: the "packages" paragraph of `aevia --help` still says "there is no registry", which
contradicts registry dependencies and should be deleted; a prebuilt `aevia` binary older than
`publish`/`login`/`search`/`docs` does not list them in its help.

## 9.10 LSP

Supported capabilities:

- Diagnostics: type, dimension and unit errors, unused bindings, non-exhaustive matches.
- Hover: type, dimension, unit, documentation.
- Completion: module names, functions, units, constructors; **field completion** after `.` lists
  only the fields of the receiver's record type; **keyword completion** offers
  `do`/`let`/`match`/`with` only in expression position.
- Navigation: definition and type definition; `textDocument/references` lists every use of the
  same name in the loaded program (honouring `context.includeDeclaration`; a local binding stays
  within its file, while top-level declarations, imported and qualified names, types and
  constructors, and record fields declared in this document can span modules).
- Rename: `prepareRename` + `rename` return a `WorkspaceEdit` carrying document versions; they
  can rename local bindings, top-level declarations, imported names used in this program and
  record fields declared in this document, and refuse keywords, predefined names, unit names,
  imported names not edited by this document, and invalid or clashing new names (with an
  explicit error and never a partial edit).
- Signature help: inside call parentheses it gives the function name, the active parameter and
  the available overloads; trigger characters `(` and `,`, retrigger `,`. Limits: the active
  overload is chosen by arity; parameter labels show **types**, not parameter names;
  documentation comes only from the `##` above a declaration.
- Formatting: the same engine as `aevia fmt`. Code actions: add a type annotation, turn `degC`
  into `K`, extract a `let`, fix a dimension. Semantic highlighting: types, dimensions, units,
  operators. `workspace/symbol` lists the declarations of all modules of the loaded program.

The LSP reuses `aevia-syntax` and `aevia-types` and starts no runtime. For a saved file,
analysis loads its import graph through `aevia_core::driver` (the package root follows
`aevia.toml`), and cross-module navigation returns the defining module's file URI; unsaved
buffers keep the import graph too (one snapshot over all open physical buffers), so an
imported module not yet saved to disk still takes part in resolution; a virtual document
without a package root falls back to single-file analysis. A bounded LRU cache (default 4
programs) compares the actual source contents read, the existence and physical identity of
import candidates and the package-resolution context; it does not scan unrelated directories
and does not rely on `(len, mtime)`. When an imported buffer changes, diagnostics of the
affected open importers are republished. The exact capabilities `initialize` returns:

```json
{
  "positionEncoding": "utf-16",
  "textDocumentSync": { "openClose": true, "change": 2, "save": true },
  "hoverProvider": true,
  "completionProvider": { "resolveProvider": false },
  "definitionProvider": true,
  "typeDefinitionProvider": true,
  "documentSymbolProvider": true,
  "documentFormattingProvider": true,
  "codeActionProvider": true,
  "workspaceSymbolProvider": true,
  "referencesProvider": true,
  "renameProvider": { "prepareProvider": true },
  "signatureHelpProvider": {
    "triggerCharacters": ["(", ","],
    "retriggerCharacters": [","]
  },
  "semanticTokensProvider": {
    "legend": {
      "tokenTypes": ["type", "dimension", "unit", "operator", "function", "variable", "enumMember"],
      "tokenModifiers": []
    },
    "full": true,
    "range": false
  },
  "experimental": {
    "multiModuleAnalysis": {
      "savedFiles": true,
      "unsavedBuffers": "snapshot overlays for all open physical buffers"
    },
    "analysisCache": {
      "strategy": "observed source contents + import candidates + package resolution",
      "capacity": 4
    }
  }
}
```

`aevia-lsp/tests/protocol.rs` asserts this capability table (only implemented behaviour is
declared).

## 9.11 Build and package management

- The package graph is topologically sorted; the lock file makes builds reproducible (§7.5.2);
  build artefacts live in `.aevia/` and are not committed; the central registry is the default
  source (protocol: §13.7), and `add`/`publish`/`yank` use the same protocol.

### 9.11.1 In-process incremental checking

Checking a module is a function of its source text, `FileId`, the **checked interfaces** of the
modules it imports, its import bindings, its module identity and the check options.
`CheckCache` stores `CheckedModule` and diagnostics keyed by a structural fingerprint of these
inputs: a module whose inputs are all unchanged is not re-checked, and because the fingerprint
includes the imported checked interfaces, **an edit that does not change an interface does not
re-check its importers** (early cutoff). `FileId` is a position in the `SourceMap` and is the
same on every load while the import graph is unchanged; a cache entry is reused only under the
same `FileId`, so every span in a reused interface points into the text it was checked
against.

`aevia watch <file> [--once|--runs N]` checks repeatedly with one session cache and reports
"how many checked, how many reused" each round. The standard library and unchanged package
modules are checked once per session; while waiting it watches every input the load observed
(files read, and import candidates that did not exist then but would change resolution if they
appeared).

### 9.11.2 Cross-process whole-program cache

`aevia check <file>` records, in `.aevia/cache/` beside the entry, every input the load
observed (files read with their text hashes, import candidates that did not exist), the
compiler identity (the executable with its size and modification time), the rendering style and
the result. A later `check` whose inputs all still match replays the output and exit code
directly; any change (an edit, a new module that would be imported, a rebuilt compiler, a
different colour style) is a miss.

### 9.11.3 Per-module disk cache (`.aevi`)

One file per checked module, at
`.aevia/cache/modules/<module path>-<16 hex digits of the key>.aevi` beside the entry; standard
library modules are cached too, while the kernel namespace generated on the fly from the RSSN
manifest and check results containing errors are not. File layout: magic `AEVI`, format version
(`FORMAT_VERSION`, little-endian u32), a 128-bit key, the body length and an FNV-1a checksum,
and the body (`bincode` written by `persist::encode`: the entry's `FileId`, the interface
digest, diagnostics and the **complete** `CheckedModule`). Writes go to a temporary file and are
renamed; a missing, truncated, checksum-failing or key- or version-mismatched file is a silent
miss and is overwritten; at most 4 revisions per module name are kept.

- The **key** is a stable 128-bit hash (two-lane FNV-1a, not `DefaultHasher`) of: the module
  source, `FileId`, the module path and the nominal identity the loader gives (including package
  origin), import bindings, the **interface digest of every module in the dependency closure**,
  the check options (capability set) and the compiler identity (`aevia-core` version,
  `FORMAT_VERSION`, the RSSN manifest hash, the executable's size and modification time). In
  the interface digest every ADT is replaced by its symbolic definition, so it contains no
  `AdtId`. Hence an edit that leaves the interface unchanged re-checks only the edited module,
  an interface change re-checks all dependants, and a change of capability set, import bindings
  or compiler build is a miss.
- **`AdtId` does not enter the file (§7.3).** On encoding, each `AdtId` that occurs gets an
  in-file index and the ADT table records its full definition (`AdtKey` = module path + name +
  declaration schema, parameters, parameter kinds, constructors and field types, with cycles
  expressed by index); the reserved `Option`/`Result`/`Ordering` (0/1/2) are written as is. On
  decoding, the ADT table is first **re-interned** into this process's registry (a complete type
  with the same `AdtKey` reuses its `AdtId`, otherwise one is created; host and opaque types
  are `intern`ed by name) and only then is the body decoded.
- An entry is reused only under the same `FileId`, which is in both the key and the file.
  `LoadStats` gains `disk_hits` and `disk_writes`; the entry points are
  `driver::load_with_disk_cache` and `driver::load_incremental_with_disk_cache`
  (`DiskCache::beside(entry)`).

Status: known limits — each file carries the complete unit table and dimension environment and
is larger than the interface; hashing the interface digests of the dependency closure grows
linearly with closure size on large graphs; `watch`, the LSP and `test` do not use the disk
cache yet; old-revision cleanup is the heuristic "keep 4 per module name" with no global
capacity limit.

## 9.12 Tests and benchmarks

- Unit tests: `test` declarations, `aevia test [--filter PAT]`. Property tests: `forall` +
  `Arbitrary`.
- Doctests: `aevia test --doc [FILE…/DIRS…]` extracts every fenced block tagged `aevia` from
  `##` comments and compiles each. The first line of a block may be `compile_fail` (compilation
  must fail) or `compile_fail EXXXX` (it must fail and include that code); without a marker the
  block must compile. Each snippet is its own synthetic module: the declaring module's
  `import`/`open` lines are copied, imports resolve against the declaring file's package and
  adjacent directories, and the module's own declarations do not enter the snippet implicitly;
  snippets use in-memory source and write no temporary file; a failing dependency or inherited
  import does not satisfy an expected compile failure. `--locked` checks the package of each
  checked document, not the caller's working directory. Any failure exits 1; no checkable block,
  or an empty result after filtering, also exits 1. The extractor is `aevia-doc::doctest`.
- Compile-failure tests: `compile_fail("<source>") : String -> Bool` compiles a fragment in an
  isolated environment (no imports, the Prelude only) and is `true` when it yields at least one
  error; it does **not** check the error code, which the doctest marker `compile_fail EXXXX`
  asserts.
- Golden tests: snapshots of REPL output and error messages.
- Benchmarks: `bench` declarations; each reports the median, mean and 95% confidence interval of
  N iterations (default N=100; `--iterations`/`--warmup`). `--save-baseline FILE` writes JSON
  (`format=1`, `tool`/`version`, `profile=debug|release`, `iterations`, `warmup`, and per item
  `median_ns`/`mean_ns`/`ci_ns`); `--compare-baseline FILE --threshold RATIO` (default 0.05, may
  be written `5%`) compares medians and, above the threshold, reports `REGRESSION` and exits 1; a
  profile mismatch exits 2; a workload present on one side only is reported but not counted as
  a regression. Protocol: compare only release builds, on the same machine, median of N.

## 9.13 Performance targets

| Scenario | Target |
|---|---|
| start-up (empty program) | < 50 ms |
| REPL input to result (simple expression) | < 100 ms |
| `Float` tensor element-wise operations | same order as NumPy (≤ 2×) |
| `Float` matrix multiplication | same order as faer (≤ 1.2×) |
| simplifying a small symbolic expression | < 10 ms |
| heavy integration/solving | governed by the budget |
| JIT hot function | ≥ 5× the interpreter |

These are design targets, not specification guarantees; implementations should track them in
benchmarks.

## 9.14 Security and sandboxing

- There is no `unsafe` user code; file and network IO happen only through `IO` actions;
  capabilities (`fs`, `net`, `env`, `rand`, `gpu`) are checked by the runtime and are not type
  parameters (§13.9.6). The symbolic budget prevents unbounded simplification and the stack
  limit prevents unbounded recursion.
- A package build executes no arbitrary script and there is no `build.aev`; when code generation
  is needed use plugins or operators, not build scripts or macros.

```
with_cap : (String, () -> IO a) -> IO a
```

- `with_cap(caps, thunk)` runs the action `thunk()` builds while keeping only the capabilities
  listed in `caps` (comma-separated: `"fs"`, `"fs,rand"`; `""` grants nothing). Nested uses
  intersect, and the outer set is restored when the block ends, including by error.
- Actions that need a capability: file reads and writes, NPY, `plot`/`save` (`fs`), `random()`
  (`rand`). Without the capability a data action returning `Result` gives
  `Err("E0902: permission denied: …")` and any other action terminates with `E0902`; an unknown
  capability name is `E0302`.
- A program has every capability by default; `aevia run --caps fs,rand` starts with the given set
  and `--caps ""` sandboxes completely.

Status: `fs` and `rand` are actually checked; the standard library currently has no `net`, `env`
or `gpu` actions, so those names are accepted and take part in intersection. A missing backend
capability is `E1001` (§13.8). `[capabilities]` in the manifest is not read (§7.5.1). "A
configurable memory limit" exists today only as the interpreter thread's stack limit.

## 9.15 Error reporting

All diagnostics share the format of appendix B.1: code, message, primary location, label, notes
and help. A dimension error carries the canonical forms and the difference; a type error
carries the expected and actual types and, where possible, the "most likely fix"; a parse error
carries the expected token and the nearest legal repair.

The next chapter defines the concrete mapping between Aevia and rssn: which capabilities are
used directly, which need a wrapper and which are known gaps.


<a id="ch-10-backend"></a>

# 10 — The rssn backend mapping

Aevia does not reimplement computer algebra. rssn provides the term representation, the rewriting engine, the dimension layer, numeric kernels, the JIT and simulation; Aevia provides syntax, type and dimension checking, closures, records, tensor shapes, modules, IO and the orchestration of the standard library. This chapter defines the contract between the two: what maps directly, what needs a wrapper, and what the known gaps are.

## 10.0 Reflection first: no hand-written operator bindings

Aevia **does not maintain** binding tables such as `sin -> rssn::sin` or `det -> rssn::det`. At build time it reads rssn's **Kernel Manifest** and generates the `Rssn` module (contract in [13 — Extensibility](#ch-13-extensibility) §13.2):

```aevia
Rssn.sin(0.5)                       # numeric overload of a Derived operator: Real
Rssn.sin(x)                         # symbolic overload of a Derived operator: Dyn<term>
Rssn.det(M)                         # Overlay：Tensor<[n,n],Real> -> Real
Rssn.dynamic("myplugin_softplus", args)  # plugin/leaf: no signature
```

- `signature_source = Declared | Overlay | Derived` → typed declarations. `Derived` is a conservative signature inferred from registration data: `eval` → `(Real,…) -> Real` (a predicate gives `-> Bool`), and a non-leaf also gets `(Term,…) -> Term`; it does not distinguish `Tensor`/`Quantity`/`Int`, and IDEs mark it "conservatively inferred".
- `None` (leaf / non-callable) → a `dynamic` declaration. rssn validates at the **signature level** (`check_args`; an argument accepted by no overload is `OpError::Type`); the full type and dimension check belongs to the static checker or `Rssn.typed`.
- A new operator appears at the next build as soon as it is registered in rssn, with no change to the Aevia compiler.
- The generated source spells signatures exactly: dimension parameters are written as Aevia dimension variables (`Real[T]`, `Real[T^(1/2)]`), with variable names local to a declaration and **shared between parameters and return** — in `sde_wiener(a3: Real[T]): Tensor<2, Real[T^(1/2)]>`, `T` and `T^(1/2)` are the same variable. `Dyn<term>` remains only where a signature genuinely cannot be spelled: signatures the manifest itself declares as `Dyn(rep, any)`, rank-variable tensors (`transpose`/`rank`/`dot`/`norm`/`sum`; `Tensor<n, τ>` is `E0304`), type parameters, unknown-row `Table`, `Device`, variants, host opaque values and `Never`.
- The generated `Units.aev`/`Constants.aev`/`Rssn.aev` type-check with no diagnostics: for each declaration the generator emits only an `Rssn.invoke` with literal `op` + `key`; the checker validates those two literals against the pinned overload and types the result as that overload's declared result (with fresh dimension variables at each call). A non-literal key stays a dynamic `Value` and the run time answers `E1002`/`E1003`/`E1005`. The test `emit_typecheck` in `aevia-rssn` asserts this acceptance.
- Static overloads use `OpKey { rule_set, name, signature_hash }`; typed calls pin the overload through `Rssn.call_key`; dynamic calls go through `call_value`, choosing by the `Match` lattice when several registrations exist and reporting `E1002` on a tie (§13.4.1).

The kernel-side contract is in rssn's `docs/KERNEL.md`.

## 10.1 What rssn offers

```rust
let s = Session::new();
let x = s.sym("x");
let answer = s.compute(diff(sin(x) * cos(x), x), &Config::new())?;
```

- `Session` owns a hash-consed term graph; `Term<'s>` is a handle with a lifetime.
- `Config` specifies rule sets, the target phase (`Symbolic`/`Numeric`), bindings, assumptions, budgets, the dimension mode, dimension assumptions and custom base dimensions.
- `compute` copies the request into a clone of the template graph, runs e-graph rewriting, extracts the answer and imports it back into the session. `Answer` holds `term`, `value`, `error`, `reduced`, `dimension`, `diagnostics` and `report`.
- `Term::eval(bindings)` evaluates numerically without rewriting; `Term::compile(inputs)` compiles a closed form (the `jit` feature uses Cranelift); `Term::as_tensor()` turns nested `list` terms into `(shape, Vec<f64>)`; `to_latex()`/`to_typst()`/`to_pretty()` typeset.
- `sim::scenario::run(name, json)` runs a simulation; `sim::scenario::SCENARIOS` is the only source of scenario names (29 at present).
- `Session::register_unit(UnitDef)` registers a unit; `Session::manifest()` uses `KernelManifest::build_with_units`, so session-private units appear in that session's manifest.
- SDE: `rules::sde` exposes the callback-free `sde_wiener`/`sde_levy_areas` and the scheme metadata `sde_scheme_info(name) -> Tensor<1, Real>` as manifest operators (`features.sde`, capability `core.sde`). The solve loop needs host callbacks (`&Fn + Sync`) and is not in the manifest; Aevia's `sdeint_diag`/`sdeint_general` call `kernels::sde` on the host side.

What Aevia reuses directly:

| Capability | rssn module |
|---|---|
| Exact numbers (big integers / rationals / floats) | `graph::Number` |
| Term construction and hash-consing | `graph::Graph`, `Session::call_value`/`Session::parse` |
| Rewriting engine and rule sets | `graph::Engine`, `rules::*` |
| Calculus / algebra / equations / ODE / PDE | `rules::calculus`, `solve`, `ode`, `pde`, … |
| SDE noise primitives | `rules::sde` (manifest operators), `kernels::sde` (integrators) |
| Dimension inference, unit parsing | `graph::dim`, `rules::units` |
| Numeric kernels (linear algebra, FFT, ODE, optimisation, special functions) | `kernels::*` (`faer`, rustfft), `sim::integrators` |
| Simulation | `sim::scenario` |
| JIT | `backend::jit`, `Term::compile` |
| Typesetting | `io::latex`, `io::typst`, `io::pretty` |

What Aevia implements itself: lexing, parsing, layout and formatting; **compile-time** type and dimension checking; closures, records, variants, strings, sets and `IO`; the host representation of tensors, shape checking, broadcasting and slicing; modules, packages, the REPL and the LSP; orchestration and typed wrappers of the standard library; the run-time dimension check of `Dyn`.

## 10.2 Scalar mapping

### 10.2.1 Exact numbers

| Aevia | rssn |
|---|---|
| `Int` (arbitrary precision) | `Number::Int(BigInt)` |
| `Real` exact rational | `Number::Rat(BigRational)` |
| `Float` | `f64` / `Number::Float` |
| `Complex`, numeric | `num_complex::Complex64` (host) or an `a + b*i` term |
| `Complex`, symbolic | an `a + b*i` term |

- `Session::int`/`rational` accept only `i64`; for larger values use `Session::parse(&n.to_string())`.
- The `Exact(Number)` of `Real` maps directly and `Sym(TermId)` maps to an rssn term; the `F64` of `Float` does not enter rssn.
- The numeric path of `Complex` uses `Complex64`; the symbolic path builds `add(re, mul(im, i))`. The two paths are unified on the Aevia side because rssn's `Number` has no complex numbers.

### 10.2.2 Symbols

```rust
let name = format!("{base}#{id}");     // internal unique name
let term = session.sym(&name);
```

- Display and internal names are separate: the internal name of `symbol("x")` is `x#1` and it displays as `x`; the same symbol reuses the same `TermId` across `compute` calls.
- The static path's term graph is dimensionless and **no** dimension is passed for symbols; dimensions are the type checker's job. Full names are passed to `Config::dimension` only for FFI/export.
- Symbol assumptions (`positive`, `real`, `integer`) go in through `Config::assume`; `assuming` turns unary predicates (`x > 0`, `x >= 0`, `x != 0`, `x ∈ ℤ`) into `Facts`. **General equations and inequalities have no language entry point** (`Rssn` has no `assume` member and there is no `core.assume.general` capability row).

## 10.3 Operator mapping: a generated façade, not hand-written bindings

The tables below are **examples of the semantic mapping**, explaining how Aevia's syntactic sugar lands on rssn's structural operators and requests; the actual bindings are generated from the manifest.

### 10.3.1 Core arithmetic

| Aevia | rssn operator |
|---|---|
| `a + b` | `add(a, b)` |
| `a - b` | `add(a, mul(-1, b))` |
| `a * b` | `mul(a, b)` |
| `a / b` | `mul(a, pow(b, -1))` |
| `a ^ n` | `pow(a, n)` |
| `-a` | `mul(-1, a)` |
| `abs`/`sqrt`/`sin`/`exp`/`ln`/`atan2` | the same-named operator |

`Session::call_value(name, args)` resolves the overload, validates the arguments, builds the term and performs only the operator's own local rewriting (`add` is one n-ary flattening), with no global rewrite loop; whole expressions use `Session::parse`. `Kernel::call(&OpKey, …)` forwards to `call_key` and **pins** the overload `OpKey` points to; `Kernel::call_ho` likewise finds the registration by key.

### 10.3.2 Heavy requests

| Aevia | rssn request |
|---|---|
| `diff(f)` | `diff(term, ξ)` |
| `antiderivative(f)` | `integral(term, ξ)` |
| `defint(f,a,b)` / `limit(f,a)` / `series(f,a,n)` | `defint` / `limit` / `series` |
| `sum(f,a,b)` / `product(f,a,b)` | `sum(term, κ, a, b)` / `product(term, κ, a, b)` |
| `solve(f)` / `solve_complex(f)` / `nsolve(f,x0)` | `solve` / `solve_complex` / `nsolve` |
| `dsolve(eq,y,t)` | `dsolve(eq, y(t))` |
| `simplify(x)` / `approx(x)` | `compute(term, Config::symbolic())` / `compute(term, Config::numeric(tol))` |

### 10.3.3 Numeric kernels

| Aevia | rssn |
|---|---|
| `defint` (numeric) | `kernels::quadrature` |
| `odeint` | `kernels::ode` / `sim::integrators` |
| `sdeint_diag`/`sdeint_general` | `kernels::sde` (Aevia-side wrapper) |
| `fft` | `kernels::fft` (rustfft) |
| `eig`/`svd`/`qr`/`lu` | `kernels`/`faer` |
| `interp1`/`spline`, `minimize`, special functions | `kernels::*` |

Numeric kernels consume `f64`/`ndarray` directly and bypass the term graph; the `Float` tensor path prefers them.

### 10.3.4 Linear algebra

- `Float`/`Complex` matrices use rssn's numeric kernels (faer); small exact/symbolic `Real` matrices use `rules::linalg`; a large matrix that cannot be numericised returns `Err` or stays symbolic.
- The dimensionless restriction of `det` is in the Aevia layer (rssn's `det` can compute dimensioned matrices, but Aevia does not expose that overload); the dimension `e/d` of `linsolve(A,b)` is checked statically by Aevia, and rssn's answer dimension is used for validation.

### 10.3.5 Unknown and new operators

Any operator not in the generated façade goes through the dynamic call:

```aevia
let r = Rssn.dynamic("foo", [to_value(x), to_value(y)])
match r with
| Ok(v) -> ...
| Err(e) -> ...
```

Arguments and results are `Value`s; rssn checks the shape of the arguments against the signature, and the full type and dimension check stays on the Aevia side. After a new operator is registered, regenerating the `Rssn` module gives typed declarations; plugin and user operators need no compiler change; a failure returns `OpError`, which Aevia turns into `E1003`/`E1001`.

### 10.3.6 Units

**Aevia's term graph carries no units.** Unit literals are folded to coherent SI numbers at compile time; `Real`/`Float` values carry an optional display unit on the host side. rssn sees dimensionless numbers and symbolic expressions (`3 m + x` is `3 + x` in the term graph), and dimensions are guaranteed by static types.

The reason: rssn's `quantity` has no numeric eval, and `dimension_from_text` parses `L` as litre and `T` as tesla; putting units into the static term graph would break both the numeric path and the dimension path (more trade-offs in §12.2). Units appear at only two boundaries:

1. **Display / typesetting**: `show`/`to_latex` splice the number and the display unit on the host side; when rssn typesetting is needed, wrap with `quantity(v, "<coherent SI unit>")`, for output only.
2. **Export / FFI**: when writing CSV/NPY/JSON, unit metadata is attached on the host side.

A `Point` is converted to a `Float` in K before entering rssn; the run-time dimension of `Dyn<ρ>` is `rssn::dim::DimTerm`. `Units.*` is generated from `manifest.units` and no second table is kept; the start-up unit-table consistency check (§5.10) serves only display and export.

### 10.3.7 Simulation

```rust
let json = rssn::sim::scenario::run("ising", params)?;
```

`simulate(name, json)` calls it directly; the only source of scenario names is the kernel's `SCENARIOS`, and a host keeps no second list. The typed wrappers `Physics.*` (§8.17) are Aevia source modules that call `simulate` internally and convert between records and JSON; the `Rssn_Sim_<Name>` modules are generated on import from rssn's scenario catalogue (`rssn::sim::schema`, §8.17). `ising` returns a flat `Vec<i8>`, which the wrapper reshapes row-major by `width`/`height` into `Tensor<2, Int>`; `sim` errors are strings, which the wrapper maps to the `SimError` enum by prefix.

### 10.3.8 Domain modules: `Rssn_<Domain>`

The flat `Rssn.<op>` and the per-rule-set `Rssn_<rule_set>` follow the **kernel's** organisation and cannot say "`invmod` takes a modulus and returns a residue". Domain modules are an Aevia-side grouping, generated from the **manifest plus a small declarative overlay** (`crates/aevia-rssn/domains.overlay`):

- **Membership.** `domain`/`family` assign operators to 29 domains (`NumberTheory`, `Combinatorics`, `Group`, `FiniteField`, `Graph`, `Coding`, `Crypto`, `Topology`, `Numeric`, …) by `rule_set:glob`, first match wins. Every rule set MUST be claimed by some family; when the kernel adds a rule set or operator that the overlay does not place, the coverage check fails.
- **Three exposure tiers plus a witnessed tier.** *typed*: `sig` gives the Aevia type (`Result<T, OpError>`) and the wrapper is derived from the signature; *witnessed*: see below; *dynamic*: an automatically generated `Result<Value, OpError>` wrapper, so a new operator is callable with zero edits; *excluded*: `exclude`, with a mandatory reason. In a `typed`-mode domain an operator with no `sig`, `dynamic` or `exclude` is a coverage error; a `sig` whose parameter count disagrees with the manifest arity, or that names an operator that no longer exists, fails likewise.
- **The witnessed tier (automatic typing).** At the end of each engine run, the request (`factorint(360)`) and the answer (`list(list(2, 3), ...)`) sit in one equivalence class, which is itself a type judgement; the kernel records these judgements as "sort facts", clusters them by argument kind and then **replays** each witness call, producing `signatures.witnessed.json` (rssn `docs/KERNEL.md` §3a). Each overload in the manifest carries `overload_sources`. For a witnessed overload that has no `sig` but has a usable Aevia type, the domain module generates a wrapper of the same shape as the typed tier (`fn divisors(n: Int): Result<Tensor<1, Int>, OpError>`). Type mapping: `Int` → `Int`; rationals, symbolic expressions and symbols → `Real`; floats → `Float`; `Bool` → `Bool`; rectangular lists → `Tensor<rank, element>`; tuples, opaque objects and ragged lists stay in the dynamic tier. When an operator has several witnessed overloads, the one with the most evidence takes the operator name and the others get `_int`/`_real`/`_float` suffixes. A witnessed signature is **evidence, not a universal proof**: the parameter side may be narrower than what the operator actually accepts (`Rssn.dynamic` is always available), and the result is validated by `from_value<T>`, so going outside the observed range gives `Err`, never a wrongly typed value. `dynamic op | reason` can force an operator to stay in the dynamic tier.
- **Conversions are generic**: arguments `to_value(x)`, results `from_value<T>(v)`; rich types declare a pair of conversion functions with `codec Type to_fn from_fn` (one pair per type): `Perm` (0-based, ±1 at the boundary), `Graph`/`Group` (nominal types wrapping an opaque `Value` handle), `PermGroup`.
- **Error channel.** A kernel failure is `Err(OpError)`; a graph-backend operator whose argument leaves its domain returns an unreduced call term (`invmod(6, 15)`), which `from_value` reports as a `TypeMismatch` carrying the text of that term.
- **Diagnostic channel: why it did not reduce.** When a numeric operator rejects its arguments (shape mismatch, axis out of range, singular system, incompatible Neumann data, …) the kernel leaves only the unreduced call term. `Session::explain(term)` (`rssn::kernel::{Diag, DiagKind}`) replays the operator's evaluator on the calling thread and returns `Diag { op, arg, kind, message }`. When the result is exactly the operator's own unreduced call and `kind` is not `Symbolic`, the bridge (`Kernel::call_with_witnesses`, `call_key`, `call_key_with_witnesses`) returns `KernelError::from_diag`, with the stable code `rssn.diag.<kind>` (`arity`, `shape`, `domain`, `singular`, `incompatible`, `not_converged`, `unsupported`, `symbolic`, `unspecified`). `Rssn.dynamic` writes it in user form — function name, parameter name, position — inside `Err`: ``Err(rssn.diag.domain: tensordot_2_2_1: argument `axes_a` (#3): an axis is past the rank of the first tensor (rank 2): [2])``. Parameter names come from the overlay's `sig`; witnessed/dynamic wrappers give only a position (`argument #3`). `OpError` stays an opaque type: `"{e}"` gives `code: message`, and no constructor or `Rssn.*` member was added. At a **throwing** site (a typed call `Rssn.<op>(..)`) the same finding is `E0807` with the stable code in a note. Only operators of `rules::numeric::TABLES` record reasons.
- **Dimension evidence.** `sig` also declares "this operator is a pure function of dimensionless arguments"; rules the kernel declares `Dimensionless`, and `List/NonMetric/Compare/Angle` rules when the arguments are known to be dimensionless, give bare-number results `Scalar(1)` evidence; other rules (`Same`, `Sum`, `Custom`, …) are not trusted. A `list(...)` term result is read back as a tensor when trusted (equal-length rows merge into a higher-rank tensor; unequal ones stay a tensor of tensors).
- **Generated at run time.** A module is rendered from the live manifest when `import Rssn_NumberTheory` is resolved (`codegen::namespace_source`), and no generated source is committed, so it cannot drift from the linked kernel. Only the overlay and the coverage report `tools/rssn-api-audit/RSSN-COVERAGE.md` are committed; `cargo test -p aevia-rssn --test domains` regenerates and compares them (`AEVIA_BLESS=1` rewrites).

**`Rssn_Numeric`.** rssn's `numeric` rule set (robust statistics, windows and spectra, wavelets, Krylov/sparse/low-rank/tensor decompositions, dimension-independent finite-difference solvers, complex special functions, expression operators) all have a `sig`, and none stays in the dynamic tier; user-facing interface and examples are in §8.19.1. The rules:

- Results are floating point, so signatures use `Float`/`Tensor<n, Float>`, **not** `Real` (`from_value<Real>` accepts only exact `Int`/`Rat`); integer-valued results (ranks, permutation indices) are likewise returned as `Float`.
- Method, wavelet and statistic parameters named by symbols have type `Real`; every call of `symbol("db2")` makes a new symbol, so bind each name once with `let` and reuse it.
- **One kernel operator may have several `sig`s, each with its own Aevia name** (the overlay writes `sig heat_fd as heat_fd_2d(..)`; a duplicate name is an error): since a signature has a single literal rank, dimension-independent operators are named by dimension count (`_2d`/`_3d`) and rank-polymorphic tensor operators by rank (`tensor_norm_<r>`, `tensordot_<ra>_<rb>_<k>`, etc., §8.19.1).
- **Decoding mixed lists.** The result of a tensor decomposition is a list of parts of different types. The bridge keeps it as a one-dimensional value with per-element dimension evidence, and `from_value<Tensor<1, Value>>` is the generic "list accessor": it hands out the parts along the first axis one by one as `Value` handles, and each is then decoded with `from_value<T>`. The decoders are Aevia source in the `Numeric` prelude (`hosvd3_from_value`, …), declared as decode-only types with `codec Hosvd3 - hosvd3_from_value` (`-` means no encoder; using such a type as a parameter is a coverage error).

Examples: `tests/domain_number_theory.aev`, `domain_combinatorics.aev`, `domain_finite_fields.aev`, `domain_numeric.aev`.

**Language gap (not implemented)**: `let x: Zmod<17> = 5; x * x ^ -1` needs three language changes, all in `aevia-types`/`aevia-core`: (1) variant types accepting `Nat` parameters (currently `E0317`); (2) `Num`/`Fractional` classes with `add/sub/mul/neg/from_int/inv` methods, and binary operators dispatching to instances for user nominal types; (3) integer literals promoted through `from_int` by the expected type. Until then the modulus of `Zmod` is carried at run time and operations are written as functions (`zmod_mul(x, inv)`).

## 10.4 The dimension bridge

The only source of truth for dimensions and units is `rssn::dim` (`DimTerm`/`DimArena`/`UnitRegistry`). Aevia's `Dim` is its type-level mirror; `DimValue` at run time is a `DimTerm`, not a string.

### 10.4.1 Static to run time

**The static path passes no dimensions to rssn**: the type checker has already guaranteed dimensional correctness, symbols in the term graph are dimensionless, and `DimensionMode` is `Off` (or `Advisory`, for diagnostics only). Dimensions are handed over in only three situations:

- the run-time check of `Dyn<ρ>`: the host keeps a `DimTerm`, not a string;
- FFI/export: prefer `Config::dimension_term(symbol, DimExpr)` (no text parsing; unknown base names are interned). The plain-text entry `Config::dimension(symbol, text)` MUST be given **full names** (`Length`, `Mass`, `Time`, `Current`, `Temperature`, `Amount`, `Luminosity`) and **never single letters** (`L` is litre, `T` is tesla, `N` is newton, `J` is joule);
- the user explicitly requests rssn's dimension diagnostics: `Config::dimensions(Advisory)`.

A custom base dimension is declared with `Config::base_dimension(name)`; `prepare` calls `define_base_dimension` on the **cloned** graph. `Graph::clone` shares an `Arc<RwLock<DimArena>>` and a write detaches when `strong_count > 1` (copy on write), so registration during a request does not leak into the template or concurrent requests, and a request that defines no base dimension pays no copy cost. Never modify the template graph of a `Session` directly.

### 10.4.2 Run time to static

The dimension of a numeric answer is taken from `Answer.dimension`; do **not** re-infer it from `Answer.term`. `Answer.dimension` is an `Option`: it is `None` under `DimensionMode::Off`, in which case the static dimension is used; for a run-time dimension the numeric path MUST use `Advisory` or `Strict`. On the static path the answer's dimension is already known and `Answer.dimension` serves only an optional consistency check; a disagreement is the internal error `E0809`.

### 10.4.3 Dimension modes

| Situation | Mode |
|---|---|
| Ordinary code that passed static checking | `Off` (term graph has no dimensions) |
| `Dyn`, FFI, terms built at run time | `Strict`, with full names |
| User explicitly requests diagnostics | `Advisory` |

Aevia does not expose `bypass` to ordinary users; `Dyn<ρ>` is the explicit escape.

## 10.5 Threads and lifetimes

### 10.5.1 `Session` is not `Sync`

`Session` is internally a `RefCell`, and `Term<'s>` borrows the `Session`. The bridge keeps the kernel session **thread-local** (`kernel()` in `aevia-rssn`):

```rust
thread_local! {
    static KERNEL: RefCell<Option<Rc<Kernel>>> = const { RefCell::new(None) };
}

pub fn kernel() -> Rc<Kernel>;   // this thread's session, created on first use
```

- One session per thread; the worker threads of `par_map` each hold an evaluator replica and a session (§9.7.2).
- Symbolic values are not shared across threads: the kernel's `TermRef` carries a `SessionId` and a handle from another session is rejected (`OpError::ForeignTerm`); values that cannot cross threads fall back to sequential evaluation. The bridge does not use `Session::fork`/`merge` (primitives the kernel provides).
- `compute_all` runs in parallel within one session, but **the bridge does not use it**: batched requests are issued one after another; when parallelism is needed, assign sessions per thread on the host side.

### 10.5.2 Lifetime of terms

A `Term<'s>` is borrowed only during a `compute` call and converted to a `u32` id on return; `Session::term_by_id(id)` retrieves it, and ids are stable within a session; the answer term of `compute` has already been imported back into the session. The term graph only grows; the REPL's `:reset` rebuilds the session.

## 10.6 Performance strategy

### 10.6.1 Do not call `compute` for every operator

`compute` clones the template graph and runs rewriting; for an O(1) construction like `x + 1` it costs orders of magnitude more. The rules:

- Arithmetic and elementary functions: build terms with `Session::call_value` (local rewriting only), not `compute`.
- `simplify`/`eval`/`diff`/`integrate`/`solve`: `compute`.
- Printing does not simplify by default; the user asks with an explicit `simplify` or `:latex`.
- Comparison and `if`: a `Prop` is not evaluated; use `decide` or `assuming` when a decision is needed.

### 10.6.2 Caching

- `Config` is cached per rule set / assumptions / mode; terms are cached by structural hash; compiled functions by term hash + input names; the unit table and dimension aliases are compile-time constants. rssn also has a request-level answer cache and an optional sub-term lemma cache (appendix C, Revision 12).
### 10.6.3 Batching

- Do not use `compute_all` for element-wise symbolic tensor operations (one request per element is costly): build terms with `map` and `simplify` once at the end.
- Numeric tensors use the `F64` path and never enter the term graph.

### 10.6.4 JIT

`Term::compile` compiles a closed `Float` expression into a function; Aevia's hot functions take this path. `TieredBackend` interprets first and compiles in the background; if the JIT fails it falls back to the interpreter with no change of semantics. The default build has `features.jit = false`, and `@jit` MUST read `Features`, otherwise it reports `E1001("core.jit")`.

### 10.6.5 Devices: data-parallel execution

rssn has two independent execution axes:

| Axis | rssn abstraction | The question it answers |
|---|---|---|
| How one term becomes a function | `Backend` / `Compiled` | evaluate at one point |
| Where many rows run | `Device` / `Buffer` / `KernelIr` | evaluate on 10⁶ rows; integrate 10⁴ initial values |

The JIT (§9.6) belongs to the first axis and is **not** the GPU path; `@jit(target = Gpu(n))` selects a device on the second axis.

**Capability discovery.** Build level: manifest `features.devices` (`cpu`, `cpu-parallel`, plus `gpu-wgpu` when that feature is enabled) is a compile-time fact and may enter `manifest_hash`; `core.gpu` holds when `features.devices` contains any `gpu-*`. Run level: `rssn::backend::device::registry().list()` returns `DeviceInfo` (`id`, `kind`, `dtypes`, `ops`, `parallelism`, `unified_memory`, `deterministic`); having no adapter is not an error, just no GPU device. The Aevia-side wrapper is `aevia_rssn::device` (`available()`, `has_gpu()`, `resolve(Selector)`).

**`Device` to rssn device.**

| Aevia | rssn id | Note |
|---|---|---|
| `Cpu` | `cpu` | single-threaded; reference semantics |
| `Cpu` (hint `cpu_parallel`, `Selector::CpuParallel`) | `cpu-parallel` | rayon; **bit-identical** to `cpu` |
| `Gpu(n)` | the `n`-th `DeviceKind::Gpu` (`gpu:wgpu:n`) | if absent, `E1001("core.gpu")` with the number of registered GPUs in the message |
| `Distributed(c)` | none | `E1001("core.distributed")`; outside this design |

**Transfers are explicit.** `to_device(t, Gpu(n))` is one `upload` (an `F64` tensor is converted to `f32` on an `f32`-only device, which changes values); `device_of` reads the tensor's device tag; bringing a result back to the host needs a `download`, and there are no implicit transfers. `on(device, thunk)` is scoped execution: batch primitives inside `thunk` use that device and buffers are released when the scope ends.

**Precision contract.** The semantics of kernel IR is `f64`. `cpu` and `cpu-parallel` provide `F64` bit-identical to the interpreter (including NaN, `select`, and out-of-range `gather` reading NaN), extending §9.7.2 ("the thread count never changes any result") to devices. `gpu:*` (WGSL) provides `F32` and a **double-single `F64`**: WGSL has no `f64`, so the wgpu device represents `Dtype::F64` as a pair of `f32` values `(hi, lo)`, with about 48 bits of precision (on an RTX 3060 / Vulkan the relative error is `4e-15` for `+ - * /`, `1e-14` for `sqrt`, `7e-14` for `exp`; the device tests require `5e-12`). Limits: the exponent range is still that of `f32` (keep data within `1e-30 .. 1e30`), `sin`/`cos`/`tan` return NaN for `|x| > 1e6`, and `F32` and `F64` buffers cannot be mixed in one launch. So it is **not** IEEE double precision and is not bit-identical to the CPU. Aevia never silently turns a `Float` result into `f32`; code needing bit-exact results chooses `Cpu`/`CpuParallel`.

**Launch fusion.** rssn's `device::plan` fuses a chain of element-wise kernels into one launch (intermediate columns become registers and never reach device memory); fusion changes where values are kept, not how they are computed, so on deterministic devices it is bit-identical to the unfused plan. This is an explicit rssn-side API and is not yet exposed in Aevia.

**Functions that can be lowered to a device** follow the JIT gate of §9.6.1: no captures, `Float` scalar arithmetic, elementary functions, comparisons and `if` (both sides pure, evaluated eagerly and then `select`ed). Terms containing host calls (the `EvalFn` of gamma/erf/Bessel, C externs) cannot be lowered to a device, and `Device.map` falls back to the interpreter row by row, with values unchanged (on a CPU device).

**Batch primitives (design).**

```
Device.available : () -> IO (List DeviceInfo)       # registry snapshot
Device.map       : (Float -> Float, Tensor<[n],Float>) -> Tensor<[n],Float>
                                                     # on Cpu/CpuParallel; pure, deterministic
Device.map_on    : (Device, Float -> Float, Tensor<[n],Float>) -> IO (Tensor<[n],Float>)
Device.reduce    : (Reduce, Tensor<[n],Float>) -> Float   # Sum | Prod | Min | Max
Device.ensemble  : (OdeSystem, Tensor<[m,k],Float>, Float, Float, Int) -> Tensor<[m,k],Float>
                                                     # one program, k initial values; fixed-step RK4
```

Only the pure, bit-deterministic versions are promised on CPU devices; `map_on(Gpu(n), ..)` is `IO` because allocation and transfer are effects, and the result is subject to the device's precision.

**Status.** Implemented: the whole rssn device layer (registry, `Buffer`, `KernelIr`, CPU reference device, WGSL emitter, optional `gpu-wgpu`, `ensemble_rk4`, and `device::array` reductions / matmul / CSR product / stencil, etc.); the manifest `features.devices`; `aevia_rssn::device` and `jit::map_floats` in `aevia-core` (apply a pure `Float` function in batch on a CPU device). **Not implemented**: `Device.map`/`map_on`/`reduce`/`ensemble`/`available` as language primitives (entries are needed in the type table and the built-in table, and `Device.aev` regenerated); `to_device`/`on` on `Gpu(n)` still report `E1001("core.gpu")` (tensor values do not yet carry device buffer handles).

## 10.7 Error mapping

| rssn error | Aevia diagnostic |
|---|---|
| `ComputeError::Dimension(e)` | `E0401` or `E0412` (`Dyn`) |
| `ComputeError::NotNumeric(s)` | `E0803` (free symbol) or `E0806` (domain) |
| `ComputeError::Inconsistent(s)` | `E0809` (internal error) |
| `ComputeError::Rule(e)`, `UnknownOperator(s)` | `E0899` (internal error) |
| Budget exhausted | `E0808` |
| `BackendError` | `E0807` or `E0899` |

`Answer.diagnostics` become Aevia warnings; under `Advisory` mode they do not fail.

## 10.8 Known gaps and risks

**The numbers below are measurements; the live manifest (`aevia rssn`) is the only source of truth.** On `rssn 3.0.0-rc.1` (`api_version = 2`): 1620 operators (99 declared + 103 overlay + 1418 derived, none dynamic-only), 82 units, 30 constants (18 exact), `manifest_hash = d12fc2b7…`. rssn has implemented the Manifest, dynamic `Value`/`call_value`/`call_key`, overload resolution, 100% signature coverage, the physical-constant table, the cdylib plugin ABI, `rssn::dim`, custom base dimensions, `Config::dimension_term`, `fork/merge`, structured results and JIT lowering. Only the integration gaps that **still exist** are listed.

1. **`Session` is single-threaded.** Crossing threads needs `fork/merge`, not a shared session; the bridge currently shares no symbolic values across threads (§10.5.1).
2. **`compute` still clones the graph.** `OpTable` is an `Arc` (clones share it) and `DimArena` is copy-on-write, leaving the shallow-copy cost of the node table; `Session::prepare` is not public.
3. **The JIT is off by default** (see §10.6.4).
4. **The "not reported" semantics of `Solutions`.** An empty `multiplicities` means not reported and MUST NOT be filled with 1; the kernel fills it when it can prove it (a univariate rational polynomial whose roots are all exact numbers, via exact square-free decomposition); `conditions` holds only conditions declarable by construction; `complete` is distinguished by whether the result is a `list(...)`, and `Solutions::new` defaults to `false`. Aevia MUST NOT declare exhaustiveness unconditionally when constructing one.
5. **Constants are values, not units.** `manifest.constants` is not in the `UnitRegistry` and cannot take part in `quantity` parsing; `e` (elementary charge) has the same name as `Math.e` and MUST be qualified when both modules are `open`.
6. **100% typed, but only `Declared` + `Overlay` are exact.** `Derived` distinguishes only numeric `(Real,…)` from symbolic `(Term,…)` and not `Tensor`/`Quantity`/`Int`, so `Derived` MUST be marked "conservatively inferred".
7. **`assuming` is mostly unary predicates**; general equations and inequalities have no language entry point (§10.2.2).
8. **Symbolic exponents: the bridge round-trips, the language does not support them.** `DimExpr::to_term` interns `Length^n` as an atom and `try_from_term` is its exact inverse; Aevia's static type positions still accept only rational exponents (§4.12.3). Do not read "the backend can represent it" as "the language should support it".
9. **Simulation parameters are still JSON**; typed wrappers come from the `Physics.*` source modules or the generated `Rssn_Sim_<Name>` (§8.17).
10. **Budgets and interruption.** rssn's budget lives inside `compute`; Aevia's outer timeout/cancellation is cooperative and cannot kill.
11. **Version drift.** Aevia pins `api_version` + `manifest_hash` and runs conformance. The version-2 manifest adds Rust source-path metadata, and its binary layout is incompatible with version 1 with no version negotiation before decoding, so a host MUST know the producer version in advance. The manifest may still change during the rc cycle, which is why numbers in this document are measurements and not constants.
12. **`manifest_hash` is a bincode BLAKE3 and can be taken two ways.** `hash_hex()` (64 hex characters, which hosts SHOULD pin) and `hash_u64()` (the first 8 bytes read little-endian, for ABI/display only) are not interchangeable; JSON is for humans, and hashing or comparing JSON text is not allowed.
13. **Whether a `Value` can enter the term graph follows `manifest.value_kinds`** (10 of 17 variants can, 7 cannot, §13.3.1); sparse operations MUST go through `eval_dyn` or Aevia's local layout kernels. A generator MUST NOT hard-code this table.
14. **The callback and plugin bridges.** `core.callback` calls a checked source closure through a same-thread scoped handle, and the handle expires when the outer call returns (§8.19); `core.plugin.source` traces `@op` functions into kernel definitions and `@rule` into rewrites, registered as in-process plugins (§13.5.2).
15. **Units belong to a session.** A unit registered by `Session::register_unit` appears only in that session's manifest; the process-level `KernelManifest::build` gives a different unit table and hash, and Aevia MUST use `Session::manifest()`.
16. **SDE.** The manifest exposes only the noise primitives and the scheme metadata `sde_scheme_info` (strong order only; the weak order is the host-side `sde_order`, §8.9.1); the solve loop needs a `&Fn + Sync` host closure that the manifest's `Value` ABI cannot carry, so it stays on the host side (`aevia-std::sde`), and `sdeint_*` are not `Rssn` operators.

### 10.8.1 Mitigations

- **Conformance tests**: for every standard operator, compare Aevia's static dimension with rssn's `Answer.dimension`; any disagreement fails.
- **Unit-table validation**: at start-up compare base, derived and affine units one by one, against `manifest.units` only.
- **Fallback paths**: a JIT failure falls back to the interpreter; a failed symbolic solve returns `Err` and never panics.
- **Budget defaults**: 2 s / 1e6 rewrites; adjustable by the user.
- **Version pinning**: pin `api_version` + `hash_hex()`; run the full conformance suite on upgrade; operator and unit counts in documents never enter cache keys.
- **Bridge isolation**: every rssn call is concentrated in `aevia-rssn`, and upper layers do not depend on `rssn` types directly.

## 10.9 An end-to-end call chain

```aevia
fn fall_time(h: Float[L]): Float[T] =
    sqrt(2 * h / g)
```

1. Type checking: `h : Float[L]`, `g : Float[L/T^2]`, `2*h/g : Float[T^2]`, `sqrt : Float[T]`.
2. Calling `fall_time(20.0 m)`: `20.0 m` is folded at compile time to the `Float` value 20.0 with display unit `m`.
3. The body: `2 * h`, `/ g` and `sqrt` use plain f64 arithmetic on `Float` and never enter the term graph; if the argument were a symbolic `Real`, they would be `mul`/`pow`/`sqrt` terms built by `Session::call_value` (local rewriting only), still without `compute`.
4. `println` calls `show` and prints in the display unit `s`: `2.0196199771025523 s`.
5. `approx(x)` on a symbolic or exact value runs `compute` with a `Numeric` target; rssn returns a numeric ball and Aevia takes its midpoint.

This chain shows the core performance strategy: **types and dimensions at compile time, arithmetic at term construction, only heavy solving enters the e-graph, and numeric hot spots go to f64/JIT**.

The next chapter shows how these rules combine in complete programs.


<a id="ch-11-examples"></a>

# 11 — Complete examples

These programs are not fragments: each is a complete module that can go into a `.aev` file and be run with `aevia run` or checked with `aevia check` (the `spec_programs` test of `aevia-std` checks every one). After each example we explain the language features it uses and how types and dimensions are derived. Outputs are the actual results of `aevia run`.

## 11.1 Free fall: units, dimensions and `Float`

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

Output:

```
fall time from 20.0 m = 2.0196199771025523 s
```

Derivation:

- `9.80665` is a decimal literal of type `Float[1]`; multiplied by `m/s^2` it gives `Float[L/T^2]`.
- `20.0 m` is `Float[L]`.
- `2 * h`: `2 : Int` is promoted to `Float`, giving `Float[L]`.
- `/ g`: `L / (L/T^2) = T^2`.
- `sqrt`: the exponent is halved, `T^2 → T`.
- The result unifies with `Float[T]`.

`println` uses `show`, which prints in display units: the display unit of `h` is `m`; the result of `fall_time` has no explicit display unit, so `s` is chosen when printing.

## 11.2 Projectile motion: records, ODEs and structured state

```aevia
module Projectile

open Units.SI
import Math { sqrt, sin, cos }
import LinearAlgebra { linspace }
import Ode { odeint }

type State = {
    x:  Float[L],
    y:  Float[L],
    vx: Float[L/T],
    vy: Float[L/T],
}

type Rate = {
    x:  Float[L/T],
    y:  Float[L/T],
    vx: Float[L/T^2],
    vy: Float[L/T^2],
}

let g = 9.80665 m/s^2

fn rhs(_t: Float[T], s: State): Rate =
    let speed = sqrt(s.vx^2 + s.vy^2)
    let k = 0.02 / m
    {
        x  = s.vx,
        y  = s.vy,
        vx = -k * speed * s.vx,
        vy = -g - k * speed * s.vy,
    }

fn trajectory(v0: Float[L/T], angle: Float[1]): Tensor<1, State> =
    let s0 = {
        x  = 0.0 m,
        y  = 0.0 m,
        vx = v0 * cos(angle),
        vy = v0 * sin(angle),
    }
    let ts = linspace(0.0 s, 3.0 s, 1000)
    odeint(rhs, s0, ts)

fn main(): IO Unit = do
    let path = trajectory(50.0 m/s, 45 deg)
    let last = path[-1]
    println("range = {to(m, last.x)}, height = {to(m, last.y)}")
```

Derivation:

- `k = 0.02 / m`: `Float[1] / Unit[L] = Float[1/L]`.
- `k * speed * s.vx`: `1/L * L/T * L/T = L/T^2`, matching `Rate.vx`.
- `-g - k*...`: both terms are `Float[L/T^2]`.
- Each field of `State` is `Float[d]` and the corresponding field of `Rate` is `Float[d/T]`; the compiler computes `RateOf<State> = Rate` and checks "divide by T" field by field.
- `path[-1]` is a `State`; `last.x` is `Float[L]`.

The type of `odeint`:

```
odeint : ((Float[T], s) -> RateOf<s>) -> s -> Tensor<1, Float[T]> -> Tensor<1, s>
```

Here `s = State` and `RateOf<State> = Rate`. If `rhs` returned `State` (forgetting to divide by T), the type checker would report an error at the `RateOf` constraint instead of letting the numerical integration silently compute the wrong thing.

## 11.3 Symbolic calculus: exact values and `Result`

```aevia
module Symbolic

import Math { exp }
import Calculus { diff, defint, series }
import Solve { solve }

fn main(): IO Unit = do
    let x = symbol("x")

    let df = diff(fn t => t^3 - 3*t + 1)
    println("f'(x) = {df(x)}")
    println("f'(2) = {df(2)}")          # exactly 9

    let g = fn t => exp(t)
    println("g'(x) = {diff(g)(x)}")

    let roots = solve(fn t => t^2 - 5*t + 6)
    match roots with
    | Ok(s) -> println("roots = {s}")
    | Err(e) -> println("solve failed: {e}")

    let area = defint(fn t => t^2, 0, 1)
    match area with
    | Ok(a) -> println("∫₀¹ t² dt = {a}")
    | Err(e) -> println("integral failed: {e}")

    let s = series(fn t => exp(t), 0, 5)
    println("exp(t) ≈ {s}")
```

Output:

```
f'(x) = 3*x^2 - 3
f'(2) = 9
g'(x) = exp(x)
roots = { variable = Some("x"), roots = [2, 3], multiplicities = [1, 1], conditions = [], complete = true }
∫₀¹ t² dt = 1/3
exp(t) ≈ 1 + x + 1/2*x^2 + 1/6*x^3 + 1/24*x^4 + 1/120*x^5
```

Points to note:

- `diff` takes a function, not an expression. It creates a symbol of the same dimension, applies the function, differentiates, and substitutes back, so `diff(g)` works for any `g`.
- `df(2)` is exactly `9`, of type `Real[1]`; for a float, write `approx(df(2))`.
- `solve` and `defint` return `Result` because there may be no solution or no antiderivative; `diff` is total and leaves the expression unsimplified on failure. The result of `solve` is a `Solutions` record (§8.6): roots, multiplicities, conditions and `complete`.
- `series` returns a polynomial truncated at the requested order (the variable name of the expansion is chosen by the solver); its dimension is still `Real[1]`.

## 11.4 Ohm's law: linear algebra with dimensions

```aevia
module Circuits

open Units.SI
import LinearAlgebra { linsolve, det }

fn main(): IO Unit = do
    let r = [[1.0, 2.0], [3.0, 4.0]] * ohm
    let i = [0.1, 0.2] * A

    let v = r @ i
    println("V = {v}")                  # [0.5 V, 1.1 V]

    let p = v * i
    println("P = {sum(p)}")             # 0.27 W

    match linsolve(r, v) with
    | Ok(i2) -> println("I = {i2}")     # back to [0.1 A, 0.2 A] (floating-point rounding)
    | Err(e) -> println("solve failed: {e}")

    # det accepts only dimensionless matrices:
    # let d = det(r)                    # E0410
    let d = det(r / ohm)
    println("det(R/Ω) = {d}")
```

Derivation:

- `[[...]]` is a `Tensor<2, Float>`; multiplied by `ohm : Unit[Resistance]` it gives `Tensor<2, Float[Resistance]>`.
- `r @ i`: the element dimension is `Resistance * Current = Voltage`; the result is `Tensor<1, Float[Voltage]>`.
- `v * i`: `Voltage * Current = Power`; `sum` keeps the dimension.
- `linsolve(r, v)`: `Voltage / Resistance = Current`.
- `det(r)` is rejected because the dimension of `det` is `d^n` with `n` a run-time order; `r / ohm` is a dimensionless matrix, for which `det` is legal.

Matrix elements carry units and matrix multiplication yields a voltage automatically; there is no need to tag each element with a label at run time.

## 11.5 Temperature: affine points and differences

```aevia
module Thermo

open Units.SI

fn main(): IO Unit = do
    let t1 = 25.0 °C                 # Point[Θ]
    let t2 = point(300.0, K)         # Point[Θ]
    let dt = t2 - t1                 # Float[Θ]
    let t3 = t1 + 10.0 K             # Point[Θ]

    println("T1 = {t1}")
    println("T2 = {to(°C, t2)}")
    println("T3 = {t3}")
    println("ΔT = {dt}")

    # the next two lines are type errors:
    # let bad1 = t1 + t2             # E0405
    # let bad2 = 2.0 * t1            # E0405
```

Output:

```
T1 = 25.0 °C
T2 = 26.85 °C
T3 = 35.0 °C
ΔT = 1.8500000000000227 K
```

Points to note:

- `25.0 °C` is an affine unit literal of type `Point[Θ]`, with value 298.15 K.
- `300.0 K` is a linear unit literal of type `Float[Θ]`: a temperature difference, not an absolute temperature. Use `point(300.0, K)` for an absolute temperature.
- `Point - Point` gives a difference; `Point + difference` gives a point; `Point + Point` and `k * Point` are errors.
- `to(°C, T2)` changes only the display unit; the value of `T2` is still in K.

## 11.6 Data analysis: `Table` and dynamic columns

```aevia
module Analysis

open Units.SI
import Data.Csv { read_csv, column, Table }
import Stats { mean, std }
import Plot { line, combine, plot, label }

fn main(): IO Unit = do
    r <- read_csv<{ time: Float[T], velocity: Float[L/T] }>("measurements.csv")
    match r with
    | Err(e) -> println("cannot read: {e}")
    | Ok(tbl) ->
        match (column(tbl, "time"), column(tbl, "velocity")) with
        | (Ok(t), Ok(v)) -> do
            println("mean speed = {mean(v)}")
            println("std  speed = {std(v)}")
            let spec = label(line(t, v), "v(t)")
            plot(combine([spec]))
        | _ -> println("column type mismatch")
```

Points to note:

- `read_csv<ρ>` is `IO (Result<Table<ρ>, DataError>)`; `ρ` is the row type, and header units are used to infer field types. `<-` binds the result and `match` handles the error.
- `column(t, "time")` is checked at compile time against the field types of `Table<ρ>` and validated defensively at run time; CSV headers may carry units (`time [s]`), without which columns are dimensionless, and an explicit `ρ` can override this.
- `line` accepts dimensioned `x`/`y` tensors and generates axis labels automatically (`t / s`, `v / (m/s)`); to override, use `PlotOpts.xunit`/`yunit`.
- `combine([spec])` builds a `PlotSpec`; `plot` writes HTML/SVG.

## 11.7 Simulation: typed wrappers

```aevia
module IsingDemo

import Physics.Ising

fn main(): IO Unit = do
    let params = {
        width = 32,
        height = 32,
        temperature = 1.8,
        mc_steps = 200,
        seed = Some(42),
    }
    match Physics.Ising.run(params) with
    | Ok(r) -> println("magnetization = {r.magnetization}")
    | Err(e) -> println("simulation failed: {e}")
```

`Physics.Ising.run` is `Params -> Result<IsingResult, SimError>`; the fields of `Params` are typed (`temperature : Float[1]`, `seed : Option<Int>`). Underneath it calls rssn's `sim::scenario::run("ising", json)`; the wrapper handles JSON encoding and decoding and dimension checking.

## 11.8 Dimension polymorphism: standardisation and linear fitting

```aevia
module Regression

import Stats { mean, std }

fn standardize<d>(xs: Tensor<1, Real[d]>): Tensor<1, Real[1]> =
    let m = mean(xs)
    let s = std(xs)
    (xs - m) / s

fn covariance<d, e>(xs: Tensor<1, Real[d]>, ys: Tensor<1, Real[e]>): Real[d*e] =
    mean((xs - mean(xs)) * (ys - mean(ys)))

fn linear_fit<d, e>(xs: Tensor<1, Real[d]>, ys: Tensor<1, Real[e]>): { slope: Real[e/d], intercept: Real[e] } =
    let mx = mean(xs)
    let my = mean(ys)
    let slope = sum((xs - mx) * (ys - my)) / sum((xs - mx)^2)
    { slope = slope, intercept = my - slope * mx }
```

Derivation:

- `standardize`: `(xs - m)` is `Real[d]`, and `/ s` gives `Real[d]/Real[d] = 1`.
- `covariance`: the element-wise product has dimension `d*e`, which `mean` preserves.
- `linear_fit`: the dimension of `slope` is `(d*e)/(d^2) = e/d`; `slope * mx` is `e`; `my - ...` is `e`. The dimensions of the returned record's fields are correct.

These functions work for any dimension; there is no monomorphisation at run time and dimensions are erased after type checking. The module is a library (it has no `main`) and is checked with `aevia check`.

## 11.9 Error examples: how the compiler rejects

```aevia
let n: Int = 3
let bad1 = 3 m + 2 s
let bad2 = sin(3 m)
let bad3 = (3 m)^n              # n : Int
let bad4 = 25.0 °C * 2.0
let bad5 = 1 / (0 m)
```

Diagnostics (excerpt):

```
error[E0401]: dimension mismatch
  --> bad.aev:3:12
   |
3 | let bad1 = 3 m + 2 s
   |            ^^^^^^^^^ expected T, found L
   |            ^^^ this is L
   |                  ^^^ this is T
   |
   = difference: L/T

error[E0403]: `sin` requires a dimensionless argument, found L
  --> bad.aev:4:16
   |
4 | let bad2 = sin(3 m)
   |                ^^^
   |
help: divide by a unit first, for example `x / 1 m`

error[E0404]: a dimensioned value cannot be raised to a runtime exponent
  --> bad.aev:5:13
   |
5 | let bad3 = (3 m)^n
   |             ^^^^^^
   |
help: use a literal exponent, or `pow_dyn` for a checked dynamic power

error[E0405]: affine points cannot be multiplied
  --> bad.aev:6:12
   |
6 | let bad4 = 25.0 °C * 2.0
   |            ^^^^^^^^^^^^^
```

`E0401`–`E0405` are compile-time errors (`aevia check` reports only these four); the `E0801` (division by zero) of `bad5` is a run-time error that the compiler MAY report early during constant folding. The aim of an error message is to **point at the dimension difference and, where possible, give an actionable fix**, not merely print type names.

## 11.10 A "full-feature" program

```aevia
module Full

open Units.SI
import Math { exp }
import LinearAlgebra { linsolve, linspace }
import Calculus { diff }
import Plot { line, combine, plot, label }

fn kinetic(m: Float[M], v: Float[L/T]): Float[Energy] =
    m * v^2 / 2

fn main(): IO Unit = do
    # 1. units and dimensions
    let m = 2.0 kg
    let v = 3.0 m/s
    println("E = {kinetic(m, v)}")

    # 2. symbolic derivation
    let sym = symbol("x")
    println("f' = {diff(fn t => exp(-t^2))(sym)}")

    # 3. linear algebra
    let a = [[2.0, 1.0], [1.0, 3.0]]
    let b = [1.0, 2.0]
    match linsolve(a, b) with
    | Ok(sol) -> println("x = {sol}")
    | Err(e) -> println("solve failed: {e}")

    # 4. data and plotting
    let ts = linspace(0.0 s, 1.0 s, 100)
    let vs = map(fn t => (3.0 m/s) * exp(-(t / s) / 0.5), ts)
    let spec = label(line(ts, vs), "v(t)")
    plot(combine([spec]))
```

This program strings together units, symbols, linear algebra, tensors and plotting: **one language covers the whole workflow from formula to chart**, instead of carrying strings between several libraries.

The next chapter explains the reasons for these design choices, the alternatives, and the known limits.


<a id="ch-12-rationale"></a>

# 12 — Design trade-offs, limits and roadmap

This chapter explains why Aevia looks the way it does, how it differs from existing approaches, what is deliberately left out, and how far the implementation has come. It is informative.

## 12.1 Why dimensions are types

Most errors in scientific computing are not "the algorithm is wrong" but "the unit or dimension is wrong": `kW` taken for `W` (off by 1000), `°C` taken for `K` (off by 273.15), `Hz` taken for `rad/s` (off by `2π`), a mass flow taken for a volume flow. Run-time unit libraries (Pint, Unitful) catch these in tests, but the error may only surface in production. A type system catches them at compile time and can also **guide inference**: the dimension of `mean(xs)` is that of `xs`, `var(xs)` is its square, and `diff(f)` is `f` divided by the independent variable. A library function cannot express these rules; they must be in the types.

The price is a more complex type system. Aevia keeps it in check by:

- limiting dimension algebra to a free abelian group with rational exponents, which is uniformly decidable;
- not supporting static symbolic exponents, which would be undecidable;
- using the explicit escape `Dyn` when something cannot be expressed statically, instead of inventing unsound types;
- keeping units out of types (§12.2), avoiding a combinatorial explosion of type-level conversions.

## 12.2 Why units are not in types

If `m` and `ft` were different types, function signatures would have to quantify over units, type equality would carry units, and conversion would happen at the type level: type errors would read "metres vs feet" rather than "length vs time"; changes to the unit table would break type compatibility; generic functions would have to handle unboundedly many unit combinations.

Aevia's choice: **a type records only the dimension, and a unit is a compile-time scale**. `3 km` and `3000 m` have the same type and the same value; use `to` when display is needed. This gives up the type-level distinction between different units of one dimension, in exchange for a simple, composable type system; in practice, errors are almost always dimension errors, not "metres vs feet".

## 12.3 Why pure functions

Pure functions can be safely parallelised, cached, differentiated, substituted and simplified; symbolic computation requires expressions to be replaceable, and mutable state breaks substitution; dimension rules are simpler too (no "when was this variable changed"). IO is confined to `IO`, so library functions never read files or print behind your back. The costs: numeric algorithms cannot use in-place loops and use tail recursion with an accumulator, with performance coming from tail-call elimination and copy-on-write; some algorithms (in-place LU, in-place FFT) are optimised at run time when the buffer is uniquely held, while the semantics stay pure; users learn `fold`/`map`/recursion instead of `for`. `|>`, `map`, `fold`, pattern matching and tail calls let pure code read like a data flow.

## 12.4 Why strict evaluation

Performance is predictable, with no thunk build-up or space leaks; errors surface where they happen; it matches rssn's strict term construction. Symbolic computation does not need laziness: a `symbol` is a value, an expression is a term, and simplification is requested explicitly with `eval`; laziness would make "when does rewriting run" unpredictable.

## 12.5 Why `Real` and `Float` are separate

- `Real`: exact rationals or symbolic expressions, for derivation, simplification and exact results.
- `Float`: IEEE double, for numeric computation, big data and the JIT.
- A decimal literal defaults to `Float` (someone who writes `0.1` usually wants a float; someone who writes `1/10` wants exactness); in mixed arithmetic `Real` absorbs `Float` (exactness first), and hot paths use `Float` explicitly.

A single numeric type would be bad either way: with only `Float`, `solve(x^2 - 2)` yields 1.414…, losing exactness, and symbolic derivation would need another type system; with only `Real`, numeric loops are slow, BLAS and the JIT cannot be used. Exact versus approximate is the dividing line of scientific computing and deserves to be explicit.

## 12.6 Why HM and not dependent types

HM plus dimension algebra is enough for the vast majority of scientific formulas; inference is the main source of usability (users write no signatures); dependent types (shape-indexed, static exponents) would complicate the types of `reshape`, CSV, slicing and broadcasting and make error messages harder to read; for what cannot be expressed statically, `Dyn` is more practical than a dependent type that cannot be written. Aevia is not a proof assistant: most programs need no annotations, and annotations, when given, are checked.

## 12.7 Comparison with existing approaches

| Approach | Dimensions | Units | Symbolic | Pure | Inference | Assessment |
|---|---|---|---|---|---|---|
| F# units of measure | types | types | no | no | yes | pioneer of typed dimensions; units in types, generics are complex |
| Julia Unitful | run time | run time | partial | no | no | good ecosystem; errors at run time |
| Python Pint/Astropy | run time | run time | no | no | no | easy to use; weak performance and typing |
| Frink | run time | run time | no | no | no | a unit calculator, not a general language |
| Mathematica | run time | run time | yes | partial | no | strong symbolics; dimensions are not types |
| Haskell units | types | types | no | yes | yes | expressive; steep learning curve |
| Squants | types | types | no | yes | yes | a Scala library; units in types |
| **Aevia** | **types** | **compile-time scale** | **yes** | **yes** | **yes** | dimension types + symbolic/numeric phases + pure functions |

What sets Aevia apart: dimensions are types and units are not; symbolic and numeric work live in one language, and `diff`/`solve` take functions rather than strings; pure functions with `IO` isolation make symbolic substitution safe; `Dyn` handles dimensions that cannot be expressed statically; the backend reuses rssn's e-graph, numeric kernels, JIT and simulation.

## 12.8 Design boundaries (not version limits)

These are **deliberate design boundaries**, not "later" to-dos. Each says why it was drawn there and how it behaves when the backend lacks support (a capability diagnostic, never removal of a language feature).

| # | Boundary | Effect and workaround |
|---|---|---|
| 12.8.1 | Shape refinement covers only linear `Nat` (literals, `+`, `*`, linear equations), with no general recursion or proof | Complex shape proofs need an explicit `refine_shape`/`reshape`, or a fall back to run-time checks with `Tensor<n,τ>` |
| 12.8.2 | Static symbolic exponents are unsupported: `x^n` (`n : Int`, `x` dimensioned), dimensioned `det`/`product` | A real boundary of dimension algebra; use `Dyn`, or divide by a unit, compute dimensionlessly and restore the dimension |
| 12.8.3 | `Point[d]` is represented as `Float` + `UnitId`; symbolic absolute temperatures do not enter `Point` | Symbolic thermodynamics cannot do absolute-temperature algebra directly; use differences plus a reference point, or `Dyn` |
| 12.8.4 | Uncertainty is not a core type: `Measurement<d>` does independent error propagation only, correlated errors use `Covariance<d,e>` | Uncertainty in experimental data must be managed explicitly; it is a library type |
| 12.8.5 | No user-defined effects or macros: `IO` is the only effect; the extension points are rssn operators, plugins, `Rssn` dynamic calls and the registry | Use `@op`, plugins and `Rssn.dynamic` for DSLs and code generation |
| 12.8.6 | `Table<ρ>` is a static schema; dynamic CSV returns `Table<Dynamic>` and `to_table<ρ>` validates at run time | Give an explicit `ρ` when reading, or use `Table<Dynamic>` plus validation |
| 12.8.7 | Symbolic values are bound to an rssn session and cannot cross threads: the `par_map` workers each hold a replica, and values that cannot cross threads fall back to sequential evaluation (§9.7.2) | Keep the symbolic phase serial and the numeric phase parallel |
| 12.8.8 | Unit-name ambiguity: `ms` may be milliseconds or metre-seconds; the longest match wins | Write an explicit `*` for rare combinations; the formatter hints |
| 12.8.9 | `Real` absorbs `Float`: `Real + Float` becomes `Real` and the `Float` is converted exactly to a rational | Mixing symbolic and float leaves the fast path and produces large numerators and denominators; keep hot paths `Float` throughout; the compiler hints `W0302` |
| 12.8.10 | Devices/GPU/distributed are run-time targets: `Device` is a run-time value, not a tensor type parameter | The type system does not distinguish CPU and GPU tensors, and device errors surface at run time or at the capability check; use `on(device, ...)` and `@jit(target=...)`, avoiding a type combinatorial explosion |
| 12.8.11 | No formal semantics or proofs: the specification is prose plus rules, with no Coq/Lean formalisation | Rely on conformance tests, property tests and cross-checks against rssn |
| 12.8.12 | `RateOf<s>` covers only numeric state (recursively over scalars, records, tuples, tensors) | For states with `Int`/`Bool`/`String`/function fields, put the constant parameters in the `rhs` closure |

### 12.8.13 Dispatch and extension boundary

Aevia has neither multiple dispatch nor per-type or per-dimension monomorphisation. The former is a semantic decision, the latter an engineering one.

- **No run-time multimethods.** Method calls are resolved at check time: a concrete head type records an exact instance key (class + canonical head); only a quantified head type may choose, by a run-time witness, among candidate instances **proved pairwise disjoint**, and instances that no witness can distinguish are rejected at check time (§3.8.4, §9.2.3). "Which instance" therefore remains a static question; the run time only executes the selection the checker has proved, with no backtracking and no guessing by value.
- **No monomorphisation.** Monomorphisation duplicates generic code per type, which inflates compile time and code size, breaks separate compilation, and conflicts directly with type erasure (§9.2.3, §4.4.4) and dimension polymorphism — `mean<L>` and `mean<T>` MUST be the same code; nor is there a correctness gain, since type errors are already excluded at check time. Per-operation, tag-guarded **representation specialisation** keeps the generic path, duplicates no function and changes no semantics; it enters the performance plan only after a measured baseline (§9.13).
- **The extension points are explicit**: ADTs, record rows, modules/imports, `derive` (a restricted set of structures, §3.8.5), `@op`/`@rule`, plugins and the registry, and the `Dyn` escape. Module interfaces export types, constructors, classes, instances and alias definitions (§7.1.3/§7.3). There are no implicit global instances, no default methods in classes, and no macros, quotation or splicing.

The cost: users cannot add type-class instances to existing types (the orphan rule); "parameterised dimension families" such as distributions and units are expressed with nominal types plus built-in overload tables (§8.7.1) rather than multi-parameter classes.

## 12.9 Closed design decisions

These are final decisions; implementation may proceed in stages, but the language design no longer forks.

| # | Question | Decision |
|---|---|---|
| 1 | Extend `Point` to tensors? | Yes; `Tensor<shape, Point[d]>` with element-wise point arithmetic |
| 2 | Make `Angle` a base dimension? | No; `rad` is dimensionless and `deg` is a scale |
| 3 | Shape-indexed `Tensor`? | Yes, as a refinement: `Tensor<[3,4],τ>`; linear `Nat` arithmetic |
| 4 | Should `det` return `Dyn` by default? | No; dimensionless `det` + `det_unit` |
| 5 | Mixing `Real`/`Float`? | `Real` absorbs `Float`; hot paths use explicit `Float` |
| 6 | `Dyn` errors? | arithmetic `panic` + `try_*`; `check` returns `Result` |
| 7 | Nested `RateOf`? | scalars, records, tuples, tensors, defined recursively |
| 8 | Static `Table` schema? | Yes, row-polymorphic `Table<ρ>`; `Table<Dynamic>` kept |
| 9 | `@` or `*` for matrix product? | keep `@`; `*` is element-wise |
| 10 | `%` for modulo? | No; `mod`/`rem` |
| 11 | Is `let` recursive by default? | No; `fn` is recursive, `let rec` is explicit |
| 12 | Top-level IO? | No; `main`/REPL |
| 13 | Central registry? | Yes; content-addressed + signatures + capabilities, protocol in §13.7 |
| 14 | Export user dimension aliases? | Yes; exported by default, `private` hides |
| 15 | Type-level guarantee of unit conversion? | No; units are scales, dimensions are types |
| 16 | GPU/distributed? | Devices are run-time values: `Device`/`on`/`@jit(target)` |
| 17 | Sparse? | `Sparse<F,shape,τ>` as a core type, with a `Layout` class |
| 18 | User effects/macros? | No |
| 19 | Uncertainty? | `Measurement`/`Covariance` standard-library types |
| 20 | Operator integration? | Reflection first: Kernel Manifest + dynamic `Value` + generated façade; no hand-written bindings |

## 12.10 Implementation progress

Implementation proceeds in dependency order and is not a language version; every stage keeps the manifest and dynamic-call contract of chapter 13. History by revision is in appendix C.

| Stage | Contents | Status |
|---|---|---|
| M0 | lexing, layout, parsing; name resolution; type and dimension algebra; shape refinement; diagnostics and formatter | implemented; all nine complete programs of chapter 11 check |
| M1 | Core IR, interpreter, closures/records/variants/pattern matching; Kernel Manifest and `Rssn` generation; the rssn bridge; units and `Point` | implemented |
| M2 | tensor storage/broadcasting/slicing; linear-algebra kernels; `Sparse` | implemented (`Bcsr`, Krylov solvers, slicing) |
| M3 | `diff`/`antiderivative`/`defint`/`limit`/`series`/`solve`/`sum`/`product`; budgets and error mapping | implemented |
| M4 | modules/packages/lock files; registry protocol and client; REPL/CLI/formatter; LSP | implemented; gaps in appendix C, Revision 15 |
| M5 | CSV/NPY/JSON; `Table<ρ>`; Plot; `Device`; `simulate` and `Physics.*` | implemented (Plot has only the HTML/SVG renderer; no GPU backend is wired in) |
| M6 | JIT (`jit` feature); plugin operators; benchmarks | partial: the JIT is off by default; see §9.6 |

**Acceptance criteria for a first stable release**: every example of chapter 11 checks and runs; every dimension error is a compile-time error (except at `Dyn` and data boundaries); operators in the manifest need no hand-written bindings; conformance tests against rssn pass; `rssn::dim`/`UnitRegistry` round-trip consistently; plugins and the registry work; shape refinement, `Point` tensors, `Table<ρ>`, `Sparse` and `Device` each have an end-to-end example; at least one real ODE/PDE/statistics project runs end to end.

## 12.11 Success criteria

1. A physics, engineering or data-science user can write, in a day, a program with units, symbolic derivation and numeric solving;
2. dimension errors are caught at compile time, and the messages guide the fix;
3. numeric hot spots perform on the same order as Python/NumPy;
4. symbolic computation covers common calculus, equations and linear algebra;
5. the code is readable, like mathematics rather than like configuration.

The next chapter covers extensibility and the rssn integration; after it come the complete grammar, the diagnostics catalogue and the revision log.


<a id="ch-13-extensibility"></a>

# 13 — Extensibility and the rssn integration

This chapter is part of the specification and **takes precedence over** any "unsupported for now / to be evaluated later" wording elsewhere. Aevia has no `v1`/`v2` layering: language capabilities are designed once and in full, and whatever the backend has not yet implemented is reported as a **capability diagnostic** (`E1001`) rather than by removing a language feature.

The detailed backend contract is in rssn's `docs/KERNEL.md` (Kernel Manifest, dynamic-call ABI, plugins), `docs/DIMENSIONS.md` and `docs/CALLBACKS.md`; this chapter defines the normative contract on the Aevia side.

---

## 13.1 Three axioms

1. **A single operator registry.** rssn's `OpTable` is the only source of truth. Aevia keeps no list of "which operators are supported"; it reads the **Kernel Manifest** and generates the `Rssn` module. A new operator registered in rssn is visible at Aevia's next build.
2. **A single dimension/unit layer.** `rssn::dim`'s `DimTerm`/`DimArena`/`UnitRegistry` is the canonical layer shared by run time and compile time; Aevia's `Dim` is its type-level mirror, and `Units.*` is generated from `manifest.units` with no second unit table.
3. **Dynamic calls are a first-class capability.** An operator without a type signature can still be called through `Rssn.dynamic(name, args)`; the typed façade is an increment to the static experience, not a precondition for capability. rssn validates on the dynamic path at the **signature level** (`check_args`) and does no dimension or type inference; the full check belongs to Aevia's static checker or `Rssn.typed`.

Together these answer two things: **thousands of operators cannot be covered by hand-written bindings** (they drift, miss things and rot), and **new capabilities cannot wait for "the next version"** (the language, the manifest and the plugin ABI are fixed now, and the backend fills in capabilities step by step).

---

## 13.2 The Kernel Manifest and the generated `Rssn` module

### 13.2.1 The manifest

At start-up rssn provides a deterministic `KernelManifest`:

```
KernelManifest
  rssn_version : String
  api_version  : Int
  manifest_hash: Bytes32
  features     : Features
  rule_sets    : [RuleSetInfo]
  ops          : [OpInfo]
  units        : [UnitValue]
  base_dimensions : [BaseDimInfo]
  plugins      : [PluginInfo]
  constants    : [ConstantValue]
  value_kinds  : [ValueKindInfo]
```

- `OpInfo`: `keys: [OpKey]` (one per overload; `keys[0]` is the primary overload), `name`, `aliases`, `category`, `arity`, `overloads`, `signature_source`, `flags`, `binder`, `cost`, `eval`, `higher_order`, `purity`, `determinism`, `thread_safety`, `stability`, `since`, `doc`, `examples`, `dim_rule`, `backend_support`, `dynamic_reason`, `rule_set`, `jit`. The full schema is in §3.1 of the documents at the root of the rssn repository.
- `manifest_hash` is the BLAKE3 of deterministic bincode; `to_json()` is for humans only.
- `units: [UnitValue]` carries `name`/`display`/`dimension`/`scale`/`offset`/`affine`/`aliases`/`prefixable`/`category`/`since`; generating `Units.*` needs to read only this. `KernelManifest::build_with_units(graph, sets, plugins, &registry)` reads units from the caller's registry and `build(...)` from the process-level table; `Session::manifest()` uses the former, so a unit registered by `Session::register_unit` enters **that** session's manifest only.
- `value_kinds: [ValueKindInfo]` describes whether each `Value` variant can enter the term graph (`in_term_graph`) and the alternative when it cannot (§13.3.1).
- `constants: [ConstantValue]` is the table of physical constants, with fields `name`/`symbol`/`value: ScaleValue`/`dimension: DimExpr`/`unit`/`exact`/`uncertainty`/`source`; `Units.Constants` is generated from it (§13.6.5).
- `features.plugin_kinds` says which plugin kinds the build can load (default `["in-process"]`; `plugin-cdylib` adds `"cdylib"`); `features.sde` says that the SDE noise primitives and the scheme-metadata operator can be called from the manifest (host capability name `core.sde`). `Features` is part of the manifest: adding a field or operator changes `manifest_hash`.

### 13.2.2 Generation rules

At build time:

1. Read the manifest and check `api_version` (an incompatible one is `E1004`).
2. For each `OpInfo`: `signature_source = Declared | Overlay | Derived` → generate a typed `Rssn.<name>` declaration (**one per overload**); `None` → `Rssn.dynamic`; an alias → `Rssn.<alias>`, or `Rssn.<rule_set>.<name>` on a clash.
3. A typed declaration remembers the corresponding `OpKey` from `OpInfo.keys` and calls pin that overload, so static selection and run-time execution cannot diverge. The public form is `Rssn.call_key(key, …)`; the generator actually emits the equivalent `Rssn.invoke(name, signature_hash, args)` (the same pinning semantics with a different error channel: a generated `fn` MUST return its declared type, so a kernel failure raises instead of returning `Err`).
4. Generate `Rssn.Version`, `Rssn.ManifestHash`, `Rssn.Capabilities`, `Rssn.ValueKinds`, `Rssn.Constants` and `Rssn.Units`.
5. Generate LSP completions, documentation and signature stubs, recording the source of each declaration.

**Coverage is a measurement, not a constant.** On `rssn 3.0.0-rc.1` (`aevia rssn` gives the live figures) all 1620 operators have signatures (99 declared + 103 overlay + 1418 derived). The precision tiers the generator MUST distinguish:

| Source | Precision | What is generated |
|---|---|---|
| `Declared` | exact | typed |
| `Overlay` | exact | typed |
| `Derived` | shape-level | typed, but only `Real`/`Bool`/`Term` |
| `None` | none (leaf / non-callable) | `dynamic` |

`Derived` is "inferred from registration data", not name guessing:

| Registration data | Inferred signature |
|---|---|
| has `eval` | `(Real, …) -> Real` |
| has `eval` + `PREDICATE` | `(Real, …) -> Bool` |
| not a leaf | additionally `(Term, …) -> Term` |
| `Binder` at position k | the k-th parameter is a `Symbol` |
| `Arity::Variadic` | the trailing parameter is variadic |
| `OpFlags::LEAF` | no signature (`DynamicReason::OpaquePayload`) |

So `sin` has two overloads, `(Real) -> Real` and `(Term) -> Term`; `det` is a `HEAVY` request without `eval`, so without an overlay it has only `(Term) -> Term` and the static type of `det(matrix)` would be `Dyn<term>`; the overlay supplies `(Tensor<[n,n],Real>) -> Real`. Hence "100% typed" is not "100% exact": `Derived` does not distinguish `Tensor`/`Quantity`/`Int`, and IDEs mark it "conservatively inferred".

Overlays are also the carrier of **dimension type parameters**: `sde_wiener`/`sde_levy_areas` use a dimension parameter such as `T` to write `sqrt(h)` (`RootOf{degree:2,index:3}`) and `Tensor<1, Real[T]>` into the signature, which derivation cannot infer. The spelling of the generated source is exact (§10.0): the `Rssn.sde_wiener` produced by `aevia rssn --emit` is `(Int, Int, Int, Real[T]) -> Tensor<2, Real[T^(1/2)]>`. `Dyn<term>` remains only for types that genuinely cannot be spelled (§10.0).

**Typing a literal pinned key.** The generator emits only `Rssn.invoke(op, key, …)` with literal `op` and `key`; the checker validates these two strings against the pinned overload in the live manifest and gives that overload's declared result type (`Rssn.det` → `Real`), with fresh dimension variables at each call. A **non-literal, unknown or ambiguous key** stays a dynamic `Value`, with the diagnostic produced at run time: `E1005` (unknown or mismatched `signature_hash`), `E1002` (ambiguity), `E1003` (a dynamic argument or non-function value). The checker never re-parses or executes a key.

The SDE solve loop is not in the manifest (host callbacks, kernel CO-03); the host-side `sdeint_*` do not go through the `Rssn` operator channel, and `sde_scheme_info` is a manifest-only metadata query (strong order only; the weak order is the host-side `sde_order`, §8.9.1).

**Repository rule.** The Aevia repository MUST NOT contain per-operator hand-written binding files, and keeps no copy of `signatures.overlay.json` (it is an rssn build input, already merged into `OpInfo.overloads`). Only the generators are allowed (`aevia rssn --emit` in `aevia-rssn`; the generated views of the standard library are written by `aevia-stdlib-gen`). CI checks: generator output equals the committed generated files, and a change of the manifest snapshot MUST be updated explicitly.

### 13.2.3 Namespaces

- Ordinary operators: `Rssn.<name>` (`Declared|Overlay|Derived` are all typed; only `None` and plugin operators use `Rssn.dynamic`).
- Rule-set qualification: `Rssn.<rule_set>.<name>` is only a namespace; the run time still goes by name/arity. rssn's `call_value` looks up only by name/alias and does **not** support `rule_set/name`; to pin an overload use `Rssn.call_key(key, args)` with a `key` from `Rssn.overloads(name)` or `OpInfo.keys`.
- Plugins: `Rssn.<plugin>.<op>`.
- Domain modules: `import Rssn_NumberTheory` (`Rssn_<Domain>`, CamelCase) gives wrappers grouped by mathematical domain with types and a `Result` channel (§10.3.8); `Rssn_<rule_set>` remains grouped by kernel and stays available.

On a name clash, a short name keeps only the **most stable** one (`Stable` first, then earlier `since`); the others must be qualified. The generator records the short-name map in the manifest.

---

## 13.3 Dynamic values and dynamic calls

### 13.3.1 `Value`

```aevia
type Value =
    | VInt(Int)
    | VReal(Real[1])
    | VFloat(Float[1])
    | VComplex(Complex[1])
    | VBool(Bool)
    | VString(String)
    | VSymbol(String)                 # a free symbol, not a String
    | VUnit(Unit[1])
    | VQuantity(Quantity)
    | VTensor(TensorData)             # shape + row-major Values
    | VSparse(SparseData)
    | VRecord(Record)
    | VVariant(Variant)
    | VTerm(TermRef)                  # owned handle
    | VFunction(Callback)
    | VDyn(Dyn<Real>)
    | VOpaque(Opaque)
```

`Value` is the type of the **dynamic boundary**, not an implicit subtype in the core language; it appears only in `Rssn.dynamic`, plugins, FFI and the reflection API.

**Term-graph admissibility** comes from `manifest.value_kinds`, and Aevia keeps no second copy. Currently (17 variants):

- `in_term_graph = true` (10): `VInt`/`VReal`/`VFloat`/`VBool`/`VString`/`VSymbol`/`VQuantity`/`VTensor`/`VTerm`/`VDyn`;
- `in_term_graph = false` (7): `VComplex` (lowered to a `Float` only when the imaginary part is 0, otherwise `Unsupported("complex literals")`), `VUnit` (a unit is a definition, not a number; cross the boundary with `quantity(value, unit)`; only declared unit-parameter positions take a `Value::Unit` directly), `VSparse`/`VRecord`/`VVariant`/`VFunction`/`VOpaque`.

A variant with `false` can only be fed to operators that have `eval_dyn` or serve as a return value; a generator MUST NOT assume every `Value` can take part in rewriting.

### 13.3.2 Calls

```aevia
Rssn.call    : (String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.dynamic : (String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.typed   : (String, Tensor<1, Value>, TypeRep) -> Result<Value, OpError>
Rssn.overloads       : String -> Tensor<1, String>
Rssn.resolve_overload: (String, Tensor<1, Value>) -> Result<String, OpError>
Rssn.call_key        : (String, String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.Constants       : Tensor<1, String>     # constant symbols; values are in the generated Constants module
Rssn.Units           : Tensor<1, String>     # unit names; values are in the generated Units module
```

- `Rssn.call` is an alias of `Rssn.dynamic` and looks up only by short name/alias; an ambiguous short name is `E1002`.
- `Rssn.call_key` "runs without resolving": it takes a name + `signature_hash` (32 hex characters), which together are the writable form of an `OpKey`; all zeros means the operator has no declared overload. A key that does not match the current manifest is `E1005` (regenerate the module); an argument that departs from the pinned signature on the kernel side is `UnknownOp`, which becomes an error value at the Aevia boundary.
- `Rssn.resolve_overload` exposes the run-time selection for caching and diagnostics: on success it returns `"rule_set.name@signature_hash"`; both a tie and "no signature accepts" are `OpError::Ambiguous`. `Rssn.overloads` returns the rendered signature of every overload.
- Arguments are injected with `to_value(x)` and results converted back to a static type with `from_value(v)`. Operators without signatures, plugin operators and experimental entries use `dynamic`; the standard set currently has no `None`, so on it `dynamic` is only the plugin channel.
- `type_rep<T>()` keeps the concrete type resolved by the checker; it is an opaque value that an ordinary record cannot forge. After the dynamic call, `Rssn.typed` checks representation, shape and dimensions against that witness; a mismatch or missing evidence returns `Err(OpError)`. A successful result is still a `Value`, validated and converted through `from_value<T>`; a generic not made concrete by a run-time type dictionary cannot produce a `TypeRep`.
- Higher-order operators use `Rssn.call_ho(name, args, fns)`; `to_callback` builds an opaque callback from a checked source closure, with the limits of §8.19. A callback runs synchronously on the original thread and expires when the outer call ends; calls across threads and re-entry in the same scope are rejected, and callback errors come back through `OpError`; an empty list is just a dynamic call.
- `Rssn.register`/`Rssn.register_plugin` accept a host `Plugin`; source plugins are published by `@op`/`@rule` and `load_plugin` (§13.5.2). `core.plugin.source` is enabled in builds with in-process plugins; `core.callback` means the restricted callback bridge is available.

### 13.3.3 Errors

```aevia
type OpError =
    | UnknownOp(String)
    | Arity(Int, Int)
    | Type(TypeRep, TypeRep, Int)
    | Ambiguous(String, Int)         # name, number of ties
    | Dimension(DimError)
    | NotNumeric(String)
    | NotReduced(String)
    | Domain(String)
    | BudgetExceeded
    | Unsupported(String, String)   # backend, feature
    | Plugin(String)
    | ForeignTerm(String, String)   # expected session, got session
    | Codec(String)
    | Host(String)
    | Internal(String)
```

`E1003` is "no matching operator/overload" and carries the list of candidate signatures; `E1001` is "backend capability missing" and carries the capability name and the minimum rssn version.

---

## 13.4 Overloads, signatures and resolution

### 13.4.1 Overloads

rssn allows several signatures under one name, decided in `kernel::overload`:

```aevia
type Match = | No | Generic | Shaped | Widened | Exact   # ordered
matches_type : (TypeExpr, TypeExpr) -> Match
matches      : (TypeExpr, Value) -> Match
score        : (Signature, Tensor<1, Value>) -> Option<Nat>
score_types  : (Signature, Tensor<1, TypeExpr>) -> Option<Nat>
best         : (Tensor<1, Signature>, Tensor<1, Value>) -> Best
best_types   : (Tensor<1, Signature>, Tensor<1, TypeExpr>) -> Best
type Best    = | None | Unique(Int) | Ambiguous(Tensor<1, Int>)
```

- Each argument is classified separately, and the score of the whole call is that of the **weakest** argument.
- `Int -> Real` and `Int|Real|Float -> Complex` are `Widened` and the reverse does not hold; the same constructor with element/dimension not pinned by the signature is `Shaped`; type variables and `Dyn` are `Generic`.
- A tie at the top inside `best` gives `Ambiguous`; it never silently takes the first.
- `score_types`/`best_types` share the same score fold as `score`/`best`: Aevia resolves once at compile time and gets the same number the kernel would get at run time.

In the standard set each name has one registration, but **each registration has 1–5 overloads** (an operator with `eval` has both a numeric and a symbolic one, and an overlay adds exact signatures). Two things must be kept apart:

- **Registration selection** (`resolve_op`) returns directly at `fitting.len() == 1` and does no scoring on the standard set; a cross-registration tie can come only from plugins.
- **Overload selection** (`resolve_overload` → `overload::best`) really runs over the signatures within a registration: `sin(0.5)` selects `(Real) -> Real` and `sin(x)` (`x` a `Symbol`) selects `(Term) -> Term`. But when the argument itself is a `Term`/`Dyn`, the numeric overload also passes as `Generic`, and the two tie → `Ambiguous`; so `resolve_overload` suits only arguments of concrete type, and for symbolic expressions use a key from `OpInfo.keys`.

`call_value` selects a registration by name/arity and then calls `check_args`: when no overload accepts the arguments it returns `OpError::Type`. But the `Dyn<term>` of the symbolic overload is `Generic` for any value, so derived operators nearly always pass — the check really blocks only when all signatures are concrete (`eq`/`lit`/`pow`); it does not choose between numeric and symbolic (the operator's `eval` decides).

Aevia's own static resolution rule (isomorphic to rssn's lattice, for Aevia's own overloads and plugins):

1. filter by arity; 2. classify each argument and take the weakest; 3. a unique highest score is selected; 4. a tie at the top is `E1002`; 5. no match → try `Dynamic`; still failing → `E1003`.

Three run-time entry points:

- `Session::call_key(&OpKey, …)` **does not re-resolve**: it finds the registration by `rule_set` + `signature_hash`, pins the overload, and requires the arguments to be accepted by the pinned signature (`overload::score(pinned, args).is_some()`), otherwise `UnknownOp`.
- `Session::call_value(name, …)` finds the registration by name/arity; with several candidates it picks the most specific with `overload::score` and reports a tie as `OpError::Ambiguous`; arguments accepted by no overload are `OpError::Type`.
- `Kernel::call(&OpKey, …)` forwards to `call_key` and therefore also pins the overload; passing through `dyn Kernel` loses nothing.

`Session::resolve_overload(name, args)` is for "resolve first, then cache": a `Unique` `best` returns a key with `signature_hash` that can be handed straight to `call_key`; an operator with no signature at all returns a zero-hash key (which `call_key` accepts); a tie or no accepting signature is `Err(OpError::Ambiguous)` and **never** yields an unexecutable zero-hash key. So the return value can always be handed to `call_key` safely. `OpKey.signature_hash` is the first 16 bytes of `Signature::hash()` (the BLAKE3 of deterministic bincode); a forged, stale or argument-mismatched key returns `UnknownOp`.

### 13.4.2 Signature coverage

Coverage is 100% (`aevia rssn` reports `0 dynamic-only`). The sources, in decreasing priority:

1. `Declared`: `OpDescriptor::new(...).signature(...)`, written by the operator itself;
2. `Overlay`: a manual correction in `signatures.overlay.json` (merged into the manifest on the rssn side; Aevia keeps no copy);
3. `Derived`: inferred by `kernel::infer::derive` from registration data;
4. `None`: a leaf or non-callable form, with `dynamic_reason` stating why (the standard set has none).

Rules: `Derived` reads only `eval`/`PREDICATE`/`Binder`/`Variadic`/`LEAF` and never the operator name, so it is conservative but reliable; when precision is insufficient write an overlay rather than change the inferrer (overlays belong only on operators whose signature must be exact: tensor, symbolic and unit parameters); CI's `signature-audit` reports coverage and source distribution, and a high share of `Derived` does not block the build but blocks marking those operators "exact"; plugins declare signatures with `Registry::register_op`, and `Registry` rejects a registration with neither signature nor `dynamic_reason`. This lets "complete capability" and "type precision" advance separately, without any operator silently forgotten.

---

## 13.5 Plugins and user extension

### 13.5.1 Plugins

A plugin is not new syntax but an **ordinary Aevia package + a manifest marker + attributes**:

```toml
# aevia.toml
[plugin]
kind = "aevia"          # in-process; corresponds to rssn's PluginKind::InProcess
abi = 1
```

| `[plugin] kind` | rssn | Loaded by |
|---|---|---|
| `"aevia"` / `"in-process"` | `PluginKind::InProcess` | `Session::register(Box<dyn Plugin>)` |
| `"cdylib"` | `PluginKind::Cdylib` | `Session::load_plugin(path)` (needs `plugin-cdylib`) |

The C ABI of `cdylib` is a **byte buffer**: the entry symbol is `rssn_plugin_v1`, and arguments and results are bincode-encoded `Vec<Value>`/`Result<Value, String>`, not a `#[repr(C)]` mirror of `Value`; `cdylib::load` checks `PLUGIN_ABI` and `RssnPluginDesc`, and `Session::load_plugin` = `load` + `register`. The module is controlled by the `plugin-cdylib` feature (off by default, included only in `full`) because loading a dynamic library runs arbitrary code inside the host process with no sandbox. Aevia MUST check `manifest.features.plugin_kinds` before loading: without `"cdylib"` it reports `E1001("core.plugin.cdylib")` instead of letting the loader throw a low-level error.

```aevia
module MyDomain

@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))

@rule("softplus(x) => ln(1 + exp(x))")
let softplus_rule = ()

dimension MyDim
unit myunit : Length = 0.5 m
```

`Rssn.register` (alias `register_plugin`) wraps `Session::register(Box<dyn Plugin>)`: `Rssn.register(plugin)  # -> Result<PluginId, PluginError>`. A plugin can register operators + signatures + eval callbacks, rewrite rules, dimension rules, units and base dimensions, and (optionally) a JIT lowering.

### 13.5.2 Aevia functions as rssn operators

`@op` publishes an Aevia function as an rssn operator and `@rule` publishes a rewrite rule:

```aevia
@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))

@rule("mydomain/sinh-exp")
fn sinh_exp(x: Real[1]) = (sinh(x), (exp(x) - exp(-x)) / 2)
```

**Decision: tracing, not callbacks.** The kernel's extension language is its own term language: an operator is a definition `name(a, b) := body` and a rule is a rewrite `name: lhs => rhs`. An Aevia function becomes one by **tracing**: call the function with fresh symbols carrying the checked dimension of each parameter, and the term it builds is the definition body. This needs no second compiler and inherits everything the kernel can do — once `mydomain/softplus` is a definition, the kernel can call it, differentiate it, simplify and integrate through it, and evaluate it numerically, which an opaque host callback cannot do (a callback also cannot cross threads, because the `Rc` interpreter is not `Send`).

- The price is checked: a function whose result depends on control flow over its arguments (`if x > 0`, a `match` on a value) or that performs IO cannot be traced into a term, and registration reports `E1006` with the reason.
- A rule function returns a pair `(lhs, rhs)` and its parameters are the rule's pattern variables; `lhs` MUST be an operator application (it is locally simplified while tracing, so `x * 0` is not a legal left-hand side).
- Publication happens after the module's function bindings and before the evaluation of top-level `let`s, so top-level bindings can already call it through `Rssn.dynamic("mydomain/softplus", …)`. Each loaded module registers an in-process plugin `aevia/<module>#n` belonging to the current thread's kernel session.
- **Source operators are first-class operators.** Publication order: first trace each `@op` into a kernel definition and register the checked parameter/result dimensions as a **dimension signature** (`Registry::register_dim_signature`: parameters and result are monomials in signature variables, solved from the argument dimensions at each use; `demo/area(r)` for `fn area(r: Real[d]): Real[d^2]` is `dim(r)^2` in the kernel's dimension inference); then replace the function's binding in the module by a wrapper: when **all arguments are numeric** (or closed terms without free symbols) it runs the Aevia body, and with **symbolic arguments** it builds the operator node `demo/softplus(x)`, whose dimension comes from the signature, and an argument dimension that conflicts with the signature is `E0401`; only then are `@rule`s traced, so a rule can talk about source operators. When `diff`/`simplify`/`integral` act on an operator node, the kernel sees the operator, its definition and all rules about it. If the kernel rejects a plugin, the original binding is restored.
- **Dynamic loading**: `load_plugin(path) : String -> IO (Result<Int, String>)` loads, checks and evaluates a source file (including its imports) at run time, publishes its `@op`/`@rule`s and returns the number published; it needs the `fs` capability.

The `mydomain/` in `@op("mydomain/softplus")` is **part of the operator name** (the plugin namespace), not a `rule_set/name` qualifier; the kernel's definition head allows `/`-separated identifier segments, and `call_value` looks up the full string.

### 13.5.3 Security

- Plugins MUST be signed; by default only plugins the registry trusts are loaded.
- A plugin declares capabilities (`fs`, `net`, `unsafe`); undeclared capabilities are denied in the sandbox.
- A plugin ABI version mismatch is `E1006`.

---

## 13.6 Dimensions and units: unified on `rssn::dim`

### 13.6.1 The canonical layer

`rssn::dim` provides `DimTerm` (a product of base dimensions/variables with rational exponents, with structured access `factors(&arena)`/`vars(&arena)`), `DimArena` (interning, with snapshot/restore), `UnitRegistry` (unit names, aliases, exact scale `Scale { rat, pi }`, affine offset, prefixes, display, category, since), and `Dimension` (a **printable snapshot** with string exponents; use `DimTerm` for exact comparison and computation and never parse `Dimension.factors`).

Aevia's `Dim` is the type-level mirror of `DimTerm`; the canonical form, base names and exponent arithmetic of the two MUST agree. The compiler copies no unit table; `Units.*` is a generated view of `manifest.units` (82 entries at present). Units belong to a **session**: `Session::register_unit(UnitDef)` writes that session's registry, and only `build_with_units` writes it into the manifest; the process-level `build` sees the process table; units registered by a plugin belong only to the session that installed it.

The structured entry point is `Config::dimension_term(symbol, DimExpr)`: it parses no text, unknown base names are interned, and structured assumptions are applied after textual ones (the structured one wins on a name clash). `DimExpr::to_term` writes a term; `try_from_term` is the exact inverse for term-representable dimensions (`Length^n` reads back as `Length^n`); `from_term` is the total version and yields `DimExpr::Any` rather than a wrong dimension when it cannot read back.

The `DimRuleInfo` of `OpInfo.dim_rule` has `RootOf { degree, index }`: the result dimension is the `degree`-th root of the dimension of the **`index`-th argument** (the older `Root(n)` always took argument 0 and could not express the `sqrt(h)` of `sde_wiener(seed, m, steps, h)`); its manifest wire form is `{"root_of":{"degree":2,"index":3}}`. A host reading `dim_rule` MUST match on the variant and not assume only `Root`/`Same`/`Product`.

A custom base dimension is declared by `Config::base_dimension(name)` (idempotent): it sends the base name into the `DimArena`, the internal representation of `Dim` grows from a fixed seven SI exponents to "seven SI + an ordered side table", and `expr_to_dim` accepts any base name; this is what makes `unit EUR : Currency` work (`dimension_term` can produce a `DimTerm` only for base names already in the arena).

### 13.6.2 Unit declarations

```aevia
dimension Currency            # custom base dimension
unit USD : Currency = 1       # base unit
unit EUR : Currency = 1.08 USD
unit myunit : Length = 0.5 m  # built-in base dimension
```

For each `dimension` the generator declares `base_dimension("<Name>")` once on the `Config` and only then registers units; otherwise unit registration succeeds but no `DimTerm` can be produced.

- `dimension` registers in the `DimArena` (at compile time through `DimExpr`, at run time through `Config::dimension_term` / `Graph::assume_dimension_value`); `unit` registers in the `UnitRegistry`.
- Exported by default; `private` is visible only in the module; export information enters the manifest and downstream packages can `import` it directly.
- A user unit may be affine (`unit °X : Temperature = 1 K offset 273.15`), but an affine unit can be used only on its own.
- `features.user_base_dimensions` is hard-coded `true` (every build supports it); only `jit*` and `plugin_kinds` vary by build.

### 13.6.3 `Dyn`

The run-time dimension of `Dyn<ρ>` is a `DimTerm`, not a string. `dyn`/`check` are type-directed primitives: a concrete `d` is materialised as a `DimTerm` at compile time; generics pass it explicitly with `dyn_with`/`check_with`; `check` returns a `Result`.

### 13.6.4 `Point` and affine units

`Point[d]` is a static wrapper; at run time it is a `Float` + `UnitId`. The `affine`/`offset` of the `UnitRegistry` are the only source of truth. The arithmetic rules of `Point` are implemented in the Aevia layer, and rssn's `quantity`/`convert` handle display and conversion.

### 13.6.5 Physical constants

`manifest.constants` is a `Vec<ConstantValue>` (30 entries at present, 18 exact):

```rust
pub struct ConstantValue {
    pub name: String,        // "speed of light in vacuum"
    pub symbol: String,      // "c"
    pub value: ScaleValue,   // exact rational, coherent SI
    pub dimension: DimExpr,
    pub unit: String,        // "m/s"
    pub exact: bool,
    pub uncertainty: Option<ScaleValue>,
    pub source: String,      // "SI 2019 definition" / "CODATA 2022"
}
```

`Units.Constants` is generated from it: symbols are used as is (`c`, `g_n`, `h`, `hbar`, `k_B`, `N_A`, `e`, `G`, `alpha`, `R`, `F`, `sigma_SB`, `m_e`, `m_p`, `m_n`, `a_0`, `R_inf`, `mu_0`, `epsilon_0`, `mu_B`, `M_u`, `m_u`, `T_tp`, `atm`, `G_0`, `K_J`, `R_K`, `Phi_0`, `K_cd`, `dnu_Cs`); an `exact = true` constant has no `uncertainty`; a constant is a **value**, not a unit, and cannot appear in the unit position of `quantity(v, u)`; `e` is the elementary charge and has the same name as `Math.e`, so when both are visible it MUST be qualified (`Math.e` / `Units.Constants.e`).

### 13.6.6 Threads and arena isolation

In the template, the `DimArena` is an `Arc<RwLock>` that `Graph::clone` **shares**; on a write, `with_dim_arena_mut`/`define_base_dimension` sees `Arc::strong_count > 1` and clones before writing (copy on write). Therefore: `Config::base_dimension(name)` is the safe path (`prepare` defines it on the cloned graph, so requests do not affect each other); requests that define no base dimension share the template arena and pay no copy; never call `define_base_dimension` on a `Session`'s template graph directly; and across threads still use `Session::fork`/`merge` and never share a `Session` (a `RefCell`).

---

## 13.7 The central package registry

The registry is a package index that is **content-addressed, signed and capability-declaring**. The service is implemented in `platform/` (`aevia-registry-*` and `aevia-pkg`) and deployed as `aevia.apich.org`.

**Status**: implemented — `aevia add owner/name[@req]`, `aevia update`, `aevia fetch`, `aevia tree` (all with `--offline`) and `aevia pkg publish/yank/search/verify`; dependency resolution is done by `aevia-resolve` (SemVer 2.0, backtracking solver, lock files with `checksum`, §7.5.1/§7.5.2); git dependencies and unknown manifest keys remain `E0605`. Known gaps are in appendix C, Revision 15.

### 13.7.1 Package manifest

```toml
[package]
name = "aevia-thermo"
version = "0.3.0"
edition = "2026"
license = "MIT"
content_hash = "blake3:..."

[deps]
aevia-std = "^1.0"

[compat]
aevia = ">=1.0, <2.0"
rssn_api = "1"

[capabilities]
requires = ["fs"]
provides = ["thermo"]

[operators]
exports = ["thermo/enthalpy", "thermo/entropy"]

[units]
exports = ["J", "kJ", "cal"]

[dimensions]
exports = ["Entropy"]
```

### 13.7.2 Protocol

Logical operations (HTTP paths follow `/api/v1` of `platform/`):

| Operation | Description |
|---|---|
| Read the index | all package names + latest versions + summaries; carries an `ETag` |
| Download package / signature | a content-addressed archive; the digest of the stored bytes is verified on download |
| Search | `aevia pkg search` |
| Publish (signed) | checks: no path-only dependencies, no self-dependency, every dependency exists and has a non-yanked satisfying version; pre-release versions allowed, build metadata rejected; publication runs in one transaction |
| Yank | deletes no content, only blocks new resolution |
| Resolve / dependents | server-side dependency resolution; lists the dependents of a package |

An index entry contains at least: name, version, content_hash, deps (`package`/`req`/optional `import`), aevia_range, rssn_api, capabilities, operators, units, dimensions, stability, yanked.

### 13.7.3 Reproducibility

The package content hash covers the source, the manifest and the signature of the generated `Rssn` module; builds run no arbitrary scripts; dependency resolution uses content hashes + semver; registry mirrors MUST be verifiable by signature.

---

## 13.8 Stability and capabilities (replacing v1/v2)

### 13.8.1 Stability markers

```aevia
@stable
@experimental
@deprecated("use foo2")
```

`Stable`: signature and semantics unchanged within a major; `Experimental`: may change, but the old name is kept for at least one minor; `Deprecated`: a warning period of at least one minor.

### 13.8.2 Capabilities

Backend abilities are described by capability names. **The only source of names is the capability table**: it is derived from the manifest's `Features`, and `Rssn.Capabilities` lists only rows with `enabled = true` (`Capabilities::table()` of `aevia-rssn`). The full table:

```
core.dynamic_call
core.jit                    # false: the default build has no jit feature
core.jit_branch             # false (as above)
core.jit_index              # false (as above)
core.parallel_symbolic
core.higher_order
core.structured_results
core.sde                    # derived from features.sde
core.callback               # same-thread scoped callbacks; concrete structured signatures
core.plugin.source          # source-level @op/@rule/load_plugin (needs in-process plugins)
core.plugin
core.plugin.in-process
core.plugin.cdylib          # plugin-cdylib builds only
dim.exact_units
dim.affine_units
dim.user_base_dimensions
```

The default build's `Rssn.Capabilities` actually lists twelve rows (all except the three `jit*` rows and `plugin.cdylib`). There are also names that are **declared by the specification but have no corresponding `Features` field in the manifest**; they can appear in diagnostics but are not listed by `Rssn.Capabilities`: `core.symbolic`, `core.numeric`, `core.tensor`, `core.linalg`, `core.ode`, `core.pde`, `core.fft`, `core.stats`, `core.units`, `core.assume.general`, `core.gpu`, `core.distributed`, `io.binary`.

`storage.sparse` (§13.9.5/§8.4.1) is Aevia's own storage class, is enabled, and is not published by the manifest's `Features`, so it does not appear in `Rssn.Capabilities`.

After type checking the Aevia compiler checks capabilities; when one is missing: statically known (such as a `@jit` target) → `E1001` at compile time; a dynamic call → `OpError.Unsupported` at run time, which Aevia turns into `E1001`. **No language feature is removed**: when the GPU is unimplemented, `on(Gpu(0), ...)` is still well-typed and reports `E1001("core.gpu")` at run time.

---

## 13.9 Decided extension designs

### 13.9.1 Shape refinement

**Decision: supported, as a refinement and not full dependent types.**

- `Tensor<n, τ>` carries only a rank; `Tensor<[n1, ..., nk], τ>` is a shape refinement where each `n_i` is a type-level `Nat`, with `?` for unknown; `Matrix<n, m, τ>` is an alias of `Tensor<[n, m], τ>`.
- Tensor literals get a refined shape automatically; `zeros_of<[3,4]>()` and similar can state one explicitly. `matmul : Tensor<[m,k],τ> × Tensor<[k,n],τ> -> Tensor<[m,n],τ>`; the rank-only version is kept and checked at run time.
- `reshape<[n1, ..., nk]>(a)` always returns a `Result`: when shapes are statically known the element count is checked at compile time and the result is always `Ok`, otherwise it is checked at run time; `reshape_dyn` always checks at run time.
- Indexing: a refined shape plus a literal index gives a compile-time bounds check; dynamic indices are still checked at run time.
- Refined → rank-only is a safe implicit conversion; rank-only → refined uses `refine_shape`, returning an `Option`/`Result`.
- Type-level `Nat` supports literals, `+`, `*` and linear equations, with no general recursion or proof, which suffices for matrix products, reshape and concatenation.

### 13.9.2 `Point` tensors

**Decision: `Tensor<shape, Point[d]>` is supported.** Element-wise `+`/`-` follow the point rules (`Point - Point -> Float[d]`, `Point + Float[d] -> Point`, `Point + Point` is `E0405`); `mean` accepts only `Float`/`Real`, and the mean of points uses `centroid : Tensor<1,Point[d]> -> Point[d]`; a temperature field can be a `Tensor<[nx,ny], Point[Θ]>` directly; storage is `Float` + `UnitId`, and the affine offset is handled only in element-wise operations.

### 13.9.3 Row-polymorphic `Table` schemas

**Decision: `Table<ρ>` uses a row-polymorphic static schema, and the dynamic `Table` is kept.** `ρ` is a row type (field names + types + dimensions); `read_csv<ρ>(path)` infers field types from header units and reports `DataError.Schema` when it cannot; `column(t, "time")` is a type-level field lookup; `select`/`join`/`rename` stay row-polymorphic, and new fields require a `lacks` constraint; dynamic CSV returns `Table<Dynamic>` and `to_table<ρ>` validates at run time (details in §8.11).

### 13.9.4 Devices, GPU, distributed

**Decision: a device is a run-time value, not a tensor type parameter.**

- `Device = Cpu | Gpu(Int) | Distributed(Cluster)`; the `Tensor` type is unchanged, and values carry `device` metadata.
- `to_device : (Tensor<n,τ>, Device) -> IO (Tensor<n,τ>)`; `on : (Device, () -> a) -> IO a` is scoped execution (allocation and transfer are IO).
- `@jit(target = Gpu(0))` names the JIT target; without `core.gpu` it reports `E1001`. Distributed tensors use the library type `Sharded<Mesh, Tensor<...>>`, with `Mesh` a run-time topology. This avoids a combinatorial explosion of types for devices and shards.

**Status**: the `Device` value and `to_device`/`on`/`device_of` are typed; without `core.gpu`, `to_device`/`on` report `E1001("core.gpu")`. `@jit(target = Cpu)` is equivalent to `@jit`; `@jit(target = Gpu(n))` and `Distributed(..)` are accepted by the language and report `E1001("core.gpu")`/`E1001("core.distributed")` at check time; any other argument is `E0210`. In attribute arguments a "constructor applied to constants" (`Gpu(0)`) counts as a compile-time constant (§2.2.7). `Gpu`/`Distributed` have no backend and are only constructible run-time values (the rssn-side device layer is in §10.6.5).

### 13.9.5 Sparse and layouts

**Decision: `Sparse<F, shape, τ>` is a core type constructor and `F` is a format type.**

- Formats: `Csr`, `Csc`, `Coo`, `Bcsr` (`Block` is a synonym); the set is currently closed, and user-defined formats wait until rssn opens the `Format` kind (plugins can currently register only operators and units).
- The `Layout` class (in the design): `nnz`, `to_dense`, `from_dense`, `matmul`, `transpose`; the current implementation is built-in functions, not an instantiable type class.
- `@` dispatches on layout (dense×sparse, sparse×dense, sparse×sparse); only explicit conversions exist between `Sparse` and `Tensor`. rssn's `TypeExpr::Sparse` has only a `rank`, so `Sparse<F,shape,τ>` is lowered at the boundary to rank + run-time shape; refined shapes are checked and propagated by Aevia itself.

**Status**: implemented and `storage.sparse` is enabled — the run-time `Value::Sparse`, the four layout dispatches, `Bcsr`, preconditioned `cg_with`, non-symmetric Krylov solvers, slicing and rank-N contraction; details, exactness and error codes are in §8.4.1. **Remaining gaps**: the format set is still closed; sparse values cannot enter the rssn term graph.

### 13.9.6 Effects and capabilities

**Decision: no user-defined effect handlers; `IO` stays a single type.** The extension points are operators, plugins and the registry, not a language-level effect system. An `IO` action may declare the **capabilities** it needs (`fs`, `net`, `env`, `rand`, `gpu`) — a run-time permission, not a type parameter; `with_cap(cap, thunk)` runs in a sandbox, and an IO action fails when the capability is missing. This avoids effect rows intruding on every standard-library signature. Use plugins and operators for DSLs and code generation, not macros.

**Status**: `with_cap(caps, thunk)` and `aevia run --caps` are implemented, with semantics in §9.14 (capability sets narrow per block, nesting takes the intersection, a missing one is `E0902`). They are a different thing from the backend capability table of §13.8.2: the latter describes what the backend can do, the former what the program is allowed to do.

### 13.9.7 Macros and syntax extension

**Decision: no user macros; the syntax is fixed.** Extension is by operators, plugins, the registry and `Rssn` dynamic calls; `macro_rules`, hygienic macros and syntax extensions are not introduced, to avoid forking the toolchain. If one is ever really needed, it MUST appear as a separate, optional pre-processing layer outside the core syntax; the specification reserves no syntax for it.

### 13.9.8 Angles and uncertainty

- **`Angle` is not a base dimension**: `rad` is dimensionless and `deg` is a scale; `sin`/`cos` take `Real[1]` and a `deg` literal folds to `pi/180`; no `Angle` base dimension is introduced, to avoid breaking formulas like `exp(i*θ)` and `sin(ωt)`.
- **`Measurement<d>` is a standard-library type, not a core type**: independent error propagation is in §8.14; correlated errors use the library type `Covariance<d,e>`; the core language guarantees only dimensions.

### 13.9.9 Other closed questions

See the decision table in §12.9 (`det`, `Real`/`Float` mixing, `Dyn` errors, nested `RateOf`, `Table` schemas, `@`/`*`, `%`, `let` recursion, top-level IO, the registry, export of dimension aliases).

---

## 13.10 Compatibility and migration

- **Language versions** use semver, with no "v1 language / v2 language": only a major may break; a minor adds only capabilities, operators and library; a patch only fixes bugs.
- **Manifest compatibility**: an incompatible `api_version` is +1; the Aevia compiler pins a range; a change of `manifest_hash` invalidates caches, but old signatures stay usable until the next major.
- **Operator deprecation**: `Deprecated` has a warning period of at least one minor; aliases are kept.
- **Package registry**: content hash + signature + capabilities; a yank deletes no content and only blocks new resolution.
- **Migration**: users are not forced to rewrite code; deprecated operators keep aliases and old signatures stay usable within the pinned `api_version`. There is currently no automatic migration command.

## 13.11 Error codes

| Code | Situation |
|---|---|
| `E1001` | backend capability missing (`core.gpu`, `core.jit_branch`, …) |
| `E1002` | ambiguous operator/overload |
| `E1003` | no matching operator/overload |
| `E1004` | incompatible rssn `api_version` |
| `E1005` | manifest/signature hash mismatch |
| `E1006` | plugin ABI/load failure; failure to trace an `@op`/`@rule` |

These errors belong to the **environment/backend** and are not type errors in the user's program; a diagnostic MUST give the capability name, the minimum rssn version and an actionable alternative.


<a id="ch-appendix-a-grammar"></a>

# Appendix A — Complete grammar

This appendix is the normative EBNF. Notation:

- `{ X }` zero or more; `[ X ]` optional; `|` alternative.
- Terminals are written `"..."`; `NEWLINE`, `INDENT`, `DEDENT` and `EOF` are layout tokens
  （§1.4）。
- `INT`、`FLOAT`、`STRING`、`IDENT`、`UIDENT`、`UNIT_NAME`、`SUPERSCRIPT`、
  `LT_APP` and `ELLIPSIS` are lexical tokens (§1.2). A `UIDENT` starts with an ASCII capital; other names are
  an `IDENT`. `qname` is a qualified name.

## A.1 Layout

```
NEWLINE   ::= at the end of every logical line
INDENT    ::= when indentation increases
DEDENT    ::= when indentation decreases
```

The layout algorithm is in §1.4. Blocks and layout groups are both written `NEWLINE INDENT … DEDENT`:

```
block        ::= NEWLINE INDENT { block_stmt } expr NEWLINE? DEDENT
layout_group ::= NEWLINE INDENT expr NEWLINE? DEDENT
block_stmt   ::= let_decl | fn_decl
```

After `NEWLINE INDENT` the parser chooses `block` if it sees `let`/`fn` and `layout_group` otherwise;
`NEWLINE?` allows for an inner block that has already consumed the line end.

## A.2 Programs and declarations

```
program        ::= [ module_decl ] { import_decl } { top_decl } EOF

module_decl    ::= "module" module_path NEWLINE
module_path    ::= UIDENT { "." UIDENT }

import_decl    ::= "import" module_path [ "as" UIDENT ]
                   [ "{" name_list "}" ] NEWLINE
                 | "open" module_path [ "{" name_list "}" ] NEWLINE
name_list      ::= name { "," name }
name           ::= IDENT | UIDENT

top_decl       ::= { attribute } [ "private" ] decl
attr_arg       ::= [ IDENT "=" ] expr
attribute      ::= "@" IDENT [ "(" [ attr_arg { "," attr_arg } ] ")" ] NEWLINE?
decl           ::= dimension_decl
                 | unit_decl
                 | type_decl
                 | class_decl
                 | instance_decl
                 | derive_decl
                 | fn_decl
                 | let_decl
                 | test_decl

dimension_decl ::= "dimension" UIDENT [ "=" dim ] NEWLINE

unit_decl      ::= "unit" unit_name ":" dim "=" unit_rhs
                   [ "prefixable" ] NEWLINE
unit_rhs       ::= unit_scale [ unit_expr ] [ "offset" unit_scale unit_expr ]
unit_scale     ::= [ "-" ] number [ "/" number ]
                 | "pi" [ "/" number ]
unit_name      ::= UNIT_NAME | IDENT | qname

type_decl      ::= "type" UIDENT [ type_params ] [ "=" type_body ] NEWLINE?
type_body      ::= type
                 | "|" constructor { "|" constructor }
                 | NEWLINE INDENT { "|" constructor NEWLINE } DEDENT
constructor    ::= UIDENT [ "(" [ type { "," type } [ "," ] ] ")" ]

type_params    ::= "<" type_param { "," type_param } ">"
type_param     ::= IDENT [ ":" kind ]
kind           ::= "Type" | "Dim" | "Nat" | "Row" | "Rep" | "Format" | "Shape"

class_decl     ::= "class" UIDENT type_param { type_param } "where"
                   NEWLINE INDENT { method_sig NEWLINE } DEDENT
method_sig     ::= IDENT ":" type
                 | IDENT "(" [ params ] ")" [ ":" type ]

instance_decl  ::= "instance" UIDENT "for" type
                   [ "where" NEWLINE INDENT { method_def NEWLINE } DEDENT ]
method_def     ::= IDENT "(" [ params ] ")" [ ":" type ] "=" expr NEWLINE?

derive_decl    ::= "derive" name_list "for" type NEWLINE

fn_decl        ::= "fn" IDENT [ type_params ] "(" [ params ] ")" [ ":" type ]
                   "=" expr NEWLINE?
params         ::= param { "," param } [ "," ]
param          ::= pattern [ ":" type ]

let_decl       ::= "let" [ "rec" ] pattern [ ":" type ] "=" expr NEWLINE?

test_decl      ::= "test" STRING "=" expr NEWLINE?
```

- A one-line `type_body` variant list MUST start with `|` to distinguish it from an alias; the indented form may omit the first `|`.
- The trailing `NEWLINE?` of a declaration allows the expression to be a block.
- `import M` binds qualified names only; `import M { … }` and `open M { … }` bring in selected names; `open M` opens
  every export (§7.2).

## A.3 Expressions

```
expr           ::= let_expr
                 | if_expr
                 | match_expr
                 | fn_expr
                 | do_expr
                 | annot_expr
                 | block
                 | layout_group

let_expr       ::= "let" [ "rec" ] pattern [ ":" type ] "=" expr "in" expr
if_expr        ::= "if" expr "then" expr "else" expr

match_expr     ::= "match" expr "with" match_body
match_body     ::= NEWLINE INDENT { match_arm } DEDENT
                 | NEWLINE { match_arm }          # arms in the column of `match`
                 | inline_arm { inline_arm }      # arms on the line of `with` (§2.3.4)
match_arm      ::= "|" pattern [ "if" expr ] "->" expr NEWLINE?
inline_arm     ::= "|" pattern [ "if" expr ] "->" expr

fn_expr        ::= "fn" ( "(" [ params ] ")" | IDENT ) [ ":" type ] "=>" expr

do_expr        ::= "do" NEWLINE INDENT { do_stmt } DEDENT
do_stmt        ::= "let" pattern [ ":" type ] "=" expr NEWLINE
                 | pattern "<-" expr NEWLINE
                 | expr NEWLINE?

annot_expr     ::= pipe_expr [ ":" type ]

pipe_expr      ::= or_expr { ( "|>" | "<|" ) or_expr }
or_expr        ::= and_expr [ "||" or_expr ]
and_expr       ::= cmp_expr [ "&&" and_expr ]
cmp_expr       ::= cons_expr [ cmp_op cons_expr ]
cmp_op         ::= "==" | "!=" | "<" | "<=" | ">" | ">="
cons_expr      ::= add_expr [ "::" cons_expr ]
add_expr       ::= mul_expr { ( "+" | "-" ) mul_expr }
mul_expr       ::= unary_expr { ( "*" | "/" | "·" | "×" | "÷" | "@" ) unary_expr }
unary_expr     ::= ( "-" | "+" | "!" | "√" ) unary_expr
                 | power_expr
power_expr     ::= postfix_expr [ "^" unary_expr ]
postfix_expr   ::= primary { postfix }
postfix        ::= type_app | call | index | field | "?" | "!"
type_app       ::= LT_APP type_arg { "," type_arg } ">"
call           ::= "(" [ expr { "," expr } [ "," ] ] ")"
index          ::= "[" slice { "," slice } "]"
slice          ::= ".."
                 | expr ".." [ expr ]
                 | expr "..=" expr
                 | ":" [ expr ] [ ":" [ expr ] ]
                 | expr ":" [ expr [ ":" expr ] ]
                 | expr
field          ::= "." name

primary        ::= quantity_lit
                 | literal
                 | IDENT
                 | qname
                 | "(" expr ")"
                 | tuple
                 | record_expr
                 | tensor_lit
                 | "(" operator ")"
                 | "_"

qname          ::= UIDENT { "." UIDENT }
tuple          ::= "(" expr "," expr { "," expr } [ "," ] ")"

record_expr    ::= "{" [ record_body ] "}"
record_body    ::= expr "with" field_init { "," field_init } [ "," ]
                 | field_init { "," field_init } [ "," ]
field_init     ::= IDENT "=" expr
                 | IDENT

tensor_lit     ::= "[" [ expr { "," expr } [ "," ] ] "]"

operator       ::= "+" | "-" | "*" | "/" | "@" | "^" | "==" | "!="
                 | "<" | "<=" | ">" | ">=" | "&&" | "||" | "::"

literal        ::= INT | FLOAT | STRING | "true" | "false" | "(" ")"
quantity_lit   ::= ( INT | FLOAT ) unit_expr
```

**Notes**

- `LT_APP` is a `<` touching the preceding identifier (§2.7.2); `record_body` uses `with` to tell an update from a literal.
- `quantity_lit` arises only when a numeric literal is immediately followed by a unit expression (§1.8.1).
- `"_"` is valid only as a call argument (§2.3.6); `"(" operator ")"` parses, but the name is not yet bound
  (status note in §2.3.7).
- A `qname` starts with a `UIDENT`; `Physics.Ising.run` parses as a `qname` plus the field `.run`.
- Postfix `!` MUST touch the end of its operand (§2.7.5).

## A.4 Patterns

```
pattern        ::= "_"
                 | IDENT
                 | UIDENT
                 | UIDENT "(" [ pattern { "," pattern } ] ")"
                 | literal_pattern
                 | "{" [ field_pat { "," field_pat } ] "}"
                 | "(" pattern { "," pattern } ")"
                 | "[" [ pattern { "," pattern } [ "," ".." IDENT ] ] "]"
                 | pattern "as" IDENT
                 | "(" pattern ":" type ")"

field_pat      ::= IDENT "=" pattern
                 | IDENT

literal_pattern ::= INT | STRING | "true" | "false" | "(" ")"
```

- In a `match` arm a bare `UIDENT` is a nullary constructor pattern; in a binding position it is a variable (§2.4).
- Floating and negative literals are not patterns (`E0204`); use a guard.

## A.5 Types

```
type           ::= [ "forall" type_param { "," type_param } "." ] fn_type
fn_type        ::= type_postfix [ "->" type ]
type_postfix   ::= type_atom { "[" IDENT "]" }
type_atom      ::= "Int"
                 | "Real" [ "[" dim "]" ]
                 | "Float" [ "[" dim "]" ]
                 | "Complex" [ "[" dim "]" ]
                 | "Bool"
                 | "Prop"
                 | "String"
                 | "(" ")"
                 | "Tensor" "<" shape "," type ">"
                 | "Matrix" "<" nat "," nat "," type ">"
                 | "Sparse" "<" format_name "," shape "," type ">"
                 | "Table" "<" row_type ">"
                 | "Point" "[" dim "]"
                 | "Unit" [ "[" dim "]" ]
                 | "Set" "<" type ">"
                 | "Option" "<" type ">"
                 | "Result" "<" type "," type ">"
                 | "Solutions" "<" type "," dim ">"
                 | "Dyn" "<" rep ">"
                 | "RateOf" "<" type ">"
                 | "SymFun" "<" dim "," dim ">"
                 | "IO" type
                 | "{" [ field_type { "," field_type } [ "," ]
                         [ "..." IDENT ] ] "}"
                 | "(" type { "," type } ")"
                 | qname [ "<" type_arg { "," type_arg } ">" ]
                 | IDENT
                 | "(" type ")"

field_type     ::= IDENT ":" type
type_arg       ::= type | nat | dim | shape
format_name    ::= IDENT | UIDENT
shape          ::= nat | "[" [ shape_item { "," shape_item } ] "]"
shape_item     ::= nat | "?"
row_type       ::= type { "+" type }
nat            ::= nat_term { "+" nat_term }
nat_term       ::= nat_atom { "*" nat_atom }
nat_atom       ::= INT | IDENT | "(" nat ")"
rep            ::= "Int" | "Real" | "Float" | "Complex" | IDENT
```

- `Real`/`Float`/`Complex` without brackets mean `[1]`; bare `Unit` is `()` and `Unit[d]` is the type of unit
  values; `Tensor<0, τ>` equals `τ`; `RateOf<s>` is a built-in type function (§8.9).
- `(A, B)` is a tuple type and `(A, B) -> C` a multi-argument function type; `A * B` is not a type.
- The first argument of `Tensor` is parsed as a `shape` and the argument of `Dyn` as a `rep`; other `type_arg`s are parsed
  by kind.

## A.6 Dimensions and units

```
dim            ::= dim_factor { ( "*" | "/" | "·" ) dim_factor }
dim_factor     ::= dim_atom [ "^" exponent ]
dim_atom       ::= "1" | UIDENT | IDENT | "(" dim ")"

exponent       ::= [ "-" ] INT
                 | "(" [ "-" ] INT "/" INT ")"

unit_expr      ::= unit_factor { ( "*" | "·" | "/" ) unit_factor }
unit_factor    ::= unit_atom [ "^" exponent | SUPERSCRIPT+ ]
unit_atom      ::= unit_name | "(" unit_expr ")" | "%"
```

- In a `dim`, a `UIDENT` is a base dimension or alias and an `IDENT` a dimension variable; `/` is left-associative (`L/T^2` = `L/(T^2)`).
- `·` is normalised to `*` in the lexer (§1.9).
- Numbers are not allowed inside a unit expression (exponents excepted); a `unit_name` may carry a prefix or be `°C` (§1.8).
- An affine unit cannot appear in a product or power (`E0508`).

## A.7 Operator precedence

See §2.6. `..`/`..=` occur only in indices and take no part in expression precedence; how the slice stride `:` relates to
`::` is in §2.3.11.

## A.8 Lexical tokens (summary)

```
IDENT          ::= lower XID_Continue { XID_Continue | "'" | "₀".."₉" }
UIDENT         ::= upper XID_Continue { XID_Continue | "'" | "₀".."₉" }
INT            ::= dec_int | "0x" hex { hex } | "0o" oct { oct } | "0b" bin { bin }
FLOAT          ::= dec_int "." dec_int [ exponent ]
                 | dec_int exponent
STRING         ::= '"' { char | escape | "{" expr "}" } '"'
                 | 'r"' { char } '"'
                 | '"""' ... '"""'
UNIT_NAME      ::= "°" [ UIDENT ] | IDENT
SUPERSCRIPT    ::= "⁰".."⁹" | "⁻" | "⁺"
LT_APP         ::= "<" touching the preceding identifier (no space)
ELLIPSIS       ::= "..."
NEWLINE        ::= "\n"
INDENT         ::= produced by the layout algorithm
DEDENT         ::= produced by the layout algorithm
```

The full lexical rules are in chapter 01. `forall` is a contextual keyword (§1.5.2); `pure` is a Prelude function, not a
keyword.


<a id="ch-appendix-b-diagnostics"></a>

# Appendix B — Diagnostic catalogue

Diagnostics are part of the language experience. An error message MUST contain the code, the
primary location and expected versus actual, and, when it can be determined, at least one
actionable fix. This appendix lists the codes, severities and the message format. Every code in
the tables has an emission site in the compiler (checked by `grep` over all crates); exceptions
are noted in the "Status" paragraphs.

## B.1 Format

```
error[E0401]: dimension mismatch
  --> src/kinematics.aev:4:31
   |
 4 | fn f(h: Float[L]): Float[T] = sqrt(2 * h * g)
   |                               ^^^^^^^^^^^^^^^ expected T, found L/T
   |
   = left:  L/T
   = right: T
   = difference: L/T^2
```

- The first line is `severity[code]: message`; `-->` gives file:line:column; then come the
  source line and underline labels (kept short).
- `=` lines are notes (canonical forms, differences, related declarations); `help:` is a fix,
  at most two, ordered by confidence.
- An error with several locations uses several underline labels or a `= note:` pointing at the
  definition.

## B.2 Severity

| Level | Meaning |
|---|---|
| `error` | the program cannot compile or run |
| `warning` | the program runs but may be wrong |
| `note` | explanatory information, never alone |

## B.3 Lexical errors (E01xx)

| Code | Message | Typical fix |
|---|---|---|
| `E0101` | tab character in indentation | replace with spaces |
| `E0102` | inconsistent indentation | align to an existing level |
| `E0103` | expected an indented block | indent the next line |
| `E0104` | unclosed string/comment/bracket | add the closer |
| `E0105` | invalid character / escape | delete or replace |
| `E0106` | invalid numeric literal | check underscores and exponent |
| `E0107` | invalid superscript sequence | use `^n` |
| `E0108` | expected a unit after the numeric literal | write `3 * x` or add a unit |

## B.4 Syntax errors (E02xx)

| Code | Message | Typical fix |
|---|---|---|
| `E0201` | empty block | add an expression |
| `E0202` | syntax error (including `private` on a disallowed declaration, and `_` evaluated outside a call at run time) | check the nearby tokens |
| `E0203` | ragged tensor literal | make every level the same length |
| `E0204` | invalid pattern | check the constructor or fields |
| `E0205` | invalid operator section | use `(+)` or a lambda |
| `E0206` | reserved word used as identifier | rename |
| `E0207` | ambiguous type application | add a space before `<` or parenthesise the callee |
| `E0208` | duplicate field | delete the duplicate |
| `E0209` | invalid unit literal (including `3 (2)`: the parentheses do not hold a unit expression) | check the unit expression |
| `E0210` | unknown attribute | check the attribute name or arguments |

## B.5 Type errors (E03xx)

| Code | Message | Typical fix |
|---|---|---|
| `E0301` | unbound name | import or fix the spelling |
| `E0302` | type mismatch (including an unsolvable type-level `Nat` equation, an unknown capability name, a `to_system` name that is not a literal) | check annotations and arguments |
| `E0303` | infinite type (occurs check) | annotate or change the recursion |
| `E0304` | ambiguous type (including an unsolved `Nat` variable, a non-linear `Nat` equation, a shape variable with no binder, a missing `from_system<D>`) | add a type annotation |
| `E0305` | not a function | check the call |
| `E0306` | missing record field | add the field or change the type |
| `E0307` | constructor arity mismatch | add or remove arguments |
| `E0308` | pattern type mismatch (including a `Prop` used as a guard) | change the pattern |
| `E0309` | non-exhaustive match | add arms |
| `E0310` | no instance for class | import or add an instance |
| `E0311` | overlapping instances | merge or delete |
| `E0312` | non-positive recursive type | move it to the right of the arrow |
| `E0313` | IO action in a pure context | use `do` or return `IO` |
| `E0314` | cannot infer rank/element type (empty tensor, `read_npy` without a rank) | add a `Tensor<n, …>` annotation / `read_npy<2>` |
| `E0315` | wrong number of arguments | add or remove arguments |
| `E0316` | duplicate definition | rename or delete |
| `E0317` | cannot derive instance / unsupported parameter kind | write the instance by hand |
| `E0318` | function is less generic than its declared type parameters | make the body valid for every choice, or replace the parameters with the concrete types the body needs |

## B.6 Dimension errors (E04xx)

| Code | Message | Typical fix |
|---|---|---|
| `E0401` | dimension mismatch | give both sides the same dimension |
| `E0402` | exponent must be a dimensionless number / finite | the exponent must be dimensionless (an infinite exponent in the exact phase uses this code too, §6.3.4) |
| `E0403` | transcendental function requires dimensionless argument | divide by a unit |
| `E0404` | dimensionful base with a run-time exponent | literal exponent / `pow_dyn` |
| `E0405` | invalid affine point operation | use a difference or `lerp` |
| `E0406` | bare `0` used where a dimensioned value is expected | write `0 m` |
| `E0407` | unknown dimension/alias | declare or import |
| `E0408` | unsatisfiable dimension constraint | reserved |
| `E0409` | dimension occurs check | reserved |
| `E0410` | dimensionful `det`/`product` requires `Dyn<ρ>` | divide by a unit / `det_unit` |
| `E0411` | incompatible tensor element types or dimensions | unify the element type or dimension |
| `E0412` | `Dyn`/kernel dimension check failed (run time) | check the run-time dimensions |

Status: `E0408` and `E0409` are reserved and unreachable in the implementation. Dimension
unification is exponent-linear unification over a free abelian group; a residual with a
non-zero coefficient on a variable is always solvable, so the only failure is a non-zero
constant residual, which is `E0401`, and no occurs state arises (proof in
`aevia-types/tests/diagnostics_coverage.rs`).

## B.7 Unit errors (E05xx)

| Code | Message | Typical fix |
|---|---|---|
| `E0501` | unknown unit (including a stacked prefix `kkm` and use before declaration) | import or fix the spelling |
| `E0502` | unit not imported | reserved |
| `E0503` | prefix cannot be applied | reserved |
| `E0504` | affine offset dimension mismatch | check the offset |
| `E0505` | duplicate unit | delete the duplicate |
| `E0506` | the scale has no exact value for that exponent | check the definition or exponent |
| `E0507` | prefixed unit conflicts with explicit unit | reserved |
| `E0508` | affine unit in product/power | use it alone |
| `E0509` | `to`/`value_in` unit measures a different dimension | change the unit |
| `E0510` | affine unit used on a temperature difference | use `delta_to` |
| `E0511` | unit has no exact scale in the module's unit system (§5.11.4) | write the number in system units, or `to_system` |
| `E0512` | unknown unit system (§5.11.1) | use a name the kernel provides (natural, planck, atomic, geometrized) |

Status: `E0502`, `E0503` and `E0507` are reserved: there is no module-level unit namespace
(§5.9), so the corresponding states do not exist.

## B.8 Module errors (E06xx)

| Code | Message |
|---|---|
| `E0601` | module name does not match path |
| `E0602` | cyclic import |
| `E0603` | the same name opened by two modules with different types |
| `E0604` | orphan instance |
| `E0605` | invalid package manifest |
| `E0606` | dependency version conflict |
| `E0607` | module/package not found |
| `E0608` | no runnable module/entry point (embedding API) |

## B.9 Effect errors (E07xx)

| Code | Message |
|---|---|
| `E0701` | `do` statement or `<-` expects an `IO` action |
| `E0702` | `?` requires a `Result`/`Option` and a matching return boundary |

## B.10 Run-time errors (E08xx)

| Code | Message | Recoverable |
|---|---|---|
| `E0801` | division by zero | exact: no; floating point: IEEE |
| `E0802` | shape/index error | no |
| `E0803` | free symbols in `approx` | yes |
| `E0804` | cannot decide symbolic condition / assumption is false | no |
| `E0805` | stack overflow / tail-call or IO budget exceeded | no |
| `E0806` | domain error | yes |
| `E0807` | numerical method did not converge | yes |
| `E0808` | budget exhausted | yes |
| `E0809` | internal type/dimension invariant violated | no |
| `E0810` | IO error | yes |
| `E0811` | task failure: a result that cannot cross threads, a task awaited twice, or a stopped worker (§9.7.3) | yes |
| `E0899` | panic | no |

Status: a few built-ins borrow `E0806`–`E0809` to report their own argument or internal
failures (for example `approx_map`, `degree`); the class of the code is as in the table and the
message is whatever the built-in states.

## B.11 IO errors (E09xx)

| Code | Message |
|---|---|
| `E0901` | file not found |
| `E0902` | permission denied (including a capability not granted, §9.14) |
| `E0903` | parse error in data file |
| `E0904` | unsupported format |
| `E0905` | plot rendering failed |

## B.12 Warnings (W0xxx)

| Code | Message |
|---|---|
| `W0101` | unused variable |
| `W0102` | unused import (reserved) |
| `W0103` | shadowed binding |
| `W0201` | redundant match arm |
| `W0202` | discarded IO result |
| `W0203` | unreachable code (reserved) |
| `W0301` | unresolved dimension defaulted to 1 |
| `W0302` | `Real`/`Float` mix uses the exact `Real` path |
| `W0303` | unused symbol |
| `W0501` | display unit dropped (no emission site yet, §5.4.2) |
| `W0601` | unused private declaration |
| `W0602` | deprecated name used: `@deprecated("...")` on the declaration (§2.2.7/§13.8.1) |

Status: `W0102` and `W0203` are reserved. The checker sees only a flattened name table, so a
selective import cannot be told from `open M`; the language has no `return` or `break`, so the
AST cannot represent an unreachable statement. `W0501` currently has no emission site.

## B.13 Writing messages

1. **Speak plainly.** `expected Real[L], found Real[T]` beats `type unification failed`.
2. **Point small.** The underline covers the smallest erroneous subtree.
3. **Give the difference.** A dimension error lists the canonical forms `left`, `right` and
   `difference`.
4. **Give one fix.** Offer `help` only with high confidence; do not list five guesses.
5. **Do not repeat.** Report one error once and point to the primary location with `note`.
6. **Preserve source.** A source line in a message does not truncate operators; wrap if needed.
7. **Be parseable.** Codes and positions are stable, for IDE and CI parsing.

## B.14 Example: type error

```
error[E0302]: type mismatch
  --> t.aev:2:14
   |
 2 | let n: Int = 1.5
   |              ^^^ a bare numeric literal cannot have type Int
   |
```

## B.15 Example: unit error

```
error[E0501]: unknown unit `kms`
  --> t.aev:2:9
   |
 2 | let d = 3.2 kms
   |         ^^^^^^^
   |
help: did you mean `km`?
```

## B.16 Example: run-time shape error

```
error[E0802]: matmul requires left columns (2) = right rows (3)
  --> r.aev:5:13
   |
 5 |     let C = A @ B
   |             ^^^^^
   |
```

## B.17 Extension and backend errors (E10xx)

| Code | Message | Notes |
|---|---|---|
| `E1001` | backend capability or witness missing | carries the capability name and the minimum RSSN version; also used for a missing kernel dimension witness or an invalid callback handle |
| `E1002` | ambiguous operator/overload | lists the candidate signatures |
| `E1003` | no matching operator/overload | lists candidate signatures or the dynamic entry; also used for a callback arity or shape mismatch |
| `E1004` | incompatible RSSN api_version / kernel result fields | asks to upgrade or downgrade; also when the kernel omits or adds a declared record field |
| `E1005` | no pinned overload / signature hash mismatch | regenerate the `Rssn` module |
| `E1006` | plugin ABI/load/registration failure | check the plugin signature or version |

These are **environment/backend** errors, not type errors in the user's program; a diagnostic
MUST offer an actionable alternative (for example "install an RSSN with `core.gpu`" or "use
`Rssn.dynamic`").

The next appendix records the specification's revision history.


<a id="ch-appendix-c-changelog"></a>

# Appendix C — Revision log and known implementation limits

This appendix records the evolution of the Aevia specification and its implementation, the "status stories" the chapters no longer carry, and the limits that are explicitly on record as unimplemented. The normative chapters say only **what is true now**; history lives here. This appendix is informative.

---

## Revision 23 (2026-11-14): full review against the implementation, and an English edition

This pass compared the specification with the implementation chapter by chapter, condensed repetition, moved status notes out of the text, and added an English edition (`spec/en/`; writing and terminology rules in `spec/STYLE.md`). The two machine-read chapters (08 and 11) keep their shape and are guarded by `cargo test -p aevia-std --test spec_signatures --test spec_programs`.

**Whole book**

- New English edition: every Chinese chapter has a counterpart `spec/en/<same name>`, with the same section numbers, anchors, code blocks and diagnostic codes.
- Status notes ("implementation status (date)", "added in Revision N", "task-NN closed") moved out of the chapters into this appendix; a chapter keeps one line of *Status*.
- Measured figures (operator, unit and test counts) no longer scatter across chapters: the manifest size is given once, in §10.8, and the other chapters point to `aevia rssn`.

**This batch (08, 10–13, README, this appendix)**

| File | Corrections | Additions | Removals / moved out |
|---|---|---|---|
| `08-stdlib.md` | Stale figures in §8.19/§8.20 such as the manifest counts, "323/323", "309 declarations", "37 modules" (now 1620 operators and 41 module files, with coverage asserted by tests); the typed count of `Rssn_Numeric` became "all typed"; an English sentence inside a Chinese paragraph in §8.7.1 | Sparse, FFT and sequence-operation subsections merged into one narrative with the rules unchanged | "Implementation status" stories (`par_map`, `args`, SDE, sparse, Plot, NPY, simulation) compressed into one line of status; Revision-number asides; references to the `.workbuddy-ai` audit document |
| `10-backend.md` | Manifest figures in §10.0/§10.8 (1382 → 1620, new `manifest_hash`); §10.5.1 "`par_map` runs sequentially" became worker-thread replicas (§9.7.2); the §10.9 deviation note ("dimensioned literals fold to exact rationals, printing `sqrt(800000/196133) s`"; `fall_time(20.0 m)` now prints `2.0196199771025523 s`) | §10.3.7 points to the generated `Rssn_Sim_<Name>` modules; the gap list of §10.8 merged duplicates | The task-31/`emit_typecheck` narrative, a duplicated SDE status paragraph, the English summary mixed into §10.6.5 |
| `11-examples.md` | The outputs of 11.1–11.5 now match `aevia run`; the end time of 11.2 is 3 s (at 10 s the projectile had landed and the height was negative); the shape of `series` and the `Solutions` record of `solve` in 11.3; the diagnostics of 11.9 match `aevia check`; shadowing warnings in 11.10 | A note that 11.8 is a library module (no `main`) | Repeated "this shows the benefit of…" paragraphs |
| `12-rationale.md` | 12.8.7, parallel symbolic values (the `fork/merge` wording replaced by the actual behaviour of §9.7.2); 12.10, the roadmap changed from effort estimates to a progress table | — | The week counts of M0–M6, duplicated acceptance items |
| `13-extensibility.md` | §13.7 registry: it said "design complete, implementation not started, dependencies report `E0605`", whereas `aevia add/update/fetch/tree`, `aevia pkg publish/yank/search/verify` and the resolver are implemented (Revision 15); the capability table of §13.8.2 gains `core.callback` and `core.plugin.source`, with the actual twelve rows authoritative; "sparse checks only the rank" in §13.9.5 became "refined shapes are checked"; the non-existent `aevia fix` and `tools/gen_rssn_module.aev` removed | The precision-tier table of 13.2.2 merged with the scoring rules of §13.4.1 | The history of manifest hashes (`1920638b…→8ed92a22…→8d2c3764…`), task-31/task-22 narratives, repeated SDE status, long "implementation status" paragraphs of several sections |
| `README.md` | "Revision 13" → 23; counts such as "1476 operators in 37 rule sets" replaced by a pointer to `aevia rssn`; the reading-guide table aligned with chapter titles | A Chinese edition (`spec/README.md` used to be English only) | — |
| `appendix-c-changelog.md` | "37 standard-library modules" → 41 in §2; the known-limits list of §5 updated (SDE events and shape refinement are implemented, the registry is implemented) | This entry | Revisions 12–22 condensed to key points |

**Other chapters (01–07, 09, appendices A/B)**: per-chapter records are collected by spec-batch-a (01, 02, appendix A) and spec-batch-b (03–07, 09, appendix B) and will be added here once their final reports arrive.

**Stale points found in review, outside this batch**: the end of §9.11 in `09-runtime.md` still says "the registry client is still unimplemented (dependencies are path-only)", which contradicts §13.7 and Revision 15; it should be corrected by that chapter's batch.

---

## 1. Summary of the specification's evolution

- **Revision 0 (2026-10-31)**: fixed the language core (pure functional, dimensions as types, static rank with dynamic-shape tensors, symbolic/numeric phases, the rssn backend).
- **Revisions 1–6 (2026-11-01 ~ 2026-11-04)**: bidirectional type checking, unit derivation and affine temperature scales, row polymorphism and pattern matching, a unified ODE/PDE interface, diagnostic codes (E0100–E1000).
- **Revisions 7–10 (2026-11-05 ~ 2026-11-07)**: the standard-library module tree (§8.20); native JIT (`aevia-core/src/jit.rs`, the `jit` feature); worker-replica evaluation of `par_map` (`--jobs N`); `read_npy`/`write_npy`.
- **Revision 11 (2026-11-08)**: fixed a `BigRational` panic for a negative power of zero and a DoS on huge exponents; Unicode superscript brackets in the lexer; a zero-length-message loop and a `did_change` crash in the LSP; the `0^(-n)` singularity and fractional-root guards in the symbolic engine; tensor Cons dimension checks; measurement division by zero; machine-readable `diagnostic_codes` for RSSN FFI batch computation; 0×0/1×1 boundary cases of the dense eigenvalue solver.

## 2. Current implementation status (correcting stale audit records)

1. **`par_map` parallel evaluation**: implemented (Rayon + independent evaluator replicas, input order, `--jobs N`).
2. **`@jit`**: implemented (the `jit` feature; pure numeric branches and self tail-recursive loops are lowered to Cranelift, unsupported constructs fall back to the interpreter); the default build has `core.jit` off.
3. **Standard-library module import**: implemented; 41 `.aev` module files in `crates/aevia-std/stdlib/`.
4. **NPY reading and writing**: implemented (with a static rank check).

## 3. Revision 12 (2026-11-08): caching, `Bcsr`, source operators, non-commutative SDE

1. **Caching.** rssn's `Session` request-level answer cache (on by default; keyed by the request node, template and configuration fingerprint; runs that hit the wall-clock limit are not cached; invalidated wholesale when a plugin or unit is registered) and a sub-term lemma cache (`CachePolicy::lemmas`, off by default); Aevia's per-module on-disk interface cache `.aevi` (§9.11; `AdtId`s are replaced by symbol keys when written).
2. **`Bcsr` (§8.4.1).** rssn gained `kernels::bsr::Bsr`; Aevia gained `SparseFormat::Bcsr`, `to_bcsr`/`block_size`, and automatic block choice in `convert<Bcsr>`; preconditioned `cg_with`, Cholesky-first `spsolve` for symmetric positive definite matrices, and sparse slicing. `E1001("storage.sparse.block")` was removed.
3. **Source operators (§13.5.2).** `@op` registers a dimension signature from the checked dimensions; the module binding is replaced by a wrapper (numeric arguments run the body, symbolic arguments build an operator node); `@rule` is traced after the operators; the rule parser accepts `ns/name`.
4. **Non-commutative SDE (§8.9.1).** Stratonovich non-commutative Milstein, non-commutative SRK, adaptive Stratonovich (exact Itô conversion); the method name selects the scheme family and interpretation, the noise structure selects the kernel; `terms`/`cells` have defaults; record state with general noise.
5. **Features that errored on use.** `symfun` application builds an undefined-function term; `dsolve` was added (§6.4.5); `@jit(target = Cpu)` is accepted and `Gpu`/`Distributed` report `E1001` at check time; the single-file specification is built by `tools/bundle_spec.py`.
6. **Long tensor literals.** Literals over 128 elements no longer give `E0304`: variable chains are followed iteratively and duplicate numeric constraints are deduplicated, taking the check of a 2000-element literal from ~7 s to 0.6 s.
7. **rssn limits.** `limit(sqrt(x+1) - sqrt(x), x, oo)` is now found by a Puiseux substitution; fixed an error in the l'Hôpital pre-check (`limit(sqrt(x)*(sqrt(x+1) - sqrt(x)), x, oo)` should be 1/2).

## 4. Revisions 13–22

**Revision 13 (2026-11-07 ~ 11-09): trust boundary and tightened rules.**
Whitespace rule for unit expressions (§1.8.1): whitespace separates a unit from arithmetic, so `3 m * x` is `3 m` times `x` while the tight `3 m*x` is still a unit expression; an unresolved dimension variable used as the base of an exponent is made `1` (§3.4.3); declared dimension parameters are rigid and a violation is `E0318` (§4.4.1); `show` keeps the phase of a `Float` (`10.0` differs from `10`, §6.12.3); `Set<τ>` gained values and operations (§8.1.5.1); the run-time trust boundary (§9.2.3): the run time executes the checker's proof and does not re-prove it; literal folding (§3.10.4): a literal is rounded only once, and a closed tree of literal arithmetic is a compile-time constant; unit systems `module M with units natural|planck|atomic|geometrized` (§5.11, `E0511`/`E0512`); the commands and flags of §9.9 aligned with the actual CLI.

**Revision 14 (11-10): automating rssn bindings.** Domain modules `Rssn_<Domain>` (§10.3.8): manifest + declarative overlay, generic conversions, errors through `Result`, a coverage check and report; dimension evidence; rich types `Zmod`/`Perm`/`Graph`/`Group`/`PermGroup`. Still missing: type-level moduli and operator overloading for `Zmod<n>`/`GF<p>` (§10.3.8 lists the language changes needed).

**Revision 15 (11-10): package platform and dependency resolution.**
`aevia-resolve` (SemVer 2.0 `Version`/`VersionReq`, a backtracking solver with conflict-directed backjumping, a sparse-index reader, cross-checked against exhaustive search over 2000 random rounds); manifests accept `Units = "^1.2"` and `{ package = "owner/name", version = … }`, lock files gain `checksum` (source `registry+aevia`), and `E0605` still rejects git and unknown keys; the `platform/` index records `deps` (`package`/`req`/optional `import`) and checks on publish (no path-only dependencies, dependencies exist and have a non-yanked satisfying version), publish and yank each run in one transaction, with `POST /api/v1/resolve` and a dependents endpoint; client commands `aevia add/update/fetch/tree` (`--offline`) and `aevia-pkg` `index`/`vendor`/`lock [--check]`.
Still missing: per-package dependency namespaces (same-named import roots pointing to different versions remain `E0606`); `[patch]` overrides; `aevia-pkg tree/lock` read only the root manifest; `[source]` replacement for mirrors; no per-file digests for unpacked source trees.

**Revision 16 (11-12): witnessed automatic typing (§10.3.8).** The kernel records the engine's equality judgements (a request and its answer in one equivalence class) as sort facts, and replay, clustering and mutation probing produce `signatures.witnessed.json`; Aevia generates same-shape typed wrappers for operators with no `sig` (the dynamic tier fell from 1298 to 871 operators, adding 453 automatically typed ones); opaque `Value` handles; dimension evidence for witnessed overloads; exact/symbolic sparse matrices (`sp_*`) and D-finite decision (`holonomic_*`) became available. Still missing: witnessed parameter names are inferred from types; tuples and ragged lists stay in the dynamic tier; several overloads are told apart by `_int`/`_real`/`_float` suffixes.

**Revision 17 (11-12): numeric operator groups, dimension-independent names, GPU `f64`.** `Rssn_Numeric` absorbed the 73 operators newly added to rssn's `numeric` rule set (robust statistics, windows and spectra, wavelets, Krylov/sparse/low-rank/tensor, finite differences, complex special functions, expression operators); conventions: results are `Float` (the kernel encodes only floats) and method/wavelet/statistic names are symbols (bind each name once with `let`); dimension-independent names (the spatial dimension of `heat_fd` etc. is the nesting depth, likewise symbolic PDEs and affine transforms); rssn's wgpu device provides `Dtype::F64` as double-single, and a launch-fusion planner `device::plan` was added (on the Aevia side `Gpu(n)` still reports `E1001`); parameter names of witnessed wrappers are inferred from types.

**Revision 18 (11-13): multiple signatures named by rank/dimension.** An overlay's `sig op as name(..)` may repeat for one operator; `codec Type - from_fn` (decode-only types); `_2d`/`_3d` finite differences; rank-polymorphic tensor operators `tensor_norm_<r>`/`tensor_inner_<r>`/`tensordot_<ra>_<rb>_<k>` and decompositions `hosvd`/`hooi`/`cp_als`/`tt_svd`/`tt_dense` (r = 2–4); mixed lists and the list accessor `from_value<Tensor<1, Value>>`; `Rssn_Numeric` fully typed. Still missing: tensor operators and decompositions beyond rank 4 still go through `Rssn.dynamic`.

**Revision 19 (11-13): language-side completion.** `sum`/`prod`/`mean`/…/`transpose` at ranks 2–4 (`transpose` reverses axes for rank ≥ 2); `Result` `map`/`and_then`/`unwrap_or`/`is_ok`/`is_err`/`ok`/`ok_or`; `first`/`last`/`cumsum`/`cumprod`/`argmax`/`argmin`, `round(x, n)`, `is_nan`/`is_finite`/`is_infinite`; diagnostic suggestions (`E0407` says `m` is a unit, spelling suggestions, record-field hints); `aevia fmt` keeps blank lines between statements.

**Revision 20 (11-13): the FFT family and sequence operations.** `fft`/`ifft` accept `Complex`/`Float`/`Int`; multi-dimensional transforms `fft2`/`fftn`/`fft_axis`/`fftn_axes`/`rfft2`/`rfftn`/`irfft*`/`fftshift` etc. (row–column decomposition); the normalisation of `fft_norm(x, Forward)` was fixed to `1/N` forward; `reverse`/`filter`/`any`/`all`/`find`/`first`/`last`/`take`/`drop`/`concat` at rank ≥ 2 act on rows along the first axis (a semantic change for `reverse`: it no longer flattens), plus `reverse_axis`; tie-breaking of unit suggestions (edit distance → minimal-containing edit → common unit → lexicographic).

**Revision 21 (11-13): kernel diagnostics enter `Err(OpError)`.** rssn gained `Diag`/`DiagKind` and `Session::explain`; when an operator leaves its own unreduced call, the bridge fetches the reason, and the `Err` of `Rssn.dynamic` has the form `rssn.diag.<kind>: <wrapper name>: argument `<parameter name>` (#<position>): <message>`; a typed call site raises `E0807`; `Rssn_Numeric` gained `poisson_fd_bc`/`heat_fd_bc`/`wave_fd_bc`, `advect_fd_upwind`, `tensor_contract_*`, `tucker_dense_*`, `cp_dense_*` and others. Still missing: only operators of `rules::numeric::TABLES` record reasons; `OpError` has no `code(e)`/`message(e)` accessors.

**Revision 22 (11-13): rank has no upper limit.** The signatures of rank-polymorphic operations are **templates** over the rank (`builtins::rank_schemes_for`), instantiated by the checker at the rank of the argument when every built-in candidate fails; `fftn`/`sum` at rank 5 and 6 take the same path as rank 2; `fold`/`fold_left`/`sort`/`sort_by`/`sort_float`/`zip`/`enumerate` at rank ≥ 2 changed from element-wise (flattened) to **row-wise** (a semantic change); added `sort_by_key`, `sort_axis`, `argsort`, `rfftn_axes`/`irfftn_axes`; `sort_by` became a stable merge sort. Still missing: user functions cannot be rank-polymorphic; `cumsum`/`cumprod`/`argmax`/`argmin` have no per-axis forms at rank ≥ 2.

## 5. Known limits on record

1. **Package dependencies**: registry dependencies, SemVer resolution and checked local copies are implemented (Revision 15); `git` dependencies do not exist; per-package dependency namespaces, `[patch]` overrides and mirror `[source]` replacement are not implemented.
2. **Device backends**: `Gpu`/`Distributed` are only constructible run-time values; `on`/`to_device`/`@jit(target = Gpu(..))` report `E1001("core.gpu")`/`E1001("core.distributed")`; batch primitives such as `Device.map` are not exposed as language primitives (§10.6.5).
3. **Sparse**: sparse values cannot enter the rssn term graph; user-defined formats wait for rssn to open `Format`; `gmres_with`/`bicgstab_with` do not accept exact elements; there is no sparse eigenvalue solver or SVD (§8.4.1, §13.9.5).
4. **SDE**: Brownian-bridge dense output and terminal events that stop in the middle of integration are not provided (`sde_at` is a linear chord, `sde_events` cuts after the fact); closures are `!Send` in the interpreter, so parallel ensembles fall back to sequential (§8.9.1).
5. **ODE**: multiple shooting / collocation for BVPs; `AutoSwitch` decides by step budget rather than an eigenvalue estimate (§8.9).
6. **Type system**: type-level moduli for `Zmod<n>`/`GF<p>`; rank polymorphism in user functions; symbolic exponents (§12.8.2).
7. **Plot**: only the HTML/SVG-inline renderer, with no standalone `.svg`/PNG/JPEG output (§8.12).
8. **REPL**: Ctrl-C cancelling the current evaluation (§9.10).

