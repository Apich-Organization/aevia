# 04 — 量纲

这一章是 Aevia 的核心。第 03 章把 `Real[d]` 中的 `d` 当成一个类型参数；
本章定义 `d` 是什么、如何相等、如何求解、如何推断，以及静态系统表达不了时
怎么办。

## 4.1 基量纲

国际单位制的七个基量纲，加上用户可扩展的基量纲：

| 符号 | 别名 | 含义 |
|---|---|---|
| `L` | `Length` | 长度 |
| `M` | `Mass` | 质量 |
| `T` | `Time` | 时间 |
| `I` | `Current` | 电流 |
| `Θ` | `Temperature` | 热力学温度 |
| `N` | `Amount` | 物质的量 |
| `J` | `Luminosity` | 发光强度 |

- 基量纲是**生成元**，不是类型构造器；`L` 本身是一个量纲表达式。
- 用户可以用 `dimension Currency` 声明新的基量纲；它加入全局基集合。
- 基量纲名大小写敏感；单字母符号是规范名，单词别名用于可读性。
- `1` 表示无量纲（空乘积）。

### 4.1.1 派生别名

别名是透明的，不产生新类型：

```
dimension Area          = L^2
dimension Volume        = L^3
dimension Velocity      = L/T
dimension Acceleration  = L/T^2
dimension Frequency     = 1/T
dimension Force         = M*L/T^2
dimension Pressure      = M/(L*T^2)
dimension Energy        = M*L^2/T^2
dimension Power         = M*L^2/T^3
dimension Momentum      = M*L/T
dimension Density       = M/L^3
dimension Charge        = I*T
dimension Voltage       = M*L^2/(I*T^3)
dimension Resistance    = M*L^2/(I^2*T^3)
dimension Capacitance   = I^2*T^4/(M*L^2)
dimension Inductance    = M*L^2/(I^2*T^2)
dimension MagneticFlux  = M*L^2/(I*T^2)
dimension MagneticFluxDensity = M/(I*T^2)
dimension Entropy       = M*L^2/(T^2*Θ)
dimension MolarMass     = M/N
dimension Conductance   = 1/Resistance
dimension Angle         = 1
dimension SolidAngle    = 1
```

`Angle = 1` 是刻意的：SI 中弧度是无量纲的，`sin` 接受无量纲参数。`deg`、`rad`
是带尺度（`π/180`、`1`）的无量纲单位，用于显示和输入（§5.5）。如果将来需要
“角度类型”来防止 `sin(1.0)` 与 `sin(1 rad)` 混淆，可以引入一个 `Angle` 基
量纲，但当前规范不做；那会破坏 `exp(i*θ)` 等大量公式。

别名只影响打印：`Real[M*L^2/T^2]` 和 `Real[Energy]` 是同一个类型。打印时
优先选择最短的已知别名（`Energy` 优于 `M*L^2/T^2`），除非用户显式指定。

## 4.2 量纲表达式

```
δ ::= 1 | b | d | δ * δ | δ / δ | δ ^ q
```

- `b` 是基量纲（`L`、`M`、…、用户声明的 `Currency`）。
- `d` 是量纲变量（kind `Dim`）。
- `q` 是有理数字面量：`2`、`-1`、`1/2`、`-3/4`。

**静态量纲的指数只能是编译期有理数。** 这一点必须与 rssn 区分：rssn 的
`graph::dim` 支持符号指数（`L^n`、`L^(a+b)`），因为它在项图上做推断；Aevia
的静态类型系统不支持，因为 `n` 是运行时值时，类型层无法表示 `d^n`。这类
情况走 `Dyn`（§4.9）。

### 4.2.1 规范形

一个量纲表达式规范化为一个有限映射：

```
δ̂ : (Base ∪ DimVar) -> Q
```

只保留指数非零的生成元。例如：

```
L * T^-1        => { L: 1, T: -1 }
L / L           => {}
(L^2)^(1/2)     => { L: 1 }
L^2 / L         => { L: 1 }
```

规范化规则：

1. 乘法：指数相加。
2. 除法：指数相减。
3. 幂：指数相乘。
4. 去掉指数为 0 的项。
5. 按固定顺序排序（SI 顺序，然后用户基，然后变量名字典序）。

两个量纲相等当且仅当规范形相等。`L/T` 与 `L*T^-1` 是同一个量纲；`L^2` 与
`L^(4/2)` 是同一个量纲。

### 4.2.2 有理数指数

指数用任意精度有理数（`BigRational`）表示，至少支持 `i64` 范围的分子/分母。
常见指数 `1/2`、`1/3`、`-1`、`2` 必须精确。`sqrt` 产生 `1/2`，`cbrt` 产生
`1/3`。不存在浮点指数：`L^0.5` 在类型层被解释为 `L^(1/2)`（字面量 0.5 是
精确的 1/2），但 `L^x`（变量）不合法。

### 4.2.3 量纲变量

量纲变量 `d` 表示“某个未知量纲”，取值范围是整个自由阿贝尔群。它可以出现
在指数上：

```
Real[d]        未知量纲
Real[d^2]      d 的平方
Real[L/d]      L 除以 d
Real[d^(1/2)]  d 的平方根
```

量纲变量在 `let` 处泛化，除非它是弱变量（§3.3.3）。例如：

```aevia
fn square<d>(x: Real[d]): Real[d^2] = x^2
# square : ∀d. Real[d] -> Real[d^2]
```

## 4.3 量纲统一

量纲统一是阿贝尔群上的方程求解。给定 `δ1` 和 `δ2`，求替换 `σ` 使得
`σ(δ1) = σ(δ2)`。

### 4.3.1 算法

```
unify(δ1, δ2):
    q = normalize(δ1 / δ2)
    loop:
        if q == 1: return success
        pick a variable v with exponent r != 0 in q
        if no such variable:
            return mismatch(q)          # 只剩基量纲，指数不全为 0
        # 解 v^r = rest^-1
        rest = q / v^r
        if v occurs in rest with exponent s != 0:
            # v 出现在自己的解里：v^r = v^-s * ... => v^(r+s) = ...
            # 合并后重试；若 r+s == 0 则无解（除非 rest == 1）
            q = normalize(q / v^(r+s) * ...)   # 见下
            continue
        σ[v] = rest^(-1/r)
        apply σ to q
```

更简单的等价实现：把 `q` 写成指数向量，选一个变量 `v`，用它的指数 `r` 把
其他项消去：

```
σ[v] = (q / v^r)^(-1/r)
```

然后对 `q` 应用 `σ`，重复。由于每次消去一个变量，终止。若没有变量且 `q ≠ 1`，
报量纲不匹配。若 `v` 在 `rest` 中出现（occurs check），说明方程形如
`v^r = v^s * c`；合并为 `v^(r-s) = c`，若 `r ≠ s` 可解，否则要求 `c = 1`。
实现 MUST 做 occurs check，不能构造无限递归的替换。

### 4.3.2 例子

**例 1：简单匹配**

```
unify(L, T)：
q = L/T = { L:1, T:-1 }
没有变量，q ≠ 1 => mismatch
```

**例 2：解出变量**

```
unify(d, L/T)：
q = d * T / L = { d:1, L:-1, T:1 }
选 d，r=1，rest = T/L
σ[d] = (T/L)^(-1) = L/T
成功
```

**例 3：变量被约束为无量纲**

```
unify(d^2, d^3)：
q = d^-1
选 d，r=-1，rest = 1
σ[d] = 1^(1) = 1
成功，d = 1
```

这解释了为什么 `fn f<d>(x: Real[d], y: Real[d^2]) = x^2 + y` 可以泛化，
而 `fn g<d>(x: Real[d], y: Real[d^3]) = x^2 + y` 会把 `d` 特化为 `1`：
后者只对无量纲输入成立。

**例 4：多个变量**

```
unify(d*e, L)：
q = d*e/L
选 d，σ[d] = L/e
成功，e 仍自由
```

最一般统一子（MGU）不唯一，但都等价；实现按变量名排序以保证可复现。

### 4.3.3 失败与诊断

- 只剩基量纲且 `q ≠ 1`：量纲不匹配 `E0401`。诊断要列出两边规范形和
  差异（例如“左边多出 `T^-1`”）。
- occurs check 失败：`E0409`（通常意味着类型级递归，不应出现在用户代码）。
- 多个变量导致非唯一解：选择字典序最小的变量，结果是 MGU；不需要报错。
- 变量被解成含自身的表达式：occurs check。

### 4.3.4 与类型统一的集成

量纲统一发生在类型统一遇到 `Scalar<ρ1,d1>` 与 `Scalar<ρ2,d2>` 时：

```
unify(Scalar<ρ1,d1>, Scalar<ρ2,d2>) =
    unify(ρ1, ρ2) ∧ unify(d1, d2)
```

`ρ` 的统一是常规的一阶统一（`Int`、`Real`、`Float`、`Complex` 或变量）。
量纲变量和类型变量分开存储，但共享替换/层级。

## 4.4 量纲多态

### 4.4.1 泛化

在 `let` 和顶层声明处，自由量纲变量（不在环境中出现）被全称量化：

```aevia
fn mean<d>(xs: Tensor<1, Real[d]>): Real[d] =
    sum(xs) / length(xs)
# mean : ∀d. Tensor<1, Real[d]> -> Real[d]
```

量纲变量可以出现在类型的任意位置，包括函数参数、返回值和嵌套类型：

```aevia
fn map_quantity<d, e>(f: Real[d] -> Real[e], xs: Tensor<1, Real[d]>)
    : Tensor<1, Real[e]> = map(f, xs)
```

### 4.4.2 弱变量

`symbol`、`symfun` 和空张量引入弱变量，不泛化：

```aevia
let x = symbol("x")       # x : Real[d]，d 弱
let f = fn (t) => x * t   # d 与 t 的量纲统一
```

如果 `x` 在声明结束时没有约束，默认 `1` 并警告。弱变量保证“同一个符号在
所有使用处量纲一致”，这是符号计算正确性的基础。

### 4.4.3 实例中的量纲多态

```
instance Show for Real[d] where ...
```

`d` 隐式全称量化；这个实例覆盖所有量纲。不能同时定义 `Show for Real[L]`，
否则重叠（`E0311`）。

### 4.4.4 量纲多态与值多态的区别

量纲参数在运行时被擦除；`mean<L>` 和 `mean<T>` 是同一份代码。量纲只影响
类型检查和显示。因此：

- 不需要为每个量纲单态化；
- 不能对量纲做运行时分支（`if d == L` 不合法）；
- 需要运行时量纲时用 `Dyn`。

## 4.5 运算符的量纲规则

### 4.5.1 标量算术

设 `ρ = join(ρ1,ρ2)`：

| 表达式 | 结果量纲 | 约束 |
|---|---|---|
| `a + b`、`a - b` | `d` | `dim(a)=dim(b)=d` |
| `a * b` | `d1*d2` | — |
| `a / b` | `d1/d2` | — |
| `a ^ n`（`n` 字面量） | `d^n` | `n ∈ Q` |
| `a ^ n`（`n : Int` 运行时） | `d^n` 不可静态表达 | `d=1` 或 `Dyn` |
| `-a`、`abs(a)` | `d` | — |
| `sign(a)` | `1` | — |
| `min(a,b)`、`max(a,b)` | `d` | 同 `d` |
| `clamp(x,lo,hi)` | `d` | 三者同 `d` |
| `mod(a,b)`、`rem(a,b)` | `d` | 同 `d` |
| `hypot(a,b)` | `d` | 同 `d` |
| `atan2(y,x)` | `1` | 同 `d` |
| `lerp(a,b,t)` | `d` | `t : 1` |
| `sqrt(a)` | `d^(1/2)` | — |
| `cbrt(a)` | `d^(1/3)` | — |
| `exp/ln/log/sin/cos/...` | `1` | 参数 `d=1` |
| `floor/ceil/round/trunc` | `d` | — |

### 4.5.2 张量

| 表达式 | 结果 |
|---|---|
| `sum(a)`、`mean(a)`、`min(a)`、`max(a)` | 元素量纲 |
| `var(a)` | 元素量纲平方 |
| `std(a)` | 元素量纲 |
| `norm(a)` | 元素量纲 |
| `normalize(a)` | `1` |
| `dot(a,b)` | 两个元素量纲之积 |
| `matmul(A,B)`、`matvec(A,x)` | 元素量纲之积 |
| `outer(a,b)` | 元素量纲之积 |
| `det(A)` | 仅 `1`；带量纲用 `Dyn` |
| `inv(A)` | 元素量纲的倒数 |
| `linsolve(A,b)` | `dim(b)/dim(A)` |
| `eig(A)` | 特征值同元素量纲，特征向量 `1` |
| `svd(A)` | 奇异值同元素量纲，向量 `1` |
| `qr(A)` | Q 为 `1`，R 同元素量纲 |
| `lu(A)` | L 为 `1`，U 同元素量纲 |
| `trace(A)` | 元素量纲 |
| `fft(a)` | 元素量纲 |
| `ifft(a)` | 元素量纲 |

`prod` 的量纲是 `d^n`，`n` 是元素个数（运行时），所以只接受 `d=1` 或返回
`Dyn`。`det` 同理。`mean` 不改变量纲，因为除以的是无量纲计数。

### 4.5.3 微积分

| 表达式 | 类型 | 说明 |
|---|---|---|
| `diff(f)` | `(Real[d] -> Real[e]) -> (Real[d] -> Real[e/d])` | 导数除以自变量量纲 |
| `antiderivative(f)` | `(Real[d] -> Real[e]) -> (Real[d] -> Real[e*d])` | 积分乘 `dx` |
| `defint(f,a,b)` | `(Real[d] -> Real[e], Real[d], Real[d]) -> Result<Real[e*d], MathError>` | 定积分 |
| `limit(f,x)` | `(Real[d] -> Real[e], Real[d]) -> Result<Real[e], MathError>` | 极限同函数值 |
| `series(f,x,n)` | 同上，返回级数 | 同函数值 |
| `sum(f,a,b)` | `(Real[1] -> Real[d], Real[1], Real[1]) -> Real[d]` | 求和同元素 |
| `product(f,a,b)` | 仅 `d=1`，或 `Dyn<Real>` | 乘积的量纲是 `d^n` |
| `solve(f)` | `(Real[d] -> Real[e]) -> Result<Solutions<Real,d>, MathError>` | 解的量纲同自变量 |
| `nsolve(f,x0)` | `(Real[d] -> Real[e], Real[d]) -> Result<Real[d], MathError>` | 数值解 |
| `odeint(f,y0,ts)` | 见 §8.9 | 状态与时间 |

`diff` 的量纲规则值得强调：对 `f : Real[L] -> Real[T]` 求导，得到
`Real[T/L]`，即“每米多少秒”。这自动阻止了把导数当成原函数量纲使用。

### 4.5.4 统计

| 表达式 | 结果量纲 |
|---|---|
| `mean(xs)` | `d` |
| `median(xs)`、`quantile(xs,q)` | `d` |
| `var(xs)`、`cov(xs,ys)` | `d^2`、`d*e` |
| `std(xs)` | `d` |
| `corr(xs,ys)` | `1` |
| `skewness(xs)`、`kurtosis(xs)` | `1` |
| `histogram(xs)` | 计数 `1`，边界 `d` |

### 4.5.5 集合、结果与 IO

```
Solutions<Real,d>       元素量纲 d（符号解，不要求全序）
Option<Real[d]>    元素量纲 d
Result<Real[d],e>  成功值量纲 d
IO<Real[d]>        动作产生量纲 d 的值
```

容器不改变内部量纲。`map` 可以改变量纲（由函数决定），`filter` 不改变。

## 4.6 单位值的量纲

单位是 `Unit[d]` 类型的值：

```
m : Unit[L]
s : Unit[T]
km : Unit[L]
degC : Unit[Θ]        # 仿射，见 §5.6
```

单位运算：

```
u * v : Unit[d*e]
u / v : Unit[d/e]
u ^ n : Unit[d^n]     # n 字面量
```

标量与单位：

```
Scalar<ρ,d> * Unit[e] : Scalar<ρ,d*e>
Scalar<ρ,d> / Unit[e] : Scalar<ρ,d/e>
Scalar<ρ,d> / Unit[d] : Scalar<ρ,1>     # 得到以该单位计数的数值
Tensor<n, Scalar<ρ,d>> * Unit[e] : Tensor<n, Scalar<ρ,d*e>>
Tensor<n, Scalar<ρ,d>> / Unit[e] : Tensor<n, Scalar<ρ,d/e>>
```

`x / m` 得到“以米为单位的数值”，类型 `Real[1]`；`x * m` 得到量。这比
`to(m,x)` 更基本：`to` 只改显示单位，`/` 真正做量纲消去。`R / ohm` 对
张量逐元素进行，得到无量纲矩阵。

## 4.7 零与无穷

`0` **不是**量纲多态的。它是普通的整数字面量，默认 `Int`：

```
0        : Int
0.0      : Float[1]
0 m      : Real[L]        # 零长度
0.0 m    : Float[L]
```

规则：

- `0 + x`：类型错误 `E0406`，除非 `x : Int`。要加零长度，写 `0 m + x`。
- `0 * x`：`Int` 提升为 `x` 的表示，结果量纲是 `1 * dim(x) = dim(x)`，
  正确。
- `0 / x`：结果量纲 `1/dim(x)`（`Int` 提升为 `Real`）。
- `x / 0`：运行时错误 `E0801`（精确）或 IEEE Inf/NaN（浮点）。
- `(0 m)^2 : Real[L^2]`，因为 `0 m` 是 `Real[L]` 的零，不是多态零。

这条规则牺牲了 `0 + 3 m` 的便利，换来类型系统的简单与可靠。用户写
`0 m` 只多三个字符；编译器可以在错误信息里提示这一点。

`oo` 是 `Real[1]` 的无穷，不是量纲多态：

```
oo       : Real[1]
-oo      : Real[1]
nan      : Float[1]
```

`oo m` 是 `Real[L]`；`oo - oo` 是 `nan`；`oo * 0` 是 `nan`。这些是数值
语义，不影响类型。要写“带量纲的无穷”，写 `oo m` 或 `oo * u`。

## 4.8 量纲推断算法

实现分三步：

1. **约束生成**：遍历 AST，为每个子表达式生成类型/量纲约束。字面量、变量、
   运算符、函数调用、记录、张量、模式匹配都产生约束。
2. **统一**：用并查集/替换求解类型变量、量纲变量、行变量和类约束。量纲
   统一使用 §4.3 的算法。
3. **默认化与泛化**：在 `let` 边界泛化自由变量；在声明结束默认化剩余变量；
   检查类约束；导出接口。

### 4.8.1 约束形式

```
τ1 ~ τ2                 类型相等
δ1 ~ δ2                 量纲相等
ρ1 ~ ρ2                 行相等
C τ                     类约束
n ~ Succ<m>             秩约束（仅索引）
```

量纲约束可以直接化为 `δ1/δ2 ~ 1`，交给统一器。

### 4.8.2 一个完整推导

```aevia
fn fall_time(h: Float[L]): Float[T] =
    sqrt(2 * h / g)
```

1. `h : Float[L]`。
2. `g : Float[L/T^2]`（来自顶层 `let g = 9.80665 m/s^2`）。
3. `2 : Int`，提升为 `Float[1]`（与 `h` 相乘需要）。
4. `2 * h : Float[L]`。
5. `2 * h / g : Float[L / (L/T^2)] = Float[T^2]`。
6. `sqrt` 把指数除以 2：`Float[T]`。
7. 与返回类型 `Float[T]` 统一，成功。

如果写成 `sqrt(2 * h * g)`，第 5 步得到 `Float[L^2/T^2]`，第 6 步
`Float[L/T]`，与 `Float[T]` 不匹配，报 `E0401`，并提示“检查 `g` 是乘还是除”。

### 4.8.3 错误信息

量纲错误必须包含：

- 期望量纲与实际量纲的规范形；
- 差异（多出/缺少哪些基量纲）；
- 表达式的最小子树；
- 一个可能的修正（如果能从上下文推断）。

例如：

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

### 4.9.1 为什么需要

静态量纲系统不能表达：

1. `x^n`，`n : Int` 运行时，`x` 带量纲；
2. `det(A)`，`A` 的元素量纲 `d`，阶数 `n` 运行时；
3. `product(f, a, b)`，元素量纲 `d`，项数运行时；
4. 从 CSV/JSON 读入的量，量纲在运行时才知道；
5. 用户输入的公式，量纲在解析后才知道。

这些不是错误，而是“量纲是数据”的场景。`Dyn` 把它们收进一个显式的、
运行时检查的类型。

### 4.9.2 表示

```
Dyn            : Rep -> Type
DimValue       : Type        # = rssn_dim::DimTerm（基量纲 + 有理指数）
dyn            : Scalar<ρ,d> -> Dyn<ρ>          # d 必须是具体量纲
check          : Dyn<ρ> -> Result<Scalar<ρ,d>, DimError>   # d 必须是具体量纲
dyn_with       : (Scalar<ρ,d>, DimValue) -> Dyn<ρ>
check_with     : (DimValue, Dyn<ρ>) -> Result<Scalar<ρ,1>, DimError>
dim_of         : Dyn<ρ> -> DimValue
value_of       : Dyn<ρ> -> Scalar<ρ,1>                     # 以相干 SI 单位的数值
```

`Dyn<ρ>` 内部是 `(Scalar<ρ,1> 值, DimValue 量纲, 显示单位)`。它不是
`Scalar`，不能隐式转换为 `Scalar<ρ,d>`；必须 `check`。`DimValue` 是运行时
类型，与 kind `Dim` 不同名，避免混淆。

### 4.9.3 运算

```
(+) : Dyn<ρ> × Dyn<ρ> -> Dyn<ρ>        # 运行时要求量纲相等
(-) : Dyn<ρ> × Dyn<ρ> -> Dyn<ρ>
(*) : Dyn<ρ> × Dyn<ρ> -> Dyn<ρ>        # 量纲相加
(/) : Dyn<ρ> × Dyn<ρ> -> Dyn<ρ>        # 量纲相减
(^) : Dyn<ρ> × Int -> Dyn<ρ>           # 量纲指数相乘
sqrt : Dyn<ρ> -> Dyn<ρ>                # 指数除以 2
```

`Dyn` 的算术在量纲不匹配时 `panic`（`E0412`），因为 `Dyn` 已经是逃逸层；
需要可恢复错误时用 `try_add`、`try_mul` 等返回 `Result` 的变体。文档 MUST
说明这一点。

### 4.9.4 与静态层的边界

- `dyn(x)` 是**类型指导原语**：`x : Scalar<ρ,d>` 中的 `d` 必须是具体量纲
  （不含自由量纲变量）；编译器把 `d` 物化成 `DimValue`，附加到 `Dyn<ρ>`。
  类型擦除不影响这一步，因为量纲在调用点已知。
- `check<d>(x)` 同样要求 `d` 具体；编译器把 `d` 物化后与 `x` 的
  `DimValue` 比较，成功返回 `Scalar<ρ,d>`，失败返回 `DimError`。
- 泛型代码里的 `d` 是变量，不能直接 `dyn`/`check`；用
  `dyn_with(x, dim)`/`check_with(dim, x)` 显式传入运行时量纲。`dim` 由
  调用者（例如 CSV schema、用户输入）提供。
- `Dyn<ρ>` 不能用于需要 `Scalar<ρ,d>` 的算术；先 `check`。
- `Dyn<ρ>` 可以打印、比较（同量纲）、存入容器。
- `Dyn<ρ>` 的性能低于 `Real`/`Float`；不要在主循环中使用。

### 4.9.5 例子

```aevia
# 运行时指数
fn scale_law(x: Real[L], n: Int): Dyn<Real> =
    pow_dyn(dyn(x), n)

# 带量纲的行列式
fn det_unit(A: Tensor<2, Real[d]>, u: Unit[d]): Dyn<Real> =
    let n = shape(A)[0]
    let a = A / u                    # Tensor<[2,2], Real[1]>
    pow_dyn(dyn(det(a)), n) * unit_pow_dyn(u, n)
```

第二个例子展示了一个重要模式：先把单位因子提出来，做无量纲计算，再用
`Dyn` 把量纲装回去。这比让 `det` 返回一个不可知的静态量纲更诚实。

`unit_pow_dyn(u, n)` 的类型是 `Dyn<Real>`，表示“单位 `u` 的 `n` 次幂”；
`n` 是运行时整数。`pow_dyn(x, n)` 接受 `Dyn<ρ>`，返回 `Dyn<ρ>`；对
`Scalar<ρ,d>` 直接调用时先用 `dyn` 包装。

## 4.10 与 rssn 量纲层的关系

rssn 的 `graph::dim` 是一个**项级**量纲推断层：

- 每个标量项有一个量纲；
- 每个算子有 `DimRule`；
- 量纲是 e-class 的属性，用抽象解释 + 统一计算；
- 支持有理/符号指数、量纲变量、`Advisory`/`Strict` 模式；
- 单位表达式由 `units` 规则集解析。

Aevia 的静态层与它共享同一套基量纲和指数代数，但**默认不把量纲传给
rssn**：静态路径的项图是无量纲的，rssn 的项级层只在 FFI/导出（带
`quantity`）或可选诊断时使用。两者目的不同：

| | Aevia 静态层 | rssn 项级层 |
|---|---|---|
| 时机 | 编译期 | 运行时/化简时 |
| 对象 | 源程序的类型 | 项图的 e-class |
| 指数 | 仅有理常量 | 有理 + 符号 |
| 失败 | 拒绝程序 | 诊断/严格模式报错 |
| 作用 | 防止坏程序 | 保证化简正确、报告答案量纲 |

实现 SHOULD 把量纲代数抽成共享 crate（`aevia-dim`），rssn 的 `Dim` 与
Aevia 的 `Dim` 使用相同的基名和规范形。两者不一致会导致：

- 静态检查通过但 rssn 报量纲冲突；
- 单位文本在两边解析出不同量纲；
- 打印的别名不同。

桥接层 MUST 在启动时做一次一致性检查：对标准基量纲、常见派生量纲和单位表
逐项比较。发现不一致时，以 Aevia 的表为准，并记录警告。

## 4.11 量纲错误与警告

| 代码 | 情况 |
|---|---|
| `E0401` | 量纲不匹配（加、减、比较、函数参数） |
| `E0402` | 量纲作为指数（`2^(3 m)`） |
| `E0403` | 超越函数参数带量纲（`sin(3 m)`） |
| `E0404` | 带量纲底数的运行时指数（`x^n`，`n : Int`） |
| `E0405` | 仿射点运算非法（`Point + Point`、`k * Point`） |
| `E0406` | 裸 `0` 用于需要量纲的上下文；写 `0 m` |
| `E0407` | 未知量纲/别名 |
| `E0408` | 量纲约束无解（多变量冲突） |
| `E0409` | occurs check 失败 |
| `E0410` | `det`/`product` 的带量纲形式需要 `Dyn<ρ>` |
| `E0411` | 张量元素量纲不一致 |
| `E0412` | `Dyn` 运行时量纲检查失败（runtime） |

## 4.12 设计取舍

### 4.12.1 为什么量纲是类型，而不是运行时标签

运行时标签（Pint、Unitful）能捕获错误，但错误发生在测试或生产；类型系统
在编译期捕获，且能指导推断（`mean` 的量纲由输入决定）。代价是类型系统
更复杂，且必须处理静态不可表达的情况（`Dyn`）。

### 4.12.2 为什么单位不进类型

如果 `m` 和 `ft` 是不同类型，`3 m + 4 ft` 需要类型级换算，`Real[L]` 会变成
`Real[L, m]`，函数签名要量化单位，类型相等变得复杂。把单位降为编译期
尺度，类型只保留量纲，换来：

- 类型相等 = 量纲相等；
- 单位表可扩展；
- 函数签名简短；
- 换算在编译期折叠。

代价是无法在类型层区分同一量纲的不同单位。这几乎不是科学计算的错误来源；
真正的错误是 `m + s`。

### 4.12.3 为什么不支持符号指数

符号指数 `L^n` 需要类型层携带 `n` 的表达式，统一变成多项式方程，可能不可
判定。rssn 在项级支持它，因为那里可以返回“符号约束”而不是拒绝。Aevia
选择静态保守 + `Dyn` 逃逸：静态路径可判定、可解释；动态路径显式、可检查。

### 4.12.4 为什么 `Angle = 1`

把角度作为基量纲会让 `sin` 只接受 `Angle`，但物理公式中大量出现
`sin(ω t)`、`exp(i θ)`，其中 `ω t` 和 `θ` 是无量纲的。把角度设为无量纲、
用单位 `rad`/`deg` 提供输入输出，是 SI 的选择，也是实践中最不碍事的。

下一章把 `Unit[d]` 从类型变成可用的单位表：前缀、换算、仿射点和常数。
