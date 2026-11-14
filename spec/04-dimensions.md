# 04 — 量纲

本章是 Aevia 的核心。第 03 章把 `Real[d]` 中的 `d` 当作类型参数；本章定义 `d` 是什么、
如何相等、如何求解和推断，以及静态系统表达不了时怎么办。

## 4.1 基量纲

| 规范名 | 源码/显示别名 | 含义 |
|---|---|---|
| `Length` | `L` | 长度 |
| `Mass` | `M` | 质量 |
| `Time` | `T` | 时间 |
| `Current` | `I` | 电流 |
| `Temperature` | `Θ` | 热力学温度 |
| `Amount` | `Amt` | 物质的量 |
| `Luminosity` | `Lum` | 发光强度 |

- 基量纲是**生成元**，不是类型构造器；`1` 表示无量纲（空乘积）。名字大小写敏感，**全名
  是规范名**：FFI、manifest 和 `rssn::dim` 的边界一律用全名，单字母只是源码与显示别名。
- `dimension Currency` 声明新的基量纲。其身份由定义模块的 canonical package/source owner、
  模块路径和声明名共同确定，不是全局裸名：两个包各自声明的 `Currency` 是不同生成元；同一
  声明经不同 import 别名引入仍相等。`A.Currency` 之类只是显示/查找提示，不参与相等、
  排序、hash 或 ADT schema；导入接口传递已解析的量纲。泛型 `d: Dim` 的实例化只替换量纲
  变量，保留全部名义生成元及其指数。
- 源码基量纲与宿主（rssn）量纲分开：宿主的普通 `Base("Currency")` 不会自动等同于任一源码
  `dimension Currency`。名义生成元过桥接时使用带版本、含完整 owner 与声明名的规范编码
  （保留前缀下的普通宿主名会转义以免碰撞），载体仍是 RSSN 的 `Base` 字符串，SI 的签名
  hash 与二进制布局不变。生成的外观模块不能按显示名重声明外部 owner：这类算子使用动态
  路径，外部名义量纲的单位和常量标注为“不能生成”并跳过，使用者须从其定义模块导入。
- `private dimension` 只在定义模块可见；公共别名、函数签名和类型别名可以引用它，接口保存
  其已解析语义，使用者无需也无法解析私有名字。导入不构成传递导出。

### 4.1.1 派生别名

别名是透明的，不产生新类型：

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

`Angle = 1` 是刻意的：SI 中弧度无量纲，`sin` 接受无量纲参数，`deg`/`rad` 是带尺度
（`π/180`、`1`）的无量纲单位（§5.5）。引入 `Angle` 基量纲会破坏 `exp(i*θ)`、`sin(ω*t)`
等大量公式。别名只影响打印：`Real[M*L^2/T^2]` 与 `Real[Energy]` 是同一类型，打印时优先
选择最短的已知别名。

## 4.2 量纲表达式

```
δ ::= 1 | b | d | δ * δ | δ / δ | δ ^ q
```

`b` 是基量纲，`d` 是量纲变量（kind `Dim`），`q` 是有理数字面量（`2`、`-1`、`(1/2)`、
`(-3/4)`；小数 `0.5` 不合法）。**静态量纲的指数只能是编译期有理数**：`n` 是运行时值时类型
层无法表示 `d^n`，这类情况走 `Dyn`（§4.9）。rssn 的 `graph::dim` 在项图上支持单个符号
指数，Aevia 的静态类型不支持。

### 4.2.1 规范形

量纲规范化为有限映射 `δ̂ : (Base ∪ DimVar) → Q`，只保留指数非零的生成元：乘法指数相加，
除法相减，幂相乘，去掉零项，按固定顺序排序（SI 顺序、用户基、变量名字典序）。例如
`L * T^-1 ⇒ {L:1, T:-1}`，`L / L ⇒ {}`，`(L^2)^(1/2) ⇒ {L:1}`。**两个量纲相等当且仅当规范形
相等**：`L/T` 与 `L*T^-1`、`L^2` 与 `L^(4/2)` 是同一个量纲。

指数用任意精度有理数表示，至少支持 `i64` 范围的分子分母；`1/2`、`1/3`、`-1`、`2` 必须精确，
`sqrt` 产生 `1/2`，`cbrt` 产生 `1/3`。

### 4.2.2 量纲变量

量纲变量 `d` 表示“某个未知量纲”，取值范围是整个自由阿贝尔群，可出现在指数位置：
`Real[d]`、`Real[d^2]`、`Real[L/d]`、`Real[d^(1/2)]`。

```aevia
fn square<d>(x: Real[d]): Real[d^2] = x^2
# square : ∀d. Real[d] -> Real[d^2]
```

## 4.3 量纲统一

量纲统一是阿贝尔群上的方程求解：给定 `δ1`、`δ2`，求替换 `σ` 使 `σ(δ1) = σ(δ2)`。

### 4.3.1 算法

令 `q = normalize(δ1 / δ2)`，写成指数向量：

1. `q = 1` 则成功。
2. 否则选一个指数 `r ≠ 0` 的量纲变量 `v`（按名字排序，保证可复现），令
   `σ[v] = (q / v^r)^(-1/r)`，把 `σ` 应用到 `q` 并回到 1。
3. 没有变量而 `q ≠ 1`：量纲不匹配（`E0401`）。

每步消去一个变量，必定终止。因为解的表达式不含 `v` 自身，阿贝尔群上不存在 occurs check 失败
（`E0409` 保留，当前统一器不会产生）。最一般统一子不唯一但彼此等价。

**例**

```
unify(L, T)：     q = L/T，无变量，q ≠ 1 ⇒ 不匹配
unify(d, L/T)：   q = d·T/L，选 d：σ[d] = L/T
unify(d^2, d^3)： q = d^-1，选 d：σ[d] = 1  ⇒ d = 1
unify(d*e, L)：   选 d：σ[d] = L/e，e 仍自由
```

第三例说明：未声明量纲参数的函数 `fn f(x: Real[d], y: Real[d^3]) = x^2 + y` 会把 `d` 特化为
`1`；声明了 `<d>` 的同样函数会被 `E0318` 拒绝（§4.4.1）。

### 4.3.2 诊断与集成

- 量纲不匹配 `E0401` 必须列出两边规范形和差异（例如“左边多出 `T^-1`”）。
- 量纲统一发生在类型统一遇到 `Scalar<ρ1,d1>` 与 `Scalar<ρ2,d2>` 时：
  `unify(Scalar<ρ1,d1>, Scalar<ρ2,d2>) = unify(ρ1, ρ2) ∧ unify(d1, d2)`。`ρ` 的统一是常规
  一阶统一；量纲变量与类型变量分开存储，共享替换与层级。

## 4.4 量纲多态

### 4.4.1 泛化与刚性参数

在 `let` 和顶层声明处，不在环境中出现的自由量纲变量被全称量化，可出现在类型的任何位置：

```aevia
fn mean<d>(xs: Tensor<1, Real[d]>): Real[d] = sum(xs) / length(xs)

fn map_quantity<d, e>(f: Real[d] -> Real[e], xs: Tensor<1, Real[d]>)
    : Tensor<1, Real[e]> = map(f, xs)
```

**声明的量纲参数是刚性的。** `fn f<d>` 声明 `f` 对任意 `d` 成立，函数体不能替调用者选定它。
检查完函数体后，每个声明的量纲参数必须仍只由自由变量组成（不含 `L`、`T` 等基量纲），并且
各声明参数的指数向量线性无关（仍可被独立选择）。违反时报 `E0318`：

```aevia
fn h<d>(x: Real[d]): Real[d] = x + 1 m
# error[E0318]: `h` is less generic than its declared type parameters
#   = the body requires `d = L`

fn g<d, e>(x: Real[d], y: Real[e]): Real[d] = x + y
#   = the body determines `e` from `d`
```

统一器可把一个变量写成其他自由变量的积（§11.8 的 `linear_fit` 中 `d` 解为
`d26*d28*d30/(d25*d29)`），这仍是泛型的：判定看独立性，不是“是否仍是裸变量”。声明的
**类型**参数可被细化为表示多态的数值类型（`fn number<a>(): a = 1` 对四种表示都成立），只有
函数体把它固定为不含变量的类型时才报 `E0318`；声明的 `Nat` 参数按 §3.5.1 解方程，不受
此限。

### 4.4.2 弱变量

`symbol`、`symfun` 和空张量引入的弱变量不泛化（§3.3.3）：

```aevia
let x = symbol("x")       # x : Real[d]，d 弱
let f = fn (t) => x * t   # d 与 t 的量纲统一
```

`x` 在声明结束时仍无约束则默认 `1` 并警告 `W0301`。弱变量保证同一个符号在所有使用处量纲一致。

### 4.4.3 实例与擦除

`instance Show for Real[d]` 覆盖所有量纲；同时定义 `Show for Real[L]` 会重叠（`E0311`）。量纲
参数在运行时被擦除：`mean<L>` 与 `mean<T>` 是同一份代码，不需要单态化，也不能对量纲做运行时
分支；需要运行时量纲时用 `Dyn`。

## 4.5 运算符的量纲规则

### 4.5.1 标量

设 `ρ = join(ρ1,ρ2)`（§3.4）：

| 表达式 | 结果量纲 | 约束 |
|---|---|---|
| `a + b`、`a - b`、`min`、`max`、`mod`、`rem`、`hypot`、`clamp` | `d` | 操作数同 `d` |
| `a * b` / `a / b` | `d1*d2` / `d1/d2` | — |
| `a ^ n`（`n` 字面量） | `d^n` | `n ∈ Q` |
| `a ^ n`（`n : Int` 运行时） | 不可静态表达 | `d = 1` 或 `Dyn`（§3.4.3） |
| `-a`、`abs`、`floor`、`ceil`、`round`、`trunc` | `d` | — |
| `sign(a)` | `1` | — |
| `atan2(y,x)` | `1` | 同 `d` |
| `sqrt(a)`、`cbrt(a)` | `d^(1/2)`、`d^(1/3)` | — |
| `exp ln log sin cos …` | `1` | 参数 `d = 1` |

### 4.5.2 张量

张量归约与线性代数的量纲规则见 §3.5.5。`fft`/`ifft` 保持元素量纲；`prod`、`det` 只接受
无量纲元素，带量纲时用 `Dyn`（`E0410`）。`mean` 不改变量纲，因为除数是无量纲计数。

### 4.5.3 微积分

| 表达式 | 类型 |
|---|---|
| `diff(f)` | `(Real[d] -> Real[e]) -> (Real[d] -> Real[e/d])` |
| `antiderivative(f)` | `(Real[d] -> Real[e]) -> (Real[d] -> Real[e*d])` |
| `defint(f,a,b)` | `(Real[d] -> Real[e], Real[d], Real[d]) -> Result<Real[e*d], MathError>` |
| `limit(f,x)` | `(Real[d] -> Real[e], Real[d]) -> Result<Real[e], MathError>` |
| `sum(f,a,b)` | `(Real[1] -> Real[d], Real[1], Real[1]) -> Real[d]` |
| `product(f,a,b)` | 仅 `d = 1`，或 `Dyn<Real>`（量纲是 `d^n`） |
| `solve(f)` | `(Real[d] -> Real[e]) -> Result<Solutions<Real,d>, MathError>` |
| `nsolve(f,x0)` | `(Real[d] -> Real[e], Real[d]) -> Result<Real[d], MathError>` |
| `odeint(f,y0,ts)` | 见 §8.9 |

对 `f : Real[L] -> Real[T]` 求导得 `Real[T/L]`（“每米多少秒”），自动阻止把导数当原函数
量纲使用。

### 4.5.4 统计

`mean`、`median`、`quantile` 保持元素量纲 `d`；`var` 得 `d^2`；`std` 得 `d`；`cov(xs,ys)` 得
`d*e`；`corr`、`skewness`、`kurtosis` 无量纲；`histogram(xs, n)` 的计数无量纲、边界量纲 `d`。

### 4.5.5 容器

`Solutions<Real,d>`、`Option<Real[d]>`、`Result<Real[d],e>`、`IO (Real[d])` 都不改变内部量纲；
`map` 的结果量纲由函数决定，`filter` 不改变量纲。

## 4.6 单位值的量纲

单位是 `Unit[d]` 类型的值：`m : Unit[L]`，`km : Unit[L]`，`degC : Unit[Θ]`（仿射，§5.6）。

```
u * v : Unit[d*e]       u / v : Unit[d/e]       u ^ n : Unit[d^n]   # n 字面量
Scalar<ρ,d> * Unit[e] : Scalar<ρ,d*e>
Scalar<ρ,d> / Unit[e] : Scalar<ρ,d/e>
Scalar<ρ,d> / Unit[d] : Scalar<ρ,1>     # 以该单位计数的数值
```

张量同样逐元素适用。`x / m` 得到“以米计的数值”（`Real[1]`），`x * m` 得到量；这比 `to(m, x)`
更基本：`to` 只改显示单位，`/` 真正做量纲消去。

## 4.7 零与无穷

`0` **不是**量纲多态的，是普通整数字面量，默认 `Int`：

```
0      : Int           0.0    : Float[1]
0 m    : Real[L]       0.0 m  : Float[L]
```

- `0 + x` 是 `E0406`，除非 `x : Int`；要加零长度写 `0 m + x`。
- `0 * x` 把 `Int` 提升为 `x` 的表示，结果量纲 `dim(x)`；`0 / x` 的量纲是 `1/dim(x)`。
- `x / 0` 对精确值是运行时 `E0801`，对浮点是 IEEE Inf/NaN。`(0 m)^2 : Real[L^2]`。

这牺牲了 `0 + 3 m` 的便利，换来类型系统的简单可靠；错误信息会提示写 `0 m`。

`oo` 是 `Real[1]` 的无穷，不是量纲多态；`nan` 是 `Float[1]`。带量纲的无穷写 `oo * 1 m`。
`oo - oo` 与 `oo * 0` 是 `nan`，这些是数值语义，不影响类型。

## 4.8 量纲推断

实现分三步：**约束生成**（遍历 AST，字面量、变量、运算符、调用、记录、张量、模式都产生约束）；
**统一**（求解类型、量纲、行变量与类约束，量纲用 §4.3）；**默认化与泛化**（`let` 边界泛化，声明
结束默认化，检查类约束，导出接口）。约束形式：`τ1 ~ τ2`（类型）、`δ1 ~ δ2`（量纲，等价于
`δ1/δ2 ~ 1`）、`ρ1 ~ ρ2`（行）、`C τ`（类约束）。

### 4.8.1 一个完整推导

```aevia
fn fall_time(h: Float[L]): Float[T] =
    sqrt(2 * h / g)
```

`h : Float[L]`，`g : Float[L/T^2]`；`2 : Int` 提升为 `Float[1]`；`2 * h : Float[L]`；
`2 * h / g : Float[L / (L/T^2)] = Float[T^2]`；`sqrt` 把指数除以 2 得 `Float[T]`，与返回类型
统一，成功。写成 `sqrt(2 * h * g)` 则得 `Float[L/T]`，不匹配 `Float[T]`，报 `E0401`。

### 4.8.2 错误信息

量纲错误 MUST 包含期望与实际量纲的规范形、差异（多出或缺少哪些基量纲）、出错的最小子树，
并在能从上下文推断时给出修正：

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

## 4.9 动态量纲 `Dyn`

### 4.9.1 用途

静态量纲无法表达：`x^n`（`n : Int` 运行时，`x` 带量纲）、`det(A)`（阶数 `n` 运行时）、
`product(f, a, b)`（项数运行时）、CSV/JSON 读入的量和用户输入的公式（量纲是数据）。`Dyn`
把它们收进一个显式的、运行时检查的类型。

### 4.9.2 表示

```
Dyn        : Rep -> Type
DimValue   : Type        # 运行时量纲值：基量纲加有理指数
dyn        : Scalar<ρ,d> -> Dyn<ρ>                       # d 必须具体
check      : Dyn<ρ> -> Result<Scalar<ρ,d>, DimError>     # d 必须具体
dyn_with   : (Scalar<ρ,d>, DimValue) -> Dyn<ρ>
check_with : (DimValue, Dyn<ρ>) -> Result<Scalar<ρ,1>, DimError>
dim_of     : Dyn<ρ> -> DimValue
value_of   : Dyn<ρ> -> Scalar<ρ,1>                       # 相干 SI 单位下的数值
pow_dyn    : (Dyn<ρ>, Int) -> Dyn<ρ>
det_unit   : (Tensor<2, Scalar<ρ,d>>, Unit[d]) -> Dyn<ρ>
```

`Dyn<ρ>` 内部是（数值、`DimValue`、显示单位），不是 `Scalar`，不能隐式转换为
`Scalar<ρ,d>`，必须 `check`。`DimValue` 保存可无损还原的规范量纲文本：自定义宿主基、名义
owner 和变量都有显式编码，SI 文本保持既有形式；`dim_of` → `dyn_with`/`check_with` 不经显示名
恢复身份，诊断和数值后缀使用的可读别名不能当作跨模块量纲身份的证据。

### 4.9.3 运算

`+`、`-` 要求运行时量纲相等；`*`、`/` 让量纲指数相加、相减；`^ Int` 让指数相乘；`sqrt` 指数
除以 2。**量纲不匹配是运行时错误 `E0412`**，因为 `Dyn` 已是逃逸层，它终止求值，不是 `Result`。
需要可恢复的检查时，先用 `check`/`check_with`，它们返回 `Result`。

### 4.9.4 与静态层的边界

- `dyn(x)` 与 `check(x)` 是**类型指导原语**：`d` 必须是具体量纲（不含自由变量），编译器把它
  物化成 `DimValue`；`check` 的结果类型要由标注或后续使用确定，否则是 `E0304`。
- 泛型代码里的 `d` 是变量，不能直接 `dyn`/`check`，用 `dyn_with(x, dim)`/`check_with(dim, x)`
  显式传入运行时量纲（来自 CSV schema、用户输入等）。
- `Dyn<ρ>` 可以打印、比较（同量纲）、存入容器，但不能参与需要 `Scalar<ρ,d>` 的算术，性能
  也低于 `Real`/`Float`，不要用在主循环里。

### 4.9.5 例子

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

`det_unit` 体现一个模式：把单位因子提出来，做无量纲计算，再用 `Dyn` 把量纲装回去，这比让
`det` 返回一个不可知的静态量纲更诚实。

## 4.10 与 rssn 量纲层的关系

rssn 的 `graph::dim` 是**项级**量纲推断：每个标量项有量纲，每个算子有 `DimRule`，量纲是
e-class 属性，支持有理与符号指数、量纲变量和 `Advisory`/`Strict` 模式。Aevia 的静态层与它
共享同一套基量纲和指数代数，但**默认不把量纲传给 rssn**：静态路径的项图是无量纲的，项级层
只在 FFI/导出（带 `quantity`）或可选诊断时使用。

| | Aevia 静态层 | rssn 项级层 |
|---|---|---|
| 时机 | 编译期 | 运行时/化简时 |
| 对象 | 源程序的类型 | 项图的 e-class |
| 指数 | 仅有理常量 | 有理 + 符号 |
| 失败 | 拒绝程序 | 诊断/严格模式报错 |

两侧使用同一份量纲代数（`aevia-dim`）与相同的基名与规范形。桥接层 MUST 在启动时对标准基量纲、
常见派生量纲和单位表逐项做一致性检查，发现不一致时以 Aevia 的表为准并记录警告。rssn 的
`DimExpr::to_term` 能把 `Length^n` intern 成原子，所以 `Dyn` 逃逸路径不在桥接层报错；这不改变
语言决定：静态类型位置只接受有理指数，符号指数只能出现在 `Dyn` 的运行时量纲里。

## 4.11 量纲错误与警告

| 代码 | 情况 |
|---|---|
| `E0401` | 量纲不匹配（加、减、比较、函数参数） |
| `E0402` | 量纲作为指数（`2^(3 m)`） |
| `E0403` | 超越函数参数带量纲（`sin(3 m)`） |
| `E0404` | 带量纲底数的运行时指数（`x^n`，`n : Int`） |
| `E0405` | 仿射点运算非法（`Point + Point`） |
| `E0406` | 裸 `0` 用于需要量纲的上下文；写 `0 m` |
| `E0407` | 未知量纲或别名 |
| `E0408` | 保留（量纲约束无解） |
| `E0409` | 保留（occurs check，§4.3.1） |
| `E0410` | `det`/`prod`/`product` 的带量纲形式需要 `Dyn` |
| `E0411` | 张量运算的元素类型不兼容 |
| `E0412` | `Dyn` 运行时量纲检查失败（运行时错误） |
| `E0318` | 声明的量纲参数比函数体更泛（§4.4.1） |

## 4.12 设计取舍

- **量纲是类型，不是运行时标签。** 运行时标签（Pint、Unitful）把错误留到测试或生产；类型在
  编译期捕获，并能指导推断（`mean` 的量纲由输入决定）。代价是类型系统更复杂，且必须为
  静态不可表达的情况提供 `Dyn`。
- **单位不进类型。** 若 `m` 与 `ft` 是不同类型，`Real[L]` 会变成 `Real[L, m]`，签名要量化单位，
  类型相等变复杂。把单位降为编译期尺度，类型相等就是量纲相等，单位表可独立扩展，换算在
  编译期折叠；代价是类型层不区分同一量纲的不同单位——科学计算真正的错误是 `m + s`。
- **不支持符号指数。** `L^n` 需要类型层携带 `n` 的表达式，统一变成多项式方程，可能不可判定。
  rssn 在项级可以返回“符号约束”而不是拒绝；Aevia 选择静态保守加 `Dyn` 逃逸：静态路径可判定、
  可解释，动态路径显式、可检查。
- **`Angle = 1`。** 物理公式中 `sin(ω t)`、`exp(i θ)` 的参数本就无量纲；设角度为无量纲、用
  `rad`/`deg` 提供输入输出，是 SI 的选择，也最不碍事。

下一章把 `Unit[d]` 从类型变成可用的单位表：前缀、换算、仿射点和常数。
