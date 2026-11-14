# 02 — Syntax

This chapter defines the syntax of declarations, expressions, patterns and types, and explains
how the ambiguous places are parsed. The full EBNF is in [appendix A](appendix-a-grammar.md)
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
