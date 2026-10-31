# 06 — 求值语义

这一章定义 Aevia 程序运行时的行为：值是什么、表达式按什么顺序求值、符号与
数值如何共存、张量如何表示、IO 如何执行、错误如何报告。

## 6.1 求值策略

Aevia 是**严格求值、按值调用、从左到右**的语言：

- 函数参数在调用前求值。
- 二元运算符先求左操作数，再求右操作数。
- `let` 的右侧立即求值，然后绑定。
- 记录字段按书写顺序求值；张量元素按行优先顺序求值。
- `if` 只求值被选中的分支；`&&`/`||` 在条件可判定时短路。
- `match` 只求值匹配的分支。

严格求值让性能可预测，也让符号表达式的构造顺序确定。纯函数没有副作用，
所以“先算哪个”不影响结果，只影响资源消耗。

## 6.2 值

```
v ::= () | true | false
    | n : Int
    | r : Real
    | f : Float
    | c : Complex
    | s : String
    | tensor(shape, storage)
    | sparse(format, shape, storage)
    | table(schema, columns)
    | device(cpu|gpu|distributed)
    | quantity(value, unit)
    | value(dynamic)                 # RSSN 动态边界
    | plugin(handle)
    | { l1 = v1, ..., ln = vn }
    | C(v1, ..., vn)
    | closure(env, params, body)
    | io(action)
    | set(elements)
    | Ok(v) | Err(v) | None | Some(v)
```

### 6.2.1 标量表示

```
Real    ::= Exact(Number) | Sym(TermId)
Float   ::= F64(f64)                     # 严格是 f64，永不符号化
Complex ::= C(Real, Real) | C64(Complex64) | Sym(TermId)
Int     ::= BigInt
```

- `Number` 是 rssn 的精确数：任意精度整数或有理数。
- `TermId` 是运行时符号图中的项句柄；项可以含自由符号、精确数、浮点数。
- `Float` **只有** `F64` 形式；含符号的浮点表达式由 `Real` 承担。这样
  `Float` 的 IEEE 语义、`Ord`、JIT 都是干净的。
- `Complex` 的 `C(re, im)` 用两个 `Real` 表示精确/符号复数；`C64` 是
  `eig`/`fft` 等数值内核返回的 `Complex64`；`Sym` 是符号复数项。类型
  `Complex[d]` 不区分精确/近似，需要精确性时用 `re`/`im` 检查。
- 显示单位是标量值的元数据 `Option<UnitRef>`，不参与相等。

### 6.2.2 闭包

闭包捕获词法环境。递归函数通过环境中的自引用绑定实现；运行时用
`Rc<Closure>` + 弱引用打破循环，或用会话级 arena + 周期回收。语义上，
递归函数是值，不是特殊形式。

### 6.2.3 IO 动作

`IO a` 是一个动作描述：一个接受世界令牌、返回 `a` 的 thunk。Aevia 不暴露
世界令牌；运行时在执行 `main` 时提供。`IO` 动作可以组合、传递、存储，但
只有运行时能执行。

## 6.3 精确、浮点与符号

### 6.3.1 三种相位

一个标量值可以处于三种相位：

1. **精确**：`Exact(Number)`，例如 `1/3`、`2`、`sqrt(2)` 的符号形式？
   `sqrt(2)` 是符号项，不是精确数；精确数只含有理数。
2. **符号**：`Sym(TermId)`，含有自由符号或未化简的算子，例如 `x^2 + 1`、
   `sin(1)`、`sqrt(2)`。
3. **浮点**：`F64`，已求值的 IEEE 数。

`Real` 可以在精确与符号之间移动；`Float` 只有 f64，不参与符号相位。
`approx` 把符号/精确变成 f64（如果无自由符号）。

### 6.3.2 算术分派

对二元运算 `op`：

```
Exact(a) op Exact(b)：
    如果 op 在有理数上封闭且无定义域错误 -> Exact(a op b)
    否则 -> Sym(build(op, a, b))

Sym(_) op _ 或 _ op Sym(_)：
    -> Sym(build(op, ...))，除非能立即折叠

F64(a) op F64(b)：
    -> F64(IEEE op)

F64(_) 与 Exact(_) 或 Sym(_)：
    -> Real，先把 F64 精确转换成有理数，再走 Real 的规则
```

- `Real + Float` 的结果是 `Real`：`Float` 被精确转换成有理数，符号信息
  保留。不会因为符号而运行时失败。
- 要留在浮点路径，写 `to_float(x)` 或让所有操作数都是 `Float`。
- `Int / Int` 产生 `Exact` 有理数，不产生 `Float`。
- `0 / 0`、`1 / 0` 在精确相位是错误 `E0801`；在浮点相位是 NaN/Inf
  （IEEE）。
- `sqrt(-1)` 在 `Real` 相位构造符号项 `sqrt(-1)`；`Real` 的符号函数是
  **形式**的，不自动变成复数。需要复数时用 `complex_sqrt` 或 `Complex` 类型。

### 6.3.3 符号项的构造

`build(op, args)` 调用 rssn 的算子构造（`Session::call`），不立即运行重写
引擎。这样 `x + 1` 是 O(1) 的项构造，不会为每个 `+` 触发一次 e-graph 搜索。

只有以下边界触发 `compute`：

- 显式 `simplify`/`eval`；
- `diff`、`integrate`、`solve`、`limit`、`sum`、`product`、`series`、
  `dsolve` 等重型请求；
- `approx`；
- 打印时的可选轻量规范化（默认不做）。

这条边界是性能的关键：科学公式通常有几百个算子，如果每个 `+` 都跑一遍
重写，交互式使用会不可接受。

### 6.3.4 定义域与类型不变量

- `Real` 的 `sqrt` 只对非负值有定义；负值构造符号项，不自动变成复数。
  需要复数时用 `complex_sqrt` 或 `Complex` 类型。
- `ln(0)` 返回 `-oo`（`Real` 的无穷）；`ln(-1)` 构造符号项 `ln(-1)`。
- `asin(2)` 构造符号项；`approx` 会失败并报 `E0806`。
- `Real` 的符号函数是**形式**的：`sqrt(-1)` 保持 `sqrt(-1)`，`ln(-1)`
  保持 `ln(-1)`，不会自动变成复数。桥接层 MUST 关闭 rssn 中把实域表达式
  化简为复数的规则，或在返回前检查值域；破坏 `Real` 的值域是内部错误
  `E0809`。
- `approx` 对形式表达式尝试数值化；定义域外的结果报 `E0806`。

### 6.3.5 `eval`、`simplify`、`approx`

```
eval      : Real[d] -> Real[d]          # 符号化简，可能仍含符号
simplify  : Real[d] -> Real[d]          # eval 的别名
approx    : Real[d] -> Result<Float[d], EvalError>
approx    : Float[d] -> Result<Float[d], EvalError>
```

- `eval` 用 `Target::Symbolic` 运行 rssn；返回最简闭式或原式。
- `approx` 用 `Target::Numeric`，需要表达式无自由符号；有自由符号报
  `E0803`。容差默认 `1e-12`，可用 `approx(x, tol)`。
- `eval` 不保证终止；受预算限制（§6.9）。
- `simplify` 是 `eval` 的同义词，保留给可读性。

### 6.3.6 符号身份与替换

```aevia
let x = symbol("x")       # 新鲜符号，显示名 x
let y = symbol("x")       # 另一个符号，显示名也是 x
```

- 每次 `symbol("x")` 创建**新鲜**符号；内部名唯一（`x`、`x#2`），显示名
  相同。这样不同推导中的 `x` 不会意外串味。
- `subst(x, v, e)` 按符号身份替换；`x` 必须是符号值。
- `subst_all([(x, 1), (y, 2)], e)` 批量替换。
- 符号相等 `x == y` 是命题 `Prop`，不是 `Bool`；`if` 不能直接用它，需
  `decide(x == y)` 或 `assuming`。
- 打印时同名符号可能看起来一样；`repr` 会带内部 id 以区分。

## 6.4 微积分与方程的实现语义

### 6.4.1 `diff`

```
diff     : (Real[d] -> Real[e]) -> (Real[d] -> Real[e/d])
diff_at  : ((Real[d] -> Real[e]), Real[d]) -> Real[e/d]      # = diff(f)(x)
numeric_deriv : (Float[d] -> Float[e]) -> (Float[d] -> Float[e/d]) # 数值/AD，非符号
```

求值 `diff(f)`：

1. 创建新鲜符号 `ξ : Real[d]`。
2. 把 `f` 应用到 `ξ`，得到 `Real[e]` 的符号项 `t`。
3. 构造 `diff(t, ξ)`，用 `Target::Symbolic` 计算，得到 `t'`。
4. 返回闭包 `fn (x) => subst(ξ, x, t')`。

如果 `f` 在构造 `t` 时对符号值做了不可符号化的操作（例如 `if ξ > 0`），
`if` 要求 `Bool` 而 `ξ > 0` 是 `Prop`，报类型错误 `E0302`。这是符号求导的
固有限制：分段函数的导数需要用户显式分段（`by_cases`/`assuming`）。

### 6.4.2 `antiderivative` 与 `defint`

```
antiderivative : (Real[d] -> Real[e]) -> (Real[d] -> Real[e*d])
defint         : (Real[d] -> Real[e]) -> Real[d] -> Real[d] -> Result<Real[e*d], MathError>
```

`antiderivative(f)` 同样创建 `ξ`，构造 `integral(t, ξ)`，计算，返回闭包。
不定积分带常数；Aevia 约定常数为 0。

`defint(f,a,b)` 构造 `defint(t, ξ, a, b)`，计算。失败返回 `Err`。

### 6.4.3 `limit`、`series`、`sum`、`product`

- `limit(f, a)`：`limit(t, ξ, a)`。
- `series(f, a, n)`：`series(t, ξ, a, n)`，返回带 `O(...)` 的表达式。
- `sum(f, a, b)`：索引符号 `κ : Real[1]`，构造 `sum(f(κ), κ, a, b)`。
- `product(f, a, b)`：仅当 `f` 无量纲；否则用 `product_dyn` 返回 `Dyn<ρ>`。

### 6.4.4 `solve` 与 `nsolve`

```
solve    : (Real[d] -> Real[e]) -> Result<Solutions<Real,d>, MathError>
nsolve   : (Real[d] -> Real[e]) -> Real[d] -> Result<Real[d], MathError>
nsolve_f : (Float[d] -> Float[e]) -> Float[d] -> Result<Float[d], MathError>
```

`solve(f)` 把 `f(ξ) = 0` 交给 rssn 的 `solve`；结果转成
`Solutions<Real,d>`（根 + 重数 + 参数条件），而不是 `Set`。`Set` 要求 `TotalOrd`，符号根
没有全序。无解返回空 `Solutions`；无法求解返回 `Err`。`nsolve` 是数值
求根；`nsolve_f` 走浮点内核。

### 6.4.5 `dsolve` 与 `SymFun`

```aevia
let y : SymFun<T, L> = symfun("y")
let eq = diff(y)(t) + y(t) == 0      # eq : Prop
let sol = dsolve(eq, y, t)           # Result<(Real[T] -> Real[L]), MathError>
```

- `SymFun<d,e>` 是未知函数符号；应用它到 `Real[d]` 构造项。
- `dsolve` 接受 `Prop` 方程、未知函数、自变量；返回解函数。
- 解中的积分常数用 `C1`、`C2` 表示；用户可以 `subst` 或作为函数参数。
- 数值 ODE 用 `odeint`（§8.9），不走 `dsolve`。

## 6.5 模式匹配

- 按分支顺序尝试；第一个匹配的分支胜出。
- 构造器模式按标签匹配；记录模式按字段匹配（开放）；元组模式按位置。
- 张量模式 `[a, b, ..rest]` 只匹配 rank 1，长度至少 2；`rest` 绑定剩余
  元素的切片。
- 字面量模式：`Int`、`Bool`、`String`；浮点字面量不是合法模式。
- 守卫在模式绑定后求值，必须是具体 `Bool`；`Prop` 报 `E0308`。
- 非穷尽匹配在编译期报 `E0309`；运行时不会“掉出去”。

## 6.6 张量运行时

### 6.6.1 表示

```
TensorData {
    shape: Vec<usize>,          # 行优先
    storage: F64(Vec<f64>)
           | C128(Vec<Complex64>)
           | Exact(Vec<Number>)
           | Boxed(Vec<Value>)
           | Bool(Vec<bool>),
    display: Option<UnitRef>,
}
```

- `Float` 元素用 `F64`；`Complex` 用 `C128`；`Int`/`Real` 用 `Exact` 或
  `Boxed`；`Bool` 用 `Bool`。
- 混合类型在构造时提升到最宽的存储；`[1, 2.0]` 用 `F64`。
- 形状是 `Vec<usize>`；秩是长度，与静态类型 `Tensor<n, τ>` 的 `n` 一致
  （运行时断言）。
- 显示单位作用于整个张量；元素级单位不单独存储。

### 6.6.2 构造

- 字面量：递归检查矩形；计算形状；统一元素类型；展平存储。
- `zeros`/`ones`/`fill`：分配连续存储。
- `range`/`linspace`：生成 `F64` 或 `Exact`。
- 从 CSV/JSON 读入：先按 `Float` 解析，再由调用者标注量纲。

### 6.6.3 广播

同秩广播规则：

```
shape1 ~ shape2:
    对每一维 i：
        d1 == d2 -> d
        d1 == 1  -> d2
        d2 == 1  -> d1
        否则 -> ShapeError E0802
```

标量与张量：标量广播到所有元素。不同秩：编译错误（§3.5.3），除非显式
`reshape`。

### 6.6.4 索引与切片

- 负索引 `-1` 表示最后一维的最后一个元素。
- 越界 `E0802`。
- `a[i]` 返回子张量视图（rank 减一）；视图共享底层缓冲区，不可变语义下
  安全。
- 切片 `a[i..j]` 返回视图；`reshape` 在连续内存上返回视图，否则复制。
- 写入不存在；所有“修改”返回新张量，运行时在唯一持有时原地完成。
- `set(a, idx, v)` 是函数式更新：返回新张量，原张量不变；唯一持有时原地。
- `at(a, i)` 是秩多态索引：`Tensor<n,τ> -> Tensor<n-1,τ>`，要求 `n>=1`。
- `a @ b` 按秩分派：2×2 → `matmul`，2×1 → `matvec`，1×2 → `vecmat`，
  1×1 → `dot`（§3.5.3a）。

### 6.6.5 线性代数

- `F64`/`C128` 的 `matmul`、`solve`、`inv`、`eig`、`svd`、`qr`、`lu` 走
  rssn 的数值内核（faer/ndarray）。
- `Exact`/`Boxed` 的小矩阵走 rssn 的符号线性代数规则；大矩阵若无法数值化，
  返回 `Err` 或保持符号形式。
- `det` 只接受无量纲；带量纲用 `det_unit` 返回 `Dyn<ρ>`。
- 奇异矩阵 `solve` 返回 `Err(SingularMatrix)`；`inv` 同理。
- 特征值可能是复数；`eig` 返回 `Complex`，即使输入是实数。

### 6.6.6 归约

- `sum` 空张量 = 0；`prod` 空张量 = 1；`mean` 空张量 = `Err`；
  `min`/`max` 空张量 = `Err`。
- `sum(a, axis)` 保留秩（该轴大小为 1，keepdims）；`sum_all(a)` 返回标量。
- `sum` 的实现对 `F64` 用 pairwise/Kahan 求和，减少误差；`Exact` 用精确
  有理数求和。
- `var` 默认样本方差（除以 `n-1`）；`var(xs, population=true)` 除以 `n`。
- `std` 是 `sqrt(var)`。
- `norm` 默认 2-范数；`norm(xs, p)` 支持 1、2、∞。

## 6.7 记录、变体与集合

- 记录字段按名字排序存储；字段访问 O(log n) 或哈希 O(1)。
- 记录更新产生新记录；未变字段共享。
- 变体是 `(tag, payload)`；构造器是函数/值。
- `Set<a>` 是持久有序集合，要求 `TotalOrd a`；元素按 `total_compare` 排序，
  迭代确定性。`Set` 的 `==` 是集合相等（与插入顺序无关）。`Float` 不是
  `TotalOrd`，所以 `Set<Float>` 不合法；需要时用 `TotalFloat` 包装。
- `Map<a,b>` 同理，要求 `TotalOrd a`；标准库提供 `Map`，核心语言不特殊
  对待。
- `Solutions<ρ,d>` 是符号解集合：`{ roots: Tensor<1, Scalar<ρ,d>>,
  multiplicities: Tensor<1, Int>, conditions: Tensor<1, String> }`。它不要求
  `Ord`，保留重数和参数条件。

## 6.8 IO

### 6.8.1 执行模型

`main : IO Unit` 由运行时执行：

1. 初始化标准库和模块（纯求值）。
2. 构造 `main` 的 `IO` 动作。
3. 执行动作；`print` 写标准输出，`read_file` 读文件，`plot` 写 HTML。
4. 动作返回 `()`；未捕获的 `panic` 终止并打印调用栈。

### 6.8.2 常用动作

```
print    : Show a => a -> IO Unit
println  : Show a => a -> IO Unit
eprintln : Show a => a -> IO Unit
read_line: () -> IO String
read_file: String -> IO (Result<String, IOError>)
write_file: String * String -> IO (Result<(), IOError>)
read_csv : String -> IO (Result<Table, DataError>)
plot     : PlotSpec -> IO Unit
time_now : () -> IO Float[T]
random   : () -> IO Float[1]
```

- `print` 用 `show`；`println` 追加换行。
- `read_file` 返回 `Result`，不抛异常；`?` 可以传播。
- `plot` 把图写到当前目录的 `plot.html`（或 `--out` 指定）；返回 `IO Unit`。
- `random` 使用运行时 RNG；需要可复现时用 `rng(seed)` 纯 API。

### 6.8.3 纯度

- `IO` 动作不能出现在纯函数中；类型检查器拒绝。
- 纯函数可以接收/返回 `IO` 动作（作为值），但不能执行。
- 没有 `unsafePerformIO`。
- `main` 是唯一自动执行的入口；库函数不自动执行 IO。

## 6.9 预算、错误与资源

### 6.9.1 符号预算

rssn 的 `compute` 接受 `Budget`（时间、重写次数、节点数）。Aevia 默认：

```
时间     2 s
重写     1_000_000
节点     5_000_000
```

可用 `with_budget(seconds, rewrites, thunk)` 覆盖：

```aevia
let r = with_budget(10.0, 5_000_000) => simplify(integral(...))
```

预算耗尽报 `E0808`；`eval`/`simplify` 终止并返回错误（或 `panic`，取决于
API）。重型请求 `integrate`/`solve` 返回 `Result`，预算错误是 `Err`。

### 6.9.2 错误分类

| 代码 | 情况 | 可恢复 |
|---|---|---|
| `E0801` | 精确除法/取模除以零 | 否（`Real`）；浮点是 IEEE |
| `E0802` | 形状/索引错误 | 否 |
| `E0803` | `approx` 遇到自由符号 | 是（`Result`） |
| `E0804` | 符号条件无法判定 | 否 |
| `E0805` | 栈溢出（非尾递归过深） | 否 |
| `E0806` | 定义域错误（`asin(2)`、`ln` 负数） | 是（`Result`） |
| `E0807` | 数值算法不收敛 | 是（`Result`） |
| `E0808` | 预算耗尽 | 是（`Result`/`panic`） |
| `E0809` | 类型不变量被后端破坏 | 否（内部错误） |
| `E0810` | IO 错误 | 是（`Result`） |
| `E0899` | `panic` | 否 |

### 6.9.3 栈与尾调用

- 自递归尾调用用 trampoline，常量栈。
- 非尾递归受 `--max-stack` 限制（默认 8 MB 等价深度）；超过报 `E0805`。
- `panic` 打印调用栈（源码位置 + 函数名）。

## 6.10 确定性与并行

- 纯求值完全确定；相同输入得到相同输出（浮点 NaN 规范化）。
- `par_map(f, xs)` 并行映射，结果顺序与输入一致；线程数不影响结果。
  要求 `Parallel a`（仅 `Int`/`Float` 及其张量）；符号 `Real` 不并行。
- `compute_all` 的并行只影响速度，不影响答案。
- `Set`/`Map` 迭代顺序由 `TotalOrd` 决定，不依赖哈希种子。
- `random()` 是 IO，不参与纯求值的确定性；`rng(seed)` 是纯的、确定的。

## 6.11 模块初始化

- 模块的顶层 `let` 按依赖顺序**急切求值**；纯值，无副作用。
- 相互递归的顶层函数先绑定名字再求值闭包。
- `symbol` 的创建顺序按源码顺序；内部 id 确定。
- 单位/类型/类声明不产生运行时值（单位值除外）。
- `main` 在所有模块初始化后执行。

## 6.12 相等、顺序与显示

### 6.12.1 相等

- `Int`/`Float` 具体值按数值相等；`1.0/2.0 == 0.5` 的结果是 `Bool`。
- `Float` 按 IEEE：`NaN != NaN`，`-0.0 == 0.0`。
- `Real` 的 `==` 返回 `Prop`；不自动判定。要具体布尔，用
  `decide(x == y)`；要近似相等，用 `approx_eq`。
- 张量相等：形状相同且逐元素相等；`Float` 逐元素 IEEE；`Real` 张量的
  相等返回 `Prop`。
- 记录/变体：结构相等；函数无 `Eq`。
- `Set`：集合相等。

### 6.12.2 顺序

- `Int` 全序；`String` 按 Unicode 码点序；记录/元组按字段字典序。
- `Float` 的 `compare` 返回 `Option<Ordering>`：NaN 为 `None`；`<`/`>` 对
  NaN 返回 `false`。
- `TotalOrd` 是独立的类：`total_compare` 把 NaN 排在最后，供 `Set`/`sort`
  使用；`Float` 本身不是 `TotalOrd`，需要 `TotalFloat` 包装。
- `Real` 没有 `Ord`（符号表达式不可判定）；`solve` 返回 `Solutions`，不
  返回 `Set`。
- 张量没有 `Ord`；需要时用 `lexicographic` 或 `norm`。

### 6.12.3 显示

- `show(x)`：人类可读，带显示单位。
- `repr(x)`：可解析回同一值的文本；符号带内部 id。
- `to_latex(x)`、`to_typst(x)`：数学排版。
- 浮点用最短往返表示；`Real` 精确值用分数/根式；`Complex` 用 `a + b*i`。

## 6.13 一个求值例子

```aevia
let x = symbol("x")
let f = fn t => t^3 - 3*t + 1
let df = diff(f)
let a = df(2)          # 9，Exact
let b = approx(a)      # Ok(9.0)，Float
```

求值步骤：

1. `symbol("x")` 创建符号 `x`，弱量纲默认 1。
2. `f` 是闭包。
3. `diff(f)` 创建 `ξ`，应用 `f` 得 `ξ^3 - 3ξ + 1`，构造 `diff(...)`，
   rssn 化简为 `3ξ^2 - 3`，返回闭包。
4. `df(2)` 替换 `ξ=2`，精确计算 `3*4 - 3 = 9`。
5. `approx(9)` 返回 `Ok(9.0)`。

如果 `f` 写成 `fn t => if t > 0 then t^2 else -t^2`，`t > 0` 是 `Prop`，
`if` 要求 `Bool`，第 3 步会报类型错误 `E0302`。符号分段函数需要显式
`assuming` 分支，或对具体点用数值导数；`by_cases` 会构造 `ite` 项；`if` 不自动构造。

下一章定义模块、包和可见性；然后进入标准库与运行时。
