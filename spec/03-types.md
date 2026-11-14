# 03 — 类型系统

Aevia 的类型系统是 **HM 推断 + 量纲代数 + 类型类 + 行多态**。没有完整依赖类型，也没有
子类型；灵活性来自参数多态、量纲多态和行多态。本章定义类型的结构、推断规则和默认值
策略，量纲代数在第 04 章。

## 3.1 Kind

| kind | 含义 | 例子 |
|---|---|---|
| `Type` | 值类型 | `Int`、`Real[L]`、`Tensor<[3,4], Float>` |
| `Dim` | 量纲表达式 | `L`、`L/T`、`d` |
| `Nat` | 类型级自然数 | `0`、`2`、`n` |
| `Row` | 记录行 | `{ x: Real[L], ...r }` 中的 `r` |
| `Rep` | 数值表示，封闭 kind：`Int`、`Real`、`Float`、`Complex` | `ρ` |
| `Format` | 稀疏格式，封闭：`Csr`、`Csc`、`Coo`、`Block`、`Bcsr` | `Csr` |
| `Shape` | 类型级形状 | `[3,4]`、`[n,m]`、`[?]` |

用户不能扩展 `Rep` 与 `Format`。

## 3.2 类型

### 3.2.1 标量 `Scalar<ρ, δ>`

所有数值标量的统一形式是 `Scalar : Rep -> Dim -> Type`，表面语法有四个别名：

| 写法 | 内部形式 | 含义 |
|---|---|---|
| `Int` | `Scalar<Int, 1>` | 任意精度整数，无量纲 |
| `Real[d]` | `Scalar<Real, d>` | 精确实数或形式符号表达式 |
| `Float[d]` | `Scalar<Float, d>` | IEEE-754 binary64，永不符号化 |
| `Complex[d]` | `Scalar<Complex, d>` | 复数（精确/符号或 `Complex64`） |

`Real`/`Float`/`Complex` 无方括号等价于 `[1]`。`Int` 只能无量纲：带量纲的精确整数是值为
整数的 `Real[L]`，所以 `Int` 可安全用于索引和计数。`Float` 严格是 f64；含符号的表达式由
`Real` 承担，因此 `Real + Float` 是 `Real`（`Float` 被精确转为有理数，符号信息保留，
`pi + 1.0` 仍是 `pi + 1`）。要留在浮点路径，写 `to_float(x)` 或让所有操作数都是 `Float`。

### 3.2.2 其他类型构造器

```
()            : Type                         unit 值
Bool          : Type                         具体布尔
Prop          : Type                         符号命题（§3.8.6）
String        : Type
Tensor        : Shape -> Type -> Type        张量（秩或细化形状）
Matrix        : Nat -> Nat -> Type -> Type   = Tensor<[n,m], τ>
Sparse        : Format -> Shape -> Type -> Type   稀疏张量（§13.9.5）
Point         : Dim -> Type                  仿射点（Float 表示，可作张量元素）
Unit          : Dim -> Type                  单位值
Table         : Type -> Type                 行多态表；参数是封闭记录类型（§13.9.3）
Dynamic       : Type                         未知 schema（仅 Table<Dynamic>）
Device        : Type                         运行时设备（§13.9.4）
Set Option    : Type -> Type
Result        : Type -> Type -> Type
IO            : Type -> Type
(->)          : 多参数函数类型 (τ1, …, τn) -> τ；() -> τ 是零参函数
SymFun        : Dim -> Dim -> Type           符号函数（用于 dsolve）
Dyn           : Rep -> Type                  动态量纲标量（§4.9）
RateOf        : Type -> Type                 ODE 状态的时间导数类型（§8.9）
DiffusionOf   : Type -> Type                 SDE 状态的扩散系数类型（§8.9.1）
Solutions     : Rep -> Dim -> Type           符号解集合（§8.6）
```

- `Tensor<0, τ>` 定义上等于 `τ`，使索引的类型规则统一为“秩减一”。
- `Point[d]` 只有 `Float` 表示；符号温度用 `Real[Θ]` 的温差（§5.6）。
- `Unit[d]` 是单位值的类型（`m : Unit[L]`），不是 `()`。
- `DiffusionOf<s>` 与 `RateOf<s>` 同构，但每个叶子的量纲再除以 `T^(1/2)`（布朗增量 `dW`
  的量纲是 `T^(1/2)`）；含 `Int`/`Bool`/函数字段的状态是 `E0302`。
- Prelude 与各库还定义 `Ordering`（`Less | Equal | Greater`）和不透明/错误类型
  `DimValue`、`DimError`、`EvalError`、`MathError`、`DataError`、`LinAlgError`、`Record`、
  `Json`、`Value`、`OpError`、`Plugin`、`PluginId`、`Callback`、`TypeRep`、`TermRef`
  （§4.9、§13.3、§13.5）。

### 3.2.3 记录与行

```
{ x: Real[L], y: Real[L] }              封闭记录
{ x: Real[L], ...r }                    开放记录，r : Row
```

记录类型是 `{ l1: τ1, …, ln: τn | ρ }`，`ρ` 是行变量或空行，字段顺序无关。

### 3.2.4 变体

变体是名义类型：`type Shape = | Circle(Real[L]) | Rect(Real[L], Real[L])` 与任何结构相同
的记录或变体都不相等。构造器可带类型参数：`type Option<a> = | None | Some(a)`。

### 3.2.5 类型方案

`σ ::= ∀a:κ. σ | C => σ | τ`。`∀` 量化类型、量纲、自然数、行与表示变量；`C` 包括类约束
（`Show a`）和数值约束：字面量的表示域、`Join(ρ1, ρ2, ρout)`、`Divide(ρin, ρout)` 以及依赖
输入的私有字面量读取规则。约束随方案、函数别名和模块接口保存，编译器自动重建，用户无需
手写。实例化同时重命名约束中的变量；运算产生的中间表示独立实例化，但不成为可显式选择的
`<…>` 参数。

## 3.3 环境与推断

### 3.3.1 环境

`Γ ::= ∅ | Γ, x : σ | Γ, α:κ | Γ, C`。顶层声明先收集签名再检查体，因此模块内声明可以互相
引用（含递归）。

### 3.3.2 双向检查

- **合成** `Γ ⊢ e ⇒ τ`：从表达式推出类型；**检查** `Γ ⊢ e ⇐ τ`：已知期望类型。
- 字面量合成新鲜类型并带字面量约束；变量实例化其方案；调用合成函数类型并与实参统一；
  lambda 有期望类型时检查参数，否则给新鲜类型变量；`let` 推断右侧后泛化，`let rec` 先绑定
  新鲜类型；`if` 要求条件为 `Bool` 并统一两分支；`match` 用 scrutinee 类型检查模式并
  统一分支体；记录/张量字面量按字段/元素统一；标注 `(e : τ)` 用检查模式。

### 3.3.3 泛化与弱变量

在 `let` 和顶层声明处，把不在 `Γ` 中自由出现的类型、量纲、行变量连同类约束一起全称
量化：

```
let id = fn (x) => x
# id : ∀a. a -> a
let mean = fn (xs: Tensor<1, Real[d]>) => sum(xs) / length(xs)
# mean : ∀d. Tensor<1, Real[d]> -> Real[d]
```

**弱变量**不泛化，用于“同一个名字在所有使用处必须是同一个量纲”的值：

```
let x = symbol("x")        # x : Real[d]，d 是弱变量
let a = x + 3 m            # d 统一为 L
let b = x + 2 s            # E0401：x 已经是 L
```

声明结束时仍未解决的弱变量默认为 `1` 并警告 `W0301`；泛化成 `∀d. Real[d]` 会让同一个符号
在不同处被当作不同量纲，所以不这样做。

- `symbol : String -> Real[d]` 与 `symfun : String -> SymFun<d,e>` 是内建原语，每个**语法
  调用**分配新鲜弱变量；循环或函数内重复执行时运行时创建新符号，类型仍是同一个弱变量。
- `let f = symbol` 把 `symbol` 当值传递时，`f` 的所有调用共享一个量纲（符号工厂只有
  一个量纲）。弱变量带层级，泛化时排除弱变量及其替换中可达的变量，不能经闭包逃逸成
  多态：`let mk = fn s => symbol(s)` 的 `d` 仍是弱的。
- 空张量 `[]` 不引入弱变量，必须有期望类型（`E0314`）。
- Aevia 的 `Symbol` 就是 `Real[δ]`（`δ` 弱）；不存在第五种值类型。生成 `Rssn` façade 时，
  签名中的 `Symbol` 参数读作新鲜弱 `Real[δ]`，`Real[d]` 参数读作 `Dim(Real, d)`；因此
  `diff` 的签名 `(Term, Symbol) -> Term` 读作 `(Dyn<term>, Real[δ]) -> Dyn<term>`，弱变量在
  调用点统一。

### 3.3.4 值限制

纯语言没有可变引用，不需要值限制；任何表达式都可泛化。弱变量不是值限制，而是名字同一性
约束。

## 3.4 数值塔与提升

### 3.4.1 提升格

```
Int < Float < Complex        Int < Real < Complex
```

`Float` 与 `Real` 不可比较，`join` 在两者相遇时选 `Real`（精确优先）。`Int` 可提升为任意
其他表示；`Float` 可精确提升为 `Real`（每个 f64 都是有理数，但分子分母可能很大）或
`Complex`；`Real` 可提升为 `Complex`；`Complex` 不降级。`Real → Float` 只在显式
`to_float` 时发生，且含自由符号时失败。提升只在运算需要时发生。

```
join(Int, Int) = Int       join(Int, Real) = Real       join(Int, Float) = Float
join(Real, Real) = Real    join(Real, Float) = Real     join(Float, Float) = Float
join(_, Complex) = Complex
```

未解决的表示保留有限的算术关系而不是直接等同于某个输入：`Join(ρ1, ρ2, ρout)` 按上表约束
三者，`Divide(ρin, ρout)` 约束整数除法的提升；普通偏序下界不能替代它们。

```aevia
fn half<r: Rep>(x: r) = x / 2
let exact = half<Int>(3)        # Real，精确的 3/2
let floating = half<Float>(3.0) # Float
```

`half` 只有输入 `r` 一个显式参数，调用者不能另选与除法规则冲突的结果表示；这些关系在检查期
和闭包的运行时泛型证明中都保留。

### 3.4.2 算术类型规则

设 `ρ = join(ρ1, ρ2)`，`ρ' = div_rep(ρ)`（`div_rep(Int) = Real`，其余不变）：

| 表达式 | 操作数 | 结果 |
|---|---|---|
| `a + b`、`a - b`、`min(a,b)` | `Scalar<ρ1,d>`、`Scalar<ρ2,d>` | `Scalar<ρ,d>` |
| `a * b` | `Scalar<ρ1,d>`、`Scalar<ρ2,e>` | `Scalar<ρ,d*e>` |
| `a / b` | 同上 | `Scalar<ρ',d/e>` |
| `a ^ n` | `Scalar<ρ,d>`、整数字面量 | `Scalar<ρ',d^n>` |
| `a ^ q` | `Scalar<ρ,d>`、有理字面量 | `Scalar<ρ,d^q>` |
| `-a`、`abs(a)` | `Scalar<ρ,d>` | `Scalar<ρ,d>` |
| `sign(a)` | `Scalar<ρ,d>` | `Int` |
| `clamp(x,lo,hi)` | 三者同 `d` | `Scalar<ρ,d>` |

除法、负指数、根、求逆把整数提升为精确有理数：`Int / Int` 是 `Real[1]`（`7 / 2` 是精确
`7/2`，不能标注为 `Int`），整数除法用 `div(a,b)` 或 `quot(a,b)`；`2^-1` 是 `Real` 的 `1/2`。
`min`/`max`/`mod`/`rem` 要求 `ρ ≠ Complex`；复数的模用 `magnitude`（§8.15），不是 `abs`。

### 3.4.3 幂的静态限制

指数是编译期整数或有理数时在类型层计算：`(3 m)^2 : Real[L^2]`，`sqrt(4 m^2) : Real[L]`，
`(2 m)^(1/2) : Real[L^(1/2)]`。指数是运行时 `Int` 时：

- 底数无量纲（`Scalar<ρ,1>`）：允许，结果无量纲。
- 底数量纲 `δ` 是尚未求解的推断变量（如 `diff(fn t => t^t)` 的 `t`）：运行时指数要求底数
  无量纲，这是约束而非错误，检查器令 `δ = 1`，与 `sin(t)` 让 `t` 无量纲同理。
- 底数量纲已知且不是 `1`：`E0404`。改用 `pow_dyn(a, n) : Dyn<ρ>`（§4.9），或写成
  `exp(n * ln(a / unit))`。泛型 `fn f<d>(x: Real[d], n: Int)` 里的 `x^n` 因 `d` 未被约束为
  `1` 而被拒绝。

这是静态量纲系统的边界：`d^n` 的指数是运行时值，类型层无法表示，`Dyn` 是显式出口。

### 3.4.4 超越函数

| 函数 | 参数 | 结果 |
|---|---|---|
| `sin cos tan asin acos atan`、`sinh … atanh`、`exp ln log log2 log10` | `Scalar<ρ,1>` | `Scalar<ρ,1>` |
| `atan2(y,x)` | 同 `d` | `Scalar<ρ,1>` |
| `hypot(x,y)` | 同 `d` | `Scalar<ρ,d>` |
| `sqrt(x)` / `cbrt(x)` | `Scalar<ρ,d>`，`ρ ∈ {Real,Float}` | `d^(1/2)` / `d^(1/3)` |
| `floor ceil round trunc`、`mod(a,b)` | `Scalar<ρ,d>`（`mod` 两者同 `d`） | `Scalar<ρ,d>` |

`sin(3 m)` 是 `E0403`；`sin(30 deg)` 合法，因为 `deg` 无量纲（§5.5）。

## 3.5 张量类型

### 3.5.1 秩与形状

```
Tensor<n, τ>                 # 只带秩；形状是运行时值
Tensor<[n1, …, nk], τ>       # 形状细化
Matrix<n, m, τ>              # = Tensor<[n, m], τ>
```

- `Tensor<n, τ>` 是 `Tensor<[?, …, ?], τ>`（`n` 个 `?`）的糖：秩错误是编译错误；形状错误在
  细化类型下是编译错误，否则是运行时 `E0802`。秩 `n` 必须是具体整数（`E0304`）。
- 细化形状的 extent 是类型级 `Nat`（字面量、变量、`+`、乘以字面量，或 `?`）。字面量自动
  获得细化形状，`zeros_of<[3,4]>()` 等构造器可显式指定。`Tensor` 的形状参数是特殊类型
  形式，不是普通类型参数。
- 细化 → 秩-only 是安全的隐式转换；反方向用 `refine_shape(a) : Option<Tensor<[…],τ>>`
  做运行时检查，直接标注是 `E0302`。依形状分支用 `refine_shape<[3, ?]>(a)`（§8.4）。
- `reshape<[n1, …, nk]>(a)` 总返回 `Result<Tensor<shape,τ>, ShapeError>`：形状静态已知时
  元素数在编译期检查、结果恒为 `Ok`，否则运行时检查；`reshape_dyn` 总是运行时检查。
- 索引：细化形状加字面量下标在编译期检查 `-n_i <= i < n_i`，动态下标运行时检查。

**Nat 方程。** 统一把等式化为多项式处理：

- 只有一个变量时解出非负整数：`n + 1 ~ 3` ⇒ `n = 2`；`2·n ~ 6` ⇒ `n = 3`；`n ~ 2·n` ⇒
  `n = 0`。多变量等式保留为待解约束，后续绑定后重试；常数残差非零是 `E0302`。
- 变量乘积（`n*m`）是不透明单项式，只与同一乘积相消（`n*m ~ m*n`）；一个因子解出后代回
  成线性方程。仍无法解出的乘积在声明末尾报 `E0304` 并给出方程。没有 binder 的 shape
  变量和实例头中的 Nat 变量也是 `E0304`。
- `fn f<n, m>` 里**声明的**变量是刚性的：可被字面量或表达式解出，但两个不同的声明变量
  不会被合并，`n ~ m` 是 `E0302`。泛型被调用方实例化出的变量是推断变量，可与任何表达式
  消去，所以带符号 extent 的泛型函数可以调用另一个泛型函数。

**广播。** 逐元素运算按轴取：相等，或一侧是字面量 `1`（结果取另一侧）；`[3,1] + [1,4]` 是
`[3,4]`，`[n,1] + [1,m]` 是 `[n,m]`；两个都不是 `1` 的符号 extent 被统一；秩-only 一侧视为各轴
未知。**秩不同不广播**（`E0302`，提示用 `map` 或 `outer` 显式构造）——结果秩因此始终静态
可知，也避免 `(3,1) + (1,4)` 这类静默错误。

形状细化在不引入完整依赖类型的前提下捕获 `3×4`/`4×3`、矩阵乘维度和 reshape 元素数错误，
同时保留 `Tensor<n,τ>` 作为“形状未知/来自 CSV”的擦除形式；它只有 `Nat` 线性算术，没有
一般递归和证明项。

### 3.5.2 字面量

```
[1, 2, 3]                 : Tensor<[3], Int>
[[1.0, 2.0], [3.0, 4.0]]  : Tensor<[2, 2], Float>
[1, 2.0]                  : Tensor<[2], Float>      # 提升
[]                        : 需要标注
```

嵌套深度决定秩，同层长度必须相等（`E0203`），元素类型统一，空字面量必须由期望类型提供
（`E0314`）。

### 3.5.3 逐元素运算

对 `Tensor<n, Scalar<ρ1,d>>` 与 `Tensor<n, Scalar<ρ2,e>>`：`+`/`-` 要求 `d = e`，结果
`Scalar<join,d>`；`*`/`/` 得 `d*e`/`d/e`；`a ^ n`（字面量 `n`）得 `d^n`；一元 `-` 与 `abs` 保持
类型。标量与张量、张量与单位值的混合：

```
Scalar<ρ1,d> * Tensor<n, Scalar<ρ2,e>> : Tensor<n, Scalar<join,d*e>>
Scalar<ρ1,d> + Tensor<n, Scalar<ρ2,e>> : Tensor<n, Scalar<join,d>>，要求 d=e
Tensor<shape, Scalar<ρ,d>> * Unit[e]   : Tensor<shape, Scalar<ρ,d*e>>
Tensor<shape, Scalar<ρ,d>> / Unit[e]   : Tensor<shape, Scalar<ρ,d/e>>
```

同秩的细化形状与秩-only 形状在运算中互相统一（细化 → 秩-only 隐式转换）。

`Tensor<shape, Point[d]>` 的元素规则：`Point - Point : Float[d]`；`Point + Float[d] : Point`；
`Point + Point` 是 `E0405`；`mean` 只接受数值张量，点的平均用
`centroid : Tensor<1, Point[d]> -> Point[d]`。

### 3.5.3.1 `@` 的秩分派

`@` 按秩分派（优先级同 `*`，左结合）：

| 左 | 右 | 结果 | 函数 |
|---|---|---|---|
| `Tensor<2, ·d>` | `Tensor<2, ·e>` | `Tensor<2, ·d*e>` | `matmul` |
| `Tensor<2, ·d>` | `Tensor<1, ·e>` | `Tensor<1, ·d*e>` | `matvec` |
| `Tensor<1, ·d>` | `Tensor<2, ·e>` | `Tensor<1, ·d*e>` | `vecmat` |
| `Tensor<1, ·d>` | `Tensor<1, ·e>` | `Scalar<ρ,d*e>` | `dot` |

其中 `·d` 表示 `Scalar<ρ,d>`。细化形状下 `[m,k] @ [k,n] → [m,n]`、`[m,k] @ [k] → [m]`、
`[k] @ [k,n] → [n]`，`m/k/n` 在编译期检查，秩-only 在运行时检查。稀疏矩阵用
`Sparse<F, shape, τ>`；dense/sparse 混合按布局分派（§13.9.5），不隐式互转。

### 3.5.4 索引

`a : Tensor<n, τ>` 的 `n` 必须是已知具体自然数。每个**标量下标**消去一个轴，每个**切片**
（`..`、`i..j`、`i:j:s`）保留一个轴：

```
a[i]          : Tensor<n-1, τ>      要求 n >= 1
a[i, j]       : Tensor<n-2, τ>      等价于 a[i][j]
a[i, ..]      : Tensor<n-1, τ>
a[.., j]      : Tensor<n-1, τ>
a[i..j, ..]   : Tensor<n, τ>
a[i..j, j]    : Tensor<n-1, τ>
```

秩多态代码用 `map`/`fold`/`sum` 而非下标。越界是运行时 `E0802`，字面量下标可静态检查。

### 3.5.5 归约与线性代数的量纲规则

完整签名在 §8.4，这里只列量纲与表示的规则：

| 函数 | 结果 |
|---|---|
| `sum`、`min`、`max`、`norm`、`trace` | `Scalar<ρ,d>` |
| `sum(a, axis)` | 同 `d`，保持秩（keepdims） |
| `mean`、`std` | `Scalar<div_rep(ρ),d>` |
| `var` | `Scalar<div_rep(ρ),d^2>` |
| `normalize` | 元素无量纲 |
| `dot`、`matmul`、`matvec`、`outer` | `d*e` |
| `det`、`prod` | **只接受无量纲**元素 |
| `inv` | `Result<Tensor<2, Scalar<div_rep(ρ),1/d>>, LinAlgError>` |
| `linsolve(A, b)` | `Result<Tensor<1, Scalar<div_rep(ρ),e/d>>, LinAlgError>` |
| `eig` | 特征值量纲 `d`，特征向量无量纲 |
| `svd`、`qr`、`lu` | 奇异值/`R`/`U` 同量纲 `d`，其余因子无量纲 |

`det` 的量纲是 `d^n` 而 `n` 是运行时形状，所以只接受无量纲矩阵；带量纲的行列式用
`det_unit(a, u) : Dyn<ρ>`（§4.9）。`prod` 同理。

## 3.6 记录、行与变体

### 3.6.1 行统一

```
{ l: τ1 | ρ1 } ~ { l: τ2 | ρ2 }   ⇒  τ1 ~ τ2, ρ1 ~ ρ2
{ l: τ | ρ } ~ { l1: τ1, …, ln: τn | ρ' }
   ⇒ l 与某个 li 匹配，或 l 不在其中而由 ρ 吸收剩余字段
```

行统一采用 Rémy 算法并携带 **lacks 约束**：`ρ lacks l` 时，`ρ` 吸收字段要求其不含 `l`，
否则会产生重复标签。缺少字段是 `E0306`，重复字段是 `E0208`。记录字面量是封闭行，函数
参数上的记录标注可以是开放行：

```aevia
fn norm(p: { x: Real[L], y: Real[L], ...r }): Real[L] =
    sqrt(p.x^2 + p.y^2)
```

`norm({ x = 1 m, y = 2 m, z = 3 m })` 合法，`r` 绑定 `{ z: Real[L] }`。

### 3.6.2 记录更新

`{ p with x = 3.0 m }` 要求 `p` 有字段 `x`，结果类型是 `p` 的类型并把 `x` 换成新类型。字段
不存在时是新增，要求 `row(p) lacks z`（否则 `E0208`）。更新是纯的，旧记录不变；运行时对
唯一拥有的记录原地更新。

### 3.6.3 变体与穷尽性

构造器是函数或值：`Circle : Real[L] -> Shape`，`None : Option<a>`，`Some : a -> Option<a>`。
变体 `match` 必须覆盖所有构造器或用 `_` 兜底；记录模式是开放的；冗余分支警告 `W0201`。
构造器模式的元数不符是 `E0307`，构造器调用的实参数不符是 `E0315`。

### 3.6.4 递归类型的正性

递归类型允许（`type Tree<a> = | Leaf | Node(Tree<a>, a, Tree<a>)`），但递归出现不能在函数
箭头左侧：`type Bad = | B(Bad -> Int)` 是 `E0312`。Aevia 不做证明，但要求正性以保证运行时
值的构造是良基的。

## 3.7 函数、闭包与尾调用

### 3.7.1 函数类型

`(τ1, …, τn) -> τ`：参数按值传递，函数是闭包，可以作参数和返回值。函数相等不可判定，
`==` 不适用于函数（`E0310`，没有 `Eq` 实例）。

### 3.7.2 多参数

函数是多参的，不自动柯里化：

```aevia
fn add(x: Int, y: Int): Int = x + y
add(1, 2)          # 3
add(1)             # E0315：参数数量不匹配
add(1, _)          # fn y => add(1, y)
```

需要柯里化时用 `curry`/`uncurry`（§8.3）；`_` 占位符是轻量的部分应用。

### 3.7.3 尾调用

- 自递归的尾调用 MUST 在常量栈空间内执行；互递归的尾调用也被优化。
- 非尾递归深度受运行时栈限制，超限报 `E0805`（stack overflow），不会静默崩溃。

```aevia
fn sum_to(n: Int, acc: Int): Int =
    if n == 0 then acc else sum_to(n - 1, acc + n)
```

### 3.7.4 递归与效果

`IO` 动作可以递归，但递归必须通过具名函数，`do` 内不能声明 `fn` 或 `let rec`
（§2.3.12）。

## 3.8 类型类

### 3.8.1 声明与实例

```aevia
class Show a where
    show(x: a): String

instance Show for Real[d] where
    show(x) = format_quantity(x)
```

- 类**只有一个**类型参数；`class Convert a b` 是 `E0317`。
- 方法只有签名；默认实现来自 `derive`、内置结构实现（Prelude 类）或插件。
- 实例头是类型构造器应用；裸类型变量头 `instance C for a` 是 `E0317`；头中的变量隐式
  全称量化。
- 用户类的实例必须覆盖所有方法，缺失的方法在 `E0317` 中列出。Prelude 类是**钩子**：实例
  可只覆盖部分方法，其余由内置结构实现提供（如记录与元组的打印）。
- 类与实例的身份是符号化的：类键 `(定义模块路径, 类名)`，实例键再加规范头类型；`AdtId`
  只是进程内见证，不进入接口或缓存。`private class` 只在定义模块可见；`private instance`
  不允许（实例随模块导出，§7.4）。

### 3.8.2 解析

约束 `C τ` 的解决：`τ` 是具体类型构造器时查找唯一匹配的实例；含未解决变量时延迟；声明
结束仍无法解决则 `E0310`（无实例）或 `E0304`（歧义）。实例匹配是头部匹配，不回溯；重叠
实例是 `E0311`。

### 3.8.3 一致性与孤儿规则

同一类、同一类型头只能有一个实例；实例必须定义在类型所属模块或类所属模块中（orphan
rule），所以用户不能给 `Int` 添加 `Show`。这保证单独编译时实例集合是确定的。

### 3.8.4 内置类

| 类 | 方法 | 说明 |
|---|---|---|
| `Eq a` | `==` `!=` | 具体结构相等；函数、`Real` 无实例 |
| `Ord a` | `compare`、`<` 等 | 偏序；`compare : Option<Ordering>`（NaN 为 `None`） |
| `TotalOrd a` | `total_compare` | `Set`/`Map`/`sort` 所需的全序；`Float` 无实例 |
| `Show a` | `show` | 可读表示；字符串插值使用 |
| `Hash a` | `hash` | 哈希容器 |
| `Num a` `Fractional a` `Floating a` | 算术/`from_int`/`sqrt` 等 | **仅无量纲**用户数值类型 |
| `Container f` | `map`、`fold` | 张量、`Option`、`Set` |
| `Sized a` | `length` | `String`、`Tensor` |
| `Arbitrary a` | `arbitrary` | 属性测试的随机生成器 |
| `Parallel a` | `par_map` | 仅 `Int`/`Float[d]` 及其张量；符号 `Real` 不并行 |

- `Num` 等不适用于带量纲的标量，因为 `*` 会改变量纲，不满足 `a -> a -> a`；带量纲的
  算术是语言内置规则。
- `Real` 不是 `Eq`：`x == y` 返回 `Prop`（§3.8.6）；要具体相等用 `decide(x == y)` 或
  `approx_eq`。`==` 对 `Real` 解析为 `sym_eq`，不经 `Eq`，所以 `Eq a` 约束的泛型代码不会
  实例化 `Real`。
- `Float` 是 `Eq`（`NaN != NaN`）但不是 `TotalOrd`，`Set<Float>` 不合法；排序用
  `sort_float`（NaN 最后）。`Set<a>` 与 `Map<a,b>` 要求 `TotalOrd a`。
- `solve` 返回 `Solutions<Real,d>`（§8.6），不是 `Set`（符号根没有全序）。

标准库的 `Show`/`Eq`/`Ord`/`Hash` 实例头必须用量纲多态形式（`Float[d]`），否则 `20.0 m`
无法打印。`Show` 对记录、元组、`Tensor`、`Point`、`Solutions`、`Dyn` 与单位值是结构性
保证：无用户实例时由内置打印器渲染；用户为**自己声明**的名义类型写的实例（或 `derive
Show`）优先。函数没有 `Show` 实例也没有结构性回退（`E0310`）。内置类的实例集参与全局
一致性（§7.4）：两个实例必须能由运行时见证区分（§9.2.3），见证相同则 `E0311`，例如
`Tensor<1, Int>` 与 `Tensor<1, Real>` 都是精确存储，不能同时拥有同一类的实例。

### 3.8.5 派生

`derive Eq, Show, Hash for Vec2` 只对结构类开放：`Eq`、`Ord`、`TotalOrd`、`Show`、`Hash`；
派生其他类是 `E0317`。派生实例生成在类型所属模块（受孤儿规则约束），`Ord` 按字段声明
顺序做字典序；含函数字段时失败，失败的字段类型与类名出现在 `E0317` 中。

### 3.8.6 命题 `Prop`

`if`、`match`、`assert` 只接受 `Bool`。`Real` 的比较产生命题 `Prop`：

```
(==) (!=) (<) (<=) (>) (>=) : Real[d] -> Real[d] -> Prop
decide   : Prop -> Option<Bool>
assuming : (Prop, () -> a) -> a
by_cases : (Prop, () -> Real[d], () -> Real[d]) -> Real[d]
```

- `Int`、`Float`、`String`、记录、变体的比较返回 `Bool`；`Real` 的比较返回 `Prop`，不在
  运行时“猜”布尔值。`Prop` 支持 `&&`、`||`、`!`，结果仍是 `Prop`。
- `decide(p)` 能证明或否证时返回 `Some(true/false)`，否则 `None`。
- `assuming(p, thunk)` 把 `p` 作为假设加入 `thunk` 内的符号计算；`p` 可判定为 `false` 时
  `E0804`。目前只支持单元谓词（`x > 0`、`x >= 0`、`x != 0`、`x ∈ ℤ`）。
- `by_cases(p, yes, no)` 在 `p` 可判定时选分支，否则构造符号 `ite` 项（分段函数的符号
  推导）。
- 在 `if` 中使用 `Prop` 是 `E0302`（symbolic proposition is not a `Bool`）；先 `decide`，或改写为
  `assuming`/`by_cases`。

这一区分避免“符号 Bool 伪装成 Bool”：`Bool` 的穷尽性、`Eq` 定律和 `if` 的确定性都成立。

## 3.9 效果：`IO`

### 3.9.1 类型

`IO : Type -> Type`，`pure : a -> IO a`；`do` 去糖为 `bind` 链（`bind` 不是用户可调用的
函数）。`IO` 是单一效果类型；capability（`fs`/`net`/`env`/`rand`/`gpu`）是运行时权限，
不是类型参数（§13.9.6），标准库签名因此不被 effect row 侵入，而插件与沙箱仍可声明
所需能力。

`IO a` 是动作**描述**，只有运行时执行 `main` 时才发生副作用。纯函数不能执行 `IO`。

### 3.9.2 `do` 的类型规则

```
Γ ⊢ e1 : IO a     Γ, x: a ⊢ rest : IO b
----------------------------------------
Γ ⊢ (x <- e1; rest) : IO b

Γ ⊢ e : IO a     a 不是 ()
--------------------------------
Γ ⊢ (e; rest) : IO b      （丢弃 e 的结果，警告 W0202）
```

`do` 内的 `let` 是纯绑定。

### 3.9.3 错误

可恢复错误用 `Result<T, E>` 与 `?`；可能缺失用 `Option<T>`（`?` 也适用）；IO 失败通过
`Result` 返回（如 `read_file`）。没有 `try/catch`，也没有用户可调用的 `panic`；不可恢复
的运行时错误终止程序并打印调用栈（§6.9）。

### 3.9.4 纯度边界

纯上下文中出现 `IO` 是 `E0313`；`IO` 动作可以捕获纯值，反之不行；没有 `unsafePerformIO`；
`main : IO Unit` 是唯一入口，库模块导出的 `IO` 动作由调用者执行。

## 3.10 字面量与默认值

### 3.10.1 字面量类型

字面量是**表示多态**的：带 `Literal` 约束，在期望类型处实例化，在声明结束处默认化。

| 字面量 | 默认类型 | 可实例化为 |
|---|---|---|
| `123` | `Int` | `Int`、`Real`、`Float`、`Complex` |
| `1.5`、`1e3` | `Float[1]` | `Float[1]`、`Real[1]`（精确十进制） |
| `3 m` | `Real[L]` | `Real[L]`、`Float[L]`、`Complex[L]` |
| `3.0 m` | `Float[L]` | `Float[L]`、`Real[L]` |
| `25 °C` | `Point[Θ]` | 仅 `Point[Θ]`（仿射，固定 Float） |
| `oo` | `Real[1]` | `Real[1]`、`Float[1]`；带量纲写 `oo * 1 m` |
| `nan` | `Float[1]` | `Float[1]` |

- 整数字面量在期望 `Real`/`Float` 处自动转换（`df(2)` 中 `2` 是 `Real`）。小数字面量在
  期望 `Real` 处按**精确十进制**转换：`1.5` 是 `3/2`，因此 `x + 1.5`（`x : Real`）保持精确。
- 单位字面量的表示由系数决定：整数系数默认 `Real`，小数系数默认 `Float`，期望类型可覆盖
  （`fall_time(20 m)` 在 `h : Float[L]` 处把 `20 m` 实例化为 `Float[L]`）。
- `0` 不是量纲多态的，默认 `Int`；零长度写 `0 m`，浮点零写 `0.0`（§4.7）。

### 3.10.2 默认化

声明结束时仍未解决的变量按顺序默认：

1. 整数字面量产生的 `Rep` 变量 → `Int`；2. 小数字面量产生的 → `Float`；3. 单位字面量
   的 → `Real`（整数系数）或 `Float`（小数系数）；4. 其他 `Rep` 变量 → `Real`；5. `Dim` 变量
   → `1` 并警告 `W0301`；6. `Nat` 变量**不**默认，无法由约束解出则 `E0304`；7. 行变量 →
   空行。

默认化只影响未被方案或词法环境量化的变量，并先传播所有数值约束；`Literal` 约束选择合法的
默认类型，不能把小数字面量读成 `Int`。函数体里依赖泛型输入的私有字面量，在输入确定后优先
按同一数值约束分量中输入的算术 `join` 读取，再尝试其字面量默认表示；无关参数不参与，调用
者的期望结果也不能另选这个读取。直接使用泛型参数的字面量（`fn zero<r: Rep>(): r = 0`）
保留自己的合法表示域。实际构造并捕获的数据保持同一类型实例，函数别名传递构造要求，不会把
同一个捕获数值同时解释成 `Int` 和 `Float`。

### 3.10.3 类型标注优先

有期望类型时字面量按期望类型检查，不触发默认化：

```aevia
let x: Float[L] = 0.0 m    # 零长度，Float
let n: Int = 0
let y = 0                  # 默认 Int
let z: Real[L] = 0 m       # 零长度，精确
```

仅含字面量的 `+ - * /` 表达式及正负号也接受这个局部上下文：`let exact: Real = 0.1 + 2`
精确等于 `21/10`。规则不越过变量或调用：`fn f(x) = x + 1.0` 的 `f(1)` 是 `Float`，不能由
调用者标注成 `Int`。

### 3.10.4 字面量折叠

折叠是编译期求值，必须与运行期给出同一个值，是优化而不是另一套算术。

1. **一个字面量只舍入一次。** 十进制文本是精确有理数；在 `Float` 位置取最近的 f64。单位
   字面量先在精确算术里乘尺度再舍入：`1.1 km : Float[L]` 是最接近 `1100` 的 f64，不是
   `1.1 * 1000.0` 舍入两次；带 π 的尺度（`deg`）把正确舍入的有理部分乘以 f64 的 `pi^k`。
2. **闭合的字面量算术树是编译期常量。** 只由字面量、`+ - * /` 和正负号组成的表达式按其
   静态表示逐步求值一次；全是 `Float` 字面量的无量纲树在 IR 降级时按同样的 IEEE 运算折叠
   为常数。故 `0.1 + 0.2 : Float` 无论写在行内还是经 `let`，都是 `0.30000000000000004`。
3. **精确性来自类型，而不是折叠。** `1.0 + 2` 无期望类型时是 `join(Int, Float) = Float`，值
   为 `3.0`；小数字面量与精确 `Real` 操作数相遇时按精确十进制读取：`1/3 + 0.1` 是 `13/30`。

## 3.11 动态量纲 `Dyn`（摘要）

静态量纲无法表达运行时指数 `x^n`（`n : Int`）和形状相关的量纲（`det(A)` 的 `d^n`）。
`Dyn<ρ>` 是显式逃逸（保留表示）：

```
dyn       : Scalar<ρ,d> -> Dyn<ρ>                       # d 具体
check     : Dyn<ρ> -> Result<Scalar<ρ,d>, DimError>     # d 具体
dyn_with  : (Scalar<ρ,d>, DimValue) -> Dyn<ρ>
check_with: (DimValue, Dyn<ρ>) -> Result<Scalar<ρ,1>, DimError>
dim_of    : Dyn<ρ> -> DimValue
```

`dyn`/`check` 是类型指导原语：调用点的 `d` 必须具体，编译器把它物化成 `DimValue`；泛型
代码用 `dyn_with`/`check_with` 显式传入运行时量纲。`Dyn` 不能隐式转为 `Scalar`，完整规则
见 §4.9。

## 3.12 类型错误

| 代码 | 情况 |
|---|---|
| `E0301` | 未绑定名字 |
| `E0302` | 类型不匹配 |
| `E0303` | occurs check（无限类型） |
| `E0304` | 无法推断 / 歧义 |
| `E0305` | 不是函数 |
| `E0306` | 记录缺少字段 |
| `E0307` | 构造器模式元数不匹配 |
| `E0308` | 模式与 scrutinee 不匹配，或模式重复绑定 |
| `E0309` | 非穷尽匹配 |
| `E0310` | 缺少类型类实例 |
| `E0311` | 重叠实例 |
| `E0312` | 非正递归类型 |
| `E0313` | 纯上下文中出现 `IO` |
| `E0314` | 张量元素类型或秩无法推断 |
| `E0315` | 调用的参数数量不匹配 |
| `E0316` | 重复定义 |
| `E0317` | 类/实例/派生错误 |

量纲错误是 `E04xx`（第 04 章），单位错误是 `E05xx`（第 05 章）。

## 3.13 关键推断规则

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

实现顺序：解析 → 名字解析 → 收集签名 → 推断/检查 → 默认化 → 导出接口。量纲统一规则见
下一章。
