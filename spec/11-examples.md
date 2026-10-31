# 11 — 完整示例

这些程序不是片段，而是可以放进 `.aev` 文件、用 `aevia run` 运行或
`aevia check` 检查的完整模块。每个例子后面解释它用到的语言特性，以及
类型/量纲是如何推导的。

## 11.1 自由落体：单位、量纲与 `Float`

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

输出：

```
fall time from 20 m = 2.019 s
```

推导：

- `9.80665` 是十进制字面量，类型 `Float[1]`；乘以 `m/s^2` 得到
  `Float[L/T^2]`。
- `20.0 m` 是 `Float[L]`。
- `2 * h`：`2 : Int` 提升为 `Float`，结果 `Float[L]`。
- `/ g`：`L / (L/T^2) = T^2`。
- `sqrt`：指数除以 2，`T^2 → T`。
- 返回值与 `Float[T]` 统一。

`println` 用 `show`，按显示单位 `m`、`s` 打印。`h` 的显示单位是 `m`，
`fall_time` 的结果没有显式显示单位，打印时选择 `s`。

## 11.2 抛体运动：记录、ODE 与结构化状态

```aevia
module Projectile

open Units.SI
import Math { sqrt, sin, cos }

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
    let ts = linspace(0.0 s, 10.0 s, 1000)
    odeint(rhs, s0, ts)

fn main(): IO Unit = do
    let path = trajectory(50.0 m/s, 45 deg)
    let last = path[-1]
    println("range = {to(m, last.x)}, height = {to(m, last.y)}")
```

推导：

- `k = 0.02 / m`：`Float[1] / Unit[L] = Float[1/L]`。
- `k * speed * s.vx`：`1/L * L/T * L/T = L/T^2`，与 `Rate.vx` 一致。
- `-g - k*...`：两者都是 `Float[L/T^2]`。
- `State` 的每个字段 `Float[d]`，`Rate` 的对应字段 `Float[d/T]`；编译器
  计算 `RateOf<State> = Rate`，逐字段检查“除以 T”。
- `path[-1]` 是 `State`；`last.x` 是 `Float[L]`。

`odeint` 的类型：

```
odeint : ((Float[T], s) -> RateOf<s>) -> s -> Tensor<1, Float[T]> -> Tensor<1, s>
```

这里 `s = State`，`RateOf<State> = Rate`。如果 `rhs` 返回 `State`
（忘记除以 T），类型检查器会在 `RateOf` 约束处报错，而不是让数值积分
悄悄算错。

## 11.3 符号微积分：精确值与 `Result`

```aevia
module Symbolic

import Math { exp }

fn main(): IO Unit = do
    let x = symbol("x")

    let df = diff(fn t => t^3 - 3*t + 1)
    println("f'(x) = {df(x)}")
    println("f'(2) = {df(2)}")          # 精确 9

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

输出（节选）：

```
f'(x) = 3*x^2 - 3
f'(2) = 9
g'(x) = exp(x)
roots = {2, 3}
∫₀¹ t² dt = 1/3
exp(t) ≈ 1 + t + t^2/2 + t^3/6 + t^4/24 + t^5/120 + O(t^6)
```

要点：

- `diff` 接受函数，不接受表达式。它创建同量纲的符号，应用函数，求导，
  再代回。因此 `diff(g)` 对任意 `g` 都成立。
- `df(2)` 是精确的 `9`，类型 `Real[1]`；要浮点，写 `approx(df(2))`。
- `solve`/`defint` 返回 `Result`，因为可能无解/不可积；`diff` 是 total 的，
  失败时保持未化简表达式。
- `series` 的结果带 `O(t^6)`；量纲仍是 `Real[1]`。

## 11.4 欧姆定律：带量纲的线性代数

```aevia
module Circuits

open Units.SI
import LinearAlgebra { linsolve, det }

fn main(): IO Unit = do
    let R = [[1.0, 2.0], [3.0, 4.0]] * ohm
    let I = [0.1, 0.2] * A

    let V = R @ I
    println("V = {V}")                  # [0.5 V, 1.1 V]

    let P = V * I
    println("P = {sum(P)}")             # 0.27 W

    match linsolve(R, V) with
    | Ok(I2) -> println("I = {I2}")     # 回到 [0.1 A, 0.2 A]
    | Err(e) -> println("solve failed: {e}")

    # det 只接受无量纲矩阵：
    # let d = det(R)                    # E0410
    let d = det(R / ohm)
    println("det(R/Ω) = {d}")
```

推导：

- `[[...]]` 是 `Tensor<2, Float>`；乘以 `ohm : Unit[Resistance]` 得到
  `Tensor<2, Float[Resistance]>`。
- `R @ I`：元素量纲 `Resistance * Current = Voltage`；结果
  `Tensor<1, Float[Voltage]>`。
- `V * I`：`Voltage * Current = Power`；`sum` 保持量纲。
- `linsolve(R, V)`：`Voltage / Resistance = Current`。
- `det(R)` 被拒绝，因为 `det` 的量纲是 `d^n`，`n` 是运行时阶数；`R / ohm`
  是无量纲矩阵，`det` 合法。

这个例子展示了“单位是编译期尺度”的好处：矩阵元素带单位，矩阵乘法自动
得到电压，不需要在运行时给每个元素贴标签。

## 11.5 温度：仿射点与温差

```aevia
module Thermo

open Units.SI

fn main(): IO Unit = do
    let T1 = 25.0 °C                 # Point[Θ]
    let T2 = point(300.0, K)         # Point[Θ]
    let dT = T2 - T1                 # Float[Θ]
    let T3 = T1 + 10.0 K             # Point[Θ]

    println("T1 = {T1}")
    println("T2 = {to(°C, T2)}")
    println("T3 = {T3}")
    println("ΔT = {dT}")

    # 下面两行是类型错误：
    # let bad1 = T1 + T2             # E0405
    # let bad2 = 2.0 * T1            # E0405
```

输出：

```
T1 = 25 °C
T2 = 26.85 °C
T3 = 35 °C
ΔT = 1.85 K
```

要点：

- `25.0 °C` 是仿射单位字面量，类型 `Point[Θ]`，值是 298.15 K。
- `300.0 K` 是线性单位字面量，类型 `Float[Θ]`，是温差，不是绝对温度。
  绝对温度用 `point(300.0, K)`。
- `Point - Point` 得到温差；`Point + 温差` 得到点；`Point + Point` 和
  `k * Point` 是错误。
- `to(°C, T2)` 只改显示单位；`T2` 的值仍是 K。

## 11.6 数据分析：`Table` 与动态列

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
        | (Ok(t), Ok(v)) ->
            println("mean speed = {mean(v)}")
            println("std  speed = {std(v)}")
            let spec = label(line(t, v), "v(t)")
            plot(combine([spec]))
        | _ -> println("column type mismatch")
```

要点：

- `read_csv<ρ>` 是 `IO (Result<Table<ρ>, DataError>)`；`ρ` 是行类型，
  表头单位用于推断字段类型。`<-` 绑定结果，`match` 处理错误。
- `column(t, "time")` 在编译期用 `Table<ρ>` 的字段类型检查，运行时再做
  防御性校验；CSV 表头可以带单位（`time [s]`），没有时列被当作无量纲，
  显式 `ρ` 可以覆盖。
- `line` 接受带量纲的 `x`/`y` 张量，自动生成轴标签（`t / s`、
  `v / (m/s)`）；需要覆盖时用 `PlotOpts.xunit`/`yunit`。
- `combine([spec])` 构造 `PlotSpec`；`plot` 写 HTML/SVG。

## 11.7 仿真：类型化包装器

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

`Physics.Ising.run` 是 `Params -> Result<IsingResult, SimError>`；`Params` 的
字段有类型（`temperature : Float[1]`、`seed : Option<Int>`）。底层调用
rssn 的 `sim::scenario::run("ising", json)`；包装器负责 JSON 编解码和
量纲检查。

## 11.8 量纲多态：标准化与线性拟合

```aevia
module Regression

import Stats { mean, std }

fn standardize<d>(xs: Tensor<1, Real[d]>): Tensor<1, Real[1]> =
    let m = mean(xs)
    let s = std(xs)
    (xs - m) / s

fn covariance<d, e>(xs: Tensor<1, Real[d]>, ys: Tensor<1, Real[e]>): Real[d*e] =
    mean((xs - mean(xs)) * (ys - mean(ys)))

fn linear_fit<d, e>(xs: Tensor<1, Real[d]>, ys: Tensor<1, Real[e]>)
    : { slope: Real[e/d], intercept: Real[e] } =
    let mx = mean(xs)
    let my = mean(ys)
    let slope = sum((xs - mx) * (ys - my)) / sum((xs - mx)^2)
    { slope = slope, intercept = my - slope * mx }
```

推导：

- `standardize`：`(xs - m)` 是 `Real[d]`，`/ s` 是 `Real[d]/Real[d] = 1`。
- `covariance`：逐元素乘积的量纲是 `d*e`，`mean` 保持。
- `linear_fit`：`slope` 的量纲是 `(d*e)/(d^2) = e/d`；`slope * mx` 是 `e`；
  `my - ...` 是 `e`。返回记录的量纲字段正确。

这些函数对任何量纲都成立；运行时没有单态化，量纲在类型检查后擦除。

## 11.9 错误示例：编译器如何拒绝

```aevia
let bad1 = 3 m + 2 s
let bad2 = sin(3 m)
let bad3 = (3 m)^n              # n : Int
let bad4 = 25.0 °C * 2.0
let bad5 = 1 / (0 m)
```

诊断（节选）：

```
error[E0401]: dimension mismatch
 --> bad.aev:1:12
  |
1 | let bad1 = 3 m + 2 s
  |            ^^^^^^^^^ expected Real[L], found Real[T]
  = left:  L
  = right: T
help: both operands of `+` must have the same dimension

error[E0403]: transcendental function requires a dimensionless argument
 --> bad.aev:2:12
  |
2 | let bad2 = sin(3 m)
  |                ^^^ found Real[L]
help: divide by a unit first, e.g. `sin(3 m / m)`

error[E0404]: dimensionful base with a runtime exponent
 --> bad.aev:3:12
  |
3 | let bad3 = (3 m)^n
  |            ^^^^^^^ base has dimension L, exponent is Int
help: use a literal exponent, or `pow_dyn` for a runtime dimension

error[E0405]: cannot scale an affine point
 --> bad.aev:4:12
  |
4 | let bad4 = 25.0 °C * 2.0
  |            ^^^^^^^^^^^^^ Point[Θ] * Float[1]
help: scale the temperature difference instead

error[E0801]: division by zero
 --> bad.aev:5:12
  |
5 | let bad5 = 1 / (0 m)
  |            ^^^^^^^^^ exact division by zero
```

这些错误中，`E0401`–`E0405` 是编译期；`E0801` 是运行时错误，但
`1/(0 m)` 在常量折叠时就能发现，编译器 MAY 提前报出。错误信息的目标是：
**指出量纲差异，并给出一个可执行的修正**，而不是只打印类型名。

## 11.10 一个“全特性”程序

```aevia
module Full

open Units.SI
import Math { exp }
import LinearAlgebra { linsolve }
import Plot { line, combine, plot }

fn kinetic(m: Float[M], v: Float[L/T]): Float[Energy] =
    m * v^2 / 2

fn main(): IO Unit = do
    # 1. 单位与量纲
    let m = 2.0 kg
    let v = 3.0 m/s
    println("E = {kinetic(m, v)}")

    # 2. 符号推导
    let x = symbol("x")
    println("f' = {diff(fn t => exp(-t^2))(x)}")

    # 3. 线性代数
    let A = [[2.0, 1.0], [1.0, 3.0]]
    let b = [1.0, 2.0]
    match linsolve(A, b) with
    | Ok(x) -> println("x = {x}")
    | Err(e) -> println("solve failed: {e}")

    # 4. 数据与绘图
    let ts = linspace(0.0 s, 1.0 s, 100)
    let vs = map(fn t => 3.0 m/s * exp(-value_in(s, t) / 0.5), ts)
    let spec = label(line(ts, vs), "v(t)")
    plot(combine([spec]))
```

这个程序把单位、符号、线性代数、张量、数据、绘图串在一起，展示了 Aevia
的目标：**一门语言覆盖从公式到图表的整个工作流**，而不是在多个库之间
搬运字符串。

下一章解释这些设计选择的理由、替代方案和已知限制。
