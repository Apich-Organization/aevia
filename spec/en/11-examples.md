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
