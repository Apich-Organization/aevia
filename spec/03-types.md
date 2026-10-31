# 03 — 类型系统

Aevia 的类型系统是 **HM 推断 + 量纲代数 + 类型类 + 行多态**。它没有完整依赖
类型，也没有子类型；所有的“灵活”都来自参数多态、量纲多态和行多态。这一章定义
类型的形式结构、推断规则和默认值策略；量纲代数的细节在第 04 章。

## 3.1 kind

```
κ ::= Type | Dim | Nat | Row | Rep | Format | Shape
```

| kind | 含义 | 例子 |
|---|---|---|
| `Type` | 值类型 | `Int`、`Real[L]`、`Tensor<[3,4], Float>` |
| `Dim` | 量纲表达式 | `L`、`L/T`、`d` |
| `Nat` | 类型级自然数 | `0`、`1`、`2`、`n` |
| `Row` | 记录行 | `{ x: Real[L] \| r }` 中的 `r` |
| `Rep` | 数值表示 | `Int`、`Real`、`Float`、`Complex` |
| `Format` | 稀疏格式 | `Csr`、`Csc`、`Coo`、`Block` |
| `Shape` | 类型级形状 | `[3,4]`、`[n,m]`、`[?]` |

`Rep` 是一个封闭的 kind，用户不能扩展。它的四个成员对应四种标量表示：

```
Rep ::= Int | Real | Float | Complex
```

## 3.2 类型

### 3.2.1 标量：`Scalar<ρ, δ>`

所有数值标量的统一形式是：

```
Scalar : Rep -> Dim -> Type
```

表面语法是四个别名：

| 表面写法 | 内部形式 | 含义 |
|---|---|---|
| `Int` | `Scalar<Int, 1>` | 任意精度整数，无量纲 |
| `Real[d]` | `Scalar<Real, d>` | 精确实数或**形式符号**表达式 |
| `Float[d]` | `Scalar<Float, d>` | IEEE-754 binary64，**永不符号化** |
| `Complex[d]` | `Scalar<Complex, d>` | 复数（精确/符号或 `Complex64`） |

`Real` 不带方括号等价于 `Real[1]`；`Float`、`Complex` 同理。

`Int` **只能**是无量纲的。带量纲的精确整数是 `Real[L]` 中值为整数的那些。
这样 `Int` 可以安全地用于索引、计数和循环，而不会出现“3 米的整数”这种
需要额外规则的类型。

`Float` 严格是 f64：一个 `Float` 值只能是具体的 IEEE 数；含符号的浮点
表达式由 `Real` 承担。`Real + Float` 的结果是 `Real`：`Float` 被精确转换成
有理数，符号信息保留。要留在浮点路径，显式写 `to_float(x)` 或让所有操作数
都是 `Float`。这条规则让 `pi + 1.0` 保持精确，`x + 1.0` 也不会把符号项
伪装成“浮点数”；JIT/IEEE 语义因此是干净的。

### 3.2.2 其他类型构造器

```
()            : Type                         unit 值
Bool          : Type                        具体布尔
Prop          : Type                        符号命题（§3.8.6）
String        : Type
Tensor        : Nat -> Type -> Type         秩-only 张量
Matrix        : Nat -> Nat -> Type -> Type  = Tensor<[n,m], τ>
Sparse        : Format -> Shape -> Type -> Type  稀疏张量（§13.9.5）
Point         : Dim -> Type                  仿射点（Float 表示；可作张量元素）
Unit          : Dim -> Type                  单位值
Table         : Row -> Type                  行多态表（§13.9.3）
Dynamic       : Row                         未知 schema 的行标记
Device        : Type                         运行时设备（§13.9.4）
Set           : Type -> Type
Option        : Type -> Type
Result        : Type -> Type -> Type
IO            : Type -> Type
(->)          : Type -> Type -> Type         多参数函数是元组的糖
SymFun        : Dim -> Dim -> Type           符号函数（用于 dsolve）
Dyn           : Rep -> Type                  动态量纲标量（§4.9）
RateOf        : Type -> Type                  ODE 状态的时间导数类型（§8.9）
Solutions     : Rep -> Dim -> Type            符号解集合（§8.6）
```

常用辅助类型（在 Prelude 或对应库中定义）：

```
Ordering   ::= Less | Equal | Greater
DimValue   : Type                  # 运行时量纲值（§4.9）
DimError   : Type                  # 量纲检查错误
EvalError  : Type                  # eval/approx 错误
MathError  : Type                  # 求解/积分错误
DataError  : Type                  # CSV/JSON/NPY 错误
LinAlgError: Type                  # 线性代数错误
Record     : Type                  # 动态表的行（不透明）
Json       : Type                  # JSON 值（不透明）
Value      : Type                  # RSSN 动态值（§13.3）
OpError    : Type                  # 动态调用错误（§13.3）
Plugin     : Type                  # 插件句柄（§13.5）
Callback   : Type                  # 高阶回调（§13.5）
TypeRep    : Type                  # 运行时类型表示（§13.3）
TermRef    : Type                  # RSSN 项句柄（§13.3）
```

- `Tensor<0, τ>` **定义上等于** `τ`。这让索引的类型规则可以统一写成
  “rank 减一”，而不需要为标量特判。
- `Point[d]` 只有 `Float` 表示；符号温度用 `Real[Θ]` 的温差（§5.6）。
- `Unit[d]` 是单位值的类型，不是 `()`；`m : Unit[L]`。
- 函数类型 `(τ1, ..., τn) -> τ` 是多参数函数；`() -> τ` 是零参函数。
  没有自动柯里化。

### 3.2.3 记录与行

```
{ x: Real[L], y: Real[L] }              封闭记录
{ x: Real[L], ...r }                    开放记录，r : Row
```

行是字段名到类型的有限映射，带一个“剩余行”变量。记录类型等价于
`{ l1: τ1, ..., ln: τn | ρ }`，其中 `ρ` 是行变量或空行。字段顺序不重要。

### 3.2.4 变体

变体是名义类型：

```
type Shape = | Circle(Real[L]) | Rect(Real[L], Real[L])
```

`Shape` 与任何结构相同的记录/变体都不相等。构造器 `Circle : Real[L] -> Shape`。
变体可以有参数：

```
type Option<a> = | None | Some(a)
type Result<a, e> = | Ok(a) | Err(e)
```

### 3.2.5 类型方案

```
σ ::= ∀a:κ. σ | C => σ | τ
```

`∀` 量化类型变量、量纲变量、自然数变量和行变量。`C` 是类约束，例如
`Show a`。约束只在推断过程中出现；用户写签名时可以省略，编译器会重建。

## 3.3 类型环境与推断

### 3.3.1 环境

```
Γ ::= ∅ | Γ, x : σ | Γ, α:κ | Γ, C
```

环境包含值绑定、类型变量、量纲变量和类约束。顶层声明按依赖顺序检查；模块内
声明可以相互引用（包括递归函数），因此先收集签名再检查体。

### 3.3.2 双向检查

- **合成（synthesize）** `Γ ⊢ e ⇒ τ`：从表达式推出类型。
- **检查（check）** `Γ ⊢ e ⇐ τ`：在已知期望类型时检查表达式。

规则：

1. 字面量合成一个新鲜类型，带字面量约束。
2. 变量查环境，实例化其类型方案。
3. 函数调用合成函数类型，和实参类型统一。
4. lambda 优先用期望类型检查参数；没有期望类型时给参数新鲜类型变量。
5. `let` 推断右侧，泛化后绑定；`let rec` 先绑定新鲜类型再检查。
6. `if` 检查条件为 `Bool`，统一两个分支。
7. `match` 用 scrutinee 类型检查模式，统一分支体。
8. 记录/张量字面量按字段/元素统一。
9. 标注 `(e : τ)` 用检查模式。

### 3.3.3 泛化

在 `let` 和顶层声明处，把不在 `Γ` 中自由出现的类型/量纲/行变量全称量化。
类约束一并量化。例如：

```
let id = fn (x) => x
# id : ∀a. a -> a

let mean = fn (xs: Tensor<1, Real[d]>) => sum(xs) / length(xs)
# mean : ∀d. Tensor<1, Real[d]> -> Real[d]
```

**弱变量（weak variables）** 不泛化。它们用于“同一个名字必须在所有使用处
保持同一量纲”的值：

```
let x = symbol("x")        # x : Real[d]，d 是弱变量
let a = x + 3 m            # d 统一为 L
let b = x + 2 s            # error：x 已经是 L，不能又是 T
```

如果弱变量在声明结束时仍未解决，默认成 `1` 并给出警告 `W0301`。这比默默
泛化成 `∀d. Real[d]` 安全：后者会让同一个符号在不同地方被当成不同量纲。

`symfun` 的两个量纲参数也是弱变量。

**弱变量的分配规则。** `symbol` 和 `symfun` 是内建原语，不是普通多态函数：

```
symbol  : String -> Real[d]        # 每次调用分配新鲜弱变量 d
symfun  : String -> SymFun<d,e>    # 每次调用分配新鲜弱变量 d,e
```

- 每次**语法调用** `symbol(...)` 分配一个新的弱变量；同一个调用在循环/函数
  内重复执行时，运行时创建新的符号，但类型仍是同一个弱变量（单态）。
- `let f = symbol` 把 `symbol` 当值传递时，`f` 的类型固定为一个弱变量；
  之后 `f("x")` 和 `f("y")` 共享同一量纲。这是刻意的：符号工厂只有一个
  量纲。
- 弱变量带层级；泛化时排除弱变量及其替换中可达的变量。弱变量不能通过
  闭包逃逸成多态：`let mk = fn s => symbol(s)` 的类型是 `String -> Real[d]`
  且 `d` 弱，`mk` 不能被泛化成 `∀d`。
- 空张量 `[]` 不引入弱变量；它必须有期望类型，否则 `E0314`。

### 3.3.4 值限制？

Aevia 是纯语言，没有可变引用，因此**不需要** ML 的值限制。泛化可以应用于
任何表达式。唯一的例外是上面说的弱变量，它们不是值限制，而是“名字同一性”
约束。

## 3.4 数值塔与提升

### 3.4.1 提升格

```
Int  <  Float  <  Complex
Int  <  Real   <  Complex
```

`Float` 与 `Real` 不可比较；`join` 在两者相遇时选 `Real`（精确优先）。

- `Int` 可以提升为 `Real`、`Float`、`Complex`。
- `Float` 可以提升为 `Real`（精确转换）或 `Complex`。
- `Real` 可以提升为 `Complex`。
- `Complex` 不降级。

提升只在运算需要时发生。`Int → Real` 无损；`Float → Real` 是精确的
（每个 f64 都是有理数），但可能产生很大的分子/分母；`Real → Float` 只在
显式 `to_float` 时发生，且可能失败（含自由符号）。`Real + Float` 的结果是
`Real`，不会在运行时因为符号而失败。

`join` 的定义：

```
join(Int, Int)       = Int
join(Int, Real)      = Real
join(Int, Float)     = Float
join(Int, Complex)   = Complex
join(Real, Real)     = Real
join(Real, Float)    = Real        # 精确优先
join(Real, Complex)  = Complex
join(Float, Float)   = Float
join(Float, Complex) = Complex
join(Complex, _)     = Complex
```

`join` 对 Rep 变量产生偏序约束：`join(ρ, Int) = ρ'` 等价于
`ρ ⊒ Int, ρ' = ρ`（若 `ρ` 未解决），在默认化时再定。这样
`fn f(x) = x + 1` 可以推断为 `Num` 级的多态，而不是要求 `x : Int`。

### 3.4.2 算术类型规则

设 `ρ = join(ρ1, ρ2)`：

| 表达式 | 左类型 | 右类型 | 结果 |
|---|---|---|---|
| `a + b` | `Scalar<ρ1,d>` | `Scalar<ρ2,d>` | `Scalar<ρ,d>` |
| `a - b` | `Scalar<ρ1,d>` | `Scalar<ρ2,d>` | `Scalar<ρ,d>` |
| `a * b` | `Scalar<ρ1,d>` | `Scalar<ρ2,e>` | `Scalar<ρ,d*e>` |
| `a / b` | `Scalar<ρ1,d>` | `Scalar<ρ2,e>` | `Scalar<ρ',d/e>` |
| `a ^ n` | `Scalar<ρ,d>` | `Int` 字面量 | `Scalar<ρ',d^n>` |
| `a ^ q` | `Scalar<ρ,d>` | 有理数字面量 | `Scalar<ρ,d^q>` |
| `-a` | `Scalar<ρ,d>` | — | `Scalar<ρ,d>` |
| `abs(a)` | `Scalar<ρ,d>` | — | `Scalar<ρ,d>` |
| `sign(a)` | `Scalar<ρ,d>` | — | `Int` |
| `min(a,b)` | `Scalar<ρ1,d>` | `Scalar<ρ2,d>` | `Scalar<ρ,d>` |
| `clamp(x,lo,hi)` | `Scalar<ρ,d>` | 同 `d` | `Scalar<ρ,d>` |

其中：

- `ρ' = div_rep(ρ)`：`div_rep(Int) = Real`，其余 `ρ' = ρ`。除法、负指数、
  根、求逆都把整数提升为精确有理数。
- `min`/`max`/`mod`/`rem` 要求 `ρ ≠ Complex`。
- `abs` 对 `Complex` 不是同一个函数；复数用 `magnitude`（§8.15）。

除法特例：`Int / Int` 的结果是 `Real[1]`（精确有理数），不是 `Int`。整数
除法用 `div(a,b)` 或 `quot(a,b)`。`2^-1` 是 `Real` 的 `1/2`，不是 `Int`。

### 3.4.3 幂的静态限制

`a ^ n` 的量纲是 `d^n`。如果 `n` 是编译期已知的整数或有理数，指数在类型层
计算：

```
(3 m)^2        : Real[L^2]
sqrt(4 m^2)    : Real[L]
(2 m)^(1/2)    : Real[L^(1/2)]
```

如果 `n` 是运行时 `Int`，且 `d ≠ 1`，静态类型无法表达 `d^n`。这时：

- `a : Scalar<ρ,1>`：允许，结果 `Scalar<ρ,1>`（无量纲幂）。
- `a : Scalar<ρ,d>` 且 `d` 不是已知的 `1`：类型错误 `E0404`。改用：
  - `pow_dyn(a, n) : Dyn<ρ>`（§4.9），运行时检查；
  - 或者把公式改写为 `exp(n * ln(a / unit))`，显式提取单位。

这不是实现偷懒，而是静态量纲系统的边界：`d^n` 的指数是运行时值，类型层
无法表示。`Dyn` 是这个边界的显式出口。对泛型 `fn f<d>(x: Real[d], n: Int)`，
`d` 未被约束为 `1`，因此 `x^n` 被拒绝；要写成 `fn f(x: Real[1], n: Int)`。

### 3.4.4 超越函数

| 函数 | 参数 | 结果 |
|---|---|---|
| `sin cos tan asin acos atan` | `Scalar<ρ,1>` | `Scalar<ρ,1>` |
| `sinh cosh tanh asinh acosh atanh` | `Scalar<ρ,1>` | `Scalar<ρ,1>` |
| `exp ln log log2 log10` | `Scalar<ρ,1>` | `Scalar<ρ,1>` |
| `atan2(y,x)` | `Scalar<ρ,d>` × 同 `d` | `Scalar<ρ,1>` |
| `hypot(x,y)` | `Scalar<ρ,d>` × 同 `d` | `Scalar<ρ,d>` |
| `sqrt(x)` | `Scalar<ρ,d>`（`ρ ∈ {Real,Float}`） | `Scalar<ρ,d^(1/2)>` |
| `cbrt(x)` | `Scalar<ρ,d>`（`ρ ∈ {Real,Float}`） | `Scalar<ρ,d^(1/3)>` |
| `floor ceil round trunc` | `Scalar<ρ,d>` | `Scalar<ρ,d>` |
| `mod(a,b)` | `Scalar<ρ,d>` × 同 `d` | `Scalar<ρ,d>` |

`sin(3 m)` 是类型错误 `E0403`；`sin(30 deg)` 合法，因为 `deg` 是无量纲的
角度单位（§5.5）。`atan2` 的两个参数必须有相同量纲，结果是无量纲弧度。

## 3.5 张量类型

### 3.5.1 秩与形状

张量有两种类型形式：

```
Tensor<n, τ>                 # 只带秩；形状存在（existential）
Tensor<[n1, ..., nk], τ>     # 形状细化（refinement）
Matrix<n, m, τ>              # = Tensor<[n, m], τ>
```

- `Tensor<n, τ>` 的 `n` 是秩；形状是运行时值 `[d1, ..., dn]`。秩错误是
  编译错误，形状错误在细化类型下是编译错误，否则是运行时 `E0802`。
- `Tensor<[n1, ..., nk], τ>` 的 `n_i` 是类型级 `Nat`，可用 `?` 表示未知。
  这是 `Tensor` 的**特殊类型形式**（`Tensor<shape, τ>`），不是把 shape
  列表当作普通类型参数；字面量自动获得细化形状，`zeros<[3,4]>()` 等构造器
  可显式指定。
- 类型级 `Nat` 支持字面量、`+`、`*` 和线性等式；不做一般递归/证明。
  这足以表达矩阵乘法、reshape、拼接，不需要完整依赖类型。
- 细化 → 秩-only 是安全隐式转换；秩-only → 细化用
  `refine_shape(a) : Option<Tensor<[...],τ>>`（运行时检查）。
- `reshape<[n1..nk]>(a)`：若 `a` 的形状静态已知，编译期检查元素数相等；
  否则返回 `Result`。`reshape_dyn` 总是运行时检查。
- 索引：细化形状 + 字面量索引 → 编译期越界检查；动态索引仍运行时检查。

选择理由：形状细化捕获 `3×4`/`4×3`、矩阵乘法维度、reshape 元素数等错误，
同时保留 `Tensor<n,τ>` 作为“我不知道形状/从 CSV 来”的擦除形式。它不是
完整依赖类型：没有一般递归、没有证明项，只有 `Nat` 线性算术。

### 3.5.2 字面量

```
[1, 2, 3]                 : Tensor<[3], Int>
[1.0, 2.0, 3.0]           : Tensor<[3], Float>
[[1.0, 2.0], [3.0, 4.0]]  : Tensor<[2, 2], Float>
[1, 2.0]                  : Tensor<[2], Float>      # 提升
[]                        : 需要标注（形状可为 `[0]` 或 `[?]`）
```

- 嵌套深度决定秩；所有同层子序列长度必须相等，否则 `E0203`。
- 元素类型统一；`Int` 与 `Float` 统一为 `Float`。
- 空字面量没有元素可推断，必须由期望类型提供。

### 3.5.3 逐元素运算

对 `Tensor<n, Scalar<ρ1,d>>` 和 `Tensor<n, Scalar<ρ2,e>>`：

| 表达式 | 结果 |
|---|---|
| `a + b` / `a - b` | `Tensor<n, Scalar<join,d>>`，要求 `d = e` |
| `a * b` / `a / b` | `Tensor<n, Scalar<join,d*e>>` / `d/e` |
| `a ^ n` | `Tensor<n, Scalar<ρ,d^n>>`，`n` 为字面量 |
| `-a` | `Tensor<n, Scalar<ρ,d>>` |
| `abs(a)` | 同类型 |

标量与张量：

```
Scalar<ρ1,d> * Tensor<n, Scalar<ρ2,e>> : Tensor<n, Scalar<join,d*e>>
Scalar<ρ1,d> + Tensor<n, Scalar<ρ2,e>> : Tensor<n, Scalar<join,d>>，要求 d=e
Tensor<n, Scalar<ρ,d>> * Unit[e]      : Tensor<n, Scalar<ρ,d*e>>
Tensor<n, Scalar<ρ,d>> / Unit[e]      : Tensor<n, Scalar<ρ,d/e>>
Unit[e] * Tensor<n, Scalar<ρ,d>>       : Tensor<n, Scalar<ρ,e*d>>
```

**张量与张量的逐元素运算要求相同秩**。同秩内形状可以相等或某一维为 1
（广播）。细化类型在编译期检查；秩-only 在运行时检查。不做 NumPy 式跨秩
广播；这条限制让结果秩始终静态已知，也避免 `(3,1) + (1,4)` 这类静默错误。

`Point` 元素张量 `Tensor<shape, Point[d]>` 的元素级规则：

```
Point - Point : Float[d]
Point + Float[d] : Point
Point + Point : E0405
centroid : Tensor<1, Point[d]> -> Point[d]
```

`mean` 只接受数值张量；点的平均用 `centroid`。

### 3.5.3a `@` 的秩分派

`@` 不是单一函数，而是按秩分派的矩阵乘法：

| 左 | 右 | 结果 | 等价函数 |
|---|---|---|---|
| `Tensor<2, Scalar<ρ,d>>` | `Tensor<2, Scalar<ρ,e>>` | `Tensor<2, Scalar<ρ,d*e>>` | `matmul` |
| `Tensor<2, Scalar<ρ,d>>` | `Tensor<1, Scalar<ρ,e>>` | `Tensor<1, Scalar<ρ,d*e>>` | `matvec` |
| `Tensor<1, Scalar<ρ,d>>` | `Tensor<2, Scalar<ρ,e>>` | `Tensor<1, Scalar<ρ,d*e>>` | `vecmat` |
| `Tensor<1, Scalar<ρ,d>>` | `Tensor<1, Scalar<ρ,e>>` | `Scalar<ρ,d*e>` | `dot` |
| `Tensor<[m,k], Scalar<ρ,d>>` | `Tensor<[k,n], Scalar<ρ,e>>` | `Tensor<[m,n], Scalar<ρ,d*e>>` | `matmul`（细化） |

`A @ x` 是矩阵乘向量，`x @ A` 是向量乘矩阵，`x @ y` 是点积。细化形状下，
`m/k/n` 在编译期检查；秩-only 在运行时检查。`@` 的优先级与 `*` 相同，
左结合；混用时建议加括号。

稀疏矩阵用 `Sparse<F, shape, τ>`；`@` 在 dense/sparse 混合时按布局分派
（§13.9.5），不隐式互转。

### 3.5.4 索引

```
a : Tensor<n, τ>          n 必须是编译期已知的具体自然数
a[i]        : Tensor<n-1, τ>      要求 n >= 1
a[i, j]     : Tensor<n-2, τ>      要求 n >= 2，等价于 a[i][j]
a[i, ..]    : Tensor<n-1, τ>      取第 i 个切片
a[.., j]    : Tensor<n-1, τ>      取最后一维的第 j 个切片
a[i..j, ..] : Tensor<n, τ>        切片保持秩
```

- 索引要求秩已知；秩多态的代码用 `map`/`fold`/`sum`，不要用下标。
- 细化形状 `Tensor<[n1,...,nk], τ>` 下，字面量索引在编译期检查
  `0 <= i < n_i`；动态索引仍在运行时检查（`E0802`）。
- `a[i, j]` 只接受**全部是标量下标**；混合下标与切片请写成
  `a[i][.., j]` 或 `a[i, ..][j]`，避免“结果秩是切片个数”的复杂类型规则。
- 越界是运行时错误；字面量下标可以静态检查。

### 3.5.5 归约与线性代数

| 函数 | 类型 |
|---|---|
| `sum` | `Tensor<n, Scalar<ρ,d>> -> Scalar<ρ,d>` |
| `sum(a, axis)` | `(Tensor<n, Scalar<ρ,d>>, Int) -> Tensor<n, Scalar<ρ,d>>`（keepdims） |
| `mean` | `Tensor<n, Scalar<ρ,d>> -> Scalar<div_rep(ρ),d>` |
| `var` | `Tensor<n, Scalar<ρ,d>> -> Scalar<div_rep(ρ),d^2>` |
| `std` | `Tensor<n, Scalar<ρ,d>> -> Scalar<div_rep(ρ),d>` |
| `min/max` | 同上，返回 `Scalar<ρ,d>` |
| `norm` | `Tensor<n, Scalar<ρ,d>> -> Scalar<ρ,d>` |
| `normalize` | `Tensor<n, Scalar<ρ,d>> -> Tensor<n, Scalar<ρ,1>>` |
| `dot` | `Tensor<1, Scalar<ρ,d>> × Tensor<1, Scalar<ρ,e>> -> Scalar<ρ,d*e>` |
| `matmul` | `Tensor<2, Scalar<ρ,d>> × Tensor<2, Scalar<ρ,e>> -> Tensor<2, Scalar<ρ,d*e>>` |
| `matvec` | `Tensor<2, Scalar<ρ,d>> × Tensor<1, Scalar<ρ,e>> -> Tensor<1, Scalar<ρ,d*e>>` |
| `outer` | `Tensor<1, Scalar<ρ,d>> × Tensor<1, Scalar<ρ,e>> -> Tensor<2, Scalar<ρ,d*e>>` |
| `det` | `Tensor<2, Scalar<ρ,1>> -> Scalar<ρ,1>` |
| `inv` | `Tensor<2, Scalar<ρ,d>> -> Result<Tensor<2, Scalar<div_rep(ρ),1/d>>, LinAlgError>` |
| `linsolve` | `Tensor<2, Scalar<ρ,d>> × Tensor<1, Scalar<ρ,e>> -> Result<Tensor<1, Scalar<div_rep(ρ),e/d>>, LinAlgError>` |
| `eig` | `Tensor<2, Scalar<ρ,d>> -> (Tensor<1, Complex[d]>, Tensor<2, Complex<1>>)` |
| `svd` | `Tensor<2, Scalar<ρ,d>> -> (Tensor<2, Scalar<ρ,1>>, Tensor<1, Scalar<ρ,d>>, Tensor<2, Scalar<ρ,1>>)` |
| `qr` | `Tensor<2, Scalar<ρ,d>> -> (Tensor<2, Scalar<ρ,1>>, Tensor<2, Scalar<ρ,d>>)` |
| `trace` | `Tensor<2, Scalar<ρ,d>> -> Scalar<ρ,d>` |
| `rank` | `Tensor<2, τ> -> Int` |
| `at` | `(Tensor<n, τ>, Int) -> Tensor<n-1, τ>`，要求 `n >= 1` |
| `set` | `(Tensor<n, τ>, Tensor<1, Int>, τ) -> Tensor<n, τ>` |
| `map_indexed` | `((Int, a) -> b, Tensor<1, a>) -> Tensor<1, b>` |

`det` 只接受无量纲矩阵，因为 `det` 的量纲是 `d^n`，而 `n` 是运行时形状。
带量纲的行列式用 `det_unit(a, u) : Dyn<ρ>`（§4.9）；或者先除以单位
`det(a / u) * u^n`，其中 `n` 由 `shape(a)[0]` 给出，结果用 `Dyn<ρ>` 表达。
`prod` 同理：只接受无量纲元素，或返回 `Dyn<ρ>`。

`eig` 的特征值量纲与矩阵元素相同；特征向量无量纲。`svd` 的奇异值同量纲，
左右奇异向量无量纲。`qr` 的 Q 无量纲、R 同量纲。`lu` 的 L 无量纲、U 同
量纲（§8.4 给出完整签名）。

## 3.6 记录、行与变体

### 3.6.1 行统一

记录类型 `{ l: τ | ρ }` 的行统一规则：

```
{ l: τ1 | ρ1 } ~ { l: τ2 | ρ2 }
=> τ1 ~ τ2, ρ1 ~ ρ2

{ l: τ | ρ } ~ { l1: τ1, ..., ln: τn | ρ' }
=> l 与某个 li 匹配（或 l 不在其中，则 ρ 吸收剩余字段）
```

行统一 MUST 携带 **lacks 约束** `ρ lacks l`：当 `ρ` 吸收字段时，要求 `ρ`
不包含 `l`，否则会产生重复标签。标准 Rémy 规则：

```
ρ lacks l,  ρ ~ { l1: τ1, ..., ln: τn | ρ' }
=> ρ' 吸收 { l1: τ1, ..., ln: τn }，且 ρ' lacks l
```

实现用 Rémy 的行统一；缺少字段是错误 `E0306`，重复字段是 `E0208`。记录
字面量默认是封闭行；函数参数上的记录标注可以是开放行：

```
fn norm(p: { x: Real[L], y: Real[L], ...r }): Real[L] =
    sqrt(p.x^2 + p.y^2)
```

调用 `norm({ x = 1 m, y = 2 m, z = 3 m })` 合法，`r` 绑定 `{ z: Real[L] }`。

### 3.6.2 记录更新

```
{ p with x = 3.0 m }
```

要求 `p` 有字段 `x`；结果的类型是 `p` 的类型，字段 `x` 换成新类型。可以
新增字段：

```
{ p with z = 1.0 m }   # 结果行增加 z，要求 row(p) lacks z
```

- 若字段已存在，是替换；若不存在，是新增。新增要求 `row(p) lacks z`
  （§3.6.1），否则报 `E0208`。
- 更新是纯的：产生新记录，旧记录不变。运行时对唯一拥有的记录做原地更新。

### 3.6.3 变体与穷尽性

变体构造器的类型是函数或值：

```
Circle : Real[L] -> Shape
None   : Option<a>
Some   : a -> Option<a>
```

`match` 的穷尽性检查按构造器集合做。记录模式是开放的；变体模式必须覆盖
所有构造器，或用 `_` 兜底。冗余分支警告 `W0201`。

### 3.6.4 递归类型的正性

递归类型允许：

```
type Tree<a> = | Leaf | Node(Tree<a>, a, Tree<a>)
```

递归出现不能位于函数箭头的左侧：

```
type Bad = Bad -> Int        # E0312：非正递归
```

纯语言 + 一般递归下，非正递归可能破坏强规范化/逻辑一致性；Aevia 不做证明，
但仍要求正性，保证运行时值的构造是良基的。

## 3.7 函数、闭包与尾调用

### 3.7.1 函数类型

```
(τ1, ..., τn) -> τ
```

- 参数按值传递；没有引用、没有可变参数。
- 函数是闭包，捕获词法环境。
- 函数相等不可判定；`==` 不适用于函数（`Eq` 实例不存在）。
- 函数可以返回函数、接受函数。

### 3.7.2 多参与柯里化

Aevia 的函数是多参的，不是自动柯里的：

```
fn add(x: Int, y: Int): Int = x + y
add(1, 2)          # 3
add(1)             # 错误：参数数量不匹配
add(1, _)          # fn y => add(1, y)
```

需要柯里化时用 `curry`/`uncurry`（§8.3）。`_` 占位符是轻量的部分应用。

### 3.7.3 尾调用

- 自递归的尾调用 MUST 在常量栈空间内执行（解释器用 trampoline，编译器
  生成跳转）。
- 互递归的尾调用 SHOULD 被优化；自递归与互递归都保证尾调用优化。
- 非尾递归的深度受运行时栈限制；超过限制报 `E0805`（stack overflow），
  不会静默崩溃。

数值算法用尾递归 + 累加器表达循环：

```aevia
fn sum_to(n: Int, acc: Int): Int =
    if n == 0 then acc else sum_to(n - 1, acc + n)
```

### 3.7.4 递归与效应

纯函数可以递归；`IO` 动作也可以递归，但 `do` 块内的递归必须通过具名函数，
不能直接写 `let rec` 动作（当前规范限制）。这避免在 IO 中构造无限动作链。

## 3.8 类型类

### 3.8.1 声明与实例

```aevia
class Show a where
    show(x: a): String

instance Show for Real[d] where
    show(x) = format_quantity(x)
```

- 类可以有多参数：`class Convert a b where ...`。
- 方法只有签名；默认实现由 `derive` 或插件提供。
- 实例头 `Class τ`；`τ` 中的变量隐式全称量化。
- 实例方法必须覆盖所有方法。

### 3.8.2 解析

约束 `C τ` 在以下时机解决：

1. 如果 `τ` 是具体类型构造器，查找唯一匹配的实例。
2. 如果 `τ` 含未解决的类型变量，延迟到变量被解决。
3. 声明结束时仍无法解决，报 `E0310`（no instance）或 `E0304`
   （ambiguous）。

实例匹配是**头部匹配**，不做回溯。重叠实例是错误 `E0311`。

### 3.8.3 一致性与孤儿规则

- 同一类、同一类型头只能有一个实例。
- 实例必须定义在类型所属模块或类所属模块中（orphan rule）。标准库的
  实例定义在标准库；用户不能给 `Int` 添加 `Show` 实例。
- 这条规则保证单独编译时实例集合是确定的。

### 3.8.4 内置类

| 类 | 方法 | 说明 |
|---|---|---|
| `Eq a` | `==`、`!=` | **具体**结构相等；函数无实例；`Real` 无实例 |
| `Ord a` | `compare`、`<` 等 | 偏序；`compare` 返回 `Option<Ordering>`（NaN 为 `None`） |
| `TotalOrd a` | `total_compare` | 可用于 `Set`/`sort` 的全序；`Float` 的 NaN 排在最后 |
| `Show a` | `show` | 可读表示；字符串插值使用 |
| `Hash a` | `hash` | 可用于哈希容器；`Map` 需要 `TotalOrd`，不是 `Hash` |
| `Num a` | `+ - * from_int` | **仅无量纲**用户数值类型 |
| `Fractional a` | `/`、`from_rational` | 仅无量纲 |
| `Floating a` | `sqrt exp ln sin ...` | 仅无量纲 |
| `Container f` | `map`、`fold` | 张量、`Option`、`Set` 等 |
| `Sized a` | `length` | `String`、`Tensor<n,a>` |
| `Arbitrary a` | `arbitrary` | 属性测试的随机生成器 |
| `Parallel a` | `par_map` | 仅 `Int`/`Float[d]` 及其张量；符号 `Real` 不并行 |

**重要**：

- `Num`/`Fractional`/`Floating` 不适用于带量纲的标量，因为 `*` 会改变
  量纲，不满足 `a -> a -> a`。带量纲的算术是语言内置规则，不是类型类。
- `Real` 不是 `Eq`：`x == y` 对 `Real` 返回 `Prop`（§3.9），不是 `Bool`。
  需要具体相等时用 `decide(x == y)` 或 `approx_eq`。
- `Float` 是 `Eq`（IEEE：`NaN != NaN`），但**不是** `TotalOrd`；`Set<Float>`
  不合法。需要排序时用 `sort_float`（NaN 最后）或 `TotalFloat` 包装。
- `Set<a>` 要求 `TotalOrd a`；`Map<a,b>` 要求 `TotalOrd a`。
- `solve` 不返回 `Set`（符号根没有全序），返回 `Solutions<Real,d>`（§8.6）。

标准库提供的 `Show` 实例包括（不限于）：

```
Show (Int)                 Show (Real[d])
Show (Float[d])            Show (Complex[d])
Show (Tensor<n, a>)        Show (Point[d])
Show (Solutions<ρ,d>)      Show (Record)        # 动态表行
```

`Show`/`Eq`/`Ord`/`Hash` 的实例头必须用量纲多态形式（`Float[d]`），
不能只写无量纲的 `Float`，否则 `20.0 m` 无法打印。

### 3.8.5 派生

```
derive Eq, Show, Hash for Vec2
```

编译器为记录/变体生成结构化实例。派生 `Ord` 按字段声明顺序做字典序。
派生不适用于函数字段（没有 `Eq`/`Show` 实例）。

### 3.8.6 命题 `Prop`

`Bool` 只有 `true`/`false` 两个值，`if`、`match`、`assert` 只接受 `Bool`。
符号比较产生的是**命题** `Prop`，不是 `Bool`：

```
(==)  : Real[d] -> Real[d] -> Prop
(!=)  : Real[d] -> Real[d] -> Prop
(<)   : Real[d] -> Real[d] -> Prop
(<=)  : Real[d] -> Real[d] -> Prop
(>)   : Real[d] -> Real[d] -> Prop
(>=)  : Real[d] -> Real[d] -> Prop

decide  : Prop -> Option<Bool>
assuming: (Prop, () -> a) -> a
by_cases: (Prop, () -> Real[d], () -> Real[d]) -> Real[d]
```

- `Int`、`Float`、`String`、记录、变体的比较返回 `Bool`（具体、可判定）。
- `Real` 的比较返回 `Prop`；`x == y` 不会在运行时“猜”一个布尔值。
- `==` 对 `Real` 解析为 `sym_eq`，不是 `Eq` 类方法；`Eq a` 约束的泛型代码
  不会实例化 `Real`。这是刻意的 ad-hoc 重载：具体相等用 `Eq`，符号相等
  用 `Prop`。
- `Prop` 支持 `&&`、`||`、`!`，结果仍是 `Prop`。
- `decide(p)` 在能证明/否证时返回 `Some(true/false)`，否则 `None`。
- `assuming(p, thunk)` 把 `p` 作为假设加入 `thunk` 内的符号计算；`p` 不是
  具体 `Bool`。如果 `p` 能判定为 `false`，报 `E0804`。当前规范只支持单元谓词
  （`x > 0`、`x >= 0`、`x != 0`、`x ∈ ℤ`）。
- `by_cases(p, yes, no)` 在 `p` 可判定时选分支；不可判定时构造符号 `ite`
  项，用于分段函数的符号推导。
- `if`/`match` 只接受 `Bool`；要分支符号条件，先 `decide` 或改写为
  `assuming`/`by_cases`。

这条区分修复了“符号 Bool 伪装成 Bool”的不健全：`Bool` 的穷尽性、`Eq`
定律和 `if` 的确定性都成立；符号推理走 `Prop`。

## 3.9 效果：`IO`

### 3.9.1 类型

```
IO : Type -> Type
return : a -> IO a
bind   : IO a -> (a -> IO b) -> IO b
```

`IO` 是单一效果类型；capability（`fs`/`net`/`env`/`rand`/`gpu`）是运行时
权限，不是类型参数（§13.9.6）。这样标准库签名不被 effect row 侵入，
同时插件/沙箱仍能声明所需能力。

`IO a` 是一个**动作描述**，不是立即执行的值。只有运行时执行 `main` 时才
发生副作用。纯函数不能执行 `IO`；`do` 块的类型是 `IO a`。

### 3.9.2 `do` 的类型规则

```
Γ ⊢ e1 : IO a     Γ, x: a ⊢ rest : IO b
----------------------------------------
Γ ⊢ (x <- e1; rest) : IO b

Γ ⊢ e : IO a     a 不是 ()
--------------------------------
Γ ⊢ (e; rest) : IO b      （丢弃 e 的结果，警告 W0202）
```

`let` 在 `do` 中是纯绑定，不引入 `IO`。

### 3.9.3 错误与异常

- 可恢复错误用 `Result<T, E>`；`?` 传播。
- `Option<T>` 表示可能缺失；`?` 也可用于 `Option`。
- `panic(msg)` 是不可恢复的运行时错误，终止程序并打印调用栈。
- 没有 `try/catch`；IO 异常通过 `Result` 返回（例如 `read_file`）。

### 3.9.4 纯度边界

- 纯函数不能调用 `IO`；类型检查器拒绝 `IO` 出现在纯上下文。
- `IO` 动作可以捕获纯值，反之不行。
- `unsafePerformIO` 不存在。
- `main : IO Unit` 是唯一入口；库模块可以导出 `IO` 动作，由调用者执行。

## 3.10 字面量与默认值

### 3.10.1 字面量类型

字面量是**表示多态**的：它们带一个 `Literal` 约束，在期望类型处实例化，
在声明结束处默认化。

| 字面量 | 默认类型 | 可实例化为 | 备注 |
|---|---|---|---|
| `123` | `Int` | `Int`、`Real`、`Float`、`Complex` | 任意精度 |
| `1.5` | `Float[1]` | `Float[1]`、`Real[1]` | 精确十进制可转 `Real` |
| `1e3` | `Float[1]` | `Float[1]`、`Real[1]` | 同上 |
| `3 m` | `Real[L]` | `Real[L]`、`Float[L]`、`Complex[L]` | 单位字面量 |
| `3.0 m` | `Float[L]` | `Float[L]`、`Real[L]` | 单位字面量 |
| `25 °C` | `Point[Θ]` | `Point[Θ]` | 仿射，固定 Float |
| `oo` | `Real[1]` | `Real[1]`、`Float[1]` | 正无穷；`oo m` 是 `Real[L]` |
| `nan` | `Float[1]` | `Float[1]` | NaN |
| `"s"` | `String` | `String` | |
| `true` | `Bool` | `Bool` | |

- 整数字面量在期望 `Real`/`Float` 处自动转换；`df(2)` 中 `2` 是 `Real`。
- 小数字面量在期望 `Real` 处按**精确十进制**转换：`1.5` 是 `3/2`，不是
  二进制浮点。这让 `x + 1.5`（`x : Real`）保持精确。
- 单位字面量的表示由系数决定：整数系数默认 `Real`，小数系数默认 `Float`；
  期望类型可以覆盖。`fall_time(20 m)` 在 `h : Float[L]` 处把 `20 m` 实例化
  为 `Float[L]`。
- `0` **不是**量纲多态的。`0` 默认 `Int`；要写零长度，写 `0 m`；要写
  零浮点，写 `0.0`。这消除了“零乘任何值”的类型特例（§4.7）。
- `oo` 是 `Real[1]`，不是量纲多态；`oo m` 是 `Real[L]`。`nan` 是
  `Float[1]`。

### 3.10.2 默认化

在声明结束时，仍未解决的变量按以下顺序默认：

1. 由整数字面量产生的 `Rep` 变量 → `Int`。
2. 由小数字面量产生的 `Rep` 变量 → `Float`。
3. 单位字面量的 `Rep` 变量 → `Real`（整数系数）或 `Float`（小数系数）。
4. 其他 `Rep` 变量 → `Real`。
5. `Dim` 变量 → `1`，并警告 `W0301`。
6. `Nat` 变量 → 错误 `E0304`（秩无法推断）。
7. 行变量 → 空行（封闭记录）。

默认化只影响没有约束的变量；有约束的变量必须先统一。`Literal` 约束在
默认化时选择默认类型，并检查该类型是否满足上下文约束。

### 3.10.3 类型标注优先

如果表达式有期望类型，字面量按期望类型检查，不触发默认化：

```aevia
let x: Float[L] = 0.0 m    # 零长度，Float
let n: Int = 0             # 0 是 Int
let y = 0                  # 默认 Int
let z: Real[L] = 0 m       # 零长度，精确
```

## 3.11 动态量纲 `Dyn`（摘要）

静态量纲系统有两类无法表达的情况：

1. 运行时指数：`x^n`，`n : Int`，`x` 带量纲。
2. 形状相关的量纲：`det(A)` 的 `d^n`，`n` 是矩阵阶数。

Aevia 提供 `Dyn<ρ>` 作为显式逃逸（保留表示）：

```
dyn       : Scalar<ρ,d> -> Dyn<ρ>          # d 具体
check     : Dyn<ρ> -> Result<Scalar<ρ,d>, DimError>   # d 具体
dyn_with  : (Scalar<ρ,d>, DimValue) -> Dyn<ρ>
check_with: (DimValue, Dyn<ρ>) -> Result<Scalar<ρ,1>, DimError>
dim_of    : Dyn<ρ> -> DimValue
```

`dyn`/`check` 是类型指导原语：调用点的 `d` 必须具体，编译器把 `d` 物化成
`DimValue`。泛型代码用 `dyn_with`/`check_with` 显式传入运行时量纲。
`Dyn<ρ>` 的算术在运行时检查量纲；`+` 要求量纲相等，`*` 相加，`^` 乘指数。
`Dyn<ρ>` 不能隐式转换为 `Scalar<ρ,d>`；必须 `check`。完整规则见 §4.9。

## 3.12 类型错误

| 代码 | 情况 |
|---|---|
| `E0301` | 未绑定名字 |
| `E0302` | 类型不匹配 |
| `E0303` | occurs check（无限类型） |
| `E0304` | 无法推断（ambiguous） |
| `E0305` | 不是函数 |
| `E0306` | 记录缺少字段 |
| `E0307` | 构造器参数数量不匹配 |
| `E0308` | 模式与 scrutinee 类型不匹配 |
| `E0309` | 非穷尽匹配 |
| `E0310` | 缺少类型类实例 |
| `E0311` | 重叠实例 |
| `E0312` | 非正递归类型 |
| `E0313` | 纯上下文中出现 `IO` |
| `E0314` | 秩无法推断（索引/reshape） |
| `E0315` | 函数参数数量不匹配 |
| `E0316` | 重复定义 |
| `E0317` | 无法派生实例 |

量纲错误使用 `E04xx`（第 04 章），单位错误使用 `E05xx`（第 05 章）。

## 3.13 关键推断规则（形式化摘要）

```
Γ(x) = σ    σ' = inst(σ)
--------------------------------
Γ ⊢ x ⇒ σ'

Γ ⊢ e1 ⇒ τ1 -> τ2    Γ ⊢ e2 ⇐ τ1
--------------------------------
Γ ⊢ e1(e2) ⇒ τ2

Γ, x: τ1 ⊢ e ⇐ τ2
--------------------------------
Γ ⊢ (fn (x) => e) ⇐ τ1 -> τ2

Γ ⊢ e1 ⇐ Bool    Γ ⊢ e2 ⇐ τ    Γ ⊢ e3 ⇐ τ
------------------------------------------
Γ ⊢ if e1 then e2 else e3 ⇐ τ

Γ ⊢ e ⇒ { l: τ | ρ }
--------------------------------
Γ ⊢ e.l ⇒ τ

Γ ⊢ e1 ⇐ Scalar<ρ,d>    Γ ⊢ e2 ⇐ Scalar<ρ,d>
--------------------------------------------
Γ ⊢ e1 + e2 ⇐ Scalar<ρ,d>

Γ ⊢ e1 ⇐ Scalar<ρ1,d>    Γ ⊢ e2 ⇐ Scalar<ρ2,e>
------------------------------------------------
Γ ⊢ e1 * e2 ⇐ Scalar<join(ρ1,ρ2), d*e>
```

完整的量纲统一规则见下一章。类型系统的实现顺序是：解析 → 名字解析 →
收集签名 → 推断/检查 → 默认化 → 导出接口。

下一章把 `d` 从占位符变成真正的代数对象。
