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
round    : (Scalar<ρ,d>, Int) -> Scalar<ρ,d>   # 保留 n 位小数；n 为负时舍入到十、百……
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
`length(s)`、`chars(s)`、`s[i]`、`s[i..j]`/`s[i:j:k]` 都以 grapheme cluster 为
单位（`"e\u{301}"` 是一个位置）；`s[i]` 与切片都是 `String`，负下标从末尾数，
越界是 `E0802`。

`format(template, args)` 的占位符是 `{0}`、`{1}`（按位置）与 `{}`（取下一个）；
其他花括号内容和超出参数个数的占位符原样保留。普通字符串字面量里的 `{…}`
是插值，所以模板通常写成原始字符串 `r"{} m"` 或从文件读入；空插值 `"{}"`
是 `E0104`，提示这两种写法。

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
reverse  : Tensor<n, a> -> Tensor<n, a>                  # n = 2..4：翻转第一个轴
reverse_axis : (Tensor<n, a>, Int) -> Tensor<n, a>       # n = 1..4：翻转指定轴
filter   : ((Tensor<n-1, a> -> Bool), Tensor<n, a>) -> Tensor<n, a>   # n = 2..4：谓词看一行
take     : (Int, Tensor<n, a>) -> Tensor<n, a>           # n = 2..4：取前几行
drop     : (Int, Tensor<n, a>) -> Tensor<n, a>
concat   : (Tensor<n, a>, Tensor<n, a>) -> Tensor<n, a>  # n = 2..4：沿第一个轴堆叠
any      : ((Tensor<n-1, a> -> Bool), Tensor<n, a>) -> Bool
all      : ((Tensor<n-1, a> -> Bool), Tensor<n, a>) -> Bool
find     : ((Tensor<n-1, a> -> Bool), Tensor<n, a>) -> Option<Tensor<n-1, a>>
first    : Tensor<n, a> -> Option<Tensor<n-1, a>>
last     : Tensor<n, a> -> Option<Tensor<n-1, a>>
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
first    : Tensor<1, a> -> Option<a>
last     : Tensor<1, a> -> Option<a>
cumsum   : Tensor<1, Scalar<ρ,d>> -> Tensor<1, Scalar<ρ,d>>
cumprod  : Tensor<1, Scalar<ρ,1>> -> Tensor<1, Scalar<ρ,1>>
argmax   : Tensor<1, Scalar<ρ,d>> -> Int      # 首个最大值的下标
argmin   : Tensor<1, Scalar<ρ,d>> -> Int
```

**秩的完整性（Revision 16）。** 规范里写成 `Tensor<n, …>` 的操作在秩 1 到 4 上都有类型：
`sum`、`prod`、`mean`、`var`、`std`、`norm`、`normalize`、`map`、`map2`/`zip_with`（同形）以及
`flatten`（秩 ≥ 2）；`sum(a, axis)` 的 keepdims 形式在秩 1 到 4 上可用。秩不是一等类型参数，
所以每个秩一个重载，由实参的秩选择。`first`/`last` 对空张量给 `None`；`argmax`/`argmin` 对空张量
是 `E0802`，NaN 不会胜出，相等时取最前的下标。

**秩 ≥ 2 的序列操作（Revision 20）。** 秩 `n ≥ 2` 的张量是沿第一个轴的 `n−1` 秩切片（"行"）的序列：

- `reverse` 翻转第一个轴，保持形状（此前运行时把矩阵展平成向量，而类型表只有秩 1；展平请显式写 `reverse(flatten(m))`）。
  `reverse_axis(t, k)` 翻转第 `k` 轴（秩 1 到 4，负数从末尾数，越界 `E0802`）。`f64` 张量上两者都是视图，不复制缓冲区（§9.5）。
- `filter(p, m)`：`p` 接受一行（秩 `n−1` 的张量），结果保持秩 `n`，行数为被接受的行数；全部拒绝得到第一轴长度为 0 的同秩张量。
- `any`/`all`/`find` 对行逐个测试；`find` 返回 `Option<Tensor<n-1, a>>`；`first`/`last` 返回第一行/最后一行的 `Option`。
- `take(k, m)`/`drop(k, m)` 按行取舍；`concat(a, b)` 沿第一个轴堆叠，其余轴的长度必须一致（否则 `E0802`）。
- 其余元素级操作（`sum`、`map`、`fold`、`sort` 等）仍按元素处理，不受影响。

`length` 对 `String` 和 `Tensor` 重载，由类型类 `Sized` 解析。

`par_map(f, xs)` 并行映射，结果顺序与输入一致（线程数不改变结果）。它
要求 `Parallel a`：`Int`、`Float[d]` 及其张量可以并行，符号 `Real` 不并行
（§3.8.4/§6.10）。元素类型由调用点决定，所以约束在调用点检查，而不是一个
全局实例。

### 8.1.5.1 集合

```
set_of       : TotalOrd a => Tensor<1, a> -> Set<a>   # 去重，按 total_compare 排序
elements     : Set<a> -> Tensor<1, a>                 # 升序
length       : Set<a> -> Int                          # length 的第三个重载
member       : (a, Set<a>) -> Bool                    # O(log n)
set_insert   : (Set<a>, a) -> Set<a>
set_remove   : (Set<a>, a) -> Set<a>
union        : (Set<a>, Set<a>) -> Set<a>
intersection : (Set<a>, Set<a>) -> Set<a>
difference   : (Set<a>, Set<a>) -> Set<a>
is_subset    : (Set<a>, Set<a>) -> Bool
```

`Set<a>`（§3.2、§6.6）是持久有序集合，要求 `TotalOrd a`（`Set<Float>` 是
`E0310`）。表示是规范的——元素按 `total_compare` 升序且唯一——所以集合的 `==`
就是集合相等，与插入顺序无关；`show` 打印为 `{1, 2, 3}`，`repr` 打印为
`set_of([1, 2, 3])`。集合运算是线性归并，`member` 是二分查找。修改不在原处进行：
`set_insert` 返回新集合，旧值不变（§0.4 原则三）。Revision 13 之前 `Set<τ>` 只有
类型、没有任何值或操作。

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

> **实现状态（2026-11-08）**：`par_map`/`par_tabulate`/`par_reduce`/`rng`
> 已进入 §3.8.4 的内置类表、Prelude 签名与运行时 dispatch；checker 在调用点
> 检查 `Parallel a`。`par_map` 在工作线程副本上真正并行（§9.7.2）：结果顺序
> 与输入一致，`par_reduce` 的块边界只取决于长度，线程数不改变任何结果；值
> 不能跨线程时回退为顺序求值。`spawn`/`await`/`pure` 见 §9.7.3。

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
fresnel_s fresnel_c : Scalar<ρ,1> -> Scalar<ρ,1>   # S(x)=∫₀ˣ sin(πt²/2)dt，C 同理（DLMF 7.2.7）
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
refine_shape<shape> : Tensor<n, τ> -> Option<Tensor<shape, τ>>
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

`transpose` 在秩 ≥ 2 的张量上反转轴序（秩 3 的 `[a, b, c]` 变成 `[c, b, a]`，细化形状同样反转）；
秩 1 是 `E0302`（向量没有第二个轴，提示里说明了这一点）。`norm` 在秩 ≥ 2 上是对全部元素的
范数（`p = 2` 即 Frobenius 范数），`var`/`std`/`mean`/`sum`/`prod` 对全部元素归约（见 §8.1.5）。

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

**形状分支：`refine_shape`。** `refine_shape<[3, ?]>(a)` 在运行时检查 `a` 的各
extent 是否等于类型实参里写出的字面量（`?` 表示任意 extent），相符得
`Some(b)`（`b : Tensor<[3, ?], τ>`），否则 `None`；秩在编译期检查（秩不符是 `E0302`）。
因此"依形状分支"写成对 `Option` 的 `match`，每个分支里的 `b` 在**类型**中已
带有具体 extent，后续的矩阵乘法、索引、`reshape` 都按它做静态检查：

```
match refine_shape<[3, 3]>(m) with
| Some(sq) -> sq @ sq            # sq : Tensor<[3, 3], Float>，乘法维度静态已知
| None -> m
```

extent 必须是字面量或 `?`：类型级变量在运行时没有值，`refine_shape<[n]>(a)` 是
`E0304`（变量 extent 的细化只能由签名里的等式求解，不能靠运行时测试）。

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

**非对称系统：`gmres_with` 与 `bicgstab_with`。** `cg`/`cg_with` 要求对称正定；
一般方阵（对流—扩散、非对称离散化）用 `gmres_with(A, b, { tol, maxiter, restart,
precond })`（重启 GMRES(`restart`)，右预条件，残差范数单调不增，内存
`O(restart·n)`）或 `bicgstab_with(A, b, { tol, maxiter, precond })`（BiCGSTAB，
短递推、常数内存，残差历史可能不规则）。两者来自 RSSN 的 Krylov 内核，从零向量
起步，返回与 `cg_with` 同形的记录（`x`、`iterations`、`residual`、`converged`、
`precond`）；未达容差是 `converged = false` 而非错误，`restart = 0` 是 `E0802`。
预条件子取 `Precond`，但对非对称矩阵 `Ic0` 与 `Auto` 都**改用** `Ilu0`（`Auto` 对
`Bcsr` 先试块 ILU(0)）；分解失败退回 `Jacobi`，`precond` 字段如实报告用了哪一种。
元素必须是 `Float`；精确元素用 `spsolve`。结果量纲是 `e/d`。直接法仍是
`spsolve`（对称正定先试 Cholesky，否则 LU）。

**并行与确定性。** `sparse @ dense`（向量或矩阵，含 `Bcsr` 的块内核）在乘加量不小
于 2^17 时按行分块并行：块大小是常数（256 行，`Bcsr` 为一个块行），每个输出行
由恰好一个工作项按列序累加，所以结果与线程数无关，且与顺序内核逐位相同。
Krylov 迭代本身是顺序的（只有 `A @ x` 并行），因此 `cg_with`/`gmres_with`/
`bicgstab_with` 的迭代序列也与线程数无关。

**预条件共轭梯度与切片。** `cg_with(A, b, { tol, maxiter, precond })` 是带预条件子的
`cg`（`cg` 本身不变）。`Precond` 是标准库变体（`import LinearAlgebra.Sparse { Precond,
Jacobi, cg_with }`）：`Identity`（无预条件子）、`Jacobi`（对角缩放）、`BlockJacobi`
（对角块求逆，rssn `Bsr::block_jacobi`）、`Ilu0`（保持稀疏模式的不完全 LU）、`Ic0`
（不完全 Cholesky）、`Auto`（IC(0) → ILU(0) → Jacobi 中第一个能建成的；`Bcsr` 操作数
先试块 ILU(0) 再试块 Jacobi）。只接受 `Float`，要求对称正定；`Bcsr` 操作数的
`BlockJacobi`/`Ilu0` 用它自己的方块形状（非方块时自动选方块），其它格式的
`BlockJacobi` 自动选方块。某个分解失败（零主元、不定）时退回 Jacobi，结果记录的
`precond` 字段（`"Identity"`、`"Jacobi"`、`"BlockJacobi"`、`"BlockIlu0"`、`"Ilu0"`、
`"Ic0"`）报告**实际使用**的那一种。`Auto` 之外的构造器之所以没有叫 `None`，是为了不与
`Option` 的 `None` 冲突，所以无预条件子写作 `Identity`。

`spsolve` 对**对称正定的 `Float` 矩阵**先试 `faer` 稀疏 Cholesky（`A = L Lᵀ`，
先检查严格对称与正对角线），失败（不定、分解中断）则退回稀疏 LU；结果与 LU 一致。

稀疏值支持**切片**：`A[i, :]`、`A[:, j]`、`A[r0..r1, c0..c1]`、`A[::2, 1..]`（语法与
稠密张量相同）。整数下标去掉该轴，区间保留该轴并重新编号，所以 `A[i, :]` 是秩 1 的
`Sparse<Coo, [n], τ>`，`A[r0..r1, c0..c1]` 是同格式的秩 2 稀疏矩阵；结果秩不是 2 时
一律是 `Coo`（秩 > 2 的 `Coo` 切片同理）。切片只过滤并重新编址已存储项，
**不稠密化**，代价 `O(nnz)`；越界整数下标是 `E0802`，区间的规范化与稠密张量一致，
下标个数必须等于秩。

另有三个对 `Sparse` 重载的既有名字：`sum(A)`、`sum(A, axis)`（与稠密版同样
保留被归约的轴，结果是稠密张量）和 `get(A, i, j, …)`（也可写 `A[i, j, …]`，
返回存储的元素，未存储位置返回**隐式零**）。

`Csr`/`Csc`/`Coo`/`Block`/`Bcsr` 是内建 `Format` 标签（§3.1），不是值构造器。
`Sparse` 是核心类型构造器，`F` 是 `Format` kind；它**不**擦除为 `Tensor`：
`Sparse<F, shape, τ>` 与 `Tensor<shape, τ>` 之间没有隐式转换，格式是类型的一
部分（`Sparse<Csr, …>` 不能当作 `Sparse<Coo, …>` 传递，要用 `convert<G>`）。
`from_dense<F>`/`speye<F>`/`spdiag<F>` 的 `F` 缺省为 `Csr`；`convert<G>` 必须
写出目标格式。`Csr`/`Csc` 是二维（矩阵）格式，`Coo` 是任意秩的**稀疏张量**。
`Block` 是 `Bcsr`（块压缩稀疏行）在类型里的另一种写法，二者是**同一个**格式
标签：`Sparse<Block, …>` 与 `Sparse<Bcsr, …>` 是同一个类型，`format_of` 一律报告
`Bcsr`。

**块压缩稀疏行（`Bcsr`，BSR）。** 矩阵（秩 2）被切成 `br × bc` 的稠密小块，只存
含非零元素的块，块之间按 CSR 方式压缩：`ptr` 是**块行**的指针（长 `m/br + 1`），
`idx` 是每个已存储块的**块列**下标（块行内严格递增），值缓冲每个块占 `br·bc`
个槽位，块内行主序、块按 `idx` 顺序连续。块形状要求 `br | m`、`bc | n`（均为正
整数），可以用 `block_size(A)` 读回（非 `Bcsr` 的格式是 `(1, 1)`）。适用于每个结点
带多个未知量的系统（弹性、Navier–Stokes、耦合反应扩散）：下标数组缩小为
`1/(br·bc)`，`@` 变成小稠密块的乘加而不是逐元素的间接寻址。

**填充规则。** "已存储项永不等于隐式零"这条不变量在 `Bcsr` 里**只放宽到块内**：
一个已存储的**块**至少含一个非零元素，但块内其余槽位可以是零（**填充**）。填充
不是逻辑项：`nnz(A)` 数的是逻辑非零元素（不含填充），`A[i, j]` 在填充处回答隐式
零，`nonzeros`、`==`、`show`/`repr`、`to_dense`、到内核的边界（`to_flat_parts`）
都只看到逻辑非零。所有其它运算（`±`、Hadamard 积、标量积、`transpose`、
`spsolve`、`cg`、`kron`、`contract`、`map_nonzero`）经由按字典序排列的规范项
形式进行，该形式跳过填充，因此对 `Bcsr` 的行为与对 `Csr` 完全相同；结果保持
`Bcsr`（只要原块形状——转置时为 `(bc, br)`——仍整除结果形状就沿用它，否则按下面
的启发式重选）。装箱（`Int`/`Real`/`Complex`/符号）元素的填充是该元素类的零，
所有运算仍走语言自己的标量算术，精确值**永不**舍入。

**块形状的选取。** `to_bcsr(A, br, bc)` 显式指定块形状；`br` 或 `bc` 为零、不整除
`A` 的形状、或 `A` 的秩不是 2，是 `E0802`。`convert<Bcsr>(A)`、
`from_dense<Bcsr>(M)`、`speye<Bcsr>(n)`、`spdiag<Bcsr>(v)` 以及对 `Bcsr` 运算后
需要重选块形状的结果按**填充率**自动选取：候选是 `{4, 3, 2, 1}²` 中整除形状的
块（方块排在前），填充率 = 已存储槽位数 / 逻辑非零数；取**面积最大**且填充率
不超过 `1.5` 的候选，面积相同取填充率低者，仍相同取靠前（方块）者，都不满足则
`(1, 1)`（这时 `Bcsr` 退化为逐元素的 CSR，一个标量即一个块）。rssn 内核的
`kernels::bsr::choose_block_size` 用同一个规则。

**复杂度。** 记 `B = br·bc`，`nb` 为已存储块数，逻辑非零数为 `nnz`：存储
`nb·B + nb + m/br` 个字；`A @ x`（向量或 `k` 列矩阵）为 `O(nb·B·k)`，且只对非零槽
位做乘加，结果与同一矩阵的 `Csr` 内核逐位相同；转为规范项形式 `O(nb·B)`；按给定
块形状构造 `O(nnz + nb·B)`（自动选取再乘以候选数，至多 15 趟 `O(nnz)` 扫描）。
填充率越接近 1，`Bcsr` 越省内存也越快；填充率很大时（块太大）`Bcsr` 反而浪费，
应换小块或用 `Csr`。

**运行时表示。** `Value::Sparse` 是一等运行时值，不是记录约定。它保存形状、
元素量纲、格式，以及按字典序排列、无重复的已存储项（CSR/CSC 用压缩指针，COO 用
平铺坐标，`Bcsr` 用块指针与块列下标）；**已存储项永不等于隐式零**（`Bcsr` 的
块内填充除外，见上；构造与每个运算都丢弃零项，
`SparseData::validate` 在 debug 构建里检查这些不变量）。元素存储与稠密张量
同一规则：当且仅当每个已存储元素都是 `Float`（隐式零也是 `0.0`）时是 `f64`
缓冲，否则是装箱的 `Int`/`Real`/`Complex`/符号标量，所有运算都经过语言自己的
标量算术——精确值**永不**舍入为 `f64`。隐式零是元素表示与量纲的零：`Int` 为 `0`、
`Real` 为精确的 `0`、`Float` 为 `0.0`、`Complex` 为 `0+0i`。

**构造与校验。** `sparse_coo(shape, indices, values)` 的 `indices` 可以是
`[nnz, rank]` 的 `Int` 矩阵、`Int` 元组列表（`[(0, 0), (1, 1)]`），秩为 1 时也
可以是 `Int` 列表；`values` 与 `indices` 等长。越界、负下标、长度不符是 `E0802`。
**重复坐标按求和合并**（有限元装配的语义），合并后等于零的项被丢弃。
`sparse_csr(rows, cols, row_ptr, col_indices, values)` 要求 `row_ptr` 长
`rows+1`、以 `0` 开始、单调、以 `nnz` 结束，且每行的列下标严格递增且小于
`cols`，否则 `E0802`。`from_dense` 丢弃为零的元素，`to_dense` 填回隐式零；
稠密结果的元素个数超过实现上限（10^8）报 `E0802`，所以任何意外的稠密化都会被
拒绝而不是耗尽内存。

**算术与布局分派**（元素量纲的规则与稠密张量完全一致：和要求量纲相等 `E0401`，
积相乘，`spsolve` 相除）：

| 运算 | 结果 |
|---|---|
| `sparse ± sparse` | `Sparse`，格式取左操作数 |
| `sparse ± dense`、`dense ± sparse` | 稠密 `Tensor`（混合布局的结果落在能容纳它的布局） |
| `sparse * s`、`s * sparse`、`sparse / s`（`s` 为标量或单位，含带量纲标量） | `Sparse` |
| `sparse * sparse`、`sparse * dense`、`dense * sparse`（Hadamard 积） | `Sparse`；只遍历已存储项，稠密一侧在未存储位置的值被忽略 |
| `sparse @ dense`（向量或矩阵）、`dense @ sparse` | 稠密 `Tensor` |
| `sparse @ sparse`、`matmul`、`kron` | `Sparse<Csr, …>` |
| `-A`、`transpose(A)` | `Sparse`，格式不变 |

`sparse / sparse`、标量除以稀疏、`sparse + 标量` 没有定义（会稠密化或产生无穷
零），报 `E0302`；需要时先 `to_dense`。`==`/`!=` 比较两个稀疏值的**逻辑内容**
（形状相同且已存储项相同，数值按与稠密张量相同的规则比较，所以 `1` 等于 `1.0`），
**与格式无关**：同一矩阵的 `Csr` 与 `Coo` 相等。`show` 写成
`sparse(Csr, [3, 3], { (0, 0) = 4.0, … })`（`Bcsr` 写 `sparse(Bcsr, …)`）；`repr` 是能
重建同一个值的表达式 `convert<Csr>(sparse_coo([3, 3], [(0, 0), …], [4.0, …]))`，
`Bcsr` 的 `repr` 带上块形状：`to_bcsr(sparse_coo([4, 4], […], […]), 2, 2)`。

**求解。** `spsolve(A, b)` 对 `Float` 元素调用 `faer` 的稀疏 LU（COLAMD 排序，
`aevia_std::sparse`），不稠密化——10^5 阶三对角系统是毫秒级；对精确元素
（`Int`/`Real`/`Complex`/混合）用稀疏高斯消元，结果保持精确（`Int` 系统得到
精确有理数）。奇异矩阵是 `E0806`。结果量纲是 `e/d`。`cg(A, b, tol, maxiter)`
是无预条件共轭梯度（要求对称正定的 `Float` 矩阵），返回
`{ x, iterations, residual, converged }`；未达到容差不是错误而是
`converged = false`。

**秩 N 与收缩。** `contract(A, B, axes_a, axes_b)` 是成对张量收缩
（`tensordot`）：`axes_a`/`axes_b` 是**列表字面量**（结果的秩由它们的长度静态
决定），结果的轴是 `A` 的自由轴再接 `B` 的自由轴。两个稀疏操作数得到
`Sparse<Coo, …>`；一个稀疏、一个稠密得到稠密 `Tensor`；全部轴都收缩得到标量；
两个都是稠密是类型错误。`map_nonzero(f, A)` 只对**已存储项**调用 `f`，因此要求
`f(0) = 0`（对未存储位置不调用，返回零的项被丢弃）；`nonzeros(A)` 返回
`(下标, 值)` 元组的列表。

**与内核的边界。** RSSN 的 `TypeExpr::Sparse` 只有 `rank`，所以 Aevia 的
`Sparse<F, shape, τ>` 在边界降为 rank + 运行时格式；`aevia-rssn` 的
`Val::Sparse` 与 RSSN 的 `Value::Sparse`（平铺行主序升序索引 + 值）一一对应，
**不再稠密化**。RSSN 的 `term_of_value` 仍不接受 `Value::Sparse` 进入项图
（`manifest.value_kinds` 里 `in_term_graph = false`），所以稀疏值不能直接喂给通用
重写算子；稀疏运算由 Aevia 自己的布局内核承担，`faer`（`sparse-linalg`）负责
稀疏 LU。用户自定义格式要等 RSSN 开放 `Format`（§13.9.5）。

> **实现状态（2026-11-07）**：以上全部已实现并有测试覆盖
> （`crates/aevia-core/tests/sparse.rs`）：`Value::Sparse` 运行时值（`Csr`/
> `Csc`/`Coo`/`Bcsr`，`f64` 与精确两类存储）、checker 中独立的 `Sparse<F, shape, τ>`
> 类型（不再擦除为 `Tensor`）、`@` 的四种布局分派（AST 与 IR 求值器）、
> 稀疏 LU/高斯消元/CG、秩 N 收缩与到内核的无稠密化过桥。能力
> `storage.sparse` 因此**已启用**。`Bcsr` 现在是真实格式
> （`crates/aevia-core/tests/sparse_block.rs`；`@` 在 `f64` 类用块内核，rssn
> 侧的块 Jacobi 与块 ILU(0) 经 `cg_with` 接到语言层。预条件 `cg_with`、
> `spsolve` 的 Cholesky 优先路径与稀疏切片都已实现（`tests/sparse_followup.rs`）。
> **形状传播（2026-11 核对）**：`Sparse<F, [m,n], τ>` 的细化形状**会**被检查
> 与传播（`crates/aevia-types/src/check/sparse.rs`）：`A @ B` 的内维、`A @ x`、
> `A ± B`、`spsolve(A, b)`、`transpose`、`kron`（`[m*p, n*q]`）、`to_dense`/
> `from_dense` 都对字面量与带 Nat 变量的 extent 做统一，不符是编译错误
> （`tests/shape_refinement.rs`）。此前这里写的"只检查秩"已过时。
> **剩余缺口**：稀疏值不能进入 RSSN 的项图；`gmres_with`/`bicgstab_with` 不接受
> 精确元素；没有稀疏特征值/SVD。

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

`sum(f, a, b)`/`product(f, a, b)` 的两个界都是整数（精确非整数界是 `E0302`）。
不超过 10^4 项的具体区间逐项精确求和；无穷界（`oo`）、符号界或更长的区间向
内核请求闭式（`sum(fn k => 1/k^2, 1, oo) = pi^2/6`、
`sum(fn k => k, 1, n) = n^2/2 + n/2`、几何级数 `sum(fn k => 2^(-k), 0, oo) = 2`）；
内核找不到闭式时报 `E0808`，不会返回未求值的 `sum(…)` 冒充答案。

### 8.5.1 代数形式

```
expand   : Real[d] -> Real[d]
factor   : Real[d] -> Real[d]            # 有理数域上的因式分解；Int 重载见 §8.16
cancel   : Real[d] -> Real[d]            # 有理函数约去公因子：p/q，gcd(p, q) = 1
together : Real[d] -> Real[d]            # 通分为单个分式
apart    : (Real[d], Real[e]) -> Real[d] # 对变量做部分分式分解
collect  : (Real[d], Real[e]) -> Real[d] # 按变量的幂合并同类项
degree   : (Real[d], Real[e]) -> Int     # 关于变量的多项式次数
```

这些函数改变精确表达式的**形式**而不改变它的值，因此结果的量纲就是参数的
量纲。它们是 `simplify` 的补充：`simplify` 让内核选“最好”的等价形式（通常是
最短的），而用户需要的往往是某个特定形式——展开后读系数、分解后读根、部分
分式后逐项积分。每个函数直接调用内核 `poly` 规则集的同名算子，而不是发出
`simplify` 请求（后者会把 `factor(x^4 - 1)` 抽取回更短的 `x^4 - 1`）。内核
没有给出该形式时报 `E0808`，不返回未求值的 `factor(…)`。`factor` 对 `Int`
参数仍是 §8.16 的整数分解：`factor(360)` 是 `[(2, 3), (3, 2), (5, 1)]`，
`factor(x^4 - 1)` 是 `(x - 1)*(x + 1)*(x^2 + 1)`。

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

这些声明是 `Stats.Distributions` 模块的源码（`open Stats.Distributions` 后
`Normal({ mu = 1.7 m, sigma = 0.1 m })` 是一个 `Distribution<L>` 值）。
`pdf`、`cdf`、`sample`、`sample_n` 是对这个封闭 ADT 的**内建重载**：

```
pdf      : (Distribution<d>, Real[d]) -> Real[1/d]
cdf      : (Distribution<d>, Real[d]) -> Real[1]
sample   : Distribution<d> -> IO Real[d]
sample_n : (Distribution<d>, Int, Option<Int>) -> Tensor<1, Real[d]>
```

`Normal<L>` 的 `pdf` 量纲是 `1/L`；`cdf` 无量纲；`sample` 返回 `Real[L]`。

**用户自定义分布不是语言扩展点。** 多参数类被移除（§3.8.1），而“分布参数 →
量纲”的映射需要类型函数（类似 `RateOf`），这是另一个设计；在没有它之前，
把 `class Distribution dist d` 留在规范里只会让用户写出无法实例化的代码。
用户自定义分布列在 §12.8.13 的扩展边界之外；标准库可以为新的分布形态增加
内建构造器，但那是库的封闭集合，不是语言扩展点。

## 8.8 FFT / Interpolate

```
fft      : Tensor<1, X[d]> -> Tensor<1, Complex[d]>       # 无归一化；X ∈ {Complex, Float, Int}
ifft     : Tensor<1, X[d]> -> Tensor<1, Complex[d]>       # 1/N
fft_norm : (Tensor<1, X[d]>, Norm) -> Tensor<1, Complex[d]>
ifft_norm : (Tensor<1, X[d]>, Norm) -> Tensor<1, Complex[d]>
fft_axis : (Tensor<n, X[d]>, Int) -> Tensor<n, Complex[d]>      # 沿一个轴，n = 1..4
ifft_axis : (Tensor<n, X[d]>, Int) -> Tensor<n, Complex[d]>
fft2     : Tensor<n, X[d]> -> Tensor<n, Complex[d]>             # 最后两个轴，n = 2..4
ifft2    : Tensor<n, X[d]> -> Tensor<n, Complex[d]>
fftn     : Tensor<n, X[d]> -> Tensor<n, Complex[d]>             # 全部轴，n = 1..4
ifftn    : Tensor<n, X[d]> -> Tensor<n, Complex[d]>
fftn_axes : (Tensor<n, X[d]>, Tensor<1, Int>) -> Tensor<n, Complex[d]>   # 任意轴列表
ifftn_axes : (Tensor<n, X[d]>, Tensor<1, Int>) -> Tensor<n, Complex[d]>
fftn_norm : (Tensor<n, X[d]>, Norm) -> Tensor<n, Complex[d]>
ifftn_norm : (Tensor<n, X[d]>, Norm) -> Tensor<n, Complex[d]>
rfft     : Tensor<1, Float[d]> -> Tensor<1, Complex[d]>
irfft    : Tensor<1, Complex[d]> -> Tensor<1, Float[d]>
irfft    : (Tensor<1, Complex[d]>, Int) -> Tensor<1, Float[d]>  # 显式长度（奇数长度）
rfft2    : Tensor<n, Float[d]> -> Tensor<n, Complex[d]>         # 最后两个轴，n = 2..4
irfft2   : Tensor<n, Complex[d]> -> Tensor<n, Float[d]>
irfft2   : (Tensor<n, Complex[d]>, Int) -> Tensor<n, Float[d]>
rfftn    : Tensor<n, Float[d]> -> Tensor<n, Complex[d]>         # 全部轴，n = 1..4
irfftn   : Tensor<n, Complex[d]> -> Tensor<n, Float[d]>
irfftn   : (Tensor<n, Complex[d]>, Int) -> Tensor<n, Float[d]>
fftshift : Tensor<n, a> -> Tensor<n, a>                         # 全部轴，n = 1..4
ifftshift : Tensor<n, a> -> Tensor<n, a>
fftshift_axis : (Tensor<n, a>, Int) -> Tensor<n, a>
ifftshift_axis : (Tensor<n, a>, Int) -> Tensor<n, a>
fftfreq  : (Int, Float[T]) -> Tensor<1, Float[1/T]>
rfftfreq : (Int, Float[T]) -> Tensor<1, Float[1/T]>             # n/2 + 1 个非负频率

interp1  : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Scalar<ρ,d>) -> Scalar<ρ,e>
interp2  : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Tensor<2, Scalar<ρ,f>>,
            Scalar<ρ,d>, Scalar<ρ,e>) -> Scalar<ρ,f>
spline   : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Scalar<ρ,d>) -> Scalar<ρ,e>
polyfit  : (Tensor<1, Scalar<ρ,1>>, Tensor<1, Scalar<ρ,e>>, Int) -> Tensor<1, Scalar<ρ,e>>
polyval  : (Tensor<1, Scalar<ρ,e>>, Scalar<ρ,1>) -> Scalar<ρ,e>
chebyshev_fit : (Tensor<1, Scalar<ρ,1>>, Tensor<1, Scalar<ρ,e>>, Int) -> Tensor<1, Scalar<ρ,e>>
```

`fft` 不归一化，`ifft` 除以 `N`，因此 `ifft(fft(x)) == x`。需要
`1/√N` 或其他约定时用 `fft_norm(x, Unitary)`（`import FFT { Unitary }`，或写 `FFT.Unitary`）。

**变换族的完整性（Revision 20）。** 所有正向/逆向变换接受复数、`Float` 或 `Int` 张量（实数、整数按虚部为 0 提升），
结果总是复数，量纲与样本相同（频率轴的量纲只出现在 `fftfreq`/`rfftfreq` 的结果里，它们取采样间隔 `Float[T]`，
返回 `Float[1/T]`——`fftfreq(4, 0.5 s)` 的单位是 `Hz`）。多维变换由行列分解实现：沿每个被变换的轴依次做一维变换。

- `fft2`/`ifft2` 变换最后两个轴；`fftn`/`ifftn` 变换全部轴；`fft_axis(t, k)` 一个轴；`fftn_axes(t, axes)` 任意轴列表。
  轴号从 0 开始，负数从末尾数（`-1` 是最后一轴）；越界或重复的轴是 `E0802`。秩 1 到 4 都有类型。
- 归一化按被变换的总点数 `N`（各变换轴长度之积）：`Backward` 正向 1、逆向 `1/N`；`Forward` 正向 `1/N`、逆向 1；
  `Unitary` 两向都是 `1/√N`。（`Forward` 在 Revision 20 之前错用了 `1/√N`，现已按文档修正。）
- `rfft`/`rfft2`/`rfftn` 取实数输入，最后一个被变换的轴只保留 `n/2 + 1` 个非冗余频点；`irfft`/`irfft2`/`irfftn`
  由半谱重建实数张量，默认原长度为 `2(m − 1)`（偶数），奇数长度用带 `Int` 的形式显式给出；`m` 与长度不相容是 `E0802`。
- `fftshift`/`ifftshift` 把零频率移到中间（每个被移位的轴循环平移 `⌊n/2⌋`，逆操作反向），对任意元素类型成立；
  `*_axis` 形式只移一个轴。

```
type Norm = | Backward | Forward | Unitary
type AxisScale = | Linear | Log
```

`polyfit` 要求自变量无量纲；带量纲时先除以单位 `x/u`，系数保持 `y` 的
量纲。这是量纲系统对 API 的约束：多项式系数 `a_i` 的量纲是 `e/d^i`，
不同次幂不同，无法放进同质张量。

## 8.9 ODE

```
type OdeMethod = | Rk4 | Dopri5 | Radau | Bdf | Rosenbrock23 | AutoSwitch
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

**8.9.0 解对象、稠密输出、多事件、高阶方程、边值问题**

`odeint`/`solve_ivp` 只给离散输出和一个终止事件。需要在**任意时刻**取值、多个事件
（带方向、可终止或不终止）、高阶方程或边值问题时用下面的入口；它们与
`odeint_with` 共用 `OdeOpts<s>`、`RateOf<s>` 和展平契约，并与 §8.9.1 的 SDE 事件/
稠密输出 API 同形（`{ g, terminal, direction }`、`{ index, t, y }`、`at`）。

```
type OdeEvent<s> = {
    g: (Float[T], s) -> Float[1],   # 变号即事件；必须返回无量纲数
    terminal: Bool,                 # true：积分在该事件处停止
    direction: Int,                 # -1 只认下降穿越，0 两个方向，1 只认上升穿越
}
type OdeHit<s> = { index: Int, t: Float[T], y: s }
type OdeSolution<s> = {
    t: Tensor<1, Float[T]>, y: Tensor<1, s>,        # 接受的步节点
    at: (Float[T]) -> s,                            # 连续扩展：任意 t 处取值
    events: Tensor<1, OdeHit<s>>,                   # 按时间排序的事件
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

**连续输出。** `ode_solve(rhs, y0, (t0, t1), opts, events)` 在整个区间上积分
（`t1 ≥ t0`；反向积分不支持）并保留方法自带的连续扩展，`sol.at(t)` 在
`[t0, t_end]` 内任意时刻取值（区间外是 `E0802`，量纲不是 `T` 是 `E0401`）。`Dopri5`
用 RSSN 的 4 阶连续扩展；`Radau`/`Bdf`/`Rosenbrock23` 用各自的配置多项式/向后差分
插值；`Rk4` 没有级数据，用节点上的三次 Hermite 插值（插值误差 `O(h^4)`，随步长
四阶收敛）。`Rk4` 的 `steps` 在这里是**整个区间的总步数**（`odeint_with` 里是每个
输出区间的子步数）。`at` 是一个函数值（不是数据），所以含 `at` 的解不能跨线程
返回；要在 `par_map` 里返回，只取 `t`/`y`。

**事件。** `events` 是 `OdeEvent` 的列表，可以为空（`[]`）。每个事件的 `g` 在接受的
步之间变号（`g0 < 0 ≤ g1` 为上升，`g0 > 0 ≥ g1` 为下降，`direction` 过滤）时由根
查找在连续扩展上定位（`Rk4`：重积分的二分），命中记入 `events`（`index` 是列表下
标）；`terminal = true` 的事件让积分在该点结束，`t`/`y` 的最后一个节点就是命中点，
`at` 的有效区间随之截断。同一步内的多个命中按时间排序，终止事件之后的命中丢弃。
`g` 返回有量纲的值是 `E0302`；方向取值不是 -1/0/1 是 `E0802`。

**刚性求解器选择。** `AutoSwitch` 先用 `Dopri5`，且只给它 20000 次尝试步；预算耗尽
（刚性问题受稳定性限制，步数远超精度所需）就改用 `Radau` 重新积分整个区间，
`sol.method` 报告最终用的方法（`"Dopri5"` 或 `"Radau"`）。这是**按步数预算**判定，
不是特征值估计：中等刚性（例如需要几万步）的问题会白白多跑一遍显式方法；
明知刚性时直接选 `Radau`/`Bdf`/`Rosenbrock23`。`AutoSwitch` 也用于 `odeint_with`、
`solve_ivp` 和事件积分（同样的规则）。

**高阶方程。** `odeint_order(f, (y0, y1, …, y_{n-1}), times)` 积分
`y⁽ⁿ⁾ = f(t, y, y′, …, y⁽ⁿ⁻¹⁾)`（`n` 是 `f` 的参数个数减一，`n ≥ 1`），内部化成
堆叠状态 `z = (y, y′, …, y⁽ⁿ⁻¹⁾)` 上的一阶系统，除了 `y⁽ⁿ⁾ = f` 之外每个分量
都是平凡的 `zₖ′ = zₖ₊₁`。**量纲逐阶追踪**：第 `k` 阶导数的类型是 `RateOf` 作用 `k` 次，
`y : Float[L]` 时 `y′ : Float[L/T]`、`y″ : Float[L/T^2]`，`f` 必须返回 `L/T^n`
（检查期 `E0401`/`E0302`；运行期再核对初值每一项的量纲与结构，量纲不符 `E0401`）。
`y` 可以是标量、张量或记录，各阶导数结构相同。返回 `d` 是各阶导数的元组历史，
`y` 是第 0 阶。`odeint_order_with` 多一个 `OdeOpts<y0>`（方法、容差；`events` 必须为
`None`，要事件请手工化阶后用 `ode_solve`）。例（弹簧 `x″ = -(k/m) x`）：

```
let sol = odeint_order(fn (_t, x, v) => -(k / m) * x, (1.0 m, 0.0 m/s), times)
# x : Float[L]，v : Float[L/T]，返回值 Float[L/T^2]
```

**边值问题（单步打靶）。** `ode_bvp(rhs, guess, (t0, t1), bc, opts)` 求解
`y′ = rhs(t, y)`、`bc(y(t0), y(t1)) = 0`：`bc` 返回与状态分量个数相同的残差张量。
算法是对初始状态的牛顿迭代（前向差分 Jacobian，每个未知量一次积分；带回溯线搜索），
`guess` 是牛顿的起点，收敛判据 `‖r‖∞ ≤ 1e-9 (1 + ‖y0‖∞)`，最多 60 次迭代；返回
最后一次积分的 `OdeSolution`（所以有 `at`）。二阶边值问题先用 `odeint_order` 的化阶
思路写成状态 `(y, y′)`。残差个数不符是 `E0802`，雅可比奇异或不收敛是
`NotConverged`。**限制**：这是单步打靶，只适用于 `y(t1)` 对 `y(t0)` 的灵敏度不爆炸的
问题；指数不稳定（长区间、刚性）的边值问题需要多重打靶或配置法，此处**没有**提供。

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
    | MilsteinNoncommutativeStratonovich

type SdeStructure =
    | Diagonal                     # 每个状态分量一个噪声通道，m = n
    | General(Int)                 # (m)；调用者声明噪声可交换
    | GeneralNoncommutative(Int)   # (m)；噪声不可交换：方法名分派到 Lévy 面积内核

type SdeFixedOpts<T> = {
    structure: SdeStructure,
    method: SdeMethod,
    seed: Int,
    t0: Float[T], dt: Float[T], steps: Int,
    terms: Int,                    # Lévy 面积的 Fourier 模数；0 = 默认 clamp(ceil(1/√h), 16, 4096)，h 为步长
}

type SdeAdaptiveOpts<T> = {
    structure: SdeStructure,
    method: SdeMethod,
    seed: Int,
    t0: Float[T], t1: Float[T],
    rtol: Float[1], atol: Float[1],
    max_steps: Int,
    cells: Int,                    # 非交换自适应的 Brownian 网格 cell 数；0 = 默认 4096，可交换噪声忽略
    terms: Int,                    # 同上；h 取 cell 宽度 (t1 - t0)/cells
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
                  # s = { l: τ, … }    : { l: GeneralDiffusionOf<τ>, … }（记录状态，见下）

wiener           : (Int, Int, Int, Float[T]) -> Tensor<2, Float[T^(1/2)]>
                   # (seed, m, steps, dt)；与 manifest 的 sde_wiener(seed, m, steps, h) 同序
sde_order        : SdeMethod -> { strong: Float[1], weak: Float[1] }
                   # 可交换噪声下的阶
sde_order_with   : (SdeMethod, SdeStructure) -> { strong: Float[1], weak: Float[1] }
                   # 按结构给阶：HeunStratonovich 在不可交换噪声上只有强 1/2
sde_ensemble_seed: (Int, Int) -> Int     # (seed, index)，纯函数

type SdeEvent<s> = {
    g: (Float[T], s) -> Float[1],  # 变号即事件，无量纲
    terminal: Bool,                # true：路径在第一个这样的事件处截断
    direction: Int,                # -1 下降，0 两个方向，1 上升
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

**连续输出（`saveat`、可在任意 `t` 调用的路径）。** 数值 SDE 路径只在节点上已知。
`sde_at(path, t)` 取 `t` 处的状态，`sde_resample(path, ts)` 是 `saveat`（逐个时刻取值，
结果 `Tensor<1, s>`），`sde_interp(path)` 返回可调用的 `Float[T] -> s`。插值是节点之间的
**线性弦**：连续、确定、节点处精确，精度与格式本身同阶（强阶至多 1，Brownian 路径在
节点间本来只有 Hölder-1/2）；它**不**补回步内的 Brownian 桥方差，所以节点之间的统计量
比真过程低 `O(dt)`，需要精确的节点分布统计时直接用 `y`。区间外是 `E0802`，`t` 的量纲
不是路径的时间量纲是 `E0401`。路径本身仍是**纯数据**（没有闭包字段），因此可以跨线程
返回；可调用对象由 `sde_interp` 单独构造。

**事件。** `sde_events(path, events)` 在离散路径上检测 `g` 的符号变化（节点上
`g0 < 0 ≤ g1` 上升、`g0 > 0 ≥ g1` 下降，`direction` 过滤；与 ODE 事件同约定），
穿越时刻是弦上 `g` 的根（二分），状态是弦上的插值。返回 `{ path, hits }`：`hits` 按时间
排序；若有 `terminal` 事件，`path` 在第一个终止命中处截断（命中点作为最后一个节点，
`steps` 随之更新），其后命中丢弃，否则 `path` 原样返回。因为只观察离散路径，两个节点
之间"穿越后又返回"的连续过程事件是看不见的（任何离散格式都如此）。终止事件是**事后**
截断：同一随机流上与"在事件处停止"得到相同的路径，但不会节省被丢弃部分的计算。
`g` 返回有量纲的值是 `E0302`，`direction` 越界是 `E0802`。

**集合（并行、确定）。** `sde_ensemble_diag/general(drift, diffusion, y0, opts, n)` 积分 `n`
条路径，第 `i` 条使用种子 `sde_ensemble_seed(seed, i)`，其余选项相同；返回长度 `n`
的 `Tensor<1, Result<SdePath<s>, SdeError>>`（`n ≤ 0` 是 `E0802`；个别成员的失败是该
成员的 `Err`）。成员 `i` 只依赖 `(seed, i)`，所以**结果与执行顺序、线程数无关**。成员
经 §9.7.2 的无共享副本并行执行（成员数不少于 32、drift/diffusion 可跨线程、未
`--jobs 1` 时），否则在本线程顺序执行，两条路径给出逐位相同的值（回退由
`AEVIA_PARALLEL_TRACE=1` 报告）。等价的手写形式是
`par_tabulate(n, fn i => sdeint_diag(drift, diffusion, y0, <opts 的种子换成 sde_ensemble_seed(seed, i)>))`。

**分派规则**：**方法名选择方案族和积分约定，结构选择可交换还是不可交换
内核**。因此每个方法名在每种结构上都可用，不再有 `E1001`：

| method | calculus | 可交换（`Diagonal`、`General`） | 不可交换（`GeneralNoncommutative`） | strong/weak |
|---|---|---|---|---|
| `EulerMaruyama` | Itô | Euler–Maruyama | Euler–Maruyama | 1/2, 1 |
| `Milstein` | Itô | Milstein | Milstein + Lévy 面积（`milstein_noncommutative`） | 1, 1 |
| `MilsteinStratonovich` | Stratonovich | Milstein (Strat.) | Milstein (Strat.) + Lévy 面积（`milstein_noncommutative_stratonovich`） | 1, 1 |
| `HeunStratonovich` | Stratonovich | Heun | Heun | 1, 1；**不可交换时 1/2, 1** |
| `SrkPlaten` | Itô | Platen 无导数 SRK | Kloeden–Platen SRK（11.1.7，`srk_noncommutative`）+ Lévy 面积 | 1, 1 |
| `MilsteinNoncommutative` | Itô | 同下（显式别名，任何结构） | Milstein + Lévy 面积 | 1, 1 |
| `MilsteinNoncommutativeStratonovich` | Stratonovich | 同下（显式别名，任何结构） | Strat. Milstein + Lévy 面积 | 1, 1 |

内核文档明确没有强于一阶的方案，Milstein 系也只是弱一阶，因此不声称弱二阶。
`sde_order(method)` 给可交换噪声下的阶；`sde_order_with(method, structure)`
按结构给阶（`HeunStratonovich` 没有 Lévy 面积项，在不可交换噪声上降为强
1/2——允许调用，但阶如实报告）。迭代积分：Itô `I_jl = ½ΔW_jΔW_l − ½δ_jl h + A_jl`，
Stratonovich `J_jl = I_jl + ½δ_jl h = ½ΔW_jΔW_l + A_jl`（`A` 是 Lévy 面积）。
SRK 的支持值 `Υ^j = Y + a h + b^j √h`（每个噪声通道一个，每步多 `m` 次扩散
求值），`Y' = Y + a h + Σ_j b^j ΔW_j + h^(-1/2) Σ_{j,l} (b^l(Υ^j) − b^l(Y)) I_jl`
（Stratonovich 版用 `J_jl` 与 Stratonovich 漂移）。

**Lévy 面积的 `terms` 默认值**：需要面积的路径（不可交换结构上的
`Milstein`/`MilsteinStratonovich`/`SrkPlaten`，以及两个 `*Noncommutative*`
方法）在 `terms == 0` 时取 `clamp(ceil(1/√h), 16, 4096)`，`h` 为步长（自适应
时为 Brownian cell 宽度 `(t1 − t0)/cells`）；显式 `terms > 0` 原样使用。
`terms` 不再是必填项。

`General`/`GeneralNoncommutative` 的 `m` 是噪声通道数；`sdeint_general` 在
运行时检查扩散矩阵形状是 `n × m`（`E0802`），`SdePath.noise` 报告实际
`m`（对角时是 `n`）。

**记录状态 + 一般噪声**：`sdeint_general` 接受记录状态。扩散返回**同形状
的记录**，其中每个标量字段是 `m` 个系数的 `Tensor<1, Float[d/T^(1/2)]>`，
秩 `r` 的张量字段变成秩 `r+1` 的张量（末尾多一个噪声轴 `[.., m]`）：
`GeneralDiffusionOf<{ l: τ, … }> = { l: GeneralDiffusionOf<τ>, … }`。运行时
按**状态记录的字段顺序**取字段、深度优先、行主序展平，恰好是内核的
`n × m` 行主序矩阵（标量字段是一行）。例：`{ x: Float, v: Tensor<1, Float> }`
（`n = 1 + k`）+ `General(2)` 的扩散返回 `{ x: [σx1, σx2], v: Tensor<2> [k×2] }`。

**步长策略**：`Fixed` 是定步长 `dt`、共 `steps` 步（时间网格
`t0 + k·dt`）；`Adaptive` 在 `[t0, t1]` 上用局部误差 `rtol`/`atol` 控制，
Brownian bridge 拒绝/重采（内核）。**所有方法都支持自适应**：

- Itô 可交换（`EulerMaruyama`/`Milstein`/`SrkPlaten`）：内核的自适应控制器
  返回 Milstein 解（其误差估计是 EM-vs-Milstein），所以三者给出同一条
  受控路径，方法名保留请求值；
- **Stratonovich 自适应 = 精确 Itô 转换**：漂移换成 Itô 等价漂移
  `ã_i = a_i + ½ Σ_j Σ_k b_kj ∂_k b_ij`（修正项用沿扩散各列方向的中心差分
  计算，步长标度与 Milstein 项的差分一致），然后在 `(ã, b)` 上跑 Itô 控制器。
  返回的路径是**等价 Itô SDE 的受控 Itô–Milstein 解**，收敛到原系数的
  Stratonovich 解（对 `dX = σX∘dW` 收敛到 `X0·exp(σ W_T)`）；
- 不可交换噪声（以及两个 `*Noncommutative*` 方法）：细 cell 网格 + Chen
  关系的控制器（`cells`，0 = 默认 4096），Stratonovich 方法同样先转 Itô。
  单个 cell 仍被拒绝时报 `E0807`，需要更多 `cells`。

**确定性**：`seed` 必填，没有默认值。定步长可交换路径的增量来自语言 RNG
流（与 `rng(seed)` 相同的 LCG，正态用 Box–Muller），因此
`wiener(seed, m, steps, dt)` 与同 seed 的定步长路径消费**同一组数**；同
构建内可复现，不承诺跨平台逐位相同。非交换定步长与所有自适应路径需要内核
自己的 xoshiro 流（bridge 拒绝与 Lévy 面积在求解过程中采样），由 `seed`
播种，同样只承诺同构建可复现。`sde_ensemble_seed` 只依赖 `(seed, index)`，
所以集合成员与执行顺序/线程数无关；`sde_ensemble_diag/general` 在语言层并行运行
成员（见下）。

**错误表**（不新增 Appendix-B 码）：`E0802` 形状/参数（含 `General` 的
`m` 与扩散矩阵不符、非均匀网格、缺失必填项）；`E0806` 状态非有限；
`E0807` 自适应步长塌缩或步数预算耗尽；`E1001` 保留给真正未实现的能力
（如 `core.sde`、`sde.events`），当前没有任何方法/结构/步长组合产生它——
原来的 `sde.noncommutative`、`sde.interpretation`、`sde.general.record`
三个缺口已关闭。`SdeError` 是不透明错误类型（像 `SimError`），语言层
不暴露构造器。

**v1 明确不做**：用户自定义噪声结构、显式非均匀网格（定步长内核假设均匀步长，报 `E0802`；需要变
步长时用 `Adaptive`）、求解器 manifest 算子（求解器需要回调，无法越过
manifest ABI）、Brownian 桥稠密输出（`sde_at` 是线性弦，见上）、在积分**过程中**提前停止
的终止事件（`sde_events` 是事后截断）。事件与稠密输出已在上面实现（原先此处列为不做）。

> **实现状态（2026-11-08，SDE 非交换/Stratonovich 补全轮）**：运行时与
> `aevia-std` 的 SDE 求解器已落地，下面的组合也已用源码程序实际跑通（见
> §8.20 的模块导入注）。可用的组合：
>
> - **方法**：`EulerMaruyama`、`Milstein`、`MilsteinStratonovich`、
>   `HeunStratonovich`、`SrkPlaten`、`MilsteinNoncommutative`、
>   `MilsteinNoncommutativeStratonovich` 都能积分，且**每个方法在每种结构上
>   都可用**（方法名选方案族和积分约定，结构选可交换/不可交换内核，见上面的
>   分派表）；内核新增 `milstein_noncommutative_stratonovich`、
>   `srk_noncommutative`、`srk_noncommutative_stratonovich`（强阶 1，
>   Kloeden–Platen 11.1.7/11.1.10）与自适应 Stratonovich 求解器
>   `sde_adaptive_stratonovich_with`/
>   `sde_adaptive_noncommutative_stratonovich_with`（精确 Itô 转换）。强收敛
>   斜率由 `rssn/tests/sde_noncommutative.rs` 在 2 维非交换线性 SDE 上实测
>   （> 0.8，EM 约 0.5）；
> - **结构**：`Diagonal`（标量与记录状态）、`General(m)`、
>   `GeneralNoncommutative(m)` 可用；`sdeint_general` 的状态可以是标量、rank-1
>   张量或**记录**（扩散为带噪声轴的同形状记录，见上）。扩散矩阵 `n×m` 在
>   运行时校验；
> - **步长**：`Fixed`（`t0`/`dt`/`steps`，均匀网格）与 `Adaptive`
>   （`[t0,t1]`、`rtol`/`atol`、`max_steps`、`cells`），所有方法都支持；
>   Stratonovich 自适应走精确 Itô 转换，不可交换需要的 `terms`/`cells`
>   有默认值。源码验证：`Fixed` 20 步对角路径给出 `steps=20, nfev=40`；
>   两条路径两次运行输出逐位相同（同 seed/同构建）。源码级回归测试在
>   `aevia-core/tests/sde_language.rs`（构造策略并积分、可复现、diffusion
>   量纲检查、缺失字段是静态错误，以及不可交换 Milstein/Strat./SRK、自适应
>   Stratonovich 与记录状态 + 一般噪声的积分与统计检查）；
> - **两条 RNG 流**：可交换定步长路径消费与 `wiener(seed,m,steps,dt)`/
>   `rng(seed)` 相同的语言 LCG 流；非交换定步长与自适应路径用内核 xoshiro
>   流，由 `seed` 播种；只承诺同构建可复现，不承诺跨平台逐位一致；
> - **确定性**：`seed` 必填；`sde_ensemble_seed(seed,index)` 是纯函数，成员
>   与执行顺序/线程数无关。**ensemble runner 已接线**（`sde_ensemble_diag`/`sde_ensemble_general`，经并行副本，
>   见上）；
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
>   缺失或类型不对得到 `E0802` 的 `SdeError` 值，不会自动填默认值（`terms`
与 `cells` 字段必须存在，但值 `0` 表示默认）；
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
>   与 §8.9.1 的 `sde_order`/`sde_order_with`（返回 `{strong, weak}` 表）互补：需要弱阶时用
>   `sde_order`，需要 flag/自适应标记时用 `sde_scheme_info`（它的 8 个方案
>   名尚未包含本轮新增的三个内核方案）。生成源是精确的
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

> **生成的仿真模块（2026-11-13）**：`import Rssn_Sim_<Name>`（如
> `Rssn_Sim_PoissonMultigrid`、`Rssn_Sim_HeatNd`）在导入时由 rssn 的场景目录
> （`rssn::sim::schema`）生成类型化模块：`Params`/`Output` 记录、每个方法名
> 字段一个变体类型（`P.W`、`S.Metropolis`）、`defaults()`（场景自身的默认请求或
> 内置示例）与 `run(p)`。参数类型与默认值从场景自己的请求类型推断，结果类型
> 来自一次见证运行，注解表补充推断看不到的部分（可选值、单位、文档）；rssn
> 新增场景、参数或方法时这里无需手写任何代码。29 个场景全部可生成。参考测试：
> `tests/sim_catalog.aev`。

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
- `Rssn.register`/`Rssn.register_plugin` 接受 host `Plugin`；Aevia 源码不持有
  这种值（运行期 `E0302`，附注指向源码机制）。源码插件经声明发布（§13.5.2）：
  `@op`/`@rule` 函数在模块加载时注册，`load_plugin(path) : String -> IO
  (Result<Int, String>)` 在运行期加载一个插件文件。
- `core.callback` 标记上述受限的回调桥可用；`core.plugin.source` 现为
  `enabled = true`。生成的高阶绑定保留显式 `(values, callbacks)` 动态接口；
  对声明精确、回调和结果结构均可认证的单重载，另生成钉住签名的 typed 别名。
  别名在调用时认证回调的具体签名，并按实例化结果类型验证返回值；无法证明
  类型或维度的签名仍只提供动态入口。

`Rssn_numerical_callback` 中 `num_least_squares` 的生成 typed 别名接收初始
参数向量、残差容差、梯度容差、步长容差、初始阻尼、迭代与求值预算以及
残差回调。参数与残差分别具有量纲 `D`、`E`；初始向量确定参数长度 `N`，
回调具体签名可约束残差长度 `M`，否则求解过程中要求实际残差长度保持不变。
结果保留最佳参数、残差范数、迭代/残差求值/Jacobian 求值及接受/拒绝步数，
并以 `converged` 和 `status` 区分残差或梯度收敛、预算耗尽和停滞。残差
容差使用 `E`；其余数值控制量无量纲。回调错误原样传播。typed 别名名称可由
绑定清单查询，其签名来自 RSSN 声明并固定到对应 overload。

`Rssn.dynamic` 是**正式能力**，不是降级路径：插件算子、叶子算子、
实验性入口都走它。多注册时 RSSN 在运行时按
`Exact > Widened > Shaped > Generic > No` 选择，并列时报 `E1002`；参数不被
任何 overload 接受时报 `OpError::Type`（`check_args`）。标准集每个名字只有
1 个注册，所以这条路径不做注册级评分，但**会**做签名级校验。
`Rssn.call_key` 则不解析，直接执行 name + `signature_hash` 指定的
overload。

### 8.19.1 数值域模块 `Rssn_Numeric`

rssn 的 `numeric` 规则集把一批数值内核做成算子（表驱动，只对字面数字求值；符号参数、
参差列表、被拒绝的参数、不收敛都让调用保持为请求，包装器报告 `Err(NotReduced(..))`）。
Aevia 把它们归入 `Rssn_Numeric`，其中 67 个有手写的 typed 签名
（`domains.overlay` 的 `sig`，一律 `Result<_, OpError>`），其余留在 dynamic 档。
`import Rssn_Numeric as N` 之后：

| 组 | 算子（Aevia 签名） |
|---|---|
| 稳健统计 | `mad`、`mad_sigma(xs: Tensor<1, Float>): Float`，`trimmed_mean`、`winsorized_mean(xs, proportion)`，`huber_location(xs, k)`，`biweight_location(xs, c)`，`ecdf(xs, x)`，`kde(xs, x, bandwidth)`，`bandwidth_silverman`、`bandwidth_scott(xs)`，`quantile_hf(xs, p, kind: Int)`（Hyndman–Fan 1–9），`bootstrap_ci(xs, statistic, alpha, resamples, seed): Tensor<1, Float>`（种子固定，可复现） |
| 窗与谱 | `hann_window`、`hamming_window`、`blackman_window(n: Int)`，`cross_correlation`、`fftconv(xs, ys)`，`fft_abs`、`fft_re`、`fft_im(xs)`，`ifft_re`、`ifft_im(re, im)`，`frft_re`、`frft_im(re, im, order)`（分数傅里叶），`interpolate(xs, ys, x)` |
| 小波 | `dwt_approx`、`dwt_detail(xs, wavelet)`，`idwt(approx, detail, wavelet)`，`wavedec(xs, wavelet, levels): Tensor<1, Tensor<1, Float>>`，`waverec(coefficients, wavelet)`，`dwt2(m, wavelet): Tensor<3, Float>`（`[LL, LH, HL, HH]`），`idwt2(ll, lh, hl, hh, wavelet)` |
| Krylov、低秩、排序 | `minres_solve`、`fgmres_solve`、`bicgstab_l_solve(a, b, tol)`，`sparse_solve`、`amg_solve(entries, b, tol, method)`（三元组 `(i, j, v)`，0 起始），`randomized_singular_values(a, k)`，`lobpcg_eigenvalues(a, k, tol)`，`qr_pivoted_rank`、`qr_pivoted_permutation`、`qr_pivoted_diagonal`，`nested_dissection_order(adjacency)`，`polynomial_system_roots(system): Tensor<3, Float>`（同伦延拓） |
| 采样函数 | `function_l1_norm`、`function_l2_norm`、`function_inf_norm(points: Tensor<2, Float>)`，`function_inner`、`function_project(f, g)`，`function_normalize(f)`；函数是升序的 `(x, y)` 行表 |
| 复变特殊函数 | `lgamma_complex`、`gamma_complex`、`digamma_complex(re, im)`，`lambert_w_complex(re, im, branch: Int)`，`bessel_j_complex`、`bessel_i_complex(order, re, im)`；结果是 `[re, im]` |
| 表达式算子 | `oscillatory_integral`（Levin 配点），`anderson_fixed_point`，`newton_cg_minimize`（符号梯度与 Hessian），初值问题 `ode_solve`（`dopri5`、`rosenbrock23`、`radau5`、`bdf`、`sdirk3`、`exprb32`）、`ode_symplectic`（Verlet 与 Yoshida 组合）、`ode_imex`、`ode_etdrk4`；右端项与变量是 `Real` 符号项，其余参数是数字 |

约定与限制：

- **结果是 `Float`**。内核对这些算子只编码浮点数，所以计数与 0 起始下标
  （`qr_pivoted_rank`、`qr_pivoted_permutation`、`nested_dissection_order`）也以 `Float` 返回；
  `from_value<Int>` 不会接受它们。`Real`（精确）不是它们的结果类型。
- **方法、小波、统计量用符号命名**，符号在 Aevia 里是 `Real`：
  `let haar = symbol("haar")`，再 `N.dwt_approx(xs, haar)`。**每次** `symbol("haar")` 都造一个新符号
  （`haar#2`、`haar#3`…），内核不认识；所以每个名字只在顶层 `let` 一次再复用。
- **维度无关，按维数/秩命名**。`heat_fd`、`wave_fd`、`poisson_fd`、`advect_fd` 在内核里不分维度：空间维数就是场数据的嵌套深度
  （列表一维，列表的列表二维，三层三维；定义域是单位区间/方块/立方体，`u[i][j][k]` 是节点
  `(x_i, y_j, z_k)`）。一个 Aevia 签名只有一个字面量秩，所以一个内核算子带**多条签名，各用自己的名字**：
  不带后缀的名字是一维形式（`poisson_fd(f: Tensor<1, Float>, boundary: Float)`），`_2d`/`_3d` 取 `Tensor<2, Float>`/
  `Tensor<3, Float>` 场（`heat_fd_2d`、`wave_fd_3d`、`poisson_fd_2d`、`advect_fd_3d` …），答案与场同形状；
  平流速度仍是 `Tensor<1, Float>`，每个空间维一个分量。
  秩多态的张量算子同理按秩命名：`tensor_norm_<r>`、`tensor_inner_<r>`（r = 1..4）与
  `tensordot_<ra>_<rb>_<k>`（收缩 k 个轴，`axes_a`/`axes_b: Tensor<1, Int>` 从 0 起，结果秩 ra + rb − 2k，0 时为 `Float`；
  结果秩至多 4）。分解 `hosvd_<r>`、`hooi_<r>`、`cp_als_<r>`、`tt_svd_<r>`、`tt_dense_<r>`（r = 2、3、4）的结果是记录
  `Hosvd<r>`、`Hooi<r>`、`CpFit`、`TensorTrain`（字段见 `Rssn_Numeric` 前言；因子表是 `Tensor<1, Tensor<2, Float>>`）；
  混合列表由通用的列表访问器 `from_value<Tensor<1, Value>>` 拆开再逐部分解码（§10.3.8）。
  符号 PDE 同样只有一个名字：`solve_heat_equation`、`solve_wave_equation`、`solve_laplace_equation`、
  `solve_poisson_equation`（维数由方程的变量决定，旧的 `_1d`/`_2d`/`_3d` 是别名）；
  仿射变换 `translation`、`scaling`、`shear`、`reflection`、`rotation` 同理，归入 `Rssn_Graphics`。
- 参差输入（邻接表、项数不同的多项式方程）没有字面量写法，用 `map(f, range(0, n))` 构造
  `Tensor<1, Tensor<1, Float>>`。

端到端示例与封闭形式核对：`tests/domain_numeric.aev`（45 个 `test`：`mad([1,2,3,4,100]) = 1`、Hann 窗对称、
`ifft(fft(x)) = x`、小波分解/重构往返、`poisson_fd` 的 `-u'' = 1` 得 `x(1-x)/2`、`heat_fd` 的 `exp(-π²t)` 衰减、
`tensordot_2_2_1` 对手算矩阵乘、`poisson_fd_2d`/`heat_fd_2d` 的封闭形式、`hosvd_3`/`tt_svd_3` 的重构、`ode_solve` 的 `y' = -2y` 得 `e^-2`、`lambert_w_complex(1,0,0) = 0.5671…` 等），
由 `cargo test -p aevia-rssn --test aev_suites` 经真实驱动运行。

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
`Csr`/`Csc`/`Coo`/`Block`/`Bcsr` 格式标签与 §8.4.1 的全部稀疏内建
（`nnz`/`to_dense`/`from_dense`/`convert`、稀疏 `matmul`/`transpose`/`kron`、
`@` 的布局分派、`spsolve`/`cg`）；`Layout` 仍是内建函数集合而不是可实例化的
类型类（§13.9.5）；`Device` 提供 `Cpu`/`Gpu`/`Distributed` 与
`on`/`to_device`（§13.9.4，无 `core.gpu` 时 `E1001`）。

下一章定义运行时：从源码到值，从值到 rssn 项，从 REPL 到 JIT。
