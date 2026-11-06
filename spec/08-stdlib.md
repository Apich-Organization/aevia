# 08 — 标准库

标准库是语言的一部分：它定义 Prelude、数学函数、单位、线性代数、微积分、
统计、数据、绘图和仿真。本章给出**规范性签名**；实现可以增加函数，但不能
改变已有函数的类型或量纲规则。

签名中的 `ρ` 是数值表示（`Int`、`Real`、`Float`、`Complex`），`d`、`e` 是
量纲，`n` 是秩，`a`、`b` 是类型。`Scalar<ρ,d>` 在表面写成 `Real[d]` 等。

## 8.1 Prelude

Prelude 自动导入，无需 `open`。

### 8.1.1 函数组合

```
id       : a -> a
const    : (a, b) -> a
compose  : ((b -> c), (a -> b)) -> (a -> c)
flip     : ((a, b) -> c) -> ((b, a) -> c)
curry    : ((a, b) -> c) -> (a -> (b -> c))
uncurry  : ((a -> (b -> c))) -> ((a, b) -> c)
apply    : ((a -> b), a) -> b
```

`compose(f, g)(x) = f(g(x))`。`|>` 和 `<|` 是语法糖，不是函数。

### 8.1.2 布尔与比较

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

`by_cases(p, yes, no)` 在 `p` 可判定时选分支；不可判定时构造符号
`ite` 项（用于分段函数的符号推导）。`assuming` 只支持单元谓词
（`x > 0`、`x >= 0`、`x != 0`、`x ∈ ℤ`）。

`Real` 没有 `Eq`/`Ord` 实例；它的比较是 `Prop`。`Int`/`Float`/`String`
返回 `Bool`。`TotalOrd` 用于 `Set`/`sort`，`Float` 需要 `TotalFloat` 包装。

### 8.1.3 数值

```
abs      : Scalar<ρ,d> -> Scalar<ρ,d>        # ρ ≠ Complex
sign     : Scalar<ρ,d> -> Int                 # ρ ≠ Complex
min      : (Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,d>   # ρ ≠ Complex
max      : (Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,d>   # ρ ≠ Complex
clamp    : (Scalar<ρ,d>, Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,d>
floor    : Scalar<ρ,d> -> Scalar<ρ,d>         # ρ ≠ Complex
ceil     : Scalar<ρ,d> -> Scalar<ρ,d>         # ρ ≠ Complex
round    : Scalar<ρ,d> -> Scalar<ρ,d>         # ρ ≠ Complex
trunc    : Scalar<ρ,d> -> Scalar<ρ,d>         # ρ ≠ Complex
mod      : (Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,d>   # ρ ≠ Complex
rem      : (Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,d>   # ρ ≠ Complex
div      : (Int, Int) -> Int
quot     : (Int, Int) -> Int
gcd      : (Int, Int) -> Int
lcm      : (Int, Int) -> Int
factorial: Int -> Int
to_float : Scalar<ρ,d> -> Result<Float[d], EvalError>   # 符号/自由符号时失败
to_real  : Scalar<ρ,d> -> Real[d]
to_int   : Scalar<ρ,1> -> Int
zero     : Scalar<ρ,d>                 # 该表示/量纲的零，用于 fold/sum
rng      : Int -> (() -> Float[1])     # 纯、确定；返回“下一个数”函数
```

`min`/`max` 是二元函数；张量归约用 `min_all`/`max_all`。`zero` 是
`fold((+), zero, xs)` 的起点；`sum([])` 内部使用对应表示/量纲的零。

`rng(seed)` 是**纯**的伪随机数生成器：同一个 seed 在同一程序里给出同一
序列，与执行顺序/线程数无关。它返回一个无参函数，每次调用给出下一个
`Float[1]`；`random()`（§6.8.2）是 `IO` 版本，不保证可复现。

### 8.1.4 字符串

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

字符串索引按 grapheme cluster 计；`chars(s)` 返回 `Tensor<1, String>`。

### 8.1.5 容器

```
map      : ((a -> b), Tensor<n, a>) -> Tensor<n, b>
map2     : ((a, b) -> c, Tensor<n, a>, Tensor<n, b>) -> Tensor<n, c>
par_map  : Parallel a => ((a -> b), Tensor<1, a>) -> Tensor<1, b>
filter   : ((a -> Bool), Tensor<1, a>) -> Tensor<1, a>
fold     : ((b, a) -> b, b, Tensor<1, a>) -> b
fold_left: 同上
zip      : (Tensor<1, a>, Tensor<1, b>) -> Tensor<1, (a, b)>
zip_with : ((a, b) -> c, Tensor<n, a>, Tensor<n, b>) -> Tensor<n, c>
enumerate: Tensor<1, a> -> Tensor<1, (Int, a)>
range    : (Int, Int) -> Tensor<1, Int>
range    : (Int, Int, Int) -> Tensor<1, Int>
take     : (Int, Tensor<1, a>) -> Tensor<1, a>
drop     : (Int, Tensor<1, a>) -> Tensor<1, a>
reverse  : Tensor<1, a> -> Tensor<1, a>
sort     : TotalOrd a => Tensor<1, a> -> Tensor<1, a>
sort_by  : ((a, a) -> Ordering, Tensor<1, a>) -> Tensor<1, a>
sort_float : (Tensor<1, Float>, Bool) -> Tensor<1, Float>   # NaN last/first
concat   : (Tensor<1, a>, Tensor<1, a>) -> Tensor<1, a>
flatten  : Tensor<n, Tensor<m, a>> -> Tensor<n+m, a>   # 仅规则嵌套
any      : ((a -> Bool), Tensor<1, a>) -> Bool
all      : ((a -> Bool), Tensor<1, a>) -> Bool
find     : ((a -> Bool), Tensor<1, a>) -> Option<a>
sum      : Tensor<n, Scalar<ρ,d>> -> Scalar<ρ,d>
prod     : Tensor<n, Scalar<ρ,1>> -> Scalar<ρ,1>
mean     : Tensor<n, Scalar<ρ,d>> -> Scalar<div_rep(ρ),d>
length   : Tensor<n, a> -> Int
shape    : Tensor<n, a> -> Tensor<1, Int>
rank     : Tensor<n, a> -> Int
```

`length` 对 `String` 和 `Tensor` 重载，由类型类 `Sized` 解析。

`par_map(f, xs)` 并行映射，结果顺序与输入一致（线程数不改变结果）。它
要求 `Parallel a`：`Int`、`Float[d]` 及其张量可以并行，符号 `Real` 不并行
（§3.8.4/§6.10）。元素类型由调用点决定，所以约束在调用点检查，而不是一个
全局实例。

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
is_none  : Option<a> -> Bool
map_err  : ((e -> f), Result<a, e>) -> Result<a, f>
```

`?` 是语法，不是函数。

### 8.1.7 显示

```
show     : Show a => a -> String
repr     : Show a => a -> String
to_latex : Show a => a -> String
to_typst : Show a => a -> String
print    : Show a => a -> IO Unit
println  : Show a => a -> IO Unit
eprintln : Show a => a -> IO Unit
```

> **实现状态（2026-11-04）**：`par_map`/`rng` 已进入 §3.8.4 的内置类表、
> Prelude 签名与运行时 dispatch；checker 在调用点检查 `Parallel a`
> （`Int`/`Float` 及其张量可以并行，符号 `Real` 被静态拒绝，§3.8.4）。
> `rng` 是纯函数，返回“下一个数”闭包。`par_map` 的结果顺序与输入一致；
> 当前解释器是 `!Send`，这一构建里 map 实际按顺序执行——这是
> §9.7.2 rayon 措辞的实现偏差，不是结果语义的偏差（顺序/确定性不变）。

### 8.1.8 程序参数

```
args : () -> Tensor<1, String>
```

`args()` 返回宿主传给程序的参数向量，**只包含 `aevia run file -- a b` 中
`--` 之后的参数**（`--` 本身不是参数）；没有参数时是空张量
（`length(args()) == 0`，打印 `[]`）。它按位序保留参数的原始字符串，不做
shell 解析（引号、空格、通配符已由 shell/宿主处理）；值在进程内不变，多次
调用返回同一组。类型是 `Tensor<1, String>`，因此可以 `map`/索引/`length`。

> **实现状态（2026-11-05）**：已实现——`Interp::set_args` 设置参数向量，
> builtin 返回 rank-1 `Tensor<1, String>`；`aevia-core` 的
> `io_taxonomy` 测试 `args_are_the_host_set_vector` 断言空向量（`[]`）与
> 两个参数（`["one", "two"]`）的情形；CLI 的 `run -- args…` 转发已接线
> （实测 `aevia run p.aev -- a b` → `args() = [a, b]`，无 `--` 时 `[]`）。

## 8.2 Math

```
pi       : Real[1]
e        : Real[1]
i        : Complex[1]
oo       : Real[1]
nan      : Float[1]
```

`oo` 是 `Real[1]`，不是量纲多态；`oo m` 是 `Real[L]`。`-oo` 由一元负号
得到。`nan` 是 `Float[1]`。**无穷的运算语义见 §6.3.4**：那里给出哪些边界
值会折叠（`ln(0)=-oo`、`ln(oo)=oo`、`exp(oo)=oo`、`exp(-oo)=0`）、哪些含
`oo` 的 `+ - * / ^` 保留为符号项（精确相位不发明 NaN），以及 `Float` 相位
按 IEEE 给出 `inf`/`nan`；本章只声明 `oo`/`nan` 的类型与量纲。

### 8.2.1 初等函数

```
sqrt     : Scalar<ρ,d> -> Scalar<ρ,d^(1/2)>        # ρ ∈ {Real,Float}
cbrt     : Scalar<ρ,d> -> Scalar<ρ,d^(1/3)>        # ρ ∈ {Real,Float}
exp      : Scalar<ρ,1> -> Scalar<ρ,1>
ln       : Scalar<ρ,1> -> Scalar<ρ,1>
log      : Scalar<ρ,1> -> Scalar<ρ,1>              # 自然对数
log2     : Scalar<ρ,1> -> Scalar<ρ,1>
log10    : Scalar<ρ,1> -> Scalar<ρ,1>
sin cos tan          : Scalar<ρ,1> -> Scalar<ρ,1>
asin acos atan        : Scalar<ρ,1> -> Scalar<ρ,1>
atan2                : (Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,1>
sinh cosh tanh        : Scalar<ρ,1> -> Scalar<ρ,1>
asinh acosh atanh     : Scalar<ρ,1> -> Scalar<ρ,1>
hypot                : (Scalar<ρ,d>, Scalar<ρ,d>) -> Scalar<ρ,d>
```

### 8.2.2 特殊函数

```
gamma        : Scalar<ρ,1> -> Scalar<ρ,1>
lgamma       : Scalar<ρ,1> -> Scalar<ρ,1>
beta         : (Scalar<ρ,1>, Scalar<ρ,1>) -> Scalar<ρ,1>
erf erfc     : Scalar<ρ,1> -> Scalar<ρ,1>
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

特殊函数的参数必须无量纲；结果无量纲。

## 8.3 函数工具

```
curry    : ((a, b) -> c) -> (a -> (b -> c))
uncurry  : (a -> (b -> c)) -> ((a, b) -> c)
partial  : ((a, b) -> c, a) -> (b -> c)
```

`partial(f, a)` 是 `f(a, _)` 的函数版本。

## 8.4 LinearAlgebra

```
dot      : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Scalar<ρ,d*e>
matmul   : (Tensor<2, Scalar<ρ,d>>, Tensor<2, Scalar<ρ,e>>) -> Tensor<2, Scalar<ρ,d*e>>
matvec   : (Tensor<2, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Tensor<1, Scalar<ρ,d*e>>
vecmat   : (Tensor<1, Scalar<ρ,d>>, Tensor<2, Scalar<ρ,e>>) -> Tensor<1, Scalar<ρ,d*e>>
outer    : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Tensor<2, Scalar<ρ,d*e>>
transpose: Tensor<n, a> -> Tensor<n, a>          # 反转轴序
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
zeros    : (Int, ..., Int) -> Tensor<n, Float>       # n = 参数个数
zeros_of<shape>() : Tensor<shape, Float>              # 细化形状构造
ones_of<shape>()  : Tensor<shape, Float>
ones     : (Int, ..., Int) -> Tensor<n, Float>
fill     : (Scalar<ρ,d>, Int, ..., Int) -> Tensor<n, Scalar<ρ,d>>
linspace : (Scalar<ρ,d>, Scalar<ρ,d>, Int) -> Tensor<1, Scalar<ρ,d>>
reshape<shape> : Tensor<shape0, τ> -> Result<Tensor<shape, τ>, ShapeError>
reshape_dyn : (Tensor<n, τ>, Tensor<1, Int>) -> Result<Tensor<n, τ>, ShapeError>
flatten  : Tensor<n, a> -> Tensor<1, a>
at       : (Tensor<n, a>, Int) -> Tensor<n-1, a>     # 要求 n >= 1
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

`zeros<Float>(3, 4)` 的 `n=2`；`zeros<Int>` 显式指定元素类型。`zeros`/
`ones`/`fill` 是**内建可变参数**：`n` 由 `Int` 实参的个数决定。它们可从
`Tensor` 或 `LinearAlgebra` 导入并通过值别名再导出；类型参数可在调用前
或调用时给出（`let z = zeros<Int>; z(2, 3)`）。局部同名定义照常遮蔽内建；
构造器经泛型恒等函数传递时保留身份。相同类型特化的 `zeros` 与 `ones`
可作为同一条件表达式的分支；运行时按实际值选择操作。`fill` 属于另一
签名族，不同特化也不能混合。需要固定函数签名的高阶参数不能直接接受可变
参数构造器，应写固定参数的 lambda，例如 `fn (n) => zeros(n)`。`reshape<[n1, ..., nk]>(a)` 的目标形状
显式给出；元素数在编译期（形状已知时）或运行时检查。`reshape_dyn(a,
shape)` 用运行时形状向量，返回 `Result`。

`@` 按秩分派：`A @ B`（2×2）、`A @ x`（2×1）、`x @ A`（1×2）、
`x @ y`（1×1 点积）。`matmul` 只做 2×2；`matvec`/`vecmat`/`dot` 是显式
版本。

`det` 只接受无量纲；带量纲用 `det_unit`：

```
det_unit : (Tensor<2, Scalar<ρ,d>>, Unit[d]) -> Dyn<ρ>

type LinAlgError = | Singular | NotConverged | NotPositiveDefinite | ShapeMismatch
```

### 8.4.1 稀疏

```
type Sparse<F, shape, τ>

from_dense<F> : Tensor<shape, τ> -> Sparse<F, shape, τ>
to_dense   : Sparse<F, shape, τ> -> Tensor<shape, τ>
nnz        : Sparse<F, shape, τ> -> Int
transpose  : Sparse<F, [m,n], τ> -> Sparse<F, [n,m], τ>
matmul     : Sparse<F, [m,k], Scalar<ρ,d>> × Sparse<G, [k,n], Scalar<ρ,e>>
           -> Sparse<Csr, [m,n], Scalar<ρ,d*e>>
```

`Csr`/`Csc`/`Coo`/`Block`/`Bcsr` 是内建 `Format` 标签（§3.1），不是值构造器。`Sparse` 是核心类型构造器，`F` 是 `Format` kind；`from_dense`/`to_dense`/`nnz` 是内建函数（实现为一个 CSR 记录往返，`stdlib` 测试覆盖），不是可实例化的 `Layout` 类型类。**v1 的稀疏值就是 `from_dense` 返回的 CSR 记录**（`rows`/`cols`/`values`/`col_indices`/`row_ptr`），不是运行时存储类：checker 的 `Sparse`/`TyKind::Sparse` 是类型层镜像，并**擦除为对应的 dense `Tensor`**（所以 `transpose` 只有 dense `Tensor<2>` 的 typing，没有 `Sparse` 重载）；`aevia-core` 的 `Storage` 没有 sparse 变体，kernel 的稀疏值过桥时被 densify。存储类级别的通用稀疏算法（含 `transpose`/`matmul`/`@` 布局分派）以命名 capability gap `storage.sparse` 记录（§13.9.5），是**运行时缺口**而不是类型层承诺。`@` 在 dense/sparse
混合时按布局分派；不隐式互转。格式当前是封闭集合；用户自定义格式要等
RSSN 开放 `Format`（§13.9.5）。RSSN 的 `TypeExpr::Sparse` 只有 rank，所以
`shape` 细化在 Aevia 侧检查，边界降为 rank + 运行时 shape。RSSN 的
`term_of_value` 不接受 `Value::Sparse` 进入项图（`manifest.value_kinds`
里 `in_term_graph = false`），所以稀疏运算必须走 `eval_dyn` 内核或 Aevia
自己的布局内核；不能把稀疏值直接喂给通用重写算子。稀疏存储由 kernel 的
`faer`（`sparse-linalg`）后端承担；宿主不接触具体格式的存储布局。

> **实现状态（2026-11-05）**：`from_dense`/`to_dense`/`nnz` 已实现并通过
> CSR 记录往返测试；**没有运行时 `Sparse` 值类型**（记录约定，不是存储类）。
> `transpose`/`matmul` 的稀疏版本和 `@` 的 dense/sparse 布局分派**尚未
> 实现**——属 `storage.sparse` 命名 capability gap（§13.9.5），不是静默
> 遗漏；当前只对 dense 张量分派，稠密矩阵仍可走 `faer`。用户自定义
> `Format` 也不可用。

## 8.5 Calculus

```
diff          : (Real[d] -> Real[e]) -> (Real[d] -> Real[e/d])
diff_at       : ((Real[d] -> Real[e]), Real[d]) -> Real[e/d]
numeric_deriv : (Float[d] -> Float[e]) -> (Float[d] -> Float[e/d])   # 数值/AD
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

`diff`/`antiderivative`/`defint`/`limit`/`series`/`grad` 走符号相位，参数是
`Real`。数值数据用 `numeric_deriv`/`quadrature`/`grad_f`/`sum_f`，走 `Float` 内核
（有限差分/自动微分/求积），不把 `Float` 数据转成 `Real`。

`sum` 的索引是无量纲的 `Real`，所以 `k` 可以是符号；`product` 只支持
无量纲，带量纲用 `product_dyn` 返回 `Dyn<Real>`。

## 8.6 Solve / Optimize

```
solve        : (Real[d] -> Real[e]) -> Result<Solutions<Real,d>, MathError>
solve_complex: (Real[d] -> Complex[e]) -> Result<Solutions<Complex,d>, MathError>
nsolve       : (Real[d] -> Real[e], Real[d]) -> Result<Real[d], MathError>
nsolve_f     : (Float[d] -> Float[e], Float[d]) -> Result<Float[d], MathError>
root         : 同上
solve_system : (Tensor<1, Real[d]> -> Tensor<1, Real[e]>, Tensor<1, Real[d]>)
             -> Result<Tensor<1, Real[d]>, MathError>
minimize     : (Tensor<1, Real[d]> -> Real[e], Tensor<1, Real[d]>)
             -> Result<Tensor<1, Real[d]>, MathError>
minimize_f   : (Tensor<1, Float[d]> -> Float[e], Tensor<1, Float[d]>)
             -> Result<Tensor<1, Float[d]>, MathError>
maximize     : (Tensor<1, Real[d]> -> Real[e], Tensor<1, Real[d]>)
             -> Result<Tensor<1, Real[d]>, MathError>    # 与 minimize 同型（Real 形式），不是 minimize_f
least_squares: ((Tensor<1, Real[d]> -> Tensor<1, Real[e]>), Tensor<1, Real[d]>)
             -> Result<Tensor<1, Real[d]>, MathError>
linsolve     : (Tensor<2, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>)
             -> Result<Tensor<1, Scalar<div_rep(ρ),e/d>>, LinAlgError>
```

`Solutions<ρ,d>` 是
`{ variable: Option<String>, roots: Tensor<1, Scalar<ρ,d>>,
   multiplicities: Tensor<1, Int>, conditions: Tensor<1, Condition>,
   complete: Bool }`，保留重数与参数条件；它不是 `Set`。

这个记录就是 RSSN `Solutions` 的记录（`Solutions::to_value`）：字段名、顺序和
含义一一对应，桥不做重新解释，因此内核说“没报告重数”就是空表，说“不完整”
就是 `false`。唯一的改形是 `variable`——RSSN 侧是符号（无未知量时是空串），
Aevia 侧是 `Option<String>`。

- `multiplicities` 是求解器给出的重数表，空表示**未报告**。RSSN 的
  `multiplicity()` 访问器对空表返回 1，但那是“没有信息时的默认回答”，
  不是声明；Aevia 保留空语义，不把它当“全 1”，也不从重复的根里反推重数。
  当前 kernel 在**能证明**时填它：方程是一元有理多项式、且每个根都是精确
  数时，用精确 square-free 分解给出重数（`(x-1)^2*(x+2)` → `[1, 2]`）；
  根是无理/符号表达式时保持空表（把符号根数值匹配到因式是猜测）。
- `conditions` 只填求解器**由构造就能声明**的条件，例如
  `solve(a*x + b = 0, x)` 的根 `-b/a` 带 `a != 0`；实性/分支条件不填。
  `complete = false` 时 `multiplicities`/`conditions` 都保持空表：沿途找到的
  单个根不足以声明重数或合法性。
- `complete` 是求解器的声明：`true` 表示它返回了穷尽形式（RSSN 的
  `list(...)`），返回单个值时 RSSN 给 `false`。桥原样传递这个值，不写成常量。
  `list(...)` 里的符号根由桥逐个展开为 `roots` 的元素（不再把整张表当作
  一个不透明根），所以“求解器给了穷尽列表”与“桥只拿到一个根”是可区分的。
  **`complete` 不是数学证明**，只是“求解器没有声明不完整”。

`Condition` 是 `{ kind: String, text: String, expression: Option<Value> }`：
`kind` 是 RSSN `ConditionKind` 的小写名（`nonzero`、`positive`、`real` 等），
`text` 是给人读的谓词，`expression` 是求解器保留的结构化条件值（RSSN 侧是
`Value`），求解器没有保留时该字段缺省。语言里没有与 `ConditionKind` 同名的
类型，所以元素类型是不透明的 `Condition`，字段按值读取。

`solve` 返回实解；`solve_complex` 返回复解。数值求解失败返回 `Err`。
`linsolve` 是线性方程组求解，避免与符号 `solve` 同名。

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

### 8.7.1 分布

分布是**名义类型**，不是多参数类型类（多参数类不在语言中，§3.8.1）：

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

type Normal<d>      = { mu: Real[d], sigma: Real[d] }
type Uniform<d>     = { lo: Real[d], hi: Real[d] }
type Exponential<d> = { rate: Real[1/d] }
type Gamma<d>       = { shape: Real[1], rate: Real[1/d] }
type Beta           = { alpha: Real[1], beta: Real[1] }
type ChiSquared     = { df: Real[1] }
type StudentT       = { df: Real[1] }
type Poisson        = { lambda: Real[1] }
type Binomial       = { n: Int, p: Real[1] }
```

`pdf`、`cdf`、`sample`、`sample_n` 是对这个封闭 ADT 的**内建重载**：

```
pdf      : (Distribution<d>, Real[d]) -> Real[1/d]
cdf      : (Distribution<d>, Real[d]) -> Real[1]
sample   : Distribution<d> -> IO Real[d]
sample_n : (Distribution<d>, Int, Option<Int>) -> Tensor<1, Real[d]>
```

`Normal<L>` 的 `pdf` 量纲是 `1/L`；`cdf` 无量纲；`sample` 返回 `Real[L]`。

**v1 不支持用户自定义分布。** 多参数类被移除（§3.8.1），而“分布参数 →
量纲”的映射需要类型函数（类似 `RateOf`），这是另一个设计；在没有它之前，
把 `class Distribution dist d` 留在规范里只会让用户写出无法实例化的代码。
用户自定义分布列在 §12.8.13 的扩展边界之外；标准库可以为新的分布形态增加
内建构造器，但那是库的封闭集合，不是语言扩展点。

## 8.8 FFT / Interpolate

```
fft      : Tensor<1, Complex[d]> -> Tensor<1, Complex[d]>       # 无归一化
ifft     : Tensor<1, Complex[d]> -> Tensor<1, Complex[d]>       # 1/N
fft_norm : (Tensor<1, Complex[d]>, Norm) -> Tensor<1, Complex[d]>
rfft     : Tensor<1, Float[d]> -> Tensor<1, Complex[d]>
irfft    : Tensor<1, Complex[d]> -> Tensor<1, Float[d]>
fftfreq  : (Int, Float[T]) -> Tensor<1, Float[1/T]>

interp1  : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Scalar<ρ,d>) -> Scalar<ρ,e>
interp2  : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Tensor<2, Scalar<ρ,f>>,
            Scalar<ρ,d>, Scalar<ρ,e>) -> Scalar<ρ,f>
spline   : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Scalar<ρ,d>) -> Scalar<ρ,e>
polyfit  : (Tensor<1, Scalar<ρ,1>>, Tensor<1, Scalar<ρ,e>>, Int) -> Tensor<1, Scalar<ρ,e>>
polyval  : (Tensor<1, Scalar<ρ,e>>, Scalar<ρ,1>) -> Scalar<ρ,e>
chebyshev_fit : (Tensor<1, Scalar<ρ,1>>, Tensor<1, Scalar<ρ,e>>, Int) -> Tensor<1, Scalar<ρ,e>>
```

`fft` 不归一化，`ifft` 除以 `N`，因此 `ifft(fft(x)) == x`。需要
`1/√N` 或其他约定时用 `fft_norm(x, Norm.Unitary)`。

```
type Norm = | Backward | Forward | Unitary
type AxisScale = | Linear | Log
```

`polyfit` 要求自变量无量纲；带量纲时先除以单位 `x/u`，系数保持 `y` 的
量纲。这是量纲系统对 API 的约束：多项式系数 `a_i` 的量纲是 `e/d^i`，
不同次幂不同，无法放进同质张量。

## 8.9 ODE

```
type OdeMethod = | Rk4 | Dopri5 | Radau | Bdf | Rosenbrock23
type OdeOpts<s> = {
    rtol: Float[1],
    atol: Float[1],
    method: OdeMethod,
    steps: Option<Int>,                 # Rk4 必填
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

`RateOf<s>` 是内建类型函数，递归定义：

```
RateOf<Scalar<ρ,d>>        = Scalar<ρ,d/T>
RateOf<Tensor<n, τ>>       = Tensor<n, RateOf<τ>>
RateOf<{ l1: τ1, ..., ln: τn }> = { l1: RateOf<τ1>, ..., ln: RateOf<τn> }
```

若 `s = { x: Float[L], v: Float[L/T] }`，则
`RateOf<s> = { x: Float[L/T], v: Float[L/T^2] }`。标量状态
`Float[1]` 的 `RateOf` 是 `Float[1/T]`。含 `Int`/`Bool`/函数字段的状态
不合法，报 `E0302`；把常量参数捕获在 `rhs` 的闭包里。编译器按字段检查；
不需要用户写实例，也不需要 `OdeState` 类。ODE 右端函数（例如 `rhs`）必须
是双参 `(t, s)`；时间参数即使不用也要写 `_`。

`events` 在 `Float[1]` 变号时终止并插值。`Dopri5`、`Radau`、`Bdf`、
`Rosenbrock23` 与定步长的 `Rk4` 都支持事件（RSSN 的
`solve_fixed_events`）；差别是定步长必须给 `steps`，事件定位精度也随
步长而定。`odeint_with` 返回实际到达的 `t`/`y`。`odeint` 是默认参数
（`rtol=1e-8`、`atol=1e-10`、`Dopri5`）的简写。

**展平契约**：桥接层按记录的**字段声明顺序**、深度优先、张量行主序，把
`s` 展平成 `Vec<f64>`；求解后按同一顺序还原。嵌套记录递归展平；`Tensor<n,
Float[d]>` 按行主序展平。当前只支持 `Float` 标量/张量字段；含 `Int`/
`Bool`/函数字段的状态报 `E0302`，把常量参数捕获在 `rhs` 闭包里。`Tensor<1,
s>` 用 `Boxed` 存储；数值内核只看到扁平 `f64`，Aevia 负责装箱/拆箱。

导入的 `odeint`/`odeint_with`/`solve_ivp`（以及 §8.9.1 的 `sdeint_*`，例如
`import Ode { odeint }`）**保持类型函数语义**：`RateOf<s>`/`DiffusionOf<s>`
的检查由入口点本身决定，标准库模块只是再导出，不改变签名；只有本地重新
定义同名函数才会遮蔽它。

## 8.9.1 SDE

SDE 与 ODE 共用 `RateOf`/展平契约（§8.9），但多了噪声结构与随机积分约定。
方法名本身编码积分约定：名字里带 `stratonovich` 的收敛到 Stratonovich 解，
其余是 Itô；没有与方法正交的 `calculus` 选项（名称不说明积分约定的方案
几乎都会被误用）。

```
type SdeMethod =
    | EulerMaruyama | Milstein | MilsteinStratonovich
    | HeunStratonovich | SrkPlaten | MilsteinNoncommutative

type SdeStructure =
    | Diagonal                     # 每个状态分量一个噪声通道，m = n
    | General(Int)                 # (m)；调用者声明噪声可交换
    | GeneralNoncommutative(Int)   # (m)；只有 MilsteinNoncommutative 合法

type SdeFixedOpts<T> = {
    structure: SdeStructure,
    method: SdeMethod,
    seed: Int,
    t0: Float[T], dt: Float[T], steps: Int,
    terms: Int,                    # Lévy 面积的 Fourier 模数；非交换方法之外传 0（显式，无默认值）
}

type SdeAdaptiveOpts<T> = {
    structure: SdeStructure,
    method: SdeMethod,
    seed: Int,
    t0: Float[T], t1: Float[T],
    rtol: Float[1], atol: Float[1],
    max_steps: Int,
    cells: Int,                    # 非交换自适应的 Brownian 网格 cell 数；其余传 0
    terms: Int,
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
                  ((Float[T], s) -> Tensor<2, Scalar<ρ, d/T^(1/2)>>),   # s = Float[d]
                  s, SdeOpts<T>) -> Result<SdePath<s>, SdeError>

wiener           : (Int, Int, Int, Float[T]) -> Tensor<2, Float[T^(1/2)]>
                   # (seed, m, steps, dt)；与 manifest 的 sde_wiener(seed, m, steps, h) 同序
sde_order        : SdeMethod -> { strong: Float[1], weak: Float[1] }
sde_ensemble_seed: (Int, Int) -> Int     # (seed, index)，纯函数
```

方法/噪声结构矩阵（内核文档明确没有强于一阶的方案，Milstein 系也只是弱
一阶，因此不声称弱二阶）：

| method | calculus | 允许的 structure | strong/weak |
|---|---|---|---|
| `EulerMaruyama` | Itô | `Diagonal`, `General`, `GeneralNoncommutative` | 1/2, 1 |
| `Milstein` | Itô | `Diagonal`, `General` | 1, 1 |
| `MilsteinStratonovich` | Stratonovich | `Diagonal`, `General` | 1, 1 |
| `HeunStratonovich` | Stratonovich | `Diagonal`, `General` | 1, 1 |
| `SrkPlaten` | Itô | `Diagonal`, `General` | 1, 1 |
| `MilsteinNoncommutative` | Itô | `GeneralNoncommutative` | 1, 1 |

`General`/`GeneralNoncommutative` 的 `m` 是噪声通道数；`sdeint_general` 在
运行时检查扩散矩阵形状是 `n × m`（`E0802`），`SdePath.noise` 报告实际
`m`（对角时是 `n`）。`sdeint_diag` 的记录状态用对角噪声；记录状态 + 一般
噪声是 `E1001("sde.general.record")`。

**步长策略**：`Fixed` 是定步长 `dt`、共 `steps` 步（时间网格
`t0 + k·dt`）；`Adaptive` 在 `[t0, t1]` 上用局部误差 `rtol`/`atol` 控制，
Brownian bridge 拒绝/重采（内核）。自适应只有 Itô 可交换
（`EulerMaruyama`/`Milstein`）与非交换 Milstein；Stratonovich 自适应是
`E1001("sde.interpretation")`，不是悄悄退化成定步长。内核的自适应控制器
返回 Milstein 解（其误差估计是 EM-vs-Milstein），所以 `Adaptive` +
`EulerMaruyama` 与 + `Milstein` 给出同一条受控路径，方法名保留请求值。

**确定性**：`seed` 必填，没有默认值。定步长可交换路径的增量来自语言 RNG
流（与 `rng(seed)` 相同的 LCG，正态用 Box–Muller），因此
`wiener(seed, m, steps, dt)` 与同 seed 的定步长路径消费**同一组数**；同
构建内可复现，不承诺跨平台逐位相同。非交换定步长与所有自适应路径需要内核
自己的 xoshiro 流（bridge 拒绝与 Lévy 面积在求解过程中采样），由 `seed`
播种，同样只承诺同构建可复现。`sde_ensemble_seed` 只依赖 `(seed, index)`，
所以集合成员与执行顺序/线程数无关；解释器闭包是 `!Send`，v1 的集合按顺序
运行。

**错误表**（不新增 Appendix-B 码）：`E0802` 形状/参数（含 `General` 的
`m` 与扩散矩阵不符、非均匀网格、缺失必填项）；`E0806` 状态非有限；
`E0807` 自适应步长塌缩或步数预算耗尽；`E1001` 未实现组合，note 名为
`core.sde`、`sde.noncommutative`、`sde.interpretation`、
`sde.general.record`。`SdeError` 是不透明错误类型（像 `SimError`），语言层
不暴露构造器。

**v1 明确不做**：events/根查找、dense output、用户自定义噪声结构、记录状态
+ 一般噪声、显式非均匀网格（定步长内核假设均匀步长，报 `E0802`；需要变
步长时用 `Adaptive`）、求解器 manifest 算子（求解器需要回调，无法越过
manifest ABI）。

> **实现状态（2026-11-04，SDE 集成轮，源码级端到端已验证）**：运行时与
> `aevia-std` 的 SDE 求解器已落地，`aevia-core` 的 `sde_runtime` 测试
> 10/10 通过；下面的组合也已用源码程序实际跑通（见 §8.20 的模块导入注）。
> 可用的组合：
>
> - **方法**：`EulerMaruyama`、`Milstein`、`MilsteinStratonovich`、
>   `HeunStratonovich`、`SrkPlaten`、`MilsteinNoncommutative` 都能积分；
> - **结构**：`Diagonal`（标量与记录状态）、`General(m)`（标量/rank-1 状态，
>   扩散矩阵 `n×m` 在运行时校验，例如标量状态 + `General(2)` 给 `1×2`
>   系数）可用；`GeneralNoncommutative(m)` 只与 `MilsteinNoncommutative`
>   组合。**记录状态 + 一般噪声在语言层是 `E0302`**：checker 拒绝
>   `sdeint_general` 的记录状态（help 指向 `sdeint_diag` 或 rank-1 状态），
>   所以源码程序不会走到运行时的 `E1001("sde.general.record")`；那个
>   `E1001` 仍是宿主/Rust 层直接调 builtin 时的守卫（`sde_runtime` 的
>   `general_noise_with_a_record_state_is_named_gap` 覆盖）；
> - **步长**：`Fixed`（`t0`/`dt`/`steps`，均匀网格）与 `Adaptive`
>   （`[t0,t1]`、`rtol`/`atol`、`max_steps`）；Stratonovich 自适应是
>   `E1001("sde.interpretation")`，不会静默退化成定步长；非交换定步长需要
>   `terms`。源码验证：`Fixed` 20 步对角路径给出 `steps=20, nfev=40`；
>   `Adaptive` 给出 `steps=11, rejected=1, nfev=60`；两条路径两次运行输出
>   逐位相同（同 seed/同构建）。源码级回归测试在
>   `aevia-core/tests/sde_language.rs`（5 个：构造策略并积分、可复现、
>   diffusion 量纲检查、缺失字段是静态错误、不支持组合是可恢复错误）；
> - **两条 RNG 流**：可交换定步长路径消费与 `wiener(seed,m,steps,dt)`/
>   `rng(seed)` 相同的语言 LCG 流；非交换定步长与自适应路径用内核 xoshiro
>   流，由 `seed` 播种；只承诺同构建可复现，不承诺跨平台逐位一致；
> - **确定性**：`seed` 必填；`sde_ensemble_seed(seed,index)` 是纯函数，成员
>   与执行顺序/线程数无关。**语言层没有 ensemble runner**：只有 seed 助手是
>   入口；`aevia-std` 内部的顺序 ensemble 尚未接线，所以不要从“集合按顺序
>   运行”推断存在可调用的集合 API；
> - **入口与类型**：`sdeint_diag`/`sdeint_general`/`wiener`/`sde_order`/
>   `sde_ensemble_seed` 是 Prelude 内建，并由 `Sde` 模块 re-export；
>   `Sde` 模块还声明本节的 `SdeMethod`/`SdeStructure`/`SdeFixedOpts`/
>   `SdeAdaptiveOpts`/`SdeOpts`/`SdePath`。checker 侧 `RateOf<s>` 与
>   `DiffusionOf<s>`（`SdeMethod`/`SdeError` 为不透明类型）已实现，drift /
>   diffusion 的签名按本节检查。**源码级策略构造可用**：`import Sde` 后写
>   `Sde.Fixed({ structure = Sde.Diagonal, method = Sde.EulerMaruyama, … })`
>   （或 `import Sde { Fixed, Diagonal, EulerMaruyama }` 后用短名）能通过
>   检查并运行——本节签名里的构造函数名来自 `Sde` 模块。选项记录由运行时
>   逐字段读取（`method`/`structure`/`seed`/`t0`/`dt`/`steps`/`terms` 等），
>   缺失或类型不对得到 `E0802`/`E1001` 的 `SdeError` 值，不会自动填默认值；
>   透明类型别名（如 `Physics.Ising.Params`）现在也可以经
>   `import { Params }`/`open Physics.Ising` 引入类型名（task-22 已闭合，
>   见 §7.2.2 与 §8.20），不再需要退回限定名。
> - **manifest**：SDE 面有三个 manifest 算子——两个噪声原语
>   `sde_wiener`/`sde_levy_areas`（overlay 精确签名 + `features.sde`）与
>   方案元数据 `sde_scheme_info(name: String) -> Tensor<1, Real>`（返回
>   `[strong_order, ito, commutative_only, adaptive, levy_areas]`，后四项是
>   1/0 flag；dim rule 是 `NonMetric`；方案名是 snake_case，覆盖 8 个方案：
>   `euler_maruyama` → `[0.5, 1, 0, 0, 0]`、`milstein` → `[1, 1, 1, 0, 0]`、
>   `milstein_stratonovich`、`milstein_noncommutative`、`srk_platen`、
>   `heun_stratonovich`，以及自适应的 `sde_adaptive`/
>   `sde_adaptive_noncommutative`（`adaptive = 1`）；未知名字保持未化简
>   请求）。它是**manifest-only 的元数据算子**，不是求解路径；求解循环仍是
>   Rust-only（host 回调，kernel CO-03）。它只给**强阶**（没有弱阶列），
>   与 §8.9.1 的 `sde_order`（返回 `{strong, weak}` 表）互补：需要弱阶时用
>   `sde_order`，需要 flag/自适应标记时用 `sde_scheme_info`。生成源是精确的
>   （`(String) -> Tensor<1, Real>`，无 `Dyn` 回退）。
>
> 数值格式方面：**分数维度指数的打印已修复**（2026-11-05）——`wiener` 的
> `Float[T^(1/2)]` 元素现在打印成 `s^(1/2)`（`Dim::to_si_symbol_string()`
> 现在渲染有理指数；同批测试覆盖 `s^(3/2)`、`1/s^(1/2)`、`m/s^2` 和真实
> `wiener` 值），见 §5.4.3。

## 8.10 PDE

PDE 求解器的参数是记录，包含网格、系数、边界条件。签名较长，这里给出
入口：

```
solve_poisson : PoissonSpec -> Result<Field, PdeError>
solve_heat    : HeatSpec -> Result<Tensor<1, Field>, PdeError>
solve_wave    : WaveSpec -> Result<Tensor<1, Field>, PdeError>
solve_advection : AdvectionSpec -> Result<Tensor<1, Field>, PdeError>
```

`Field` 是 `{ grid: Grid, values: Tensor<n, Float[d]> }`。系数和边界条件的
量纲在 `Spec` 记录中逐字段标注；求解器在运行时检查网格与系数量纲一致。
PDE 的完整类型规则见 `Pde` 模块文档；当前不把 PDE 规格放进核心类型系统。

## 8.11 Data

```
type Table<ρ>                  # 行多态静态 schema；ρ 是记录类型
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

`Table<ρ>` 是**行多态静态 schema**：`ρ` 的字段名、类型、量纲在编译期
已知。`read_csv<ρ>` 从表头单位推断字段类型；无法推断时报
`DataError.Schema`，用户可给显式 `ρ`。`read_csv_dyn` 返回
`Table<Dynamic>`，用于未知 schema；`to_table<ρ>` 在运行时校验并转换。

`column` 的字段名必须是**字符串字面量**；类型检查器把字面量提升为
类型级 label，在 `ρ` 中查找 `ρ[name]` 并返回其元素类型。字段不存在报
`E0306`；量纲不匹配报 `E0401`。非字面量用 `column_dyn`（返回
`Tensor<1, Value>`），不走静态 schema。`select`/
`filter`/`join` 保持行多态；`join` 的结果行是 `ρ + σ`（`+` 是类型级行
并集；冲突字段报 `E0208`）。

`read_npy` 的秩无法从文件静态推断，必须写 `read_npy<2>(path)`；省略是
`E0314`。类型参数目前必须是具体自然数；文件的实际秩不一致时，IO 动作
返回 `Err(DataError)`，不会把错误秩的张量交给程序。

CSV 表头可以带单位：`time [s]`、`velocity [m/s]`。`read_csv<ρ>` 解析
表头单位；`column(t, "time")` 在编译期用 `ρ` 的字段类型检查，运行时再做
一次防御性校验。没有表头单位时，列被当作无量纲；显式 `ρ` 可以覆盖，
但运行时校验失败报 `DataError.DimensionMismatch`。

> **实现状态**：`read_csv`/`write_csv`/`read_json`/`write_json` 与
> Table/column/select/filter/join 已实现。NPY 使用共享本地编解码；
> `read_npy<n>` 保存并验证具体秩，`write_npy` 接受任意秩的无量纲 Float
> 张量。支持 NPY 1.0/2.0/3.0、小端和大端 float32/float64、C 和 Fortran
> 排列；Fortran 输入转换为 Aevia 的行优先存储，写出使用小端 float64、
> C 排列。类型参数当前必须为具体自然数，未具体化的泛型秩报 `E0304`。
> NPY 文件不存在为 `E0901`，非法参数为 `E0902`，损坏、截断、秩不匹配
> 或大小溢出为 `E0903`，不支持的格式、dtype 或扩展名为 `E0904`；
> NPY 错误在 IO 动作执行后作为 `Err(DataError)` 返回。
> IO 错误分类也覆盖本节其他读写入口与 §8.12 绘图（`E0905`）；
> `aevia-core/tests/io_taxonomy.rs` 断言对应分类。

## 8.12 Plot

```
type PlotSpec
type Series

line      : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Series
line_with : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, PlotOpts) -> Series
scatter   : 同上（默认参数）
scatter_with : 同上（带 PlotOpts）
errorbar  : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>,
             Tensor<1, Scalar<ρ,e>>) -> Series
errorbar_with : 同上 + PlotOpts
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

`PlotOpts` 是完整记录：`{ label: Option<String>, color: Option<String>,
xunit: Option<String>, yunit: Option<String>, xscale: AxisScale, yscale: AxisScale,
xlim: Option<(Float, Float)>, ylim: Option<(Float, Float)> }`。
`line(x, y)` 使用默认值，自动从 `x`、`y` 的量纲生成轴标签
（`t / s`、`v / (m/s)`）；`line_with` 接受完整 `PlotOpts`；`label(series,
"v(t)")` 是设置标签的快捷方式。`plot` 默认写 `plot.html`；`save` 指定文件名。

> **实现状态（2026-11-05）**：当前只有 HTML/SVG-inline 一种渲染器——`plot`
> 和 `save` 生成同一段 HTML（内含 `<svg>` 元素）；`save` **不按扩展名**
> 切换格式，写 `.svg` 文件时内容仍是 HTML 文档。**写失败是 `E0905`**
> （"plot rendering failed"/保存失败），由 `aevia-core/tests/io_taxonomy.rs`
> 的 `a_failed_plot_write_is_e0905` 断言；`plot`/`save` 仍是 fatal 的 IO
> 动作（失败以诊断退出，不是 `Result`）。独立 `.svg` 输出与 PNG/JPEG
> 尚未实现（PNG/JPEG 需要额外依赖，见 kernel 的 IO 缺口），不要把它当作
> 已支持的格式。

## 8.13 Test

```
assert      : Bool -> IO Unit
assert_eq   : (Eq a, Show a) => (a, a) -> IO Unit
approx_eq   : (Scalar<ρ,d>, Scalar<ρ,d>, Scalar<ρ,d>) -> Bool
forall      : Arbitrary a => (a -> Prop) -> Bool
compile_fail: String -> Bool
```

`approx_eq(a, b, tol)` 要求三者同量纲；`tol` 是绝对容差。`forall` 对
`Arbitrary` 类型生成随机样本，对每个样本 `decide` 命题；任一为
`Some(false)` 则失败，全部为 `Some(true)` 才通过，出现 `None` 报
`E0804`。`compile_fail` 接受一段源码字符串，在**隔离**环境中编译（无导入、
无调用模块声明，仅 Prelude），产生至少一个错误时返回 `true`；它不检查
错误码——需要断言错误码时用 doctest 的 `compile_fail EXXXX` 标记
（§7.5.5/§9.12）。

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

不确定度按独立误差传播（平方和开根）。对 `a = (v₁, u₁)`、`b = (v₂, u₂)`：

- `a + b` / `a - b`：值 `v₁ ± v₂`，不确定度 `√(u₁² + u₂²)`；两边量纲必须
  相同，否则 `E0401`。
- `a * b`：值 `v₁·v₂`，不确定度 `√((u₁·v₂)² + (u₂·v₁)²)`（量纲 `d*e`）。
- `a / b`：值 `v₁/v₂`，不确定度 `√((u₁/v₂)² + (u₂·v₁/v₂²)²)`（量纲 `d/e`）。

相关误差需要协方差矩阵，核心库不提供。`Measurement` 是库类型，不是语言核心；
checker 把它当普通记录 `{ value, uncertainty }` **结构化识别**（不是新的
nominal 类型），字段名/顺序因此是边界的一部分：`+`/`-` 要求同量纲
（否则 `E0401`），`*`/`/` 组合量纲，标量缩放只在 `*`/`/` 一侧。
**注意**：上面的是**算子**（重载），不是名字；它们不在 §8.20 的 323 条名字
覆盖表里（名字审计看不到重载）。checker 与运行时**都已实现**这些运算
（checker 的 `binary_type` 按本节规则检查，运行时的 `apply_binary` 做独立
误差传播；`aevia-core/tests/measurement.rs` 覆盖）。实现还支持一元 `-` 与
“测量 ×/÷ 标量”（不确定度乘 `|scalar|`），这些是实现额外项；规范最小集是
上表四条二元运算。`Measurement + number`（或 `-`）不是运算符，仍是
`E0302`；只有 `*`/`/` 支持“测量与标量”的混合（`number / measurement` 也被
拒绝，提示用测量的值相除）。

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

`abs` 只用于实数；复数的模用 `magnitude`，避免 `abs : Complex[d] ->
Real[d]` 与 `abs : Scalar<ρ,d> -> Scalar<ρ,d>` 冲突。`complex_sqrt` 的结果
量纲是 `d^(1/2)`；类型显示/诊断里的维度变量指数保留下标（`:type
complex_sqrt` → `Complex[d0] -> Complex[d0^(1/2)]`，不会塌成 `d0^1`）。

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

这些模块直接映射 rssn 的规则集；完整列表见 `aevia doc Math.NumberTheory`。

## 8.17 Physics / Simulate

```
simulate : (String, Json) -> Result<Json, SimError>

to_json   : a -> Json                              # 动态边界：把可表示值编码为 JSON 值
from_json : Json -> Result<a, SimError>            # 动态边界：解码；a 在运行时擦除
```

`Json` 是不透明的动态值类型；`to_json`/`from_json` 是它唯一的语言内构造/
窄化通道，语义与 `Rssn.typed` + `from_value` 相同：`to_json` 不检查 `a`，
`from_json` 在运行期解码并把失败报成 `SimError`。类型化包装器用它们把记录
参数送进 `simulate` 再把结果取回（不是把 `simulate` 本身改成多态签名）。

类型化包装器（每个都是独立模块，顶层声明，无缩进）：

| 模块 | 参数 | 结果 | 入口 |
|---|---|---|---|
| `Physics.Ising` | `{ width: Int, height: Int, temperature: Float[1], mc_steps: Int, seed: Option<Int> }` | `{ spins: Tensor<2, Int>, magnetization: Float[1] }` | `run : Params -> Result<IsingResult, SimError>` |
| `Physics.Geodesic` | `{ black_hole_mass: Float[M], initial_state: Tensor<1, Float[1]>, proper_time_end: Float[T], initial_dt: Float[T] }` | `{ orbit: Tensor<2, Float[1]> }` | `run : Params -> Result<GeodesicResult, SimError>` |

实际源码形如 `src/Physics/Ising.aev`：

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

`simulate` 是动态入口，接受 rssn 的场景名和 JSON；类型化包装器在
`Physics.*` 中，逐字段标注量纲。场景名以下面 rssn 的 live `SCENARIOS`
列表为准（2026-11-04 测量为 29 个）：`ising`、`ising_wolff`、`geodesic`、
`gpe`、`schrodinger`、`fdtd`、`elasticity`、`lid_driven_cavity`、
`channel_flow`、`sod_shock_tube`、`kepler_symplectic`、`poisson_multigrid`、
`heat_nd`、`wave_nd`、`euler_fv`、`dg_conservation`、`acoustics_pml`、
`yee_fdtd`、`navier_stokes_mac`、`split_step`、`fem_simplex`、
`elasticity_fem`、`bem_laplace`、`sph`、`dpd`、`md_lj`、`spin_model`、
`nbody_gravity`、`kerr_geodesic`。宿主不应手写第二份场景清单：运行时
从 `rssn::sim::scenario::SCENARIOS` 读。

> **实现状态（2026-11-04）**：动态入口 `simulate` 已实现（`aevia-std`），
> `to_json`/`from_json` 已实现并类型化（§8.19 的动态边界）；§8.17 上表的
> 类型化包装器 `Physics.Ising`/`Physics.Geodesic` 已作为 Aevia 源模块实现
> （`crates/aevia-std/stdlib/Physics/`），`import Physics.Ising` 经嵌入模块
> 加载器可直接使用，`Physics.Ising.run` 有 4×4 端到端测试（`magnetization`
> 在 [-1, 1] 内）。

## 8.18 Device

```
type Device = | Cpu | Gpu(Int) | Distributed(Cluster)
type Cluster                      # 不透明运行时拓扑
type Mesh                         # 不透明分片拓扑
to_device : (Tensor<n,τ>, Device) -> IO (Tensor<n,τ>)
on        : (Device, () -> a) -> IO a
device_of : Tensor<n,τ> -> Device
```

设备是运行时值，不是张量类型参数；没有 `core.gpu` 时 `to_device`/`on`
报 `E1001`。分布式用 `Sharded<Mesh, Tensor<n,τ>>` 库类型。

## 8.19 Rssn 动态接口

```
Rssn.Version       : String
Rssn.ManifestHash  : String
Rssn.Capabilities  : Tensor<1, String>          # 本构建 enabled 的 capability 名
Rssn.Units         : Tensor<1, String>          # 单位名；单位值由生成的 Units 模块导出
Rssn.Constants     : Tensor<1, String>          # 常数符号；数值由生成的 Constants 模块导出
Rssn.ValueKinds    : Tensor<1, String>          # Value 的 17 个变体名

Rssn.call          : (String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.dynamic       : (String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.typed         : (String, Tensor<1, Value>, TypeRep) -> Result<Value, OpError>
Rssn.call_key      : (String, String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.resolve_overload : (String, Tensor<1, Value>) -> Result<String, OpError>
Rssn.overloads     : String -> Tensor<1, String>
to_value           : a -> Value
from_value         : Value -> Result<a, OpError>
type_rep<a>()      : TypeRep
to_callback        : 已检查的具体函数 -> Callback   # 编译器 intrinsic，函数参数个数不限
Rssn.call_ho       : (String, Tensor<1, Value>, Tensor<1, Callback>)
                   -> Result<Value, OpError>
Rssn.call_ho_key   : (String, String, Tensor<1, Value>, Tensor<1, Callback>)
                   -> Result<Value, OpError>
Rssn.register        : Plugin -> Result<PluginId, PluginError>
Rssn.register_plugin : Plugin -> Result<PluginId, PluginError>   # alias
```

`OpKey` 在 Aevia 侧表示成 `(rule_set, name, signature_hash)`：`call_key`
收 name + `signature_hash`（32 个 hex 字符），`resolve_overload` 成功时返回
`"rule_set.name@signature_hash"`，`overloads` 返回渲染后的签名文本。
`signature_hash` 全零表示该算子没有声明重载。`Rssn.call` 是
`Rssn.dynamic` 的别名（两者都收短名 + `Value` 数组），保留它是为了让
“调用一个算子”和“走动态边界”在源码里读起来不同。

生成的 typed 声明**不**调用 `Rssn.call_key`：它生成内部形式
`Rssn.invoke(name, signature_hash, args)`，语义同样是钉死 overload，但错误
通道不同——生成的 `fn` 必须返回声明类型，所以 kernel 失败会 raise 而不是
返回 `Err`。`Rssn.invoke` 不是 §8.19 的固定入口，是生成器与运行时之间的
形式。

`Rssn` 的成员由 Kernel Manifest 生成；上面的 `call`/`dynamic`/`call_key`
是固定入口。`signature_source = Declared | Overlay | Derived` 的算子都生成
`Rssn.<name>` 类型化声明（每条 overload 一个），并绑定生成时的
`signature_hash`，调用钉死该 overload；只有 `None`（叶子/无调用）与插件
算子生成 `dynamic`。别名生成 `Rssn.<alias>`；冲突时用
`Rssn.<rule_set>.<name>`（命名空间，不是运行时键）。SDE 的三个 manifest
算子——两个噪声原语 `sde_wiener`/`sde_levy_areas` 与方案元数据
`sde_scheme_info`——都是 overlay 精确签名，因此 `Rssn.sde_wiener`/
`Rssn.sde_levy_areas`/`Rssn.sde_scheme_info` 自动是 typed 声明
（`sde_scheme_info` 是 `(String) -> Tensor<1, Real>`，没有 `Dyn` 回退；
它是 **manifest-only 的元数据查询**，只给强阶与 flag，不返回弱阶——弱阶
用宿主侧 `sde_order`，见 §8.9.1）；SDE **求解循环**
（`sdeint_diag`/`sdeint_general`，§8.9.1）需要 host 回调，
不是 manifest 算子，也不会出现在 `Rssn` 里——它们由宿主侧实现。

本轮 manifest（`rssn 3.0.0-rc.1`，2026-11-05 实测）：**1382 个算子
（7 declared + 25 overlay + 1350 derived）、32 个 rule set、82 单位、
30 常数、overload 1568 条（1201×1 + 178×2 + 2×3 + 1×5）、
`manifest_hash = 8d2c3764…`**；数字会随 kernel 走，以 live manifest 为准。

`Derived` 是“由注册信息推出”的保守签名：`eval` → `(Real,…) -> Real`
（谓词 `-> Bool`），非叶子另加 `(Term,…) -> Term`。它不区分
`Tensor`/`Quantity`/`Int`，所以 IDE 要把它标成“保守推断”。本次测量（rssn
3.0.0-rc.1，2026-11-05）：1382 个算子里 7 个 declared、25 个 overlay、
1350 个 derived，32 个 rule set（overload 1201×1 + 178×2 + 2×3 + 1×5 =
1568 条签名）。这些数字是**测量**，不是常量；以 live manifest 为准。

固定入口的可达性与限制：

- `type_rep<T>()` 从具体的检查后类型生成不可伪造的 `TypeRep`；未解析的泛型
  不能在没有运行时类型字典时反射。`Rssn.typed` 执行动态调用，并用该见证检查
  返回值；不匹配或证据不足返回 `Err(OpError)`，成功结果仍为不透明 `Value`，
  需用 `from_value<T>` 验证并转换。可变参数构造的类型见证也可通过别名传递。
- `to_callback(f)` 对当前 evaluator 会话中的已检查源码闭包验证具体函数类型，
  构造不透明的 `Callback`。别名和已具体化的泛型函数保留此证据；内建函数或
  其他 native 函数可先包装在源码 lambda 中。回调桥支持具体数值、`Bool`、
  `String`，以及由它们递归组成的张量和闭合记录；具体量纲、秩和完整已知形状
  都会校验。空结构张量需要独立的元素类型及量纲证据，名义类型不能只凭标签推断。
  各算子可进一步限制签名，例如当前数值求解器使用 `Float` 标量与向量。
  调用在原线程同步执行，句柄在外层调用返回后失效；跨线程、同一作用域重入及
  回调错误均返回错误。空回调列表等价于动态调用。
- `Rssn.register`/`Rssn.register_plugin` 的类型正确，但 Aevia 源码无法构造
  `Plugin` 值：源码级 `@op` 打包（§13.5.2）未实现，因此运行时报
  `E1001("core.plugin.source")`。Rust host 插件仍可经嵌入 API
  （`Session::register`）注册。
- `core.callback` 标记上述受限的回调桥可用；`core.plugin.source` 仍为
  `enabled = false`。生成的高阶绑定保留显式 `(values, callbacks)` 动态接口；
  对声明精确、回调和结果结构均可认证的单重载，另生成钉住签名的 typed 别名。
  别名在调用时认证回调的具体签名，并按实例化结果类型验证返回值；无法证明
  类型或维度的签名仍只提供动态入口。

`Rssn.dynamic` 是**正式能力**，不是降级路径：插件算子、叶子算子、
实验性入口都走它。多注册时 RSSN 在运行时按
`Exact > Widened > Shaped > Generic > No` 选择，并列时报 `E1002`；参数不被
任何 overload 接受时报 `OpError::Type`（`check_args`）。标准集每个名字只有
1 个注册，所以这条路径不做注册级评分，但**会**做签名级校验。
`Rssn.call_key` 则不解析，直接执行 name + `signature_hash` 指定的
overload。

## 8.20 库的组织

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

标准库的模块在 `aevia-std` 包中；Prelude 自动导入，其他按需 `import`。
模块名就是路径：`import A.B` 解析 `A/B.aev`（§7），`import Math { sqrt }`
只引入 `sqrt`，`open Units.SI` 把 `Units.SI` 的导出名放进当前作用域。

> **实现状态（2026-11-04）**：标准库**模块树已落地**。
> （`crates/aevia-std/stdlib/**.aev`，**38** 个模块，含 §8.9.1 的 `Sde`；
> 经 `include_str!` 嵌入；`aevia_std::stdlib` 暴露
> `source`/`is_module_name`/`module_paths`。）
> 加载器在普通根目录解析不到模块时回退到嵌入模块，因此 `import Math {
> sqrt }`、`open Units.SI`、`import Data.Csv { read_csv, column, Table }`、
> `import Physics.Ising` 都能从用户源码直接使用；本地文件仍然优先于嵌入
> 模块。`Units.Constants` 由 live Kernel Manifest 生成；标准库的名字也仍然
> 直接作为 Prelude 内建可用。`Units/*` 是文档性模块：单位名本来就是
> ambient 的 `Unit[d]` 值，导出数量绑定会遮蔽它们并改变 `Tensor / Unit`
> 的语义，因此不导出（§5.3.4）。`Sde` 模块 re-export §8.9.1 的五个入口
> （`sdeint_diag`/`sdeint_general`/`wiener`/`sde_order`/`sde_ensemble_seed`）
> 并声明选项 ADT（`SdeMethod`/`SdeStructure`/`SdeFixedOpts`/
> `SdeAdaptiveOpts`/`SdeOpts`）；`SdePath` 的结果字段由推断访问；`Physics.Ising`/`Physics.Geodesic`
> 提供 §8.17 的类型化包装器。`spec/11` 的九个完整程序**全部**通过
> （11.1/11.2/11.3/11.4/11.5/11.6/11.7/11.8/11.10；11.2 的
> `RateOf<State>` 记录统一与 11.4 的 refined/rank-only shape 统一已在本轮
> 修复，测试 `spec_programs` 断言这张表）。**第 8 章的声明名已全部闭合**：
> **323/323** 有实现（`args` 计入后由 322 变 323），`stdlib_coverage` 的
> `chapter_eight_has_no_capability_gaps` 通过；NPY 也在本轮实现（§8.11）。
> **注意：323/323 只证明“名字”覆盖**——名字清单看不到参数类型、返回类型
> 与算子重载。本章声明与 checker scheme 的逐条对照由 `aevia-std` 的
> `spec_signatures` 审计（309 条声明；机械部分比较 presence/arity/return
> class，2 条已记录的 loose `sdeint_*` 方案留档，其余 type/order findings
> 见 `.workbuddy-ai/design/chapter8-signature-audit.md`，会随 task-28/29
> 的修复更新）；名字通过不代表签名逐字一致。
> **当前导入状态（task-22 已闭合）**：函数名、`Sde` 的**名义变体构造器**
> （`import Sde { Milstein, sde_order }`）与**透明类型别名**（`type X =
> { … }`，例如 `Physics.Ising` 的 `Params`）都可以经 `import { X }`/`open`
> 引入；源码里可以直接给这类别名类型写注解（`fn make(): Params = …`），
> 也可以继续用限定名 `Physics.Ising.Params`。SDE 的策略记录本来就是名义
> 变体，因此不受影响（§8.9.1 验证可写可运行）；§8.17 的包装器结果类型
> 现在也可以按名字写出来，而不必只靠推断。
>
> **生成模块与 manifest 同步**：`Rssn`/`Units`/`Constants` 是 Kernel
> Manifest 的生成视图。本轮 manifest（2026-11-05 实测）是 **1382 个算子
> （7 declared + 25 overlay + 1350 derived，其中 `sde_scheme_info` 是本轮
> 新增的 manifest-only 元数据算子）、32 个 rule set、82 单位、30 常数、
> overload 1568 条、`manifest_hash = 8d2c3764…`**。嵌入的
> `crates/aevia-std/stdlib/Units/Constants.aev` 来自 live manifest；
> manifest 变化后必须用
> `cargo run -p aevia-stdlib-gen -- --write`
> 重新生成，否则 `units_constants_is_the_live_manifest` 会失败（这是
> 生成视图与 live manifest 的诚实校验）。

`Rssn` 是**生成的模块**，不是手写库：构建时从 Kernel Manifest 生成
`Rssn.<op>` 类型化声明和 `Rssn.dynamic` 动态入口（§13.2）。`Rssn` 的
内容随 RSSN 版本变化，Aevia 源码不硬编码算子清单。`Sparse` 提供
`Csr`/`Csc`/`Coo`/`Block` 格式标签与内建 `nnz`/`to_dense`/`from_dense`
（`Layout` 类型类尚未实现，`matmul`/`transpose` 的稀疏版与 `@` 布局分派
未实现，见 §13.9.5）；`Device` 提供 `Cpu`/`Gpu`/`Distributed` 与
`on`/`to_device`（§13.9.4，无 `core.gpu` 时 `E1001`）。

下一章定义运行时：从源码到值，从值到 rssn 项，从 REPL 到 JIT。
