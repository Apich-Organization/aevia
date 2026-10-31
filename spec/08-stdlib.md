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
```

`min`/`max` 是二元函数；张量归约用 `min_all`/`max_all`。`zero` 是
`fold((+), zero, xs)` 的起点；`sum([])` 内部使用对应表示/量纲的零。

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

## 8.2 Math

```
pi       : Real[1]
e        : Real[1]
i        : Complex[1]
oo       : Real[1]
nan      : Float[1]
```

`oo` 是 `Real[1]`，不是量纲多态；`oo m` 是 `Real[L]`。`-oo` 由一元负号
得到。`nan` 是 `Float[1]`。

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
zeros<shape>() : Tensor<shape, Float>                 # 细化形状构造
ones<shape>()  : Tensor<shape, Float>
ones     : (Int, ..., Int) -> Tensor<n, Float>
fill     : (Scalar<ρ,d>, Int, ..., Int) -> Tensor<n, Scalar<ρ,d>>
linspace : (Scalar<ρ,d>, Scalar<ρ,d>, Int) -> Tensor<1, Scalar<ρ,d>>
reshape<m> : (Tensor<n, a>, Tensor<1, Int>) -> Tensor<m, a>
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
`ones`/`fill` 是**内建可变参数**：`n` 由 `Int` 实参的个数决定，不是用户
可声明的函数；类型检查器特殊处理。`reshape` 的目标秩必须显式给出
（`reshape<2>(a, shape)`），因为运行时形状向量的长度无法静态推断。

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
type Csr | Csc | Coo | Block | Bcsr

from_dense : Tensor<shape, τ> -> Sparse<F, shape, τ>
to_dense   : Sparse<F, shape, τ> -> Tensor<shape, τ>
nnz        : Sparse<F, shape, τ> -> Int
transpose  : Sparse<F, shape, τ> -> Sparse<F', shape', τ>   # shape' 是 shape 的逆序
matmul     : Sparse<F, [m,k], τ> × Sparse<G, [k,n], τ> -> Sparse<H, [m,n], τ>
```

`Sparse` 是核心类型构造器，`F` 是 `Format` kind；`Layout` 类提供
`from_dense`/`to_dense`/`nnz`/`matmul`/`transpose`。`@` 在 dense/sparse
混合时按布局分派；不隐式互转。用户格式通过插件注册（§13.9.5）。

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
maximize     : 同上
least_squares: ((Tensor<1, Real[d]> -> Tensor<1, Real[e]>), Tensor<1, Real[d]>)
             -> Result<Tensor<1, Real[d]>, MathError>
linsolve     : (Tensor<2, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>)
             -> Result<Tensor<1, Scalar<div_rep(ρ),e/d>>, LinAlgError>
```

`Solutions<ρ,d>` 是 `{ roots: Tensor<1, Scalar<ρ,d>>, multiplicities: Tensor<1, Int>,
conditions: Tensor<1, String> }`，保留重数与参数条件；它不是 `Set`。
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

```
class Distribution dist d where
    pdf    : (dist, Real[d]) -> Real[1/d]
    cdf    : (dist, Real[d]) -> Real[1]
    sample : dist -> IO Real[d]
    sample_n : (dist, Int, Option<Int>) -> Tensor<1, Real[d]>

type Normal<d>     = { mu: Real[d], sigma: Real[d] }
type Uniform<d>    = { lo: Real[d], hi: Real[d] }
type Exponential<d> = { rate: Real[1/d] }
type Gamma<d>      = { shape: Real[1], rate: Real[1/d] }
type Beta          = { alpha: Real[1], beta: Real[1] }
type ChiSquared    = { df: Real[1] }
type StudentT      = { df: Real[1] }
type Poisson       = { lambda: Real[1] }
type Binomial      = { n: Int, p: Real[1] }
```

`Normal<L>` 的 `pdf` 量纲是 `1/L`；`cdf` 无量纲；`sample` 返回 `Real[L]`。

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
type OdeMethod = | Rk4 | Dopri5 | Radau
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

`events` 在 `Float[1]` 变号时终止并插值；events 目前只在 `Dopri5` 实现；其他方法
设置 `events` 报 `E0807`。`Rk4` 必须给 `steps`。`odeint_with` 返回实际
到达的 `t`/`y`。`odeint` 是默认参数（`rtol=1e-8`、`atol=1e-10`、`Dopri5`）
的简写。

**展平契约**：桥接层按记录的**字段声明顺序**、深度优先、张量行主序，把
`s` 展平成 `Vec<f64>`；求解后按同一顺序还原。嵌套记录递归展平；`Tensor<n,
Float[d]>` 按行主序展平。当前只支持 `Float` 标量/张量字段；含 `Int`/
`Bool`/函数字段的状态报 `E0302`，把常量参数捕获在 `rhs` 闭包里。`Tensor<1,
s>` 用 `Boxed` 存储；数值内核只看到扁平 `f64`，Aevia 负责装箱/拆箱。

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
type Table<ρ>                  # 行多态静态 schema
type Dynamic                   # 未知 schema 的标记
type Column

read_csv<ρ>   : String -> IO (Result<Table<ρ>, DataError>)
read_csv_dyn  : String -> IO (Result<Table<Dynamic>, DataError>)
write_csv     : (String, Table<ρ>) -> IO (Result<(), DataError>)
read_npy<n>   : String -> IO (Result<Tensor<n, Float>, DataError>)
write_npy     : (String, Tensor<n, Float>) -> IO (Result<(), DataError>)
read_json     : String -> IO (Result<Json, DataError>)
write_json    : (String, Json) -> IO (Result<(), DataError>)

column        : (Table<ρ>, String) -> Result<Tensor<1, τ>, DataError>
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

`column` 的字段名是字符串字面量；类型检查器在 `ρ` 中查找该字段，返回
其元素类型。字段不存在报 `E0306`；量纲不匹配报 `E0401`。`select`/
`filter`/`join` 保持行多态；`join` 的结果行是 `ρ + σ`（`+` 是类型级行
并集；冲突字段报 `E0208`）。

`read_npy` 的秩无法从文件静态推断，必须写 `read_npy<2>(path)`；省略是
`E0314`。

CSV 表头可以带单位：`time [s]`、`velocity [m/s]`。`read_csv<ρ>` 解析
表头单位；`column(t, "time")` 在编译期用 `ρ` 的字段类型检查，运行时再做
一次防御性校验。没有表头单位时，列被当作无量纲；显式 `ρ` 可以覆盖，
但运行时校验失败报 `DataError.DimensionMismatch`。

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
"v(t)")` 是设置标签的快捷方式。`plot` 默认写
`plot.html`；`save` 指定文件名（`.html` 或 `.svg`）。

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
`E0804`。`compile_fail` 接受一段源码字符串，期望编译失败。

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

不确定度按独立误差传播（平方和开根）；相关误差需要协方差矩阵，核心库不提供。
`Measurement` 是库类型，不是语言核心。

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
Real[d]` 与 `abs : Scalar<ρ,d> -> Scalar<ρ,d>` 冲突。

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
```

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
`Physics.*` 中，逐字段标注量纲。覆盖 rssn 的 `SCENARIOS` 列表
（`ising`、`geodesic`、`gpe`、`schrodinger`、`fdtd`、`elasticity`、
`lid_driven_cavity`、`channel_flow`、`sod_shock_tube`、`kepler_symplectic`、
`poisson_multigrid`、`heat_nd`、`wave_nd`、`euler_fv`、`dg_conservation`、
`acoustics_pml`、`yee_fdtd`、`navier_stokes_mac`、`split_step`、`fem_simplex`、
`elasticity_fem`、`bem_laplace`、`sph`、`dpd`、`md_lj`、`spin_model`、
`nbody_gravity`、`kerr_geodesic`）。

## 8.18 Device

```
type Device = | Cpu | Gpu(Int) | Distributed(Cluster)
to_device : (Tensor<n,τ>, Device) -> IO (Tensor<n,τ>)
on        : (Device, () -> a) -> IO a
device_of : Tensor<n,τ> -> Device
```

设备是运行时值，不是张量类型参数；没有 `core.gpu` 时 `to_device`/`on`
报 `E1001`。分布式用 `Sharded<Mesh, Tensor<...>>` 库类型。

## 8.19 Rssn 动态接口

```
Rssn.Version       : String
Rssn.ManifestHash  : String
Rssn.Capabilities  : Tensor<1, String>

Rssn.call          : (String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.dynamic       : (String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.typed         : (String, Tensor<1, Value>, TypeRep) -> Result<a, OpError>
Rssn.call_ho       : (String, Tensor<1, Value>, Tensor<1, Callback>)
                   -> Result<Value, OpError>
Rssn.register_plugin : (String, Plugin) -> IO (Result<(), PluginError>)
```

`Rssn` 的成员由 Kernel Manifest 生成；上面的 `call`/`dynamic` 是固定入口。
有签名的算子还会生成 `Rssn.<name>`；别名生成 `Rssn.<alias>`；冲突时用
`Rssn.<rule_set>.<name>`。`Rssn.dynamic` 是**正式能力**，不是降级路径：
没有签名的算子、实验性算子、插件算子都走它。

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

`Rssn` 是**生成的模块**，不是手写库：构建时从 Kernel Manifest 生成
`Rssn.<op>` 类型化声明和 `Rssn.dynamic` 动态入口（§13.2）。`Rssn` 的
内容随 RSSN 版本变化，Aevia 源码不硬编码算子清单。`Sparse` 提供
`Csr`/`Csc`/`Coo`/`Block` 格式与 `Layout` 类（§13.9.5）；`Device` 提供
`Cpu`/`Gpu`/`Distributed` 与 `on`/`to_device`（§13.9.4）。

下一章定义运行时：从源码到值，从值到 rssn 项，从 REPL 到 JIT。
