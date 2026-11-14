# 06 — 求值语义

本章定义程序运行时的行为：值是什么、求值顺序、符号与数值如何共存、张量表示、IO 如何执行、
错误如何报告。

## 6.1 求值策略

Aevia **严格求值、按值调用、从左到右**：

- 函数参数在调用前求值；二元运算先左后右；`let` 右侧立即求值后绑定。
- 记录字段按书写顺序求值；张量元素按行优先顺序求值。
- `if` 与 `match` 只求值被选中的分支；`&&`/`||` 在条件可判定时短路。

纯函数没有副作用，先算哪个只影响资源消耗，不影响结果。

## 6.2 值

```
v ::= () | true | false
    | n : Int | r : Real | f : Float | c : Complex | s : String
    | tensor(shape, storage) | sparse(format, shape, storage) | table(schema, columns)
    | device(cpu|gpu|distributed) | quantity(value, unit)
    | value(dynamic)                 # RSSN 动态边界
    | plugin(handle)
    | { l1 = v1, ..., ln = vn } | C(v1, ..., vn)
    | closure(env, params, body) | io(action) | set(elements)
    | Ok(v) | Err(v) | None | Some(v)
```

### 6.2.1 标量表示

```
Real    ::= Exact(Number) | Sym(TermId)
Float   ::= F64(f64)                     # 严格是 f64，永不符号化
Complex ::= C(Real, Real) | C64(Complex64) | Sym(TermId)
Int     ::= BigInt
```

- `Number` 是 rssn 的精确数（任意精度整数或有理数）；`TermId` 是符号图中的项句柄，项可含自由符号。
- `Float` **只有** `F64` 形式；含符号的浮点表达式由 `Real` 承担，因此 `Float` 的 IEEE 语义、`Ord`、
  JIT 都是干净的。
- `Complex` 的 `C(re, im)` 是精确/符号复数；`C64` 来自 `eig`/`fft` 等数值内核；`Sym` 是符号复数项。
  类型 `Complex[d]` 不区分精确与近似，需要时用 `re`/`im` 检查。
- 显示单位是标量的元数据 `Option<UnitRef>`，不参与相等。

### 6.2.2 闭包与 IO 动作

闭包捕获词法环境；递归函数通过环境中的自引用绑定实现，语义上是值，不是特殊形式。
`IO a` 是动作描述：接受世界令牌、返回 `a` 的 thunk。世界令牌不暴露，由运行时在执行 `main` 时提供；
动作可以组合、传递、存储，只有运行时能执行。

## 6.3 精确、浮点与符号

### 6.3.1 三种相位

| 相位 | 表示 | 例 |
|---|---|---|
| 精确 | `Exact(Number)`，只含有理数 | `1/3`、`2` |
| 符号 | `Sym(TermId)`，含自由符号或未化简算子 | `x^2 + 1`、`sin(1)`、`sqrt(2)` |
| 浮点 | `F64` | `0.8414709848078965` |

`Real` 在精确与符号之间移动；`Float` 只有 f64。`approx` 把无自由符号的 `Real` 变成 f64。

### 6.3.2 算术分派

```
Exact(a) op Exact(b)：  op 在有理数上封闭且无定义域错误 -> Exact(a op b)；否则 Sym(build(op, a, b))
Sym(_) op _ 或 _ op Sym(_)： Sym(build(op, ...))，能立即折叠则折叠
F64 op F64，或 F64 与 Int： Int 提升为 F64，按 IEEE 计算
F64 与 Exact/Sym：       Real —— 先把 F64 精确转换成有理数，再走 Real 的规则
```

- `Real` 与 `Float` 混合得 `Real`（精确者胜，警告 `W0302`）：`1/3 + 0.5` 是 `5/6`，符号信息保留。
- `Int` 与 `Float` 得 `Float`；无标注的 `1.0 + 2` 是 `Float`（§3.4.1 的 `join`）。要精确读取，给
  `Real` 期望类型（§3.10.3）。
- `Int / Int` 与整数的负次幂得精确有理数，即使结果恰为整数；有理数的“分母为 1”不等同于 `Int`。
- `0 / 0`、`1 / 0` 在精确相位是 `E0801`（字面量在编译期报告）；在浮点相位按 IEEE 得 NaN/Inf。

### 6.3.3 符号项的构造

`build(op, args)` 调用 rssn 的算子构造（`Session::call_value`），**不**运行全局重写引擎，所以
`x + 1` 是 O(1) 的项构造。只有下列边界触发 `compute`：

- 显式 `simplify`/`eval`；
- `diff`、`integrate`、`solve`、`limit`、`sum`、`product`、`series`、`dsolve` 等重型请求；
- `approx`。

### 6.3.4 定义域与类型不变量

- `Real` 的符号函数是**形式**的：`sqrt(-1)`、`ln(-1)`、`asin(2)` 保持符号项，不自动变成复数；
  `approx` 对定义域外的结果报 `E0806`。需要复数时用 `complex_sqrt` 或 `Complex`。
- `ln(0) = -oo`，`ln(oo) = oo`，`exp(oo) = oo`，`exp(-oo) = 0`。
- 桥接层 MUST 关闭 rssn 中把实域表达式化简为复数的规则，或在返回前检查值域；破坏 `Real` 的
  值域是内部错误 `E0809`。
- **精确相位的扩展实数运算是部分的。** 除上述边界值外，含 `oo` 的 `+ - * / ^` 一律保留为符号项
  （`oo - oo`、`oo*0`、`oo/oo`、`oo ^ 0` 等未定义形式同样保留，`-oo` 的符号不丢），既不静默折叠
  成有限数，也不报错。需要有限答案时在浮点相位计算（`inf`/`nan`，`nan != nan`，`-0.0 == 0.0`）。
  指数为无穷的形式（`0 ^ oo`、`oo ^ oo`）要求有限有理指数，报 `E0402`。

### 6.3.5 `eval`、`simplify`、`approx`

```
eval      : Real[d] -> Real[d]          # 符号化简，可能仍含符号
simplify  : Real[d] -> Real[d]          # eval 的别名
approx    : Real[d] -> Result<Float[d], EvalError>
approx    : Float[d] -> Result<Float[d], EvalError>
```

- `eval` 用 `Target::Symbolic` 运行 rssn，返回最简闭式或原式；不保证终止，受预算限制（§6.9.1）。
- `approx` 用 `Target::Numeric`，要求无自由符号，否则 `E0803`；容差默认 `1e-12`，可写
  `approx(x, tol)`。

### 6.3.6 符号身份与替换

```aevia
let x = symbol("x")       # 新鲜符号，显示名 x
let y = symbol("x")       # 另一个符号，显示名也是 x
```

- 每次 `symbol("x")` 创建**新鲜**符号：内部名唯一，显示名相同；`repr` 带内部 id。
- `subst(x, v, e)` 按符号身份替换（`x` 必须是符号值）；`subst_all([(x, 1), (y, 2)], e)` 批量替换。
- 符号相等 `x == y` 是 `Prop` 而非 `Bool`，`if` 不能直接用；用 `decide(x == y) : Option<Bool>`
  或 `assuming`。

## 6.4 微积分与方程的实现语义

### 6.4.1 `diff`

```
diff     : (Real[d] -> Real[e]) -> (Real[d] -> Real[e/d])
diff_at  : ((Real[d] -> Real[e]), Real[d]) -> Real[e/d]      # = diff(f)(x)
numeric_deriv : (Float[d] -> Float[e]) -> (Float[d] -> Float[e/d]) # 数值/AD，非符号
```

`diff(f)` 的求值：创建新鲜符号 `ξ : Real[d]`；把 `f` 应用到 `ξ` 得符号项 `t`；以
`Target::Symbolic` 计算 `diff(t, ξ)` 得 `t'`；返回闭包 `fn (x) => subst(ξ, x, t')`。

若 `f` 对符号值做了不可符号化的操作（如 `if ξ > 0`：`ξ > 0` 是 `Prop`，`if` 要求 `Bool`），报
`E0302`。分段函数的导数须显式分段（`by_cases`/`assuming`，§8.2）。

### 6.4.2 `antiderivative` 与 `defint`

```
antiderivative : (Real[d] -> Real[e]) -> (Real[d] -> Real[e*d])
defint         : (Real[d] -> Real[e]) -> Real[d] -> Real[d] -> Result<Real[e*d], MathError>
```

`antiderivative(f)` 同样创建 `ξ`、计算 `integral(t, ξ)`、返回闭包；积分常数约定为 0。
`defint(f,a,b)` 失败返回 `Err`。

### 6.4.3 `limit`、`series`、`sum`、`product`

- `limit(f, a)`；`series(f, a, n)` 返回带 `O(...)` 的表达式。
- `sum(f, a, b)` 用索引符号 `κ : Real[1]` 构造 `sum(f(κ), κ, a, b)`。
- `product(f, a, b)` 仅当 `f` 无量纲；否则用 `product_dyn` 返回 `Dyn<ρ>`。

### 6.4.4 `solve` 与 `nsolve`

```
solve    : (Real[d] -> Real[e]) -> Result<Solutions<Real,d>, MathError>
nsolve   : (Real[d] -> Real[e]) -> Real[d] -> Result<Real[d], MathError>
nsolve_f : (Float[d] -> Float[e]) -> Float[d] -> Result<Float[d], MathError>
```

`solve(f)` 把 `f(ξ) = 0` 交给 rssn，结果转成 `Solutions<Real,d>`（根 + 重数 + 参数条件）而非
`Set`——符号根没有全序。无解得空 `Solutions`，无法求解得 `Err`。`nsolve` 是数值求根，`nsolve_f` 走浮点内核。

### 6.4.5 `dsolve` 与 `SymFun`

```aevia
let y : SymFun<T, L> = symfun("y")
let eq = diff(y)(t) + y(t) / tau == 0 m/s   # eq : Prop，两边量纲 L/T
let sol = dsolve(eq, y, t)           # Result<(Real[T] -> Real[L]), MathError>，tau : Real[T]
```

- `SymFun<d,e>` 是未知函数符号；应用到 `Real[d]` 构造项（kernel 的未定函数结点 `apply(y, t)`，
  量纲取检查器为该调用点解出的值域量纲）。
- `dsolve(eq, y, t[, 条件列表])`：条件如 `[y(0) == 1]` 用于确定积分常数；未定常数记为 `C1`、`C2`。
- 答案为显式 `y(t) = F(t)` 时返回函数 `s ↦ F(s)`；隐式、参数式或无闭式解时返回 `Err`（携带 kernel
  的渲染），不猜测。数值 ODE 用 `odeint`（§8.9）。

## 6.5 模式匹配

- 按分支顺序尝试，第一个匹配者胜出。构造器按标签、记录按字段（开放）、元组按位置匹配。
- 张量模式 `[a, b, ..rest]` 只匹配秩 1、长度至少 2；`rest` 绑定剩余切片。
- 字面量模式：`Int`、`Bool`、`String`；浮点字面量不是合法模式。
- 守卫在模式绑定后求值，必须是具体 `Bool`；`Prop` 报 `E0308`。
- 非穷尽匹配在编译期报 `E0309`，运行时不会“掉出去”；永远不会被选中的分支警告 `W0201`。

状态：穷尽性/冗余分析是有界递归，**模式嵌套深度上限 24**；超过上限按“已覆盖”处理，只会抑制诊断，
不会误报。or-pattern 与空枚举当前语法不可表达；零构造器类型的零分支匹配视为穷尽。

## 6.6 张量运行时

### 6.6.1 表示

```
TensorData {
    shape: Vec<usize>,          # 行优先
    storage: F64(Vec<f64>) | C128(Vec<Complex64>) | Exact(Vec<Number>)
           | Boxed(Vec<Value>) | Bool(Vec<bool>),
    display: Option<UnitRef>,
}
```

- `Float` 用 `F64`，`Complex` 用 `C128`，`Int`/`Real` 用 `Exact` 或 `Boxed`，`Bool` 用 `Bool`；混合
  元素在构造时提升到最宽存储（`[1, 2.0]` 是 `[1.0, 2.0]`）。
- 秩是 `shape` 的长度，与静态类型 `Tensor<n, τ>` 的 `n` 一致（运行时断言）。
- 显示单位作用于整个张量。
- 构造：字面量递归检查矩形、统一元素类型后展平；`zeros`/`ones`/`fill` 分配连续存储；`range`/`linspace`
  生成 `F64` 或 `Exact`；CSV/JSON 读入先按 `Float` 解析，由调用者标注量纲。

### 6.6.2 广播

同秩广播：对每一维，`d1 == d2` 取 `d`，一方为 1 取另一方，否则 `E0802`。标量广播到所有元素；
不同秩是编译错误（§3.5.3），除非显式 `reshape`。

### 6.6.3 索引与切片

- 负索引 `-1` 是末元素；越界 `E0802`。
- `a[i]` 返回秩减一的子张量视图；`a[i..j]` 返回视图；视图共享缓冲区（不可变语义下安全）。
  `reshape` 在连续内存上返回视图，否则复制。
- 没有写入：所有“修改”返回新张量，唯一持有时原地完成。`set(a, idx, v)` 是函数式更新。
- `at(a, i)` 是秩多态索引 `Tensor<n,τ> -> Tensor<n-1,τ>`（`n>=1`）。
- `a @ b` 按秩分派：2×2 `matmul`、2×1 `matvec`、1×2 `vecmat`、1×1 `dot`（§3.5.3.1）。

#### 6.6.3.1 步长

`a[i:j:s]` 的 `s` 是**整数**表达式（不是 `Float`，也不带量纲）；非整数步长是 `E0302`。

- 省略 `s` 为 1；`a[i:j]` 与 `a[i..j]` 同一节点。
- 省略端点按步长方向解释：`s > 0` 时 `i` 默认 0、`j` 默认维长；`s < 0` 时 `i` 默认最后一个下标、`j`
  默认第一个下标**之前**。故 `xs[::-1]` 是 `reverse(xs)`，`xs[::-2]` 是 `xs[4], xs[2], xs[0]`。
- 显式端点的负数从末尾计并截断到合法范围；终点半开：`xs[4:0:-1]` 是 `xs[4]…xs[1]`。

长度是精确值：`s > 0` 时 `ceil((b - a) / s)`，`s < 0` 时 `ceil((a - b) / |s|)`。第一步就越过 `b` 的
切片是**空切片**（`xs[5:2:1]`、`xs[0:3:-1]`），不 panic。`s == 0` 永不推进：字面量已知时静态
`E0802`，否则运行时 `E0802`。

负步长产生**视图**：同一缓冲区、不同 offset、该轴反向（`View { strides, reversed }`）；正步长把该轴
stride 乘以 `s`。所以 `xs[::-1]` 与 `reverse(xs)` 共享缓冲区，`m[:, ::-1]` 只翻转第二轴。

### 6.6.4 线性代数

- `F64`/`C128` 的 `matmul`、`solve`、`inv`、`eig`、`svd`、`qr`、`lu` 走 rssn 数值内核；
  `Exact`/`Boxed` 小矩阵走符号线性代数规则，无法数值化的大矩阵返回 `Err` 或保持符号形式。
- `det` 只接受无量纲；带量纲用 `det_unit` 返回 `Dyn<ρ>`（`E0410`）。
- 奇异矩阵的 `solve`/`inv` 返回 `Err(SingularMatrix)`；`eig` 总返回 `Complex`。

### 6.6.5 归约

- 空张量：`sum` = 0，`prod` = 1；`mean` 对 `Float` 得 `NaN`、对 `Real` 报 `E0802`；`min`/`max` 报 `E0802`。
- `sum(a, axis)` 保留秩（该轴大小 1）；`sum(a)` 返回标量。`F64` 用 pairwise/Kahan 求和，`Exact` 精确求和。
- `var` 默认样本方差（除以 `n-1`），`var(xs, population=true)` 除以 `n`；`std = sqrt(var)`；
  `norm` 默认 2-范数，`norm(xs, p)` 支持 1、2、∞。

## 6.7 记录、变体与集合

- 记录按字段名排序存储；更新产生新记录，未变字段共享。变体是 `(tag, payload)`。
- `Set<a>` 与 `Map<a,b>` 是持久有序容器，要求 `TotalOrd a`：按 `total_compare` 排序，迭代确定；
  `Set` 的 `==` 是集合相等。`Float` 不是 `TotalOrd`，`Set<Float>` 不合法，需要时用 `TotalFloat`。
- `Solutions<ρ,d>` = `{ roots: Tensor<1, Scalar<ρ,d>>, multiplicities: Tensor<1, Int>,
  conditions: Tensor<1, Condition> }`，不要求 `Ord`。

## 6.8 IO

### 6.8.1 执行模型

运行时执行 `main : IO Unit`：初始化标准库和模块（纯求值）→ 构造 `main` 的动作 → 执行动作
（`print` 写标准输出，`read_file` 读文件，`plot` 写 HTML）→ 动作返回 `()`；未捕获的 `panic` 终止并打印调用栈。

`do` 构造时保存程序及其词法环境，不执行块内的纯表达式。执行时按源码顺序求值每条语句：一个 IO 动作
完成后才求值下一条，包括后面的 `let`、动作参数及字符串插值；插入未使用的 `let` 不改变效果顺序。
同一个动作可以重放，每次运行有独立的绑定及执行位置。块的结果是最后一条动作的结果；最后一条是
`let` 或 `<-` 绑定时返回 `Unit`。

`?` 只能用于 `Result<T,E>` 或 `Option<T>`。纯函数中的失败返回该函数；`do` 内的失败返回当前 IO 程序的
结果；嵌套函数有自己的返回边界。目标必须是同一容器家族，`Result` 的错误类型必须相同，没有隐式错误
转换（未标注返回类型的函数同样受约束）。所以 `IO (Result<U,E>)` 的 `do` 可传播 `Result<T,E>`，而
`IO Unit` 不能。提前返回跳过块内剩余语句，外层 IO 仍可绑定并处理这个失败值。

### 6.8.2 常用动作

```
print    : Show a => a -> IO Unit
println  : Show a => a -> IO Unit
eprintln : Show a => a -> IO Unit
read_line: () -> IO String
read_file: String -> IO (Result<String, IOError>)
write_file: String * String -> IO (Result<(), IOError>)
read_csv_dyn : String -> IO (Result<Table<Dynamic>, DataError>)
plot     : PlotSpec -> IO Unit
time_now : () -> IO Float[T]
random   : () -> IO Float[1]
```

`print` 用 `show`，`println` 追加换行。`read_file` 返回 `Result`，不抛异常。`plot` 把图写到当前目录的
`plot.html`（或 `--out`）。`random` 使用运行时 RNG；可复现用纯 API `rng(seed)`。

### 6.8.3 纯度

`IO` 动作不能出现在纯函数中（`E0313`）；纯函数可以接收/返回动作作为值，但不能执行。没有
`unsafePerformIO`。`main` 是唯一自动执行的入口。

## 6.9 预算、错误与资源

### 6.9.1 符号预算

rssn 的 `compute` 接受 `Budget`。实际默认值（`rssn::graph::schedule::Budget::default`）：
`max_iterations = 24`、`max_nodes = 20_000`、`match_limit = 1_000`、`reduce_rounds = 32`、
`patience = 3`、`time_limit = None`。预算耗尽报 `E0808`；重型请求（`integrate`/`solve`）返回 `Err`。

状态：尚无语言级的 `with_budget(seconds, rewrites, thunk)`。CLI 的 `--budget` 是**尾调用预算**
（§9.3.4），不是符号预算；符号预算取内核默认值，需调整时走内核 `Config`/`Budget` API（嵌入层）。

### 6.9.2 错误分类

| 代码 | 情况 | 可恢复 |
|---|---|---|
| `E0801` | 精确除法/取模除以零 | 否（浮点按 IEEE） |
| `E0802` | 形状/索引错误 | 否 |
| `E0803` | `approx` 遇到自由符号 | 是（`Result`） |
| `E0804` | 符号条件无法判定，或 `assuming` 的假设为假 | 否 |
| `E0805` | 栈溢出（非尾递归过深） | 否 |
| `E0806` | 定义域错误 | 是（`Result`） |
| `E0807` | 数值算法不收敛 | 是（`Result`） |
| `E0808` | 预算耗尽 | 是（`Result`/`panic`） |
| `E0809` | 类型不变量被后端破坏 | 否（内部错误） |
| `E0810` | IO 错误 | 是（`Result`） |
| `E0811` | 任务失败（§9.7.3） | 是 |
| `E0899` | `panic` | 否 |

### 6.9.3 栈与尾调用

- 自递归尾调用用 trampoline，常量栈。
- 非尾递归受两个界限：库默认的调用深度上限 2048（`Interp::max_depth`）；`aevia` CLI 在
  `--max-stack <MiB>`（默认 1024）指定的线程栈上运行解释器，把深度上限放宽为 10⁷ 并改由**剩余栈空间**
  限制（留出栈的 1/8，8–64 MiB）。超过任一界限报 `E0805` 并带调用栈。
- 一个**不终止**的尾循环由“连续尾调用预算”限制：默认 100,000,000（`Interp::max_tail_calls`，CLI
  `--budget` 修改），超过报 `E0805`；这是 runaway 保护，不是语义上限（§9.3.4）。
- 表达式嵌套深度另有上限（默认 16,384 个节点），超过同样是 `E0805`。`panic` 打印调用栈。

## 6.10 确定性与并行

- 纯求值完全确定（浮点 NaN 规范化）。
- `par_map(f, xs)` 结果顺序与输入一致，线程数不影响结果；要求 `Parallel a`（仅 `Int`/`Float` 及其张量），
  符号 `Real` 不并行。值不能跨线程时回退为顺序求值，结果不变（§9.7.2）。`compute_all` 的并行只影响速度。
- `Set`/`Map` 的迭代顺序由 `TotalOrd` 决定，不依赖哈希种子。`random()` 是 IO，`rng(seed)` 纯且确定。

## 6.11 模块初始化

- 顶层 `let` 按依赖顺序**急切求值**；相互递归的顶层函数先绑定名字再求值闭包。
- `symbol` 的创建顺序按源码顺序，内部 id 确定。单位/类型/类声明不产生运行时值（单位值除外）。
- `main` 在所有模块初始化后执行。

## 6.12 相等、顺序与显示

### 6.12.1 相等

- `Int`/`Float` 具体值按数值相等；`Float` 遵循 IEEE（`NaN != NaN`，`-0.0 == 0.0`）。
- `Real` 的 `==` 返回 `Prop`；要具体布尔用 `decide(x == y)`，要近似相等用 `approx_eq`。
- 张量：形状相同且逐元素相等（`Real` 张量得 `Prop`）；记录/变体结构相等；函数无 `Eq`；`Set` 集合相等。

### 6.12.2 顺序

- `Int` 全序；`String` 按 Unicode 码点序；记录/元组按字段字典序。
- `Float` 的 `compare` 返回 `Option<Ordering>`（NaN 为 `None`，`<`/`>` 对 NaN 为 `false`）。
- `TotalOrd` 的 `total_compare` 把 NaN 排在最后，供 `Set`/`sort` 使用；`Float` 本身不是 `TotalOrd`。
- `Real` 与张量没有 `Ord`；张量需要时用 `lexicographic` 或 `norm`。

### 6.12.3 显示

- `show(x)` 人类可读并带显示单位；`repr(x)` 可解析回同一值，符号带内部 id；`to_latex`、`to_typst`
  做数学排版（单字母符号斜体，希腊字母名转字母，负幂积写 `\frac`，初等函数用直立算子名，`exp(u)` 写 `e^{u}`）。
- 浮点用最短往返表示并**总是保留相位**：整数值的 `Float` 带小数点（`10.0`、`-0.0`），大/小量级用指数
  （`1e20`、`1e-9`）；精确 `10` 与浮点 `10.0` 看起来不同。`Real` 精确值用分数/根式；`Complex` 用
  `a + b*i`，两个分量按 `Float` 规则打印。
- IEEE 无穷打印为 `inf`/`-inf`，NaN 为 `nan`；`oo`/`-oo` 只表示 `Real` 相位的精确无穷。`repr` 把浮点
  无穷写成 `(1.0 / 0.0)`。
- 字符串单独 `show` 是其文本；位于容器内时带引号：`show(Err("not found"))` 是 `Err("not found")`，
  `show(["a, b"])` 是 `["a, b"]`。
- 带显示单位的 `Float` 换算时只舍入一次（先在精确有理数上做 `(x - offset) / scale`，再取最近的 `f64`）：
  `to(°C, point(300.0, K))` 显示为 `26.85 °C`；值本身不变。
- 和式按次数降序打印（`x^3 - 3*x + 1`），`b^(1/2)` 打印为 `sqrt(b)`；内核返回的项使用 Aevia 的拼写
  （`e` 而不是 `E`）。

## 6.13 一个求值例子

```aevia
let f = fn t => t^3 - 3*t + 1
let df = diff(f)
let a = df(2)          # 9，精确
let b = approx(a)      # Ok(9.0)
```

`diff(f)` 创建 `ξ`，应用 `f` 得 `ξ^3 - 3ξ + 1`，rssn 化简导数为 `3ξ^2 - 3`，返回闭包；`df(2)` 代入
`ξ=2` 精确得 `9`；`approx(9)` 得 `Ok(9.0)`。若 `f` 写成 `fn t => if t > 0 then t^2 else -t^2`，
`t > 0` 是 `Prop`，报 `E0302`；符号分段函数用 `by_cases` 构造 `ite` 项，或对具体点用数值导数。

下一章定义模块、包和可见性。
