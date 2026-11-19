# Aevia 语言规范

**单文件版 · 修订 1**

> 本文件由 `spec/` 下的章节按顺序合并生成（`tools/bundle_spec.py`）。

---

## 目录

1. [Aevia 语言规范](#ch-index)
2. [00 — 总览](#ch-00-overview)
3. [01 — 词法结构](#ch-01-lexical)
4. [02 — 语法](#ch-02-syntax)
5. [03 — 类型系统](#ch-03-types)
6. [04 — 量纲](#ch-04-dimensions)
7. [05 — 单位](#ch-05-units)
8. [06 — 求值语义](#ch-06-semantics)
9. [07 — 模块、包与可见性](#ch-07-modules)
10. [08 — 标准库](#ch-08-stdlib)
11. [09 — 运行时](#ch-09-runtime)
12. [10 — rssn 后端映射](#ch-10-backend)
13. [11 — 完整示例](#ch-11-examples)
14. [12 — 设计取舍与限制](#ch-12-rationale)
15. [13 — 可扩展性与 RSSN 集成](#ch-13-extensibility)
16. [附录 A — 完整文法](#ch-appendix-a-grammar)
17. [附录 B — 诊断目录](#ch-appendix-b-diagnostics)

---

<a id="ch-index"></a>

# Aevia 语言规范

**Aevia — 设计标准**

Aevia 是一门面向科学计算的纯函数式语言，**量纲是类型系统的一部分**。一个量不是带运行时标签的数字，而是类型里记着物理量纲的值。编译器在任何代码运行之前就拒绝
`3 m + 2 s`、`sin(3 kg)` 和 `exp(1 m)`。

语言建立在 `rssn` 计算机代数与科学计算引擎之上：符号与数值求值是同一个表达式的两个相位；单位、量纲推断、重写、数值内核、JIT 和物理仿真由后端提供。Aevia 不维护第二份算子表或单位表：
它读取 rssn 的 **Kernel Manifest** 与 `rssn::dim` 注册表，并由它们生成 `Rssn` 模块。见 [13 — 可扩展性](#ch-13-extensibility) 与 rssn 仓库的 `docs/KERNEL.md`。

> 本文档是**规范性设计**：文法完整，类型规则明确，运行时架构固定，每个后端依赖都已标出。示例与取舍说明是资料性的。

---

## 阅读指南

| 章节 | 内容 | 规范性 |
|---|---|---|
| [00 — 总览](#ch-00-overview) | 目标、非目标、原则、十分钟导览 | 是（目标） |
| [01 — 词法结构](#ch-01-lexical) | 源码编码、字面量、单位、布局 | 是 |
| [02 — 语法](#ch-02-syntax) | 声明、表达式、模式、模块 | 是 |
| [03 — 类型系统](#ch-03-types) | 种类、类型、推断、类型类、效果 | 是 |
| [04 — 量纲](#ch-04-dimensions) | 量纲代数、类型规则、多态、`Dyn` | 是 |
| [05 — 单位](#ch-05-units) | 单位声明、前缀、仿射点、常数 | 是 |
| [06 — 求值语义](#ch-06-semantics) | 求值、值、符号/数值相位、张量 | 是 |
| [07 — 模块、包与可见性](#ch-07-modules) | 命名空间、可见性、包、测试 | 是 |
| [08 — 标准库](#ch-08-stdlib) | Prelude 与库签名 | 基本是 |
| [09 — 运行时](#ch-09-runtime) | 流水线、解释器、REPL、CLI、LSP、性能 | 是（架构） |
| [10 — rssn 后端映射](#ch-10-backend) | 与 rssn 的映射、桥接设计、缺口与风险 | 是 |
| [11 — 完整示例](#ch-11-examples) | 从运动学到仿真的完整程序 | 否 |
| [12 — 设计取舍与限制](#ch-12-rationale) | 替代方案、取舍、设计边界 | 否 |
| [13 — 可扩展性与 RSSN 集成](#ch-13-extensibility) | rssn 反射、生成的 `Rssn`、插件、registry、形状/设备/稀疏决定 | 是 |
| [附录 A — 完整文法](#ch-appendix-a-grammar) | 完整 EBNF | 是 |
| [附录 B — 诊断目录](#ch-appendix-b-diagnostics) | 错误码与消息格式 | 是 |

后端接口契约在 rssn 仓库的 `docs/KERNEL.md`（Kernel Manifest、动态调用 ABI、插件、单位与量纲）和 `docs/DIMENSIONS.md` 中；这是 Aevia 规范成立的前提，不是可选的实现细节。
实现的 crate 划分见仓库根目录的 [ARCHITECTURE.md](https://github.com/Apich-Organization/aevia/blob/main/ARCHITECTURE.md)。每章另有英文版（`spec/en/`），写作与术语规则见 `spec/STYLE.md`。

## 规范用语

**必须**（MUST）、**不得**（MUST NOT）、**应该**（SHOULD）、**可以**（MAY）按 RFC 2119 理解。符合规范的实现必须接受本规范测试语料中的每个程序，必须拒绝每个标记为类型错误的程序，
并产生规定类别的诊断（具体措辞不是规范性的）。

## 一致性与查询

各编号章节定义语言契约。当后端无法提供所请求的能力时，报告 `E1001`。运行 `aevia rssn` 可查询生效的内核清单及其算子、单位和常数数量。第 13 章规定编译器如何选择实例，以及如何固定每次调用的内核重载。

## 一句话概括

> Aevia 让量的量纲成为类型，让单位成为编译期尺度，让公式成为一等值，让数值结果成为一次明确的请求。

## 快速一瞥

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

```
$ aevia run kinematics.aev
fall time from 20.0 m = 2.0196199771025523 s
```

下一章解释这个程序为什么能通过类型检查，以及不能通过时语言会做什么。


<a id="ch-00-overview"></a>

# 00 — 总览：Aevia 是什么，为什么这样设计

## 0.1 定义

**Aevia 是一门纯函数式的科学计算语言：量纲是类型的一部分，单位是编译期已知的尺度，
公式是一等值，数值结果是显式请求的另一种相位。**

它瞄准一类具体的程序：从一条物理定律、一组实验数据或一个微分方程出发，经过符号推导、
量纲检查、数值离散和可视化，得到可复现的结果。今天这类程序多由 Python + NumPy +
SymPy + Pint 拼成：量纲检查靠库，符号与数值是两套 API，单位错误常在运行时才暴露。
Aevia 把它们收进一门语言，让编译器在最便宜的时刻——类型检查时——拒绝量纲错误。

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

没有类型转换、单位字符串或 `Quantity` 包装。`h : Float[L]`，`g : Real[L/T^2]`，
`2 * h / g` 的量纲由量纲代数算出为 `T^2`，`sqrt` 把指数除以二，得 `Float[T]`。
写成 `h + g` 则程序不会运行。

## 0.2 目标

按优先级排列：

1. **量纲正确性由构造保证。** 加、减、比较要求量纲相等；乘、除、幂按量纲代数组合；
   超越函数要求无量纲参数。量纲错误是类型错误，不是运行时警告。
2. **符号与数值是同一门语言的两个相位。** 同一表达式既可化简为闭式，也可在给定点求值。
3. **写出来像数学。** 单位字面量 `9.81 m/s^2`、Unicode 标识符 `Δt`、中缀算子、管道
   `|>`、模式匹配。
4. **纯核心，显式效果。** 没有可变状态和隐式 IO；副作用隔离在 `IO` 中，纯函数可放心
   并行、缓存、求导和代入。
5. **性能可预测。** 严格求值；张量的秩在类型里、形状在运行时；数值热点下沉到 rssn
   的内核与 JIT；符号搜索的预算显式可控。
6. **小核心，厚标准库。** 语言只有函数、代数数据类型、记录、类型类、模块和 `IO`；
   数学、单位、线性代数、统计、ODE/PDE、绘图都在标准库。
7. **错误是教学时刻。** 量纲错误要指出两边的量纲并给出可能的修正（附录 B）。

## 0.3 非目标

- **不是系统编程语言。** 没有手动内存管理、`unsafe` 和指针算术；底层性能交给 rssn 内核。
- **不是证明助手。** 量纲类型是阿贝尔群上的等式，不是完整依赖类型。数组形状以
  refinement 进入类型，类型级 `Nat` 只做线性算术（§13.9.1）。
- **不追求 Python 级别的库生态。** 通过 CSV/NPY/JSON、C ABI、插件和 registry 与外部
  交换数据，不内嵌 Python。
- **不提供用户自定义语法、宏或 effect handlers。** 扩展点是 RSSN 算子、插件、`Rssn`
  动态调用和包 registry，使工具链、格式化与错误信息保持统一（§13.9.6、§13.9.7）。
- **不自动并行。** 并行必须由 `par_map` 等原语显式请求；设备/GPU/分布式是运行时目标，
  不是类型参数（§13.9.4）。

## 0.4 设计原则

### 一、量纲是类型，单位不是

- **量纲**属于类型层：`L`、`T`、`M*L^2/T^2`。它决定一个值能参与哪些运算。
- **单位**是值层的编译期常量：`m`、`km`、`degC`。它只提供尺度（及仿射偏移），在编译期
  折叠。

所以 `3 km` 与 `3000 m` 是**同一类型** `Real[L]`，运行时是同一个值；`to(km, x)` 只改变
显示单位。这样类型相等就是量纲相等，不会出现“米与厘米是不同类型”的组合爆炸，单位表也
可以独立扩展而不污染类型检查器。代价是类型不区分“以米表示的长度”与“以英尺表示的长度”
——这是刻意的：科学计算的错误几乎总是量纲错误，而不是同一量纲下的单位选择错误。

### 二、精确与近似是两种类型，不是两种模式

| 类型 | 含义 |
|---|---|
| `Int` | 任意精度整数，无量纲 |
| `Real[d]` | 精确实数或符号表达式，量纲 `d`；`1/3`、`sqrt(2)`、`x^2 + 1` |
| `Float[d]` | IEEE-754 binary64，量纲 `d`；`1.5` |
| `Complex[d]` | 复数标量 |

整数字面量是 `Int`，十进制字面量是 `Float`，`1/10` 是精确的 `Real`。`Real` 与 `Float`
混合运算时 **`Real` 吸收 `Float`**（精确优先）并给出警告 `W0302`：`1/3 + 0.5` 是
`5/6`。语言从不在你没写 `Float` 的地方做隐式 `Real → Float` 转换，因此 `sin(1)` 保持
符号形式 `sin(1)`，而 `sin(1.0)` 得到 `0.8414709848078965`：作者必须明确自己在做精确
推导还是数值计算（动机见 §12.8.9）。

### 三、纯函数，但允许看起来像命令式

没有可变变量、`for` 循环和原地修改。迭代用递归，编译器保证尾调用；数组更新在语义上是
复制，运行时在唯一所有权时原地完成。`do` 块只用于 `IO`，不是通用 monad 语法。

### 四、符号运算不是宏，是值

不需要 `quote`/`unquote`。`symbol("x")` 返回一个 `Real` 值，对它能做的一切——加、乘、
求导、代入——与普通 `Real` 相同。`diff(fn x => x^2)(3 m)` 的类型是 `Real[L]`：导数把
量纲除以自变量的量纲。“先推导公式，再代入数据”因此是默认工作流。

### 五、为易用性让步，不为可靠性让步

采用 HM 推断 + 类型类 + 量纲代数，而非完整依赖类型：用户不必为 `mean(xs)` 写签名，
却能在运行前知道它的量纲；量纲等式是阿贝尔群上的线性方程，推断可判定且线性。静态类型
无法表达时（如 `x^n` 的 `n` 是运行时整数），语言提供显式的动态逃逸 `Dyn`，把检查推迟到
运行时——静态路径保持可靠，动态路径保持可用。

### 六、单一注册表，反射优先

Aevia 不手写 RSSN 算子绑定，也不维护第二张单位表：`OpTable` 是唯一算子注册表，
`rssn::dim` 是唯一量纲/单位层，Aevia 读取 Kernel Manifest 并生成 `Rssn` 模块（类型化 +
`dynamic`）。新算子、新单位、新插件无需改动编译器。

## 0.5 十分钟导览

以下示例均可运行（输出已核对）。

### 量纲与单位

```aevia
let d = 100 m                 # Real[L]
let t = 9.58 s                # Real[T]
let v = d / t                 # Real[L/T]
let E = 70 kg * v^2 / 2       # Real[M*L^2/T^2]

let bad = d + t               # error[E0401]: dimension mismatch
```

`Energy` 之类名字只是 `M*L^2/T^2` 的打印别名，不是新类型。

### 量纲多态

```aevia
fn mean<d>(xs: Tensor<1, Real[d]>): Real[d] =
    sum(xs) / length(xs)

fn variance<d>(xs: Tensor<1, Real[d]>): Real[d^2] =
    let m = mean(xs)
    sum((xs - m)^2) / length(xs)
```

`mean` 对任何量纲成立，`variance` 的量纲是输入的平方。量纲在运行时被擦除，不为每个量纲
生成一份代码。

### 符号与数值

```aevia
let x = symbol("x")                   # Real，默认无量纲
let df = diff(fn t => t^3 - 3*t + 1)  # 导函数
let slope = df(x)                     # 3*x^2 - 3
let y = approx(df(2))                 # Ok(9.0)
```

`diff` 接受函数、返回导函数：内部创建同量纲的符号，应用函数、求导、再代回，因此对高阶
函数、部分应用和用户函数都成立。

### 张量

```aevia
let M = [[1.0, 2.0], [3.0, 4.0]]
let b = [5.0, 6.0]
let x = match linsolve(M, b) with
        | Ok(v) -> v
        | Err(_) -> [0.0, 0.0]        # 示例；实际应处理错误
let y = M @ x                         # ≈ b

let R = [[1.0, 2.0], [3.0, 4.0]] * ohm
let I = [0.1, 0.2] * A
let V = R @ I                         # [0.5 V, 1.1 V]
```

`@` 是矩阵乘法，`*` 是逐元素乘法（标量自动广播），与 NumPy 一致。

### 变体与模式匹配

```aevia
type Shape =
    | Circle(Real[L])
    | Rect(Real[L], Real[L])

fn area(s: Shape): Real[L^2] =
    match s with
    | Circle(r) -> pi * r^2
    | Rect(w, h) -> w * h
```

### 纯核心与 IO

```aevia
fn main(): IO Unit = do
    r <- read_csv<{ time: Float[T], velocity: Float[L/T] }>("data.csv")
    match r with
    | Err(e) -> println("cannot read: {e}")
    | Ok(xs) ->
        match (column(xs, "time"), column(xs, "velocity")) with
        | (Ok(t), Ok(v)) ->
            println("mean speed = {mean(v)}")
            plot(combine([label(line(t, v), "v(t)")]))
        | _ -> println("column type mismatch")
```

`read_csv`、`plot`、`println` 是 `IO` 动作，`mean` 是纯函数；`main` 是运行时调用的唯一入口。

## 0.6 语言与后端的分层

```
┌──────────────────────────────────────────────────────────────┐
│ Aevia 源码 (.aev)                                            │
│  词法 / 布局 → 语法树 → 名字解析 → 类型+量纲检查 → 核心 IR    │
└───────────────────────────┬──────────────────────────────────┘
                            │ 纯值、闭包、张量、符号标量
┌───────────────────────────▼──────────────────────────────────┐
│ Aevia 运行时                                                 │
│  解释器 · 张量运行时 · 符号桥 · 预算/缓存 · REPL · LSP        │
└───────────────────────────┬──────────────────────────────────┘
                            │ Term / Config / compute / kernels
┌───────────────────────────▼──────────────────────────────────┐
│ rssn                                                         │
│  e-graph 重写引擎 · 量纲推断 · 数值内核 · JIT · 仿真场景      │
└──────────────────────────────────────────────────────────────┘
```

Aevia 负责语法、类型与量纲检查、闭包与记录、张量形状、模块、IO 与标准库的编排；rssn
负责符号表达式的表示与重写、量纲代数的运行时部分、数值算法、线性代数、FFT、ODE/PDE、
特殊函数、JIT 与物理仿真。两者的边界是 **Term**：需要符号能力的 `Real`/`Complex` 标量
持有一个 rssn 项；`Float` 标量与数值张量走原生快速路径，不进入项图。

语言层的量纲检查是**编译期**的，rssn 的量纲层是**运行时/项级**的；二者共用同一套基量纲
与指数代数，前者拒绝坏程序，后者在化简中保持正确并报告诊断。rssn 的具体能力与接口见
第 10、13 章。

## 0.7 文档约定

- 标 `aevia` 的代码块是 Aevia 源码；标 `text` 的是文法、推导或输出。
- 类型写作 `e : τ`，量纲写作 `d`、`e`，类型环境写作 `Γ`。`Real[d]` 中的 `d` 是量纲表达式，
  `<d>` 声明量纲参数。
- “必须 / 不得 / 应该 / 可以”对应 RFC 2119 的 MUST / MUST NOT / SHOULD / MAY（见
  [README](#ch-index)）。
- 示例中的 `...` 表示省略，不是合法语法。

下一章从最底层开始：一个 Aevia 源文件由哪些字符、记号与布局规则组成。


<a id="ch-01-lexical"></a>

# 01 — 词法结构

本章定义源文件如何被切分为记号。语法在第 02 章；单位字面量的语义在第 05 章，本章
只定义它如何被识别。

## 1.1 源文件与编码

- 扩展名 `.aev`，编码 UTF-8；允许并忽略 BOM。
- 换行 `\n`、`\r\n`、`\r` 在词法阶段统一为 `\n`。
- 标识符（含关键字、单位名）按 Unicode NFC 比较：预组合的 `é` 与 `e` 加组合重音是
  同一个名字。字符串和注释按原样保存。
- 文件首行可以是 shebang（`#!` 到行尾），被忽略。
- 制表符不得出现在行首缩进中（`E0101`）；字符串和注释中不受限制。

## 1.2 记号

```
token ::= IDENT | UIDENT | KEYWORD | INT | FLOAT | STRING
        | UNIT_NAME | SUPERSCRIPT | LT_APP | ELLIPSIS
        | OP | LPAREN | RPAREN | LBRACKET | RBRACKET | LBRACE | RBRACE
        | COMMA | SEMICOLON | COLON | DOT | DOTDOT | DOTDOTEQ
        | ARROW | FATARROW | LARROW | PIPE | EQUALS
        | NEWLINE | INDENT | DEDENT | EOF
```

空白分隔记号，换行同时参与布局（§1.4）。除单位字面量外没有“相邻记号自动相乘”：
`3 x` 不是乘法，`f x` 不是调用。调用必须写 `f(x)`，乘法必须写 `3 * x`。

## 1.3 注释

```aevia
# 行注释
let x = 1  # 行尾注释

#- 块注释
   #- 可以嵌套 -#
   仍在块内 -#
```

- `#` 起行注释；`#-` … `-#` 是可嵌套的块注释，未闭合报 `E0104`。
- `##` 起文档注释（Markdown），必须在行首（可前置缩进），附着到下一个声明。
- 注释不产生记号。

## 1.4 布局

Aevia 用 offside 规则：块只由缩进界定，没有 `end`。

### 1.4.1 INDENT / DEDENT

词法器维护缩进栈，初始 `[0]`。对每个**逻辑行**（不在括号内、非空行、非纯注释行）：

1. 计算行首空格数 `n`。
2. `n > 栈顶`：压栈，发 `INDENT`。
3. `n == 栈顶`：不发记号。
4. `n < 栈顶`：弹栈并每次发一个 `DEDENT`，直到栈顶 `== n`；弹空仍不等则 `E0102`。
5. 行尾发 `NEWLINE`。

空行和纯注释行不参与缩进，也不发 `NEWLINE`。

### 1.4.2 括号内自由布局

`(` `[` `{` 内部不产生 `NEWLINE`/`INDENT`/`DEDENT`，末尾可带逗号：

```aevia
let A = [[1.0, 2.0],
         [3.0, 4.0]]

let B = [
    1.0, 2.0,
    3.0, 4.0,
]
```

### 1.4.3 续行

括号外没有续行抑制：每行以 `NEWLINE` 结束，行尾的 `+` 之类不会把下一行拼上
（`1 +` 换行是 `E0202`）。多行表达式 MUST 用括号：

```aevia
let energy = (m * c^2
            + 1/2 * m * v^2
            - m * g * h)
```

行尾是块引导记号（`=`、`then`、`else`、`->`、`=>`、`do`、`with`）且下一行缩进增加时，
解析器把 `NEWLINE INDENT expr DEDENT` 作为**布局组**，用于函数体与 `let` 右侧、`if`
分支、`match` 分支体、`do` 语句体。这条规则让词法器不必知道哪些记号是运算符。

### 1.4.4 块

块也是 `NEWLINE INDENT … DEDENT`，内容是 `let`/`fn` 声明序列加一个最终表达式；
块的值是最终表达式的值，只有 `let` 的块是 `E0201`。解析器在 `NEWLINE INDENT` 后看到
`let`/`fn` 选块，否则选布局组：

```aevia
fn mean<d>(xs: Tensor<1, Real[d]>): Real[d] =
    let n = length(xs)
    sum(xs) / n
```

要求缩进块的位置没有缩进是 `E0103`。多行 `if` 的 `else` 必须与 `if` 同列。同一 `match` 的 `|` 分支必须彼此同列，可与 `match` 同列或再缩进一级（§2.3.4）。

## 1.5 标识符与关键字

### 1.5.1 标识符

```
ident_start    ::= XID_Start | "_"
ident_continue ::= XID_Continue | "'" | "₀".."₉"
```

- 允许 Unicode 字母（`Δt`、`λ`）、撇号（`f'`）、下标数字（`x₁`），区分大小写。
- 首字符是 ASCII 大写的是 `UIDENT`（类型、构造器、模块、量纲）；其余是 `IDENT`。
  `Δt` 因此是值名，不是构造器。
- 单独的 `_` 是通配符；作为调用实参时是占位符（§2.3.6）；不是绑定名。

### 1.5.2 关键字

```
as       class     derive    dimension  do       else     false
fn       for       if        import     in       instance let
match    module    open      private    rec      test     then
true     type      unit      where      with
```

关键字不能作普通标识符（`E0206`）。`forall` 是上下文关键字：只在类型开头、后跟
类型参数和 `.` 时是量词，同时是标准库函数名（§8.13）。`Int`、`Real`、`Float`、
`Bool` 等类型名只在类型位置特殊解析，不是关键字。`in` 属于 `let … in`，`for` 属于
`instance … for` 与 `derive … for`，`rec` 属于 `let rec`。`pure` 是 Prelude 函数，
不是关键字（§9.7.3）。

导入绑定使用非关键字名称。源码中的反引号报 `E0105`。

### 1.5.3 命名约定（非规范）

类型、构造器、模块、量纲用 `UpperCamelCase`；函数、值、字段用 `snake_case`；类型参数
小写 `a b`，量纲参数 `d e`；单位名保持国际写法（`m`、`kg`、`Pa`、`°C`）。

## 1.6 数值字面量

```
int_lit   ::= dec_int | "0x" hexdigit {["_"] hexdigit}
            | "0o" octdigit {["_"] octdigit} | "0b" bindigit {["_"] bindigit}
dec_int   ::= digit { ["_"] digit }
float_lit ::= dec_int "." dec_int [ exponent ] | dec_int exponent
exponent  ::= ("e" | "E") ["+" | "-"] dec_int
```

- 下划线只用于分组，不能在开头、结尾或连续出现（`E0106`）；`1_000` 合法，`1__0`、
  `1_` 不合法。
- 在值表达式中，十进制整数是 `Int`；带小数点或指数的是 `Float`。`0755` 是十进制 755；没有数字后缀；
  `1.` 与 `.5` 不合法。
- `oo`、`-oo`、`nan` 是预置常量，不是字面量；`i` 是预置虚数单位，复数写 `3 + 4 * i`。
- `0` 不是量纲多态的，默认 `Int`；写零米要写 `0 m`，浮点零写 `0.0`（§4.7）。

## 1.7 字符串字面量

```aevia
"hello"
"value = {x + 1}"
"line\nbreak"
r"C:\data\raw.csv"          # 原始字符串：无转义、无插值
"""
多行字符串
  保留相对缩进
"""
```

- 双引号分隔；单引号不是字符串分隔符（`E0105`）。
- 转义：`\n \t \r \\ \" \{ \} \u{1F600}`；未知转义是 `E0105`。
- 插值 `{expr}`：表达式类型必须实现 `Show`；空插值 `{}` 是 `E0104`；字面花括号写 `\{`。
- 原始字符串 `r"…"` 不处理反斜杠和花括号，不能含 `"`。
- 多行字符串 `"""…"""`：去掉开头换行和各行的公共缩进，内部换行保留。
- 字符串不可变，是 UTF-8 序列；索引按 grapheme cluster 计（§8.1.4）。

## 1.8 单位字面量

```aevia
9.80665 m/s^2
3.0e8 m/s
1.5 kg*m^2/s^3
25 °C
50 %
2.5 µm
```

### 1.8.1 识别规则

**数值字面量后紧邻的标识符序列一律按单位字面量解析，不按乘法。**

```aevia
3 m          # 量：3 米
3 * m        # 值 3 乘以单位值 m；显示单位可能不同
3 x          # 单位字面量；x 不是单位则 E0501
```

规则上下文无关：词法和语法阶段不需要知道哪些名字是单位，名字解析再检查 `x` 是否在
单位命名空间中，因此单位声明可以后置，解析器无需回溯。

```
quantity_lit ::= number unit_expr
unit_expr    ::= unit_factor { ("*" | "·" | "/") unit_factor }
unit_factor  ::= unit_atom [ "^" exponent | SUPERSCRIPT+ ]
unit_atom    ::= unit_name | "(" unit_expr ")" | "%"
unit_name    ::= UNIT_NAME | IDENT | qname
exponent     ::= ["-"] (number | "(" rational ")")
rational     ::= ["-"] number ["/" ["-"] number]
```

- `number` 是 §1.6 的 `INT` 或 `FLOAT`。数值字面量后紧邻 `IDENT`、`UNIT_NAME`（`°C`）、
  `%` 或 `(` 时进入单位字面量。
- `*` 与 `·` 同义；`/` 左结合：`kg*m^2/s^3` = `(kg*m^2)/s^3`。
- 指数为精确有理数字面量：`m^2`、`s^-1`、`m^(1/2)`、`m^1.5`（即 `m^(3/2)`）。
  小数/科学记数法按原始拼写精确读取，不经过 `Float` 舍入。变量指数不允许
  （用 `Dyn`，§4.9）。上标 `m²`、`s⁻¹`、`m³·s⁻²` 是 `^n` 的等价写法（`E0107`：非法序列）。
- 单位表达式内不允许空白并置：`kg m^2` 不是 `kg*m^2`。
- **单位表达式内的 `*` `·` `/` 必须紧贴两侧。** 任一侧有空白的 `*`/`/` 结束单位字面量，
  是普通算术：`9.81 m/s^2 * mass` = `(9.81 m/s^2) * mass`；`70 kg * v^2 / 2` =
  `((70 kg) * v^2) / 2`；`3 m * x` = `(3 m) * x`。紧贴的 `3 m*x` 仍是单位表达式，`x`
  不是单位时 `E0501`。格式化器总是紧贴输出单位、在算术运算符两侧加空格，所以格式化
  不改变含义。
- 单位表达式内不允许数字：`3 m/2 s` 是 `(3 m) / (2 s)`；`1/2 m` 是 `1 / (2 m)`，类型
  `Real[1/L]`，“半米”写 `(1/2) m`。
- 括号只用于单位：`3 (m/s)` 合法；`3 (2)` 是 `E0108`；`3 ()` 等括号内单位写坏是 `E0209`。

### 1.8.2 单位名与限定

- 单位名在**单位命名空间**解析，与值命名空间分离：`let s = 5` 不影响 `3 s`；乘变量写
  `3 * s`。
- 可带前缀（`km`、`MHz`、`µs`），最长匹配（§5.3）；大小写敏感（`Pa` ≠ `pa`）。
- 可带限定路径：`3 Units.SI.km` 合法；`3 p.x` 不是（`p` 不是单位）。
- `%` 是单位，不是取模：`50%` 与 `50 %` 都是 `Real[1]` 的 0.5；`50 % 3` 是语法错误。
  取模用 `mod(a, b)` 或 `rem(a, b)`。

### 1.8.3 上标与特殊字符

上标字符 `⁰¹²³⁴⁵⁶⁷⁸⁹ ⁻ ⁺` 只能紧跟单位名；`x²` 不是表达式，写 `x^2`。

| 写法 | 含义 |
|---|---|
| `°C` `°F` | 摄氏、华氏（仿射，§5.6） |
| `°` 单独 | 角度 `deg` |
| `Ω` / `Å` | `ohm` / `angstrom` |
| `µ`（U+00B5）、`μ`（U+03BC） | 都是微前缀，同一个前缀 |

### 1.8.4 类型与值（预览）

| 字面量 | 默认类型 | 值 |
|---|---|---|
| `3 m` | `Real[L]`（期望 `Float[L]` 处可实例化为 `Float`） | 3（相干 SI） |
| `3.0 m` | `Float[L]`（期望 `Real[L]` 处可为精确） | 3.0 |
| `25 °C` | `Point[Θ]` | 298.15 K（§5.6） |
| `1.5 kg*m^2/s^3` | `Float[M*L^2/T^3]` | 1.5 |
| `50 %` | `Real[1]`（期望 `Float` 时 `Float[1]`） | 0.5 |

显示单位（`m`、`km`、`°C`）不进入类型，是值的元数据，`to(unit, x)` 改变它（§5.7）。

## 1.9 运算符与标点

优先级与结合性见 §2.6。

| 记号 | 含义 |
|---|---|
| `+ - * /` `@` `^` | 算术、矩阵乘、幂 |
| `== != < <= > >=` | 比较 |
| `&&` `\|\|` `!` | 逻辑与、或、非；后缀 `!` 是阶乘（§2.7.5） |
| `\|>` `<\|` | 管道、反向应用 |
| `::` | 列表 cons |
| `..` `..=` `...` | 半开区间、闭区间（仅索引内）、记录行变量 |
| `->` `=>` `<-` `=` `:` | 函数类型/match 分支、lambda、do 绑定、定义、标注 |
| `?` `√` | 后缀错误传播、前缀平方根 |

同义写法在词法阶段归一为 ASCII 记号（保留源位置供诊断和格式化）：`·` `×` = `*`，
`÷` = `/`，`−` = `-`，`≤` `≥` `≠` = `<=` `>=` `!=`。`%` 只出现在单位字面量中。

## 1.10 词法错误

| 代码 | 情况 |
|---|---|
| `E0101` | 缩进中出现制表符 |
| `E0102` | 缩进不匹配（DEDENT 找不到层级） |
| `E0103` | 期待缩进块但没有缩进 |
| `E0104` | 未闭合的字符串、块注释、括号；字符串中的空插值 `{}` |
| `E0105` | 非法字符或未知转义 |
| `E0106` | 非法数值字面量（下划线位置、无数字） |
| `E0107` | 非法上标序列 |
| `E0108` | 数值字面量后缺少单位 |

词法错误 MUST 报告准确的行列范围，并尽可能给出修复建议。


<a id="ch-02-syntax"></a>

# 02 — 语法

本章定义声明、表达式、模式和类型的语法，并说明容易歧义处如何解析。完整 EBNF 在
[附录 A](#ch-appendix-a-grammar)，本章不重复。

## 2.1 源文件结构

```aevia
module Kinematics            # 可选，必须是第一条声明

import Units.SI as SI        # 限定访问
import Math { sqrt, pi }     # 选择性打开
open Units.SI                # 打开单位与常量

dimension Currency           # 自定义基量纲
unit EUR : Currency = 1      # 单位声明

type Vec2 = { x: Real[L], y: Real[L] }

fn fall_time(h: Float[L]): Float[T] = sqrt(2 * h / g)
```

一个文件是一个模块。顶层声明的顺序不影响名字解析（单位名在全部声明解析后统一解析）。
顶层可以是 `module`（至多一次，且第一）、`import`/`open`、`dimension`/`unit`、
`type`/`class`/`instance`/`derive`、`fn`/`let`、`test`。`private` 可加在 `type`、
`dimension`、`unit`、`fn`、`let`、`class` 前表示不导出，默认全部导出。`open` 的操作数
是模块路径，不能是 `import … as` 引入的别名（§7.2）。

## 2.2 声明

### 2.2.1 函数

```aevia
fn kinetic_energy(m: Real[M], v: Real[L/T]): Real[M*L^2/T^2] =
    m * v^2 / 2

fn norm((x, y): (Real[L], Real[L])): Real[L] = sqrt(x^2 + y^2)
```

- 参数可带类型标注，省略时由函数体和调用点推断；参数可以是模式。
- 返回类型写在 `)` 之后、`=` 之前；函数体是一个表达式，多行时是块（§1.4.4）。
- 函数声明是递归的：`fn f(x) = e` 是 `let rec f = fn (x) => e` 的语法糖。

### 2.2.2 值

```aevia
let g = 9.80665 m/s^2
let (x, y) = (1.0 m, 2.0 m)
let rec fact = fn (n: Int) => if n == 0 then 1 else n * fact(n - 1)
```

- 普通 `let` 不是递归的：`let x = x + 1` 的右侧 `x` 指向外层绑定，没有则 `E0301`。
  `let rec` 让名字在自身定义中可见。
- 顶层与局部 `let` 语义相同。模式必须与右侧类型匹配（`E0308`）；构造器模式绑定
  （`let Some(x) = e`）按不可驳处理。

### 2.2.3 类型

```aevia
type Vec2 = { x: Real[L], y: Real[L] }

type Shape =
    | Circle(Real[L])
    | Rect(Real[L], Real[L])

type Result<a, e> = | Ok(a) | Err(e)
```

- `type Name = T` 是**透明别名**。
- `type Name`（无 `=`）是**抽象类型**，构造器与操作由标准库或插件提供。
- `type Name = | C1(…) | C2(…)` 是**名义变体类型**，构造器是值/函数。单行变体 MUST 以 `|`
  开头以区别于别名（`type T = A(Int) | B` 是 `E0202`）；缩进形式可省略首个 `|`。
- 递归类型的递归出现须在函数箭头右侧、记录字段、元组或构造器参数中（正性，§3.6.4）。
- 构造器名在模块内全局可见；跨模块同名用 `Module.Ctor` 限定。

### 2.2.4 类型类与实例

```aevia
class Show a where
    show(x: a): String

instance Show for Real[d] where
    show(x) = format_quantity(x)

derive Eq, Show for Vec2
```

- 类至少有一个类型参数；方法只有签名，默认实现由 `derive` 或插件提供。
- 实例头是 `instance Class for Type`，`Real[d]` 中的 `d` 隐式全称量化。一致性与孤儿规则
  见 §7.4。
- `derive` 为记录和变体生成常见实例。

### 2.2.5 单位与量纲

```aevia
dimension Currency
unit USD : Currency = 1
unit EUR : Currency = 1.08 USD
unit m : L = 1 prefixable
unit degC : Θ = 1 K offset 273.15 K
```

语法与语义见第 05 章。

### 2.2.6 测试

```aevia
test "fall time from 20 m" =
    approx_eq(fall_time(20.0 m), 2.019 s, 0.001 s)

test "零是加法单位元" =
    forall(fn (x: Real) => x + 0 == x)
```

名称是字符串，体是 `Bool`（或 `Prop`，§8.13）表达式；`aevia test` 收集并运行。`forall`
是标准库函数，不是关键字，其谓词只有一个参数。

### 2.2.7 属性

```aevia
@stable
@deprecated("use new_api")
fn old_api(x: Real[1]): Real[1] = ...

@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))
```

- 已知属性：`@stable`、`@experimental`、`@deprecated`（稳定性，§13.8）、`@jit`（请求 JIT；
  后端没有 `core.jit` 时 `E1001`）、`@op`、`@rule`（注册 RSSN 算子与重写规则，§13.5）、
  `@bench`（§9.12）、`@allow`/`@warn`/`@deny`/`@forbid`（lint 级别，接收一个或多个 lint/组名与可选 `reason = "..."`）。
- 未知属性是 `E0210`；属性参数必须是编译期常量（lint 属性允许直接写 `unused-variable` 等 lint/组名）；试图降低 `forbid` 级别是 `E0211`，未知 lint/组名是 `E0212`。

## 2.3 表达式

### 2.3.1 块

块是 `let`/`fn` 声明序列加一个最终表达式（§1.4.4）；`let` 的作用域延伸到块尾。

```aevia
let y =
    let a = 1
    let b = 2
    a + b
```

### 2.3.2 `let … in`

```aevia
let x = let y = 2 in y^2 + 1
```

`let … in` 是非递归的单行形式；递归用块中的 `let rec` 或 `fn`。

### 2.3.3 `if`

```aevia
let s = if x > 0 then "positive" else "non-positive"
```

`if` 是表达式，必须有 `else`，`else if` 可连写；两个分支类型必须统一（量纲也要统一）。
多行写法的 `else` 与 `if` 同列（§1.4.4）。

### 2.3.4 `match`

```aevia
fn area(s: Shape): Real[L^2] =
    match s with
    | Circle(r) -> pi * r^2
    | Rect(w, h) -> w * h

fn sign(x: Real[1]): Int =
    match x with
    | 0 -> 0
    | n if n > 0 -> 1
    | _ -> -1
```

- 分支按顺序尝试；守卫 `if` 在模式匹配成功后求值，必须是 `Bool`；分支体可以是块。
- 非穷尽是 `E0309`，冗余分支是警告 `W0201`；穷尽性分析见 §2.4。
- 三种布局：分支缩进在 `match` 之下；分支与 `match` 同列；或**行内**写在 `with` 之后
  （括号内没有 `NEWLINE`，所以 `println(match o with | Some(v) -> v | None -> 0)` 只能
  行内）。行内分支的体到下一个 `|` 或外层表达式结束处为止，嵌套的行内 `match` 会吞掉
  后面的分支，须加括号。格式化器在表达式内部输出行内形式，行尾输出块形式。

### 2.3.5 lambda

```aevia
let square = fn (x: Real[L]) => x^2
let add = fn (x, y) => x + y
let inc = fn x => x + 1
let f = fn (x: Real[L]) =>
    let y = x^2
    y + 1
```

参数与返回类型可省略；闭包按词法捕获；`fn (x, y) => …` 接受两个参数，没有自动柯里化
（`curry`/`uncurry`，§8.3）。

### 2.3.6 调用与占位符

```aevia
f(x, y)
g()(x)                 # 调用返回的函数
map(_, xs)             # fn f => map(f, xs)
map(f, _)              # fn ys => map(f, ys)
```

调用必须写括号，实参以逗号分隔，没有可变参数，调用左结合。`_` 作为实参表示缺一个参数，
整个调用变成 lambda，参数按 `_` 出现顺序排列；`_` 在其他位置是 `E0202`。

### 2.3.7 运算符与管道

```aevia
let y = x |> square |> sqrt          # sqrt(square(x))
let z = sqrt <| square <| x          # 同上
let w = xs |> map(f, _) |> sum
```

- `a |> f` 与 `f <| a` 都等价于 `f(a)`；右侧是函数值，不自动柯里化，多参函数用占位符
  （`xs |> map(f, _)`）。两者同级，混用时按出现顺序左结合，建议只用一种。
- 部分应用 section（`(== 1)`、`(* 2)`）是 `E0205`，写 `fn y => y == 1`。

将运算符作为函数传递时，写作 `fn (x, y) => x + y`。括号中的运算符符号（如 `(+)`）报 `E0301`。

### 2.3.8 记录

```aevia
let p = { x = 1.0 m, y = 2.0 m }
let q = { p with x = 3.0 m }
let r = { x, y }                      # 字段 punning
let a = p.x
```

- 记录是结构化的，字段顺序不影响类型相等；重复字段是 `E0208`；`{}` 是合法的空记录。
- `{ p with x = … }` 产生新记录，未列字段不变，可新增字段。
- 字段访问是优先级最高的后缀运算。

### 2.3.9 变体构造

```aevia
let s = Circle(1.0 m)
let n = None
```

构造器是普通函数/值：`Circle : Real[L] -> Shape`，`None` 是 `Option<a>` 的多态值。构造器名
必须以大写字母开头。

### 2.3.10 张量

```aevia
let v = [1.0, 2.0, 3.0]              # Tensor<[3], Float>
let m = [[1.0, 2.0], [3.0, 4.0]]     # Tensor<[2,2], Float>
let e: Tensor<1, Float> = []         # 空张量需要标注
let q = zeros<Int>(3, 4)             # 显式元素类型；默认 Float
```

- 嵌套字面量构造 rank-n 张量，必须是矩形，否则 `E0203`；元素类型统一推断
  （`[1, 2.0]` 为 `Float`）；无标注的 `[]` 是 `E0314`。
- 显式类型实参写 `f<T>(…)`，`<` 必须紧邻函数名；比较的 `<` 前要有空格（§2.7.2）。

### 2.3.11 索引与切片

```aevia
let x = v[0]                         # 元素
let row = m[1]                       # rank 2 → rank 1
let a = m[0, 1]                      # 元素
let col = m[.., 1]                   # 列
let block = m[0..2, 1..3]            # 子矩阵
let last = v[-1]                     # 负索引：末尾
let ev = v[::2]                      # 步长切片
```

- 索引从 0 起，负数从末尾计；`..` 半开、`..=` 闭，省略端点表示到边界；`a[i]` 在 rank 1
  取元素，在 rank > 1 取第 i 个切片（rank 减一）。每个选择项对应原张量的一条轴，整数移除该轴，
  范围保留该轴，未指定的末尾轴保持原样。选择项多于轴时报告 `E0802`。下标、端点和步长都是
  无量纲的 `Int`。
- `a[i:j:s]` 是带步长的切片，`i`、`j`、`s` 都可省略（`s` 缺省为 1），`a[i:j]` 与 `a[i..j]`
  是同一节点。
- `::` 始终是 cons（§2.6），所以 `xs[1::2]` 是 `xs[1 :: 2]`；有起点又要步长时写出终点
  （`xs[1:5:2]`），省略终点时必须省略起点（`xs[::2]`）。`[...]` 内 `:` 是切片分隔符，
  带类型标注的下标要加括号：`xs[(i : Int)]`。
- 越界是运行时 `E0802`；索引是字面量时编译器可静态检查。切片产生不可变值，运行时可以共享
  缓冲区（§6.6.3.1）。

### 2.3.12 `do` 与 IO

```aevia
fn main(): IO Unit = do
    let name = "data.csv"
    text <- read_file(name)
    let rows = parse_csv(text)
    println("rows = {length(rows)}")
    pure(())
```

- `x <- action` 绑定 `IO` 动作的结果；`let` 是纯绑定；裸 `IO` 表达式依次执行，结果丢弃
  （类型不是 `IO Unit` 时警告 `W0202`）；非 `IO` 的语句是 `E0701`。
- `do` 的值是最后一条 `expr` 语句；`do` 语句中不能声明 `fn` 或 `let rec`，需要时先写成块表达式。
- `pure : a -> IO a` 是 Prelude 函数，不是关键字（§9.7.3）。`do` 只接受 `IO`。

### 2.3.13 错误传播 `?`

`e?` 要求 `e : Result<T, E>` 且所在函数返回 `Result<U, E>`：成功得 `T`，失败提前返回
`Err(e)`。`?` 也可用于 `Option`，所在函数返回 `Option`。

### 2.3.14 其他表达式

```aevia
()                                   # unit
(1, 2)                               # 元组
(1)                                  # 括号，不是元组
x : Real[L]                          # 类型标注
x!                                   # 阶乘（Int）
√x                                   # sqrt(x)
```

- 不支持单元素元组（`(1,)` 是 `E0202`），用值本身或记录。
- 类型标注 `e : T` 优先级最低，作用于整个表达式。
- `x!` 的规则见 §2.7.5。`√x` 是 `sqrt(x)` 的前缀写法，操作数类型必须在该处已知
  （`√x` 对已标注的 `x` 有效，`√4.0` 尚不能推断，写 `sqrt(4.0)`）。

## 2.4 模式

```aevia
let (x, y) = p
let { x, y } = point
let [a, b, ..rest] = xs

match shape with
| Circle(r) -> …
| Rect(w, h) if w > h -> …
| _ -> …
```

文法见附录 A.4。

- 变量模式绑定名字；`_` 不绑定；`p as x` 在匹配 `p` 的同时绑定整个值；`(p : T)` 标注。
- **绑定位置中的大写名字是变量。** 在 `let`、`do` 中的 `let`、`<-`、函数与 lambda 参数中，
  不带括号的 `UIDENT`（`let A = …`、`fn step(M: Tensor<2, Float>)`）绑定变量，因为科学
  代码惯用大写命名矩阵和常量。绑定位置的构造器模式必须带参数表（`let Some(x) = …`）；
  无参构造器只在 `match` 分支中按构造器匹配。绑定的变量遮蔽同名的值（包括单位名），
  但不影响 `3 A` 这类单位字面量。
- 构造器模式使用检查器确定的名义类型与构造器序号，显示名相同不代表同一构造器。
- 记录模式只匹配列出的字段（开放匹配）；`{ x, y }` 绑定同名字段，`{ x = p }` 重命名；
  元组模式要求元数相同。
- 张量模式只支持 rank 1：`[a, b, ..rest]`。无 `..rest` 要求长度精确相等；有 `..rest` 要求
  长度不小于前缀，rest 保留元素表示、单位和名义 schema，并共享原存储。
- 同一模式内绑定名必须唯一（含简写、`as`、rest），否则 `E0308`。
- 浮点字面量不是合法模式（`E0204`），用守卫。
- 穷尽性：保留多字段相关性（`(true,true)` 与 `(false,false)` 不覆盖 `(true,false)`）；
  `[]` 与 `[head, ..tail]` 覆盖全部长度；守卫不参与覆盖；分析有固定的工作与深度预算，
  无法证明穷尽时要求补充 `_`，绝不假定已覆盖。

## 2.5 类型语法

```aevia
Real[L/T]
Float[1]
Tensor<2, Float[L]>
Tensor<[3, ?], Real[d]>
{ x: Real[L], ...r }
(a, b) -> c
forall d. Real[d] -> Real[d^2]
Point[Θ]
Result<T, E>
IO Unit
```

文法见附录 A.5。要点：

- `Real`、`Float`、`Complex` 不带方括号等价于 `[1]`。
- `Tensor<2, Float>` 的 `2` 是秩；`Tensor<[3,4], Float>` 是细化形状，`?` 表示该维未知。
- `Unit[d]` 是单位值的类型（§5.2）；`Unit` 不带方括号是 unit 类型 `()`。
- 元组类型写 `(A, B)`；`A * B` 不是类型，`*` 只出现在量纲中。
- `type_param` 的 kind（`Type`、`Dim`、`Nat`、`Row`、`Rep`、`Format`、`Shape`）可省略，
  由使用位置推断。声明上的 `<…>` 已隐含全称量化，`forall` 通常省略。
- `IO` 作用于一个原子类型：`IO Unit`、`IO (Result<T, E>)`。

### 2.5.1 量纲语法

- `dim` 是 `*`、`/` 连接的因子，因子可带指数 `^n` 或 `^(p/q)`，`1` 是无量纲；`/` 左结合，
  `L/T^2` = `L/(T^2)`。
- 首字母大写的名字是基量纲或别名，小写名是量纲变量，只能出现在 `[…]` 中；裸写 `d` 是
  类型变量。
- 量纲别名（`Velocity`、`Force`）在检查时展开，打印时优先显示别名。

## 2.6 运算符优先级与结合性

从低到高：

| 级别 | 运算符 | 结合性 |
|---|---|---|
| 1 | `\|>` `<\|` | 左（混用时按出现顺序） |
| 2 | `\|\|` | 右 |
| 3 | `&&` | 右 |
| 4 | `==` `!=` `<` `<=` `>` `>=` | 非结合（连写是 `E0202`） |
| 5 | `::` | 右 |
| 6 | `+` `-` | 左 |
| 7 | `*` `/` `@` `·` `×` `÷` | 左 |
| 8 | 前缀 `-` `+` `!` `√` | 前缀 |
| 9 | `^` | 右 |
| 10 | 调用、索引、`.field`、`?`、后缀 `!` | 后缀 |

`..` 与 `..=` 只出现在索引中。`-x^2` = `-(x^2)`；`2^3^2` = `2^(3^2)`；`a * b @ c` =
`(a * b) @ c`，矩阵乘建议加括号；`x == y == z` 写作 `x == y && y == z`。`if`/`match`/`fn`/`let`
的体尽可能向右延伸，用括号消除歧义。`|` 只用于 match 分支和变体声明。

## 2.7 歧义与解决

### 2.7.1 数值后的标识符

`3 m` 是单位字面量，`3 x` 也是（`x` 非单位则 `E0501`），见 §1.8.1。

### 2.7.2 类型实参的 `<`

`zeros<Float>(3, 4)` 中 `<` 紧邻 `zeros`，按类型实参解析；`a < b` 中 `<` 前有空格，按比较
解析。解析器在 `ident<` 后尝试类型实参，失败则回退为比较，因此格式化器 MUST 在比较的
`<` 前保留空格。相邻的 `<…>` 只在 callee 是函数名（或其限定名）时合法；`3<Int>(2)`、
`f()<Int>` 是 `E0207`，提示加空格或给 callee 加括号。

### 2.7.3 `{` 是记录，不是块

块只用缩进，`{…}` 永远是记录：`if x then { y = 1 } else { y = 2 }` 是记录表达式。

### 2.7.4 `|` 的角色

`|` 出现在 `match` 分支（`| pattern -> expr`）和变体声明（`| Ctor(T)`）；逻辑或是两个字符
`||`；没有匿名 union 类型。

### 2.7.5 `!` 的前缀与后缀

- 前缀 `!x` 是逻辑非；`x != y` 的 `!=` 是单个记号。
- 后缀 `x!` 是阶乘，`x!!` 是双阶乘，`!` MUST 紧贴前一个操作数的末尾（无空白），操作数
  类型为 `Int`；`n !` 是语法错误。后缀 `!` 比前缀 `-` 和 `^` 结合得更紧：`-3!` = `-6`，
  `2^3!` = `2^6`。需要对复合表达式求阶乘时写 `factorial(n + 1)`。

带括号的操作数使用 `factorial(n + 1)`。后缀 `!` 紧随右括号时报 `E0202`。

### 2.7.6 一元 `-`

一元 `-` 优先级低于 `^`、高于 `*`：`-2^2` = `-4`，`(-2)^2` = `4`；`a - -b` 合法。

## 2.8 去糖

下列去糖形式只用于类型检查和核心 IR（§9.2.2），不是用户语法。

| 语法 | 去糖 |
|---|---|
| `fn f(x) = e` | `let rec f = fn (x) => e` |
| `a \|> f`、`f <\| a` | `f(a)` |
| `√x` | `sqrt(x)` |
| `x!`、`x!!` | `factorial(x)`、`double_factorial(x)` |
| `a @ b` | `matmul_rank(a, b)`（按秩分派到 matmul/matvec/vecmat/dot） |
| `a * b`（张量） | `mul_elementwise(a, b)` |
| `{ p with x = e }` | 记录更新原语 |
| `f(_, x)` | `fn a => f(a, x)` |
| `do …` | `bind`/`pure` 链 |
| `[a, b, c]` | `tensor([a, b, c])` |
| `a[i, j]`、`a[i..j]`、`a[i:j:s]` | `index(a, [i, j])`、`slice(a, i, j)`、`slice_step(a, i, j, s)` |

## 2.9 完整例子

```aevia
module Projectile

open Units.SI
import Math { sqrt, atan, sin, cos }

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
    let s0 = { x = 0.0 m, y = 0.0 m,
               vx = v0 * cos(angle), vy = v0 * sin(angle) }
    let ts = linspace(0.0 s, 10.0 s, 1000)
    odeint(rhs, s0, ts)

fn main(): IO Unit = do
    let path = trajectory(50.0 m/s, 45 deg)
    println("range = {path[-1].x}")
```

这个例子用到记录、字段访问、单位、量纲推断、`Float` 与 `Real` 混用、张量、ODE 积分、
`do` 和字符串插值（第 11 章逐行解释）。第 03 章说明这些表达式的类型从何而来。


<a id="ch-03-types"></a>

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
（`Show a`）、行排除约束（`r lacks z`）和数值约束：字面量的表示域、`Join(ρ1, ρ2, ρout)`、`Divide(ρin, ρout)` 以及依赖
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
`div` 采用欧几里得除法，`mod` 给出对应的非负余数。`quot` 将商向零截断，`rem` 的非零余数与被除数同号。
这些运算用于 `Int` 时保持任意精度，`gcd` 和 `lcm` 也如此。精确有理数的余数仍为精确值。
`to_int` 将有限的无量纲值向零截断，输入没有有限实数值时报告 `E0806`。

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

更新显式字段会替换其值和类型。例如，`{ p with x = true }` 把 `x` 的类型设为 `Bool`。
已知行中缺少的字段会追加。向开放记录添加 `z` 时，行尾不能包含 `z`。编译器会在函数调用、
别名和导入中保留该约束。若传入的行尾已包含 `z`，则报 `E0208`。

更新产生新的记录值，原值保持不变。记录具有唯一所有者时，运行时复用其存储。

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


<a id="ch-04-dimensions"></a>

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
`(-3/4)`；小数记法精确，例如 `0.5` = `1/2`）。**静态量纲的指数只能是编译期有理数**：`n` 是运行时值时类型
层无法表示 `d^n`，这类情况走 `Dyn`（§4.9）。rssn 的 `graph::dim` 在项图上支持单个符号
指数，Aevia 的静态类型不支持。 此扩展仅是单位/量纲的记法，不改变值级 `Float` 乘幂；
输出仍用规范有理数，`0.333333` 不等于 `1/3`。

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


<a id="ch-05-units"></a>

# 05 — 单位

第 04 章把量纲变成了类型；本章把单位变成**编译期已知的尺度**。单位不进入类型，只提供：

1. 一个量纲 `d`；
2. 一个尺度 `s`（相对相干 SI 单位的精确倍数）；
3. 一个仿射偏移 `o : Real[d]`（仅仿射单位）。

单位字面量 `3.2 km` 在编译期折叠为 `3200.0`（相干 SI 值），类型 `Float[L]`；`km` 不留在
值里。需要显示时用 `to(km, x)` 附加显示单位（§5.4）。

## 5.1 单位声明

```aevia
dimension Currency                  # 自定义基量纲
unit USD : Currency = 1             # 基单位
unit EUR : Currency = 1.08 USD      # 派生单位

unit m : Length = 1 prefixable      # 基单位，允许 SI 前缀
unit km : L = 1000 m                # 显式派生
unit N : M*L/T^2 = 1 kg*m/s^2
unit degC : Θ = 1 K offset 273.15 K # 仿射单位
```

### 5.1.1 语法

```
dimension_decl ::= [ "private" ] "dimension" UIDENT [ "=" dim ]
unit_decl      ::= [ "private" ] "unit" unit_name ":" dim "=" unit_rhs [ "prefixable" ]
unit_rhs       ::= unit_scale [ unit_expr ] [ "offset" unit_scale unit_expr ]
unit_scale     ::= number [ "/" number ] | "pi" [ "/" number ]
```

- `dimension`/`unit` 默认**导出**，`private` 仅本模块可见。导出项进入 manifest 与 registry，
  下游包可 `import`。`Units.*` 是 `manifest.units` 的生成视图，用户声明无需改标准库。
- `unit_name` 是标识符，或带 `°` 的特殊名（`°C`、`°F`）。
- `unit_scale` 是正有理数或 `pi` 的有理倍数。十进制字面量按**精确十进制**解释
  （`0.3048` 是 `381/1250`），不是 `Float`，所以 `ft → m` 换算精确。单位声明在编译期求值，
  不允许运行时表达式；要近似尺度就写足够精度的十进制字面量。
- `unit_expr`（§5.2）省略时表示基单位：尺度必须为 1，量纲必须是基量纲。
- `offset` 仅用于仿射单位，偏移量纲必须等于单位量纲（`E0504`）。
- `prefixable` 自动生成 SI 前缀版本（§5.1.4）。

### 5.1.2 基单位与派生单位

- 单位名必须唯一，重复声明报 `E0505`。同一量纲可以有多个尺度为 1 的单位；它们互相等价。
- 单位按声明顺序可见：使用尚未声明的单位是 `E0501`，因此定义不可能成环。
- 派生单位的尺度在编译期按精确有理数计算；若指数使尺度没有精确值（`3 ft^(1/2)`，
  `ft` 的尺度不是完全平方）报 `E0506`。
- 单位声明可跨模块导入；`private unit` 不导出。

### 5.1.3 单位命名空间

单位名与值名是**两个命名空间**：

- 字面量 `3 m` 中的 `m` 在单位命名空间解析；`let m = 5` 不改变 `3 m`。要乘以变量，写 `3 * m`。
- 普通表达式中的单位名是值 `Unit[d]`（全局可见）。
- 单位名冲突时最长匹配优先：`ms` 是毫秒，不是 `m*s`；要写米乘秒，写 `m*s`。
- 单位值不能用限定名访问（`Units.SI.km` 无效）；单位在内核表中全局可见，用短名即可。

单位名与量纲名也是两个命名空间，同一拼写可各有含义；解析器按位置区分：

| 拼写 | 单位位 | 量纲位 |
|---|---|---|
| `L` | 升（`liter`/`litre` 的别名，`prefixable`） | `Length` 的显示别名 |
| `T` | 特斯拉 | `Time` 的显示别名 |
| `G` | 高斯（`1e-4 T`） | 不是引力常数 |
| `h` | 小时 | 不是普朗克常数 |

规范名一律用全名（`Length`/`Mass`/`Time`/…），单字母只用于源码与显示。

### 5.1.4 前缀

| 符号 | 因子 | 符号 | 因子 | 符号 | 因子 | 符号 | 因子 |
|---|---|---|---|---|---|---|---|
| `Q` quetta | 10^30 | `P` peta | 10^15 | `h` hecto | 10^2 | `µ` micro | 10^-6 |
| `R` ronna | 10^27 | `T` tera | 10^12 | `da` deca | 10^1 | `n` nano | 10^-9 |
| `Y` yotta | 10^24 | `G` giga | 10^9 | `d` deci | 10^-1 | `p` pico | 10^-12 |
| `Z` zetta | 10^21 | `M` mega | 10^6 | `c` centi | 10^-2 | `f` femto | 10^-15 |
| `E` exa | 10^18 | `k` kilo | 10^3 | `m` milli | 10^-3 | `a` atto | 10^-18 |
| | | | | | | `z`/`y`/`r`/`q` | 10^-21/-24/-27/-30 |

- 前缀只加在 `prefixable` 单位上。`kg` 是 SI 基单位，前缀加在 `g`（`1/1000 kg`，
  `prefixable`）上；解析时**精确匹配优先于前缀分解**，所以 `kg` 是基单位而不是 `k`+`g`。
- 最长匹配：`dam` 是 `da`+`m`，不是 `d`+`am`。
- 前缀不叠加：`kkm` 不是单位，报 `E0501`。
- 二进制前缀 `Ki`、`Mi`、`Gi`、`Ti` 仅用于信息单位（`bit`、`B`）。

## 5.2 单位表达式

```
unit_expr   ::= unit_factor { ("*" | "·" | "/") unit_factor }
unit_factor ::= unit_atom [ "^" exponent ]
unit_atom   ::= unit_name | "(" unit_expr ")" | "%"
```

- `*` 与 `·` 同义；`/` 左结合。指数是精确有理数，支持小数记法：`m^2`、`s^-1`、`m^(1/2)`、`m^1.5`（§1.8）。
- 表达式内不允许数字（指数除外），不允许空白并置。
- 仿射单位只能单独出现，出现在乘积/幂中报 `E0508`（§5.6.5）。

### 5.2.1 单位值

单位表达式在值层是 `Unit[d]` 值：

```aevia
let speed_unit = km / h          # Unit[L/T]，尺度 1000/3600
let v = 100 * speed_unit         # Real[L/T]，值 250/9
```

`100 speed_unit` 不是单位字面量（`speed_unit` 不是单位名），报 `E0501`；写 `100 * speed_unit`。
`Unit[d]` 支持 `u * v : Unit[d*e]`、`u / v : Unit[d/e]`、`u ^ n : Unit[d^n]`（`n` 为字面量），
也可比较（同量纲）、打印、作参数。单位不是数值，不能直接与 `Real` 相加。

### 5.2.2 单位字面量的求值

```
value = number * scale(unit_expr) + offset(unit_expr)
type  = Scalar<rep(number), dim(unit_expr)>
```

- 线性单位 `offset = 0`：`3 km` → `3000`，`Real[L]`。
- 仿射单位只能单独出现：`25 °C` → `25 + 273.15 = 298.15`，`Point[Θ]`（§5.6）。
- `number` 为 `Int` 时默认 `Real`（精确），为 `Float` 时默认 `Float`：`3 km : Real[L]`，
  `3.0 km : Float[L]`。期望类型可覆盖表示：`let x: Real[L] = 3.0 m` 得精确 `3`；
  `fall_time(20 m)` 中 `20 m` 实例化为 `Float[L]`。
- 尺度含 `pi` 时结果可能是符号表达式：`30 deg` → `pi/6 : Real[1]`；期望 `Float` 时为
  `0.5235987755982988`。

## 5.3 单位表

标准库 `Units.SI` 提供下列单位（节选）。

### 5.3.1 SI 基单位

| 单位 | 量纲 | 说明 |
|---|---|---|
| `m` | `L` | 米，prefixable |
| `kg` | `M` | 千克（`g` prefixable） |
| `s` | `T` | 秒，prefixable |
| `A` | `I` | 安培，prefixable |
| `K` | `Θ` | 开尔文，prefixable |
| `mol` | `Amt` | 摩尔，prefixable |
| `cd` | `Lum` | 坎德拉，prefixable |

### 5.3.2 SI 派生单位

| 单位 | 量纲 | 定义 |
|---|---|---|
| `Hz` | `1/T` | `1/s` |
| `N` | `M*L/T^2` | `kg*m/s^2` |
| `Pa` | `M/(L*T^2)` | `N/m^2` |
| `J` | `M*L^2/T^2` | `N*m` |
| `W` | `M*L^2/T^3` | `J/s` |
| `C` | `I*T` | `A*s` |
| `V` | `M*L^2/(I*T^3)` | `W/A` |
| `F` | `I^2*T^4/(M*L^2)` | `C/V` |
| `Ω` / `ohm` | `M*L^2/(I^2*T^3)` | `V/A` |
| `S` | `I^2*T^3/(M*L^2)` | `1/Ω` |
| `Wb` | `M*L^2/(I*T^2)` | `V*s` |
| `T` | `M/(I*T^2)` | `Wb/m^2` |
| `H` | `M*L^2/(I^2*T^2)` | `Wb/A` |
| `lm` | `Lum` | `cd*sr` |
| `lx` | `Lum/L^2` | `lm/m^2` |
| `Bq` | `1/T` | `1/s` |
| `Gy`, `Sv` | `L^2/T^2` | `J/kg` |
| `kat` | `Amt/T` | `mol/s` |
| `rad`, `sr` | `1` | 无量纲，尺度 1 |

### 5.3.3 常用非 SI 单位

| 单位 | 量纲 | 定义 |
|---|---|---|
| `min` / `h` / `d` / `week` / `yr` | `T` | `60 s` / `3600 s` / `86400 s` / `604800 s` / `31557600 s` |
| `°` / `deg` | `1` | `pi/180` |
| `′` / `arcmin` | `1` | `pi/10800` |
| `″` / `arcsec` | `1` | `pi/648000` |
| `l` / `liter` | `L^3` | `1/1000 m^3` |
| `t` | `M` | `1000 kg` |
| `eV` | `Energy` | `1.602176634e-19 J` |
| `Da` / `u` | `M` | `1.66053906660e-27 kg` |
| `au` | `L` | `149597870700 m` |
| `pc` | `L` | `3.0856775814913673e16 m` |
| `ly` | `L` | `9460730472580800 m` |
| `Å` / `angstrom` | `L` | `1e-10 m` |
| `bar` / `atm` / `mmHg` / `psi` | `Pressure` | `100000 Pa` / `101325 Pa` / `133.322387415 Pa` / `6894.757293168 Pa` |
| `cal` | `Energy` | `4.184 J` |
| `Wh` | `Energy` | `3600 J`（`kWh` 是前缀形式） |
| `hp` | `Power` | `745.6998715822702 W` |
| `kn` / `mph` | `L/T` | `1852/3600 m/s` / `1609.344/3600 m/s` |
| `ft` / `in` / `yd` / `mi` / `nmi` | `L` | `0.3048` / `0.0254` / `0.9144` / `1609.344` / `1852` `m` |
| `acre` | `L^2` | `4046.8564224 m^2` |
| `gal` | `L^3` | `0.003785411784 m^3` |
| `oz` / `lb` | `M` | `0.028349523125 kg` / `0.45359237 kg` |
| `lbf` | `Force` | `4.4482216152605 N` |
| `°C` | `Θ` | 仿射：`1 K offset 273.15 K` |
| `°F` | `Θ` | 仿射：`5/9 K offset 255.3722222222222 K` |
| `bit` / `B` / `KiB` | `1` | 信息：`1` / `8 bit` / `1024 B` |

信息单位是无量纲的，因为大量公式把 bit 当概率/熵用。需要独立的 `Information` 基量纲时，
声明 `dimension Information`（kernel 的 `Config` 须声明 `base_dimension("Information")`）。

### 5.3.4 单位模块

| 模块 | 内容 |
|---|---|
| `Units.SI` | 基单位、派生单位、前缀、常用非 SI |
| `Units.Imperial` | 英制长度/质量/体积/压强/温度 |
| `Units.Astronomy` | `au`、`pc`、`ly`、`Rsun`、`Msun`、`Mearth`、`jup`（`Mpc` 由 `pc` 加前缀） |
| `Units.CGS` | `cm`、`g`、`dyn`、`erg`、`Gal`、`P`、`St`、`G`、`Mx`、`Ba`、`Oe` |
| `Units.Constants` | 物理常数（§5.8） |
| `Units.Natural`、`Units.Planck`、`Units.Atomic`、`Units.Geometrized` | 单位制下的常数模块（§5.11.1） |

以上模块均可 `open`，内容从 kernel manifest 生成（`manifest.units`、`manifest.constants`、
`manifest.unit_systems`），每个 `UnitValue` 带 `aliases`/`prefixable`/`category`/`since`。单位是
**session 私有**的：`Session::register_unit` 注册的单位只出现在该 session 的 manifest 中。

## 5.4 换算与显示

### 5.4.1 三个操作

```aevia
let d = 3.2 km              # Float[L]，值 3200.0
let e = to(m, d)            # 值 3200.0，显示单位 m
let f = value_in(km, d)     # Float[1]，值 3.2
let g = base(d)             # 值 3200.0，去掉显示单位
```

- `to(u, x)`：`x : Scalar<ρ,d>`、`u : Unit[d]`；值与类型不变，只附加显示单位。
- `value_in(u, x)` 等价于 `x / u`，返回无量纲数值。
- `base(x)` 去掉显示单位，返回相干 SI 值。
- 单位量纲与值不符是 `E0509`。
- **仿射单位只接受 `Point[d]`**：`value_in(°C, point(298.15, K))` 是 `25`；对温差
  （`Scalar<ρ,Θ>`）用仿射单位报 `E0510`，所以 `value_in(°C, 298.15 K)` 错误。温差的显示用
  `delta_to(°C, dT)`，它只用尺度、不用偏移。

例如 `delta_to(°C, 10.0 K)` 显示为 `10 °C`，而不是点 `-263.15 °C`。

### 5.4.2 显示单位的传播

运行时每个标量可带一个可选的显示单位：

- `x + y`、`x - y`：两边显示单位相同则保留，否则回到 SI。
- `x * y`、`x / y`：一方无量纲且带显示单位时保留另一方的显示，否则丢弃。
- `to` 显式设置，`base` 显式清除；函数调用默认丢弃（`abs` 等保持形状的除外）。

显示单位是**元数据**，不参与相等、类型或序列化：`3.2 km == 3200 m` 为 `true`。

`W0501`（丢弃显示单位）是保留码，语言不会发出它。

### 5.4.3 打印

- 有显示单位时按该单位换算后打印（`3.2 km`、`25 °C`）。
- 没有时选“最佳单位”：优先 SI 派生单位（`N`、`J`、`Pa`），否则基单位乘积
  （`kg*m/s^2`）；分数指数正确打印（`T^(1/2)` → `s^(1/2)`）。
- `show` 保证可读；`repr` 保证可解析回同一个值。
- 打印精度由 `Float` 的最短往返表示决定；`Real` 精确值打印为分数或根式。

## 5.5 角度与无量纲单位

- `rad` 是尺度为 1 的无量纲单位；`deg`/`°` 的尺度是 `pi/180`。
- `sin(30 deg)` 与 `sin(30)`（30 弧度）都合法，`sin` 只要求无量纲；类型系统不区分弧度与纯数。
- `atan2(y, x)` 返回无量纲弧度；用 `to(deg, ...)` 显示为角度。

## 5.6 仿射单位与 `Point`

### 5.6.1 问题与类型

温度有两种用法：**绝对温度**（仿射空间中的点，`25 °C` = 298.15 K）与**温差**（向量，可相加、缩放）。
混为一谈会让 `20 °C + 30 °C` 得到错误的 50 °C 或 596.3 K。

```
Point[Θ]        绝对温度（仿射点）
Float[Θ]        温差（向量）
```

- 只有仿射单位字面量产生 `Point[d]`：`25 °C`、`77 °F`；线性单位字面量产生温差：`25 K` 是温差。
- 用开尔文写绝对温度用 `point(298.15, K)`。
- `Point` 只支持 `Float` 表示；符号温度用 `Real[Θ]` 的温差。
- `Point` 可作张量元素（`Tensor<[nx,ny], Point[Θ]>` 是温度场），元素级运算按点规则；
  温度场的平均用 `centroid`，不是 `mean`（§13.9.2）。

### 5.6.2 运算

```
p - q        : Float[d]        两点相减得温差
p + x, x + p : Point[d]        点加温差
p - x        : Point[d]        点减温差
lerp(p,q,t)  : Point[d]        仿射组合，t : Float[1]
p + q, k * p, p / k, p / K   : 错误 E0405
```

- `Point` 可比较：`p == q`、`p < q`；`min`、`max` 返回 `Point`；
  `centroid : Tensor<1,Point[d]> -> Point[d]`。
- `p + x` / `p - x` 的 `x` 须是 `Float[Θ]`。精确的 `Real[Θ]` 变量不能直接参与（`E0302`，须
  `to_float`）；字面量 `10 K` 按期望类型实例化为 `Float`，所以 `25.0 °C + 10 K` 合法。
- `to(°C, p)` 设置显示单位，`value_in(°C, p)` 返回摄氏数值，`base(p)` 返回 K 值 `Float[Θ]`。

```
point     : (Float[1], Unit[d]) -> Point[d]
to        : (Unit[d], Scalar<ρ,d>) -> Scalar<ρ,d>      | (Unit[d], Point[d]) -> Point[d]
value_in  : (Unit[d], Scalar<ρ,d>) -> Scalar<ρ,1>      # 仿射单位时 E0510
value_in  : (Unit[d], Point[d]) -> Float[1]
base      : Scalar<ρ,d> -> Scalar<ρ,d>                 | Point[d] -> Float[d]
delta_to  : (Unit[d], Scalar<ρ,d>) -> Scalar<ρ,d>      # 只用尺度，不用偏移
unit_pow_dyn : (Unit[d], Int) -> Dyn<Real>             # 运行时指数
lerp      : (Point[d], Point[d], Float[1]) -> Point[d]
```

`point(25.0, °C)` 是 298.15 K；第一个参数是以该单位计的数值系数。

### 5.6.3 为什么开尔文是线性的

`K` 是线性单位（尺度 1、偏移 0），所以 `300 K` 在类型上是温差：开尔文刻度同时用于绝对温度与
温差，单位本身无法区分；摄氏/华氏是仿射刻度，字面量天然是点。

```aevia
let T1 = 25.0 °C            # Point[Θ]
let T2 = point(300.0, K)    # Point[Θ]
let dT = T2 - T1            # Float[Θ]
let T3 = T1 + 10.0 K        # Point[Θ]
```

### 5.6.4 仿射单位在单位表达式中

`°C/s`、`°C*m` 没有意义，解析器拒绝（`E0508`）。`°C` 只能单独作字面量单位，或作
`to`/`value_in` 的参数。

## 5.7 换算的实现

- 尺度在编译期折叠为精确有理数（或含 `pi` 的符号值）；`3 km` 直接生成 `3000`。
- 运行时只保留显示单位的名字与换算信息：`to(u, x)` 设置显示单位，打印时用 `x / u.scale`；
  仿射换算 `value_in(°C, p) = (p - 273.15 K) / 1 K`。
- 单位相等：同量纲且尺度相同（仿射还要偏移相同）。`m` 与 `100 cm` 是不同单位值，但同量纲，
  `to` 可互换。

## 5.8 物理常数

`Units.Constants` 由 `manifest.constants` 生成，符号与 RSSN 完全一致：

| 类别 | 常数（SI 单位） |
|---|---|
| 精确 | `c` (`m/s`)、`g_n` (`m/s^2`)、`h`、`hbar` (`J*s`)、`k_B` (`J/K`)、`N_A` (`1/mol`)、`e` (`C`)、`F` (`C/mol`)、`R` (`J/(mol*K)`)、`M_u` (`kg/mol`)、`T_tp` (`K`)、`atm` (`Pa`)、`G_0` (`S`)、`K_J` (`Hz/V`)、`R_K` (`ohm`)、`Phi_0` (`Wb`)、`K_cd` (`lm/W`)、`dnu_Cs` (`Hz`) |
| 测量 | `G` (`m^3/(kg*s^2)`)、`alpha` (`1`)、`R_inf` (`1/m`)、`a_0` (`m`)、`epsilon_0` (`F/m`)、`mu_0` (`N/A^2`)、`mu_B` (`J/T`)、`m_e`、`m_p`、`m_n`、`m_u` (`kg`)、`sigma_SB` (`W/(m^2*K^4)`) |

- 精确常数是定义值或其精确派生，无 `uncertainty`；测量常数是 CODATA 2022 值，带标准不确定度。
  两类在 manifest 中都是精确有理数（`ScaleValue`），`exact` 只表示“定义值”，不表示存储类型。
- 常数不是单位：有量纲、可参与运算，但不能出现在单位位置。核心语言不携带不确定度，
  需要时用 `Measurement`（§8.14）。
- `pi`、`e`（自然常数）、`i` 在 `Math` 中。`Units.Constants.e` 是**基本电荷**，与 `Math.e` 同名，
  同时 `open` 时须限定。

每个条目的元数据：

```
type ConstantInfo = {
    name: String,              # "speed of light in vacuum"
    symbol: String,            # "c"
    value: Scalar<Real, d>,    # 相干 SI 值
    dimension: Dim,
    unit: String,              # "m/s"
    exact: Bool,
    uncertainty: Option<Scalar<Float, d>>,
    source: String,
}
```

## 5.9 单位错误

| 代码 | 情况 |
|---|---|
| `E0501` | 未知单位名（含不可叠加的前缀 `kkm`、先使用后声明） |
| `E0502` | 单位未导入/未打开（保留码） |
| `E0503` | 单位不可加前缀（保留码） |
| `E0504` | 仿射单位偏移量纲不匹配 |
| `E0505` | 重复的单位名 |
| `E0506` | 尺度对该指数没有精确值 |
| `E0507` | 前缀单位与显式单位冲突（保留码） |
| `E0508` | 仿射单位出现在乘积/幂中 |
| `E0509` | `to`/`value_in` 的单位量纲与值不匹配 |
| `E0510` | 对温差使用仿射单位显示 |
| `E0511`/`E0512` | 单位制错误（§5.11） |

`E0502`/`E0503`/`E0507` 是保留码，不可达：单位表是 kernel 全表加本地声明，没有模块级
单位命名空间，“未导入”“禁止前缀”“前缀/显式冲突”三种状态都不存在；编译器不会用别的错误冒充它们。

## 5.10 与 rssn 单位层的衔接

1. **单一事实来源。** `Units.*` 是 `manifest.units` 的生成视图；用户 `dimension`/`unit` 声明注册进
   同一个 registry 并回流到 manifest。`UnitRegistry` 只用于 `resolve`/`format` 这类运行时服务。
2. **项图不带单位。** `Real`/`Float` 在宿主侧是相干 SI 数值；仅在 rssn 排版/导出时用
   `quantity(v, "<相干 SI 单位>")` 包一层，不进入 `compute`。
3. **仿射单位在桥接层消解。** `Point` 进入 rssn 前转成 K 的 `Float`；`to(°C, ...)` 只发生在显示层。
4. **registry 往返。** 仅对纯有理指数，经 `Dimension::exact_factors` 做
   `DimTerm → Dimension → DimTerm` 精确往返；符号指数不支持往返，不得解析 `Dimension.factors` 文本。
   `UnitDef → scale/offset → UnitDef` 必须精确往返。
5. **`DimensionMode::Strict` 只用于 FFI/导出。** 静态路径项图无量纲，用 `Off`；`Dyn<ρ>` 的量纲
   由宿主保存为 `DimTerm`。

## 5.11 单位制

物理学家常把几个常数取作 1（`c = 1`、`ħ = 1`）。Aevia 把这做成**模块级的量纲约化**：模块里
*写出来*的量纲随单位制化简，而每个值仍是诚实的 SI 量。

### 5.11.1 选择单位制

由模块头选择，名字是标识符：

```aevia
module Hep with units natural
```

kernel 提供四个单位制（`manifest.unit_systems`）；头里写其他名字是 `E0512`，诊断列出可选名字。

| 名字 | 取作 1 的量 | 约化后剩下的生成量纲 |
|---|---|---|
| `natural` | `c`、`hbar`、`k_B`、`epsilon_0`（Heaviside–Lorentz） | 能量、物质的量、发光强度 |
| `planck` | `c`、`hbar`、`G`、`k_B`、`4 pi epsilon_0` | 物质的量、发光强度 |
| `atomic` | `hbar`、`m_e`、`e`、`4 pi epsilon_0` | 温度、物质的量、发光强度 |
| `geometrized` | `c`、`G` | 长度、电流、温度、物质的量、发光强度 |

`Units.Natural` 等常数模块自己声明 `with units <system>`，`c`、`hbar`、`k_B` 在 `Units.Natural`
中由约化本身得到精确的 `1`。

### 5.11.2 约化

设取作 1 的量的量纲为 `q₁ … q_k`、生成量纲为 `g₁ … g_m`，它们在有理数上构成七个 SI 基量纲的一组基。
每个 SI 量纲 `d` 唯一分解为

```
d = Π qᵢ^λᵢ · Π gⱼ^μⱼ
```

`R(d) = Π gⱼ^μⱼ` 称 `d` 的**代表**。量纲为 `d` 的量 `x` 的**单位制值**是 `x / Π Qᵢ^λᵢ`（`Qᵢ` 为精确
SI 值），它是量纲为 `R(d)` 的 SI 量。例：`natural` 中 `R(L) = R(T) = Energy^-1`，`1 m` 的单位制值是
`1 m / (ħ c) ≈ 5.07e24 J⁻¹`。用户基量纲与量纲变量不在 SI 基张成的空间里，约化不改动它们。

带单位制的模块中，**该模块里写出的**量纲标注与单位字面量先约化再检查，所以 `fn f(x: Real[L])` 与
`fn f(x: Real[T])` 的参数在 `natural` 中是同一个类型。

### 5.11.3 值不被重新解释

约化只改变*写下*的量纲，从不改变值：每个量仍是 SI 量，只是类型用代表量纲书写。值可无需转换地
进出带单位制的模块；**导入的绑定保持它们在原模块中的 SI 类型**。

### 5.11.4 没有精确尺度的单位

单位字面量按单位制约化时需要精确尺度。若某个指数不是整数，尺度无理，该单位在此单位制下没有
精确尺度，报 `E0511`（典型：`natural` 中的 `C`，需要 `(ε₀ħc)^(1/2)`）。办法：直接写单位制中的数，
或用 `to_system`（§5.11.5）换算 SI 值。

### 5.11.5 显式换算

```aevia
to_system("natural", x)           # SI 量 x → 其代表量纲 R(d) 的单位制值
from_system<L>("natural", y)      # 单位制值 y → SI 量纲 L 的量
```

- 单位制名必须是字符串字面量，否则 `E0302`；未知名字 `E0512`；参数个数不是 2 是 `E0315`。
- `to_system` 要求实参量纲在检查时已知，否则 `E0304`；结果是同一表示、量纲为 `R(d)` 的 `Scalar`。
- `from_system<D>` 必须写出目标 SI 量纲 `D`（约化会遗忘它：`L` 与 `T` 都是 `Energy^-1`），否则
  `E0304`；`D` 总按 SI 读。实参量纲必须等于 `R(D)`，否则按普通量纲不匹配报告。
- 结果表示与实参相同：`Float`/`Complex` 乘以浮点因子；精确 `Int`/`Real` 在因子是不含 π 的精确尺度时
  保持精确有理数，否则是符号项，不会静默降为浮点。

### 5.11.6 显示

带单位制的模块里，没有自带显示单位的量按单位制的生成量纲显示：`natural` 中能量倒数显示为
`eV^-1`，而不是 `s^2/(m^2*kg)`。这只选择显示单位，值不变；记录、变体与元组逐字段显示。

下一章定义求值：`Real` 在什么时候是精确数、符号项或 `Float`，以及张量和 IO 如何运行。


<a id="ch-06-semantics"></a>

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

穷尽性与冗余分析是有界递归，**模式嵌套深度上限 24**；超过上限按“已覆盖”处理，只会抑制诊断，
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
  默认第一个下标**之前**。故 `xs[::-1]` 按逆序读取 `xs`。若 `xs` 有五个元素，`xs[::-2]` 依次读取 `xs[4]`、`xs[2]`、`xs[0]`。
- 显式端点的负数从末尾计并截断到合法范围；终点半开：`xs[4:0:-1]` 是 `xs[4]…xs[1]`。

长度是精确值：`s > 0` 时 `ceil((b - a) / s)`，`s < 0` 时 `ceil((a - b) / |s|)`。第一步就越过 `b` 的
切片是**空切片**（`xs[5:2:1]`、`xs[0:3:-1]`），不 panic。`s == 0` 永不推进：字面量已知时静态
`E0802`，否则运行时 `E0802`。

对于可共享缓冲区的非空 `F64` 切片，运行时使用视图。步长的绝对值乘到该轴的存储步幅上，负步长
反转方向。其他存储类型按切片结果生成新缓冲区。`m[:, ::-1]` 只翻转第二轴。

### 6.6.4 线性代数

- `det`、`inv` 与 `linsolve` 对精确整数和有理数使用有理消元。浮点输入使用 RSSN 的密集矩阵内核。
- `det` 接收无量纲方阵，奇异矩阵的行列式为零。需要动态量纲的行列式时使用 `det_unit`。非有限浮点元素触发 `E0806`。
- `inv` 与 `linsolve` 对奇异矩阵返回 `Err(Singular)`，对不兼容的形状返回 `Err(ShapeMismatch)`，对非有限浮点输入返回 `Err(NotConverged)`。`eig` 返回复数特征值。

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

CLI 的 `--budget` 限制 Aevia 的尾调用和 IO 步数（§9.3.4）。符号运算采用内核默认预算，
嵌入宿主通过 RSSN 的 `Config` 和 `Budget` 配置符号运算上限。

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

`to_pretty(x)` 提供二维终端数学排版（分数、根号、矩阵）；其他值使用人类可读文本。

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


<a id="ch-07-modules"></a>

# 07 — 模块、包与可见性

模块系统是**文件级、无 functor、无隐式全局状态**的：一个文件是一个模块，一个包是一组模块加一个
清单。名字解析在编译期完成，运行时没有动态导入。

## 7.1 模块

### 7.1.1 文件即模块

```
src/
  kinematics.aev          -> 模块 Kinematics
  thermo/
    ideal_gas.aev         -> 模块 Thermo.IdealGas
    cycles.aev            -> 模块 Thermo.Cycles
```

- 模块名由相对 `src/` 的路径决定，`UpperCamelCase` 分段，区分大小写；一个文件只有一个模块。
- 文件可写 `module Thermo.IdealGas`。被导入时，声明名 MUST 与源路径一致，否则 `E0601`；包依赖的导入别名
  属于导入者的名字空间，不改变定义处模块名（`Library` 包的 `src/Model.aev` 可声明 `module Model` 或
  `module Library.Model`，使用者写 `Renamed.Model` 时无需改名）。直接运行的入口文件没有导入路径上下文，
  不做此检查。
- 模块查找与执行遵守定义包的作用域：依赖里的 `import Helper` 查找该依赖的 `src/Helper.aev`，不被应用的
  同名模块覆盖。加载节点以（canonical 文件，所属包）去重；循环按完整节点报 `E0602`。
- `import Pkg` 使用依赖自身清单的 `entry`。

### 7.1.2 声明顺序

顶层声明可以相互引用（含前向引用与递归）：编译器先收集所有顶层名字与签名，再检查函数体。单位与量纲
声明同样可以后置；跨模块的单位通过 `import` 提供。

```aevia
fn f(x: Int): Int = g(x) + 1
fn g(x: Int): Int = x * 2
```

脚本与 REPL 都允许重复的顶层 `let`、`fn` 名称。新声明覆盖后续代码可见的绑定，并独立检查类型，
因此可以改变表示或量纲：

```aevia
let b = 1 s
let earlier = b
let b = 1 km
let later = b
```

`earlier` 仍是时间，`later` 是长度；已有闭包及函数别名保留原绑定与签名。非递归的覆盖声明在初始化式中
读取前一绑定（如 `let x = x + 1`），递归声明读取自身绑定。名字首次声明之前的前向引用选择它的第一个
定义。导出名称指向最终可见的定义；最终声明若为 `private`，则隐藏该名称。

### 7.1.3 导出

默认所有顶层声明导出；`private` 隐藏：

```aevia
private let internal_scale = 1.0e-3
private fn helper(x: Real[L]): Real[L] = x * internal_scale

fn public_api(x: Real[L]): Real[L] = helper(x)
```

- `private` 只能加在 `let`、`fn`、`type`、`dimension`、`unit`、`class` 上，否则 `E0202`；私有声明在模块内
  仍可见。
- 变体类型的构造器随类型导出，不能单独 `private`；`private type` 的构造器不进入公开接口。
- 实例总是随模块导出（否则单独编译会缺实例）；`private instance` 不存在（`E0202`）。

## 7.2 导入与打开

### 7.2.1 `import`

```aevia
open Units.SI
import Math as M
import Data.Csv { read_csv, Table }
```

- `import M` 把 `M` 绑定为限定名，用 `M.f`；`import M as N` 起别名。
- `import M { f, g }` 是选择性打开，等价于 `open M { f, g }`。
- 导入幂等，重复导入不报错。
- 找不到模块是 `E0607`，诊断给出期望的文件路径。

### 7.2.2 `open`

- `open M` 把 `M` 的所有导出名字不加限定地引入当前作用域；`open M { f, g }` 只引入指定名字。
- 普通 `open` 只影响当前模块的查找作用域。用 `export open M` 或 `export import M` 把导入的名字加入
  当前模块的接口。每个重新导出都沿实际解析出的导入边查找，保留定义模块的类型和可调用对象身份。
  选择性重新导出变体类型时，其公开构造器一并导出。
- 本地定义遮蔽打开的名字。两个模块 `open` 进同名**且类型不同**的名字时报 `E0603`；类型相同（同一模块
  打开两次）不算冲突。
- `import`/`open` 可引入值（函数、常量、单位值）、名义变体构造器与**透明类型别名**
  （`import A { Pair }`、`open Physics.Ising` 后直接写 `Params`）；别名随接口导出并在导入者展开。

**单位不依赖导入。** 内核单位表是全局的：未写任何 `import`/`open` 时 `3 km` 与 `to(m, x)` 都可用
（没有模块级单位命名空间，§5.9）；用户在 `unit` 声明里引入的单位随声明模块导出。单位值不能用限定名
`Units.SI.km` 访问。

### 7.2.3 限定名

`Math.sqrt`、`Thermo.IdealGas.pressure`：限定名从模块根开始，没有相对导入；它只是查找路径，不引入绑定。
类型、构造器、值都可以限定。需要先 `import` 该模块。

### 7.2.4 名字解析顺序

未限定名 `x` 依次在以下位置查找：

1. 局部绑定（参数、`let`、模式）；
2. 当前模块的顶层声明；
3. 当前模块 `open` 的名字；
4. Prelude；
5. 否则 `E0301`。

前面的步骤遮蔽后面的；冲突只在步骤 3 内部报 `E0603`。

## 7.3 依赖、接口与身份

- 模块依赖图 MUST 无环，循环导入是 `E0602`；类型/类/实例的依赖同样参与排序，实例解析需要相关模块已检查。
- 每个模块导出**接口**：类型与构造器、值签名、单位、类声明、实例、别名定义。别名 MUST 携带展开体，否则导入者
  无法展开 `type Vec2 = { x: Float, y: Float }`（只导出名字会得到 `E0301`）。
- 模块只合并自身依赖闭包的接口，不因另一模块较早被检查而获得其接口；没有隐式全局实例：实例只从导入的
  模块可见，标准库的常用实例由 Prelude 导入。
- 接口可缓存为 `.aevi` 文件，失效键为源码哈希、依赖接口哈希、能力集与编译器构建（§9.11.3）。
- **名义身份是符号化的。** 类键 `(定义模块路径, 类名)`；实例键再加规范头类型（别名先展开）；ADT 的身份由
  定义包的 canonical 源目录（含包名/版本）与包内相对源路径决定，可读的 `module` 名单独保留。同名同版本的两个
  本地包因源目录不同而有不同的类型与实例键；同一包的两个导入别名共享身份。`AdtId` 只是进程内见证，不进入
  接口、缓存或跨进程身份。
- 独立源与虚拟会话（REPL、未保存缓冲）也有稳定来源身份；身份不使用 `FileId`、整文件内容哈希或随机 nonce。
- ADT 的声明 schema 由其可达的名义类型图（构造器名/顺序、展开后的字段类型、参数种类、alpha 规范化的量纲
  绑定）规范编码；无关的源码编辑不改变它。
- 私有类及其本地实例保留确切运行时 key，但不进入公开接口；类方法作为值传递或被闭包捕获时保留完整
  `ClassKey`。
- 公开透明别名携带已解析的模板；私有 helper 别名在定义处展开，消费者无需看到它。

## 7.4 类型类实例与模块

- 导入一个模块即导入它的类、实例与别名。导入的实例参与**全局一致性**检查（重叠 `E0311`，消息指出两个
  模块），不重复孤儿检查。
- 孤儿规则：实例必须定义在类型所属模块或类所属模块，否则 `E0604`；`derive` 生成的实例同样适用。
- `private class` 只在定义模块可见，其实例也不进入接口。
- 运行时只在**量化的调用点**按见证分派（§9.2.3）；检查器必须能证明候选实例的见证两两不相交，否则在检查期
  报 `E0311`/`E0310`，绝不按运行时形状猜测。

## 7.5 包

### 7.5.1 清单

`aevia.toml`：

```toml
[package]
name = "kinematics"
version = "0.1.0"
edition = "2026"
entry = "src/main.aev"      # 可选；默认 src/main.aev
authors = ["Your Name <you@example.com>"]
description = "Kinematics solvers"
license = "Apache-2.0"
readme = "README.md"
repository = "https://github.com/apich/kinematics"
homepage = "https://aevia.org"
documentation = "https://docs.aevia.org"
keywords = ["physics"]
categories = ["science"]

[dependencies]
Units = "^1.2"                                        # 官方作用域：aevia/units
Plot = { package = "apich/plot", version = "~2.1" }   # registry 包
Kinematics = { path = "../kinematics" }               # 本地路径包
Dev = { path = "../dev", package = "apich/dev", version = "^0.3" }  # 本地开发用路径，发布后按版本
```

清单读取器读 `[package]`（`name`、`version`、`edition`、`entry`、`authors`、`description`、`license`、
`license-file`、`readme`、`repository`、`homepage`、`documentation`、`keywords`、`categories`）与
`[dependencies]`；其他表（`[compat]`、`[capabilities]`、`[build]`、`[profile.*]`）与未知键都是 `E0605`。
发布到 registry 时强制要求非空的 `description`，以及非空的 `license` 或打包在归档内的非空 `license-file`；
`aevia publish` 的选项（§9.9）和 `aevia-pkg.toml` 也可覆盖这些元数据。能力声明不作运行时权限检查，
权限通过 `--caps`（§9.14）控制。不支持 `git` 依赖（`E0605`）。

- `name` 是包名，`version` 是 SemVer 2.0 的 `major.minor.patch`，可带 `-pre-release`；发布版本不得带
  `+build`（两个仅 build 不同的版本优先级相同，registry 无法区分）。
- 依赖有三种来源：registry、path，或两者并存。registry 是内容寻址 + 签名的。
- 入口 `entry` 须定义 `main : IO Unit` 才能 `aevia run`；没有 `main` 时 CLI 报 `this program has no main`（无诊断码，退出码 1）；库包只被 `import`。

#### 7.5.1.1 依赖声明与版本需求

`[dependencies]` 的键是**导入根**（`UpperCamelCase`，不含 `-`；否则 `E0605`），值有以下形式：

| 写法 | 含义 |
|---|---|
| `Units = "^1.2"` | 官方作用域包 `aevia/units`。包名由键转 `snake_case`（`UnitsExtra` → `aevia/units_extra`）；缩写键（`FFT`）须显式写 `package` |
| `Plot = { package = "apich/plot", version = "~2.1" }` | 指定 `owner/name` 的 registry 包 |
| `Lib = { path = "../lib" }` | 本地路径包 |
| `Dev = { path = "../dev", package = "apich/dev", version = "^0.3" }` | 两者并存：从源码构建本包时用 `path`；作为 registry 包被使用时（`path` 目录不存在）只用 `package`/`version`。**发布的清单不得含纯 `path` 依赖**，服务器在发布时拒绝（`E_MANIFEST`） |

`version` 是**版本需求**（Cargo 的语法与含义）：

| 写法 | 含义 |
|---|---|
| `1.2.3`、`^1.2.3` | `>=1.2.3, <2.0.0`（裸版本即 `^`） |
| `^0.2.3` / `^0.0.3` | `>=0.2.3, <0.3.0` / 恰为 `0.0.3` |
| `~1.2.3`、`~1.2`、`~1` | `>=1.2.3, <1.3.0`、`<1.3.0`、`<2.0.0` |
| `1.*`、`1.2.x` | 通配 |
| `>=1.2, <2`、`=1.2.3`、`>1`、`<=1.2` | 比较子，逗号表示合取 |
| `*` | 任意**正式**版本 |

预发布版本（`1.0.0-rc.1`）只有当需求中某个比较子写出**同一** `major.minor.patch` 的预发布时才满足；`^1`、
`*` 永不选中预发布版本，所以 `aevia update` 不会悄悄采用候选版。

#### 7.5.1.2 解析：每个兼容系列至多一个版本

解析器是 `aevia-resolve`（纯 crate，无 I/O；工具链与 registry 共用）：

1. 满足所有需求，所选版本的依赖也被满足，且**无依赖环**（环报错并列出环）。
2. **每个包的每个兼容系列至多选一个版本**。兼容系列：`>=1.0` 取主版本（`1.x`），`0.y` 取 `0.y`，`0.0.z`
   取 `0.0.z`。所以 `1.4` 与 `1.9` 不会同时出现，`1.x` 与 `2.x` 可以共存并各有名义类型（§7.3）。共存的前提是
   两条边用**不同的导入根**；程序范围的导入根表仍只允许一个根名对应一个包，否则 `E0606`（消息列出争用的
   两个版本）。
3. 偏好**已锁定**的版本，其余取**最高**的兼容版本；`yanked` 版本只在锁文件已包含它时可被选中。
4. 结果与索引迭代顺序无关；不可满足时给出推导式说明：哪个需求无法满足、试过哪些候选及失败原因。
5. 实现使用冲突导向回跳。求解器优先处理可选版本最少的需求，数量相同则按待处理顺序选择。

registry 包的索引 `deps` 只记录 `package`、`req` 和（与默认导入根不同时）`import`。

### 7.5.2 锁文件

`aevia.lock` 记录每个依赖的精确版本、来源和摘要；构建 MUST 使用锁文件，`aevia update` 更新，`--locked` 下
任何变化都是错误。格式（`version = 1`，字段与顺序固定，包按 `(source, import)` 排序，未变的解析产生逐字节
相同的文件）：

```toml
[[package]]
name = "apich/plot"          # registry 包：registry 身份 owner/name
version = "2.1.3"
import = "Plot"              # 导入根（每个别名一条，与 path 包一致）
source = "registry+aevia"    # registry 的逻辑名，不是 URL：换镜像不改锁
checksum = "sha256:…"        # 归档摘要，来自索引（签名覆盖它）

[[package]]
name = "dep"                 # path 包：[package] name
version = "0.2.0"
import = "Dep"
source = "path+../dep"
hash = "fnv1a64:…"           # 清单 + 源树 + 入口的摘要
```

- 路径相对根包；path 依赖的摘要覆盖 canonical 清单、`src/**/*.aev` 与入口文件，改 `entry` 或依赖边也使
  `--locked` 失效；源目录中的符号链接循环报 `E0607`。
- 索引：`.aevia/registry/index/<owner>/<name>`（`$AEVIA_REGISTRY_DIR` 可共享）是
  `GET /api/v1/index/{owner}/{name}` 的原文。解析只读这些文件；缺失时 `aevia add/update/tree` 调用
  `aevia-pkg index` 下载（`--offline` 禁止），**编译器本身从不联网**。
- 源码：`aevia fetch`（= `aevia-pkg vendor`）下载归档，校验摘要与 OpenPGP/SSH 签名后解包到
  `.aevia/registry/src/<owner>/<name>/<version>/` 并写入 `.aevia-pkg-checksum`。构建 MUST 只读这些目录，拒绝
  没有该标记或标记与索引摘要不符的目录（`E0607`），也拒绝清单依赖与索引记录不一致的目录（`E0605`）。
- `aevia update` 无参数时忽略锁取最新；`aevia update owner/name…` 只移动指定的包。`aevia tree` 打印依赖树。
  `aevia clean` 保留 `.aevia/registry`，`aevia clean --registry` 才删除。
- `aevia add` 先解析完整候选图再暂存清单与锁；普通 I/O 失败会恢复原清单，失败的解析不改写输入。

解析层允许同一包的不兼容系列共存，但共存须使用不同的导入根。

### 7.5.3 目录布局

```
kinematics/
  aevia.toml
  aevia.lock
  src/
    main.aev
    kinematics.aev
    thermo/ideal_gas.aev
  tests/fall_time.aev
  examples/projectile.aev
  docs/
```

- `src/` 下的 `.aev` 都是模块。默认入口为 `src/main.aev`。独立测试文件可放在 `tests/`，运行时
  逐个指定路径，例如 `aevia test tests/fall_time.aev`。
- `aevia run --example name` 在 `examples/<name>.aev` 按名解析（名字不是路径，含路径分隔符被拒绝），找不到时
  列出可用示例；`--example` 与直接给文件互斥。
- `aevia doc` 生成 Markdown API 文档，默认写入 `docs/`。

### 7.5.4 测试

```aevia
# tests/fall_time.aev
open Kinematics

test "fall time from 20 m" =
    approx_eq(fall_time(20.0 m), 2.019 s, 0.001 s)
```

- `test` 声明是 `Bool` 表达式。`aevia test [file.aev]` 执行所选文件中的测试声明，省略文件时使用包入口。
  `approx_eq(a, b, tol)` 要求 `a`、`b`、`tol` 同量纲。
- 编译期错误测试用 `compile_fail("<source>")`（§8.13）：把源码片段交给编译器并要求**编译失败**；当前只支持
  不带导入的片段。`@compile_fail` 不是属性（未知属性 `E0210`）。属性测试用 `forall`（§8.13）。

### 7.5.5 文档

- `##` 文档注释附着到下一个声明，内容是 Markdown；`aevia doc` 提取类型签名、单位与例子，未文档化的声明
  在索引中报告。
- 文档注释中标 `aevia` 的代码块会被编译检查：`aevia test --doc [FILE…]` 抽取围栏块逐个编译；块首行可写
  `compile_fail` 或 `compile_fail EXXXX`。片段在自己的合成模块中编译，声明模块的 `import`/`open` 行被复制，
  但模块自身的声明不隐式进入片段，需要时显式导入。依赖或继承导入失败不满足 `compile_fail`。

## 7.6 版本与兼容性

- `edition` 是可选的包元数据。清单读取器保留该字符串，所有包使用本规范定义的语法和语义。
  库遵循 SemVer，破坏性变更升 major，新增 API 升 minor，修复升 patch。
- 类型/量纲签名的变更、单位尺度/偏移的变更、实例的增删是破坏性变更；新增单位是 minor。

## 7.7 模块错误

| 代码 | 情况 |
|---|---|
| `E0601` | 模块名与路径不一致 |
| `E0602` | 循环导入 |
| `E0603` | 两个模块打开了同名但类型不同的名字 |
| `E0604` | 孤儿实例 |
| `E0605` | 包清单错误（未知表/键、非 SemVer 版本、`git` 依赖、依赖键不是模块名、锁与索引不一致） |
| `E0606` | 依赖版本冲突（需求不可满足、依赖环、导入根争用） |
| `E0607` | 找不到模块/包（含 registry 索引未缓存、包未 `fetch`、校验标记不符、符号链接循环） |
| `E0608` | 嵌入 API `aevia_core::run` 没有得到可运行的模块（CLI 缺 `main` 时只打印消息，不带码） |

下一章给出标准库的签名。


<a id="ch-08-stdlib"></a>

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
reverse  : Tensor<n, a> -> Tensor<n, a>                  # n ≥ 2：翻转第一个轴
reverse_axis : (Tensor<n, a>, Int) -> Tensor<n, a>       # n ≥ 1：翻转指定轴
filter   : ((Tensor<n-1, a> -> Bool), Tensor<n, a>) -> Tensor<n, a>   # n ≥ 2：谓词看一行
take     : (Int, Tensor<n, a>) -> Tensor<n, a>           # n ≥ 2：取前几行
drop     : (Int, Tensor<n, a>) -> Tensor<n, a>
concat   : (Tensor<n, a>, Tensor<n, a>) -> Tensor<n, a>  # n ≥ 2：沿第一个轴堆叠
any      : ((Tensor<n-1, a> -> Bool), Tensor<n, a>) -> Bool
all      : ((Tensor<n-1, a> -> Bool), Tensor<n, a>) -> Bool
find     : ((Tensor<n-1, a> -> Bool), Tensor<n, a>) -> Option<Tensor<n-1, a>>
first    : Tensor<n, a> -> Option<Tensor<n-1, a>>
last     : Tensor<n, a> -> Option<Tensor<n-1, a>>
sort     : TotalOrd a => Tensor<1, a> -> Tensor<1, a>
sort     : TotalOrd a => Tensor<n, a> -> Tensor<n, a>      # n ≥ 2：行按字典序
sort_by  : ((a, a) -> Ordering, Tensor<1, a>) -> Tensor<1, a>
sort_by  : ((Tensor<n-1, a>, Tensor<n-1, a>) -> Ordering, Tensor<n, a>) -> Tensor<n, a>
sort_by_key : TotalOrd k => ((a -> k), Tensor<1, a>) -> Tensor<1, a>
sort_by_key : TotalOrd k => ((Tensor<n-1, a> -> k), Tensor<n, a>) -> Tensor<n, a>
sort_float : (Tensor<1, Float>, Bool) -> Tensor<1, Float>   # NaN last/first
sort_float : (Tensor<n, Float>, Bool) -> Tensor<n, Float>   # n ≥ 2：行按字典序
sort_axis : TotalOrd a => (Tensor<n, a>, Int) -> Tensor<n, a>   # 沿一个轴把每条一维切片排序
argsort  : TotalOrd a => Tensor<n, a> -> Tensor<1, Int>        # 稳定排序的置换（行的下标）
fold     : ((b, Tensor<n-1, a>) -> b, b, Tensor<n, a>) -> b    # n ≥ 2：累加器看一行
zip      : (Tensor<n, a>, Tensor<m, b>) -> Tensor<1, (Tensor<n-1, a>, Tensor<m-1, b>)>   # 逐行配对
enumerate : Tensor<n, a> -> Tensor<1, (Int, Tensor<n-1, a>)>    # n ≥ 2：下标与行
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

**任意秩。** 规范里写成 `Tensor<n, …>` 的操作对**任意秩** `n` 都有类型：`sum`、`prod`、`mean`、`var`、`std`、`norm`、`normalize`、`map`、`map2`/`zip_with`（同形）、
`flatten`（秩 ≥ 2）、`transpose`、`sum(a, axis)` 的 keepdims 形式、§8.8 的整个 FFT 族和下面的序列操作。秩不是一等类型参数，所以这些签名是**对秩的模板**：类型表列出秩 1 到 4 的实例
（`builtins::schemes`，供悬停与首类函数使用），调用时检查器按实参的秩实例化模板；实参的秩与任何实例都不符是 `E0302`，并带期望/实际类型。`first`/`last` 对空张量给 `None`；
`argmax`/`argmin` 对空张量是 `E0802`，NaN 不会胜出，相等时取最前的下标。

`range(start, stop, step)` 从 `start` 开始生成精确整数，不含终点 `stop`。
步长默认为 `1`，负步长产生降序序列，朝终点反方向前进时得到空张量。
步长为零或结果超过张量大小上限时报告 `E0802`。

**序列操作按第一个轴。** 秩 `n ≥ 2` 的张量是沿第一个轴的 `n−1` 秩切片（“行”）的序列（`Tensor<0, a>` 即 `a`，所以秩 1 的“行”是元素）：

- `reverse` 翻转第一个轴并保持形状（展平请显式写 `reverse(flatten(m))`）；`reverse_axis(t, k)` 翻转第 `k` 轴（负数从末尾数，越界 `E0802`）。`f64` 张量上两者都是视图，不复制缓冲区（§9.5）。
- `filter(p, m)`：`p` 接受一行（秩 `n−1`），结果保持秩 `n`；全部拒绝得到第一轴长度为 0 的同秩张量。`any`/`all`/`find` 逐行测试，`find`、`first`、`last` 返回 `Option<Tensor<n-1, a>>`。
- `take(k, m)`/`drop(k, m)` 按行取舍；`concat(a, b)` 沿第一个轴堆叠，其余轴长度必须一致（否则 `E0802`）。
- `fold(f, init, m)`/`fold_left`：`f(acc, row)` 从第一行到最后一行。
- `zip(a, b)` 逐行配对，长度取较短者，得到 `Tensor<1, (行或元素, 行或元素)>`（两个操作数的秩可以不同）；`enumerate(m)` 给出字段名为 `index`、`value` 的记录序列。
- `sort(m)`：行按字典序稳定排序，结果与 `m` 同形；元素类型要 `TotalOrd`（`Float` 用 `sort_float`）。`sort_by(cmp, m)` 的比较函数接受两行；`sort_by_key(f, m)` 按 `f(行)` 的键稳定排序
  （秩 1 时 `f` 接受元素）；`sort_float(m, nan_last)` 同 `sort`，NaN 前置或后置；`argsort(m)` 返回稳定排序的置换 `Tensor<1, Int>`（秩 1 是元素下标，秩 ≥ 2 是行下标）；
  `sort_axis(t, k)` 把沿第 `k` 轴的每条一维切片分别排序，形状不变。
- **元素级操作不变：** `map`/`map2`/`zip_with` 逐元素作用并保持形状；`sum`/`prod`/`mean`/`var`/`std`/`norm`/`normalize` 对全部元素归约；`cumsum`/`cumprod`/`argmax`/`argmin` 只对秩 1 定义。
  需要元素级的 `fold`/`sort`/`zip`，先 `flatten`。

`length` 对 `String` 和 `Tensor` 重载，由类型类 `Sized` 解析。

`par_map(f, xs)` 并行映射，结果顺序与输入一致（线程数不改变结果）。它
要求 `Parallel a`：`Int`、`Float[d]` 及其张量可以并行，符号 `Real` 不并行
（§3.8.4/§6.10）。元素类型由调用点决定，所以约束在调用点检查，而不是一个
全局实例。`par_tabulate`、`par_reduce` 与 `rng` 属于同一族；`par_reduce` 的块边界只取决于长度（§9.7.2）。

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
`set_of([1, 2, 3])`。集合运算是线性归并，`member` 是二分查找。修改不在原处进行：`set_insert` 返回新集合，旧值不变（§0.4 原则三）。

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
to_pretty : Show a => a -> String
to_latex : Show a => a -> String
to_typst : Show a => a -> String
print    : Show a => a -> IO Unit
println  : Show a => a -> IO Unit
eprintln : Show a => a -> IO Unit
```

### 8.1.8 程序参数

```
args : () -> Tensor<1, String>
```

`args()` 返回宿主传给程序的参数向量，**只包含 `aevia run file -- a b` 中
`--` 之后的参数**（`--` 本身不是参数）；没有参数时是空张量
（`length(args()) == 0`，打印 `[]`）。它按位序保留参数的原始字符串，不做
shell 解析（引号、空格、通配符已由 shell/宿主处理）；值在进程内不变，多次
调用返回同一组。类型是 `Tensor<1, String>`，因此可以 `map`/索引/`length`。

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
linspace : (Scalar<ρ,d>, Scalar<ρ,d>, Int) -> Tensor<1, Float[d]>
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

`diag(v)` 把数值向量或布尔向量放在方阵的主对角线上，其余位置使用输入表示中的零或 `false`。`diag(A)` 将矩阵的主对角线取为向量，并保留元素类型。`eye(n)` 返回 `n` 行 `n` 列的浮点单位矩阵。密集矩阵构造函数在分配前检查结果大小。`linspace(start, end, n)` 返回 `n` 个浮点采样值，量纲与端点一致。对有限端点，`n ≥ 2` 时包含两个端点，`n = 1` 时只取 `start`，`n ≤ 0` 时返回空张量。

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

**非对称系统：`gmres_with` 与 `bicgstab_with`。** `cg`/`cg_with` 要求对称正定；一般方阵用 `gmres_with(A, b, { tol, maxiter, restart, precond })`（重启 GMRES，右预条件，残差范数单调不增，
内存 `O(restart·n)`）或 `bicgstab_with(A, b, { tol, maxiter, precond })`（短递推、常数内存，残差历史可能不规则）。两者来自 rssn 的 Krylov 内核，从零向量起步，返回与 `cg_with` 同形的记录
（`x`、`iterations`、`residual`、`converged`、`precond`）；未达容差是 `converged = false` 而非错误，`restart = 0` 是 `E0802`。对非对称矩阵，`Ic0` 与 `Auto` 都改用 `Ilu0`（`Auto` 对 `Bcsr`
先试块 ILU(0)）；分解失败退回 `Jacobi`，`precond` 字段报告实际用的一种。元素必须是 `Float`（精确元素用 `spsolve`），结果量纲是 `e/d`。

**预条件共轭梯度。** `cg_with(A, b, { tol, maxiter, precond })` 是带预条件子的 `cg`（`cg` 本身不变）。`Precond` 是标准库变体（`import LinearAlgebra.Sparse { Precond, Jacobi, cg_with }`）：
`Identity`（无预条件子；不叫 `None` 是为了不与 `Option` 的 `None` 冲突）、`Jacobi`（对角缩放）、`BlockJacobi`（对角块求逆）、`Ilu0`（保持稀疏模式的不完全 LU）、`Ic0`（不完全 Cholesky）、
`Auto`（IC(0) → ILU(0) → Jacobi 中第一个能建成的；`Bcsr` 先试块 ILU(0) 再试块 Jacobi）。只接受 `Float`，要求对称正定。某个分解失败（零主元、不定）时退回 Jacobi，结果的 `precond` 字段
（`"Identity"`、`"Jacobi"`、`"BlockJacobi"`、`"BlockIlu0"`、`"Ilu0"`、`"Ic0"`）报告实际使用的一种。直接法是 `spsolve`：对 `Float` 元素调用 `faer` 的稀疏 LU（COLAMD 排序，不稠密化，
10^5 阶三对角系统是毫秒级）；对称正定矩阵（先检查严格对称与正对角线）先试稀疏 Cholesky，失败再退回 LU；精确元素（`Int`/`Real`/`Complex`/混合）用稀疏高斯消元，结果保持精确；
奇异矩阵是 `E0806`。`cg(A, b, tol, maxiter)` 是无预条件版本，返回 `{ x, iterations, residual, converged }`。

**并行与确定性。** `sparse @ dense`（含 `Bcsr` 的块内核）在乘加量不小于 2^17 时按行分块并行：块大小是常数（256 行，`Bcsr` 为一个块行），每个输出行由恰好一个工作项按列序累加，
所以结果与线程数无关，且与顺序内核逐位相同。Krylov 迭代本身是顺序的（只有 `A @ x` 并行），迭代序列同样与线程数无关。

**切片与重载。** 稀疏值支持切片：`A[i, :]`、`A[:, j]`、`A[r0..r1, c0..c1]`、`A[::2, 1..]`（语法与稠密张量相同）。整数下标去掉该轴，区间保留该轴并重新编号，所以 `A[i, :]` 是秩 1 的
`Sparse<Coo, [n], τ>`；结果秩不是 2 时一律是 `Coo`。切片只过滤并重新编址已存储项，**不稠密化**，代价 `O(nnz)`；越界整数下标是 `E0802`，下标个数必须等于秩。另有三个对 `Sparse` 重载的既有名字：
`sum(A)`、`sum(A, axis)`（结果是稠密张量）和 `get(A, i, j, …)`（也写 `A[i, j, …]`，未存储位置返回**隐式零**）。

**类型与格式。** `Csr`/`Csc`/`Coo`/`Block`/`Bcsr` 是内建 `Format` 标签（§3.1），不是值构造器。`Sparse` 是核心类型构造器，`F` 是 `Format` kind；它**不**擦除为 `Tensor`：二者之间没有隐式转换，格式是类型的一部分
（`Sparse<Csr, …>` 不能当作 `Sparse<Coo, …>` 传递，要用 `convert<G>`）。`from_dense<F>`/`speye<F>`/`spdiag<F>` 的 `F` 缺省为 `Csr`；`convert<G>` 必须写出目标格式。`Csr`/`Csc` 是二维格式，
`Coo` 是任意秩的稀疏张量。`Block` 是 `Bcsr` 的另一种写法，二者是同一个格式标签，`format_of` 一律报告 `Bcsr`。`Sparse<F, [m,n], τ>` 的细化形状会被检查与传播（`A @ B` 的内维、`A ± B`、
`spsolve`、`transpose`、`kron` 的 `[m*p, n*q]`、`to_dense`/`from_dense`），不符是编译错误。

**块压缩稀疏行（`Bcsr`）。** 矩阵被切成 `br × bc` 的稠密小块，只存含非零元素的块，块之间按 CSR 压缩：`ptr` 是块行指针（长 `m/br + 1`），`idx` 是每个已存储块的块列下标（块行内严格递增），
值缓冲每个块占 `br·bc` 个槽位（块内行主序）。块形状要求 `br | m`、`bc | n`，可用 `block_size(A)` 读回（非 `Bcsr` 格式是 `(1, 1)`）。适用于每个结点带多个未知量的系统（弹性、Navier–Stokes、耦合反应扩散）。

- **填充。** “已存储项永不等于隐式零”在 `Bcsr` 里只放宽到块内：已存储的**块**至少含一个非零元素，其余槽位可以是零（填充）。填充不是逻辑项：`nnz(A)` 数逻辑非零，`A[i, j]` 在填充处回答隐式零，
  `nonzeros`、`==`、`show`/`repr`、`to_dense` 与到内核的边界都只看到逻辑非零。其它运算经由按字典序排列的规范项形式进行，该形式跳过填充，因此对 `Bcsr` 的行为与 `Csr` 完全相同；结果保持 `Bcsr`
  （原块形状——转置时为 `(bc, br)`——仍整除结果形状就沿用，否则按下面的规则重选）。装箱元素的填充是该元素类的零，精确值永不舍入。
- **块形状。** `to_bcsr(A, br, bc)` 显式指定；`br`/`bc` 为零、不整除 `A` 的形状、或 `A` 的秩不是 2 是 `E0802`。`convert<Bcsr>`、`from_dense<Bcsr>`、`speye<Bcsr>`、`spdiag<Bcsr>` 与需要重选块形状的运算结果
  按**填充率**自动选取：候选是 `{4, 3, 2, 1}²` 中整除形状的块（方块在前），填充率 = 已存储槽位数 / 逻辑非零数；取面积最大且填充率不超过 `1.5` 者，面积相同取填充率低者，仍相同取靠前者，
  都不满足则 `(1, 1)`。rssn 内核的 `kernels::bsr::choose_block_size` 用同一规则。
- **复杂度。** 记 `B = br·bc`，`nb` 为已存储块数：存储 `nb·B + nb + m/br` 个字；`A @ x`（`k` 列）为 `O(nb·B·k)`，且只对非零槽位乘加，结果与同一矩阵的 `Csr` 内核逐位相同；
  转为规范项形式 `O(nb·B)`；构造 `O(nnz + nb·B)`（自动选取至多 15 趟 `O(nnz)` 扫描）。填充率接近 1 时最省；块太大则浪费，应换小块或用 `Csr`。

**运行时表示。** `Value::Sparse` 是一等运行时值。它保存形状、元素量纲、格式，以及按字典序排列、无重复的已存储项（CSR/CSC 用压缩指针，COO 用平铺坐标，`Bcsr` 用块指针与块列下标）；
已存储项永不等于隐式零（`Bcsr` 块内填充除外；`SparseData::validate` 在 debug 构建里检查）。元素存储与稠密张量同一规则：当且仅当每个已存储元素都是 `Float`（隐式零也是 `0.0`）时是 `f64`
缓冲，否则是装箱的 `Int`/`Real`/`Complex`/符号标量，所有运算经语言自己的标量算术，精确值**永不**舍入。隐式零是元素表示与量纲的零。

**构造与校验。** `sparse_coo(shape, indices, values)` 的 `indices` 可以是 `[nnz, rank]` 的 `Int` 矩阵、`Int` 元组列表，秩为 1 时也可以是 `Int` 列表；越界、负下标、长度不符是 `E0802`。
**重复坐标按求和合并**（有限元装配的语义），合并后等于零的项被丢弃。`sparse_csr(rows, cols, row_ptr, col_indices, values)` 要求 `row_ptr` 长 `rows+1`、以 `0` 开始、单调、以 `nnz` 结束，
且每行列下标严格递增且小于 `cols`，否则 `E0802`。`from_dense` 丢弃零元素，`to_dense` 填回隐式零；稠密结果的元素个数超过实现上限（10^8）报 `E0802`，所以意外的稠密化被拒绝而不是耗尽内存。

**算术与布局分派**（元素量纲规则与稠密张量完全一致：和要求量纲相等 `E0401`，积相乘，`spsolve` 相除）：

| 运算 | 结果 |
|---|---|
| `sparse ± sparse` | `Sparse`，格式取左操作数 |
| `sparse ± dense`、`dense ± sparse` | 稠密 `Tensor` |
| `sparse * s`、`s * sparse`、`sparse / s`（`s` 为标量或单位，含带量纲标量） | `Sparse` |
| `sparse * sparse`、`sparse * dense`、`dense * sparse`（Hadamard 积） | `Sparse`；只遍历已存储项，稠密一侧在未存储位置的值被忽略 |
| `sparse @ dense`（向量或矩阵）、`dense @ sparse` | 稠密 `Tensor` |
| `sparse @ sparse`、`matmul`、`kron` | `Sparse<Csr, …>` |
| `-A`、`transpose(A)` | `Sparse`，格式不变 |

`sparse / sparse`、标量除以稀疏、`sparse + 标量` 没有定义（会稠密化或产生无穷零），报 `E0302`；需要时先 `to_dense`。`==`/`!=` 比较两个稀疏值的**逻辑内容**（形状相同且已存储项相同，数值按与稠密张量
相同的规则比较，所以 `1` 等于 `1.0`），**与格式无关**。`show` 写成 `sparse(Csr, [3, 3], { (0, 0) = 4.0, … })`；`repr` 是能重建同一个值的表达式 `convert<Csr>(sparse_coo([3, 3], [(0, 0), …], [4.0, …]))`，
`Bcsr` 的 `repr` 带块形状：`to_bcsr(sparse_coo([4, 4], […], […]), 2, 2)`。

**秩 N 与收缩。** `contract(A, B, axes_a, axes_b)` 是成对张量收缩（`tensordot`）：`axes_a`/`axes_b` 是**列表字面量**（结果的秩由长度静态决定），结果的轴是 `A` 的自由轴再接 `B` 的自由轴。两个稀疏操作数得到
`Sparse<Coo, …>`；一个稀疏一个稠密得到稠密 `Tensor`；全部轴收缩得到标量；两个都稠密是类型错误。`map_nonzero(f, A)` 只对**已存储项**调用 `f`，因此要求 `f(0) = 0`（返回零的项被丢弃）；
`nonzeros(A)` 返回 `(下标, 值)` 元组的列表。

**与内核的边界。** rssn 的 `TypeExpr::Sparse` 只有 `rank`，所以 `Sparse<F, shape, τ>` 在边界降为 rank + 运行时格式；`aevia-rssn` 的 `Val::Sparse` 与 rssn 的 `Value::Sparse` 一一对应，**不稠密化**。
rssn 的 `term_of_value` 不接受 `Value::Sparse` 进入项图（`manifest.value_kinds` 里 `in_term_graph = false`），所以稀疏值不能直接喂给通用重写算子；Aevia 负责布局转换，RSSN 负责稀疏算法与其 `faer` 稀疏 LU 后端。
稀疏格式构成封闭集合（§13.9.5）。

注：`storage.sparse` 能力启用稀疏运算；`gmres_with`/`bicgstab_with` 仅接受浮点元素；标准库不提供稀疏特征值与 SVD。

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
histogram : (Tensor<1, Scalar<ρ,d>>, Int) -> { counts: Tensor<1, Int>, edges: Tensor<1, Float[d]> }
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
    | LogNormal(LogNormal)
    | Weibull(Weibull<d>)
    | Cauchy(Cauchy<d>)
    | F(F)
    | NegativeBinomial(NegativeBinomial)
    | Geometric(Geometric)

type Normal<d>      = { mu: Real[d], sigma: Real[d] }
type Uniform<d>     = { lo: Real[d], hi: Real[d] }
type Exponential<d> = { rate: Real[1/d] }
type Gamma<d>       = { shape: Real[1], rate: Real[1/d] }
type Beta           = { alpha: Real[1], beta: Real[1] }
type ChiSquared     = { df: Real[1] }
type StudentT       = { df: Real[1] }
type Poisson        = { lambda: Real[1] }
type Binomial       = { n: Int, p: Real[1] }
type LogNormal       = { mu: Real[1], sigma: Real[1] }
type Weibull<d>      = { shape: Real[1], scale: Real[d] }
type Cauchy<d>       = { loc: Real[d], scale: Real[d] }
type F               = { df1: Real[1], df2: Real[1] }
type NegativeBinomial = { r: Real[1], p: Real[1] }
type Geometric       = { p: Real[1] }

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

`Normal<L>` 的 `pdf` 量纲是 `1/L`；`cdf` 无量纲；`sample` 返回 `Real[L]`。`Weibull` 取其 `scale` 的量纲，`Cauchy` 取 `loc`/`scale` 的量纲；`LogNormal` 的参数描述无量纲的对数；
`NegativeBinomial` 与 `Geometric` 数成功前的失败次数，支撑集为 `0, 1, ...`。十五个构造器共用同一组 `pdf`、`cdf`、`sample`、`sample_n` 内建。

**用户自定义分布不是语言扩展点。** 多参数类已被移除（§3.8.1），而“分布参数 → 量纲”的映射需要类型函数（类似 `RateOf`），那是另一个设计。用户自定义分布在 §12.8.13 的扩展边界之外；
标准库可以为新的分布形态增加内建构造器，但那是库的封闭集合，不是语言扩展点。

## 8.8 FFT / Interpolate

```
fft      : Tensor<1, X[d]> -> Tensor<1, Complex[d]>       # 无归一化；X ∈ {Complex, Float, Int}
ifft     : Tensor<1, X[d]> -> Tensor<1, Complex[d]>       # 1/N
fft_norm : (Tensor<1, X[d]>, Norm) -> Tensor<1, Complex[d]>
ifft_norm : (Tensor<1, X[d]>, Norm) -> Tensor<1, Complex[d]>
fft_axis : (Tensor<n, X[d]>, Int) -> Tensor<n, Complex[d]>      # 沿一个轴
ifft_axis : (Tensor<n, X[d]>, Int) -> Tensor<n, Complex[d]>
fft2     : Tensor<n, X[d]> -> Tensor<n, Complex[d]>             # 最后两个轴
ifft2    : Tensor<n, X[d]> -> Tensor<n, Complex[d]>
fftn     : Tensor<n, X[d]> -> Tensor<n, Complex[d]>             # 全部轴
ifftn    : Tensor<n, X[d]> -> Tensor<n, Complex[d]>
fftn_axes : (Tensor<n, X[d]>, Tensor<1, Int>) -> Tensor<n, Complex[d]>   # 任意轴列表
ifftn_axes : (Tensor<n, X[d]>, Tensor<1, Int>) -> Tensor<n, Complex[d]>
fftn_norm : (Tensor<n, X[d]>, Norm) -> Tensor<n, Complex[d]>
ifftn_norm : (Tensor<n, X[d]>, Norm) -> Tensor<n, Complex[d]>
rfft     : Tensor<1, Float[d]> -> Tensor<1, Complex[d]>
irfft    : Tensor<1, Complex[d]> -> Tensor<1, Float[d]>
irfft    : (Tensor<1, Complex[d]>, Int) -> Tensor<1, Float[d]>  # 显式长度（奇数长度）
rfft2    : Tensor<n, Float[d]> -> Tensor<n, Complex[d]>         # 最后两个轴
irfft2   : Tensor<n, Complex[d]> -> Tensor<n, Float[d]>
irfft2   : (Tensor<n, Complex[d]>, Int) -> Tensor<n, Float[d]>
rfftn    : Tensor<n, Float[d]> -> Tensor<n, Complex[d]>         # 全部轴
irfftn   : Tensor<n, Complex[d]> -> Tensor<n, Float[d]>
irfftn   : (Tensor<n, Complex[d]>, Int) -> Tensor<n, Float[d]>
rfftn_axes : (Tensor<n, Float[d]>, Tensor<1, Int>) -> Tensor<n, Complex[d]>   # 半谱在列表最后一个轴
irfftn_axes : (Tensor<n, Complex[d]>, Tensor<1, Int>) -> Tensor<n, Float[d]>
irfftn_axes : (Tensor<n, Complex[d]>, Tensor<1, Int>, Int) -> Tensor<n, Float[d]>   # 显式长度（奇数长度）
fftshift : Tensor<n, a> -> Tensor<n, a>                         # 全部轴
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

**变换族。** 所有正向/逆向变换接受复数、`Float` 或 `Int` 张量（实数、整数按虚部为 0 提升），结果总是复数，量纲与样本相同；频率轴的量纲只出现在 `fftfreq`/`rfftfreq` 的结果里（它们取采样间隔
`Float[T]`，返回 `Float[1/T]`，`fftfreq(4, 0.5 s)` 的单位是 `Hz`）。多维变换由行列分解实现，沿每个被变换的轴依次做一维变换；任意秩都有类型（§8.1.5）。

- `fft2`/`ifft2` 变换最后两个轴；`fftn`/`ifftn` 变换全部轴；`fft_axis(t, k)` 一个轴；`fftn_axes(t, axes)` 任意轴列表。轴号从 0 开始，负数从末尾数；越界或重复的轴是 `E0802`。
- 归一化按被变换的总点数 `N`（各变换轴长度之积）：`Backward` 正向 1、逆向 `1/N`；`Forward` 正向 `1/N`、逆向 1；`Unitary` 两向都是 `1/√N`。
- `rfft`/`rfft2`/`rfftn` 取实数输入，最后一个被变换的轴只保留 `n/2 + 1` 个非冗余频点；`irfft`/`irfft2`/`irfftn` 由半谱重建实数张量，默认原长度为 `2(m − 1)`（偶数），奇数长度用带 `Int`
  的形式显式给出；`m` 与长度不相容是 `E0802`。
- `rfftn_axes(t, axes)`/`irfftn_axes(s, axes)`/`irfftn_axes(s, axes, n)` 是任意轴子集的实数变换（与 numpy 的 `rfftn(a, axes=…)` 一致）：列表里**最后一个**轴是半谱轴，其余轴做完整复数变换；
  `irfftn_axes` 在最后一个列出的轴上做 Hermitian 还原，`n` 是该轴的原长度（缺省 `2(m − 1)`）。列表为空、轴越界或重复是 `E0802`。
- `fftshift`/`ifftshift` 把零频率移到中间（每个被移位的轴循环平移 `⌊n/2⌋`，逆操作反向），对任意元素类型成立；`*_axis` 形式只移一个轴。

```
type Norm = | Backward | Forward | Unitary
type AxisScale = | Linear | Log
```

`polyfit` 要求自变量无量纲；带量纲时先除以单位 `x/u`，系数保持 `y` 的
量纲。这是量纲系统对 API 的约束：多项式系数 `a_i` 的量纲是 `e/d^i`，
不同次幂不同，无法放进同质张量。

## 8.9 ODE

```
type OdeMethod =
    | Rk4 | Dopri5 | Radau | Bdf | Rosenbrock23 | AutoSwitch
    | StormerVerlet | Ruth4 | GaussLegendre4 | GaussLegendre6
    | ImplicitMidpoint | AdaptiveRk4 | AdaptiveStormerVerlet | AdaptiveRuth4
    | AdaptiveImplicitMidpoint | AdaptiveGaussLegendre4 | AdaptiveGaussLegendre6
type OdeOpts<s> = {
    rtol: Float[1],
    atol: Float[1],
    method: OdeMethod,
    steps: Option<Int>,                 # Rk4 / 几何积分器：定步长子步数
    events: Option<(Float[T], s) -> Float[1]>,
}
```

```
type OdeAdaptiveOpts<s> = {
    rtol: Float[1],
    atol: Float[1],
    method: OdeMethod,
    steps: Option<Int>,
    events: Option<(Float[T], s) -> Float[1]>,
    h0: Float[T],
    h_min: Float[T],
    h_max: Float[T],
    max_steps: Int,
    max_evaluations: Int,
}

type OdePhysicalOpts<s> = {
    rtol: Float[1],
    atol: s,
    method: OdeMethod,
    steps: Option<Int>,
    events: Option<(Float[T], s) -> Float[1]>,
    h0: Float[T],
    h_min: Float[T],
    h_max: Float[T],
    max_steps: Int,
    max_evaluations: Int,
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

`odeint_with`、`solve_ivp`、`ode_solve` 也接受 `OdeAdaptiveOpts<s>` 或 `OdePhysicalOpts<s>`。
`h0 = 0.0 s` 自动选择初步长；`h_min`/`h_max` 限制自适应步长（最终到达端点的截断步可小于 `h_min`）。
`max_steps` 包括被拒绝的尝试步，`max_evaluations` 限制真实 RHS 调用次数，耗尽时返回数值错误。
`OdePhysicalOpts<s>` 的 `atol` 必须与初始状态的结构、张量形状和各叶节点量纲完全一致，
各分量为有限正值；运行时按该分量的容差归一化，使用 `atol_i + rtol * abs(y_i)` 的误差尺度，
在 RHS、事件与返回值边界恢复物理单位。旧的无量纲标量 `atol` 仍作用于各分量的规范单位数值。
`AdaptiveRk4`、`AdaptiveStormerVerlet`、`AdaptiveRuth4`、`AdaptiveImplicitMidpoint`、
`AdaptiveGaussLegendre4`、`AdaptiveGaussLegendre6` 用所选方法的一整步与两半步进行误差控制，
并检查三次 Hermite 连续扩展的中点缺陷；接受两半步的结果，不切换方法、不进行 Richardson 外推。
自适应状态依赖步长通常不保持定步长辛映射的长期几何性质。固定方法 `Rk4`、`StormerVerlet`、
`Ruth4`、`ImplicitMidpoint`、`GaussLegendre4`、`GaussLegendre6` 仍然使用 `steps`。

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

`StormerVerlet` 与 `Ruth4` 是二阶、四阶定步长辛积分器；展平状态须为偶数长度，
前半为位置 `q`，后半为动量 `p`，右端满足可分离的正则 Hamilton 系统。
`ImplicitMidpoint`、`GaussLegendre4`、`GaussLegendre6` 分别为二、四、六阶隐式 Gauss–Legendre RK，
支持一般状态。固定方法用 `steps` 指定子步数；非线性阶段不收敛时报错。
记录状态的各字段保留独立量纲，因此物理 `q`/`p` 可使用异质量纲记录；
单个数值张量只能有一种元素量纲。高级 `num_ode_*`、`num_dae_*`、`num_dde_*`
张量接口必须先显式归一化异质量纲分量，不能把不同物理量直接放入同一张量。

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

**刚性求解器选择。** `AutoSwitch` 先给 `Dopri5` 至多 `min(max_steps, 20000)` 次尝试；该额度耗尽且总预算仍有
余额时，用剩余尝试步额度从区间起点运行 `Radau`。两次尝试共享回调预算，`sol.method` 报告最终的方法。
选择依据是步数预算，不是特征值估计；已知刚性时可直接选 `Radau`、`Bdf` 或 `Rosenbrock23`。

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

**高级回调求解。** 下列 `Rssn_numerical_callback` 入口使用 §8.19 的已认证回调；状态张量同质，
不同量纲分量须显式归一化。时间、状态、残差量纲分别独立；导数是状态/时间，绝对容差具有所控状态的量纲。

| 入口族 | 方法与限制 |
|---|---|
| `num_ode_{rk4,verlet,ruth4,midpoint,gauss4,gauss6}_adaptive_typed` | 所选方法的步长加倍控制；端点、Hermite 历史与明确工作预算 |
| `num_dae_implicit_adaptive_typed` | 残差 `F(t,y,y') = 0`；BDF1 起步、变步长 BDF2、Newton 与步长加倍；代数初态须一致 |
| `num_dae_index{1,2}_adaptive_typed` | 半显式约束；时间、状态、代数变量与约束各有正参考尺度 |
| `num_dde_constant_adaptive_typed` | 正延迟的步进法、Dormand–Prince；步长不超过最小延迟并停在可达不连续点 |

这些入口有步长上下限、试探/回调预算与拒绝计数；预算耗尽或最小步仍不达标即失败，不强行接受。
DAE/DDE 回调入口不提供事件回调。纯函数 `Rssn_numerical_vector.num_ode_history_eval` 用物理时间、状态、
导数历史在区间内查询。可运行断言见 `examples/ode_adaptive_{geometric,dae,dde}.aev`。

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
    | DriftImplicitEuler | ThetaMaruyama | DriftImplicitMilstein

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
    cells: Int,                    # 所有自适应方法的最细噪声 cell；向上取二次幂，0 = 4096
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
内核**。本节列出的方法都可用于这些结构：

| method | calculus | 可交换（`Diagonal`、`General`） | 不可交换（`GeneralNoncommutative`） | strong/weak |
|---|---|---|---|---|
| `EulerMaruyama` | Itô | Euler–Maruyama | Euler–Maruyama | 1/2, 1 |
| `Milstein` | Itô | Milstein | Milstein + Lévy 面积（`milstein_noncommutative`） | 1, 1 |
| `MilsteinStratonovich` | Stratonovich | Milstein (Strat.) | Milstein (Strat.) + Lévy 面积（`milstein_noncommutative_stratonovich`） | 1, 1 |
| `HeunStratonovich` | Stratonovich | Heun | Heun | 1, 1；**不可交换时 1/2, 1** |
| `SrkPlaten` | Itô | Platen 无导数 SRK | Kloeden–Platen SRK（11.1.7，`srk_noncommutative`）+ Lévy 面积 | 1, 1 |
| `MilsteinNoncommutative` | Itô | 同下（显式别名，任何结构） | Milstein + Lévy 面积 | 1, 1 |
| `MilsteinNoncommutativeStratonovich` | Stratonovich | 同下（显式别名，任何结构） | Strat. Milstein + Lévy 面积 | 1, 1 |
| `DriftImplicitEuler` | Itô | 隐式漂移、显式扩散 | 同左 | 1/2, 1 |
| `ThetaMaruyama` | Itô | theta = 1/2 的隐式漂移、显式扩散 | 同左 | 1/2, 1 |
| `DriftImplicitMilstein` | Itô | 隐式漂移、Milstein 交叉项 | 加 Lévy 面积 | 1, 1 |

内核文档明确没有强于一阶的方案，Milstein 系也只是弱一阶，因此不声称弱二阶。
`sde_order(method)` 给可交换噪声下的阶；`sde_order_with(method, structure)`
按结构给阶（`HeunStratonovich` 没有 Lévy 面积项，在不可交换噪声上降为强
1/2——允许调用，但阶如实报告）。迭代积分：Itô `I_jl = ½ΔW_jΔW_l − ½δ_jl h + A_jl`，
Stratonovich `J_jl = I_jl + ½δ_jl h = ½ΔW_jΔW_l + A_jl`（`A` 是 Lévy 面积）。
SRK 的支持值 `Υ^j = Y + a h + b^j √h`（每个噪声通道一个，每步多 `m` 次扩散
求值），`Y' = Y + a h + Σ_j b^j ΔW_j + h^(-1/2) Σ_{j,l} (b^l(Υ^j) − b^l(Y)) I_jl`
（Stratonovich 版用 `J_jl` 与 Stratonovich 漂移）。

**Lévy 面积的 `terms` 默认值**：需要面积的路径（不可交换结构上的
`Milstein`/`MilsteinStratonovich`/`SrkPlaten`/`DriftImplicitMilstein`，以及两个 `*Noncommutative*`
方法）在 `terms == 0` 时取 `clamp(ceil(1/√h), 16, 4096)`，`h` 为步长（自适应
时为 Brownian cell 宽度 `(t1 − t0)/cells`）；显式 `terms > 0` 原样使用。
`terms` 字段必须存在；`0` 选择默认值。

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

**自适应步长。** `Fixed` 保持均匀网格；`Adaptive` 保留请求的方法，包括三个漂移隐式方案。
每次用同一条已采样噪声比较一整步与两半步，接受两半步的状态，不把 Euler、SRK 或 Heun 替换成 Milstein。
所有自适应方法都使用 `cells`：`0` 为 4096，否则向上取二次幂且至少为 2。粗增量由固定的最细独立增量求和，
迭代积分服从 Chen 关系；拒绝不重采已实现的噪声。最细可耦合区间仍不满足容差时报 `E0807`，不得强行接受。

Brownian 误差使用 `sqrt(dt/span)` 的局部二阶矩分配，独立的漂移误差使用 `dt/span`；相对尺度取初值、当前值、
试探值的分量最大模。这是局部估计，不保证全局误差。`max_steps` 限制试探与接受步；分配/工作上限分别为
1048576 个最细 cell、8388608 个存储系数/状态值、268435456 次 Lévy 面积更新。

**跳跃与稳定噪声。** 复合 Poisson 的到达时刻与标记在拒绝前一次采样，连续积分停在每次到达点并记录跳后状态；
非恒定补偿须提供期望跳跃回调。稳定噪声的粗增量由最细独立增量求和，局部矩阶取 `q = 0.9 alpha`。
`alpha < 2` 只支持 Euler、漂移隐式 Euler 和 theta；Brownian Milstein/Stratonovich 修正不适用。
`alpha = 2` 表示 `sqrt(2) W`，可使用所有 Brownian 方案。

`Rssn_numerical_callback` 提供 `num_sde_adaptive_{diag,general}_typed`、
`num_sde_jump_adaptive_{diag,general}_typed`、`num_sde_levy_adaptive_{diag,general}_typed`。
控制量包括物理端点/参考时间、状态量纲的绝对容差、相对容差、最细网格/试探/回调预算、种子和方法编号
（`SdeMethod` 声明顺序 0–9）；一般噪声另带通道数和可交换性。Brownian 系数为 `state/sqrt(time)`；
稳定噪声用无量纲 `L_(t/reference_time)`，系数为状态量纲，不用浮点 `alpha` 构造量纲幂。

**确定性。** 种子必须显式给出；固定/自适应的可交换 Brownian 路径使用语言 LCG/Box–Muller 流，
需要迭代面积的路径使用内核 xoshiro。种子与最细分辨率固定后，拒绝不改变已采样路径；系综种子只依赖
`(seed,index)`。只承诺同构建可复现。可运行断言见 `examples/sde_adaptive_*.aev`。

**错误表**：`E0802` 形状/参数（含 `General` 的 `m` 与扩散矩阵不符、非均匀网格、缺失必填项）；`E0806` 状态非有限；`E0807` 自适应步长塌缩或步数预算耗尽；`E1001` 保留给未实现的能力。`SdeError` 是不透明错误类型（像 `SimError`），语言层不暴露构造器。

**不提供**：用户自定义噪声结构、显式非均匀定步长网格、Brownian 桥稠密输出，以及标准
`sde_events` 在积分过程中的提前停止（它是事后截断）。回调求解器通过 §8.19 的高阶接口调用。
`Sde` 模块声明选项 ADT 并再导出本节入口；`terms`、`cells` 必须存在，值 `0` 选择默认值。
`sde_scheme_info` 只查询方案元数据；弱阶使用 `sde_order`/`sde_order_with`。

原生 Rust API 的可达性由 §8.19 的清单与源码分类分别报告。

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

### 8.10.1 清单接口的时间自适应

`Rssn_numerical_callback.num_pde_*_adaptive` 在固定空间离散上比较一整步与两半步，接受所选方法的两半步；
不替换积分器，不做 Richardson 外推。误差用场量纲的正绝对容差和无量纲相对容差缩放；拒绝的试探丢弃。
无终止事件时必须到达精确终点。初始/最小/最大步长、接受/拒绝步预算、单步内核调用与串行回调预算显式给出；
预算耗尽或最小步仍不达标即失败。

| 空间族 | 时间方法 | 空间维数 | 边界范围 |
|---|---|---|---|
| 规则差分：热 | 显式 Euler、完整后向 Euler、完整 Crank–Nicolson、Douglas ADI | 1–3 | 成对周期、静态 Dirichlet/Neumann/Robin；检查角点数据 |
| 规则差分：波 | 速度 Verlet（可重启的变步长 leapfrog） | 1–3 | 同上；位移与速度容差量纲独立 |
| 规则差分：平流 | 一阶迎风 | 1–3 | 同上；外向通量与多维 CFL 限制 |
| 单纯形有限元：热/反应 | theta：后向 Euler 或 Crank–Nicolson | 1–3 | 标记边界；ABI 构造结构化 P1/P2 单纯形网格；可选节点源回调 |
| 标量有限体积 | 迎风/Rusanov、经典 Lax–Friedrichs、MUSCL minmod/van Leer | 1 | 周期或透射；通量/波速回调 |
| 节点间断 Galerkin：标量平流 | SSPRK3 | 1–2 | 周期 |
| Fourier 伪谱 | ETDRK4 | 1–3 | 周期；非线性回调返回场/时间 |

Lax–Friedrichs 的空间粘性依赖步长，固定网格下可能无法达到紧时间容差，此时失败，不改变通量。
FEM 源在 theta 两端串行求值 `(time, coordinates) -> nodal field/time`，再对节点插值应用一致质量矩阵；
源与事件共享回调预算。DG 多维入口接收各轴单元数、物理盒长、多项式阶与各轴速度。
空间收敛须单独研究网格；空间细化不是时间自适应。Poisson、静态 Helmholtz/FEM 与椭圆 Chebyshev 求解没有时间步。

坐标、时间、场量纲独立为 `L`、`T`、`U`；扩散率为 `L^2/T`，平流/波速为 `L/T`，反应为 `1/T`，波速度为 `U/T`。
边界记录的向量字段为 `kind`、`value: U`、`flux: U/L`、`alpha: 1`、`beta: L`、`gamma: U`；
种类 0–3 为 Dirichlet、Neumann、Robin、周期。单项广播至活动面，否则每轴提供两个面；周期面须成对。
Robin 表示 `alpha*u + beta*du/dn = gamma`。FVM 通量为 `U*L/T`，特征速度为 `L/T`。
返回接受的 `times`/`states`、物理 `coordinates`、真实工作计数、最终误差比及可选终止事件时刻；波另返回 `velocities`。
可运行断言见 `examples/pde_adaptive_{heat,wave_event,advanced}.aev`。

## 8.11 Data

```
type Table<ρ>                  # 带行类型参数的不透明表格类型

read_csv<ρ>   : String -> IO (Result<Table<ρ>, DataError>)
read_csv_dyn  : String -> IO (Result<Table<Dynamic>, DataError>)
write_csv     : (String, Table<ρ>) -> IO (Result<(), DataError>)
read_npy<n>   : String -> IO (Result<Tensor<n, Float>, DataError>)
write_npy     : (String, Tensor<n, Float>) -> IO (Result<(), DataError>)
read_json     : String -> IO (Result<Json, DataError>)
write_json    : (String, Json) -> IO (Result<(), DataError>)

column        : (Table<ρ>, String) -> Result<Tensor<1, a>, DataError>
column_dyn    : (Table<ρ>, String) -> Result<Tensor<1, Value>, DataError>
row           : (Table<ρ>, Int) -> Result<ρ, DataError>
select        : (Table<ρ>, Tensor<1, String>) -> Table<σ>
filter        : ((ρ -> Bool), Table<ρ>) -> Table<ρ>
join          : (Table<ρ>, Table<σ>, String) -> Table<τ>
schema        : Table<ρ> -> Tensor<1, ColumnInfo>
to_table<ρ>   : Tensor<1, ρ> -> Result<Table<ρ>, DataError>
to_dynamic    : Table<ρ> -> Table<Dynamic>

type ColumnInfo = { name: String, dtype: String, unit: Option<String> }
```

对于运行时才获知列的文件，`read_csv_dyn` 返回 `Table<Dynamic>`。`read_csv` 的行类型参数由显式类型实参或外围表达式给出。两种读取函数都接受以逗号分隔的 `.csv` 文件和以制表符分隔的 `.tsv` 文件，并校验引号格式和各行的列数。`schema(t)` 返回运行时列名、检测到的单元格类型和可选的单位。表头 `time [s]` 将 `s` 保留为列元数据。数值单元格读为无量纲 `Float`，其余读为 `String`。

`column` 和 `column_dyn` 在运行时按字符串查找列名，列不存在时返回错误。`column` 的结果元素类型 `a` 由外围类型上下文给出，与表的行类型参数独立。`row` 从所选行构造记录。`filter` 保留行类型参数。`select` 和 `join` 在运行时确定结果列。连接时两张表必须各自包含键列，其余列名必须互异。`to_table` 从记录向量构造表格，`to_dynamic` 将同一表格表示为 `Table<Dynamic>`。

`read_npy` 的秩必须是具体自然数，例如 `read_npy<2>(path)`。省略时报告 `E0314`。文件的实际秩不符时，IO 动作返回 `Err(DataError)`。

`read_json` 接受 `.json` 文件。`read_csv`、`read_csv_dyn` 和 `read_json` 遇到不支持的扩展名时，在 IO 动作执行后返回含 `E0904` 的 `Err(DataError)`。

NPY 支持 1.0/2.0/3.0、小端与大端 float32/float64、C 与 Fortran 排列（Fortran 输入转换为行优先存储；写出用小端 float64、C 排列）；`read_npy<n>` 保存并验证具体秩，`write_npy` 接受任意秩的无量纲 `Float` 张量；未具体化的泛型秩报 `E0304`。
IO 错误码：文件不存在 `E0901`，非法参数 `E0902`，损坏/截断/秩不匹配/大小溢出 `E0903`，不支持的格式、dtype 或扩展名 `E0904`，绘图写出失败 `E0905`；NPY 错误在 IO 动作执行后作为 `Err(DataError)` 返回。

## 8.12 Plot

```
type PlotSpec
type Series

line          : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Series
line_with     : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, PlotOpts) -> Series
scatter       : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Series
scatter_with  : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, PlotOpts) -> Series
errorbar      : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Tensor<1, Scalar<ρ,e>>) -> Series
errorbar_with : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Tensor<1, Scalar<ρ,e>>, PlotOpts) -> Series
hist          : (Tensor<1, Scalar<ρ,d>>, Int) -> Series
hist_with     : (Tensor<1, Scalar<ρ,d>>, Int, PlotOpts) -> Series
heatmap       : (Tensor<2, Scalar<ρ,d>>) -> Series
heatmap_with  : (Tensor<2, Scalar<ρ,d>>, PlotOpts) -> Series
label         : (Series, String) -> Series
combine       : Tensor<1, Series> -> PlotSpec
plot          : PlotSpec -> IO Unit
save          : (String, PlotSpec) -> IO Unit
plot_save_gif : (String, Tensor<1, PlotSpec>, Int) -> IO Result<Unit, IOError>
default_plot_opts : PlotOpts
```

`line`、`scatter` 和 `errorbar` 接受等长向量，坐标须能求值为实数。含 NaN 或无穷大的坐标对会被跳过。折线连接相邻的有效坐标对，散点图逐点绘制标记。误差量须非负且与 `y` 同量纲。误差量为非有限值时仍绘制中心点，并省略该点的误差条。

`hist(data, bins)` 使用正整数个等宽区间，至少需要一个有限观测值，并跳过非有限观测值。`heatmap` 将矩阵的每个元素映射到一个单元格，第零行位于顶部。非有限值对应的单元格留空。

`PlotOpts` 是一个记录。`label`、`color`、`xunit` 和 `yunit` 的类型为 `Option<String>`。`xscale` 和 `yscale` 可取 `Linear` 或 `Log`。`xlim` 和 `ylim` 的类型为 `Option<(Float, Float)>`。可用记录更新从 `default_plot_opts` 设置字段。显式指定刻度时，可从 `FFT` 导入 `Linear` 和 `Log`。颜色接受六位十六进制 RGB 字符串和名称 `red`、`blue`、`green`、`orange`、`purple`、`black`。SVG 将标签显示为提示文字，并在坐标轴上标注单位。`label(series, text)` 可覆盖提示文字。

同一帧内的系列共用坐标轴，坐标量纲和显式指定的轴设置须一致。默认采用相干单位、线性刻度和自动轴限。指定单位时使用已注册的单位名，支持有效的 SI 前缀。轴限以显示单位给出，须是有限且递增的一对值。`Log` 表示以十为底的对数刻度，要求轴限为正，并跳过该轴坐标非正的点。

SVG 和 GIF 均使用 800 × 400 画布，绘图区为 720 × 340。`plot` 将内嵌 SVG 的 HTML 写入 `plot.html`。`save` 对扩展名为 `.svg` 的路径写入独立 SVG，扩展名匹配不区分大小写。其他路径写入内嵌 SVG 的 HTML。无效图形或写入失败报 `E0905`。

`plot_save_gif` 按顺序渲染非空的图形规格向量，动画持续循环。`Plot.plot_save_gif(path, frames, delay_cs)` 显式指定帧延时，单位为百分之一秒，可取 0 到 65535 的整数。直接调用 `plot_save_gif(path, frames)` 时，延时为 10。栅格分配上限允许最多 312 帧。IO 动作返回 `Ok(())` 或 `Err(IOError)`。帧数据或延时不合法时，错误值包含 `E0902`。

## 8.13 Test

```
assert      : Bool -> IO Unit
assert_eq   : (Eq a, Show a) => (a, a) -> IO Unit
approx_eq   : (Scalar<ρ,d>, Scalar<ρ,d>, Scalar<ρ,d>) -> Bool
forall      : (a -> Prop) -> Bool
compile_fail: String -> Bool
```

`approx_eq(a, b, tol)` 要求三个实参的量纲相同。`tol` 是绝对容差。

`forall` 的谓词应接受一个无量纲的 `Real`。样本依次为 `0`、`1`、`-1`、`2`、`-3` 和 `1/2`。
若每个样本均满足命题，返回 `true`。遇到第一个反例时返回 `false`。没有反例但存在无法判定的样本时报 `E0804`。
谓词必须返回 `Prop`，而 `Real` 上的比较会产生 `Prop`。类型检查器允许参数类型为泛型，运行时始终提供上述无量纲数值。

`compile_fail` 接受一段源码字符串，在**隔离**环境中编译（无导入、
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

相关误差需要协方差矩阵，核心库不提供。`Measurement` 是库类型：checker 把它当普通记录 `{ value, uncertainty }` **结构化识别**（不是新的名义类型），字段名/顺序因此是边界的一部分。
上面的是**算子**（重载），不是名字，所以不在名字覆盖审计里。checker（`binary_type`）与运行时（`apply_binary`）都已实现（`aevia-core/tests/measurement.rs`）。实现还支持一元 `-` 与“测量 ×/÷ 标量”
（不确定度乘 `|scalar|`）；`Measurement + number`（或 `-`）仍是 `E0302`，`number / measurement` 也被拒绝（提示用测量的值相除）。

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

> **生成的仿真模块**：`import Rssn_Sim_<Name>`（如 `Rssn_Sim_PoissonMultigrid`、`Rssn_Sim_HeatNd`）在导入时由 rssn 的场景目录（`rssn::sim::schema`）生成类型化模块：`Params`/`Output` 记录、
> 每个方法名字段一个变体类型（`P.W`、`S.Metropolis`）、`defaults()` 与 `run(p)`。参数类型与默认值从场景自己的请求类型推断，结果类型来自一次见证运行，注解表补充推断看不到的部分；
> rssn 新增场景、参数或方法时无需手写代码。

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

`OpKey` 在 Aevia 侧是 `(rule_set, name, signature_hash)`：`call_key` 收 name + `signature_hash`（32 个 hex 字符，全零表示该算子没有声明重载）；`resolve_overload` 成功时返回
`"rule_set.name@signature_hash"`；`overloads` 返回渲染后的签名文本。`Rssn.call` 是 `Rssn.dynamic` 的别名（两者都收短名 + `Value` 数组）。

`Rssn` 的成员由 Kernel Manifest 生成（规则见 §13.2.2）：具有精确 `Declared`/`Overlay` 签名、源码可表达且桥接支持的重载生成类型化声明，调用钉死生成时的 `signature_hash`；`Derived`、`None` 及无法认证的签名保留动态入口；别名生成 `Rssn.<alias>`，冲突时用 `Rssn.<rule_set>.<name>`（命名空间，不是运行时键）。生成的声明调用内部形式 `Rssn.invoke(name, signature_hash, args)`（同样钉死 overload，但错误通道不同：
生成的 `fn` 必须返回声明类型，所以 kernel 失败会 raise 而不是返回 `Err`；它不是固定入口）。manifest 规模由 `aevia rssn` 实测（§10.8）。`Derived` 是由注册信息推出的保守签名，不区分 `Tensor`/`Quantity`/`Int`，IDE 标“保守推断”（§13.2.2）。SDE 的两个噪声原语 `sde_wiener`/`sde_levy_areas`
与方案元数据 `sde_scheme_info`（`(String) -> Tensor<1, Real>`，只给强阶与 flag）是 overlay 精确签名的 typed 声明；SDE 回调求解器使用受限高阶 ABI（§8.9.1）；噪声原语与方案元数据不是求解器的替代品。

公开 Rust API 不自动成为清单算子。具体数值函数从源码及逐项量纲策略生成绑定；泛型回调、状态对象和不透明模型需要显式适配。
清单可达性与原生源码分类分别报告，不据清单覆盖推断全部原生功能已覆盖。

仅当结构变体恰好为无载荷的 `None` 和单载荷的 `Some` 时，生成器递归映射为 `Option<T>`；其他变体保持动态表示。
载荷仍须通过类型和量纲校验，空 `None` 不提供载荷单位证据；此映射不扩大回调桥可接受的 ADT 签名。

固定入口的可达性与限制：

- `type_rep<T>()` 从具体的检查后类型生成不可伪造的 `TypeRep`；未解析的泛型不能在没有运行时类型字典时反射。`Rssn.typed` 执行动态调用并用该见证检查返回值；不匹配或证据不足返回 `Err(OpError)`，
  成功结果仍为不透明 `Value`，需用 `from_value<T>` 验证并转换。
- `to_callback(f)` 对当前 evaluator 会话中已检查的源码闭包验证具体函数类型，构造不透明的 `Callback`（别名和已具体化的泛型函数保留此证据；内建函数先包装在源码 lambda 中）。回调桥支持具体数值、
  `Bool`、`String`、密封宿主类型 `NativeAdVar` 及由它们递归组成的张量和闭合记录；源码同名类型及其他不透明回调类型仍拒绝。具体量纲、秩和完整已知形状都会校验，空结构张量需要独立的元素类型及量纲证据。各算子可进一步限制签名，例如当前数值求解器使用 `Float` 标量与向量。
  调用在原线程同步执行，句柄在外层调用返回后失效；跨线程、同一作用域重入及回调错误均返回错误；空回调列表等价于动态调用。
- `Plugin` 接口供 RSSN 宿主使用。用 Aevia 值调用 `Rssn.register` 或 `Rssn.register_plugin` 时报告 `E0302`。Aevia 在模块加载时注册 `@op` 和 `@rule` 函数。`load_plugin(path) : String -> IO (Result<Int, String>)` 按 §13.5.2 和 §13.5.3 的条件加载本地源码文件。
- `core.callback` 标记受限回调桥可用，`core.plugin.source` 标记源码插件可用。生成的高阶绑定保留显式 `(values, callbacks)` 动态接口；对声明精确、回调和结果结构均可认证的单重载，另生成钉住签名的 typed
  别名，在调用时认证回调的具体签名并按实例化结果类型验证返回值；无法证明类型或维度的签名只提供动态入口。例如 `Rssn_numerical_callback` 的 `num_least_squares` 别名：参数与残差分别具有量纲 `D`、`E`，
  残差容差用 `E`，其余控制量无量纲，结果带 `converged` 与 `status`（残差/梯度收敛、预算耗尽、停滞）。

`Rssn.dynamic` 是**正式能力**，不是降级路径：插件算子、叶子算子、实验性入口都走它。多注册时 rssn 在运行时按 `Exact > Widened > Shaped > Generic > No` 选择，并列报 `E1002`；参数不被任何 overload 接受报
`OpError::Type`（`check_args`）。标准集每个名字只有 1 个注册，所以这条路径不做注册级评分，但会做签名级校验。`Rssn.call_key` 则不解析，直接执行 name + `signature_hash` 指定的 overload。

### 8.19.1 数值域模块 `Rssn_Numeric`

rssn 的 `numeric` 规则集把一批数值内核做成算子（表驱动，只对字面数字求值；符号参数、
参差列表、被拒绝的参数、不收敛都让调用保持为请求，包装器报告 `Err(NotReduced(..))`）。
Aevia 把它们归入 `Rssn_Numeric`，其中每个算子都有手写的 typed 签名（`domains.overlay` 的 `sig`，一律 `Result<_, OpError>`；规则见 §10.3.8）。
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
- **维度无关，按维数/秩命名。** `heat_fd`、`wave_fd`、`poisson_fd`、`advect_fd` 在内核里不分维度：空间维数就是场数据的嵌套深度（列表一维，列表的列表二维，三层三维；定义域是单位区间/方块/立方体）。
  一个 Aevia 签名只有一个字面量秩，所以一个内核算子带**多条签名，各用自己的名字**：不带后缀的名字是一维形式（`poisson_fd(f: Tensor<1, Float>, boundary: Float)`），`_2d`/`_3d` 取
  `Tensor<2, Float>`/`Tensor<3, Float>` 场（`heat_fd_2d`、`wave_fd_3d`、`advect_fd_3d` …），答案与场同形状；平流速度仍是 `Tensor<1, Float>`，每个空间维一个分量。秩多态的张量算子同理按秩命名：
  `tensor_norm_<r>`、`tensor_inner_<r>`（r = 1..4）与 `tensordot_<ra>_<rb>_<k>`（收缩 k 个轴，`axes_a`/`axes_b: Tensor<1, Int>` 从 0 起，结果秩 ra + rb − 2k，0 时为 `Float`；结果秩至多 4）。
  分解 `hosvd_<r>`、`hooi_<r>`、`cp_als_<r>`、`tt_svd_<r>`、`tt_dense_<r>`（r = 2、3、4）的结果是记录 `Hosvd<r>`、`Hooi<r>`、`CpFit`、`TensorTrain`（因子表是 `Tensor<1, Tensor<2, Float>>`），
  混合列表由列表访问器 `from_value<Tensor<1, Value>>` 拆开再逐部分解码（§10.3.8）。符号 PDE 只有一个名字：`solve_heat_equation`、`solve_wave_equation`、`solve_laplace_equation`、
  `solve_poisson_equation`（维数由方程的变量决定，旧的 `_1d`/`_2d`/`_3d` 是别名）；仿射变换 `translation`、`scaling`、`shear`、`reflection`、`rotation` 同理，归入 `Rssn_Graphics`。
- 参差输入（邻接表、项数不同的多项式方程）没有字面量写法，用 `map(f, range(0, n))` 构造
  `Tensor<1, Tensor<1, Float>>`。

端到端示例与封闭形式核对：`tests/domain_numeric.aev`（`mad([1,2,3,4,100]) = 1`、`ifft(fft(x)) = x`、小波往返、`poisson_fd` 的 `-u'' = 1` 得 `x(1-x)/2`、`heat_fd` 的 `exp(-π²t)` 衰减、
`tensordot_2_2_1` 对手算矩阵乘、`hosvd_3`/`tt_svd_3` 的重构、`ode_solve` 的 `y' = -2y` 得 `e^-2`、`lambert_w_complex(1,0,0) = 0.5671…` 等），由 `cargo test -p aevia-rssn --test aev_suites` 运行。

### 8.19.2 原生数值模型

以下入口由清单生成；它们调用 RSSN 的关联方法或明确的泛型实例，不在 Aevia 中实现数值算法。
`D`、`E` 表示独立的元素量纲，`X` 表示坐标量纲。

| 入口族 | 值与量纲契约 |
|---|---|
| `Rssn_numerical_matrix.num_matrix_{float,real}_*` | `Matrix<f64>` / 精确有理数实例；记录携带格式版本、后端与元素。`real` 拒绝近似 `Float`，只用 Native 后端；`decompose_*` 使用 Float 实例。`get_mut`、后端更新返回新模型。 |
| `num_dense_lu_model_*`、`num_sparse_{lu,ilu,ic,auto}_model_*`、`num_csr_model_*` | 保存定义矩阵与因子种类，调用时重建原生因子；`A x = b` 与预条件应用返回 `E/D`。CSR 下标、置换和计数是 `Int`，不是有量纲元素。 |
| `Rssn_numerical_vector.num_polynomial_model_*` | 系数按降幂排列，具有同一输出量纲；在 `x/coordinate_scale` 上求值。导数与积分分别恢复 `D/X`、`D*X`；二元运算要求相同的物理坐标尺度。 |
| `num_native_cubic_spline_*`、`num_native_pchip_*`、`num_native_cheb_*`、`num_native_aaa_*` | 坐标与输出量纲独立；第 `k` 阶导数为 `D/X^k`，积分为 `D*X`，根具有坐标量纲。 |
| `Rssn_numerical_callback.num_native_cheb_{from_fn,adaptive}_typed` | 已认证的串行回调构造同一 Chebyshev 模型；显式次数、求值预算、非有限值与原始回调错误均受检查。 |
| `Rssn_numerical_matrix.num_backend_*` | 会话持有后端、编译程序和标量句柄；各坐标按正的物理参考值归一化，再乘输出参考值。解释器编译有明确工作界限；不支持的后端报错。进程默认后端变更须显式选择。 |
| `num_ad_*` / `NativeAdVar` | 会话持有反向 AD tape、变量与梯度；异质量纲坐标使用显式参考尺度，输出量纲为 `E`、输入量纲为 `D` 时梯度为 `E/D`。 |

模型检查格式/版本、字段、形状、下标、有限数据与分配界限；无效定义在调用原生方法前失败。可移植定义不含进程内指针，重复查询可有重建成本。
行列式返回 `normalized`、`entry_unit`、`order`，表示 `normalized * entry_unit^order`；打包 LU 在规范输入单位下存储混合因子，不能把整个打包数组解释为同一物理量纲。
会话句柄检查种类、所属会话、存活状态与配额；释放有依赖的对象时先释放依赖句柄，可移植定义不能与会话句柄互换。可运行断言见 `examples/native_algebra_models.aev`；完整入口与当前限制以生成的清单签名和描述为准。

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
只引入 `sqrt`，`open Units.SI` 把 `Units.SI` 的导出名放进当前作用域。本地文件优先于嵌入标准库模块。
`Units/*` 是文档性模块：单位名本来就是 ambient 的 `Unit[d]` 值，导出数量绑定会遮蔽它们并改变 `Tensor / Unit` 的语义，因此不导出（§5.3.4）。`Sde` 模块再导出
§8.9.1 的五个入口并声明选项 ADT；`Physics.Ising`/`Physics.Geodesic` 提供 §8.17 的类型化包装器。函数名、`Sde` 的名义变体构造器与透明类型别名（`type X = { … }`，如 `Physics.Ising` 的 `Params`）
都可经 `import { X }`/`open` 引入。

`Rssn` 是**生成的模块**，不是手写库：构建时从 Kernel Manifest 生成
`Rssn.<op>` 类型化声明和 `Rssn.dynamic` 动态入口（§13.2）。`Rssn` 的
内容随 RSSN 版本变化，Aevia 源码不硬编码算子清单。`Sparse` 提供
`Csr`/`Csc`/`Coo`/`Block`/`Bcsr` 格式标签与 §8.4.1 的稀疏内建函数。布局分派见 §13.9.5。
`Device` 提供 `Cpu`/`Gpu`/`Distributed` 与
`on`/`to_device`（§13.9.4，无 `core.gpu` 时 `E1001`）。

下一章定义运行时：从源码到值，从值到 rssn 项，从 REPL 到 JIT。


<a id="ch-09-runtime"></a>

# 09 — 运行时

运行时把源码变成值，把值变成 rssn 项，把项变成结果。目标：**启动快、交互快、数值热点快、
符号操作可控**。本章定义组件、数据流、线程模型和性能边界。

## 9.1 组件

| crate | 职责 |
|---|---|
| `aevia-dim` | 量纲代数、单位表、单位制；不依赖解析器与运行时 |
| `aevia-syntax` | 词法、布局、解析、AST、诊断渲染 |
| `aevia-types` | kind、类型、量纲统一、推断、类型类、穷尽性、`.aevi` 持久化 |
| `aevia-ir` | Core IR 与从已检查 AST 的降级；只含数据，不依赖 `aevia-core` |
| `aevia-rssn` | rssn 桥：`Session`、两套值模型的转换、Kernel Manifest、`Rssn` 模块生成、JIT 透传 |
| `aevia-std` / `aevia-stdlib-model` | 标准库源码与 RSSN 适配 / 标准库生成的事实来源 |
| `aevia-core` | 值、张量、闭包、IO、解释器、内建函数、符号层、模块加载器（`driver`/`manifest`）、磁盘缓存、并行、可选 JIT |
| `aevia-fmt` / `aevia-doc` | 格式化器 / `##` 文档与 doctest 抽取 |
| `aevia-analysis` / `aevia-lsp` | 与编辑器无关的语义查询 / LSP 适配层（唯一知道 JSON-RPC 的 crate） |
| `aevia-resolve` | 版本需求与依赖解析（§7.5.1.2），纯 crate |
| `aevia-cli` | `aevia` 可执行文件 |

依赖方向：`dim → syntax → rssn → types → ir → core`；`cli`/`lsp` 依赖其上全部。`types` 的量纲代数是纯
Rust，不需要 rssn 的运行时；`core` 通过 `aevia-rssn` 使用后端。

## 9.2 编译流水线

```
源码 .aev → 词法 + 布局 → Token 流（含 INDENT/DEDENT） → 语法分析 → AST
  → 名字解析（模块路径、单位解析） → 类型/量纲检查、类解析、穷尽性 → 已检查 AST
  → 降级 → Core IR（ANF） → 解释 / 可选 JIT → 值
```

### 9.2.1 AST

AST 保留源位置，用于格式化器和 LSP；去糖（`|>`、`√`、`!`、`@`、`_` 占位符、`do`、记录更新、张量字面量、
索引）在降级前完成。单位字面量在检查时解析：名字 → 单位表 → 尺度/量纲/仿射标志。

### 9.2.2 Core IR

```
core ::= let x = core in core | lambda(params, core) | app(core, [core])
       | if(core, core, core) | match(core, [arm]) | prim(op, [core])
       | construct(tag, [core]) | project(core, field) | update(core, [(field, core)])
       | tensor_lit([core]) | index(core, [index]) | slice(core, [slice])
       | do([stmt]) | return(core) | literal(value) | var(x)
```

Core IR 是 ANF：每个中间结果绑定到名字，便于闭包转换、JIT 和调试。模式匹配保留，解释器按标签/字段
匹配。

### 9.2.3 类型擦除与信任边界

量纲和类型在 Core IR 中擦除，但保留**运行时见证**：`Real`/`Float` 的可选显示单位、`Point` 的 `is_point`
标志、`Dyn` 的运行时量纲、张量的形状与存储类型。擦除让 `mean<L>` 与 `mean<T>` 共享代码；见证只用于
显示、`Dyn` 和 FFI 检查。

**信任边界（规范性）。** 运行时执行检查器的证明，而不重新证明它：

1. **已检查的源调用不重新验证实参。** 若被调函数的激活证据是*闭合*的（签名与全部 Type/Rep/Dim/Nat/Row
   投影都不含变量），运行时直接使用，不遍历实参、不重新统一签名。证据不闭合时，先用检查器在该调用点
   记录的投影做替换；仍不闭合时才用实参的运行时见证求解剩余 binder（例如只有运行时才知道的张量长度）。
   数据只用于求解静态证据留下的未知量，从不用于复核已闭合的签名。
2. **已检查的构造器应用不遍历载荷。** 饱和的 `Cons(x, acc)` 在 Core IR 中是原语 `Construct`；名义身份在
   降级时确定，类型见证取该位置的已检查类型（按激活证据替换、按位置记忆）。
3. **边界处完整验证。** 值来源不明处仍验证：宿主（嵌入者、内核回调、`Rssn` 动态 ABI）直接调用 Aevia 闭包的
   实参、`?` 传播的错误载荷、`from_value`/IO 解码的结果。`map`、`fold` 等内建函数调用回调属于宿主调用，
   按第 1 条“不闭合”路径处理。
4. **字面量是常量。** 数值字面量与单位字面量的值只取决于位置与激活证据，按（位置、激活证据、模块）记忆；
   不在每次求值时重新解析单位表达式。

这些规则不改变任何程序的结果，只改变工作量：一个含量纲记录的 10^5 步模拟从 22.7 s 降到 0.9 s，构造并遍历
2×10^4 元素的列表从 100 s 以上降到 0.3 s（优化构建）。

已检查的闭包经词法作用域保留其检查时的 `ModuleEvidence`（表达式类型、字面量表示、量纲检查、RSSN 调用键、
字典、构造器引用、单位表）；相同源码的后续检查不改写旧闭包的证据，不同解释器会话（含 REPL `:reset`）的已
检查闭包以 `E0809` 拒绝执行。显式泛型选型携带实际声明的 origin 和按声明顺序排列的 Type/Rep/Dim/Nat/Row
投影（phantom 参数也保留）；无法证明的高阶值流保持保守，这不是任意 rank-N 多态的来源分析。原生自尾调用进入
前验证 global 仍指向同一 canonical 闭包并逐一比较五种具体投影，不满足时回到解释器 trampoline。

动态 ABI 容器：检查后的 `Tensor<1, Value>` 保持一维 boxed 元素，不因元素恰为张量而堆叠；
`[to_value(a), to_value(b)]` 始终携带两个 ABI 参数。

**见证表。** 类型类分派只读取下列见证；没有见证的类型不能用于量化的运行时分派（检查期拒绝，§3.8.4/§7.4）：

| 类型形状 | 运行时见证 |
|---|---|
| `Int` / `Real` / `Float` / `Complex` 标量 | `NumKind`（整数/精确/浮点/复数） |
| `Tensor<n, τ>` | rank + 存储类（`Exact`/`Float`/`Complex`） |
| 名义类型 `T<...>` | `(定义模块, 名字)` + 参数见证递归 |
| 记录 | 规范标签集合（排序后） |
| `Bool` / `String` / `()` / `Prop` | 各自的标签 |
| `Point` / `Unit` 值 / 函数 / `Dyn` | **无见证** |

- **具体头类型**：检查器记录精确实例键（类 + 规范头），Core IR 携带该键，解释器直接执行对应实例体。
- **量化头类型**：只有候选实例的见证两两不相交时才允许运行时按见证选择，否则检查期 `E0311`/`E0310`。
  见证不是全的：`Tensor<1, Int>` 与 `Tensor<1, Real>` 的存储类相同，两个实例不能共存。
- 记录见证是**结构**的：同标签集合的记录类型不可区分，不能各写一个实例；名义类型以模块+名字区分。
- 没有运行时多方法分派，也没有按类型/量纲的单态化：`mean<L>` 与 `mean<T>` MUST 是同一份代码（§4.4.4、
  §12.8.13）。表示专用化是按操作、带标签守卫的快速路径。

## 9.3 解释器

### 9.3.1 环境

局部环境是不可变的持久帧（`Rc`），闭包捕获环境帧；全局环境含模块导出、Prelude、单位表、类实例字典。
递归闭包经作用域绑定解析，使用 `Rc` 环境，**没有**会话级循环回收器；独占 Scope parent 链迭代释放，
不承诺回收任意闭包循环。

### 9.3.2 调用

- 参数从左到右求值；内建函数是 Rust 函数指针 + arity，用户函数是闭包。
- 自尾调用用 trampoline（`app` 在尾位置返回 `TailCall`，主循环替换当前帧），不增长 Rust 栈。
- 非尾递归的界限见 §6.9.3（`E0805`）。

IO 调度使用显式 work 栈；Core IR 的 `do` 是一个共享槽帧及顺序步骤表，不为每段后缀创建闭包。平坦语句序列
及最后动作的 IO 尾递归不按语句数积累调度续延。`Interp::max_io_steps`（默认 100,000,000）限制调度步骤，
`max_io_depth`（库默认 2048，CLI 放宽为 10⁶）限制待执行的续延，超限 `E0805`；每次 `run_io` 重置计数。
CLI `--budget` 同时设置尾调用与 IO 步骤预算，不是时间或内存上限。

`?` 的控制流标记带检查器认证的真实函数/IO 边界；失败传播在目标成功类型下重新构造 `Err`/`None`，普通运行时
错误不会被函数边界误当作成功返回。延迟动作保留其定义时的检查证据，跨会话或 reset 后执行旧动作报 `E0809`。
Prelude 的 `ParseError`、`IOError` 等内建抽象错误仍用文本 payload，`?` 仍可传播它们，但它们跨 opaque
`Value` 往返会被安全拒绝；源码同名 ADT 不适用此兼容。

### 9.3.3 模式匹配

构造器比较 tag 并递归匹配 payload；记录按字段名开放匹配；张量 rank 1 按长度/元素匹配，`..rest` 生成
`SharedSlice`（所有存储表示均可共享，连续 tail 操作指向同一原始 tensor，不形成递归 view 链）；字面量精确
比较；守卫匹配后求值，必须是具体 `Bool`。

AST 与 IR 两条路径共享显式 work 栈和暂存绑定表：结构检查全部成功后才提交绑定，失败不修改已有 scope。
构造器证据随词法模块 revision 保留，并验证声明 schema、序号、名称及字段数。逐元素尾递归消费张量的存储
工作为线性。静态穷尽矩阵最多处理 8192 个状态、128 层展开，耗尽预算时保守报告 `E0309` 并建议 `_`。

### 9.3.4 错误与栈

每个 Core IR 节点带源位置。错误对象：

```
EvalError {
    code: E08xx,
    message: String,
    span: Span,
    stack: Vec<Frame>,   # 函数名 + 调用点
    notes: Vec<String>,
}
```

`panic` 打印栈；`Result` 错误不打印，由 `?` 传播。栈与尾调用预算见 §6.9.3。

## 9.4 符号桥

### 9.4.1 Session 与 Term

rssn 的 `Session` 拥有项图，`Term<'s>` 是带生命周期的句柄；`Session` 内部是 `RefCell`，不是 `Sync`。策略：

- **每个线程一个 `Session`**（`thread_local!` 或工作线程池）；跨调用的项用 `u32` id 保存，`TermId` 含
  `session_id` 以防跨会话误用。
- `compute` 会克隆模板图，对重型请求可接受，对每个 `+` 不可接受，所以算术只构造项（`Session::call_value`），
  不调用 `compute`（§6.3.3）。

### 9.4.2 符号表

```
SymbolicCtx {
    session: Session,
    symbols: HashMap<SymbolKey, TermId>,   # 用户符号 -> 项
    display: HashMap<TermId, String>,      # 显示名
    config_cache: HashMap<ConfigKey, Config>,
    term_cache: LruCache<TermHash, TermId>,
}
```

- `symbol("x")` 生成内部唯一名 `x#n`。静态路径的符号无量纲；只有 FFI/导出才用全名传给
  `Config::dimension`，`Dyn<ρ>` 的量纲由宿主保存。
- `Config` 按（规则集、假设、量纲模式、预算）缓存；`term_cache` 用结构哈希去重。

### 9.4.3 请求构造

| Aevia | rssn 请求 |
|---|---|
| `diff(f)` | `diff(term, ξ)` |
| `antiderivative(f)` | `integral(term, ξ)` |
| `defint(f,a,b)` | `defint(term, ξ, a, b)` |
| `limit(f,a)` | `limit(term, ξ, a)` |
| `solve(f)` | `solve(term, ξ)` |
| `sum(f,a,b)` / `product` | `sum(term, κ, a, b)` / `product(term, κ, a, b)` |
| `series` | `series(term, ξ, a, n)` |
| `dsolve` | `dsolve(eq, y(t))` |
| `simplify` | `compute(term, Config::symbolic())` |
| `approx` | `compute(term, Config::numeric(tol).bind(...))` |

`ξ`/`κ` 是新鲜符号，`a`/`b` 是已求值的项或数值。`compute` 返回 `Answer`；`Answer.term` 是答案项，
`Answer.dimension` 是 rssn 推断的量纲。

### 9.4.4 量纲校验

每次 `compute` 后：

1. 静态路径：项图无量纲，答案量纲由检查器已知，不做运行时比较。
2. 数值路径：以 `Answer.dimension` 为准（**不要**从 `Answer.term` 重新推断：Numeric 目标的 term 是无量纲
   浮点字面量）；为 `None`（`DimensionMode::Off`）时回退静态量纲。
3. 开启可选诊断且 `Answer.dimension` 与静态期望不一致，报 `E0809` 并打印请求、答案、两边量纲。
4. 把 `Answer.term` 转回 Aevia 值。

对 `Dyn<ρ>`，不比较静态量纲，以宿主保存的 `DimTerm` 作为运行时量纲。

### 9.4.5 数值路径

- `Float` 标量四则直接走 f64，不进入 rssn；`sin`/`exp` 等走 Rust `libm` 或 rssn 内核。
- `Real` 的 `approx` 走 `Target::Numeric`，结果带误差球，Aevia 取中点。
- 张量数值算法走 RSSN；`Exact`/`Boxed` 通过宿主标量回调保持语言表示与量纲。

### 9.4.6 数值实现边界

RSSN 拥有数值算法，包括精确整数/有理数、复数、张量收缩、稠密/稀疏分解、统计、插值与微分方程步进。
`aevia-std` 只验证形状/量纲、适配回调、映射错误并保持语言值表示。运行时保留标量语言运算和容器遍历；
数值迭代与收缩调度在 RSSN 执行，必要时使用宿主提供的标量回调。

数值后端依赖属于 RSSN；Aevia 的独立数值库仅可作一致性测试的开发依赖。清单绑定与公开 Rust API 的覆盖分别审计（§8.19），
本边界不表示所有原生功能都已暴露。

## 9.5 张量运行时

- 存储：`Arc<TensorData>`；切片是视图（offset + strides），`reshape` 连续时是视图否则复制。写入不存在：
  `update`/`map` 产生新张量，唯一持有时原地复用。语义见 §6.6。
- `matmul`、分解、FFT、插值与数值归约由 RSSN 执行；后端库（如 faer、rustfft）封装在 RSSN。
- 形状检查在每次运算前做，错误含两边形状与运算名；细化形状在编译期检查，秩-only 在运行时检查。
- 设备：张量带 `device` 元数据；`to_device`/`on(device, ...)` 负责传输与作用域执行。CPU 是默认，GPU/分布式是运行时
  目标，不是类型参数。
- 稀疏：`Sparse<F,shape,τ>` 与 dense 共享 `@` 分派；`Layout` 类提供 `nnz`/`to_dense`/`from_dense`/`matmul`/
  `transpose`。
- 插件：`Rssn.register`（别名 `register_plugin`）把算子/重写/单位注册进当前 RSSN 会话，Aevia 通过 manifest 反射
  看到，无需重新编译。

## 9.6 JIT

### 9.6.1 触发条件与门控

先读 manifest 的 `Features`：

- `features.jit = false`：`@jit` 报 `E1001("core.jit")`，函数只解释执行；
- `features.jit_branch = false`：含 `if`/`match` 的函数不 JIT；
- `features.jit_index = false`：含张量索引的函数不 JIT。

满足门控后，还需：函数只含 `Float` 标量、算术、初等函数（按 flag 允许 `if`/索引）；被调用超过阈值（默认 1000 次）
或显式 `@jit`；输入/输出类型固定（无符号、无 `Dyn`、无记录/变体）。

### 9.6.2 流程

1. 把 Core IR 降成 rssn 的 LIR（`rssn::backend::lir`）：带类型的寄存器机，有基本块、块参数、直接（可递归）
   调用、带检查的整数运算、有界检查的切片和 *bail* 出口。每个激活（参数种类 + 结果种类 + 字面量特化）生成两个
   LIR 函数：入口形式返回 `(状态, 结果, 下一组参数, 剩余燃料)`，值形式只返回结果，供值位置的递归调用。
2. 仅 `features.jit` 为真时由 Cranelift 编译（`rssn::backend::lir_jit`）。
3. 缓存键：函数身份 + 参数种类 + 结果种类 + 字面量特化。
4. 进入机器码前逐一检查：每个自调用点仍指向本激活，所用内建函数名仍解析到内建（未被遮蔽）。bail 时解释器
   从相同参数重新执行（机器码是纯的，重做不可观察）。

### 9.6.3 语义与限制

- 可编译：无捕获、参数与结果为无量纲 `Float`、`Bool` 或 64 位 `Int` 的纯函数；`if`/`match`（字面量、通配、绑定、
  守卫）真正分支；自尾调用编译为带燃料检查的跳转，燃料耗尽时把蹦床交还解释器；值位置的自调用编译为原生递归
  （深度上限 512，超出 bail）。
- `Int` 的 `+ - *` 带溢出检查，溢出 bail 并由解释器以任意精度完成；`Int` 与 `Float` 混合按 §6.3.2 提升。
- 初等函数（`sqrt`、`cbrt`、`exp`、`ln`/`log`、`log2`、`log10`、三角与双曲函数及其反函数、`√`）带守卫：凡解释器
  不会简单返回 `f(x)` 的情形（定义域错误、§6.3.4 的扩展实数特值、`±oo`、`ln(0)`）一律 bail；`sqrt`/`cbrt` 与解释器
  一样按 `powf(1/n)`，逐位一致。
- 符号值、记录、变体、闭包、张量、精确数、单位不进入 JIT。
- JIT 结果与解释器 MUST 逐位一致；不能保证一致的情形 MUST bail，不一致是 bug。
- `map_floats`（设备接缝）仍用直线 tape 降级，因为 GPU 需要无控制流的程序。

可选构建：`cargo build -p aevia-cli --features jit` 启用 RSSN 的 Cranelift 后端（需 Rust 1.100+）；默认关闭，
`@jit` 报 `E1001`。参考测试 `tests/jit_native.aev`。

### 9.6.4 设备目标

`@jit` 始终是标量、CPU、机器码路径；`Cpu`/`CpuParallel`/`Gpu(n)` 上的数据并行是另一套机制（rssn `Device`，§10.6.5），
满足 `@jit` 的函数也满足设备批量路径。

- `@jit(target = Cpu)` 等价于 `@jit`；`target = Gpu(n)` 选择**批量**设备路径，没有 `core.gpu` 时检查期
  `E1001("core.gpu")`。
- 设备路径与 JIT 共用同一降低（Core IR → SSA）和同一门控（§9.6.1）；差别只在后端：JIT 编译一个点的函数，设备
  在 `rows` 行上启动一个内核。
- `CpuParallel` 与 `Cpu` 逐位相同，线程数不改变结果（§9.7.2）；`Gpu(n)` 是 `f32`，只保证舍入误差内一致，故不进入
  纯函数、位确定的原语。
- 设备不可用、函数不可降低或设备报错：回退解释器逐行求值，值不变；`AEVIA_PARALLEL_TRACE=1` 打印回退原因。

## 9.7 内存与并发

### 9.7.1 内存

张量与闭包在各自的解释器中通过 `Rc` 持有。`f64` 切片共享底层缓冲区，模式匹配取得的张量尾部共享原张量。
记录更新在唯一持有时复用存储。闭包无循环回收（§9.3.1）。RSSN 会话对相同的符号项复用图节点，项标识只在所属会话内有效。
`:reset` 释放 REPL 会话占用的存储。

### 9.7.2 并行求值：无共享副本

- 纯函数可并行；`par_map(f, xs)` 的结果顺序确定，**线程数不改变任何结果**。
- **无共享副本（replica）设计。** 解释器建立在 `Rc` 上，把运行时改成 `Arc` 会让每个程序为少数并行程序付费。
  因此每个工作线程持有一个**副本**：宿主安装的 `Replica` 工厂在该线程上从同一份源码再构建一次解释器（每线程
  一次，之后复用）。线程之间交换**可移植值**（`Portable`）：带量纲与显示单位的数、字符串、张量、记录、变体、
  构造器，以及可重建的 Core IR 闭包。工作线程保留构造器特化、变体类型和张量元素类型，包括空张量的类型。
- **闭包按代码身份 + 捕获数据跨线程。** lambda 是“函数的源码位置 + 捕获值”；位置以**模块身份**（逻辑源码路径 +
  所属包）加字节偏移表示（`FileId` 按加载分配，不可跨线程），副本里由同名模块解析回同一函数。
  显式特化的泛型闭包携带已选择的类型和分派依据，接收方根据其模块接口校验。
- **保证：并行从不改变值。** 值不能跨线程（符号项可能持有本线程会话句柄、kernel 句柄、IO 动作、带激活期泛型
  证据的闭包）、没有副本工厂、`--jobs 1`，或工作线程失败时，原语回退为顺序求值，顺序重算给出程序本来会给出的
  值或错误（含位置与调用栈）；`AEVIA_PARALLEL_TRACE=1` 打印回退原因。
- **确定性。** 映射批大小随工作线程数调整，结果保留输入顺序。`par_reduce(f, zero, xs)` 要求 `f` 结合且 `zero` 为单位元，
  按固定 1024 元素块折叠再顺序合并，浮点归约在任何核数及顺序回退中**逐位相同**。
- `par_map`、`par_tabulate(n, f)`、`par_reduce` 的工作线程数依次取自 `--jobs N`、`AEVIA_JOBS`、
  `RAYON_NUM_THREADS` 或当前进程可用的 CPU 数。每个工作线程有一个内核 `Session`。嵌套调用语言并行原语时在
  当前工作线程顺序执行，该线程上的 RSSN 算子使用外层 Rayon 池。主解释器线程上的 RSSN 使用全局 Rayon 池，
  由 `RAYON_NUM_THREADS` 和主机 CPU 探测决定大小。
- **数值内核的并行。** `sparse @ dense`（含 `Bcsr`）按固定 256 行块并行，每个输出行由一个工作项顺序累加，逐位等于
  顺序内核；`sde_ensemble_diag/general` 经 `par_map` 副本执行，成员种子 `ensemble_seed(seed, i)` 只依赖 `(seed, i)`。
  有意不并行：ODE/SDE 单条路径的时间步、Krylov 迭代、`tabulate`。含闭包字段的值（`ode_solve` 的 `at`）不可跨线程，
  回退顺序。

### 9.7.3 并发任务

```
spawn : (() -> IO a) -> IO (Task a)
await : Task a -> IO a
pure  : a -> IO a
```

- `spawn(f)` 在工作线程上运行 `f()`，返回 `Task`；`await(t)` 等待并取结果。任务只拥有提交时已启用且工作线程策略允许的权限。
  在工作线程启动前取消的任务，之后在 `await` 处报告 `E0812`。
- **任务的控制台输出被缓冲，在 `await` 时按 await 顺序写出**，所以程序输出确定；文件等其他效应在任务线程上直接
  发生。
- thunk 不能跨线程时任务在 `spawn` 时于本线程执行（同样缓冲），`spawn` 总是可用。工作线程上的运行期错误在
  `await` 处报告（不重跑）；任务结果须是可移植数据，否则 `await` 报 `E0811`；一个任务只能 `await` 一次
  （第二次 `E0811`）。
- `do` 本身仍顺序执行；并发只经由任务显式出现。

## 9.8 REPL

直接运行 `aevia`（无子命令）或 `aevia repl` 进入交互会话。

```console
$ aevia
Aevia 0.1.0 — dimensions are types
Enter an expression, or paste a program (including main). :help shows examples.
Tab completes names; blank line ends a block; Ctrl-C cancels; Ctrl-D exits.
aevia> let g = 9.80665 m/s^2
aevia> sqrt(2 * 20.0 m / g)
aevia> :type sqrt
sqrt : forall t0, d0. Scalar<ρ0, d0> -> Scalar<ρ0, d0^(1/2)>
```

| 命令 | 作用 |
|---|---|
| `:help [topic]` | 指南与示例；主题包括 `paste`、`main`、`units`、`files` 和命令名（`:h`、`:?`） |
| `:type e` | 只检查类型，不求值（`:t`） |
| `:bindings` | 列出用户绑定与类型（`:vars`） |
| `:show` / `:save FILE` | 显示已接受的声明 / 保存到新文件，绝不覆盖已有文件 |
| `:dims` | 列出基量纲（`Length`、`Mass`、`Time`、`Current`、`Temperature`、`Amount`、`Luminosity`） |
| `:units [d]` | 列出单位，可限定合法量纲（如 `L`、`L/T`） |
| `:ast e` / `:core e` | 不求值，显示 AST / Core IR |
| `:time e` | 求值一次、执行 IO 并报告耗时 |
| `:latex e` / `:typst e` | 求值一次并输出 LaTeX / Typst 数学源码 |
| `:pretty e` / `:pprint e` | 求值一次并绘制二维终端数学排版 |
| `:repr e` / `:print e` | 源码表示 / 人类可读文本 |
| `:format [NAME]` | 选择或查看普通结果和 `:time` 的 show、repr、pretty、latex、typst 格式；`:reset` 后保留 |
| `:doc ITEM` / `:help ITEM` | 已接受的本地/导入条目、内建函数、类型和单位的签名及文档（`:?`、`:h`） |
| `:apropos TEXT` | 搜索可用条目的名称及文档 |
| `:source [N]` | 列出原始输入，或查看 `<repl #N>` 对应的源码快照 |
| `:imports` | 列出已导入的模块与别名 |
| `:load FILE` | 按文件所属包/目录解析导入，追加声明并执行新 `main`（`:l`） |
| `:run` | 再次执行已加载的 `main` |
| `:paste` / `:end` | 显式收集多行输入；`:cancel` 丢弃该输入 |
| `:reset` | 清空会话（开始新的会话身份，§7.3） |
| `:quit` | 退出（`:q` 或在提示符处 Ctrl-D） |

终端编辑器支持光标编辑、内存历史、Tab 补全、括号匹配及括号粘贴。粘贴文本保留在可编辑缓冲区中，按 Enter
提交。手工输入缩进块时，以空行结束；尚未闭合的分隔符或运算符请求续行。Ctrl-C 取消当前输入，包括显式
粘贴；Ctrl-C 也可中断当前求值或前台程序输入并返回提示符。历史不写入磁盘，程序输入不加入 REPL 历史。

同一提交中的连续声明整体检查后再初始化，支持前向函数引用及含 `main` 的完整程序；只初始化新增声明，
新提交的 `main` 自动执行一次。之后的表达式不重复执行旧声明或 `main`。表达式显示类型与值，IO 表达式立即
执行。声明按输入顺序累积，重复定义遮蔽旧定义。复检累计源码后安装完整的检查证据，保持已执行声明的游标；
新增类型的构造器使用该次检查的确切见证。编译与初始化错误均保留旧绑定；一批声明初始化中途失败时，
回滚该批全部新绑定、实例注册、符号分配与随机状态，不执行剩余初始化。`main` 内的运行时错误发生在声明
加载之后，因此已加载绑定仍可用于检查与 `:run`。

管道输入（`aevia < session.aev`）不输出横幅或提示符，连续声明整体处理，顶层表达式分别执行；任一提交失败时
退出码非零。`:load` 与粘贴支持正常的包解析器：提示符处使用当前目录所属包及其清单依赖，加载文件的新导入使用该文件所属包/目录。
别名、open/选择性导入、传递依赖、内置标准库与名义类型均遵循正常程序语义。成功初始化的依赖只运行一次，
源码快照与身份保留至 `:reset`。失败或中断的声明/导入批次回滚绑定与初始化状态；此前未接受的模块修复后可重试。
导入解析目标改变时给出 `:reset` 提示，避免旧运行时值与新名义类型混用。`:imports` 列出活动模块与别名，Tab 补全限定名称。
CLI 与 REPL 共同遵循
`--color auto|always|never`：自动模式分别检查输出流，重定向、`NO_COLOR`、`TERM=dumb` 时禁用颜色。
用户程序输出与 LSP 协议字节保持原样。

中断使用线程安全的取消令牌并报告 `E0812`。AST、Core IR、回调、IO 调度、任务等待、并行工作线程及支持的
内核图/数值循环在检查点停止并正常展开作用域。被取消的工作线程不会因下一次输入清除前台令牌而复活；
先前成功提交产生的独立后台任务不受后来求值的取消影响。中断 await 会取消正在等待的任务。REPL 后台工作线程
不能抢占交互终端，应在前台读取输入后再启动工作。

取消是协作式的：没有检查点的编译器工作、外部数值库操作或系统调用可能先完成当前调用，再报告中断；
已完成的效果不会撤销。被取消的内核结果不会缓存或作为成功数学结果返回。REPL 安装取消令牌期间禁用可选 JIT
快速路径以保留检查点；独立 `aevia run` 保持操作系统默认的信号行为。

条目帮助使用已接受声明的 `##` 文档、推导签名及已冻结的导入源码，不执行表达式。`:help :COMMAND` 显式查询命令帮助；`:apropos TEXT` 搜索条目。`:format show|repr|pretty|latex|typst` 控制普通结果和 `:time`，在 `:reset` 后保留；渲染命令对表达式和 IO 仅执行一次。

REPL 诊断显示原始输入，不显示生成的表达式绑定。当前输入按提交内行号定位，旧定义标记为 `<repl #N>`，
`:load` 保留文件名与行号；`:source [N]` 查看对应快照。语法恢复保留独立错误；REPL 不检查语法损坏模块及其依赖者的类型，
也不重复已接受源码的警告。单独的 `:cancel` 可丢弃未完成的管道输入（字符串/注释内除外）。

## 9.9 CLI

```
aevia run [--example NAME] [file.aev] [-- args…]   运行包入口/文件/示例（`--` 之后是程序参数）
aevia check [--all] [-A|-W|-D|-F LINT] [file.aev…]  类型/量纲检查，不运行
aevia lint [--all] [--fix] [--format human|json|sarif] [-A|-W|-D|-F LINT] [file.aev…]
                                                   运行编译器/正确性/风格/pedantic lint（支持 `--list`、`--explain`）
aevia watch [file] [--once | --runs N]             反复检查，只重检变化的模块（§9.11.1）
aevia test [--filter PAT] [file.aev]               运行 `test` 声明
aevia test --doc [--filter PAT] [files…]           编译 `##` 文档中的 aevia 代码块（§9.12）
aevia repl                                         交互会话（§9.8）
aevia fmt [--check] [files…]                       格式化，默认包的 src/
aevia doc [--open] [--out DIR] [--json] [files…]   生成包的 API 文档，默认 docs/
aevia doc --stdlib [--lang en|zh] [--out DIR] [--json]  生成标准库参考
aevia bench [--filter PAT] [--iterations N] [--warmup N] [--save-baseline FILE]
            [--compare-baseline FILE] [--threshold RATIO] [files…]
aevia init <name>                                  新建包：aevia.toml、src/main.aev、aevia.lock
aevia add <path|owner/name[@req]> [--as NAME] [--offline]   添加路径或 registry 依赖并重新解析锁
aevia update [owner/name…] [--offline]             重新解析 aevia.lock 到最新兼容版本
aevia fetch [--offline] [--force]                  下载并校验 aevia.lock 指名的 registry 包
aevia tree [--offline]                             打印依赖树
aevia clean [--registry]                           删除 `.aevia/`（`.aevia/registry` 除非指定否则保留）
aevia lsp [--stdio]                                语言服务器（§9.10）
aevia debug [--stdio]                             为编辑器提供源码调试适配器
aevia rssn [--audit] [--emit [--out DIR] [--namespaces]]   审计 Kernel Manifest / 生成 Rssn 模块（第 13 章）
aevia login                                        用浏览器把签名密钥绑定到 registry 账户
aevia publish [--owner HANDLE] [--no-verify] [--dry-run] [选项…]  检查、签名并上传本包
aevia search <query>      aevia info <owner/name>  搜索 registry / 显示包页面
aevia yank <owner/name@version> [--undo]           撤回或恢复一个版本
aevia verify <owner/name@version>                  对照签名校验下载的版本
aevia docs [owner/name[@version]] [--lang en|zh] [--open]
aevia docs --section book|architecture|spec|stdlib [--lang en|zh] [--open] [--download]
aevia pkg <command>                                注册表客户端的全部命令（`key`、`status`、`lock`、…），转发给 aevia-pkg
aevia --version
```

可用 `aevia --help`、`aevia COMMAND --help` 或 `aevia help COMMAND` 查看用法、选项与示例；
帮助不会加载或修改包。

全局 flag（程序参数分隔符 `--` 之前）：

| flag | 作用 |
|---|---|
| `--max-stack MiB` | 解释器线程的栈（默认 1024） |
| `--budget STEPS` | 尾调用/IO 步骤预算，超出 `E0805`（默认 100,000,000；不是秒，§9.3.4） |
| `--caps LIST` | 只授予这些 capability：`fs,net,env,rand,gpu`（空列表 = 全部沙箱；默认全部，§9.14） |
| `--jobs N` | 并行求值的工作线程数（§9.7.2） |
| `--no-cache` | `check`/`run` 既不读也不写磁盘缓存（§9.11） |
| `--out PATH` | 输出路径（`rssn --emit`、`doc`） |
| `--color auto\|always\|never` | CLI 与 REPL 着色；自动模式遵循终端支持与 `NO_COLOR` |
| `--locked` | 拒绝改写过期的 `aevia.lock`（`update`/`add`/…） |
| `--release` | 执行前优化已检查的 Core IR：折叠常量布尔表达式、传播局部已知布尔条件并消除不可达分支；保留运行时检查、执行预算和数值表示（JIT 见 §9.6） |

- **程序参数**：`aevia run [file] -- args…` 只把 `--` **之后**的字符串交给程序，程序用 `args()` 读取
  （§8.1.8，返回 `Tensor<1, String>`；无参数为 `[]`）。`check`/`test`/`bench` 不转发参数。
- **注册中心命令。** Aevia 将 `login`、`search`、`info`、`yank`、`verify`、`key`、`status`、`lock`
  和 `index` 转发给 `aevia-pkg`。`add`、`update`、`fetch` 和 `tree` 由工具链在本地处理依赖，
  并委托配套客户端处理网络请求。用 `AEVIA_PKG` 指定可执行文件。默认依次在 Aevia 可执行文件所在目录、
  `PATH` 和源码工作区的平台构建目录中查找。转发命令的注册中心依次取 `--registry URL`、
  `AEVIA_REGISTRY`、客户端配置和公共注册中心。
- **发布。** `aevia publish` 在调用注册中心客户端前运行 `aevia check --all` 和 `aevia fmt --check`。
  `--no-verify` 跳过本地检查。`--owner HANDLE` 选择签名者拥有发布权限的命名空间。
  `--dry-run` 准备归档并打印校验和与服务器声明，省略签名和上传。客户端选项包括 `--key FINGERPRINT`、
  `--description TEXT`、`--license SPDX`、最多五个 `--keyword K`、`--repository URL` 和
  `--allow-dirty`。发布和撤回使用 OpenPGP 或 SSH（`ssh-ed25519`）密钥对服务器声明做分离签名。
  声明包含一次性随机数，有效期为五分钟。客户端优先选择 OpenPGP 密钥，使用 SSH 时建议改用硬件保护的
  OpenPGP 密钥。
- **文档。** `aevia docs` 打印在线文档地址。可指定包及可选版本，或用 `--section` 选择教程、架构、
  规范或标准库。`--lang en|zh` 选择语言。`--download` 获取教程或架构文档的独立 HTML 地址。
  当标准输出为终端时，`--open` 打开该地址。地址前缀依次取 `--registry`、`AEVIA_REGISTRY`
  或 `https://aevia.apich.org`。`aevia doc --stdlib` 生成指定语言的本地 Markdown，`--json`
  额外生成双语文档包。
- **调试。** 编辑器通过调试适配器协议启动 `aevia debug --stdio`。启动请求指定已保存的源码文件、
  可选的程序参数和输入行。行断点和单步执行支持查看调用栈及当前可见绑定。

## 9.10 LSP

支持的能力：

- 诊断：类型、量纲、单位错误，未使用绑定，非穷尽匹配。
- 悬停：类型、量纲、单位、文档。
- 补全：模块名、函数、单位、构造器；**字段补全**在 `.` 之后只列接收者记录类型的字段；**关键字补全**只在表达式位置提供
  `do`/`let`/`match`/`with`。
- 跳转：定义、类型定义；`textDocument/references`：同一名字在已加载程序里的全部使用（尊重
  `context.includeDeclaration`；局部绑定只在本文件，顶层声明、导入/限定名、类型与构造器、本文档声明的记录字段可
  跨模块）。
- 重命名：`prepareRename` + `rename` 返回带文档版本的 `WorkspaceEdit`；可改局部绑定、顶层声明、被导入且在本程序内
  使用的名字、本文档声明的记录字段；拒绝关键字/预置名/单位名/不归本文档编辑的导入名，以及非法或冲突的新名
  （返回明确错误，不做部分编辑）。
- 签名帮助：光标在调用括号内时给出函数名、活动参数与可用重载；触发字符 `(`/`,`，重触发 `,`。限制：活动重载按
  arity 选择；参数标签显示**类型**而非参数名；文档只来自声明上方的 `##`。
- 格式化：与 `aevia fmt` 同一引擎。代码动作：添加类型标注、把 `degC` 转成 `K`、提取 `let`、修正量纲。语义高亮：
  类型、量纲、单位、算子。`workspace/symbol` 列出已加载程序全部模块的声明。

LSP 复用 `aevia-syntax`/`aevia-types`，不启动运行时。文件已保存时，分析通过 `aevia_core::driver` 加载其导入图（包根沿用
`aevia.toml`），跨模块跳转返回定义模块的文件 URI；未保存缓冲区也保留导入图（对所有打开的物理缓冲区使用同一快照），
尚未保存的导入模块也可参与解析；虚拟/无包根文档回退为单文件分析。有界 LRU 缓存（默认 4 个程序）比较实际读取的源码
内容、导入候选的存在性/物理身份与包解析上下文，不扫描无关目录，也不依赖 `(len, mtime)`。导入缓冲区变化时重新发布
受影响的已打开导入者的诊断。`initialize` 返回的精确能力：

```json
{
  "positionEncoding": "utf-16",
  "textDocumentSync": { "openClose": true, "change": 2, "save": true },
  "hoverProvider": true,
  "completionProvider": { "resolveProvider": false },
  "definitionProvider": true,
  "typeDefinitionProvider": true,
  "documentSymbolProvider": true,
  "documentFormattingProvider": true,
  "codeActionProvider": true,
  "workspaceSymbolProvider": true,
  "referencesProvider": true,
  "renameProvider": { "prepareProvider": true },
  "signatureHelpProvider": {
    "triggerCharacters": ["(", ","],
    "retriggerCharacters": [","]
  },
  "semanticTokensProvider": {
    "legend": {
      "tokenTypes": ["type", "dimension", "unit", "operator", "function", "variable", "enumMember"],
      "tokenModifiers": []
    },
    "full": true,
    "range": false
  },
  "experimental": {
    "multiModuleAnalysis": {
      "savedFiles": true,
      "unsavedBuffers": "snapshot overlays for all open physical buffers"
    },
    "analysisCache": {
      "strategy": "observed source contents + import candidates + package resolution",
      "capacity": 4
    }
  }
}
```

`aevia-lsp/tests/protocol.rs` 断言这份能力表（只声明已实现的行为）。

## 9.11 构建与包管理

- 包图拓扑排序；锁文件保证可复现（§7.5.2）；构建产物在 `.aevia/`，不提交；中心 registry 是默认源（协议见 §13.7），
  `add`/`publish`/`yank` 走同一协议。

### 9.11.1 进程内增量检查

检查一个模块是它的源码文本、`FileId`、所导入模块的**已检查接口**、导入绑定、模块身份与检查选项的函数。`CheckCache` 以
这些输入的结构化指纹为键保存 `CheckedModule` 与诊断：输入全同的模块不再检查；因为指纹含导入的已检查接口，**不改变接口
的编辑不会重查导入它的模块**（early cutoff）。`FileId` 是 `SourceMap` 中的位置，导入图不变时每次加载相同；缓存项只在同一
`FileId` 下复用，所以复用接口中的 span 都指向被检查时的文本。

`aevia watch <file> [--once|--runs N]` 用同一会话缓存反复检查，每轮报告“检查了几个、复用了几个”。标准库与未改动的包模块
每会话只检查一次；等待期间监视加载观察到的全部输入（读过的文件，以及当时不存在、出现后会改变解析的导入候选）。

### 9.11.2 跨进程整程序缓存

`aevia check <file>` 在入口旁的 `.aevia/cache/` 记录本次加载观察到的每个输入（读过的文件及其文本哈希、当时不存在的
导入候选）、编译器身份（可执行文件及其大小/修改时间）、渲染样式与结果。之后的 `check` 若全部输入仍匹配，就直接重放输出
与退出码；任一变化（编辑、新建了会被导入的模块、重建编译器、换颜色样式）都是未命中。

### 9.11.3 按模块的磁盘缓存（`.aevi`）

每个已检查模块一个文件，位于入口旁的 `.aevia/cache/modules/<模块路径>-<键的 16 位十六进制>.aevi`；标准库模块也缓存，
由 RSSN 清单即时生成的内核命名空间与含错误的检查结果不缓存。文件：魔数 `AEVI`、格式版本（`FORMAT_VERSION`，小端 u32）、
128 位键、正文长度与 FNV-1a 校验、正文。正文使用 bincode-next 3.2 原生编码，采用确定性排序与模式指纹，
内容包括入口的 `FileId`、接口摘要、诊断和完整的 `CheckedModule`。写入先写临时文件再改名；缺失、截断、校验失败、键或版本不符一律是静默未命中并被覆盖；同一模块名最多
保留 4 个修订。

- **键**是下列输入的稳定 128 位哈希（两路 FNV-1a，不是 `DefaultHasher`）：模块源码、`FileId`、模块路径与加载器给的名义
  身份（含包来源）、导入绑定、各再导出声明解析到的来源、**依赖闭包中每个模块的接口摘要**、检查选项（能力集）、编译器身份（`aevia-core` 版本、
  `FORMAT_VERSION`、RSSN 清单哈希、可执行文件大小与修改时间）。接口摘要中每个 ADT 已换成其符号定义，不含 `AdtId`。
  因此不改变接口的编辑只重查被编辑的模块，改变接口则重查全部依赖者；改能力集、导入绑定或编译器构建都是未命中。
- **`AdtId` 不进入文件（§7.3）。** 编码时每个出现的 `AdtId` 在文件内分配下标，ADT 表写下完整定义（`AdtKey` = 模块路径 +
  名字 + 声明 schema、参数、参数种类、构造器及字段类型，环用下标表达）；保留的 `Option`/`Result`/`Ordering`（0/1/2）按原值
  写。解码时先把 ADT 表**重新驻留**到本进程注册表（已有完整且 `AdtKey` 相同的类型就复用 `AdtId`，否则新建；宿主/不透明类型
  按名字 `intern`），再解码正文。
- 缓存项只在同一 `FileId` 下复用，`FileId` 同时在键和文件里。`LoadStats` 增加 `disk_hits`/`disk_writes`；入口函数是
  `driver::load_with_disk_cache` 与 `driver::load_incremental_with_disk_cache`（`DiskCache::beside(entry)`）。

缓存保存完整的已检查模块、单位表和量纲环境。计算缓存键会读取依赖闭包中的接口。
CLI 的 `check` 和 `run` 使用此缓存，按模块名保留至多四个修订。

## 9.12 测试与基准

- 单元测试：`test` 声明；`aevia test [--filter PAT]`。用 `forall` 在固定数值样本上检验命题（§8.13）。
- Doctest：`aevia test --doc [FILE…/DIRS…]` 抽取 `##` 注释中所有标 `aevia` 的围栏块并逐个编译。块首行可写
  `compile_fail`（要求编译失败）或 `compile_fail EXXXX`（要求失败且含该代码）；不写标记要求编译通过。每个片段是独立合成
  模块：声明模块的 `import`/`open` 行被复制，导入按声明文件的包与相邻目录解析，模块自身声明不隐式进入片段；片段使用内存
  源码，不写临时文件；依赖或继承导入失败不满足预期编译失败。`--locked` 检查每个被检查文档所属包而非调用者工作目录。
  任何失败退出码为 1；没有可检查的块或过滤后为空也退出 1。抽取器在 `aevia-doc::doctest`。
- 编译失败测试：`compile_fail("<source>") : String -> Bool` 在隔离环境（无导入、仅 Prelude）编译片段，至少一个错误时为
  `true`；它**不**检查错误码，错误码由 doctest 的 `compile_fail EXXXX` 断言。
- 黄金测试：REPL 输出/错误信息快照。
- 基准：`bench` 声明；每项报告 N 次迭代的中位数、均值与 95% 置信区间（默认 N=100，`--iterations`/`--warmup`）。
  `--save-baseline FILE` 写 JSON（`format=1`、`tool`/`version`、`profile=debug|release`、`optimized_ir=true|false`、`iterations`、`warmup`、每项
  `median_ns`/`mean_ns`/`ci_ns`）；`--compare-baseline FILE --threshold RATIO`（默认 0.05，可写 `5%`）按中位数比较，
  超出阈值报 `REGRESSION` 并退出 1；profile 不一致退出 2；只在一侧出现的 workload 被报告但不计为回归。协议：只比较
  release 构建、同机、相同的 `--release` IR 优化设置、median of N。旧基线若没有 `optimized_ir` 字段，视为未优化。

## 9.13 性能目标

| 场景 | 目标 |
|---|---|
| 启动（空程序） | < 50 ms |
| REPL 输入到结果（简单表达式） | < 100 ms |
| `Float` 张量逐元素运算 | 与 NumPy 同量级（≤ 2×） |
| `Float` 矩阵乘法 | 与 faer 同量级（≤ 1.2×） |
| 小符号表达式化简 | < 10 ms |
| 重型积分/求解 | 受预算控制 |
| JIT 热函数 | ≥ 5× 解释器 |

这些是设计目标，不是规范保证；实现应在基准中跟踪。

## 9.14 安全与沙箱

- 没有 `unsafe` 用户代码；文件/网络 IO 只通过 `IO` 动作；capability（`fs`、`net`、`env`、`rand`、`gpu`）由运行时权限
  检查，不是类型参数（§13.9.6）。符号预算防止无限化简，栈上限防止无限递归。
- 包构建不执行任意脚本，没有 `build.aev`；需要生成代码时用插件/算子，不用构建脚本或宏。

```
with_cap : (String, () -> IO a) -> IO a
```

- `with_cap(caps, thunk)` 运行 `thunk()` 构造的动作，期间只保留 `caps` 中列出的能力（逗号分隔：`"fs"`、`"fs,rand"`；
  `""` 什么都不给）。嵌套取交集，块结束（含出错）时恢复外层。
- 需要能力的动作：文件读写、NPY、`plot`/`save`（`fs`），`random()`（`rand`）。缺少能力时，返回 `Result` 的数据动作给出
  `Err("E0902: permission denied: …")`，其余动作以 `E0902` 终止；未知能力名是 `E0302`。
- 程序默认拥有全部能力；`aevia run --caps fs,rand` 以给定集合启动，`--caps ""` 不授予任何语言级能力。

标准库中需要检查权限的动作使用 `fs` 或 `rand`。能力集合也接受 `net`、`env` 和 `gpu`。
后端能力缺失报 `E1001`（§13.8）。执行不可信程序时使用进程隔离和操作系统资源限制。

## 9.15 错误报告

所有诊断共享附录 B.1 的格式：代码、消息、主位置、标签、注释、帮助。量纲错误附带规范形与差异；类型错误附带期望/实际类型并
尽量给出“最可能的修正”；解析错误附带期望的记号与最近的合法修复。

下一章定义 Aevia 与 rssn 的具体映射：哪些能力直接用，哪些要包一层，哪些是已知缺口。


<a id="ch-10-backend"></a>

# 10 — rssn 后端映射

Aevia 不重新实现计算机代数。rssn 提供项表示、重写引擎、量纲层、数值内核、JIT 和仿真；
Aevia 提供语法、类型/量纲检查、闭包、记录、张量形状、模块、IO 和标准库编排。本章定义两者的
契约，包括直接映射的算子、类型化封装及其错误行为。

## 10.0 反射优先：没有手写算子绑定

Aevia **不维护** `sin -> rssn::sin`、`det -> rssn::det` 这样的绑定表。构建时读取 rssn 的
**Kernel Manifest**，生成 `Rssn` 模块（契约见 [13 — 可扩展性](#ch-13-extensibility) §13.2）：

```aevia
Rssn.sin(0.5)                       # Derived 的数值重载：Real
Rssn.sin(x)                         # Derived 的符号重载：Dyn<term>
Rssn.det(M)                         # Overlay：Tensor<[n,n],Real> -> Real
Rssn.dynamic("myplugin_softplus", args)  # 插件/叶子：没有签名
```

- `signature_source = Declared | Overlay | Derived` → 类型化声明。`Derived` 是“由注册信息推出”的
  保守签名：`eval` → `(Real,…) -> Real`（谓词 `-> Bool`），非叶子另加 `(Term,…) -> Term`；它不区分
  `Tensor`/`Quantity`/`Int`，IDE 标“保守推断”。
- `None`（叶子/无调用）→ `dynamic` 声明。rssn 做**签名级**校验（`check_args`，参数不被任何
  overload 接受时报 `OpError::Type`），完整的类型/量纲检查由静态检查器或 `Rssn.typed` 负责。
- 新算子只要在 rssn 注册，下一次构建就出现，不改 Aevia 编译器。
- 生成源的拼写是精确的：维度参数写成 Aevia 的量纲变量（`Real[T]`、`Real[T^(1/2)]`），变量名在声明内局部
  并**在参数与返回间共享**——`sde_wiener(a3: Real[T]): Tensor<2, Real[T^(1/2)]>` 里的 `T` 与 `T^(1/2)`
  是同一个变量。`Dyn<term>` 只保留在确实无法拼写的位置：manifest 自己声明为 `Dyn(rep, any)` 的签名、
  秩变量张量（`transpose`/`rank`/`dot`/`norm`/`sum`；`Tensor<n, τ>` 是 `E0304`）、类型参数、未知行
  `Table`、`Device`、变体、host opaque、`Never`。
- 生成的 `Units.aev`/`Constants.aev`/`Rssn.aev` 经过类型检查且无诊断：生成器对每条声明只发字面量
  `op` + `key` 的 `Rssn.invoke`，checker 按钉死的 overload 校验这两个字面量并把结果类型定为该 overload 的
  声明结果（每次调用新维度变量）。非字面量 key 保持动态 `Value`，运行时答 `E1002`/`E1003`/`E1005`。
  该验收由 `aevia-rssn` 的 `emit_typecheck` 测试断言。
- 静态重载用 `OpKey { rule_set, name, signature_hash }`；typed 调用经 `Rssn.call_key` 钉死重载；
  dynamic 调用走 `call_value`，多注册时按 `Match` 格选、并列报 `E1002`（§13.4.1）。

内核侧契约见 rssn 仓库的 `docs/KERNEL.md`。

## 10.1 rssn 的能力边界

```rust
let s = Session::new();
let x = s.sym("x");
let answer = s.compute(diff(sin(x) * cos(x), x), &Config::new())?;
```

- `Session` 拥有 hash-consed 项图；`Term<'s>` 是带生命周期的句柄。
- `Config` 指定规则集、目标相位（`Symbolic`/`Numeric`）、绑定、假设、预算、量纲模式、量纲假设、自定义基量纲。
- `compute` 把请求复制到模板图的克隆中，运行 e-graph 重写，抽取答案，再导入回会话。`Answer` 含 `term`、
  `value`、`error`、`reduced`、`dimension`、`diagnostics`、`report`。
- `Term::eval(bindings)` 不重写，直接数值求值；`Term::compile(inputs)` 编译闭式（`jit` feature 用 Cranelift）；
  `Term::as_tensor()` 把嵌套 `list` 项转成 `(shape, Vec<f64>)`；`to_latex()`/`to_typst()`/`to_pretty()` 排版。
- `sim::scenario::run(name, json)` 运行仿真；`sim::scenario::SCENARIOS` 是场景名的唯一来源（当前 29 个）。
- `Session::register_unit(UnitDef)` 注册单位；`Session::manifest()` 用 `KernelManifest::build_with_units`，
  所以 session 私有单位出现在该 session 的 manifest 里。
- SDE：`rules::sde` 把无回调的 `sde_wiener`/`sde_levy_areas` 与方案元数据 `sde_scheme_info(name) ->
  Tensor<1, Real>` 暴露成 manifest 算子（`features.sde`，能力 `core.sde`）。求解循环需要 host 回调
  （`&Fn + Sync`），不在 manifest 里；Aevia 的 `sdeint_diag`/`sdeint_general` 在宿主侧调用 `kernels::sde`。

Aevia 直接复用的部分：

| 能力 | rssn 模块 |
|---|---|
| 精确数（大整数/有理数/浮点） | `graph::Number` |
| 项构造与 hash-consing | `graph::Graph`、`Session::call_value`/`Session::parse` |
| 重写引擎与规则集 | `graph::Engine`、`rules::*` |
| 微积分/代数/方程/ODE/PDE | `rules::calculus`、`solve`、`ode`、`pde` 等 |
| SDE 噪声原语 | `rules::sde`（manifest 算子）、`kernels::sde`（积分器） |
| 量纲推断、单位解析 | `graph::dim`、`rules::units` |
| 数值内核（线性代数、FFT、ODE、优化、特殊函数） | `kernels::*`（`faer`、rustfft）、`sim::integrators` |
| 仿真 | `sim::scenario` |
| JIT | `backend::jit`、`Term::compile` |
| 排版 | `io::latex`、`io::typst`、`io::pretty` |

Aevia 自己实现：词法/语法/布局/格式化；类型与量纲的**编译期**检查；闭包、记录、变体、字符串、集合、`IO`；
张量的宿主表示、形状检查、广播、切片；模块、包、REPL、LSP；标准库编排与类型化包装；`Dyn` 的运行时量纲检查。

## 10.2 标量映射

### 10.2.1 精确数

| Aevia | rssn |
|---|---|
| `Int`（任意精度） | `Number::Int(BigInt)` |
| `Real` 精确有理数 | `Number::Rat(BigRational)` |
| `Float` | `f64` / `Number::Float` |
| `Complex` 数值 | `num_complex::Complex64`（宿主）或 `a + b*i` 项 |
| `Complex` 符号 | `a + b*i` 项 |

- `Session::int`/`rational` 只接受 `i64`；超出范围用 `Session::parse(&n.to_string())`。
- `Real` 的 `Exact(Number)` 直接映射，`Sym(TermId)` 映射到 rssn 项；`Float` 的 `F64` 不进入 rssn。
- `Complex` 的数值路径用 `Complex64`；符号路径构造 `add(re, mul(im, i))`。两条路径在 Aevia 侧统一，
  因为 rssn 的 `Number` 没有复数。

### 10.2.2 符号

```rust
let name = format!("{base}#{id}");     // 内部唯一名
let term = session.sym(&name);
```

- 显示名与内部名分开：`symbol("x")` 的内部名是 `x#1`，显示 `x`；同一符号在多次 `compute` 中复用同一 `TermId`。
- 静态路径的项图无量纲，**不**给符号传量纲，量纲由 Aevia 类型检查器负责；只有 FFI/导出才用全名传给
  `Config::dimension`。
- 符号假设（`positive`、`real`、`integer`）经 `Config::assume` 传入；`assuming` 把单元谓词（`x > 0`、
  `x >= 0`、`x != 0`、`x ∈ ℤ`）转成 `Facts`。**一般等式/不等式没有语言入口**（`Rssn` 没有 `assume` 成员，
  也没有 `core.assume.general` 能力行）。

## 10.3 算子映射：生成的 façade，不是手写绑定

下表是**语义映射的示例**，用来解释 Aevia 的语法糖如何落到 rssn 的结构算子/请求上；实际绑定由 manifest 生成。

### 10.3.1 核心算术

| Aevia | rssn 算子 |
|---|---|
| `a + b` | `add(a, b)` |
| `a - b` | `add(a, mul(-1, b))` |
| `a * b` | `mul(a, b)` |
| `a / b` | `mul(a, pow(b, -1))` |
| `a ^ n` | `pow(a, n)` |
| `-a` | `mul(-1, a)` |
| `abs`/`sqrt`/`sin`/`exp`/`ln`/`atan2` | 同名算子 |

`Session::call_value(name, args)` 解析重载、校验参数，构造项并只做该算子自身的局部重写（`add` 就是一次 n 元展平），
不跑全局重写循环；整段表达式用 `Session::parse`。`Kernel::call(&OpKey, …)` 转发 `call_key`，**钉死**
`OpKey` 指向的重载；`Kernel::call_ho` 同样按 key 找注册。

### 10.3.2 重型请求

| Aevia | rssn 请求 |
|---|---|
| `diff(f)` | `diff(term, ξ)` |
| `antiderivative(f)` | `integral(term, ξ)` |
| `defint(f,a,b)` / `limit(f,a)` / `series(f,a,n)` | `defint` / `limit` / `series` |
| `sum(f,a,b)` / `product(f,a,b)` | `sum(term, κ, a, b)` / `product(term, κ, a, b)` |
| `solve(f)` / `solve_complex(f)` / `nsolve(f,x0)` | `solve` / `solve_complex` / `nsolve` |
| `dsolve(eq,y,t)` | `dsolve(eq, y(t))` |
| `simplify(x)` / `approx(x)` | `compute(term, Config::symbolic())` / `compute(term, Config::numeric(tol))` |

### 10.3.3 数值内核

| Aevia | rssn |
|---|---|
| `defint`（数值） | `kernels::quadrature` |
| `odeint` | `kernels::ode` / `sim::integrators` |
| `sdeint_diag`/`sdeint_general` | `kernels::sde`（Aevia 侧包装） |
| `fft` | `kernels::fft`（rustfft） |
| `eig`/`svd`/`qr`/`lu` | `kernels`/`faer` |
| `interp1`/`spline`、`minimize`、特殊函数 | `kernels::*` |

数值内核直接吃 `f64`/`ndarray`，不经过项图；`Float` 张量路径优先走这里。

### 10.3.4 线性代数

- `Float`/`Complex` 矩阵：rssn 数值内核（faer）；`Real` 精确/符号小矩阵：`rules::linalg`；大矩阵无法数值化时
  返回 `Err` 或保持符号。
- `det` 的无量纲限制在 Aevia 层（rssn 的 `det` 仍可算带量纲矩阵，Aevia 不暴露那个重载）；`linsolve(A,b)` 的量纲
  `e/d` 由 Aevia 静态检查，rssn 的答案量纲用于校验。

### 10.3.5 未知/新算子

任何不在生成 façade 里的算子走动态调用：

```aevia
let r = Rssn.dynamic("foo", [to_value(x), to_value(y)])
match r with
| Ok(v) -> ...
| Err(e) -> ...
```

参数/返回值是 `Value`；rssn 按签名检查参数形态，完整的类型/量纲检查仍在 Aevia 侧。新算子注册后重新生成 `Rssn`
模块即可获得类型化声明；插件/用户算子不需要编译器改动；失败返回 `OpError`，Aevia 转成 `E1003`/`E1001`。

### 10.3.6 单位

**Aevia 的项图不带单位。** 单位字面量在编译期折叠为相干 SI 数值；`Real`/`Float` 值在宿主侧带可选的显示单位。
rssn 看到的是无量纲数值和符号表达式（`3 m + x` 在项图里是 `3 + x`），量纲由静态类型保证。

理由：rssn 的 `quantity` 没有数值 eval，且 `dimension_from_text` 把 `L` 解析成升、`T` 解析成特斯拉；把单位放进
静态项图会同时破坏数值路径和量纲路径（更多取舍见 §12.2）。单位只在两个边界出现：

1. **显示/排版**：`show`/`to_latex` 在宿主侧拼接数值与显示单位；需要 rssn 排版时用 `quantity(v, "<相干 SI 单位>")` 包一层，仅用于输出。
2. **导出/FFI**：写 CSV/NPY/JSON 时在宿主侧附单位元数据。

`Point` 进入 rssn 前转成 K 的 `Float`；`Dyn<ρ>` 的运行时量纲是 `rssn::dim::DimTerm`。`Units.*` 从
`manifest.units` 生成，不维护第二张表；启动时的单位表一致性检查（§5.10）只用于显示/导出。

### 10.3.7 仿真

```rust
let json = rssn::sim::scenario::run("ising", params)?;
```

`simulate(name, json)` 直接调用；场景名唯一来源是 kernel 的 `SCENARIOS`，宿主不手写第二份清单。类型化包装器
`Physics.*`（§8.17）是 Aevia 源模块，内部调用 `simulate` 并在记录与 JSON 间转换；`Rssn_Sim_<Name>` 模块由
rssn 的场景目录（`rssn::sim::schema`）在导入时生成（§8.17）。`ising` 返回扁平 `Vec<i8>`，包装器按
`width`/`height` 行主序 reshape 成 `Tensor<2, Int>`；`sim` 的错误是字符串，包装器按前缀映射成 `SimError`。

### 10.3.8 领域模块：`Rssn_<Domain>`

平铺的 `Rssn.<op>` 与按 rule set 分组的 `Rssn_<rule_set>` 沿用 kernel 的组织方式，说不出“`invmod` 取一个模数、返回一个
剩余”。领域模块是 Aevia 一侧的分组，由 **manifest + 一份声明式 overlay**（`crates/aevia-rssn/domains.overlay`）生成：

- **归属。** `domain`/`family` 把算子按 `rule_set:glob` 归到 29 个领域（`NumberTheory`、`Combinatorics`、`Group`、
  `FiniteField`、`Graph`、`Coding`、`Crypto`、`Topology`、`Numeric` 等），先匹配者胜。每个 rule set 必须被某个
  family 认领；kernel 新增 rule set 或算子而 overlay 没有归属时覆盖检查失败。
- **三档暴露加一档见证。** *typed*：`sig` 给出 Aevia 类型（`Result<T, OpError>`），包装器由签名推导；
  *witnessed*：见下；*dynamic*：自动生成 `Result<Value, OpError>` 包装器，新算子零编辑即可调用；*excluded*：
  `exclude`，必须写理由。`typed` 模式的领域里既无 `sig`、`dynamic` 也无 `exclude` 的算子是覆盖错误；`sig`
  的参数个数与 manifest 的 arity 不符、或指向已不存在的算子同样失败。
- **见证档（自动类型化）。** kernel 的引擎每次运行结束时，请求（`factorint(360)`）与答案（`list(list(2, 3), ...)`）
  同处一个等价类，这本身是一条类型判断；kernel 把这些判断记为“sort 事实”、按参数种类聚类，再逐条**重放**见证调用，
  得到 `signatures.witnessed.json`（rssn `docs/KERNEL.md` §3a）。manifest 里每个重载带 `overload_sources`。
  对没有 `sig`、但有可用 Aevia 类型表达的见证重载，领域模块生成与 typed 档同形的包装器
  （`fn divisors(n: Int): Result<Tensor<1, Int>, OpError>`）。类型映射：`Int`→`Int`，有理数/符号表达式/符号→`Real`，
  浮点→`Float`，`Bool`→`Bool`，矩形列表→`Tensor<秩, 元素>`；元组、不透明对象、参差列表留在 dynamic 档。一个算子有
  多个见证重载时，证据最多的占用算子名，其余以 `_int`/`_real`/`_float` 后缀区分。见证签名是**证据而非全称证明**：
  参数侧可能比算子实际接受的更窄（`Rssn.dynamic` 始终可用），结果由 `from_value<T>` 校验，越出观测范围得到 `Err`，
  不会得到类型错误的值。`dynamic op | 理由` 可强制某算子留在 dynamic 档。
- **转换是泛型的**：参数 `to_value(x)`，结果 `from_value<T>(v)`；丰富类型用 `codec Type to_fn from_fn` 声明一对转换函数
  （每个类型一对）：`Perm`（0 起始，边界上 ±1）、`Graph`/`Group`（包装不透明 `Value` 句柄）、`PermGroup`。
- **错误通道。** kernel 失败是 `Err(OpError)`；图后端算子参数越出定义域时返回未约化的调用项（`invmod(6, 15)`），
  `from_value` 把它报告为带该项文本的 `TypeMismatch`。
- **诊断通道：为什么没有约化。** 数值算子拒绝参数（形状不符、轴越界、系统奇异、Neumann 数据不相容……）时，kernel 只留下
  未约化的调用项。`Session::explain(term)`（`rssn::kernel::{Diag, DiagKind}`）在调用线程上重放该算子的求值器，给出
  `Diag { op, arg, kind, message }`。桥（`Kernel::call_with_witnesses`、`call_key`、`call_key_with_witnesses`）在结果恰好
  是该算子自己的未约化调用、且 `kind` 不是 `Symbolic` 时，返回 `KernelError::from_diag`，代码为稳定的
  `rssn.diag.<kind>`（`arity`、`shape`、`domain`、`singular`、`incompatible`、`not_converged`、`unsupported`、
  `symbolic`、`unspecified`）。`Rssn.dynamic` 在 `Err` 里写成用户形式——函数名、参数名、位置：
  ``Err(rssn.diag.domain: tensordot_2_2_1: argument `axes_a` (#3): an axis is past the rank of the first tensor (rank 2): [2])``。
  参数名取自 overlay 的 `sig`；witnessed/dynamic 包装器只给位置（`argument #3`）。`OpError` 仍是不透明类型，
  `"{e}"` 给出 `代码: 消息`，没有新增构造子或 `Rssn.*` 成员。在**抛出**位置（`Rssn.<op>(..)` 的类型化调用）同一发现是
  `E0807`，附注给出稳定代码。只有 `rules::numeric::TABLES` 的算子记录原因。
- **维度证据。** `sig` 同时声明“该算子是无量纲参数的纯函数”；kernel 声明的 `Dimensionless` 规则、以及参数已知无量纲时的
  `List/NonMetric/Compare/Angle` 规则，会让裸数结果携带 `Scalar(1)` 证据；其余规则（`Same`、`Sum`、`Custom`…）不被信任。
  `list(...)` 项结果在可信时被读回成张量（等长行合并成高阶张量，不等长保留为张量的张量）。
- **运行期生成。** 模块在解析 `import Rssn_NumberTheory` 时由 live manifest 渲染（`codegen::namespace_source`），不提交
  生成源，因此不与所链接的 kernel 漂移。提交的只有 overlay 与覆盖报告 `tools/rssn-api-audit/RSSN-COVERAGE.md`；
  `cargo test -p aevia-rssn --test domains` 重新生成并比对（`AEVIA_BLESS=1` 重写）。

**`Rssn_Numeric`。** rssn 的 `numeric` 规则集（稳健统计、窗与谱、小波、Krylov/稀疏/低秩/张量分解、维度无关的有限差分求解器、
复变特殊函数、表达式算子）全部有 `sig`，没有一个留在 dynamic 档；用户接口与示例见 §8.19.1。规则：

- 结果是浮点，签名用 `Float`/`Tensor<n, Float>`，**不是** `Real`（`from_value<Real>` 只接受精确的 `Int`/`Rat`）；整数性质的
  结果（秩、置换下标）同样以 `Float` 返回。
- 用符号命名的方法、小波、统计量参数类型是 `Real`；每次 `symbol("db2")` 产生新符号，所以每个名字只 `let` 一次再复用。
- **一个内核算子可以有多条 `sig`，各用自己的 Aevia 名字**（overlay 写作 `sig heat_fd as heat_fd_2d(..)`；同名重复是错误）：
  签名只有一个字面量秩，所以维度无关的算子按维数命名（`_2d`/`_3d`），秩多态的张量算子按秩命名
  （`tensor_norm_<r>`、`tensordot_<ra>_<rb>_<k>` 等，见 §8.19.1）。
- **混合列表的解码。** 张量分解的结果是类型各异的部分组成的列表。桥把它保留为带逐元素量纲证据的一维值，
  `from_value<Tensor<1, Value>>` 是通用的“列表访问器”：把第一个轴的各部分逐个作为 `Value` 句柄交出，再对每一部分
  `from_value<T>`。解码器是 `Numeric` 前言里的 Aevia 源码（`hosvd3_from_value` 等），用 `codec Hosvd3 - hosvd3_from_value`
  声明为只解码的类型（`-` 表示没有编码函数；把这样的类型当参数是覆盖检查的错误）。

示例：`tests/domain_number_theory.aev`、`domain_combinatorics.aev`、`domain_finite_fields.aev`、`domain_numeric.aev`。

`Zmod` 是携带运行时模数的名义类型，因此同一段代码可以接收计算得出的模数。用
`Rssn_NumberTheory.zmod(value, modulus)` 构造余数，然后调用 `zmod_add`、`zmod_mul`、
`zmod_inv` 或 `zmod_pow`。构造和运算返回 `Result<Zmod, String>`。二元运算检查模数相等，
求逆会检查余数是否可逆。例如，模 17 时 `zmod_pow(x, -1)` 求逆，`zmod_mul(x, inverse)` 返回余数 1。

## 10.4 量纲桥

量纲/单位的唯一事实来源是 `rssn::dim`（`DimTerm`/`DimArena`/`UnitRegistry`）。Aevia 的 `Dim` 是它的类型级镜像；
`DimValue` 在运行时就是 `DimTerm`，不是字符串。

### 10.4.1 静态到运行时

**静态路径不把量纲传给 rssn**：类型检查器已保证量纲正确，项图里的符号无量纲，`DimensionMode` 设为 `Off`（或 `Advisory`
仅用于诊断）。只有三种场景交出量纲：

- `Dyn<ρ>` 的运行时检查：宿主保存 `DimTerm`，不依赖字符串；
- FFI/导出：优先 `Config::dimension_term(symbol, DimExpr)`（不解析文本，未知基名被 intern）。纯文本入口
  `Config::dimension(symbol, text)` 必须传**全名**（`Length`、`Mass`、`Time`、`Current`、`Temperature`、`Amount`、
  `Luminosity`），**绝不用单字母**（`L` 是升，`T` 是特斯拉，`N` 是牛顿，`J` 是焦耳）；
- 用户显式请求 rssn 的量纲诊断：`Config::dimensions(Advisory)`。

自定义基量纲用 `Config::base_dimension(name)` 声明；`prepare` 在**克隆出来的**图上调用 `define_base_dimension`。
`Graph::clone` 共享 `Arc<RwLock<DimArena>>`，写入时 `strong_count > 1` 就 detach（写时复制），所以请求期注册不会漏进
模板或并发请求，不定义基量纲的请求也不付拷贝成本。不要直接改 `Session` 的模板图。

### 10.4.2 运行时到静态

数值答案的量纲以 `Answer.dimension` 为准，**不要**从 `Answer.term` 重新推断。`Answer.dimension` 是 `Option`：
`DimensionMode::Off` 时为 `None`，回退到静态量纲；要运行时量纲，数值路径必须用 `Advisory` 或 `Strict`。静态路径的
答案量纲已知，`Answer.dimension` 只用于可选的一致性检查；不一致是内部错误 `E0809`。

### 10.4.3 量纲模式

| 场景 | 模式 |
|---|---|
| 普通静态检查通过的代码 | `Off`（项图无量纲） |
| `Dyn`、FFI、运行时构造的项 | `Strict`，用全名 |
| 用户显式请求诊断 | `Advisory` |

Aevia 不暴露 `bypass` 给普通用户；`Dyn<ρ>` 是显式逃逸。

## 10.5 线程与生命周期

### 10.5.1 Session 不是 `Sync`

`Session` 内部是 `RefCell`，`Term<'s>` 借用 `Session`。桥把内核会话放在**线程本地**（`aevia-rssn` 的 `kernel()`）：

```rust
thread_local! {
    static KERNEL: RefCell<Option<Rc<Kernel>>> = const { RefCell::new(None) };
}

pub fn kernel() -> Rc<Kernel>;   // 当前线程的会话，首次使用时创建
```

- 每个线程一个会话；`par_map` 的工作线程各自持有求值器副本和会话（§9.7.2）。
- 符号值不跨线程共享：kernel 的 `TermRef` 带 `SessionId`，别的会话的 handle 被拒绝（`OpError::ForeignTerm`）；不能跨线程的值回退为
  顺序求值。桥不使用 `Session::fork`/`merge`（kernel 提供的原语）。
- `compute_all` 在单会话内并行，但**桥没有使用它**：批量请求按顺序调用；需要并行时应在宿主侧按线程分配会话。

### 10.5.2 项的生命周期

`Term<'s>` 只在 `compute` 调用期间借用，返回后转成 `u32` id；`Session::term_by_id(id)` 取回，id 在会话内稳定；
`compute` 的答案项已导入回会话。项图只增不减，REPL `:reset` 重建会话。

## 10.6 性能策略

### 10.6.1 不要为每个算子调用 `compute`

`compute` 克隆模板图并运行重写；对 `x + 1` 这种 O(1) 构造，代价高几个数量级。规则：

- 算术/初等函数：`Session::call_value` 构造项（只做局部重写），不 `compute`。
- `simplify`/`eval`/`diff`/`integrate`/`solve`：`compute`。
- 打印默认不化简；用户显式 `simplify` 或 `:latex`。
- 比较/`if`：`Prop` 不求值；需要判定时 `decide` 或 `assuming`。

### 10.6.2 缓存

- `Config` 按规则集/假设/模式缓存；项按结构哈希缓存；编译函数按项哈希 + 输入名缓存；单位表/量纲别名是编译期常量。
  rssn 另有请求级答案缓存与可选的子项引理缓存。
### 10.6.3 批处理

- 张量逐元素符号运算不要用 `compute_all`（每元素一个请求，开销大）：用 `map` 构造项，最后一次 `simplify`。
- 数值张量用 `F64` 路径，不进项图。

### 10.6.4 JIT

`Term::compile` 把闭式 `Float` 表达式编译成函数；Aevia 的热点函数走这条路径。`TieredBackend` 先解释、后台编译；JIT 失败回退
解释器，不改变语义。默认构建 `features.jit = false`，`@jit` 必须读 `Features`，否则报 `E1001("core.jit")`。

### 10.6.5 设备与数据并行执行

Aevia 的 `Device` 值标识请求的执行位置。`device_of(tensor)` 返回张量所在位置。
`to_device(tensor, device)` 和 `on(device, thunk)` 是 IO 动作。默认支持 CPU 执行。
请求不可用的 GPU 或分布式设备时，报 `E1001`，其能力名分别为 `core.gpu` 或
`core.distributed`（§13.9.4）。

RSSN 设备层提供缓冲区、数值内核 IR、CPU 参考及并行执行器，以及通过 `gpu-wgpu`
启用的 WGSL 后端。嵌入宿主可用它批量求值、执行数组运算与固定步长集合仿真。
Aevia 解释器的 CPU 批量路径接受 §9.6.1 所述纯标量子集。设备枚举、启动融合和显式
GPU 缓冲区传输由 RSSN 宿主 API 提供。

RSSN GPU 后端用两个 F32 分量模拟 F64 缓冲区。要求 IEEE F64 结果的程序使用 CPU
后端。宿主负责传输，并在装入内核前检查设备能力。

## 10.7 错误映射

| rssn 错误 | Aevia 诊断 |
|---|---|
| `ComputeError::Dimension(e)` | `E0401` 或 `E0412`（`Dyn`） |
| `ComputeError::NotNumeric(s)` | `E0803`（自由符号）或 `E0806`（定义域） |
| `ComputeError::Inconsistent(s)` | `E0809`（内部错误） |
| `ComputeError::Rule(e)`、`UnknownOperator(s)` | `E0899`（内部错误） |
| 预算耗尽 | `E0808` |
| `BackendError` | `E0807` 或 `E0899` |

`Answer.diagnostics` 转成 Aevia 警告；`Advisory` 模式下不失败。

## 10.8 内核边界约定

- 项属于一个 RSSN 会话。并行任务在具有对应会话的线程上准备或导入项，并按输入顺序
  恢复返回结果（§10.5.1）。
- 该会话的内核清单决定可用算子、单位、常数，以及哪些值可进入项图。生成绑定使用
  清单的 `api_version` 与完整的 `hash_hex()` 标识接口。截断的 `hash_u64()`
  用于 ABI 报告与显示。
- 精确类型化绑定固定受支持的签名。推导或观测签名仅在生成工具能表达并验证其约定时
  使用。动态接口保留算子错误（§10.3.5）。
- 无法确定重数时，`Solutions.multiplicities` 可以为空。只有求解方法得到完整解集时
  才报告完备性。条件与其对应的解一同返回。
- Aevia 的静态量纲可以使用有理指数（§4.12.3）。一些 RSSN 宿主还可表示符号指数。
  动态接口必须保留量纲证据或报告错误。
- 回调句柄限于发起它的宿主调用（§8.19）。仿真参数可通过模式检查的记录或通用 JSON
  接口传入（§8.17）。可选本机代码与设备能力在选用时确定（§10.6）。

### 10.8.1 兼容性与验证

使用 `aevia rssn --audit` 查看所链接内核的签名及能力。一致性测试比较源码声明与已检查
适配器，覆盖结构化值与单位，并将编译求值与参考路径对照。运行时预算与符号预算分别
设定（§6.9.1）。本机求值器无法执行某条受支持的路径时，可回到解释器。

## 10.9 一个端到端的调用链

```aevia
fn fall_time(h: Float[L]): Float[T] =
    sqrt(2 * h / g)
```

1. 类型检查：`h : Float[L]`，`g : Float[L/T^2]`，`2*h/g : Float[T^2]`，`sqrt : Float[T]`。
2. 调用 `fall_time(20.0 m)`：`20.0 m` 编译期折叠为 `Float` 值 20.0，带显示单位 `m`。
3. 函数体：`2 * h`、`/ g`、`sqrt` 在 `Float` 上直接用 f64 算术，不进项图；若参数是 `Real` 符号，则分别是 `Session::call_value`
   构造的 `mul`/`pow`/`sqrt` 项（只做局部重写），不 `compute`。
4. `println` 调用 `show`，按显示单位 `s` 打印：`2.0196199771025523 s`。
5. `approx(x)` 对符号/精确值以 `Numeric` 目标运行 `compute`，rssn 返回数值球，Aevia 取中点。

这个链条展示了核心性能策略：**类型/量纲在编译期，算术在项构造，重型求解才进 e-graph，数值热点走 f64/JIT**。

下一章用完整程序展示这些规则如何组合。


<a id="ch-11-examples"></a>

# 11 — 完整示例

这些程序不是片段，而是可以放进 `.aev` 文件、用 `aevia run` 运行或 `aevia check` 检查的完整模块（`aevia-std` 的 `spec_programs` 测试逐个检查它们）。
每个例子后面解释它用到的语言特性，以及类型/量纲是如何推导的。输出是 `aevia run` 的实际结果。

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
fall time from 20.0 m = 2.0196199771025523 s
```

推导：

- `9.80665` 是十进制字面量，类型 `Float[1]`；乘以 `m/s^2` 得到
  `Float[L/T^2]`。
- `20.0 m` 是 `Float[L]`。
- `2 * h`：`2 : Int` 提升为 `Float`，结果 `Float[L]`。
- `/ g`：`L / (L/T^2) = T^2`。
- `sqrt`：指数除以 2，`T^2 → T`。
- 返回值与 `Float[T]` 统一。

`println` 用 `show`，按显示单位打印：`h` 的显示单位是 `m`，`fall_time` 的结果没有显式显示单位，打印时选择 `s`。

## 11.2 抛体运动：记录、ODE 与结构化状态

```aevia
module Projectile

open Units.SI
import Math { sqrt, sin, cos }
import LinearAlgebra { linspace }
import Ode { odeint }

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
    let ts = linspace(0.0 s, 3.0 s, 1000)
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
import Calculus { diff, defint, series }
import Solve { solve }

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

输出：

```
f'(x) = 3*x^2 - 3
f'(2) = 9
g'(x) = exp(x)
roots = { variable = Some("x"), roots = [2, 3], multiplicities = [1, 1], conditions = [], complete = true }
∫₀¹ t² dt = 1/3
exp(t) ≈ 1 + x + 1/2*x^2 + 1/6*x^3 + 1/24*x^4 + 1/120*x^5
```

要点：

- `diff` 接受函数，不接受表达式。它创建同量纲的符号，应用函数，求导，
  再代回。因此 `diff(g)` 对任意 `g` 都成立。
- `df(2)` 是精确的 `9`，类型 `Real[1]`；要浮点，写 `approx(df(2))`。
- `solve`/`defint` 返回 `Result`，因为可能无解/不可积；`diff` 是 total 的，失败时保持未化简表达式。`solve` 的结果是 `Solutions` 记录（§8.6）：根、重数、条件与 `complete`。
- `series` 返回截断到给定阶的多项式（展开变量名由求解器选定），量纲仍是 `Real[1]`。

## 11.4 欧姆定律：带量纲的线性代数

```aevia
module Circuits

open Units.SI
import LinearAlgebra { linsolve, det }

fn main(): IO Unit = do
    let r = [[1.0, 2.0], [3.0, 4.0]] * ohm
    let i = [0.1, 0.2] * A

    let v = r @ i
    println("V = {v}")                  # [0.5 V, 1.1 V]

    let p = v * i
    println("P = {sum(p)}")             # 0.27 W

    match linsolve(r, v) with
    | Ok(i2) -> println("I = {i2}")     # 回到 [0.1 A, 0.2 A]（浮点舍入）
    | Err(e) -> println("solve failed: {e}")

    # det 只接受无量纲矩阵：
    # let d = det(r)                    # E0410
    let d = det(r / ohm)
    println("det(R/Ω) = {d}")
```

推导：

- `[[...]]` 是 `Tensor<2, Float>`；乘以 `ohm : Unit[Resistance]` 得到
  `Tensor<2, Float[Resistance]>`。
- `r @ i`：元素量纲 `Resistance * Current = Voltage`；结果
  `Tensor<1, Float[Voltage]>`。
- `v * i`：`Voltage * Current = Power`；`sum` 保持量纲。
- `linsolve(r, v)`：`Voltage / Resistance = Current`。
- `det(r)` 被拒绝，因为 `det` 的量纲是 `d^n`，`n` 是运行时阶数；`r / ohm`
  是无量纲矩阵，`det` 合法。

矩阵元素带单位，矩阵乘法自动得到电压，不需要在运行时给每个元素贴标签。

## 11.5 温度：仿射点与温差

```aevia
module Thermo

open Units.SI

fn main(): IO Unit = do
    let t1 = 25.0 °C                 # Point[Θ]
    let t2 = point(300.0, K)         # Point[Θ]
    let dt = t2 - t1                 # Float[Θ]
    let t3 = t1 + 10.0 K             # Point[Θ]

    println("T1 = {t1}")
    println("T2 = {to(°C, t2)}")
    println("T3 = {t3}")
    println("ΔT = {dt}")

    # 下面两行是类型错误：
    # let bad1 = t1 + t2             # E0405
    # let bad2 = 2.0 * t1            # E0405
```

输出：

```
T1 = 25.0 °C
T2 = 26.85 °C
T3 = 35.0 °C
ΔT = 1.8500000000000227 K
```

要点：

- `25.0 °C` 是仿射单位字面量，类型 `Point[Θ]`，值是 298.15 K。
- `300.0 K` 是线性单位字面量，类型 `Float[Θ]`，是温差，不是绝对温度。
  绝对温度用 `point(300.0, K)`。
- `Point - Point` 得到温差；`Point + 温差` 得到点；`Point + Point` 和
  `k * Point` 是错误。
- `to(°C, T2)` 只改显示单位；`T2` 的值仍是 K。

## 11.6 数据分析：`Table` 与动态列

将以下数据保存为 `measurements.csv`。两列数值分别记录以秒表示的时间和以米每秒表示的速度。

```csv
time [s],velocity [m/s]
0,1.0
1,1.5
2,2.1
```

```aevia
module Analysis

open Units.SI
import Data.Csv { read_csv_dyn, column }
import Stats { mean, std }
import Plot { line, combine, plot, label }

fn report_data(times: Tensor<1, Float>, velocities: Tensor<1, Float>): IO Unit = do
    let t = times * 1.0 s
    let v = velocities * (1.0 m / 1.0 s)
    println("mean speed = {mean(v)}")
    println("std  speed = {std(v)}")
    let spec = label(line(t, v), "v(t)")
    plot(combine([spec]))

fn main(): IO Unit = do
    r <- read_csv_dyn("measurements.csv")
    match r with
    | Err(e) -> println("cannot read: {e}")
    | Ok(tbl) ->
        match (column(tbl, "time"), column(tbl, "velocity")) with
        | (Ok(t), Ok(v)) -> report_data(t, v)
        | _ -> println("expected time and velocity columns")
```

要点：

- `read_csv_dyn` 返回一个产生 `Result<Table<Dynamic>, DataError>` 的 IO 动作。`<-` 绑定结果，`match` 处理读取错误。
- `column` 在运行时查找列。本例的单元格读为无量纲 `Float`，符合 `report_data` 的参数类型。表头单位是可通过 `schema` 查看的元数据。`report_data` 将数值分别乘以 `1.0 s` 和 `1.0 m / 1.0 s`，赋予样例声明的单位。
- `line` 接受带量纲的 `x`/`y` 张量，自动生成轴标签（`x / s`、
  `y / (m/s)`）；改用 `line_with` 可指定显示单位。
- `combine([spec])` 构造 `PlotSpec`；`plot` 将内嵌 SVG 的 HTML 写入 `plot.html`。

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

fn linear_fit<d, e>(xs: Tensor<1, Real[d]>, ys: Tensor<1, Real[e]>): { slope: Real[e/d], intercept: Real[e] } =
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

这些函数对任何量纲都成立；运行时没有单态化，量纲在类型检查后擦除。该模块是库（没有 `main`），由 `aevia check` 检查。该模块是库（没有 `main`），由 `aevia check` 检查。

## 11.9 错误示例：编译器如何拒绝

```aevia
let n: Int = 3
let bad1 = 3 m + 2 s
let bad2 = sin(3 m)
let bad3 = (3 m)^n              # n : Int
let bad4 = 25.0 °C * 2.0
let bad5 = 1 / (0 m)
```

诊断（节选）：

```
error[E0401]: dimension mismatch
  --> bad.aev:3:12
   |
3 | let bad1 = 3 m + 2 s
   |            ^^^^^^^^^ expected T, found L
   |            ^^^ this is L
   |                  ^^^ this is T
   |
   = difference: L/T

error[E0403]: `sin` requires a dimensionless argument, found L
  --> bad.aev:4:16
   |
4 | let bad2 = sin(3 m)
   |                ^^^
   |
help: divide by a unit first, for example `x / 1 m`

error[E0404]: a dimensioned value cannot be raised to a runtime exponent
  --> bad.aev:5:13
   |
5 | let bad3 = (3 m)^n
   |             ^^^^^^
   |
help: use a literal exponent, or `pow_dyn` for a checked dynamic power

error[E0405]: affine points cannot be multiplied
  --> bad.aev:6:12
   |
6 | let bad4 = 25.0 °C * 2.0
   |            ^^^^^^^^^^^^^
```

`E0401`–`E0405` 是编译期错误（`aevia check` 只报这四个）；`bad5` 的 `E0801`（除以零）是运行时错误，编译器 MAY 在常量折叠时提前报出。错误信息的目标是**指出量纲差异，并在可能时给出可执行的修正**，而不是只打印类型名。

## 11.10 一个“全特性”程序

```aevia
module Full

open Units.SI
import Math { exp }
import LinearAlgebra { linsolve, linspace }
import Calculus { diff }
import Plot { line, combine, plot, label }

fn kinetic(m: Float[M], v: Float[L/T]): Float[Energy] =
    m * v^2 / 2

fn main(): IO Unit = do
    # 1. 单位与量纲
    let m = 2.0 kg
    let v = 3.0 m/s
    println("E = {kinetic(m, v)}")

    # 2. 符号推导
    let sym = symbol("x")
    println("f' = {diff(fn t => exp(-t^2))(sym)}")

    # 3. 线性代数
    let a = [[2.0, 1.0], [1.0, 3.0]]
    let b = [1.0, 2.0]
    match linsolve(a, b) with
    | Ok(sol) -> println("x = {sol}")
    | Err(e) -> println("solve failed: {e}")

    # 4. 数据与绘图
    let ts = linspace(0.0 s, 1.0 s, 100)
    let vs = map(fn t => (3.0 m/s) * exp(-(t / s) / 0.5), ts)
    let spec = label(line(ts, vs), "v(t)")
    plot(combine([spec]))
```

这个程序把单位、符号、线性代数、张量和绘图串在一起：**一门语言覆盖从公式到图表的整个工作流**，而不是在多个库之间搬运字符串。

下一章解释这些设计选择的理由、替代方案和已知限制。


<a id="ch-12-rationale"></a>

# 12 — 设计取舍与限制

本章解释 Aevia 为什么长成这样，和现有方案比有什么不同，以及哪些是刻意不做的。本章是资料性的。

## 12.1 为什么把量纲做成类型

科学计算中的错误，大多数不是“算法写错了”，而是单位/量纲搞错了：把 `kW` 当成 `W`（差 1000 倍）、把 `°C` 当成 `K`（差 273.15）、
把 `Hz` 当成 `rad/s`（差 `2π`）、把质量流量当成体积流量。运行时单位库（Pint、Unitful）能在测试中抓到这些错误，但错误可能在生产才出现。
类型系统在编译期抓到，而且能**指导推断**：`mean(xs)` 的量纲由 `xs` 决定，`var(xs)` 是 `xs` 的平方，`diff(f)` 是 `f` 除以自变量——
这些规则库函数表达不了，必须进入类型。

代价是类型系统更复杂。Aevia 的控制方式：

- 量纲代数限制在自由阿贝尔群 + 有理指数，统一可判定；
- 不支持静态符号指数，避免不可判定；
- 静态不可表达时用 `Dyn` 显式逃逸，而不是发明不可靠的类型；
- 单位不进类型（§12.2），避免类型级换算的组合爆炸。

## 12.2 为什么单位不进类型

若 `m` 和 `ft` 是不同类型，函数签名要量化单位、类型相等要带单位、换算要在类型层做：类型错误信息变成“米 vs 英尺”而不是“长度 vs 时间”；
单位表的变化会破坏类型兼容；泛型函数要处理无限多的单位组合。

Aevia 的选择：**类型只记量纲，单位是编译期尺度**。`3 km` 和 `3000 m` 是同一个类型、同一个值；需要显示时用 `to`。这牺牲了“同一量纲不同单位”
的类型区分，换来简单、可组合的类型系统；实践中的错误几乎都是量纲错误，而不是“米 vs 英尺”。

## 12.3 为什么纯函数

**纯函数。** 纯函数可以安全地并行、缓存、求导、代入、化简；符号计算要求表达式可替换，可变状态会破坏替换语义；量纲规则也更简单（没有“变量何时被改”）。
IO 被隔离在 `IO`，库函数不会偷偷读文件/打印。代价：数值算法不能用原地循环，用尾递归 + 累加器，性能靠尾调用消除与写时复制；某些算法（原地 LU、就地
FFT）在运行时唯一持有时优化，语义仍是纯的；用户要学 `fold`/`map`/递归而不是 `for`。`|>`、`map`、`fold`、模式匹配和尾调用让纯代码读起来像数据流。

## 12.4 为什么严格求值

性能可预测，没有 thunk 堆积和空间泄漏；错误在发生处暴露；与 rssn 的严格项构造一致。符号计算不需要惰性：`symbol` 是值，表达式是项，
需要化简时显式 `eval`；惰性会让“什么时候跑重写”不可预测。

## 12.5 为什么区分 `Real` 与 `Float`

- `Real`：精确有理数或符号表达式，适合推导、化简、精确结果。
- `Float`：IEEE double，适合数值计算、大数据、JIT。
- 十进制字面量默认 `Float`（写 `0.1` 的人通常想要浮点；写 `1/10` 的人想要精确）；混合运算 `Real` 吸收 `Float`（精确优先），热路径显式 `Float`。

只有一种数值类型都不好：全用 `Float`，`solve(x^2 - 2)` 得到 1.414…，失去精确性，符号推导要另一套类型；全用 `Real`，数值循环慢，BLAS/JIT 用不上。
精确/近似是科学计算的分界，值得显式。

## 12.6 为什么用 HM 而不是依赖类型

HM + 量纲代数足以表达绝大多数科学公式；推断是主要的易用性来源（用户不写签名）；依赖类型（shape-indexed、静态指数）会让 `reshape`、CSV、切片、
广播的类型变得复杂，错误信息更难懂；静态不可表达的情况用 `Dyn` 比“写不出来的依赖类型”更实用。Aevia 不是证明助手：大多数程序不用标注，标注了能检查。

## 12.7 与现有方案的比较

| 方案 | 量纲 | 单位 | 符号 | 纯函数 | 类型推断 | 评价 |
|---|---|---|---|---|---|---|
| F# units of measure | 类型 | 类型 | 否 | 否 | 是 | 量纲类型化的先驱；单位进类型，泛型复杂 |
| Julia Unitful | 运行时 | 运行时 | 部分 | 否 | 否 | 生态好；错误在运行时 |
| Python Pint/Astropy | 运行时 | 运行时 | 否 | 否 | 否 | 易用；性能/类型弱 |
| Frink | 运行时 | 运行时 | 否 | 否 | 否 | 单位计算器；不是通用语言 |
| Mathematica | 运行时 | 运行时 | 是 | 部分 | 否 | 符号强；量纲不是类型 |
| Haskell units | 类型 | 类型 | 否 | 是 | 是 | 表达力强；学习曲线陡 |
| Squants | 类型 | 类型 | 否 | 是 | 是 | Scala 库；单位进类型 |
| **Aevia** | **类型** | **编译期尺度** | **是** | **是** | **是** | 量纲类型 + 符号/数值双相 + 纯函数 |

差异点：量纲是类型而单位不是；符号与数值同一门语言，`diff`/`solve` 接受函数而不是字符串；纯函数 + `IO` 隔离让符号替换安全；`Dyn` 显式处理静态不可表达的量纲；
后端复用 rssn 的 e-graph、数值内核、JIT 和仿真。

## 12.8 设计边界（不是版本限制）

这些是**有意的设计边界**，不是“以后再说”的待办。每条说明为什么这样定，以及后端未实现时的表现（capability 诊断，而不是删语言特性）。

| # | 边界 | 影响与绕法 |
|---|---|---|
| 12.8.1 | 形状细化只做线性 `Nat`（字面量、`+`、`*`、线性等式），不做一般递归/证明 | 复杂形状证明要显式 `refine_shape`/`reshape`，或退到 `Tensor<n,τ>` 运行时检查 |
| 12.8.2 | 静态符号指数不支持：`x^n`（`n : Int`，`x` 带量纲）、带量纲的 `det`/`product` | 这是量纲代数的真实边界；用 `Dyn`，或先除以单位做无量纲计算再装回量纲 |
| 12.8.3 | `Point[d]` 的表示是 `Float` + `UnitId`，符号绝对温度不进入 `Point` | 符号热力学不能直接做绝对温度代数；用温差 + 参考点，或 `Dyn` |
| 12.8.4 | 不确定度不是核心类型：`Measurement<d>` 只做独立误差传播，相关误差用 `Covariance<d,e>` | 实验数据的不确定度要显式管理；它是库类型 |
| 12.8.5 | 不做用户自定义效果与宏：`IO` 是唯一效果；扩展点是 rssn 算子、插件、`Rssn` 动态调用和 registry | DSL/代码生成用 `@op`、插件、`Rssn.dynamic` |
| 12.8.6 | `Table<ρ>` 携带行类型参数，外部输入在运行时提供列名与单位元数据 | 列未知时使用 `read_csv_dyn`，并用 `schema` 检查结果（§8.11） |
| 12.8.7 | 符号值绑定 rssn 会话，不能跨线程：`par_map` 的工作线程各持副本，不能跨线程的值回退顺序求值（§9.7.2） | 符号阶段串行，数值阶段并行 |
| 12.8.8 | 单位名歧义：`ms` 可能是毫秒或米·秒，最长匹配优先 | 少见组合写显式 `*`；格式化器提示 |
| 12.8.9 | `Real` 吸收 `Float`：`Real + Float` 变成 `Real`，`Float` 被精确转换成有理数 | 混合符号与浮点会离开快速路径，产生大分子/分母；热路径全程 `Float`，编译器提示 `W0302` |
| 12.8.10 | 设备/GPU/分布式是运行时目标，`Device` 是运行时值，不是张量类型参数 | 类型系统不区分 CPU/GPU 张量，设备错误在运行时/能力检查时发现；用 `on(device, ...)`、`@jit(target=...)`，避免类型组合爆炸 |
| 12.8.11 | 没有形式化语义/证明：规范是文字 + 规则，没有 Coq/Lean 形式化 | 靠 conformance 测试、属性测试、与 rssn 的交叉检查 |
| 12.8.12 | `RateOf<s>` 只覆盖数值状态（标量、记录、元组、张量的递归） | 含 `Int`/`Bool`/`String`/函数字段的状态，把常量参数放进 `rhs` 闭包 |

### 12.8.13 分派与扩展边界

Aevia 不做多重分派，也不做按类型或量纲的单态化。前者是语义决定，后者是工程决定。

- **没有运行时多方法。** 方法调用在检查期解决：具体头类型记录精确实例键（类 + 规范头）；只有量化头类型允许按运行时见证在**已证明两两不相交**的候选实例之间选择，
  见证不能区分的实例在检查期被拒绝（§3.8.4、§9.2.3）。“选哪个实例”仍是静态问题，运行时只执行检查器已证明的选择，不回溯、不按值猜测。
- **不单态化。** 单态化按类型复制泛型代码，放大编译时间与代码体积，破坏单独编译，并与类型擦除（§9.2.3、§4.4.4）和量纲多态直接冲突——`mean<L>` 与 `mean<T>`
  必须是同一份代码；其正确性收益也不存在，因为类型错误在检查期已排除。按操作的、带标签守卫的**表示专用化**保留泛化路径，不复制函数、不改变语义，在测量基线之后才进入性能计划（§9.13）。
- **扩展点是显式的**：ADT、记录行、模块/导入、`derive`（受限结构集，§3.8.5）、`@op`/`@rule`、插件与注册表、`Dyn` 逃逸。模块接口导出类型、构造器、类、实例与别名定义（§7.1.3/§7.3）。
  没有隐式全局实例、类型类默认方法、宏/引用/拼接。

代价：用户不能给既有类型添加类型类实例（孤儿规则）；分布、单位这类“有参数的量纲族”用名义类型 + 内建重载表达（§8.7.1），而不是多参数类。

## 12.9 已关闭的设计决策

下面是最终决定；实现可以分阶段，语言设计不再分叉。

| # | 问题 | 决定 |
|---|---|---|
| 1 | `Point` 扩展到张量？ | 是；`Tensor<shape, Point[d]>`，元素级点算术 |
| 2 | `Angle` 成为基量纲？ | 否；`rad` 无量纲，`deg` 是尺度 |
| 3 | `Tensor` shape-indexed？ | 是，作为 refinement：`Tensor<[3,4],τ>`；`Nat` 线性算术 |
| 4 | `det` 默认返回 `Dyn`？ | 否；无量纲 `det` + `det_unit` |
| 5 | `Real`/`Float` 混合？ | `Real` 吸收 `Float`；热路径显式 `Float` |
| 6 | `Dyn` 错误？ | 算术 `panic` + `try_*`；`check` 返回 `Result` |
| 7 | `RateOf` 嵌套？ | 标量、记录、元组、张量，递归定义 |
| 8 | 如何确定表格的列类型？ | `Table<ρ>` 携带行类型参数。`schema` 报告从已加载数据检测出的类型（§8.11）。 |
| 9 | `@` 还是 `*` 矩阵乘？ | 保持 `@`；`*` 逐元素 |
| 10 | `%` 取模？ | 否；`mod`/`rem` |
| 11 | `let` 默认递归？ | 否；`fn` 递归，`let rec` 显式 |
| 12 | 顶层 IO？ | 否；`main`/REPL |
| 13 | 中心 registry？ | 是；内容寻址 + 签名 + capability，协议见 §13.7 |
| 14 | 用户量纲别名导出？ | 是；默认导出，`private` 可隐藏 |
| 15 | 单位换算类型级保证？ | 否；单位是尺度，量纲是类型 |
| 16 | GPU/分布式？ | 设备是运行时值：`Device`/`on`/`@jit(target)` |
| 17 | 稀疏？ | `Sparse<F,shape,τ>` 核心类型，`Layout` 类 |
| 18 | 用户效果/宏？ | 否 |
| 19 | 不确定度？ | `Measurement`/`Covariance` 标准库类型 |
| 20 | 算子集成？ | 反射优先：Kernel Manifest + 动态 `Value` + 生成 façade；无手写绑定 |

## 12.10 成功标准

1. 物理/工程/数据科学用户能在一天内写出带单位、带符号推导、带数值求解的程序；
2. 量纲错误在编译期被抓住，错误信息能指导修正；
3. 数值热点的性能与 Python/NumPy 同量级；
4. 符号计算覆盖常见微积分/方程/线性代数；
5. 代码可读，像数学，而不是像配置。

下一章是可扩展性与 rssn 集成；之后是完整文法与诊断目录。


<a id="ch-13-extensibility"></a>

# 13 — 可扩展性与 RSSN 集成

当后端无法提供所请求的操作时，报告能力诊断 `E1001`。

后端接口的详细契约在 rssn 仓库的 `docs/KERNEL.md`（Kernel Manifest、动态调用 ABI、插件）、`docs/DIMENSIONS.md` 与 `docs/CALLBACKS.md`；
本章定义 Aevia 侧的规范性契约。

---

## 13.1 三条公理

1. **单一算子注册表。** rssn 的 `OpTable` 是唯一事实来源。Aevia 不维护“支持哪些算子”的清单；它读取 **Kernel Manifest**，生成 `Rssn` 模块。
   新算子在 rssn 注册后，Aevia 下一次构建就能看见。
2. **单一量纲/单位层。** `rssn::dim` 的 `DimTerm`/`DimArena`/`UnitRegistry` 是运行时与编译期共享的规范层；Aevia 的 `Dim` 是它的类型级镜像，
   `Units.*` 由 `manifest.units` 生成，不维护第二张单位表。
3. **动态调用是正式能力。** 没有类型签名的算子仍可经 `Rssn.dynamic(name, args)` 调用；类型化 façade 是静态体验的增量，不是能力门槛。rssn 在动态路径上做
   **签名级**校验（`check_args`），不做量纲/类型推导；完整检查由 Aevia 静态检查器或 `Rssn.typed` 负责。

---

## 13.2 Kernel Manifest 与生成的 `Rssn` 模块

### 13.2.1 Manifest

rssn 在启动时提供确定性的 `KernelManifest`：

```
KernelManifest
  rssn_version : String
  api_version  : Int
  manifest_hash: Bytes32
  features     : Features
  rule_sets    : [RuleSetInfo]
  ops          : [OpInfo]
  units        : [UnitValue]
  base_dimensions : [BaseDimInfo]
  plugins      : [PluginInfo]
  constants    : [ConstantValue]
  value_kinds  : [ValueKindInfo]
```

- `OpInfo`：`keys: [OpKey]`（每个 overload 一个，`keys[0]` 是主重载）、`name`、`aliases`、`category`、`arity`、`overloads`、`signature_source`、`flags`、
  `binder`、`cost`、`eval`、`higher_order`、`purity`、`determinism`、`thread_safety`、`stability`、`since`、`doc`、`examples`、`dim_rule`、
  `backend_support`、`dynamic_reason`、`rule_set`、`jit`。完整 schema 见 rssn 仓库根目录文档 §3.1。
- `manifest_hash` 是确定性 bincode 的 BLAKE3；`to_json()` 只供人读。
- `units: [UnitValue]` 带 `name`/`display`/`dimension`/`scale`/`offset`/`affine`/`aliases`/`prefixable`/`category`/`since`；生成 `Units.*` 只读它即可。
  `KernelManifest::build_with_units(graph, sets, plugins, &registry)` 从调用方的 registry 读单位，`build(...)` 从进程级表读；`Session::manifest()`
  用前者，所以 `Session::register_unit` 注册的单位只进入**该** session 的 manifest。
- `value_kinds: [ValueKindInfo]` 描述每个 `Value` 变体能否入项图（`in_term_graph`）及不能时的替代做法（§13.3.1）。
- `constants: [ConstantValue]` 是物理常数表，字段 `name`/`symbol`/`value: ScaleValue`/`dimension: DimExpr`/`unit`/`exact`/`uncertainty`/`source`；
  `Units.Constants` 由它生成（§13.6.5）。
- `features.plugin_kinds` 说明构建能加载哪种插件（默认 `["in-process"]`；开 `plugin-cdylib` 多一个 `"cdylib"`）；`features.sde` 说明 SDE 噪声原语与方案元数据算子
  可从 manifest 调用（宿主能力名 `core.sde`）。`Features` 属于 manifest：新增字段或算子会改变 `manifest_hash`。

### 13.2.2 生成规则

构建时：

1. 读取 manifest，校验 `api_version`（不兼容报 `E1004`）。
2. 对每个 `OpInfo`：`signature_source = Declared | Overlay | Derived` → 生成 `Rssn.<name>` 类型化声明（**每条 overload 一个**）；`None` → `Rssn.dynamic`；别名 → `Rssn.<alias>`，
   冲突时用 `Rssn.<rule_set>.<name>`。
3. typed 声明记住 `OpInfo.keys` 里对应的 `OpKey`，调用钉死该 overload，静态选择与运行时执行不会分歧。公开形式是 `Rssn.call_key(key, …)`；生成器实际生成等价的
   `Rssn.invoke(name, signature_hash, args)`（同一钉死语义，错误通道不同：生成的 `fn` 必须返回声明类型，所以 kernel 失败会 raise 而不是返回 `Err`）。
4. 生成 `Rssn.Version`、`Rssn.ManifestHash`、`Rssn.Capabilities`、`Rssn.ValueKinds`、`Rssn.Constants`、`Rssn.Units`。
5. 生成 LSP 补全、文档、签名存根，并记录每条声明的来源。

运行 `aevia rssn` 可查询当前的覆盖情况和签名来源。
精度分档（生成器必须区分）：

| 来源 | 精度 | 生成什么 |
|---|---|---|
| `Declared` | 精确 | typed |
| `Overlay` | 精确 | typed |
| `Derived` | 形态级 | typed，但只有 `Real`/`Bool`/`Term` |
| `None` | 无（叶子/无调用） | `dynamic` |

`Derived` 是“由注册信息推出”，不是猜名字：

| 注册信息 | 推出的签名 |
|---|---|
| 有 `eval` | `(Real, …) -> Real` |
| 有 `eval` + `PREDICATE` | `(Real, …) -> Bool` |
| 非叶子 | 另加 `(Term, …) -> Term` |
| `Binder` 在第 k 位 | 第 k 个参数是 `Symbol` |
| `Arity::Variadic` | 尾参数变长 |
| `OpFlags::LEAF` | 无签名（`DynamicReason::OpaquePayload`） |

因此 `sin` 有两条重载 `(Real) -> Real` 与 `(Term) -> Term`；`det` 是 `HEAVY` 请求、没有 `eval`，没有 overlay 时只有 `(Term) -> Term`，`det(matrix)` 的静态类型会是
`Dyn<term>`，overlay 把它补成 `(Tensor<[n,n],Real>) -> Real`。所以“100% typed”不等于“100% 精确”：`Derived` 不区分 `Tensor`/`Quantity`/`Int`，IDE 标“保守推断”。

overlay 也是**维度类型参数**的载体：`sde_wiener`/`sde_levy_areas` 用 `T` 这类维度参数把 `sqrt(h)`（`RootOf{degree:2,index:3}`）与 `Tensor<1, Real[T]>` 写进签名，derivation
推不出这些。生成源的拼写是精确的（§10.0）：`aevia rssn --emit` 产出的 `Rssn.sde_wiener` 是 `(Int, Int, Int, Real[T]) -> Tensor<2, Real[T^(1/2)]>`。`Dyn<term>` 只为确实无法拼写的类型
保留（§10.0）。

**字面量 pinned key 的结果类型化。** 生成器只发 `Rssn.invoke(op, key, …)` 且 `op`/`key` 都是字面量；checker 按 live manifest 的 pinned overload 校验这两个字符串并给出该 overload 的声明结果类型
（`Rssn.det` → `Real`），每次调用取新的维度变量。**非字面量、未知或歧义的 key** 保持动态 `Value`，诊断由运行时给出：`E1005`（未知/不匹配的 `signature_hash`）、`E1002`（歧义）、
`E1003`（动态参数或非函数值）。checker 不重新解析或执行 key。

SDE 求解循环不在 manifest 里（host 回调，kernel CO-03）；宿主侧 `sdeint_*` 不经过 `Rssn` 算子通道，`sde_scheme_info` 是 manifest-only 的元数据查询（只给强阶，弱阶是宿主侧 `sde_order`，§8.9.1）。

**仓库规则。** Aevia 仓库不允许出现按算子名手写的绑定文件，也不保留 `signatures.overlay.json` 的副本（它是 rssn 的构建输入，已并入 `OpInfo.overloads`）。只允许生成器
（`aevia rssn --emit`，在 `aevia-rssn` 内；标准库的生成视图由 `aevia-stdlib-gen` 写出）。CI 检查：生成器输出与提交的生成文件一致；manifest 快照变化必须显式更新。

### 13.2.3 命名空间

- 普通算子：`Rssn.<name>`（`Declared|Overlay|Derived` 都 typed；只有 `None` 与插件算子走 `Rssn.dynamic`）。
- 规则集限定：`Rssn.<rule_set>.<name>` 只是命名空间；运行时仍按 name/arity。rssn 的 `call_value` 只按 name/alias 查找，**不支持** `rule_set/name`；要钉死重载用
  `Rssn.call_key(key, args)`，`key` 来自 `Rssn.overloads(name)` 或 `OpInfo.keys`。
- 插件：`Rssn.<plugin>.<op>`。
- 领域模块：`import Rssn_NumberTheory`（`Rssn_<Domain>`，CamelCase）给出按数学领域分组、带类型与 `Result` 通道的包装器（§10.3.8）；`Rssn_<rule_set>` 仍按 kernel 分组并保持可用。

名字冲突时，短名只保留**最稳定**的那个（`Stable` 优先，`since` 早者优先），其余用限定名；生成器在 manifest 中记录短名映射。

---

## 13.3 动态值与动态调用

### 13.3.1 `Value`

```aevia
type Value =
    | VInt(Int)
    | VReal(Real[1])
    | VFloat(Float[1])
    | VComplex(Complex[1])
    | VBool(Bool)
    | VString(String)
    | VSymbol(String)                 # 自由符号，不是 String
    | VUnit(Unit[1])
    | VQuantity(Quantity)
    | VTensor(TensorData)             # shape + 行主序 Value
    | VSparse(SparseData)
    | VRecord(Record)
    | VVariant(Variant)
    | VTerm(TermRef)                  # owned handle
    | VFunction(Callback)
    | VDyn(Dyn<Real>)
    | VOpaque(Opaque)
```

`Value` 是**动态边界**的类型，不是核心语言的隐式子类型；它只出现在 `Rssn.dynamic`、插件、FFI、反射 API 中。

**可入项图性**由 `manifest.value_kinds` 给出，Aevia 不维护第二份。当前（17 个变体）：

- `in_term_graph = true`（10）：`VInt`/`VReal`/`VFloat`/`VBool`/`VString`/`VSymbol`/`VQuantity`/`VTensor`/`VTerm`/`VDyn`；
- `in_term_graph = false`（7）：`VComplex`（仅虚部为 0 时降为 `Float` 入图，否则 `Unsupported("complex literals")`）、`VUnit`（单位是定义不是数，过边界用
  `quantity(value, unit)`；只有已声明的单位参数位置才直接收 `Value::Unit`）、`VSparse`/`VRecord`/`VVariant`/`VFunction`/`VOpaque`。

`false` 的变体只能喂给有 `eval_dyn` 的算子或作为返回值；生成器不能假设所有 `Value` 都能参与重写。

### 13.3.2 调用

```aevia
Rssn.call    : (String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.dynamic : (String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.typed   : (String, Tensor<1, Value>, TypeRep) -> Result<Value, OpError>
Rssn.overloads       : String -> Tensor<1, String>
Rssn.resolve_overload: (String, Tensor<1, Value>) -> Result<String, OpError>
Rssn.call_key        : (String, String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.Constants       : Tensor<1, String>     # 常数符号；数值在生成的 Constants 模块
Rssn.Units           : Tensor<1, String>     # 单位名；单位值在生成的 Units 模块
```

- `Rssn.call` 是 `Rssn.dynamic` 的别名，只按短名/别名查找；短名歧义报 `E1002`。
- `Rssn.call_key` “不解析、直接执行”：收 name + `signature_hash`（32 hex 字符），合起来是 `OpKey` 的可写形式；全零表示该算子没有声明重载。key 与当前 manifest 不符报
  `E1005`（提示重新生成模块）；kernel 侧参数偏离钉住签名时报 `UnknownOp`，在 Aevia 边界变成错误值。
- `Rssn.resolve_overload` 暴露运行时选择结果，供缓存/诊断用：成功返回 `"rule_set.name@signature_hash"`；并列与“没有签名接受”都报 `OpError::Ambiguous`。`Rssn.overloads` 返回每条
  overload 的渲染签名。
- 参数用 `to_value(x)` 注入，返回值用 `from_value(v)` 转回静态类型。无签名的算子、插件算子、实验性入口走 `dynamic`；标准集当前没有 `None`，所以 `dynamic` 在标准集上只是插件通道。
- `type_rep<T>()` 保存检查器解析出的具体类型；它是不透明值，普通记录不能伪造。`Rssn.typed` 执行动态调用后按该见证检查表示、形状及量纲；不匹配或缺少证据返回 `Err(OpError)`。
  成功结果仍是 `Value`，经 `from_value<T>` 验证并转换；未经运行时类型字典具体化的泛型不能生成 `TypeRep`。
- 高阶算子用 `Rssn.call_ho(name, args, fns)`；`to_callback` 从已检查源码闭包生成不透明回调，限制见 §8.19。回调在原线程同步执行，外层调用结束即失效；
  跨线程调用和同作用域重入被拒绝，回调错误经 `OpError` 传回；空列表就是一次动态调用。
- Aevia 通过 `@op`、`@rule` 和 `load_plugin` 发布源码扩展（§13.5.2）。`Rssn.register` 与 `Rssn.register_plugin` 对应仅供宿主使用的 `Plugin` 接口，传入 Aevia 值时报 `E0302`。`core.plugin.source` 表示支持源码扩展，`core.callback` 表示受限回调桥可用。

### 13.3.3 错误

```aevia
type OpError =
    | UnknownOp(String)
    | Arity(Int, Int)
    | Type(TypeRep, TypeRep, Int)
    | Ambiguous(String, Int)         # name, 并列数
    | Dimension(DimError)
    | NotNumeric(String)
    | NotReduced(String)
    | Domain(String)
    | BudgetExceeded
    | Unsupported(String, String)   # backend, feature
    | Plugin(String)
    | ForeignTerm(String, String)   # expected session, got session
    | Codec(String)
    | Host(String)
    | Internal(String)
```

`E1003` 是“没有匹配的算子/重载”，携带候选签名列表；`E1001` 是“后端 capability 缺失”，携带 capability 名和最小 rssn 版本。

---

## 13.4 重载、签名与解析

### 13.4.1 重载

rssn 允许同名多签名，判定在 `kernel::overload` 里：

```aevia
type Match = | No | Generic | Shaped | Widened | Exact   # 有序
matches_type : (TypeExpr, TypeExpr) -> Match
matches      : (TypeExpr, Value) -> Match
score        : (Signature, Tensor<1, Value>) -> Option<Nat>
score_types  : (Signature, Tensor<1, TypeExpr>) -> Option<Nat>
best         : (Tensor<1, Signature>, Tensor<1, Value>) -> Best
best_types   : (Tensor<1, Signature>, Tensor<1, TypeExpr>) -> Best
type Best    = | None | Unique(Int) | Ambiguous(Tensor<1, Int>)
```

- 每个参数单独分类，整个调用的分数是**最弱**的那个参数。
- `Int -> Real`、`Int|Real|Float -> Complex` 是 `Widened`，反向不成立；同构造器但元素/量纲未被签名钉死是 `Shaped`；类型变量和 `Dyn` 是 `Generic`。
- `best` 内部并列最高 → `Ambiguous`，不静默取第一个。
- `score_types`/`best_types` 与 `score`/`best` 共用同一取分折叠：Aevia 在编译期解析一次，得到的数与 kernel 运行时一致。

标准集里每个名字只有 1 个注册，但**每个注册内部有 1–5 条重载**（有 `eval` 的算子同时有数值与符号两条，overlay 再补精确签名）。两件事要分开：

- **注册选择**（`resolve_op`）在 `fitting.len() == 1` 处直接返回，对标准集不评分；跨注册并列只可能由插件触发。
- **重载选择**（`resolve_overload` → `overload::best`）对注册内的签名真正执行：`sin(0.5)` 选 `(Real) -> Real`，`sin(x)`（`x` 是 `Symbol`）选 `(Term) -> Term`。
  但实参本身是 `Term`/`Dyn` 时，数值重载也会以 `Generic` 通过，两条同分 → `Ambiguous`；所以 `resolve_overload` 只适合具体类型的实参，符号表达式用 `OpInfo.keys` 的 key。

`call_value` 按 name/arity 选注册，再调 `check_args`：没有任何 overload 接受参数时返回 `OpError::Type`。但符号重载的 `Dyn<term>` 对任何值都是 `Generic`，所以 derived 算子几乎总能通过——
这条检查只在签名全部具体（`eq`/`lit`/`pow`）时真正拦得住；它不在数值/符号之间做选择（由算子的 `eval` 决定）。

Aevia 自己的静态解析规则（与 rssn 的格同构，供 Aevia 自己的重载/插件使用）：

1. 按 arity 过滤；2. 逐参数分类，取最弱；3. 分数最高且唯一 → 选中；4. 并列最高 → `E1002`；5. 没有匹配 → 尝试 `Dynamic`；仍失败 → `E1003`。

运行时三条入口：

- `Session::call_key(&OpKey, …)` **不重新解析**，按 `rule_set` + `signature_hash` 找注册，钉死重载，并要求参数能被钉住签名接受（`overload::score(pinned, args).is_some()`），否则 `UnknownOp`。
- `Session::call_value(name, …)` 按 name/arity 找注册；多个候选时用 `overload::score` 选最具体，并列报 `OpError::Ambiguous`；参数不被接受报 `OpError::Type`。
- `Kernel::call(&OpKey, …)` 转发 `call_key`，因此也钉死重载；经 `dyn Kernel` 传递不丢信息。

`Session::resolve_overload(name, args)` 用于“先解析、再缓存”：`best` 为 `Unique` 返回带 `signature_hash` 的 key，可直接给 `call_key`；算子完全没有签名返回零 hash key（`call_key` 接受）；并列或没有任何签名接受
都是 `Err(OpError::Ambiguous)`，**不会**给出不可执行的零 hash key。所以返回值总可以安全交给 `call_key`。`OpKey.signature_hash` 是 `Signature::hash()` 的前 16 字节（确定性 bincode 的 BLAKE3），
伪造/过期/参数偏离的 key 返回 `UnknownOp`。

### 13.4.2 签名覆盖

覆盖率 100%（`aevia rssn` 报 `0 dynamic-only`）。来源优先级从高到低：

1. `Declared`：`OpDescriptor::new(...).signature(...)`，算子自己写；
2. `Overlay`：`signatures.overlay.json` 里人工写的修正（在 rssn 侧并入 manifest，Aevia 不持有副本）；
3. `Derived`：`kernel::infer::derive` 从注册信息推出；
4. `None`：叶子/无调用形式，`dynamic_reason` 说明原因（标准集没有）。

规则：`Derived` 只读 `eval`/`PREDICATE`/`Binder`/`Variadic`/`LEAF`，不读算子名，所以保守但可靠；精度不够时写 overlay，而不是改推导器（overlay 只用于“这条签名必须精确”的算子：
张量、符号、单位参数）；CI 的 `signature-audit` 报告覆盖率与来源分布，`Derived` 占比高不阻止构建，但阻止把对应算子标成“精确”；插件用 `Registry::register_op` 声明签名，
`Registry` 拒绝“既无签名又无 `dynamic_reason`”的注册。这让“能力完整”和“类型精度”分开推进，而不会有算子被静默遗忘。

---

## 13.5 插件与用户扩展

### 13.5.1 插件

Aevia 源码扩展由含 `@op` 或 `@rule` 注解函数的模块定义，使用 §7.5.1 的普通包清单。加载模块时发布这些声明，示例见 §13.5.2。

RSSN 宿主通过 `Session::register(Box<dyn Plugin>)` 注册进程内 Rust 插件。宿主插件可以提供算子、签名、求值器、重写规则、量纲规则、单位、基量纲和 JIT 代码生成规则。启用可选的 `plugin-cdylib` 特性后，RSSN 宿主也可通过 `Session::load_plugin` 加载动态库。原生入口为 `rssn_plugin_v1`，其 ABI 通过字节缓冲区传递 bincode 编码的 `Vec<Value>` 参数和 `Result<Value, String>` 结果。原生加载器检查插件 ABI 版本和描述符。Aevia 源码使用基于模块的扩展接口。

### 13.5.2 Aevia 函数作为 rssn 算子

`@op` 把 Aevia 函数发布为 rssn 算子，`@rule` 发布为重写规则：

```aevia
@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))

@rule("mydomain/sinh-exp")
fn sinh_exp(x: Real[1]) = (sinh(x), (exp(x) - exp(-x)) / 2)
```

**决定：用追踪而不是回调。** kernel 的扩展语言就是它的项语言：算子是定义 `name(a, b) := body`，规则是重写 `name: lhs => rhs`。Aevia 函数以**追踪**变成它们：
用带各参数已检查量纲的新符号调用函数，它构造的项就是定义体。这不需要第二个编译器，并继承 kernel 的全部能力——kernel 可以调用它、对它求导、穿过它化简与积分、数值求值，
这是不透明的宿主回调做不到的（回调也不能跨线程，`Rc` 解释器不是 `Send`）。

- 代价是被检查的：结果依赖于对参数的控制流（`if x > 0`、对值的 `match`）或做 IO 的函数追踪不出项，注册报 `E1006` 并给出原因。
- 规则函数返回二元组 `(lhs, rhs)`，参数就是规则的模式变量；`lhs` 必须是算子应用（追踪时会被局部化简，所以 `x * 0` 不是合法的左端）。
- 发布发生在模块函数绑定之后、顶层 `let` 求值之前，所以顶层绑定已可经 `Rssn.dynamic("mydomain/softplus", …)` 调用。每个加载的模块注册一个进程内插件 `aevia/<module>#n`，
  属于当前线程的 kernel 会话。
- **源码算子是一等算子。** 发布顺序：先把每个 `@op` 追踪成 kernel 定义，并把检查得到的参数/结果量纲作为**量纲签名**一并注册（`Registry::register_dim_signature`：参数与结果是签名变量的单项式，
  每次使用由实参量纲求解；`fn area(r: Real[d]): Real[d^2]` 的 `demo/area(r)` 在 kernel 的量纲推断里是 `dim(r)^2`）；再把模块里该函数的绑定换成包装：**全部实参是数值**
  （或无自由符号的闭式项）时执行 Aevia 函数体，**有符号实参**时构造算子结点 `demo/softplus(x)`，结果量纲由签名给出，实参量纲与签名冲突是 `E0401`；最后才追踪 `@rule`，所以规则可以谈论
  源码算子。`diff`/`simplify`/`integral` 作用在算子结点上时，kernel 看到的是算子、它的定义和关于它的全部规则。kernel 拒绝插件时恢复原绑定。
- **动态加载**：`load_plugin(path) : String -> IO (Result<Int, String>)` 在运行期加载、检查、求值一个源码文件（含其导入），发布其中的 `@op`/`@rule`，返回发布数量；需要 `fs` 能力。

`@op("mydomain/softplus")` 里的 `mydomain/` 是**算子名的一部分**（插件命名空间），不是 `rule_set/name` 限定符；kernel 的定义头允许 `/` 分隔的标识符段，`call_value` 按完整字符串查找。

### 13.5.3 安全

发布包时，注册中心验证发布者的签名（§13.7）。`load_plugin` 接收本地源码路径，并信任指定的文件及其导入。调用方需要 `fs` 能力打开文件，求值模块的解释器继承调用方的能力掩码。只加载可信的本地源码。源码插件和 RSSN 原生插件都在宿主进程内执行。执行不可信代码时使用进程隔离和操作系统资源限制（§9.14）。

---

## 13.6 量纲与单位：统一到 `rssn::dim`

### 13.6.1 规范层

`rssn::dim` 提供：`DimTerm`（基量纲/变量的有理指数乘积，结构化访问 `factors(&arena)`/`vars(&arena)`）、`DimArena`（interning，可快照/恢复）、
`UnitRegistry`（单位名、别名、精确尺度 `Scale { rat, pi }`、仿射 offset、前缀、显示、类别、since），以及 `Dimension`（**可打印快照**，指数是字符串；精确比较/计算用 `DimTerm`，
不要解析 `Dimension.factors`）。

Aevia 的 `Dim` 是 `DimTerm` 的类型级镜像；两者的规范形、基名、指数运算必须一致。编译器不复制单位表；`Units.*` 是 `manifest.units` 的生成视图（当前 82 项）。
单位属于 **session**：`Session::register_unit(UnitDef)` 写该 session 的 registry，`build_with_units` 才把它写进 manifest；进程级 `build` 看到进程表；插件注册的单位只属于安装插件的 session。

结构化入口是 `Config::dimension_term(symbol, DimExpr)`：不解析文本，未知基名被 intern，结构化假设在文本假设之后应用（同名时结构化胜）。`DimExpr::to_term` 写 term；`try_from_term`
是 term 可表示量纲的精确逆（`Length^n` 读回 `Length^n`）；`from_term` 是全函数版，读不回时给 `DimExpr::Any` 而不是错误量纲。

`OpInfo.dim_rule` 的 `DimRuleInfo` 有 `RootOf { degree, index }`：结果量纲是**第 `index` 个参数**量纲的 `degree` 次根（旧 `Root(n)` 固定取第 0 个参数，表达不了
`sde_wiener(seed, m, steps, h)` 的 `sqrt(h)`）；manifest wire form 是 `{"root_of":{"degree":2,"index":3}}`。宿主读 `dim_rule` 时应按变体匹配，不要假设只有 `Root`/`Same`/`Product`。

自定义基量纲由 `Config::base_dimension(name)` 声明（幂等）：把基名送进 `DimArena`，`Dim` 的内部表示从固定的 7 个 SI 指数扩成“7 个 SI + 有序旁表”，`expr_to_dim` 接受任意基名；
这是 `unit EUR : Currency` 能成立的前提（`dimension_term` 只能为 arena 里已有的基名产出 `DimTerm`）。

### 13.6.2 单位声明

```aevia
dimension Currency            # 自定义基量纲
unit USD : Currency = 1       # 基单位
unit EUR : Currency = 1.08 USD
unit myunit : Length = 0.5 m  # 内置基量纲
```

生成器为每个 `dimension` 在 `Config` 上声明一次 `base_dimension("<Name>")`，然后才注册单位；否则单位注册成功但 `DimTerm` 无法生成。

- `dimension` 注册到 `DimArena`（编译期经 `DimExpr`，运行时经 `Config::dimension_term` / `Graph::assume_dimension_value`）；`unit` 注册到 `UnitRegistry`。
- 默认导出，`private` 只在本模块可见；导出信息进入 manifest，下游包可以直接 `import`。
- 用户单位可以是仿射的（`unit °X : Temperature = 1 K offset 273.15`），但仿射单位只能单独使用。
- `features.user_base_dimensions` 是硬编码 `true`（每个构建都支持）；按构建变化的只有 `jit*` 与 `plugin_kinds`。

### 13.6.3 `Dyn`

`Dyn<ρ>` 的运行时量纲是 `DimTerm`，不是字符串。`dyn`/`check` 是类型指导原语：具体 `d` 在编译期物化为 `DimTerm`；泛型用 `dyn_with`/`check_with` 显式传入；`check` 返回 `Result`。

### 13.6.4 `Point` 与仿射

`Point[d]` 是静态包装；运行时是 `Float` + `UnitId`。`UnitRegistry` 的 `affine`/`offset` 是唯一事实来源。`Point` 的算术规则在 Aevia 层实现，rssn 的 `quantity`/`convert` 负责显示/换算。

### 13.6.5 物理常数

`manifest.constants` 是 `Vec<ConstantValue>`（当前 30 条、18 条精确）：

```rust
pub struct ConstantValue {
    pub name: String,        // "speed of light in vacuum"
    pub symbol: String,      // "c"
    pub value: ScaleValue,   // 精确有理数，相干 SI
    pub dimension: DimExpr,
    pub unit: String,        // "m/s"
    pub exact: bool,
    pub uncertainty: Option<ScaleValue>,
    pub source: String,      // "SI 2019 definition" / "CODATA 2022"
}
```

`Units.Constants` 由它生成：符号原样使用（`c`、`g_n`、`h`、`hbar`、`k_B`、`N_A`、`e`、`G`、`alpha`、`R`、`F`、`sigma_SB`、`m_e`、`m_p`、`m_n`、`a_0`、`R_inf`、`mu_0`、`epsilon_0`、
`mu_B`、`M_u`、`m_u`、`T_tp`、`atm`、`G_0`、`K_J`、`R_K`、`Phi_0`、`K_cd`、`dnu_Cs`）；`exact = true` 的没有 `uncertainty`；常数是**值**，不是单位，不能出现在 `quantity(v, u)` 的单位位置；
`e` 是基本电荷，与 `Math.e` 同名，两者同时可见时必须限定（`Math.e` / `Units.Constants.e`）。

### 13.6.6 线程与 arena 隔离

`DimArena` 在模板里是 `Arc<RwLock>`，`Graph::clone` **共享**它；写入时 `with_dim_arena_mut`/`define_base_dimension` 看到 `Arc::strong_count > 1` 就克隆再写（写时复制）。因此：
`Config::base_dimension(name)` 是安全路径（`prepare` 在克隆图上定义，请求之间互不影响）；不定义基量纲的请求共享模板 arena，不付拷贝成本；不要直接对 `Session` 的模板图调用
`define_base_dimension`；跨线程仍用 `Session::fork`/`merge`，不共享 `Session`（`RefCell`）。

---

## 13.7 中心包 registry

registry 是**内容寻址 + 签名 + 能力声明**的包索引，服务实现在 `platform/`（`aevia-registry-*` 与 `aevia-pkg`），部署为 `aevia.apich.org`。
CLI 提供 `aevia add owner/name[@req]`、`aevia update`、`aevia fetch`、`aevia tree`（均支持 `--offline`）与 `aevia pkg publish/yank/search/verify`；依赖解析使用 SemVer 2.0 与锁文件校验（§7.5.1/§7.5.2），git 依赖与未知清单键报 `E0605`。

### 13.7.1 包清单

```toml
[package]
name = "aevia-thermo"
version = "0.3.0"
edition = "2026"
license = "MIT"
content_hash = "blake3:..."

[deps]
aevia-std = "^1.0"

[compat]
aevia = ">=1.0, <2.0"
rssn_api = "1"

[capabilities]
requires = ["fs"]
provides = ["thermo"]

[operators]
exports = ["thermo/enthalpy", "thermo/entropy"]

[units]
exports = ["J", "kJ", "cal"]

[dimensions]
exports = ["Entropy"]
```

### 13.7.2 协议

逻辑操作（HTTP 路径以 `platform/` 的 `/api/v1` 为准）：

| 操作 | 说明 |
|---|---|
| 读取索引 | 全部包名 + 最新版本 + 摘要；带 `ETag` |
| 下载包 / 签名 | 内容寻址的归档，下载时校验存储字节的摘要 |
| 搜索 | `aevia pkg search` |
| 发布（签名） | 校验：不得有纯 path 依赖、不得依赖自身、依赖必须存在且有未 yank 的满足版本；版本允许预发布、拒绝 build 元数据；发布在一个事务内 |
| 撤回（yank） | 不删除内容，只阻止新解析 |
| 解析 / 被依赖 | 服务端解析依赖；列出某包的依赖者 |

索引项至少包含：name、version、content_hash、deps（`package`/`req`/可选 `import`）、aevia_range、rssn_api、capabilities、operators、units、dimensions、stability、yanked。

### 13.7.3 可复现

包内容哈希覆盖源码、manifest、生成的 `Rssn` 模块签名；构建不执行任意脚本；依赖解析使用内容哈希 + semver；registry 镜像必须可验证签名。

---

## 13.8 稳定性与 capability（取代 v1/v2）

### 13.8.1 稳定性标记

```aevia
@stable
@experimental
@deprecated("use foo2")
```

`Stable`：major 内签名/语义不变；`Experimental`：可改，但旧名至少保留一个 minor；`Deprecated`：至少一个 minor 的警告期。

### 13.8.2 Capability

后端能力用 capability 名描述。**名字的唯一来源是 capability 表**：由 manifest 的 `Features` 派生，`Rssn.Capabilities` 只列 `enabled = true` 的行（`aevia-rssn` 的 `Capabilities::table()`）。完整表：

```
core.dynamic_call
core.jit                    # false：默认构建无 jit feature
core.jit_branch             # false（同上）
core.jit_index              # false（同上）
core.parallel_symbolic
core.higher_order
core.structured_results
core.sde                    # 由 features.sde 派生
core.callback               # 同线程作用域回调，支持具体结构化签名
core.plugin.source          # 源码级 @op/@rule/load_plugin（需 in-process 插件）
core.plugin
core.plugin.in-process
core.plugin.cdylib          # 仅 plugin-cdylib 构建
dim.exact_units
dim.affine_units
dim.user_base_dimensions
```

默认构建 `Rssn.Capabilities` 实际列出十二行（除 `jit*` 三行与 `plugin.cdylib` 外全部）。另有**规范声明、但 manifest 没有对应 `Features` 字段**的名字，可出现在诊断里，
`Rssn.Capabilities` 不会列出：`core.symbolic`、`core.numeric`、`core.tensor`、`core.linalg`、`core.ode`、`core.pde`、`core.fft`、`core.stats`、`core.units`、`core.assume.general`、
`core.gpu`、`core.distributed`、`io.binary`。

`storage.sparse`（§13.9.5/§8.4.1）是 Aevia 自己的存储类，已启用，不由 manifest 的 `Features` 发布，所以不出现在 `Rssn.Capabilities` 里。

Aevia 编译器在类型检查后检查 capability；缺失时：静态可知（如 `@jit` 目标）→ 编译期 `E1001`；动态调用 → 运行时 `OpError.Unsupported`，Aevia 转成 `E1001`。
**不删语言特性**：GPU 未实现时 `on(Gpu(0), ...)` 仍类型正确，运行时报 `E1001("core.gpu")`。

---

## 13.9 已决定的扩展设计

### 13.9.1 形状细化

**决定：支持，作为 refinement，不是完整依赖类型。**

- `Tensor<n, τ>` 只带秩；`Tensor<[n1, ..., nk], τ>` 是形状细化，`n_i` 是类型级 `Nat`，可用 `?` 表示未知；`Matrix<n, m, τ>` 是 `Tensor<[n, m], τ>` 的别名。
- 张量字面量自动获得细化形状；`zeros_of<[3,4]>()` 等可显式指定。`matmul : Tensor<[m,k],τ> × Tensor<[k,n],τ> -> Tensor<[m,n],τ>`；秩-only 版本保留，运行时检查。
- `reshape<[n1, ..., nk]>(a)` 始终返回 `Result`：形状静态已知时元素数在编译期检查、结果恒为 `Ok`，否则运行时检查；`reshape_dyn` 总是运行时检查。
- 索引：细化形状 + 字面量索引 → 编译期越界检查；动态索引仍运行时检查。
- 细化 → 秩-only 是安全隐式转换；秩-only → 细化用 `refine_shape` 返回 `Option`/`Result`。
- 类型级 `Nat` 支持字面量、`+`、`*` 和线性等式；不做一般递归/证明，足以表达矩阵乘法、reshape、拼接。

### 13.9.2 `Point` 张量

**决定：支持 `Tensor<shape, Point[d]>`。** 元素级 `+`/`-` 按点规则（`Point - Point -> Float[d]`，`Point + Float[d] -> Point`，`Point + Point` 报 `E0405`）；`mean` 只接受 `Float`/`Real`，点的平均用
`centroid : Tensor<1,Point[d]> -> Point[d]`；温度场可以直接是 `Tensor<[nx,ny], Point[Θ]>`；存储是 `Float` + `UnitId`，仿射偏移只在元素级运算时处理。

### 13.9.3 `Table` 的行类型参数

`Table<ρ>` 的行类型参数由程序的类型上下文确定。外部数据在运行时提供列名、单元格和单位元数据。使用 `schema` 查询表格的运行时结构，表格操作见 §8.11。

### 13.9.4 设备、GPU、分布式

**决定：设备是运行时值，不是张量类型参数。**

- `Device = Cpu | Gpu(Int) | Distributed(Cluster)`；`Tensor` 类型不变，值带 `device` 元数据。
- `to_device : (Tensor<n,τ>, Device) -> IO (Tensor<n,τ>)`；`on : (Device, () -> a) -> IO a` 是作用域执行（分配/传输是 IO）。
- `@jit(target = Gpu(0))` 指定 JIT 目标；没有 `core.gpu` 时报 `E1001`。分布式张量用库类型 `Sharded<Mesh, Tensor<...>>`，`Mesh` 是运行时拓扑。这样不为设备/分片生成类型组合爆炸。

`@jit(target = Cpu)` 与 `@jit` 等价。请求不可用的 GPU 或分布式目标时，检查阶段报 `E1001`。目标参数无效时，报 `E0210`。用常量调用构造器（如 `Gpu(0)`）可用作常量属性参数（§2.2.7）。RSSN 的宿主设备 API 见 §10.6.5。

### 13.9.5 稀疏与布局

**决定：`Sparse<F, shape, τ>` 是核心类型构造器，`F` 是格式类型。**

- `Csr`、`Csc`、`Coo` 和 `Bcsr` 构成封闭的格式集合。`Block` 是 `Bcsr` 的同义写法。
- 布局操作包括内建函数 `nnz`、`to_dense`、`from_dense`、`matmul` 和 `transpose`。Aevia 按支持的格式和操作数类型分派。
- `@` 按布局分派（dense×sparse、sparse×dense、sparse×sparse）；`Sparse` 与 `Tensor` 之间只有显式转换。rssn 的 `TypeExpr::Sparse` 只有 `rank`，所以 `Sparse<F,shape,τ>` 在边界降为
  rank + 运行时 shape；细化形状由 Aevia 自己检查与传播。

`storage.sparse` 能力涵盖四种格式、预条件与非对称迭代解法、切片和张量收缩。量纲和错误见 §8.4.1。稀疏值以结构化值通过动态内核边界。

### 13.9.6 效果与能力

**决定：不做用户自定义 effect handlers；`IO` 保持单一类型。** 扩展点是算子/插件/registry，不是语言级效果系统。`IO` 动作可声明所需 capability（`fs`、`net`、`env`、`rand`、`gpu`）——这是运行时
权限，不是类型参数；`with_cap(cap, thunk)` 在沙箱中执行，没有能力时 IO 动作失败。这避免 effect row 侵入全部标准库签名。需要 DSL/代码生成时用插件/算子，不用宏。

`with_cap(caps, thunk)` 与 `--caps` 选项的语义见 §9.14。后端能力表（§13.8.2）说明工具链提供的功能。

### 13.9.7 宏与语法扩展

**决定：不提供用户宏；语法固定。** 扩展通过算子、插件、registry、`Rssn` 动态调用完成；不引入 `macro_rules`/卫生宏，避免工具链分叉。若将来确需，必须以独立、可选的预处理层出现，不进入核心语法；
当前规范不预留语法。

### 13.9.8 角度与不确定度

- **`Angle` 不是基量纲**：`rad` 无量纲，`deg` 是尺度；`sin`/`cos` 接受 `Real[1]`，`deg` 字面量折叠为 `pi/180`；不引入 `Angle` 基量纲，避免破坏 `exp(i*θ)`、`sin(ωt)` 等公式。
- **`Measurement<d>` 是标准库类型，不是核心类型**：独立误差传播见 §8.14；相关误差用 `Covariance<d,e>` 库类型；核心语言只保证量纲。

### 13.9.9 其他已关闭的问题

见 §12.9 的决策表（`det`、`Real`/`Float` 混合、`Dyn` 错误、`RateOf` 嵌套、`Table` schema、`@`/`*`、`%`、`let` 递归、顶层 IO、registry、量纲别名导出）。

---

## 13.10 兼容性与迁移

- **语言版本**用 semver，不设“v1 语言/v2 语言”：major 才允许破坏性变更；minor 只加能力/算子/库；patch 只修 bug。
- **manifest 兼容**：`api_version` 不兼容 +1；Aevia 编译器 pin 范围；`manifest_hash` 变化使缓存失效，但旧签名仍可用到下一个 major。
- **算子弃用**：`Deprecated` 至少一个 minor 的警告；别名保留。
- **包 registry**：内容哈希 + 签名 + capability；yank 不删除内容，只阻止新解析。
- **迁移**：不强制用户重写代码；弃用算子保留别名，旧签名在 pin 的 `api_version` 内保持可用。当前没有自动迁移命令。

## 13.11 错误码

| 代码 | 情况 |
|---|---|
| `E1001` | 后端 capability 缺失（`core.gpu`、`core.jit_branch` 等） |
| `E1002` | 算子/重载歧义 |
| `E1003` | 没有匹配的算子/重载 |
| `E1004` | rssn `api_version` 不兼容 |
| `E1005` | manifest/签名哈希不匹配 |
| `E1006` | 插件 ABI/加载失败；`@op`/`@rule` 追踪失败 |

这些错误属于**环境/后端**，不是用户程序类型错误；诊断必须给出 capability 名、最小 rssn 版本和可执行的替代方案。


<a id="ch-appendix-a-grammar"></a>

# 附录 A — 完整文法

本附录是规范性 EBNF。记法：

- `{ X }` 零次或多次；`[ X ]` 可选；`|` 选择。
- 终结符用 `"..."`；`NEWLINE`、`INDENT`、`DEDENT`、`EOF` 是布局记号
  （§1.4）。
- `INT`、`FLOAT`、`STRING`、`IDENT`、`UIDENT`、`UNIT_NAME`、`SUPERSCRIPT`、
  `LT_APP`、`ELLIPSIS` 是词法记号（§1.2）。`UIDENT` 首字符是 ASCII 大写，其余名字是
  `IDENT`；`qname` 是限定名。

## A.1 布局

```
NEWLINE   ::= 每个逻辑行末尾
INDENT    ::= 缩进增加时
DEDENT    ::= 缩进减少时
```

布局算法见 §1.4。块与布局组都由 `NEWLINE INDENT … DEDENT` 表示：

```
block        ::= NEWLINE INDENT { block_stmt } expr NEWLINE? DEDENT
layout_group ::= NEWLINE INDENT expr NEWLINE? DEDENT
block_stmt   ::= let_decl | fn_decl
```

解析器在 `NEWLINE INDENT` 后看到 `let`/`fn` 选 `block`，否则选 `layout_group`；`NEWLINE?`
允许内层块已消费行尾。

## A.2 程序与声明

```
program        ::= [ module_decl ] { import_decl } { top_decl } EOF

module_decl    ::= "module" module_path NEWLINE
module_path    ::= UIDENT { "." UIDENT }

import_decl    ::= [ "export" | "pub" ] ( "import" | "open" ) module_path
                   [ "as" UIDENT ] [ "{" name_list "}" ] NEWLINE
name_list      ::= name { "," name }
name           ::= IDENT | UIDENT

top_decl       ::= { attribute } [ "private" ] decl
attr_arg       ::= [ IDENT "=" ] expr
attribute      ::= "@" IDENT [ "(" [ attr_arg { "," attr_arg } ] ")" ] NEWLINE?
decl           ::= dimension_decl
                 | unit_decl
                 | type_decl
                 | class_decl
                 | instance_decl
                 | derive_decl
                 | fn_decl
                 | let_decl
                 | test_decl

dimension_decl ::= "dimension" UIDENT [ "=" dim ] NEWLINE

unit_decl      ::= "unit" unit_name ":" dim "=" unit_rhs
                   [ "prefixable" ] NEWLINE
unit_rhs       ::= unit_scale [ unit_expr ] [ "offset" unit_scale unit_expr ]
unit_scale     ::= [ "-" ] number [ "/" number ]
                 | "pi" [ "/" number ]
unit_name      ::= UNIT_NAME | IDENT | qname

type_decl      ::= "type" UIDENT [ type_params ] [ "=" type_body ] NEWLINE?
type_body      ::= type
                 | "|" constructor { "|" constructor }
                 | NEWLINE INDENT { "|" constructor NEWLINE } DEDENT
constructor    ::= UIDENT [ "(" [ type { "," type } [ "," ] ] ")" ]

type_params    ::= "<" type_param { "," type_param } ">"
type_param     ::= IDENT [ ":" kind ]
kind           ::= "Type" | "Dim" | "Nat" | "Row" | "Rep" | "Format" | "Shape"

class_decl     ::= "class" UIDENT type_param { type_param } "where"
                   NEWLINE INDENT { method_sig NEWLINE } DEDENT
method_sig     ::= IDENT ":" type
                 | IDENT "(" [ params ] ")" [ ":" type ]

instance_decl  ::= "instance" UIDENT "for" type
                   [ "where" NEWLINE INDENT { method_def NEWLINE } DEDENT ]
method_def     ::= IDENT "(" [ params ] ")" [ ":" type ] "=" expr NEWLINE?

derive_decl    ::= "derive" name_list "for" type NEWLINE

fn_decl        ::= "fn" IDENT [ type_params ] "(" [ params ] ")" [ ":" type ]
                   "=" expr NEWLINE?
params         ::= param { "," param } [ "," ]
param          ::= pattern [ ":" type ]

let_decl       ::= "let" [ "rec" ] pattern [ ":" type ] "=" expr NEWLINE?

test_decl      ::= "test" STRING "=" expr NEWLINE?
```

- `type_body` 的单行变体 MUST 以 `|` 开头以区别于别名；缩进形式可省略首个 `|`。
- 声明末尾的 `NEWLINE?` 允许表达式是块。
- `import M` 只绑定限定名；`import M { … }` 与 `open M { … }` 选择性引入名字；`open M` 打开
  全部导出（§7.2）。

## A.3 表达式

```
expr           ::= let_expr
                 | if_expr
                 | match_expr
                 | fn_expr
                 | do_expr
                 | annot_expr
                 | block
                 | layout_group

let_expr       ::= "let" [ "rec" ] pattern [ ":" type ] "=" expr "in" expr
if_expr        ::= "if" expr "then" expr "else" expr

match_expr     ::= "match" expr "with" match_body
match_body     ::= NEWLINE INDENT { match_arm } DEDENT
                 | NEWLINE { match_arm }          # 分支与 match 同列
                 | inline_arm { inline_arm }      # 分支写在 `with` 同一行（§2.3.4）
match_arm      ::= "|" pattern [ "if" expr ] "->" expr NEWLINE?
inline_arm     ::= "|" pattern [ "if" expr ] "->" expr

fn_expr        ::= "fn" ( "(" [ params ] ")" | IDENT ) [ ":" type ] "=>" expr

do_expr        ::= "do" NEWLINE INDENT { do_stmt } DEDENT
do_stmt        ::= "let" pattern [ ":" type ] "=" expr NEWLINE
                 | pattern "<-" expr NEWLINE
                 | expr NEWLINE?

annot_expr     ::= pipe_expr [ ":" type ]

pipe_expr      ::= or_expr { ( "|>" | "<|" ) or_expr }
or_expr        ::= and_expr [ "||" or_expr ]
and_expr       ::= cmp_expr [ "&&" and_expr ]
cmp_expr       ::= cons_expr [ cmp_op cons_expr ]
cmp_op         ::= "==" | "!=" | "<" | "<=" | ">" | ">="
cons_expr      ::= add_expr [ "::" cons_expr ]
add_expr       ::= mul_expr { ( "+" | "-" ) mul_expr }
mul_expr       ::= unary_expr { ( "*" | "/" | "·" | "×" | "÷" | "@" ) unary_expr }
unary_expr     ::= ( "-" | "+" | "!" | "√" ) unary_expr
                 | power_expr
power_expr     ::= postfix_expr [ "^" unary_expr ]
postfix_expr   ::= primary { postfix }
postfix        ::= type_app | call | index | field | "?" | "!"
type_app       ::= LT_APP type_arg { "," type_arg } ">"
call           ::= "(" [ expr { "," expr } [ "," ] ] ")"
index          ::= "[" slice { "," slice } "]"
slice          ::= ".."
                 | expr ".." [ expr ]
                 | expr "..=" expr
                 | ":" [ expr ] [ ":" [ expr ] ]
                 | expr ":" [ expr [ ":" expr ] ]
                 | expr
field          ::= "." name

primary        ::= quantity_lit
                 | literal
                 | IDENT
                 | qname
                 | "(" expr ")"
                 | tuple
                 | record_expr
                 | tensor_lit
                 | "(" operator ")"
                 | "_"

qname          ::= UIDENT { "." UIDENT }
tuple          ::= "(" expr "," expr { "," expr } [ "," ] ")"

record_expr    ::= "{" [ record_body ] "}"
record_body    ::= expr "with" field_init { "," field_init } [ "," ]
                 | field_init { "," field_init } [ "," ]
field_init     ::= IDENT "=" expr
                 | IDENT

tensor_lit     ::= "[" [ expr { "," expr } [ "," ] ] "]"

operator       ::= "+" | "-" | "*" | "/" | "@" | "^" | "==" | "!="
                 | "<" | "<=" | ">" | ">=" | "&&" | "||" | "::"

literal        ::= INT | FLOAT | STRING | "true" | "false" | "(" ")"
quantity_lit   ::= ( INT | FLOAT ) unit_expr
```

**说明**

- `LT_APP` 是紧邻标识符的 `<`（§2.7.2）；`record_body` 用 `with` 区分更新与字面量。
- `quantity_lit` 只在数值字面量后紧跟单位表达式时产生（§1.8.1）。
- `"_"` 只在调用实参中合法（§2.3.6）。`(+)` 等括号内的运算符产生 `E0301`（§2.3.7）。
- `qname` 以 `UIDENT` 开头；`Physics.Ising.run` 解析为 `qname` 加字段 `.run`。
- 后缀 `!` 必须紧贴操作数末尾（§2.7.5）。

## A.4 模式

```
pattern        ::= "_"
                 | IDENT
                 | UIDENT
                 | UIDENT "(" [ pattern { "," pattern } ] ")"
                 | literal_pattern
                 | "{" [ field_pat { "," field_pat } ] "}"
                 | "(" pattern { "," pattern } ")"
                 | "[" [ pattern { "," pattern } [ "," ".." IDENT ] ] "]"
                 | pattern "as" IDENT
                 | "(" pattern ":" type ")"

field_pat      ::= IDENT "=" pattern
                 | IDENT

literal_pattern ::= INT | STRING | "true" | "false" | "(" ")"
```

- `match` 分支中裸 `UIDENT` 是零参构造器模式；绑定位置中的裸 `UIDENT` 是变量（§2.4）。
- 浮点字面量与负数字面量不是模式（`E0204`），用守卫。

## A.5 类型

```
type           ::= [ "forall" type_param { "," type_param } "." ] fn_type
fn_type        ::= type_postfix [ "->" type ]
type_postfix   ::= type_atom { "[" IDENT "]" }
type_atom      ::= "Int"
                 | "Real" [ "[" dim "]" ]
                 | "Float" [ "[" dim "]" ]
                 | "Complex" [ "[" dim "]" ]
                 | "Bool"
                 | "Prop"
                 | "String"
                 | "(" ")"
                 | "Tensor" "<" shape "," type ">"
                 | "Matrix" "<" nat "," nat "," type ">"
                 | "Sparse" "<" format_name "," shape "," type ">"
                 | "Table" "<" row_type ">"
                 | "Point" "[" dim "]"
                 | "Unit" [ "[" dim "]" ]
                 | "Set" "<" type ">"
                 | "Option" "<" type ">"
                 | "Result" "<" type "," type ">"
                 | "Solutions" "<" type "," dim ">"
                 | "Dyn" "<" rep ">"
                 | "RateOf" "<" type ">"
                 | "SymFun" "<" dim "," dim ">"
                 | "IO" type
                 | "{" [ field_type { "," field_type } [ "," ]
                         [ "..." IDENT ] ] "}"
                 | "(" type { "," type } ")"
                 | qname [ "<" type_arg { "," type_arg } ">" ]
                 | IDENT
                 | "(" type ")"

field_type     ::= IDENT ":" type
type_arg       ::= type | nat | dim | shape
format_name    ::= IDENT | UIDENT
shape          ::= nat | "[" [ shape_item { "," shape_item } ] "]"
shape_item     ::= nat | "?"
row_type       ::= type { "+" type }
nat            ::= nat_term { "+" nat_term }
nat_term       ::= nat_atom { "*" nat_atom }
nat_atom       ::= INT | IDENT | "(" nat ")"
rep            ::= "Int" | "Real" | "Float" | "Complex" | IDENT
```

- `Real`/`Float`/`Complex` 无方括号等价于 `[1]`；`Unit` 无方括号是 `()`，`Unit[d]` 是单位值
  类型；`Tensor<0, τ>` 等于 `τ`；`RateOf<s>` 是内建类型函数（§8.9）。
- `(A, B)` 是元组类型，`(A, B) -> C` 是多参函数类型；`A * B` 不是类型。
- `Tensor` 的第一个参数按 `shape` 解析，`Dyn` 的参数按 `rep` 解析，其他 `type_arg` 按 kind
  解析。

## A.6 量纲与单位

```
dim            ::= dim_factor { ( "*" | "/" | "·" ) dim_factor }
dim_factor     ::= dim_atom [ "^" exponent ]
dim_atom       ::= "1" | UIDENT | IDENT | "(" dim ")"

exponent       ::= [ "-" ] (INT | FLOAT | "(" rational ")")
rational       ::= [ "-" ] (INT | FLOAT) [ "/" [ "-" ] (INT | FLOAT) ]

unit_expr      ::= unit_factor { ( "*" | "·" | "/" ) unit_factor }
unit_factor    ::= unit_atom [ "^" exponent | SUPERSCRIPT+ ]
unit_atom      ::= unit_name | "(" unit_expr ")" | "%"
```

- `dim` 中 `UIDENT` 是基量纲或别名，`IDENT` 是量纲变量；`/` 左结合（`L/T^2` = `L/(T^2)`）。
- `·` 在词法阶段归一为 `*`（§1.9）。
- 单位表达式内不允许数字（指数除外）；`unit_name` 可带前缀或 `°C`（§1.8）。
- 仿射单位不能出现在乘积或幂中（`E0508`）。

## A.7 运算符优先级

见 §2.6。`..`/`..=` 只在索引中使用，不参与表达式优先级；切片中 `:` 的步长写法与 `::` 的
关系见 §2.3.11。

## A.8 词法记号（摘要）

```
IDENT          ::= lower XID_Continue { XID_Continue | "'" | "₀".."₉" }
UIDENT         ::= upper XID_Continue { XID_Continue | "'" | "₀".."₉" }
INT            ::= dec_int | "0x" hex { hex } | "0o" oct { oct } | "0b" bin { bin }
FLOAT          ::= dec_int "." dec_int [ decimal_exponent ] | dec_int decimal_exponent
decimal_exponent ::= ("e" | "E") ["+" | "-"] dec_int
STRING         ::= '"' { char | escape | "{" expr "}" } '"'
                 | 'r"' { char } '"'
                 | '"""' ... '"""'
UNIT_NAME      ::= "°" [ UIDENT ] | IDENT
SUPERSCRIPT    ::= "⁰".."⁹" | "⁻" | "⁺"
LT_APP         ::= "<" 紧邻标识符（无空格）
ELLIPSIS       ::= "..."
NEWLINE        ::= "\n"
INDENT         ::= 由布局算法产生
DEDENT         ::= 由布局算法产生
```

完整词法规则见第 01 章。`forall` 是上下文关键字（§1.5.2）；`pure` 是 Prelude 函数，不是
关键字。


<a id="ch-appendix-b-diagnostics"></a>

# 附录 B — 诊断目录

诊断是语言体验的一部分。错误信息 MUST 包含代码、主位置、期望/实际，并在能确定时给出至少一条可执行的修正建议。
本附录规定错误码、严重级别与消息格式。

## B.1 格式

```
error[E0401]: dimension mismatch
  --> src/kinematics.aev:4:31
   |
 4 | fn f(h: Float[L]): Float[T] = sqrt(2 * h * g)
   |                               ^^^^^^^^^^^^^^^ expected T, found L/T
   |
   = left:  L/T
   = right: T
   = difference: L/T^2
```

- 第一行 `severity[code]: message`；`-->` 给出文件:行:列；其后是源码行与下划线标签（尽量短）。
- `=`：附注（规范形、差异、相关声明）；`help:`：修正建议，至多两条，按置信度排序。
- 同行标签共用源码片段，其他位置用 `:::` 展示。列号按 Unicode 字符计数，箭头按显示宽度对齐；制表符按四列展开。
  长片段用 `...` 明示省略，保留错误位置。

## B.2 严重级别

| 级别 | 含义 |
|---|---|
| `error` | 程序不能编译/运行 |
| `warning` | 程序可运行，但可能有问题 |
| `note` | 解释性信息，不单独出现 |

## B.3 词法错误（E01xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0101` | tab character in indentation | 用空格替换 |
| `E0102` | inconsistent indentation | 对齐到某一层 |
| `E0103` | expected an indented block | 缩进下一行 |
| `E0104` | unclosed string/comment/bracket | 补闭合 |
| `E0105` | invalid character / escape | 删除/替换 |
| `E0106` | invalid numeric literal | 检查下划线/指数 |
| `E0107` | invalid superscript sequence | 用 `^n` |
| `E0108` | expected a unit after the numeric literal | 写 `3 * x` 或补单位 |

## B.4 语法错误（E02xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0201` | empty block | 加一个表达式 |
| `E0202` | syntax error（含 `private` 用在不允许的声明上；运行时 `_` 出现在调用之外） | 检查附近记号 |
| `E0203` | ragged tensor literal | 使每层等长 |
| `E0204` | invalid pattern | 检查构造器/字段 |
| `E0205` | invalid operator section | 用 `(+)` 或 lambda |
| `E0206` | reserved word used as identifier | 改名 |
| `E0207` | ambiguous type application | 在 `<` 前加空格或给 callee 加括号 |
| `E0208` | duplicate field | 删除重复字段 |
| `E0209` | invalid unit literal（含 `3 (2)`：括号内不是单位表达式） | 检查单位表达式 |
| `E0210` | unknown attribute | 检查属性名/参数 |
| `E0211` | cannot lower forbidden lint | 移除内层 `@allow`/`@warn`/`@deny` 或放宽外层 `@forbid` |
| `E0212` | unknown lint or group name | 运行 `aevia lint --list` 查看可用名称 |

## B.5 类型错误（E03xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0301` | unbound name | 导入/拼写 |
| `E0302` | type mismatch（含类型级 `Nat` 方程无解、未知 capability 名、`to_system` 的名字不是字面量） | 检查标注/参数 |
| `E0303` | infinite type (occurs check) | 加标注/改递归 |
| `E0304` | ambiguous type（含未能解出的 `Nat` 变量、非线性 `Nat` 方程、无 binder 的 shape 变量、缺少 `from_system<D>`） | 加类型标注 |
| `E0305` | not a function | 检查调用 |
| `E0306` | missing record field | 补字段/改类型 |
| `E0307` | constructor arity mismatch | 补/删参数 |
| `E0308` | pattern type mismatch（含 `Prop` 作守卫） | 改模式 |
| `E0309` | non-exhaustive match | 补分支 |
| `E0310` | no instance for class | 导入实例/加实例 |
| `E0311` | overlapping instances | 合并/删除 |
| `E0312` | non-positive recursive type | 移到箭头右侧 |
| `E0313` | IO action in a pure context | 用 `do`/返回 `IO` |
| `E0314` | cannot infer rank/element type（空张量、`read_npy` 未给秩） | 加 `Tensor<n, …>` 标注 / `read_npy<2>` |
| `E0315` | wrong number of arguments | 补/删参数 |
| `E0316` | duplicate definition | 改名/删除 |
| `E0317` | cannot derive instance / unsupported parameter kind | 手写实例 |
| `E0318` | function is less generic than its declared type parameters | 让函数体对任意选择成立，或把参数换成函数体需要的具体类型 |

## B.6 量纲错误（E04xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0401` | dimension mismatch | 使两边同量纲 |
| `E0402` | exponent must be a dimensionless number / finite | 指数必须无量纲（精确相位的无穷指数也是此码，§6.3.4） |
| `E0403` | transcendental function requires dimensionless argument | 除以单位 |
| `E0404` | dimensionful base with a run-time exponent | 字面量指数/`pow_dyn` |
| `E0405` | invalid affine point operation | 用温差/`lerp` |
| `E0406` | bare `0` used where a dimensioned value is expected | 写 `0 m` |
| `E0407` | unknown dimension/alias | 声明/导入 |
| `E0408` | unsatisfiable dimension constraint | 保留码 |
| `E0409` | dimension occurs check | 保留码 |
| `E0410` | dimensionful `det`/`product` requires `Dyn<ρ>` | 除以单位/`det_unit` |
| `E0411` | incompatible tensor element types or dimensions | 统一元素类型/量纲 |
| `E0412` | `Dyn`/kernel dimension check failed（运行时） | 检查运行时量纲 |

注：`E0408`、`E0409` 为保留码：量纲统一是自由阿贝尔群上的指数线性统一，含非零系数变量的残差总可解，唯一的失败是
非零常量残差（`E0401`），也不会出现 occurs 状态。

## B.7 单位错误（E05xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0501` | unknown unit（含叠加前缀 `kkm`、先用后声明） | 导入/拼写 |
| `E0502` | unit not imported | 保留码 |
| `E0503` | prefix cannot be applied | 保留码 |
| `E0504` | affine offset dimension mismatch | 检查 offset |
| `E0505` | duplicate unit | 删除重复 |
| `E0506` | the scale has no exact value for that exponent | 检查定义/指数 |
| `E0507` | prefixed unit conflicts with explicit unit | 保留码 |
| `E0508` | affine unit in product/power | 单独使用 |
| `E0509` | `to`/`value_in` unit measures a different dimension | 换单位 |
| `E0510` | affine unit used on a temperature difference | 用 `delta_to` |
| `E0511` | unit has no exact scale in the module's unit system（§5.11.4） | 写单位制的数，或 `to_system` |
| `E0512` | unknown unit system（§5.11.1） | 用 kernel 提供的名字（natural、planck、atomic、geometrized） |

注：`E0502`、`E0503`、`E0507` 为保留码：没有模块级单位命名空间（§5.9），对应状态不存在。

## B.8 模块错误（E06xx）

| 代码 | 消息 |
|---|---|
| `E0601` | module name does not match path |
| `E0602` | cyclic import |
| `E0603` | the same name opened by two modules with different types |
| `E0604` | orphan instance |
| `E0605` | invalid package manifest |
| `E0606` | dependency version conflict |
| `E0607` | module/package not found |
| `E0608` | no runnable module/entry point（嵌入 API） |

## B.9 效果错误（E07xx）

| 代码 | 消息 |
|---|---|
| `E0701` | `do` statement or `<-` expects an `IO` action |
| `E0702` | `?` requires a `Result`/`Option` and a matching return boundary |

## B.10 运行时错误（E08xx）

| 代码 | 消息 | 可恢复 |
|---|---|---|
| `E0801` | division by zero | 精确：否；浮点：IEEE |
| `E0802` | shape/index error | 否 |
| `E0803` | free symbols in `approx` | 是 |
| `E0804` | cannot decide symbolic condition / assumption is false | 否 |
| `E0805` | stack overflow / tail-call or IO budget exceeded | 否 |
| `E0806` | domain error | 是 |
| `E0807` | numerical method did not converge | 是 |
| `E0808` | budget exhausted | 是 |
| `E0809` | internal type/dimension invariant violated | 否 |
| `E0810` | IO error | 是 |
| `E0811` | task failure: a result that cannot cross threads, a task awaited twice, or a stopped worker（§9.7.3） | 是 |
| `E0812` | 宿主 / Ctrl-C 中断求值（§9.8） | 是 |
| `E0899` | panic | 否 |

注：个别内建函数借用 `E0806`–`E0809` 报告自己的参数/内部失败（例如 `approx_map`、`degree`），码的类别以上表为准，消息以内建
自述为准。

## B.11 IO 错误（E09xx）

| 代码 | 消息 |
|---|---|
| `E0901` | file not found |
| `E0902` | permission denied（含未授予 capability，§9.14） |
| `E0903` | parse error in data file |
| `E0904` | unsupported format |
| `E0905` | plot rendering failed |

## B.12 警告（W0xxx）

| 代码 | 消息 |
|---|---|
| `W0101` | unused variable |
| `W0102` | unused import（`aevia lint`） |
| `W0103` | shadowed binding |
| `W0201` | redundant match arm |
| `W0202` | discarded IO result |
| `W0203` | unreachable code（保留码） |
| `W0204` | 在 `@relax_result` 下丢弃 `Result` 值 |
| `W0205` | 在 `@relax_result` 下隐式解包 `Result`/`Option`（`?` 无法转换） |
| `W0206` | 在 `@relax_result` 下 `Result`/`Option` 的 `match` 不穷尽 |
| `W0301` | unresolved dimension defaulted to 1 |
| `W0302` | `Real`/`Float` mix uses the exact `Real` path |
| `W0303` | unused symbol |
| `W0501` | display unit dropped（保留码，§5.4.2） |
| `W0601` | unused private declaration |
| `W0602` | deprecated name used：声明上的 `@deprecated("...")`（§2.2.7/§13.8.1） |
| `W0701` | 函数/变量名未使用 `snake_case`（`aevia lint`） |
| `W0702` | 类型/类/构造器名未使用 `UpperCamelCase`（`aevia lint`） |
| `W0703` | 公开声明缺少 `##` 文档注释（`aevia lint`，pedantic） |
| `W0704` | 冗余类型标注（`aevia lint`，pedantic） |
| `W0705` | 绑定遮蔽预置函数名（`aevia lint`） |
| `W0706` | 对 `Float` 值使用严格相等 `==`/`!=`（`aevia lint`） |
| `W0707` | 裸零字面量被隐式赋予量纲（`aevia lint`） |
| `W0708` | 将表达式与其自身进行比较（`aevia lint`） |
| `W0709` | 冗余的 `if` 表达式（`aevia lint`） |
| `W0710` | 重复导入同一模块（`aevia lint`） |

注：`W0203` 为保留码（语言没有 `return`/`break`，语法无法表示不可达语句）；`W0501` 为保留码。
`W0102` 与 `W0701`–`W0710` 由 `aevia lint` 发射；编译器警告与 lint 检查共同遵循
`aevia.toml [lints]`、`-A/-W/-D/-F` 与 `@allow/@warn/@deny/@forbid`。

## B.13 消息写作规范

1. **说人话。** `expected Real[L], found Real[T]` 好于 `type unification failed`。
2. **指小范围。** 下划线覆盖最小错误子树。
3. **给差异。** 量纲错误列出 `left`、`right`、`difference` 的规范形。
4. **给一个修正。** 只在置信度高时给 `help`，不列五个猜测。
5. **不要重复。** 同一错误只报一次，其余用 `note` 指向主位置。
6. **保留上下文。** 省略的行/列用 `...` 标明，保留错误位置。
7. **可解析。** 代码和位置稳定，便于 IDE/CI 解析。

## B.14 示例：类型错误

```
error[E0302]: type mismatch
  --> t.aev:2:14
   |
 2 | let n: Int = 1.5
   |              ^^^ a bare numeric literal cannot have type Int
   |
```

## B.15 示例：单位错误

```
error[E0501]: unknown unit `kms`
  --> t.aev:2:9
   |
 2 | let d = 3.2 kms
   |         ^^^^^^^
   |
help: did you mean `km`?
```

## B.16 示例：运行时形状错误

```
error[E0802]: matmul requires left columns (2) = right rows (3)
  --> r.aev:5:13
   |
 5 |     let C = A @ B
   |             ^^^^^
   |
```

## B.17 扩展/后端错误（E10xx）

| 代码 | 消息 | 说明 |
|---|---|---|
| `E1001` | backend capability or witness missing | 携带 capability 名与最小 RSSN 版本；kernel 缺少量纲见证、回调句柄失效也用此码 |
| `E1002` | ambiguous operator/overload | 列出候选签名 |
| `E1003` | no matching operator/overload | 列出候选签名/动态入口；回调元数/形状不符也用此码 |
| `E1004` | incompatible RSSN api_version / kernel result fields | 要求升级/降级；kernel 缺少或多出已声明的记录字段 |
| `E1005` | no pinned overload / signature hash mismatch | 重新生成 `Rssn` 模块 |
| `E1006` | plugin ABI/load/registration failure | 检查插件签名/版本 |

这些是**环境/后端**错误，不是用户程序的类型错误；诊断必须给出可执行的替代方案（例如“安装带 `core.gpu` 的 RSSN”或“用
`Rssn.dynamic`”）。

