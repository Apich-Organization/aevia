# Aevia Language Specification

**Single-file edition — Revision 4**

> 本文件由 `spec/` 下的章节按顺序合并生成；章节内的相对链接在单文件版中不再可用。

---

## Table of contents

1. Aevia Language Specification — Index

2. 00 — 总览

3. 01 — 词法结构

4. 02 — 语法

5. 03 — 类型系统

6. 04 — 量纲

7. 05 — 单位

8. 06 — 求值语义

9. 07 — 模块、包与可见性

10. 08 — 标准库

11. 09 — 运行时

12. 10 — rssn 后端映射

13. 11 — 完整示例

14. 12 — 设计取舍、限制与路线图

15. 13 — 可扩展性与 RSSN 集成

16. 附录 A — 完整文法

17. 附录 B — 诊断目录

18. 附录 C — 修订记录


---



<!-- ===== README.md ===== -->


## Index

**Aevia — Design Standard**

Aevia is a pure functional language for scientific computing in which
**dimensions are part of the type system**. A quantity is not a number with a
runtime tag; it is a value whose type records its physical dimension. The
compiler rejects `3 m + 2 s`, `sin(3 kg)` and `exp(1 m)` before any code runs.

The language is designed to sit on top of the `rssn` computer-algebra and
scientific-computing engine. Symbolic and numeric evaluation are two phases of
the same expression; units, dimensional inference, rewriting, numerical kernels,
JIT compilation and physics simulations are provided by the backend. Aevia does
not maintain a second operator list or a second unit table: it consumes the
RSSN **Kernel Manifest** and `rssn-dim` registry, and generates its `Rssn`
module from them. See [13 — Extensibility](13-extensibility.md) and the
[backend requirements](../RSSN-INTEGRATION-REQUIREMENTS.md).

> This document is the **normative design**. It is written to be implementable:
> the grammar is complete, the typing rules are stated, the runtime architecture
> is fixed, and every backend dependency is identified. Examples and rationale
> are non-normative.

---

## Reading guide

| Chapter | Contents | Normative |
|---|---|---|
| [00 — Overview](00-overview.md) | Goals, non-goals, principles, ten-minute tour | Yes (goals) |
| [01 — Lexical structure](01-lexical.md) | Source encoding, literals, units, layout | Yes |
| [02 — Syntax](02-syntax.md) | Declarations, expressions, patterns, modules | Yes |
| [03 — Type system](03-types.md) | Kinds, types, inference, classes, effects | Yes |
| [04 — Dimensions](04-dimensions.md) | Dimension algebra, typing rules, polymorphism, `Dyn` | Yes |
| [05 — Units](05-units.md) | Unit declarations, prefixes, affine points, constants | Yes |
| [06 — Semantics](06-semantics.md) | Evaluation, values, symbolic/numeric phases, tensors | Yes |
| [07 — Modules and packages](07-modules.md) | Namespaces, visibility, packages, tests | Yes |
| [08 — Standard library](08-stdlib.md) | Prelude and library signatures | Mostly yes |
| [09 — Runtime](09-runtime.md) | Pipeline, interpreter, REPL, CLI, LSP, performance | Yes (architecture) |
| [10 — rssn backend](10-backend.md) | Mapping to rssn, bridge design, gaps and risks | Yes |
| [11 — Examples](11-examples.md) | Worked programs from kinematics to simulation | No |
| [12 — Rationale](12-rationale.md) | Alternatives, trade-offs, known limits, roadmap | No |
| [13 — Extensibility](13-extensibility.md) | RSSN reflection, generated `Rssn`, plugins, registry, shape/device/sparse decisions | Yes |
| [Appendix A — Grammar](appendix-a-grammar.md) | Complete EBNF | Yes |
| [Appendix B — Diagnostics](appendix-b-diagnostics.md) | Error codes and message format | Yes |
| [Appendix C — Changelog](appendix-c-changelog.md) | Audit revisions | Informational |

后端接口清单：[`RSSN-INTEGRATION-REQUIREMENTS.md`](../RSSN-INTEGRATION-REQUIREMENTS.md)。这是
Aevia 规范能够成立的前提，不是可选的实现细节。

## Conformance language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHOULD**, **SHOULD NOT**
and **MAY** are to be interpreted as described in RFC 2119. A conforming
implementation MUST accept every program in this specification's test corpus,
MUST reject every program marked ill-typed, and MUST produce the specified
diagnostic category (the exact wording is not normative).

## Design status

This is a **design standard**, not a frozen implementation. Chapters marked
normative are intended to be implemented as written; the changelog records every
audit round. The current text is **Revision 4**, after three rounds of
independent audits plus the extensibility/backend-integration redesign. There is
**no v1/v2 language split**: every capability is designed in; a backend that
does not yet implement one reports a capability diagnostic (`E1001`) instead of
removing the language feature.

## A one-sentence summary

> Aevia makes the dimension of a quantity a type, the unit a compile-time scale,
> the formula a first-class value, and the numeric result a deliberate request.

## Quick taste

```aevia
module Kinematics

open Units.SI

let g = 9.80665 m/s^2

fn fall_time(h: Float[L]): Float[T] =
    sqrt(2 * h / g)

fn main(): IO Unit = do
    let h = 20.0 m
    println("fall time from {h} = {fall_time(h)}")
```

```
$ aevia run kinematics.aev
fall time from 20 m = 2.019 s
```

The next chapter explains why this program type-checks, and what the language
does when it does not.



<!-- ===== 00-overview.md ===== -->


# 00 — 总览：Aevia 是什么，为什么这样设计

## 0.1 一句话定义

**Aevia 是一门纯函数式的科学计算语言：量纲是类型的一部分，单位是编译期已知的
尺度，公式是一等值，数值结果是显式请求的另一种相位。**

它不试图成为通用编程语言，也不试图取代 Python 的生态。它瞄准的是一类具体的
程序：从一条物理定律、一组实验数据或一个微分方程出发，经过符号推导、量纲检查、
数值离散和可视化，最后得到一个可复现的结果。这类程序今天通常由 Python +
NumPy + SymPy + Pint 拼成，量纲检查靠库、符号与数值靠两套 API、单位错误往往在
运行时才暴露。Aevia 把这些东西收进一门语言，并让编译器在最便宜的时刻——类型
检查时——拒绝量纲错误。

```aevia
# 一个完整的 Aevia 程序
module Kinematics

open Units.SI

let g = 9.80665 m/s^2

fn fall_time(h: Float[L]): Float[T] =
    sqrt(2 * h / g)

fn main(): IO Unit = do
    let h = 20.0 m
    println("fall time from {h} = {fall_time(h)}")
```

这段程序里没有类型转换、没有单位字符串、没有 `Quantity` 包装。`h` 的类型是
`Float[L]`，`g` 的类型是 `Float[L/T^2]`，`2 * h / g` 的类型由量纲代数自动算出是
`Float[T^2]`，`sqrt` 把指数除以二，得到 `Float[T]`。如果写成 `h + g`，程序根本
不会运行。

## 0.2 目标

按优先级排列：

1. **量纲正确性由构造保证。** 加、减、比较要求量纲相等；乘、除、幂按量纲代数
   组合；超越函数要求无量纲参数；单位换算在编译期折叠为精确尺度。量纲错误是
   类型错误，不是运行时警告。
2. **符号与数值是同一门语言的两个相位。** 同一个表达式既可以化简为闭式，也可以
   在给定点求值。用户不需要在 SymPy 和 NumPy 之间搬运字符串。
3. **写出来像数学。** 单位字面量 `9.81 m/s^2`、Unicode 标识符 `Δt`、中缀算子、
   管道 `|>`、模式匹配。标点服务于可读性，而不是服务于解析器。
4. **纯核心，显式效果。** 默认没有可变状态、没有隐藏的全局变量、没有隐式 IO。
   副作用被隔离在 `IO` 中，纯函数可以放心地并行、缓存、求导和代入。
5. **性能可预测。** 严格求值；张量的秩在类型里、形状在运行时；数值热点可以下沉
   到 rssn 的数值内核和 Cranelift JIT；符号运算的搜索预算显式可控。
6. **小核心，厚标准库。** 语言只有函数、代数数据类型、记录、类型类、模块和
   `IO`；数学、单位、线性代数、统计、ODE/PDE、绘图都在标准库里。
7. **错误是教学时刻。** 量纲错误要指出两边的量纲、给出一个可能的修正，而不是
   只打印 “type mismatch”。

## 0.3 非目标

- **不是系统编程语言。** 没有手动内存管理、没有 `unsafe`、没有指针算术。需要
  底层性能时，调用 rssn 的 Rust/C 内核，而不是在 Aevia 里写汇编。
- **不是证明助手。** 量纲类型是阿贝尔群上的等式，不是完整依赖类型。我们不做
  任意定理证明。数组形状以 **refinement** 形式进入类型（`Tensor<[3,4],τ>`），
  但类型级 `Nat` 只做线性算术，不做一般递归/证明（§13.9.1）。
- **不追求 Python 级别的库生态。** 通过 CSV/NPY/JSON、C ABI、插件和 registry
  与外部世界交换数据；不内嵌 Python。
- **不提供用户自定义语法/宏/effect handlers。** 这是有意的边界：扩展点是
  RSSN 算子、插件、`Rssn` 动态调用和包 registry，不是语言级宏或效果系统。
  这样工具链、格式化和错误信息保持统一（§13.9.6、§13.9.7）。
- **不自动并行。** 纯函数允许并行，但并行必须由 `par_map` 等显式原语请求，避免
  在小数组上产生调度开销和不可预测的延迟。设备/GPU/分布式是运行时目标，
  不是类型参数（§13.9.4）。

## 0.4 设计原则

### 原则一：量纲是类型，单位不是

这是整门语言最重要的区分。

- **量纲（dimension）** 是类型层面的：`L`、`T`、`M`、`L/T`、`M*L^2/T^2`。它决定
  一个值能参与哪些运算。
- **单位（unit）** 是值层面的编译期常量：`m`、`km`、`h`、`degC`。它只提供尺度
  （以及仿射偏移），在编译期被折叠掉。

因此 `3 km` 和 `3000 m` 具有**同一个类型** `Real[L]`，在运行时是同一个值。
`km` 不会被带进类型，也不会在每次运算时做换算。需要显示成 `km` 时，用
`to(km, x)` 改变的是显示单位，不是值。

这个区分带来两个好处：

1. 类型相等就是量纲相等，不需要处理“米和厘米是不同类型”带来的组合爆炸。
2. 单位表可以独立扩展（英制、天文、工程单位），不会污染类型检查器。

代价是：类型系统不区分“以米表示的长度”和“以英尺表示的长度”。这是刻意的。
科学计算的错误几乎总是量纲错误，而不是同一量纲下的单位选择错误；后者由
`to`/显示层解决。

### 原则二：精确与近似是两种类型，不是两种模式

- `Int`：任意精度整数，无量纲。
- `Real[d]`：精确实数或有符号表达式，量纲为 `d`。`1/3` 是精确的 `Real`，
  `sqrt(2)` 是一个符号表达式，`x^2 + 1` 也是。
- `Float[d]`：IEEE-754 binary64，量纲为 `d`。`1.5` 是 `Float`。
- `Complex[d]`：复数标量。

十进制字面量默认是 `Float`，因为写 `0.1` 的人通常想要一个浮点数；写 `1/10`
的人想要精确值。整数默认是 `Int`。`Real` 与 `Float` 混合运算时 `Float` 传染
（与 rssn 的 float contagion 一致），但不会有隐式的 `Real → Float` 转换发生在
你没写 `Float` 的地方。

这条规则让 `sin(1)` 保持为符号表达式 `sin(1)`，而 `sin(1.0)` 直接得到
`0.8414709848078965`。看起来微妙，但它迫使作者明确“我是在做精确推导还是在做
数值计算”，这正是科学计算中最容易混淆的地方。

### 原则三：纯函数，但允许“看起来像命令式”的写法

Aevia 没有可变变量、没有 `for` 循环、没有原地修改。迭代用递归表达，编译器保证
尾调用；数组更新在语义上是复制，运行时在唯一所有权时原地完成。这样既保留了
纯函数的可推导性，又不会强迫用户为每个数值算法写 monad。

`do` 块只用于 `IO`。它不是通用 monad 语法，也不试图把副作用伪装成纯函数。

### 原则四：符号运算不是宏，是值

Aevia 不需要 `quote`/`unquote`。`symbol("x")` 返回一个 `Real` 值；对它能做的
一切——加、乘、求导、代入——和普通 `Real` 一样。`diff(fn x => x^2)(3 m)` 的
类型是 `Real[L]`，因为导数把量纲除以自变量的量纲。

这让“先推导公式，再代入数据”成为默认工作流，而不是两个库之间的桥接。

### 原则五：类型系统为易用性让步，但不为可靠性让步

我们使用 HM 推断 + 类型类 + 量纲代数，而不是完整依赖类型。原因是：

- 用户不应该为 `mean(xs)` 写类型签名；
- 但用户必须能在不运行程序的情况下知道 `mean(xs)` 的量纲；
- 量纲等式是阿贝尔群上的线性方程，推断是可判定的、线性的，适合放进 HM。

当静态类型无法表达时（例如 `x^n` 的 `n` 是运行时整数），语言不发明不可靠的
类型，而是提供一个显式的动态逃逸 `Dyn`，把检查推迟到运行时。静态路径保持
可靠，动态路径保持可用。

### 原则六：单一注册表，反射优先

Aevia 不手写 RSSN 算子绑定，也不维护第二张单位表。RSSN 的 `OpTable` 是
唯一算子注册表，`rssn-dim` 是唯一量纲/单位层；Aevia 读取 Kernel Manifest，
生成 `Rssn` 模块（类型化 + `dynamic`）。新算子/新单位/新插件不需要改
Aevia 编译器。这样 1000+ 算子可以持续增长，而不是靠“下个版本支持”追赶。

## 0.5 十分钟导览

### 量纲与单位

```aevia
let d = 100 m                 # Real[L]
let t = 9.58 s                # Real[T]
let v = d / t                 # Real[L/T]
let E = 70 kg * v^2 / 2       # Real[M*L^2/T^2]

let bad = d + t               # error[E0301]: Real[L] vs Real[T]
```

`70 kg` 是 `Real[M]`，`v^2` 是 `Real[L^2/T^2]`，乘积是 `Real[M*L^2/T^2]`。
不需要写 `J`；`J` 只是 `M*L^2/T^2` 的打印别名。

### 量纲多态

```aevia
fn mean<d>(xs: Tensor<1, Real[d]>): Real[d] =
    sum(xs) / length(xs)

fn variance<d>(xs: Tensor<1, Real[d]>): Real[d^2] =
    let m = mean(xs)
    sum((xs - m)^2) / length(xs)
```

`mean` 对任何量纲都成立；`variance` 的量纲是输入量纲的平方。编译器不需要为
每个量纲生成一份代码；量纲在运行时被擦除，只留下值。

### 符号与数值

```aevia
let x = symbol("x")                   # Real，量纲待定，默认无量纲
let f = x^3 - 3*x + 1                 # Real
let df = diff(fn t => t^3 - 3*t + 1)  # 3*x^2 - 3，作为函数
let slope = df(x)                     # 代回符号 x

let y = approx(df(2))                 # Ok(9.0)，Float
```

`diff` 接受一个函数，返回它的导函数；`df(2)` 在点 2 求值。它内部创建一个
同量纲的符号，把函数应用上去，求导，再代回。这个设计让求导对高阶函数、
部分应用和用户自定义函数都成立。

### 张量

```aevia
let A = [[1.0, 2.0], [3.0, 4.0]]      # Tensor<[2,2], Float>
let b = [5.0, 6.0]                    # Tensor<[2], Float>
let x = match linsolve(A, b) with
        | Ok(v) -> v
        | Err(_) -> [0.0, 0.0]        # 示例；实际应处理错误
let y = A @ x                         # matmul，等于 b

let R = [[1.0, 2.0], [3.0, 4.0]] * ohm
let I = [0.1, 0.2] * A
let V = R @ I                         # Tensor<[2], Float[Voltage]>
```

`@` 是矩阵乘法，`*` 是逐元素乘法（标量自动广播）。这个选择与 NumPy 一致，
避免 MATLAB 式 `*`/`.*` 的歧义。

### 记录、变体与模式匹配

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

`read_csv`、`plot`、`println` 是 `IO` 动作；`mean` 是纯函数。`main` 是唯一被
运行时调用的入口。

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

Aevia 自己负责：语法、类型与量纲检查、闭包与记录、张量形状、模块、IO、标准库
的编排。rssn 负责：符号表达式的表示与重写、量纲代数的运行时部分、数值算法、
线性代数、FFT、ODE/PDE、特殊函数、JIT、物理仿真。

两者之间的边界是 **Term**：Aevia 的 `Real`/`Complex` 标量在需要符号能力时持有
一个 rssn 项；`Float` 标量和数值张量则走原生快速路径，不进入项图。

## 0.7 与 rssn 的关系

rssn 的核心是一个 hash-consed DAG + e-graph 重写引擎，入口是
`Session::compute(term, config)`，目标相位是 `Symbolic` 或 `Numeric`。它已经
提供：

- 精确的任意精度整数/有理数与浮点数（`Number`）；
- 量纲层 `graph::dim`：基量纲、有理/符号指数、量纲变量与 unification、
  `Advisory`/`Strict` 模式、单位解析；
- 大量算子与规则集：微积分、代数、方程、ODE/PDE、线性代数、统计、特殊函数、
  物理公式；
- 数值内核：求积、ODE、FFT、优化、线性代数、稀疏求解；
- `Term::compile` 与 Cranelift JIT；
- `sim::scenario::run(name, json)` 的仿真入口。

Aevia 不重复实现这些。语言层的量纲检查器是**编译期**的，rssn 的量纲层是
**运行时/项级**的；两者使用同一套基量纲和指数代数，但目的不同：前者拒绝坏程序，
后者在化简过程中保持正确性并报告诊断。

## 0.8 文档约定

- 代码块标 `aevia` 的是 Aevia 源码；标 `text` 的是文法/推导/输出。
- 类型写成 `e : τ`；量纲写成 `d`、`e`；类型环境写成 `Γ`。
- `Real[d]` 中的 `d` 是量纲表达式，不是类型参数；`<d>` 声明量纲参数。
- “必须”是规范性要求；“应该”是强烈建议；“可以”是可选。
- 示例中的 `...` 表示省略，不是合法语法。

下一章从最底层开始：一个 Aevia 源文件由哪些字符、记号与布局规则组成。



<!-- ===== 01-lexical.md ===== -->


# 01 — 词法结构

本章定义 Aevia 源文件如何被切分为记号（token）。语法在第 02 章；单位字面量的
**语义**在第 05 章，本章只定义它如何被识别。

## 1.1 源文件与编码

- 源文件扩展名 `.aev`，编码必须是 UTF-8，允许 BOM，BOM 被忽略。
- 换行可以是 `\n`、`\r\n` 或 `\r`；在词法阶段统一为 `\n`。
- 源文件按 Unicode NFC 规范化后再做词法分析。规范化不改变字符串字面量的内容
  （字符串按原样保存），只影响标识符、关键字和单位名。
- 文件第一行可以是 shebang：`#!` 开头，直到行尾，被忽略。
- 制表符（U+0009）**不得**用于缩进；出现在行首缩进中是一个词法错误
  `E0101`。字符串和注释中的制表符不受限制。

## 1.2 记号的种类

```
token ::= IDENT | UIDENT | KEYWORD | INT | FLOAT | STRING
        | UNIT_NAME | PERCENT | SUPERSCRIPT | LT_APP | ELLIPSIS
        | OP | LPAREN | RPAREN | LBRACKET | RBRACKET | LBRACE | RBRACE
        | COMMA | SEMICOLON | COLON | DOT | DOTDOT | DOTDOTEQ
        | ARROW | FATARROW | LARROW | PIPE | EQUALS
        | NEWLINE | INDENT | DEDENT | EOF
```

空白（空格、换行）分隔记号，但换行同时参与布局（§1.4）。除单位字面量外，
Aevia 没有“相邻记号自动相乘”的规则：`3 x` 不是乘法，`f x` 不是函数应用。
函数调用必须写括号 `f(x)`，乘法必须写 `3 * x`。这条规则消除了科学记数法、
单位和函数应用之间的全部歧义。

## 1.3 注释

```aevia
# 行注释，直到行尾
let x = 1  # 也可以出现在行尾

#- 块注释
   可以嵌套 #- 内层 -#
   仍然在块内 -#
```

- `#` 开始行注释。
- `#-` 开始块注释，`-#` 结束；块注释可以嵌套。
- `##` 开始文档注释，必须出现在行首（允许前置缩进），并附着到下一个声明。
  文档注释的内容是 Markdown。
- 注释不产生记号，但保留行号信息用于诊断。

## 1.4 布局：缩进与续行

Aevia 使用 offside 规则，类似 Python，但块只由缩进界定，没有 `end`。

### 1.4.1 INDENT / DEDENT

词法分析器维护一个缩进栈，初始为 `[0]`。对每一个**逻辑行**（不在括号内、
不是空行、不是纯注释行）：

1. 计算行首空格数 `n`。
2. 若 `n > 栈顶`，压入 `n`，发出 `INDENT`。
3. 若 `n == 栈顶`，不发记号。
4. 若 `n < 栈顶`，反复弹出直到栈顶 `== n`，每弹一次发一个 `DEDENT`；
   若弹到栈空仍不相等，报错 `E0102`（缩进不匹配）。
5. 行尾发 `NEWLINE`。

空行和只含注释的行不参与缩进计算，也不发 `NEWLINE`。

### 1.4.2 括号内自由布局

在 `(`、`[`、`{` 内部，换行与缩进不产生 `NEWLINE`/`INDENT`/`DEDENT`。
因此下面两种写法等价：

```aevia
let A = [[1.0, 2.0],
         [3.0, 4.0]]

let B = [
    1.0, 2.0,
    3.0, 4.0,
]
```

### 1.4.3 续行：括号与布局组

括号外没有“续行抑制”。每一行都以 `NEWLINE` 结束；多行公式 MUST 用括号
包起来，括号内的换行和缩进被忽略：

```aevia
let energy = (m * c^2
            + 1/2 * m * v^2
            - m * g * h)
```

括号外，如果一行以 `=`、`then`、`else`、`->`、`=>`、`do`、`with` 等
块开头结束，下一行缩进增加，词法器发出 `INDENT`，解析器把
`NEWLINE INDENT expr DEDENT` 解析成一个**布局组**。布局组用于：

- 函数体/`let` 右侧的单个表达式；
- `if` 的分支；
- `match` 的分支体；
- `do` 的语句体。

布局组不用于“把二元运算符续到下一行”：`m * c^2` 换行后写 `+ ...` 不会
自动拼接；请用括号。这条规则让词法器不需要知道哪些记号是运算符，也让
格式化和解析保持简单。

### 1.4.4 块

块也是 `NEWLINE INDENT ... DEDENT`，但内部是 `let`/`fn` 声明序列加一个
最终表达式：

```aevia
fn mean<d>(xs: Tensor<1, Real[d]>): Real[d] =
    let n = length(xs)
    sum(xs) / n
```

解析器在 `NEWLINE INDENT` 后看到 `let`/`fn` 时选块，否则选布局组。块的
最后一行是值；只有 `let` 的块是 `E0201`（empty block）。

`do`、`match`、`class`、`instance`、`type` 的变体体各自有专门的
`NEWLINE INDENT ... DEDENT` 形式（附录 A）。

`else` 必须与对应的 `if` 同缩进；`match` 的 `|` 分支必须彼此同缩进，且
SHOULD 与 `match` 同缩进或再缩进一级。解析器接受两种对齐方式，但同一
`match` 内必须一致。

## 1.5 标识符与关键字

### 1.5.1 标识符

```
ident_start ::= XID_Start | "_"
ident_continue ::= XID_Continue | "'" | "₀".."₉"
ident ::= ident_start { ident_continue }
```

- 允许 Unicode 字母，包括希腊字母：`Δt`、`λ`、`θ`、`σ`。
- 允许撇号：`f'`、`x''`（常用于导数记号）。
- 允许下标数字：`x₁`、`v₂`。
- 标识符区分大小写。
- 单独的 `_` 是通配符/占位符，不是合法绑定名。
- 反引号标识符 `` `type` `` 可以引用与关键字同名的外部名字；当前规范只允许在
  `import` 后的限定名中使用，不允许声明。

### 1.5.2 关键字

```
as       class     derive    dimension  do       else     false
fn       for       if        import     in       instance let
match    module    open      private    rec      test     then
true     type      unit      where      with
```

`forall` 是**上下文关键字**：只在类型开头（后跟 type_param 和 `.`）时
解析为量词；它同时是标准库函数名（§8.13）。`return` 是 Prelude 函数，
不是关键字。`in` 用于 `let ... in`。`for` 用于 `instance ... for ...` 和
`derive ... for ...`。`rec` 用于 `let rec`。

关键字不能作为标识符。`in` 是 `let ... in` 的关键字，不是旧式写法；实现
MUST 拒绝把任何关键字当普通标识符。

### 1.5.3 命名约定（非规范）

- 类型、构造器、模块、量纲：`UpperCamelCase`，如 `Real`、`Circle`、`SI`、`Length`。
- 函数、值、字段、单位：`snake_case`，如 `fall_time`、`kinetic_energy`、`degC`。
- 类型参数：小写 `a`、`b`；量纲参数：小写 `d`、`e`。
- 单位名保持国际习惯：`m`、`s`、`kg`、`Pa`、`°C`。

## 1.6 数值字面量

```
int_lit   ::= dec_int | hex_int | oct_int | bin_int
dec_int   ::= digit { ["_"] digit }
hex_int   ::= "0x" hexdigit { ["_"] hexdigit }
oct_int   ::= "0o" octdigit { ["_"] octdigit }
bin_int   ::= "0b" bindigit { ["_"] bindigit }

float_lit ::= dec_int "." dec_int [ exponent ]
            | dec_int exponent
exponent  ::= ("e" | "E") ["+" | "-"] dec_int
```

规则：

- 下划线只用于分组，不能出现在开头/结尾或连续出现；`1_000` 合法，`_1`、
  `1__0` 不合法。
- 十进制整数是 `Int`；带小数点或指数的字面量是 `Float`。
- 没有八进制前缀 `0` 的旧写法；`0755` 是十进制 755。
- 没有数字后缀。要写 `Float` 的 3，写 `3.0`；要写精确的 3，写 `3`。
- `oo` 是 `Real[1]` 的正无穷，`-oo` 是负无穷，`nan` 是 `Float[1]` 的 NaN。
  它们是预置常量，不是字面量。
- 复数字面量没有专用语法；写 `3 + 4 * i`，`i` 是预置的虚数单位。

`0` 不是量纲多态的：它默认是 `Int`。要写“零米”，写 `0 m`；要写浮点零，
写 `0.0`。这避免了“零乘任何值”的类型特例（§4.7）。

## 1.7 字符串字面量

```aevia
"hello"
"value = {x + 1}"
"line\nbreak"
"quote: \""
r"C:\data\raw.csv"          # 原始字符串：无反斜杠转义、无插值
"""
多行字符串
  保留相对缩进
"""
```

- 普通字符串用双引号；单引号不是字符串分隔符。
- 转义：`\n`、`\t`、`\r`、`\\`、`\"`、`\{`、`\}`、`\u{1F600}`。
- 插值：`{expr}`，表达式类型必须实现 `Show`。要输出字面花括号，写 `\{`。
- 原始字符串 `r"..."`：反斜杠和花括号都是字面量；不能包含 `"`。
- 多行字符串 `"""..."""`：去掉第一行换行和最后一行的缩进基准；内部换行保留。
- 字符串是不可变的 UTF-8 序列；索引按 grapheme cluster 计（§8.1.4）。

## 1.8 单位字面量

单位字面量是 Aevia 区别于普通语言的核心语法糖：

```aevia
9.80665 m/s^2
100 km
3.0e8 m/s
1.5 kg*m^2/s^3
25 °C
50 %
2.5 µm
1.2 GeV
```

### 1.8.1 识别规则

**数值字面量后面紧邻的标识符序列，一律按单位字面量解析，不按乘法解析。**
也就是说：

```aevia
3 m          # 量：3 米
3 * m        # 值 3 乘以单位值 m；结果同为 3 米，但显示单位可能不同
3 x          # 语法上是“3 个名为 x 的单位”；若 x 不是单位，报 E0501
```

这条规则是上下文无关的：词法/语法阶段不需要知道哪些名字是单位；名字解析阶段
再检查 `x` 是否在单位命名空间中。这样单位声明可以后置，解析器也不需要回溯。

单位字面量的语法：

```
quantity_lit ::= number unit_expr
unit_expr    ::= unit_factor { ("*" | "·" | "/") unit_factor }
unit_factor  ::= unit_atom [ "^" exponent | SUPERSCRIPT+ ]
unit_atom    ::= unit_name | "(" unit_expr ")" | "%"
unit_name    ::= UNIT_NAME | IDENT | qname
exponent     ::= ["-"] integer | "(" rational ")"
```

- `number` 是 §1.6 的 `INT` 或 `FLOAT`。
- **触发条件**：数值字面量后紧邻 `IDENT`、`UNIT_NAME`（`°C`）、`%` 或
  `(` 时，进入单位字面量；否则是普通数值。
- `*` 与 `·` 同义；`/` 左结合：`kg*m^2/s^3` = `(kg * m^2) / s^3`。
- `^` 的指数必须是整数字面量或括号中的有理数字面量：`m^2`、`s^-1`、
  `m^(1/2)`。不允许变量指数（那会变成动态量纲，用 `Dyn`，§4.9）。
- 上标序列是 `^n` 的等价写法：`m²`、`s⁻¹`、`m³·s⁻²`。
- 单位表达式内**不**允许空白并置：`kg m^2` 不是 `kg*m^2`，必须写
  `kg*m^2` 或 `kg·m^2`。这条限制让 `3 m x` 不会被误读为 `3*(m*x)`。
- 单位表达式内不允许数字，因此 `3 m/2 s` 中单位部分只取 `m`，整个表达式
  解析为 `(3 m) / (2 s)`。
- `1/2 m` 解析为 `1 / (2 m)`，类型是 `Real[1/L]`；要写“半米”，写
  `(1/2) m`。格式化器 SHOULD 在这种模式上加括号。
- 括号只用于单位表达式：`3 (m/s)` 合法；`3 (2)` 不合法（括号里不是单位）。

### 1.8.2 单位名与限定

- 单位名在**单位命名空间**中解析，与普通值命名空间分离。`let s = 5` 不会
  影响 `3 s`（仍是 3 秒）；要乘以变量 `s`，写 `3 * s`。
- 单位名可以带前缀：`km`、`MHz`、`µs`。前缀解析采用最长匹配（§5.3）。
- 单位名可以带限定路径：`Units.SI.km`。在单位字面量中，限定名只允许出现在
  单位位置，且不能与字段访问混淆；`3 Units.SI.km` 是合法的，`3 p.x` 不是
  （`p` 不是单位）。
- 单位名大小写敏感；`Pa`（帕斯卡）与 `pa` 不同。
- `%` 是单位，不是取模运算符。取模用 `mod(a, b)` 或 `rem(a, b)`。
  `50%` 中 `%` 紧邻数字，是单位字面量；`50 %` 也合法。`50 % 3` 是
  “50% 后跟 3”，语法错误。要取模，写 `mod(50, 3)`。

### 1.8.3 上标与特殊字符

单位表达式支持上标数字，作为 `^n` 的等价写法：

```
m²      = m^2
s⁻¹     = s^-1
m³·s⁻²  = m^3 * s^-2
```

上标字符：`⁰¹²³⁴⁵⁶⁷⁸⁹` 和 `⁻`、`⁺`。上标序列只能出现在单位名之后，
不能出现在普通表达式中。`x²` 不是合法表达式；写 `x^2`。

特殊单位字符：

- `°C`、`°F`：摄氏度、华氏度；`°` 单独出现表示角度单位 `deg`。
- `Ω`：欧姆，等价于 `ohm`。
- `Å`：埃，等价于 `angstrom`。
- `µ`（U+00B5）与 `μ`（U+03BC）都接受为微前缀，规范化为 `µ`。

### 1.8.4 单位字面量的类型与值（预览）

- `3 m` 的类型默认是 `Real[L]`，值 3（相干 SI）；在期望 `Float[L]` 处可
  实例化为 `Float[L]`。
- `3.0 m` 的类型默认是 `Float[L]`，值 3.0；在期望 `Real[L]` 处可实例化为
  精确的 `Real[L]`。
- `25 °C` 的类型是 `Point[Θ]`（仿射点），值是 298.15 K（§5.6）。
- `1.5 kg*m^2/s^3` 的类型默认是 `Float[M*L^2/T^3]`。
- `50 %` 的类型默认是 `Real[1]`，值是 0.5；期望 `Float` 时是 `Float[1]`。

显示单位（`m`、`km`、`°C`）不进入类型；它是值的元数据，`to(unit, x)` 可以
改变它（§5.7）。

## 1.9 运算符与标点

| 记号 | 含义 | 结合性/优先级（见 §2.9） |
|---|---|---|
| `+` `-` | 加、减、一元正负 | 左结合，优先级 6 |
| `*` `/` `·` `×` `÷` | 乘、除 | 左结合，优先级 7 |
| `@` | 矩阵乘法 | 左结合，优先级 7 |
| `^` | 幂 | 右结合，优先级 9 |
| `%` | 百分号单位（紧邻数字） | 单位字面量的一部分 |
| `==` `!=` `<` `<=` `>` `>=` | 比较 | 非结合，优先级 4 |
| `&&` | 逻辑与 | 右结合，优先级 3 |
| `\|\|` | 逻辑或 | 右结合，优先级 2 |
| `!` | 逻辑非、阶乘（后缀） | 前缀优先级 8；后缀仅用于 `Int` |
| `\|>` | 管道 | 左结合，优先级 1 |
| `<\|` | 反向应用 | 左结合（混用时按顺序），优先级 1 |
| `::` | 列表 cons | 右结合，优先级 5 |
| `..` `..=` | 半开/闭区间 | 非结合，优先级 5 |
| `->` | 函数类型、match 分支 | — |
| `=>` | lambda 体 | — |
| `<-` | do 绑定 | — |
| `=` | 定义/记录字段 | — |
| `:` | 类型标注 | — |
| `?` | 错误传播（后缀） | 后缀优先级 10 |
| `√` | 平方根前缀 | 前缀优先级 8 |

同义写法：`·`/`×` = `*`，`÷` = `/`，`−` = `-`，`≤` = `<=`，`≥` = `>=`，
`≠` = `!=`。这些同义记号在词法阶段归一化为 ASCII 记号，但保留源位置用于
诊断和格式化。

`!` 的二义性：前缀 `!x` 是逻辑非；后缀 `n!` 是阶乘，仅当 `n` 是 `Int` 且
后面不跟表达式时。`n!!` 是双阶乘。为避免歧义，`!` 后缀只允许紧邻整数
字面量或括号表达式：`5!`、`(n+1)!`；`n !` 是逻辑非的应用？不，`n !` 是
语法错误。建议用 `factorial(n)` 明确表达。

## 1.10 词法错误

| 代码 | 情况 |
|---|---|
| `E0101` | 缩进中出现制表符 |
| `E0102` | 缩进不匹配（DEDENT 找不到对应层级） |
| `E0103` | 期待缩进块但下一行没有缩进 |
| `E0104` | 未闭合的字符串/块注释/括号 |
| `E0105` | 非法字符 |
| `E0106` | 非法数值字面量（下划线位置、缺少指数位） |
| `E0107` | 非法上标序列 |
| `E0108` | 单位字面量中缺少单位（`3` 后面直接跟 `+` 不算） |

词法错误 MUST 报告准确的行列范围，并尽可能给出修复建议。

下一章把这些记号组织成声明、表达式和模式。



<!-- ===== 02-syntax.md ===== -->


# 02 — 语法

本章定义 Aevia 的声明、表达式、模式和类型语法。完整的 EBNF 在
[附录 A](appendix-a-grammar.md)；本章解释**为什么**这样写，以及那些容易歧义
的地方如何解析。

## 2.1 一个源文件的结构

```aevia
module Kinematics            # 可选，必须与文件路径一致

import Units.SI as SI        # 限定访问；单位字面量仍可用
import Math { sqrt, pi }     # 选择性打开
open SI                      # 打开单位值，之后 to(m, x) 可直接用

dimension Currency           # 自定义基量纲
unit EUR : Currency = 1      # 单位声明

type Vec2 = { x: Real[L], y: Real[L] }

fn fall_time(h: Float[L]): Float[T] = sqrt(2 * h / g)

fn main(): IO Unit = do
    ...
```

一个文件是**一个模块**。顶层声明按顺序出现，顺序不影响名字解析（除了单位
声明必须在使用它的单位字面量之前被解析——实现可以在解析完全部声明后统一
解析单位名）。顶层可以是：

- `module`（最多一次，且必须是第一条非注释声明）；
- `import` / `open`；
- `dimension` / `unit`；
- `type` / `class` / `instance` / `derive`；
- `fn` / `let`；
- `test`。

`private` 可以加在 `type`、`unit`、`fn`、`let`、`class` 前，表示不导出。
默认全部导出。

## 2.2 声明

### 2.2.1 函数声明

```aevia
fn kinetic_energy(m: Real[M], v: Real[L/T]): Real[M*L^2/T^2] =
    m * v^2 / 2
```

```
fn_decl ::= "fn" IDENT [ type_params ] "(" [ params ] ")" [ ":" type ] "=" expr
params  ::= param { "," param }
param   ::= pattern [ ":" type ]
```

- 参数可以有类型标注，也可以省略；省略时由函数体和调用点推断。
- 参数可以是模式：`fn norm((x, y): (Real[L], Real[L])): Real[L] = sqrt(x^2 + y^2)`。
- 返回类型标注在 `)` 之后、`=` 之前。
- 函数体是一个表达式；多行时是一个缩进块：

```aevia
fn mean<d>(xs: Tensor<1, Real[d]>): Real[d] =
    let n = length(xs)
    sum(xs) / n
```

- 函数声明是**递归**的：函数名在函数体内可见。
- `fn f(x) = e` 是 `let rec f = fn (x) => e` 的语法糖。

### 2.2.2 值声明

```aevia
let g = 9.80665 m/s^2
let (x, y) = (1.0 m, 2.0 m)
let rec fact = fn (n: Int) => if n == 0 then 1 else n * fact(n - 1)
```

```
let_decl ::= "let" [ "rec" ] pattern [ ":" type ] "=" expr
```

- 普通 `let` **不是**递归的：`let x = x + 1` 中的 `x` 指向外层同名绑定
  （若没有则报未定义）。
- `let rec` 让绑定名在自身定义中可见，主要用于递归 lambda。
- 顶层 `let` 与局部 `let` 语义相同。
- 模式绑定要求右侧类型与模式匹配；不匹配是编译错误（除非模式是 irrefutable
  的，如变量、通配符、记录、元组）。

### 2.2.3 类型声明

```aevia
type Vec2 = { x: Real[L], y: Real[L] }

type Shape =
    | Circle(Real[L])
    | Rect(Real[L], Real[L])

type Result<a, e> = | Ok(a) | Err(e)
```

```
type_decl ::= "type" UIDENT [ type_params ] "=" type_body
type_body ::= type
            | "|" constructor { "|" constructor }
            | NEWLINE INDENT { "|" constructor NEWLINE } DEDENT
constructor ::= UIDENT [ "(" [ type { "," type } ] ")" ]
```

- `type Name = T` 是**透明别名**：`Vec2` 与 `{ x: Real[L], y: Real[L] }`
  完全相同。
- `type Name = | C1(...) | C2(...)` 是**名义变体类型**；构造器是值/函数。
  单行变体 MUST 以 `|` 开头，以便与类型别名区分；缩进形式可以省略首个
  `|`（但建议保留）。
- 递归类型允许，但递归出现必须位于函数箭头的右侧、记录字段、元组或构造器
  参数中（正性检查，§3.7）。
- 构造器名在模块内全局可见；不同模块的同名构造器用 `Module.Ctor` 限定。

### 2.2.4 类型类与实例

```aevia
class Show a where
    show(x: a): String

instance Show for Real[d] where
    show(x) = format_quantity(x)

instance Show for Vec2 where
    show(v) = "({v.x}, {v.y})"
```

```
class_decl    ::= "class" UIDENT type_param { type_param } "where"
                  NEWLINE INDENT { method_sig NEWLINE } DEDENT
method_sig    ::= IDENT ":" type
                 | IDENT "(" [ params ] ")" [ ":" type ]
instance_decl ::= "instance" UIDENT "for" type
                  [ "where" NEWLINE INDENT { method_def NEWLINE } DEDENT ]
method_def    ::= IDENT "(" [ params ] ")" [ ":" type ] "=" expr
```

- 类声明至少有一个类型参数；方法只有签名；默认实现由 `derive` 或插件提供，不在类声明里写。
- 实例头是 `Class Type`；`Real[d]` 中的 `d` 隐式全称量化。
- 实例必须满足一致性（coherence）：同一类、同一类型头只能有一个实例；
  实例必须定义在类型或类所属的模块中（orphan rule，§7.4）。
- `derive` 为记录/变体自动生成常见实例：

```aevia
derive Eq, Show for Vec2
```

### 2.2.5 单位与量纲声明

```aevia
dimension Currency
unit USD : Currency = 1
unit EUR : Currency = 1.08 USD

unit m : L = 1 prefixable
unit km : L = 1000 m
unit degC : Θ = 1 K offset 273.15 K
```

语法与语义见第 05 章。

### 2.2.6 测试声明

```aevia
test "fall time from 20 m" =
    approx_eq(fall_time(20.0 m), 2.019 s, 0.001 s)

test "energy is positive" =
    forall(fn (m: Real[M], v: Real[L/T]) =>
        kinetic_energy(m, v) >= 0 J)
```

`test` 的名称是字符串，体是 `Bool` 表达式。`aevia test` 收集并运行所有
`test`。`forall` 是标准库的属性测试辅助函数，不是语言关键字（§8.13）。

### 2.2.7 属性

声明和函数可以带属性：

```aevia
@stable
@deprecated("use new_api")
fn old_api(x: Real[1]): Real[1] = ...

@jit
fn hot(x: Float[1]): Float[1] = ...

@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))
```

```
attribute ::= "@" IDENT [ "(" [ expr { "," expr } ] ")" ] NEWLINE?
top_decl  ::= { attribute } [ "private" ] decl
```

- `@stable`/`@experimental`/`@deprecated` 是稳定性标记（§13.8）。
- `@jit` 请求 JIT；后端没有 `core.jit` 时报 `E1001`。
- `@op` 把函数注册为 RSSN 算子；`@rule` 注册重写规则（§13.5）。
- 未知属性报 `E0210`；属性参数必须是编译期常量。

## 2.3 表达式

### 2.3.1 块

块是一串 `let`/`fn` 声明后跟一个最终表达式：

```aevia
let y =
    let a = 1
    let b = 2
    a + b
```

```
block ::= INDENT { block_stmt } expr DEDENT
block_stmt ::= let_decl NEWLINE | fn_decl NEWLINE
```

块的值是最后一个表达式的值。块中 `let` 的作用域延伸到块尾。块必须至少有一个
最终表达式；只有 `let` 的块是语法错误（`E0201`）。

### 2.3.2 `let ... in`

单行表达式可以用 ML 风格的 `let ... in`：

```aevia
let x = let y = 2 in y^2 + 1
```

```
let_expr ::= "let" pattern [ ":" type ] "=" expr "in" expr
```

`let ... in` 是非递归的。递归用块中的 `let rec` 或 `fn`。

### 2.3.3 `if`

```aevia
let s = if x > 0 then "positive" else "non-positive"

let y =
    if x > 0 then
        sqrt(x)
    else
        -sqrt(-x)
```

```
if_expr ::= "if" expr "then" expr "else" expr
```

`if` 是表达式，必须有 `else`。两个分支的类型必须统一（量纲也要统一）。
`else if` 可以连写。

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

```
match_expr ::= "match" expr "with" NEWLINE INDENT { match_arm } DEDENT
match_arm  ::= "|" pattern [ "if" expr ] "->" expr NEWLINE?
```

- 分支按顺序尝试；`_` 是通配符。
- 守卫 `if` 在模式匹配成功后求值，必须是 `Bool`。
- 穷尽性由编译器检查；非穷尽是错误 `E0309`，冗余分支是警告 `W0201`。
- 分支体可以是块。

### 2.3.5 `fn` 表达式（lambda）

```aevia
let square = fn (x: Real[L]) => x^2
let add = fn (x, y) => x + y
let compose = fn (f, g) => fn (x) => f(g(x))
```

```
fn_expr ::= "fn" ( "(" [ params ] ")" | IDENT ) [ ":" type ] "=>" expr
```

- 参数类型可省略；返回类型可省略。
- 多行 lambda：

```aevia
let f = fn (x: Real[L]) =>
    let y = x^2
    y + 1
```

- 函数是闭包；捕获是词法的。
- 没有自动柯里化：`fn (x, y) => ...` 接受两个参数，不是 `x -> y -> ...`。
  需要柯里化时用 `curry`/`uncurry`（§8.3）。

### 2.3.6 函数调用与占位符

```aevia
f(x)
f(x, y)
g()(x)                 # 调用返回的函数
map(_, xs)             # 等价于 fn f => map(f, xs)
map(f, _)              # 等价于 fn xs => map(f, xs)
```

- 调用必须写括号。`f x` 不是调用。
- 参数之间用逗号分隔；没有可变参数。
- `_` 作为实参表示“这里缺一个参数”，整个调用变成 lambda，参数按 `_` 出现
  的顺序排列。`_` 不能与普通表达式混用在同一参数位置。
- 调用是左结合的：`f(x)(y)` 先调用 `f(x)`，再调用结果。

### 2.3.7 运算符与管道

```aevia
let y = x |> square |> sqrt          # sqrt(square(x))
let z = sqrt <| square <| x          # sqrt(square(x))
let w = xs |> map(f) |> sum
```

- `a |> f` 等价于 `f(a)`；`f <| a` 等价于 `f(a)`。
- `|>` 与 `<|` 同级，混用时按出现顺序左结合；建议只用一种方向。
- 管道让“数据流”从左到右阅读，适合多步变换。

### 2.3.8 记录

```aevia
let p = { x = 1.0 m, y = 2.0 m }
let q = { p with x = 3.0 m }
let r = { x, y }                      # 字段 punning
let a = p.x
```

```
record_expr   ::= "{" [ record_body ] "}"
record_body   ::= expr "with" field_init { "," field_init }
                | field_init { "," field_init }
field_init    ::= IDENT "=" expr | IDENT
```

- 记录是结构化的；字段顺序不影响类型相等。
- `{ p with x = ... }` 产生一个新记录，未列出的字段保持不变；可以新增字段。
- 字段访问 `p.x` 是后缀运算符，优先级最高。
- 空记录 `{}` 是合法的，类型为 `{}`。

### 2.3.9 变体构造

```aevia
let s = Circle(1.0 m)
let o = Some(3)
let n = None
```

构造器是普通函数/值；`Circle` 的类型是 `Real[L] -> Shape`，`None` 是
`Option<a>` 的多态值。构造器名大写开头是约定，不是语法要求。

### 2.3.10 张量

```aevia
let v = [1.0, 2.0, 3.0]              # Tensor<[3], Float>
let m = [[1.0, 2.0], [3.0, 4.0]]     # Tensor<[2,2], Float>
let e: Tensor<1, Float> = []         # 空张量需要标注
let z = zeros(3, 4)                  # 默认 Float；Tensor<2, Float>
let q = zeros<Int>(3, 4)             # 显式元素类型
```

```
tensor_lit ::= "[" [ expr { "," expr } ] "]"
```

- 嵌套的方括号字面量构造 rank-n 张量；必须是矩形，否则 `E0203`。
- 元素类型由所有元素统一推断；`[1, 2.0]` 统一为 `Float`。
- 空字面量 `[]` 的元素类型和 rank 无法推断，必须有类型标注。
- 显式类型实参写作 `f<T>(...)`，`<` 必须紧邻函数名，不能有空格；
  `a < b` 比较运算要求 `<` 前有空格或左侧不是可类型应用的名字。
- `zeros`、`ones`、`fill` 等构造器在没有约束时默认 `Float`。

### 2.3.11 索引与切片

```aevia
let x = v[0]                         # 元素
let row = m[1]                       # rank 2 -> rank 1
let a = m[0, 1]                      # 元素
let col = m[.., 1]                   # 列
let block = m[0..2, 1..3]            # 子矩阵
let last = v[-1]                     # 负索引：最后一个
```

```
index ::= "[" slice { "," slice } "]"
slice ::= [ expr ] [ ".." [ expr ] | "..=" expr ]
```

- 索引从 0 开始；负索引从末尾计。
- `..` 是半开区间，`..=` 是闭区间；省略端点表示到边界。
- `a[i]` 在 rank 1 上取元素，在 rank > 1 上取第 i 个切片（rank 减一）。
- `a[i, j]` 要求 rank 2；更多下标依此类推。
- 越界是运行时错误 `E0802`；编译器在索引为字面量时可以静态检查。
- 切片产生新张量（语义上是复制；运行时在可能时共享底层缓冲区）。

### 2.3.12 `do` 与 IO

```aevia
fn main(): IO Unit = do
    let name = "data.csv"
    text <- read_file(name)
    let rows = parse_csv(text)
    println("rows = {length(rows)}")
    return ()
```

```
do_expr ::= "do" NEWLINE INDENT { do_stmt } DEDENT
do_stmt ::= "let" pattern [ ":" type ] "=" expr NEWLINE
          | pattern "<-" expr NEWLINE
          | expr NEWLINE
```

- `x <- action` 绑定 `IO` 动作的结果。
- `let` 是纯绑定。
- 裸 `IO` 表达式按顺序执行，结果被丢弃（若类型不是 `IO Unit`，警告
  `W0202`）。
- `return(e)` 是 Prelude 函数，类型 `a -> IO a`，不是关键字。
- `do` 块的值是最后一个 `expr` 语句的值；最后一条是 `let` 时是 `()`。
- `do` 只接受 `IO`；类型检查器拒绝其他 monad。

### 2.3.13 错误传播 `?`

```aevia
fn load(path: String): Result<Table, IOError> =
    let text = read_file(path)?          # read_file : String -> Result<String, IOError>
    let rows = parse_csv(text)?
    Ok(rows)
```

`e?` 要求 `e : Result<T, E>`，所在函数返回 `Result<U, E>`；成功时得到 `T`，
失败时提前返回 `Err(e)`。`?` 也可以用于 `Option`，所在函数返回 `Option`。

### 2.3.14 其他表达式

```aevia
()                                   # unit
(1, 2)                               # tuple
(1)                                  # 括号，不是 tuple
x : Real[L]                          # 类型标注
x!                                   # 阶乘（Int）
√x                                   # sqrt(x)
```

- 元组用逗号；`(1,)` 是单元素元组（当前规范要求 `(1, ())` 或记录）。
  为避免歧义，当前规范 **不支持**单元素元组；用记录或直接返回值。
- 类型标注 `e : T` 优先级最低，作用于整个表达式。
- `x!` 仅当 `x : Int`；`x!!` 是双阶乘。
- `√x` 是 `sqrt(x)` 的前缀语法糖。

## 2.4 模式

```aevia
fn f(p: (Real[L], Real[L])): Real[L] =
    let (x, y) = p
    sqrt(x^2 + y^2)

match shape with
| Circle(r) -> ...
| Rect(w, h) if w > h -> ...
| _ -> ...

let { x, y } = point
let [a, b, ..rest] = xs
```

```
pattern ::= "_" | IDENT | UIDENT | literal
          | UIDENT [ "(" [ pattern { "," pattern } ] ")" ]
          | "{" [ field_pat { "," field_pat } ] "}"
          | "(" pattern { "," pattern } ")"
          | "[" [ pattern { "," pattern } [ "," ".." IDENT ] ] "]"
          | pattern "as" IDENT
          | "(" pattern ":" type ")"
field_pat ::= IDENT [ "=" pattern ]
```

- 变量模式绑定名字；`_` 不绑定。
- 构造器模式按名称匹配；参数模式递归匹配。
- 记录模式只匹配列出的字段（开放匹配）；`{ x, y }` 绑定同名字段。
- 张量模式只支持 rank 1 的字面量结构：`[a, b, ..rest]`。
- `p as x` 在匹配 `p` 的同时把整个值绑定到 `x`。
- 浮点字面量不能作为模式（NaN 与精确相等的问题）；用守卫。
- 模式匹配必须穷尽；记录模式和变量模式是 irrefutable 的。

## 2.5 类型语法

```aevia
Real[L]
Real[L/T]
Real[M*L^2/T^2]
Float[1]
Tensor<2, Float[L]>
Tensor<n, Real[d]>
{ x: Real[L], y: Real[L] }
{ x: Real[L], ...r }
(a, b) -> c
forall d. Real[d] -> Real[d^2]
Point[Θ]
Result<T, E>
IO Unit
```

```
type ::= [ "forall" type_param { "," type_param } "." ] fn_type
fn_type ::= type_atom [ "->" type ]
type_atom ::= "Int" | "Real" [ "[" dim "]" ] | "Float" [ "[" dim "]" ]
            | "Complex" [ "[" dim "]" ] | "Bool" | "Prop" | "String" | "()"
            | "Tensor" "<" shape "," type ">"
            | "Matrix" "<" nat "," nat "," type ">"
            | "Sparse" "<" format_name "," shape "," type ">"
            | "Table" "<" row_type ">"
            | "Point" "[" dim "]"
            | "Unit" [ "[" dim "]" ]
            | "Set" "<" type ">" | "Result" "<" type "," type ">"
            | "Option" "<" type ">" | "Solutions" "<" type "," dim ">"
            | "Dyn" "<" rep ">" | "RateOf" "<" type ">"
            | "SymFun" "<" dim "," dim ">" | "IO" type
            | "{" [ field_type { "," field_type } [ "," "..." IDENT ] ] "}"
            | "(" type { "," type } ")"
            | qname [ "<" type_arg { "," type_arg } ">" ]
            | "(" type ")"
field_type ::= IDENT ":" type
qname ::= UIDENT { "." UIDENT }
format_name ::= IDENT | UIDENT
shape ::= nat | "?" | "[" [ shape_item { "," shape_item } ] "]"
shape_item ::= nat | "?"
row_type ::= type { "+" type }
```

注意：类型级乘积 `A * B` **不**表示元组；元组用 `(A, B)`。`*` 只出现在
量纲表达式里。

- `Real` 不带方括号等价于 `Real[1]`；`Float`、`Complex` 同理。
- `Tensor<2, Float>` 的 `2` 是类型级自然数（kind `Nat`）。
- `Point[Θ]` 是仿射点（§5.6）。
- `Unit[d]` 是单位值的类型（§5.2）；`Unit` 不带方括号等价于 `()`。
- 类型参数 `<d: Dim>`、`<n: Nat>`、`<r: Row>` 的 kind 可省略，由使用位置
  推断。
- `forall` 通常省略；声明上的 `<...>` 已经隐含全称量化。

### 2.5.1 量纲语法

```
dim ::= dim_term { ("*" | "/" | "·") dim_term }
dim_term ::= dim_atom [ "^" exponent ]
dim_atom ::= "1" | UIDENT | IDENT | "(" dim ")"
exponent ::= ["-"] integer | "(" rational ")"
```

- `L/T` 左结合：`L/T^2` = `L/(T^2)`。
- `L^2/T` = `(L^2)/T`。
- `L^(1/2)` 是 `L` 的平方根量纲；`sqrt` 产生它。
- `1` 是无量纲。
- 量纲别名（`Velocity`、`Force`）在类型检查时展开；打印时优先显示别名。
- 量纲变量 `d` 只能出现在 `[...]` 中；裸写 `d` 是类型变量，不是量纲。

## 2.6 运算符优先级与结合性

从低到高：

| 级别 | 运算符 | 结合性 | 说明 |
|---|---|---|---|
| 1 | `\|>` `<\|` | 左（混用时按出现顺序） | 管道 |
| 2 | `\|\|` | 右 | 逻辑或 |
| 3 | `&&` | 右 | 逻辑与 |
| 4 | `==` `!=` `<` `<=` `>` `>=` | 非结合 | 比较，不能连写 |
| 5 | `::` | 右 | cons |
| 5 | `..` `..=` | 非结合 | 仅索引中使用 |
| 6 | `+` `-` | 左 | 加、减 |
| 7 | `*` `/` `@` `·` `×` `÷` | 左 | 乘、除、矩阵乘 |
| 8 | `-` `+` `!` `√`（前缀） | 前缀 | 一元 |
| 9 | `^` | 右 | 幂 |
| 10 | `f(...)` `a[i]` `p.x` `e?` `e!` | 后缀 | 调用/索引/字段/传播/阶乘 |

- `-x^2` = `-(x^2)`。
- `2^3^2` = `2^(3^2)`。
- `a * b @ c` = `(a * b) @ c`（同级别左结合）。矩阵乘法建议加括号。
- `x == y == z` 是语法错误；写 `x == y && y == z`。
- `if`/`match`/`fn`/`let` 的体尽可能向右延伸；用括号或块消除歧义。
- `|` 只用于 match 分支和变体声明，不是逻辑或。

## 2.7 语法歧义与解决

### 2.7.1 数值后的标识符

`3 m` 是单位字面量，不是 `3 * m`。`3 x` 在语法上也是单位字面量；若 `x`
不是单位，报 `E0501`。要乘以变量，写 `3 * x`。这条规则在词法章已经固定，
解析器不需要符号表。

### 2.7.2 类型实参的 `<`

`zeros<Float>(3, 4)` 中 `<` 紧邻 `zeros`，按类型实参解析。`a < b` 中 `<`
前有空格，按比较解析。解析器 SHOULD 在 `ident<` 后尝试类型实参；若失败，
回退为比较。这条规则要求格式化器在 `<` 比较前保留空格。

### 2.7.3 `{` 记录与块

块只用缩进，不用花括号。`{}` 永远是记录。因此 `if x then { y = 1 } else { y = 2 }`
是记录表达式，不是块。

### 2.7.4 `|` 的多重角色

- `match` 分支：`| pattern -> expr`。
- 变体声明：`| Ctor(T)`。
- 逻辑或：`||`（两个字符）。
- 类型 union？当前规范没有匿名 union；用变体类型。

解析器根据上下文区分；`|` 不会出现在表达式中作为二元运算符。

### 2.7.5 `!` 的前缀与后缀

- `!x` 是逻辑非。
- `x!` 是阶乘，仅当 `x` 是 `Int` 或括号表达式，且后面不跟另一个表达式。
- `x != y` 是不等号，词法阶段识别为单个 `!=` 记号。
- `x!!` 是双阶乘；`!!x` 是双重逻辑非（合法但无意义）。

### 2.7.6 `-` 的一元与二元

标准规则：一元 `-` 优先级低于 `^`，高于 `*`。`-2^2` = `-(4)`；
`(-2)^2` = `4`。`a - -b` 合法，等价于 `a - (-b)`。

## 2.8 语法糖与去糖

| 语法 | 去糖 |
|---|---|
| `fn f(x) = e` | `let rec f = fn (x) => e` |
| `a \|> f` | `f(a)` |
| `f <\| a` | `f(a)` |
| `√x` | `sqrt(x)` |
| `x!` | `factorial(x)` |
| `x!!` | `double_factorial(x)` |
| `a @ b` | `matmul_rank(a, b)`（按秩分派到 matmul/matvec/vecmat/dot） |
| `a * b`（张量） | `mul_elementwise(a, b)` |
| `{ p with x = e }` | `{ ...p, x = e }` 的记录更新原语 |
| `f(_, x)` | `fn a => f(a, x)` |
| `do ...` | `bind`/`return` 链 |
| `[a, b, c]` | `tensor([a, b, c])` |
| `a[i, j]` | `index(a, [i, j])` |
| `a[i..j]` | `slice(a, i, j)` |

去糖后的形式不是用户语法；它用于类型检查和解释器的核心 IR。

## 2.9 一个完整例子

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

这个例子用到了：记录、字段访问、单位、量纲推断、`Float` 与 `Real` 的混合、
张量、ODE 积分、`do` 和字符串插值。第 11 章会逐行解释。

下一章进入类型系统：这些表达式的类型从哪里来，量纲如何参与推断。



<!-- ===== 03-types.md ===== -->


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



<!-- ===== 04-dimensions.md ===== -->


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



<!-- ===== 05-units.md ===== -->


# 05 — 单位

第 04 章把量纲变成了类型；本章把单位变成**编译期已知的尺度**。一个单位不
进入类型，它只提供三样东西：

1. 一个量纲 `d`；
2. 一个尺度 `s : Real[1]`（相对相干 SI 单位的倍数）；
3. 一个仿射偏移 `o : Real[d]`（仅仿射单位）。

单位字面量 `3.2 km` 在编译期折叠为 `3200.0`（相干 SI 值），类型是 `Float[L]`。
`km` 本身不留在值里；需要显示时，用 `to(km, x)` 把显示单位附加到值上。

## 5.1 单位声明

```aevia
dimension Currency                  # 自定义基量纲
unit USD : Currency = 1             # 基单位
unit EUR : Currency = 1.08 USD      # 派生单位

unit m : L = 1 prefixable           # 基单位，允许 SI 前缀
unit km : L = 1000 m                # 显式派生
unit h : T = 3600 s
unit N : M*L/T^2 = 1 kg*m/s^2
unit degC : Θ = 1 K offset 273.15 K # 仿射单位
```

### 5.1.1 语法

```
dimension_decl ::= [ "private" ] "dimension" UIDENT [ "=" dim ]
unit_decl ::= [ "private" ] "unit" unit_name ":" dim "=" unit_rhs [ "prefixable" ]
unit_rhs  ::= unit_scale [ unit_expr ] [ "offset" unit_scale unit_expr ]
unit_scale ::= number [ "/" number ] | "pi" [ "/" number ]
```

- `dimension`/`unit` 默认**导出**；`private` 只在本模块可见。导出的
  量纲/单位进入 Kernel Manifest 和 registry，下游包可以直接 `import`。
- 用户自定义量纲别名可以导出（例如 `dimension Currency`），不需要改
  标准库；`Units.SI` 只是 registry 的一个视图。

- `unit_name` 是标识符或带 `°` 的特殊名（`°C`、`°F`）。
- `unit_scale` 是正有理数，或 `pi` 的有理倍数。`1/1000`、`pi/180` 合法；
  `1.5` 合法（按精确十进制转有理数？见下）。
- `unit_expr` 是单位表达式（§5.2）。省略时表示基单位，尺度必须是 1，
  量纲必须是基量纲。
- `offset` 只用于仿射单位；偏移量的量纲必须等于单位量纲。
- `prefixable` 表示自动生成 SI 前缀版本。

**尺度精度**：`unit_scale` 中的十进制字面量按**精确十进制**解释（`1.5` 是
`3/2`，`0.3048` 是 `381/1250`），不是 `Float`。这让 `ft → m` 这类换算精确。
单位声明是编译期求值，不允许运行时表达式或 `approx`；需要近似尺度时，
写足够精度的十进制字面量（它仍按精确有理数处理）。

### 5.1.2 基单位与派生单位

- `unit m : L = 1` 声明 `m` 是 `L` 的相干基单位，尺度 1。
- `unit km : L = 1000 m` 声明派生单位，尺度 1000。
- 一个量纲可以有多个基单位吗？MUST NOT。每个基量纲有且只有一个尺度为 1 的
  基单位；其他都是派生。重复声明报 `E0505`。
- 派生单位的尺度在编译期计算；循环定义报 `E0506`。
- 单位声明可以跨模块导入；`private unit` 不导出。

### 5.1.3 单位命名空间

单位名住在**独立的命名空间**中：

- 单位字面量 `3 m` 中的 `m` 在单位命名空间解析。
- 普通表达式中的 `m` 是值命名空间中的单位值 `Unit[L]`（如果 `open` 了）。
- `let m = 5` 不会改变 `3 m` 的含义（仍是 3 米）；要乘以变量，写 `3 * m`。
- 单位名冲突时，最长匹配优先；`ms` 如果是毫秒，就解析为毫秒，而不是
  `m*s`。要写米乘秒，写 `m*s`。
- 单位名可以限定：`Units.SI.km`。在字面量中限定名只允许出现在单位位置；
  推荐 `open Units.SI` 后使用短名。

### 5.1.4 前缀

`prefixable` 的单位自动生成 SI 前缀版本：

| 前缀 | 符号 | 因子 |
|---|---|---|
| quetta | `Q` | 10^30 |
| ronna | `R` | 10^27 |
| yotta | `Y` | 10^24 |
| zetta | `Z` | 10^21 |
| exa | `E` | 10^18 |
| peta | `P` | 10^15 |
| tera | `T` | 10^12 |
| giga | `G` | 10^9 |
| mega | `M` | 10^6 |
| kilo | `k` | 10^3 |
| hecto | `h` | 10^2 |
| deca | `da` | 10^1 |
| deci | `d` | 10^-1 |
| centi | `c` | 10^-2 |
| milli | `m` | 10^-3 |
| micro | `µ` | 10^-6 |
| nano | `n` | 10^-9 |
| pico | `p` | 10^-12 |
| femto | `f` | 10^-15 |
| atto | `a` | 10^-18 |
| zepto | `z` | 10^-21 |
| yocto | `y` | 10^-24 |
| ronto | `r` | 10^-27 |
| quecto | `q` | 10^-30 |

规则：

- 前缀只加在 `prefixable` 单位上；`kg` 是特例：SI 基单位是千克，但前缀
  加在 `g` 上（`mg`、`µg`、`kg`）。`g` 声明为 `1/1000 kg prefixable`，
  `kg` 本身是基单位；解析 `kg` 时优先精确匹配基单位，而不是 `k`+`g`。
- 前缀解析最长匹配：`dam` 是 `da`+`m`（十米），不是 `d`+`am`。
- 前缀不能叠加：`kkm` 非法 `E0503`。
- 二进制前缀 `Ki`、`Mi`、`Gi`、`Ti` 只用于信息单位（`bit`、`B`），不用于
  物理单位。
- 前缀单位与显式单位冲突时（例如同时声明 `unit ms : T = 1/1000 s` 和
  `prefixable s`），显式声明优先；若两者尺度不同，报 `E0507`。

## 5.2 单位表达式

```
unit_expr ::= unit_factor { ("*" | "·" | "/") unit_factor }
unit_factor ::= unit_atom [ "^" exponent ]
unit_atom ::= unit_name | "(" unit_expr ")" | "%"
```

- `*` 与 `·` 同义；`/` 左结合。
- 指数是整数字面量或括号中的有理数：`m^2`、`s^-1`、`m^(1/2)`。
- 单位表达式内不允许数字（除了指数），不允许空白并置。
- 仿射单位（`degC`、`degF`）只能单独出现；出现在乘积/幂中报 `E0508`。
- 单位表达式可以包含单位变量：
  ```
  let u = km/h
  let v = 100 * u
  ```
  但 `100 u` 不是单位字面量（`u` 不是单位名），会报 `E0501`；写
  `100 * u`。

### 5.2.1 单位值

单位表达式在值层是一个 `Unit[d]` 值：

```aevia
let u = km / h          # Unit[L/T]，尺度 1000/3600
let v = 100 * u         # Real[L/T]，值 27.777...
```

`Unit[d]` 支持：

```
u * v : Unit[d*e]
u / v : Unit[d/e]
u ^ n : Unit[d^n]       # n 字面量
```

单位值可以比较（同量纲）、打印、作为参数传递。单位不是数值，不能直接与
`Real` 相加。

### 5.2.2 单位字面量的求值

`number unit_expr` 的求值：

```
value = number * scale(unit_expr) + offset(unit_expr)
type  = Scalar<rep(number), dim(unit_expr)>
```

- 线性单位：`offset = 0`。`3 km` → `3000`，`Real[L]`。
- 仿射单位：只有单独单位时合法。`25 °C` → `25 * 1 + 273.15 = 298.15`，
  `Point[Θ]`。
- `number` 是 `Int` 时，结果默认 `Real`（精确）；`number` 是 `Float` 时，
  结果默认 `Float`。`3 km` 默认 `Real[L]`，`3.0 km` 默认 `Float[L]`。
- 期望类型可以覆盖表示：`fall_time(20 m)` 中 `h : Float[L]`，`20 m` 实例化
  为 `Float[L]`；`let x: Real[L] = 3.0 m` 把 `3.0 m` 实例化为精确的
  `Real[L]`（`3`）。
- 尺度是 `pi` 的倍数时，结果可能是符号表达式：`30 deg` → `pi/6`，
  `Real[1]`；期望 `Float` 时求值为 `0.5235987755982988`。

## 5.3 单位表

标准库 `Units.SI` 提供以下单位（节选）：

### 5.3.1 SI 基单位

| 单位 | 量纲 | 说明 |
|---|---|---|
| `m` | `L` | 米，prefixable |
| `kg` | `M` | 千克（基单位；`g` prefixable） |
| `s` | `T` | 秒，prefixable |
| `A` | `I` | 安培，prefixable |
| `K` | `Θ` | 开尔文，prefixable |
| `mol` | `N` | 摩尔，prefixable |
| `cd` | `J` | 坎德拉，prefixable |

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
| `lm` | `J` | `cd*sr` |
| `lx` | `J/L^2` | `lm/m^2` |
| `Bq` | `1/T` | `1/s` |
| `Gy` | `L^2/T^2` | `J/kg` |
| `Sv` | `L^2/T^2` | `J/kg` |
| `kat` | `N/T` | `mol/s` |
| `rad` | `1` | 无量纲，尺度 1 |
| `sr` | `1` | 无量纲，尺度 1 |

### 5.3.3 常用非 SI 单位

| 单位 | 量纲 | 定义 |
|---|---|---|
| `min` | `T` | `60 s` |
| `h` | `T` | `3600 s` |
| `d` | `T` | `86400 s` |
| `°` / `deg` | `1` | `pi/180` |
| `′` / `arcmin` | `1` | `pi/10800` |
| `″` / `arcsec` | `1` | `pi/648000` |
| `L` | `L^3` | `1/1000 m^3` |
| `t` | `M` | `1000 kg` |
| `eV` | `Energy` | `1.602176634e-19 J` |
| `Da` / `u` | `M` | `1.66053906660e-27 kg` |
| `au` | `L` | `149597870700 m` |
| `pc` | `L` | `3.0856775814913673e16 m` |
| `ly` | `L` | `9460730472580800 m` |
| `Å` / `angstrom` | `L` | `1e-10 m` |
| `bar` | `Pressure` | `100000 Pa` |
| `atm` | `Pressure` | `101325 Pa` |
| `mmHg` | `Pressure` | `133.322387415 Pa` |
| `cal` | `Energy` | `4.184 J` |
| `kWh` | `Energy` | `3.6e6 J` |
| `hp` | `Power` | `745.6998715822702 W` |
| `kn` | `L/T` | `1852/3600 m/s` |
| `mph` | `L/T` | `1609.344/3600 m/s` |
| `ft` | `L` | `0.3048 m` |
| `in` | `L` | `0.0254 m` |
| `yd` | `L` | `0.9144 m` |
| `mi` | `L` | `1609.344 m` |
| `nmi` | `L` | `1852 m` |
| `acre` | `L^2` | `4046.8564224 m^2` |
| `gal` | `L^3` | `0.003785411784 m^3` |
| `oz` | `M` | `0.028349523125 kg` |
| `lb` | `M` | `0.45359237 kg` |
| `psi` | `Pressure` | `6894.757293168 Pa` |
| `°C` | `Θ` | 仿射：`1 K offset 273.15 K` |
| `°F` | `Θ` | 仿射：`5/9 K offset 255.3722222222222 K` |
| `bit` | `1` | 信息，尺度 1 |
| `B` | `1` | `8 bit` |
| `KiB` | `1` | `1024 B` |

信息单位是否应该是无量纲？当前规范是；如果需要独立的 `Information` 基量纲，用户
可以 `dimension Information` 并声明自己的单位。标准库不强制，因为大量
公式把 bit 当无量纲概率/熵使用。

### 5.3.4 单位模块

| 模块 | 内容 |
|---|---|
| `Units.SI` | 基单位、派生单位、前缀、常用非 SI |
| `Units.Imperial` | 英制长度/质量/体积/压强/温度 |
| `Units.Astronomy` | au、pc、ly、Mpc、太阳质量、木星质量 |
| `Units.CGS` | cm、g、dyn、erg、Gal、P |
| `Units.Natural` | `c = 1`、`hbar = 1`、`eV` 系 |
| `Units.Constants` | 物理常数（§5.8） |

`Units.Natural` 通过“单位制”而不是“改常数”实现：它导出一组新的单位，
其中 `c`、`hbar` 的数值是 1。用户 `open Units.Natural` 后，`1 GeV` 等写法
成立；切回 SI 用 `open Units.SI`（名字冲突时限定）。

## 5.4 换算与显示

### 5.4.1 三个操作

```aevia
let d = 3.2 km              # Float[L]，值 3200.0
let e = to(m, d)            # Float[L]，值 3200.0，显示单位 m
let f = value_in(km, d)     # Float[1]，值 3.2
let g = base(d)             # Float[L]，值 3200.0，去掉显示单位
```

- `to(u, x)`：要求 `x : Scalar<ρ,d>` 且 `u : Unit[d]`；返回值不变，只附加
  显示单位。对 `Float` 和 `Real` 都适用。
- `value_in(u, x)`：等价于 `x / u`，返回无量纲数值。
- `base(x)`：去掉显示单位，返回相干 SI 值。
- `to` 不改变类型，也不改变值；它只影响 `show`、`print`、绘图标签。
- **仿射单位只接受 `Point[d]`**：`to(°C, p)`、`value_in(°C, p)` 合法；
  对 `Scalar<ρ,Θ>`（温差）使用仿射单位报 `E0510`。温差显示用
  `delta_to(°C, dT)`，它只用尺度、不用偏移。
- 例子：`value_in(°C, point(298.15, K)) = 25`；`value_in(°C, 298.15 K)`
  是错误（`298.15 K` 是温差）。

### 5.4.2 显示单位的传播

运行时每个标量可以带一个可选的显示单位。规则：

- `x + y` / `x - y`：如果两边显示单位相同，保留；否则丢弃，回到 SI。
- `x * y` / `x / y`：如果一方无量纲且带显示单位，保留另一方的显示；
  否则丢弃。
- `to(u, x)` 显式设置。
- `base(x)` 显式清除。
- 函数调用默认丢弃显示单位（除了 `to`、`value_in`、`abs` 等保持形状的）。

显示单位是**元数据**，不参与相等、类型或序列化。`3.2 km == 3200 m` 为
`true`；打印时按各自的显示单位。

### 5.4.3 打印

```
3.2 km
3200 m
25 °C
1.5e-3 m
2.99792458e8 m/s
```

- 有显示单位时，数值按该单位换算后打印。
- 没有显示单位时，选一个“最佳单位”：优先 SI 派生单位（`N`、`J`、`Pa`），
  否则基单位乘积；指数为 1 的用 `m`、`s`，复杂量纲用 `kg*m/s^2`。
- `show` 保证可读；`repr` 保证可解析回同一个值（带单位文本）。
- 打印精度由 `Float` 的最短往返表示决定；`Real` 精确值打印为分数或根式。

## 5.5 角度与无量纲单位

- `rad` 是无量纲单位，尺度 1。
- `deg` / `°` 是无量纲单位，尺度 `pi/180`。
- `30 deg` 的值是 `pi/6`（符号）或 `0.5235987755982988`（Float）。
- `sin(30 deg)` 合法；`sin(30)` 也合法（30 弧度），因为 `sin` 只要求
  无量纲。这是 SI 的语义，不是漏洞。
- 如果用户想区分“弧度”和“纯数”，用 `rad` 单位标注；类型系统不区分。
- `atan2(y, x)` 返回无量纲弧度；用 `to(deg, ...)` 显示为角度。

## 5.6 仿射单位与 `Point`

### 5.6.1 问题

温度有两种用法：

- **绝对温度**：`T = 298.15 K`，是仿射空间中的一个点；
- **温差**：`ΔT = 25 K`，是向量，可以相加、缩放。

`25 °C` 是绝对温度（273.15 + 25 K），不是 25 K 的温差。如果把它们混为一谈，
`20 °C + 30 °C` 会得到 50 °C（错误）或 596.3 K（也错误）。

### 5.6.2 类型

```
Point[Θ]        绝对温度（仿射点）
Float[Θ]        温差（向量）
```

- 只有仿射单位字面量产生 `Point[d]`：`25 °C`、`77 °F`。
- 线性单位字面量产生 `Float[d]`/`Real[d]`：`25 K` 是温差，不是绝对温度。
- 绝对温度用开尔文写时，用 `point(298.15, K)`。
- `Point` 只支持 `Float` 表示；符号温度用 `Real[Θ]` 的温差。
- `Point` 可以是张量元素：`Tensor<[nx,ny], Point[Θ]>` 是温度场。元素级
  `+`/`-` 按点规则（`Point - Point -> Float[Θ]`，`Point + Float[Θ] ->
  Point`，`Point + Point` 报 `E0405`）。温度场的平均用
  `centroid`，不是 `mean`（§13.9.2）。

### 5.6.3 运算

```
p - q        : Float[d]        两个点相减得到温差
p + x        : Point[d]        点加温差
x + p        : Point[d]
p - x        : Point[d]        点减温差
p + q        : 错误 E0405
k * p        : 错误 E0405
p / k        : 错误 E0405
lerp(p,q,t)  : Point[d]        仿射组合，t : Float[1]
```

- `Point` 可以比较：`p == q`、`p < q`（沿仿射线）。
- `Point` 与温差：`p + x`/`p - x` 只接受 `x : Float[Θ]`（`Point` 本身是
  Float）。精确温差 `Real[Θ]` 必须先 `to_float`（返回 `Result`）；因此
  `25 °C + 10.0 K` 合法，`25 °C + 10 K` 报 `E0302`（提示写 `10.0 K` 或
  `to_float(10 K)`）。
- `min(p,q)`、`max(p,q)` 返回 `Point`；`centroid : Tensor<1,Point[d]> -> Point[d]`
  是点的平均（`mean` 只接受标量）。
- `to(°C, p)` 设置显示单位；`value_in(°C, p)` 返回摄氏数值。
- `base(p)` 返回 `Float[Θ]` 的 K 值（去掉点标记）。
- `Point` 不能与 `Unit` 相乘/相除；`p / K` 报 `E0405`。

构造函数与换算函数：

```
point     : (Float[1], Unit[d]) -> Point[d]
to        : (Unit[d], Scalar<ρ,d>) -> Scalar<ρ,d>
to        : (Unit[d], Point[d]) -> Point[d]
value_in  : (Unit[d], Scalar<ρ,d>) -> Scalar<ρ,1>      # 仿射单位时 E0510
value_in  : (Unit[d], Point[d]) -> Float[1]
base      : Scalar<ρ,d> -> Scalar<ρ,d>
base      : Point[d] -> Float[d]
delta_to  : (Unit[d], Scalar<ρ,d>) -> Scalar<ρ,d>     # 只用尺度，不用偏移
unit_pow_dyn : (Unit[d], Int) -> Dyn<Real>            # 运行时指数
lerp      : (Point[d], Point[d], Float[1]) -> Point[d]
```

`point(300.0, K)` 的第一个参数是以 `K` 为单位的数值系数，类型 `Float[1]`；
`point(25.0, °C)` 是 298.15 K。`delta_to(°C, 10.0 K)` 把温差 10 K 显示为
“10 °C 的温差”（尺度 1，无偏移），与绝对温度 `10 °C` 区分。

### 5.6.4 为什么开尔文是线性的

`K` 是线性单位（尺度 1、偏移 0），所以 `300 K` 在类型上是温差。这看起来
不对称，但它是物理上诚实的：开尔文刻度同时用于绝对温度和温差，单位本身
无法区分。Celsius/Fahrenheit 是仿射刻度，字面量天然是点。要写绝对温度：

```aevia
let T1 = 25.0 °C            # Point[Θ]
let T2 = point(300.0, K)    # Point[Θ]
let dT = T2 - T1            # Float[Θ]
let T3 = T1 + 10.0 K        # Point[Θ]
```

### 5.6.5 仿射单位在单位表达式中

`°C/s`、`°C*m` 没有意义；单位解析器拒绝仿射单位出现在乘积/幂中
（`E0508`）。`°C` 只能单独作为字面量单位，或作为 `to`/`value_in` 的参数。

## 5.7 单位换算的实现

单位值在运行时是：

```
Unit {
    name: String,
    dim: Dim,
    scale: Real,        # 相干 SI 倍数，精确有理或符号
    offset: Option<Real>,  # 仅仿射
    prefixable: bool,
}
```

- 尺度在编译期折叠；`3 km` 直接生成 `3000`，不保留 `km` 的乘法。
- 单位表在编译期是常量；运行时只保留显示单位的名字和换算所需的信息。
- `to(u, x)` 在运行时把 `x` 的显示单位设为 `u`；打印时用 `x / u.scale`。
- 仿射换算：`value_in(°C, p) = (p - 273.15 K) / 1 K`。
- 单位相等：同量纲且尺度相同（仿射还要偏移相同）。`m` 与 `100 cm` 是
  不同单位值，但同量纲；`to` 可以互换。

## 5.8 物理常数

`Units.Constants` 提供 CODATA 2022 的常数（节选）：

| 常数 | 类型 | 值 |
|---|---|---|
| `c` | `Real[L/T]` | `299792458 m/s`（精确） |
| `g_0` | `Real[L/T^2]` | `9.80665 m/s^2`（精确） |
| `h` | `Real[Energy*T]` | `6.62607015e-34 J*s`（精确） |
| `hbar` | `Float[Energy*T]` | `1.054571817e-34 J*s` |
| `k_B` | `Real[Energy/Θ]` | `1.380649e-23 J/K`（精确） |
| `N_A` | `Real[1/N]` | `6.02214076e23 /mol`（精确） |
| `e` | `Real[Charge]` | `1.602176634e-19 C`（精确） |
| `m_e` | `Float[M]` | `9.1093837015e-31 kg` |
| `m_p` | `Float[M]` | `1.67262192369e-27 kg` |
| `G` | `Float[L^3/(M*T^2)]` | `6.67430e-11 m^3/(kg*s^2)` |
| `eps_0` | `Float[Capacitance/L]` | `8.8541878128e-12 F/m` |
| `mu_0` | `Float[Inductance/L]` | `1.25663706212e-6 H/m` |
| `alpha` | `Float[1]` | `7.2973525693e-3` |
| `R` | `Float[Energy/(N*Θ)]` | `8.31446261815324 J/(mol*K)` |
| `F` | `Real[Charge/N]` | `96485.33212 C/mol` |
| `sigma_SB` | `Float[Power/(L^2*Θ^4)]` | `5.670374419e-8 W/(m^2*K^4)` |

- 精确常数用 `Real`（有理数或符号）；测量常数用 `Float`。
- 常数不是单位；它们有量纲，可以参与运算。
- `pi`、`e`（自然常数）、`i` 在 `Math` 中，不在 `Units.Constants`。
  注意 `e` 与基本电荷 `e` 重名；基本电荷用 `e_charge` 或 `q_e`。标准库
  用 `q_e`，避免与自然常数冲突。
- 常数的不确定度：核心语言不携带不确定度；需要时用 `Measurement` 库类型
  （§8.14）。

## 5.9 单位错误

| 代码 | 情况 |
|---|---|
| `E0501` | 未知单位名 |
| `E0502` | 单位未导入/未打开 |
| `E0503` | 前缀不能叠加或单位不可加前缀 |
| `E0504` | 仿射单位偏移量纲不匹配 |
| `E0505` | 重复的基单位 |
| `E0506` | 单位定义循环或尺度量纲不为 1 |
| `E0507` | 前缀单位与显式单位冲突 |
| `E0508` | 仿射单位出现在乘积/幂中 |
| `E0509` | `to` 的单位量纲与值不匹配 |
| `E0510` | 对温差使用仿射单位显示 |

## 5.10 与 rssn 单位层的衔接

rssn 的 `rssn-dim` 提供 `DimTerm`/`DimArena`/`UnitRegistry`；`units` 规则集
只是 registry 的一个消费者。Aevia 的单位层是编译期视图，`Units.SI` 由
`UnitRegistry` 生成，并且有 `Point` 类型。两者衔接方式：

1. **单一 registry。** `Units.SI` 是 `UnitRegistry` 的生成视图；Aevia 不
   维护第二张表。用户 `dimension`/`unit` 声明注册进同一个 registry。
2. **项图不带单位。** `Real`/`Float` 值在宿主侧是相干 SI 数值；需要
   rssn 排版/导出时，才用 `quantity(v, "<相干 SI 单位>")` 包一层，仅用于
   输出，不进入 `compute`。
3. **仿射单位在桥接层消解。** `Point` 在进入 rssn 前转成 K 的 `Float`；
   `to(°C, ...)` 只发生在显示层。
4. **registry 往返。** `DimTerm -> Dimension -> DimTerm` 与
   `UnitDef -> scale/offset -> UnitDef` 必须精确往返；启动时对基单位/
   派生单位/仿射单位做快照校验。
5. **rssn 的 `DimensionMode::Strict` 只用于 FFI/导出。** 静态路径的项图
   无量纲，用 `Off`；`Dyn<ρ>` 的量纲由宿主保存为 `DimTerm`，不依赖字符串。

下一章定义求值：一个 `Real` 值在什么时候是精确数、什么时候是符号项、
什么时候变成 `Float`，以及张量和 IO 如何运行。



<!-- ===== 06-semantics.md ===== -->


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



<!-- ===== 07-modules.md ===== -->


# 07 — 模块、包与可见性

Aevia 的模块系统是**文件级、无 functor、无隐式全局状态**的。一个文件是一个
模块；一个包是一组模块加上一个清单文件。名字解析在编译期完成，运行时没有
动态导入。

## 7.1 模块

### 7.1.1 文件即模块

```
src/
  kinematics.aev          -> 模块 Kinematics
  thermo/
    ideal_gas.aev         -> 模块 Thermo.IdealGas
    cycles.aev            -> 模块 Thermo.Cycles
```

- 模块名由相对 `src/` 的路径决定，`UpperCamelCase` 分段。
- 文件可以写 `module Thermo.IdealGas`，但 MUST 与路径一致；不一致报
  `E0601`。
- 一个文件只能有一个模块；不支持一个文件多个模块。
- 模块名区分大小写。

### 7.1.2 顶层声明顺序

模块内的声明可以相互引用，包括前向引用和递归函数。编译器先收集所有顶层
名字和签名，再检查函数体。因此：

```aevia
fn f(x: Int): Int = g(x) + 1
fn g(x: Int): Int = x * 2
```

合法，即使 `g` 在 `f` 之后。

单位声明是例外：单位字面量的解析需要单位表。实现 SHOULD 先收集所有
`unit`/`dimension` 声明，再解析函数体；因此单位也可以后置声明。跨模块的
单位通过 `import` 解析。

### 7.1.3 导出

默认所有顶层声明导出。`private` 隐藏：

```aevia
private let internal_scale = 1.0e-3
private fn helper(x: Real[L]): Real[L] = x * internal_scale

fn public_api(x: Real[L]): Real[L] = helper(x)
```

- `private` 可以加在 `let`、`fn`、`type`、`dimension`、`unit`、`class` 上。
- `private` 声明仍然在模块内可见；不导出到其他模块。
- 变体类型的构造器随类型一起导出；不能单独 `private` 某个构造器（当前规范）。
- 实例总是随模块导出（否则单独编译会缺实例）；`private instance` 不允许。

## 7.2 导入与打开

### 7.2.1 `import`

```aevia
open Units.SI
import Math as M
import Data.Csv { read_csv, Table }
```

- `import M` 把 `M` 绑定到当前作用域的限定名；使用 `M.f`。
- `import M as N` 起别名。
- `import M { f, g }` 是**选择性打开**：把指定名字直接引入当前作用域，
  等价于 `open M { f, g }`；要限定访问，用 `import M`。
- **单位是例外**：`import M` 会把 `M` 导出的单位加入当前模块的**单位
  命名空间**，因此 `import Units.SI` 之后 `20.0 m`、`3 s` 直接可用，不需要
  `open`。单位值（`m : Unit[L]`）仍在限定名下；要写 `to(m, x)`，用
  `open Units.SI` 或 `Units.SI.m`。
- 导入是幂等的；重复导入不报错。

### 7.2.2 `open`

```aevia
open Units.SI
open Math { sqrt, pi }
```

- `open M` 把 `M` 的所有导出名字引入当前作用域（不限定）。
- `open M { f, g }` 只引入指定名字。
- `open` 只影响当前模块的后续声明；不传递到导入者（没有 re-export）。
- 本地定义遮蔽打开的名字；同名冲突时，本地优先，否则报 `E0603`。
- 单位命名空间与值命名空间分开：`open Units.SI` 同时把单位名加入单位
  命名空间和值命名空间（供 `to(m, x)` 使用）。

### 7.2.3 限定名

```
Units.SI.km
Math.sqrt
Thermo.IdealGas.pressure
```

- 限定名从模块根开始；没有相对导入。
- 类型、构造器、值、单位都可以限定。
- 限定名不引入新绑定；只是查找路径。

### 7.2.4 名字解析顺序

对一个未限定名 `x`：

1. 当前函数的局部绑定（参数、`let`、模式）；
2. 当前模块的顶层声明；
3. 当前模块 `open` 的名字；
4. 预置（Prelude）的名字；
5. 否则 `E0301`。

同名冲突在步骤 3 内部报 `E0603`；步骤 1/2 遮蔽 3/4 是允许的。

## 7.3 模块依赖与循环

- 模块依赖图 MUST 是无环的。循环导入报 `E0602`。
- 类型/类/实例的依赖也参与排序；实例解析需要所有相关模块已检查。
- 单独编译：每个模块导出接口（类型、签名、实例、单位、构造器）。接口
  可以缓存为 `.aevi` 文件；缓存失效按源码哈希和依赖接口哈希。
- 没有隐式全局实例：实例只从导入的模块可见。为避免“实例丢失”，
  标准库的常用实例在 Prelude 中导入。

## 7.4 类型类实例与模块

- 实例在定义模块中唯一；导入类型/类所在模块即可见。
- 孤儿规则：实例必须定义在类型或类所属模块。跨模块的孤儿实例报
  `E0604`。
- 实例不因 `private` 隐藏；它们随模块导出。
- 派生实例（`derive`）在类型所在模块生成。

## 7.5 包

### 7.5.1 清单

`aevia.toml`：

```toml
[package]
name = "kinematics"
version = "0.1.0"
edition = "2026"
authors = ["..."]
description = "..."

[dependencies]
aevia-std = "0.1"
units-extra = { path = "../units-extra" }
plotting = { git = "https://example.com/plotting.git", tag = "v0.2.0" }

[compat]
aevia = ">=1.0, <2.0"
rssn_api = "1"

[capabilities]
requires = ["fs"]
provides = ["kinematics"]

[build]
target = "executable"       # 或 "library"
entry = "src/main.aev"      # 可执行包的入口；默认 src/main.aev

[profile.release]
opt = 3
```

- `name` 是包名，`snake_case`；导入时用包名作为根模块。
- `version` 是 SemVer。
- `edition` 是语言版本（`2026`）。
- 依赖可以是 registry、path、git；默认从中心 registry 解析（§13.7），
  path/git 用于本地开发和私有包。registry 是内容寻址 + 签名 + capability
  声明的，不是“以后再加”的附属品。
- `target` 为 `library` 时不需要 `main`；为 `executable` 时必须有
  `main : IO Unit`。
- `entry` 默认 `src/main.aev`。

### 7.5.2 锁文件

`aevia.lock` 记录每个依赖的精确版本、源 URL、内容哈希和签名。构建 MUST
使用锁文件；`aevia update` 更新。锁文件保证可复现构建；registry 包必须
带内容哈希，yank 不删除内容。

### 7.5.3 目录布局

```
kinematics/
  aevia.toml
  aevia.lock
  src/
    main.aev
    kinematics.aev
    thermo/
      ideal_gas.aev
  tests/
    fall_time.aev
  examples/
    projectile.aev
  docs/
```

- `src/` 下的 `.aev` 文件都是模块；`main.aev` 是入口（如果可执行）。
- `tests/` 下的文件是测试模块；`aevia test` 编译并运行。
- `examples/` 下的文件是示例；`aevia run --example name`。
- `docs/` 放 Markdown 文档；`aevia doc` 生成 API 文档。

### 7.5.4 测试

```aevia
# tests/fall_time.aev
import Kinematics

test "fall time from 20 m" =
    approx_eq(fall_time(20.0 m), 2.019 s, 0.001 s)

test "dimension rejection" =
    # 编译期错误不能用 test 表达；用 compile_fail 指令
    ...
```

- `test` 声明是 `Bool` 表达式；`aevia test` 运行所有测试。
- `approx_eq(a, b, tol)` 要求 `a`、`b` 同量纲，`tol` 同量纲。
- 编译期错误测试用 `#[compile_fail]` 注释指令（当前规范用 `compile_fail` 块，
  见 §8.13）。
- 属性测试用 `forall`（§8.13）。

### 7.5.5 文档

- `##` 文档注释附着到下一个声明，内容是 Markdown。
- `aevia doc` 生成 HTML；类型签名、单位、例子自动提取。
- 文档注释中的代码块标 `aevia` 会被编译检查（doctest）。

## 7.6 版本与兼容性

- 语言版本 `edition` 决定语法/语义；包可以声明最低版本。
- 库的 SemVer：破坏性变更升 major；新增 API 升 minor；修复升 patch。
- 类型/量纲签名的变更视为破坏性变更。
- 单位表的变更（尺度/偏移）视为破坏性变更；新增单位是 minor。
- 实例的增删是破坏性变更（影响解析）。

## 7.7 模块错误

| 代码 | 情况 |
|---|---|
| `E0601` | 模块名与路径不一致 |
| `E0602` | 循环导入 |
| `E0603` | 打开的名字冲突 |
| `E0604` | 孤儿实例 |
| `E0605` | 包清单错误 |
| `E0606` | 依赖版本冲突 |
| `E0607` | 找不到模块/包 |
| `E0608` | 入口缺失或签名错误 |

下一章给出标准库的签名。标准库是语言的一部分：单位、数学、线性代数、
统计、微积分、数据、绘图和仿真都在这里。



<!-- ===== 08-stdlib.md ===== -->


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



<!-- ===== 09-runtime.md ===== -->


# 09 — 运行时

运行时把源码变成值，把值变成 rssn 项，把项变成结果。它的设计目标是：
**启动快、交互快、数值热点快、符号操作可控**。这一章定义组件、数据流、
线程模型和性能边界。

## 9.1 组件

```
aevia-syntax   词法、布局、解析、AST、格式化、pretty-print
aevia-types    kind、类型、量纲代数、推断、类型类、穷尽性
aevia-core     核心 IR、解释器、值、内建函数、张量运行时
aevia-rssn     rssn 桥：Session、Term、Config、compute、JIT
aevia-std      标准库（Aevia 源码 + Rust 内建）
aevia-cli      CLI、REPL、包管理
aevia-lsp      Language Server
```

依赖方向：`syntax → types → core → rssn`；`std` 依赖 `core`；`cli`/`lsp`
依赖全部。`types` 不依赖 `rssn`（量纲代数是纯 Rust）；`core` 通过
`aevia-rssn` 的可选 feature 使用后端。

## 9.2 编译流水线

```
源码 .aev
  │ 词法 + 布局
  ▼
Token 流（含 INDENT/DEDENT）
  │ 语法分析
  ▼
CST（保留全部括号/注释/位置）
  │ 去糖
  ▼
AST（表面语法）
  │ 名字解析
  ▼
HIR（绑定名字、模块路径、单位解析）
  │ 类型/量纲检查、类解析、穷尽性
  ▼
Typed HIR（每个表达式带类型和量纲）
  │  lowering
  ▼
Core IR（ANF，显式 let、闭包、模式匹配）
  │ 解释 / 可选编译
  ▼
值
```

### 9.2.1 CST 与 AST

- CST 保留括号、注释、空白和所有源位置，用于格式化器和 LSP。
- AST 去掉括号（用优先级恢复），保留源位置。
- 去糖在 AST → HIR 之间完成：`|>`、`√`、`!`、`@`、`_` 占位符、`do`、
  记录更新、张量字面量、索引。
- 单位字面量在 HIR 阶段解析：名字 → 单位表 → 尺度/量纲/仿射标志。

### 9.2.2 Core IR

```
core ::= let x = core in core
       | lambda(params, core)
       | app(core, [core])
       | if(core, core, core)
       | match(core, [arm])
       | prim(op, [core])
       | construct(tag, [core])
       | project(core, field)
       | update(core, [(field, core)])
       | tensor_lit([core])
       | index(core, [index])
       | slice(core, [slice])
       | do([stmt])
       | return(core)
       | literal(value)
       | var(x)
```

Core IR 是 ANF：每个中间结果绑定到名字，便于闭包转换、JIT 和调试。
模式匹配在 Core IR 中保留，解释器按标签/字段匹配；编译器可以把简单模式
降成 switch。

### 9.2.3 类型擦除

量纲和类型在 Core IR 中擦除，但保留**运行时见证**：

- `Real`/`Float` 值带显示单位（可选）；
- `Point` 值带 `is_point` 标志；
- `Dyn` 值带运行时量纲；
- 张量带形状和存储类型。

类型擦除让 `mean<L>` 和 `mean<T>` 共享代码；运行时见证只用于显示、`Dyn`
和 FFI 检查。

## 9.3 解释器

### 9.3.1 环境

- 局部环境是持久映射（`im::HashMap` 或扁平 `Vec` + 名字索引）。
- 闭包捕获环境帧；帧是 `Rc`，不可变。
- 递归闭包用 `Rc<RefCell<Option<Closure>>>` 自引用，作用域退出时断开；
  周期由会话级 cycle collector 回收。
- 全局环境包含模块导出、Prelude、单位表、类实例字典。

### 9.3.2 调用

- 参数从左到右求值，压入新帧。
- 尾调用：解释器用 trampoline；`app` 在尾位置返回 `TailCall`，主循环
  替换当前帧，不增长 Rust 栈。
- 非尾调用递归受 `--max-stack` 限制；超过报 `E0805`。
- 内建函数是 Rust 函数指针 + arity；用户函数是闭包。

### 9.3.3 模式匹配

- 构造器：比较 tag，递归匹配 payload。
- 记录：按字段名查找，开放匹配。
- 张量：rank 1 长度/元素匹配；`..rest` 生成切片视图。
- 字面量：精确比较；`Float` 模式不允许。
- 守卫：匹配后求值；必须是具体 `Bool`。

### 9.3.4 错误与栈

每个 Core IR 节点带源位置。错误对象包含：

```
EvalError {
    code: E08xx,
    message: String,
    span: Span,
    stack: Vec<Frame>,   # 函数名 + 调用点
    notes: Vec<String>,
}
```

`panic` 打印栈；`Result` 错误不打印，由 `?` 传播。

## 9.4 符号桥

### 9.4.1 Session 与 Term

rssn 的 `Session` 拥有一个项图，`Term<'s>` 是带生命周期的句柄，`Session`
内部是 `RefCell`，因此不是 `Sync`。Aevia 的策略：

- **每个线程一个 `Session`**，放在 `thread_local!` 或执行器的工作线程池中。
- 跨调用的项用 `u32` id 保存；需要时 `Session::term_by_id(id)` 取回。
- `TermId` 包含 `session_id`，防止跨会话误用。
- `compute` 会克隆模板图；对重型请求可接受，对每个 `+` 不可接受。因此
  算术只构造项（`Session::call`），不调用 `compute`。

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

- `symbol("x")` 生成内部唯一名 `x#n`，在 rssn 中 `Session::sym(name)`。
- 静态路径的符号无量纲；量纲由类型检查器负责。只有 FFI/导出才用全名
  传给 `Config::dimension`；`Dyn<ρ>` 的运行时量纲由宿主保存。
- `Config` 按（规则集、假设、维度模式、预算）缓存；避免每次 `compute`
  重建。
- `term_cache` 用结构哈希去重，减少重复项。

### 9.4.3 请求构造

| Aevia | rssn 请求 |
|---|---|
| `diff(f)` | `diff(term, ξ)` |
| `antiderivative(f)` | `integral(term, ξ)` |
| `defint(f,a,b)` | `defint(term, ξ, a, b)` |
| `limit(f,a)` | `limit(term, ξ, a)` |
| `solve(f)` | `solve(term, ξ)` |
| `sum(f,a,b)` | `sum(term, κ, a, b)` |
| `product` | `product(term, κ, a, b)` |
| `series` | `series(term, ξ, a, n)` |
| `dsolve` | `dsolve(eq, y(t))` |
| `simplify` | `compute(term, Config::symbolic())` |
| `approx` | `compute(term, Config::numeric(tol).bind(...))` |

构造请求时，`term` 是已构造的项；`ξ`/`κ` 是新鲜符号；`a`/`b` 是已求值的
项或数值。`compute` 返回 `Answer`；`Answer.term` 是答案项，`Answer.dimension`
是 rssn 推断的量纲。

### 9.4.4 量纲校验

每次 `compute` 后：

1. 静态路径：项图无量纲，答案量纲由类型检查器已知；不做运行时比较。
2. 数值路径：以 `Answer.dimension` 为准（**不要**从 `Answer.term` 重新
   推断；Numeric 目标的 term 是无量纲浮点字面量）。`Answer.dimension` 为
   `None`（`DimensionMode::Off`）时回退到静态量纲；需要运行时量纲就用
   `Advisory`/`Strict`。
3. 如果开启可选诊断且 `Answer.dimension` 与静态期望不一致，报 `E0809`
   （内部错误），并打印请求、答案、两边量纲。
4. 把 `Answer.term` 转回 Aevia 值。

对 `Dyn<ρ>`，不比较静态量纲；把宿主保存的 `rssn_dim::DimTerm` 作为运行时量纲。

### 9.4.5 数值路径

- `Float` 标量的四则运算直接走 f64，不进入 rssn。
- `Float` 的 `sin`/`exp` 等走 Rust `libm` 或 rssn 内核；选择标准是速度和
  一致性。
- `Real` 的数值化（`approx`）走 rssn 的 `Target::Numeric`；结果带误差球，
  Aevia 取中点，误差可用于诊断。
- 张量的 `F64` 运算走 ndarray/faer；`Exact`/`Boxed` 走逐元素标量路径。

## 9.5 张量运行时

- 存储：`Arc<TensorData>`；切片是视图（offset + strides），`reshape` 在
  连续时是视图，否则复制。
- 写入不存在；`update`/`map` 产生新张量；唯一持有时原地复用。
- 元素级 `F64` 循环用 4/8 路展开；`sum` 用 pairwise/Kahan。
- `matmul`/`solve`/`eig`/`svd`/`qr`/`lu` 调 rssn 的数值内核（faer）。
- `fft` 调 rustfft；`interp`/`spline` 调 rssn 内核。
- 形状检查在每次运算前做；错误包含两边形状和运算名。细化形状在编译期
  检查，秩-only 在运行时检查。
- 空张量、零维、单元素张量的边界行为在 §6.6 定义。
- 设备：`Tensor` 值带 `device` 元数据；`to_device`/`on(device, ...)` 负责
  传输与作用域执行。CPU 是默认；GPU/分布式是运行时目标，不是类型参数。
- 稀疏：`Sparse<F,shape,τ>` 与 dense 张量共享 `@` 分派；`Layout` 类提供
  `nnz`/`to_dense`/`from_dense`/`matmul`/`transpose`。
- 插件：`Rssn.register_plugin` 把算子/重写/单位注册进当前 RSSN 会话；
  Aevia 通过 manifest 反射看到它们，不需要重新编译。

## 9.6 JIT

### 9.6.1 触发条件

- 函数只含 `Float` 标量、算术、初等函数，**不含** `if`/`match`/张量索引；
- 被调用超过阈值（默认 1000 次）或显式 `@jit`；
- 输入/输出类型固定（无符号、无 `Dyn`、无记录/变体）。

### 9.6.2 流程

1. 把 Core IR 降成 rssn 项（`Float` 标量表达式）。
2. `Term::compile(&["x", "y"])` 得到 `Compiled`。
3. 缓存键：项结构哈希 + 输入名 + 后端版本。
4. 调用时直接走机器码；失败回退解释器。

rssn 的 `TieredBackend` 可以先解释、后台编译；Aevia 复用这个策略。

### 9.6.3 限制

- 符号值、记录、变体、闭包不进入 JIT。
- `Real` 的精确算术不进入 JIT；需要时先 `to_float`。
- JIT 的结果与解释器 MUST 数值一致（同一 IEEE 语义）；不一致是 bug。

## 9.7 内存与并发

### 9.7.1 内存

- 张量：`Arc` + COW；切片共享；唯一持有时原地。
- 闭包：`Rc`；递归闭包用弱引用/断链；周期收集器。
- 符号项：rssn 的 hash-consed 图；项 id 不跨会话。
- 会话级 arena 在 REPL 中增长；`:reset` 释放。

### 9.7.2 并发

- 纯函数可以并行；`par_map(f, xs)` 用 rayon，结果顺序确定。
- 每个工作线程一个 `Session`；`compute_all` 在单会话内并行，但 Aevia 的
  并行 map 需要跨会话时，按线程分配会话。
- `IO` 不并行；`do` 顺序执行。
- 没有共享可变状态；`Session` 的 `RefCell` 不跨线程。

## 9.8 REPL

```
$ aevia repl
Aevia — dimensions are types
> let g = 9.80665 m/s^2
g : Float[L/T^2] = 9.80665 m/s^2
> let h = 20.0 m
h : Float[L] = 20 m
> sqrt(2 * h / g)
Float[T] = 2.019 s
> :type sqrt
sqrt : Scalar<ρ, d> -> Scalar<ρ, d^(1/2)>
> :dims
L M T I Θ N J
> :units L
m, km, cm, mm, µm, nm, ft, in, mi, ...
> :latex sqrt(2*h/g)
\sqrt{\frac{2h}{g}}
> :time simplify(integral(x^2*exp(x), x))
...
```

命令：

| 命令 | 作用 |
|---|---|
| `:type e` | 显示类型与量纲 |
| `:dims` | 列出基量纲 |
| `:units [d]` | 列出单位 |
| `:ast e` | 显示 AST |
| `:core e` | 显示 Core IR |
| `:time e` | 计时 |
| `:latex e` | LaTeX |
| `:load f` | 加载文件 |
| `:reset` | 清空会话 |
| `:quit` | 退出 |

REPL 支持多行输入（缩进块）、Ctrl-C 取消当前求值、Ctrl-D 退出。声明
按输入顺序累积；重复定义遮蔽旧定义。

## 9.9 CLI

```
aevia run [file] [-- args...]      # 运行可执行包/文件
aevia repl                         # 交互式
aevia check [--all]                # 类型/量纲检查
aevia fmt [--check]                # 格式化
aevia test [--filter pat]          # 运行测试
aevia doc [--open]                 # 生成文档
aevia bench [--filter pat]         # 基准
aevia init name                    # 新建包
aevia add dep                      # 添加依赖
aevia update                       # 更新锁文件
aevia clean                        # 清理构建产物
```

常用 flag：

```
--release          优化模式
--jobs N           并行度
--max-stack BYTES  栈上限
--budget SECONDS   符号预算
--out PATH         输出路径
--color auto|always|never
```

## 9.10 LSP

- 诊断：类型错误、量纲错误、单位错误、未使用绑定、非穷尽匹配。
- 悬停：类型、量纲、单位、文档。
- 补全：模块名、函数、单位、字段、构造器。
- 跳转：定义、类型定义、单位声明。
- 格式化：`aevia fmt` 的同一引擎。
- 代码动作：添加类型标注、把 `degC` 转成 `K`、提取 `let`、修正量纲。
- 语义高亮：类型、量纲、单位、算子来源；没有宏，语法高亮是固定的。

LSP 复用 `aevia-syntax`/`aevia-types`，不启动运行时；`check` 模式即可。

## 9.11 构建与包管理

- 包图拓扑排序；并行编译无依赖的模块。
- 接口文件 `.aevi` 缓存类型/量纲/实例/单位；哈希失效。
- 增量：源码哈希 + 依赖接口哈希 + 编译器版本。
- 锁文件保证可复现；`--locked` 禁止更新。
- 构建产物在 `.aevia/target/`；不提交。
- 中心 registry 是默认源；协议见 §13.7。registry 索引是内容寻址 + 签名 +
  capability 声明；`aevia add`/`publish`/`yank` 都走同一协议。

## 9.12 测试与基准

- 单元测试：`test` 声明。
- 属性测试：`forall` + `Arbitrary`。
- Doctest：文档中的 `aevia` 代码块。
- 编译失败测试：`compile_fail`。
- 黄金测试：REPL 输出/错误信息快照。
- 基准：`bench` 声明；比较中位数和置信区间；回归阈值 5%。

## 9.13 性能目标

| 场景 | 目标 |
|---|---|
| 启动（空程序） | < 50 ms |
| REPL 输入到结果（简单表达式） | < 100 ms |
| `Float` 张量逐元素运算 | 与 NumPy 同量级（≤ 2×） |
| `Float` 矩阵乘法 | 与 faer 同量级（≤ 1.2×） |
| 小符号表达式化简 | < 10 ms |
| 重型积分/求解 | 受预算控制，可中断 |
| JIT 热函数 | ≥ 5× 解释器 |

这些是设计目标，不是规范保证；实现应在基准中跟踪。

## 9.14 安全与沙箱

- 没有 `unsafe` 用户代码；Rust 内建经过审计。
- 文件/网络 IO 只通过 `IO` 动作；库可以声明所需 capability（`fs`、
  `net`、`env`、`rand`、`gpu`），由运行时权限检查，不是类型参数（§13.9.6）。
- 符号预算防止无限化简；栈上限防止无限递归；内存上限可配置。
- 包构建不执行任意脚本；`build.aev` 不存在。需要生成代码时用插件/算子，
  不用构建脚本或宏。

## 9.15 错误报告

所有诊断共享：

```
error[E0401]: dimension mismatch
  --> src/kinematics.aev:7:13
   |
 7 |     sqrt(2 * h * g)
   |              ^^^^^ expected Real[T^2], found Real[L^2/T^2]
   |
   = left:  L^2/T^2
   = right: T^2
   = difference: L^2/T^4
help: did you mean `2 * h / g`?
```

- 代码、消息、主位置、标签、注释、帮助。
- 量纲错误附带规范形和差异。
- 类型错误附带期望/实际类型，尽量给出“最可能的修正”。
- 解析错误附带期望的记号和最近的合法修复。

下一章定义 Aevia 与 rssn 的具体映射：哪些能力直接用，哪些要包一层，
哪些是已知缺口。



<!-- ===== 10-backend.md ===== -->


# 10 — rssn 后端映射

Aevia 不重新实现计算机代数。rssn 提供项表示、重写引擎、量纲层、数值内核、
JIT 和仿真；Aevia 提供语法、类型/量纲检查、闭包、记录、张量形状、模块、
IO 和标准库编排。这一章定义两者的契约：哪些能力直接映射，哪些要包一层，
哪些是已知缺口。

## 10.0 反射优先：没有手写算子绑定

Aevia **不维护** `sin -> rssn::sin`、`det -> rssn::det` 这样的绑定表。启动/
构建时读取 RSSN 的 **Kernel Manifest**，生成 `Rssn` 模块：

```aevia
Rssn.sin(x)                  # 有签名：类型化
Rssn.Calculus.diff(f, x)     # 规则集限定
Rssn.dynamic("special/meijer_g", args)   # 无签名/实验性/插件
```

- 有 `Signature` 的算子 → 类型化声明；
- 无签名 → `dynamic` 声明，运行时检查类型/量纲；
- 新算子只要在 RSSN 注册，下一次构建就出现，不需要改 Aevia 编译器；
- 重载按 `OpKey { rule_set, name, signature_hash }` 解析，不依赖注册顺序。

完整接口清单见根目录 [`RSSN-INTEGRATION-REQUIREMENTS.md`](../RSSN-INTEGRATION-REQUIREMENTS.md)，
Aevia 侧规则见 [13 — Extensibility](13-extensibility.md)。

## 10.1 rssn 的能力边界

rssn 的核心入口是：

```rust
let s = Session::new();
let x = s.sym("x");
let answer = s.compute(diff(sin(x) * cos(x), x), &Config::new())?;
```

- `Session` 拥有一个 hash-consed 项图；`Term<'s>` 是带生命周期的句柄。
- `Config` 指定规则集、目标相位（`Symbolic`/`Numeric`）、绑定、假设、预算、
  量纲模式、量纲假设、自定义基量纲。
- `compute` 把请求复制到模板图的克隆中，运行 e-graph 重写，抽取答案，
  再把答案导入回会话。
- `Answer` 包含 `term`、`value`、`error`、`reduced`、`dimension`、
  `diagnostics`、`report`。
- `Term::eval(bindings)` 不做重写，直接数值求值。
- `Term::compile(inputs)` 把闭式编译成可调用函数；`jit` feature 用
  Cranelift。
- `Term::as_tensor()` 把嵌套 `list` 项转成 `(shape, Vec<f64>)`。
- `Term::to_latex()`/`to_typst()`/`to_pretty()` 输出排版。
- `sim::scenario::run(name, json)` 运行物理仿真。

Aevia 直接复用的部分：

| 能力 | rssn 模块 |
|---|---|
| 精确数（大整数/有理数/浮点） | `graph::Number` |
| 项构造与 hash-consing | `graph::Graph`、`Session::call` |
| 重写引擎与规则集 | `graph::Engine`、`rules::*` |
| 微积分/代数/方程/ODE/PDE | `rules::calculus`、`solve`、`ode`、`pde` 等 |
| 量纲推断 | `graph::dim` |
| 单位解析 | `rules::units` |
| 数值内核 | `kernels::*` |
| 线性代数 | `kernels`/`faer`/`rules::linalg` |
| FFT | `kernels::fft` |
| ODE 积分 | `kernels::ode`、`sim::integrators` |
| 仿真 | `sim::scenario` |
| JIT | `backend::jit`、`Term::compile` |
| 排版 | `io::latex`、`io::typst`、`io::pretty` |

Aevia 自己实现的部分：

- 词法/语法/布局/格式化；
- 类型与量纲的**编译期**检查；
- 闭包、记录、变体、字符串、集合、`IO`；
- 张量的宿主表示、形状检查、广播、切片；
- 模块、包、REPL、LSP；
- 标准库的编排与类型化包装；
- `Dyn` 的运行时量纲检查。

## 10.2 标量映射

### 10.2.1 精确数

| Aevia | rssn |
|---|---|
| `Int`（任意精度） | `Number::Int(BigInt)` |
| `Real` 精确有理数 | `Number::Rat(BigRational)` |
| `Float` | `f64` / `Number::Float` |
| `Complex` 数值 | `num_complex::Complex64`（宿主）或 `a + b*i` 项 |
| `Complex` 符号 | `a + b*i` 项 |

- `Session::int(i64)` 只接受 `i64`；超出范围时用
  `Session::parse(&n.to_string())` 构造大整数。
- `Session::rational(i64, i64)` 同理；大有理数用文本解析。
- `Real` 的 `Exact(Number)` 直接映射；`Sym(TermId)` 映射到 rssn 项。
- `Float` 的 `F64` 不进入 rssn；`Sym` 项含浮点字面量时由 rssn 处理。
- `Complex` 的数值路径用 `Complex64`；符号路径构造 `add(re, mul(im, i))`。

### 10.2.2 符号

```rust
let name = format!("{base}#{id}");     // 内部唯一名
let term = session.sym(&name);
```

- 显示名与内部名分开；`symbol("x")` 的内部名是 `x#1`，显示 `x`。
- 静态路径的项图无量纲，**不**给符号传量纲；量纲由 Aevia 类型检查器
  负责。只有 FFI/导出时才用全名（`Length/Time`）传给 `Config::dimension`。
- 同一符号在多次 `compute` 中复用同一个 `TermId`；`Session` 的项图负责
  hash-consing。
- 符号的假设（`positive`、`real`、`integer`）通过 `Config::assume` 传入；
  Aevia 的 `assuming` 把简单 `Prop`（`x > 0`、`x >= 0`、`x != 0`、
  `x ∈ ℤ`）转成 `Facts`。一般等式/不等式通过 `Rssn.assume` 动态接口传给
  RSSN；后端不支持时返回 `E1001`（`core.assume.general`），而不是静默丢弃。

## 10.3 算子映射：生成的 façade，不是手写绑定

下面表格是**语义映射的示例**，不是 Aevia 仓库里的绑定文件。实际实现由
manifest 生成；新算子不需要改这些表。表格用来解释 Aevia 的语法糖如何
落到 RSSN 的结构算子/请求上。

### 10.3.1 核心算术

| Aevia | rssn 算子 |
|---|---|
| `a + b` | `add(a, b)` |
| `a - b` | `add(a, mul(-1, b))`（rssn 的减法糖） |
| `a * b` | `mul(a, b)` |
| `a / b` | `mul(a, pow(b, -1))` |
| `a ^ n` | `pow(a, n)` |
| `-a` | `mul(-1, a)` |
| `abs(a)` | `abs(a)` |
| `sqrt(a)` | `sqrt(a)` |
| `sin(a)` | `sin(a)` |
| `exp(a)` | `exp(a)` |
| `ln(a)` | `ln(a)` |
| `atan2(y,x)` | `atan2(y, x)` |

`Session::call(op, args)` 构造项；不触发重写。Aevia 的 `+` 在 `Real`/`Float`
符号路径上就是一次 `Session::call`，O(1)。

### 10.3.2 重型请求

| Aevia | rssn 请求 |
|---|---|
| `diff(f)` | `diff(term, ξ)` |
| `antiderivative(f)` | `integral(term, ξ)` |
| `defint(f,a,b)` | `defint(term, ξ, a, b)` |
| `limit(f,a)` | `limit(term, ξ, a)` |
| `series(f,a,n)` | `series(term, ξ, a, n)` |
| `sum(f,a,b)` | `sum(term, κ, a, b)` |
| `product(f,a,b)` | `product(term, κ, a, b)` |
| `solve(f)` | `solve(term, ξ)` |
| `solve_complex(f)` | `solve_complex(term, ξ)` |
| `nsolve(f,x0)` | `nsolve(term, ξ, x0)` |
| `dsolve(eq,y,t)` | `dsolve(eq, y(t))` |
| `simplify(x)` | `compute(term, Config::symbolic())` |
| `approx(x)` | `compute(term, Config::numeric(tol))` |

这些请求用 `Target::Symbolic` 或 `Target::Numeric`；结果从 `Answer` 取。

### 10.3.3 数值内核

| Aevia | rssn |
|---|---|
| `defint`（数值） | `kernels::quadrature` |
| `odeint` | `kernels::ode` / `sim::integrators` |
| `fft` | `kernels::fft`（rustfft） |
| `eig`/`svd`/`qr`/`lu` | `kernels`/`faer` |
| `interp1`/`spline` | `kernels` |
| `minimize` | `kernels::optimize` |
| 特殊函数 | `kernels::special` |

数值内核直接吃 `f64`/`ndarray`，不经过项图。Aevia 的 `Float` 张量路径优先
走这里。

### 10.3.4 线性代数

- `Float`/`Complex` 矩阵：用 rssn 的数值内核（faer）。
- `Real` 精确/符号矩阵：小矩阵用 `rules::linalg`；大矩阵若无法数值化，
  返回 `Err` 或保持符号。
- `det` 的无量纲限制在 Aevia 层；rssn 的 `det` 仍可算带量纲矩阵，但
  Aevia 不暴露那个重载。
- `linsolve(A,b)` 的量纲 `e/d` 由 Aevia 静态检查；rssn 的答案量纲用于校验。

### 10.3.5 未知/新算子

任何不在生成 façade 里的算子走动态调用：

```aevia
let r = Rssn.dynamic("myplugin/foo", [x, y])
match r with
| Ok(v) -> ...
| Err(e) -> ...
```

- `Rssn.dynamic` 的参数/返回值是 `Value`，运行时检查类型/量纲；
- 新算子注册到 RSSN 后，重新生成 `Rssn` 模块即可获得类型化声明；
- 插件/用户算子不需要 Aevia 编译器改动；
- 动态调用失败返回 `OpError`，Aevia 转成 `E1003`/`E1001`。

### 10.3.6 单位

**Aevia 的项图不带单位。** 单位字面量在编译期折叠为相干 SI 数值；`Real`/
`Float` 值在宿主侧带一个可选的显示单位。rssn 看到的只是无量纲数值和符号
表达式，例如 `3 m + x` 在项图里是 `3 + x`（`x` 是无量纲符号），量纲由
Aevia 的静态类型保证。

理由：当前 rssn 的 `quantity` 没有数值 eval，且 `dimension_from_text` 会把
`L` 解析成升、`T` 解析成特斯拉；把单位放进静态项图会同时破坏数值路径和
量纲路径。RSSN 仍应提供 `Value::Quantity` 的 eval 供 FFI/显示使用，但
Aevia 的静态路径不依赖它。单位只在两个边界出现：

1. **显示/排版**：`show`/`to_latex` 在宿主侧把数值和显示单位拼成文本；
   需要 rssn 排版时，用 `quantity(v, "<相干 SI 单位>")` 包一层，仅用于输出。
2. **导出/FFI**：写 CSV/NPY/JSON 时在宿主侧附单位元数据，不进入项图。

`Point` 在进入 rssn 前转成 K 的 `Float`；`Dyn<ρ>` 的运行时量纲是
`rssn_dim::DimTerm`。启动时做单位表一致性检查（§5.10），但只用于显示/
导出；`Units.SI` 从 `UnitRegistry` 生成，不维护第二张表。

### 10.3.7 仿真

```rust
let json = rssn::sim::scenario::run("ising", params)?;
```

Aevia 的 `simulate(name, json)` 直接调用；`Physics.*` 包装器由 RSSN 的
场景 manifest 生成（名称、参数 schema、返回 schema），新增场景不需要手写
Aevia 包装。`ising` 返回扁平的 `Vec<i8>`，包装器按 `width`/`height` 行主序
reshape 成 `Tensor<2, Int>`；`sim` 的错误是字符串，包装器按前缀映射成
`SimError` 枚举。

## 10.4 量纲桥

量纲/单位的唯一事实来源是 RSSN 的 `rssn-dim`（`DimTerm`/`DimArena`/
`UnitRegistry`）。Aevia 的 `Dim` 是它的类型级镜像；`DimValue` 在运行时就是
`rssn_dim::DimTerm`，不是字符串。桥接细节见根目录文档 §3.5。

### 10.4.1 静态到运行时

**静态路径不把量纲传给 rssn。** Aevia 的类型检查器已经保证量纲正确；
项图里的符号是无量纲的，rssn 的 `DimensionMode` 设为 `Off`（或
`Advisory` 仅用于诊断）。这避免了文本歧义和 `quantity` 无 eval 的问题。

只有以下场景需要把量纲交给 rssn：

- `Dyn<ρ>` 的运行时检查：宿主保存 `rssn_dim::DimTerm`，不依赖字符串；
- FFI/导出：在边界用 `Config::dimension_term(symbol, DimTerm)`，或全名
  （`Length`、`Mass`、`Time`、`Current`、`Temperature`、`Amount`、
  `Luminosity`）传给 `Config::dimension`，**绝不使用单字母**（`L` 是升，
  `T` 是特斯拉，`N` 是牛顿，`J` 是焦耳）；
- 用户显式请求 rssn 的量纲诊断：`Config::dimensions(Advisory)`。

基量纲在模板构建期注册，不在每次请求的 `prepare` 里注册；否则
`DimArena` 的共享 `Arc` 会被并发请求污染。

### 10.4.2 运行时到静态

数值答案的量纲以 `Answer.dimension` 为准；**不要**从 `Answer.term` 重新
推断（Numeric 目标的 term 是无量纲浮点字面量）。`Answer.dimension` 是
`Option`：`DimensionMode::Off` 时为 `None`，此时回退到静态量纲；要得到
运行时量纲，数值路径必须用 `Advisory` 或 `Strict`。对静态路径，答案量纲
已知，`Answer.dimension` 只用于可选的一致性检查；不一致是内部错误
`E0809`，不是用户错误。

### 10.4.3 量纲模式

| 场景 | 模式 |
|---|---|
| 普通静态检查通过的代码 | `Off`（项图无量纲） |
| `Dyn`、FFI、运行时构造的项 | `Strict`，用全名 |
| 用户显式请求诊断 | `Advisory` |

Aevia 不暴露 `bypass` 给普通用户；`Dyn<ρ>` 是显式逃逸。

## 10.5 线程与生命周期

### 10.5.1 Session 不是 `Sync`

`Session` 内部是 `RefCell`，`Term<'s>` 借用 `Session`。Aevia 的桥：

```rust
thread_local! {
    static BRIDGE: RefCell<RssnBridge> = RefCell::new(RssnBridge::new());
}

struct RssnBridge {
    session: Session,
    symbols: HashMap<SymbolKey, u32>,   // TermId
    configs: HashMap<ConfigKey, Config>,
}
```

- 每个线程一个 bridge；`par_map` 的工作线程各自初始化。
- 符号值跨线程传递时，`TermId` 不能直接复用；需要重新构造（用项文本或
  结构化复制）。当前 RSSN `Session` 单线程；Aevia 用 `fork/merge` 支持符号并行，数值张量直接并行。
- `compute_all` 在单会话内并行；Aevia 用它批量处理独立的重型请求。

### 10.5.2 项的生命周期

- `Term<'s>` 只在 `compute` 调用期间借用；返回后转成 `u32` id。
- `Session::term_by_id(id)` 取回；id 在会话内稳定。
- `compute` 的答案项已经导入回会话，id 有效。
- `Session` 的项图只增不减；REPL `:reset` 重建会话。

## 10.6 性能策略

### 10.6.1 不要为每个算子调用 `compute`

`compute` 会克隆模板图并运行重写；对 `x + 1` 这种 O(1) 构造，代价高几个
数量级。Aevia 的规则：

- 算术/初等函数：`Session::call` 构造项，不 `compute`。
- `simplify`/`eval`/`diff`/`integrate`/`solve`：`compute`。
- 打印：默认不化简；用户显式 `simplify` 或用 `:latex`（可配置轻量规范化）。
- 比较/`if`：`Prop` 不求值；需要判定时 `decide` 或 `assuming`。

### 10.6.2 缓存

- `Config` 按规则集/假设/模式缓存；避免每次重建。
- 项按结构哈希缓存；重复子表达式复用。
- 编译函数按项哈希 + 输入名缓存。
- 单位表/量纲别名编译期常量。

### 10.6.3 批处理

- 多个独立重型请求用 `compute_all`；它在一个会话内并行准备/运行/导入。
- 张量逐元素符号运算不要用 `compute_all`（每个元素一个请求，开销大）；
  用 `map` 构造项，最后一次性 `simplify`。
- 数值张量用 `F64` 路径，不进项图。

### 10.6.4 JIT

- `Term::compile` 把闭式 `Float` 表达式编译成函数；Aevia 的热点函数走
  这条路径。
- `TieredBackend` 先解释、后台编译；适合 REPL 和交互式。
- JIT 失败回退解释器；不改变语义。

## 10.7 错误映射

| rssn 错误 | Aevia 诊断 |
|---|---|
| `ComputeError::Dimension(e)` | `E0401` 或 `E0412`（`Dyn`） |
| `ComputeError::NotNumeric(s)` | `E0803`（自由符号）或 `E0806`（定义域） |
| `ComputeError::Inconsistent(s)` | `E0809`（后端不一致，内部错误） |
| `ComputeError::Rule(e)` | `E0899`（内部错误） |
| `ComputeError::UnknownOperator(s)` | `E0899` |
| 预算耗尽 | `E0808` |
| `BackendError` | `E0807` 或 `E0899` |

`Answer.diagnostics` 转成 Aevia 警告；`Advisory` 模式下不失败。

## 10.8 已知缺口与风险

1. **`Session` 单线程。** 每个线程一个会话；符号值不能跨线程。并行符号
   计算受限。
2. **`Term` 生命周期。** 需要 `u32` id + `term_by_id`；没有序列化。
3. **`compute` 克隆图。** 小算子开销大；必须区分“构造”和“求解”。
4. **复数表示。** rssn 的 `Number` 没有复数；复数靠 `i` 项或宿主
   `Complex64`。符号复数与数值复数的路径要统一。
5. **量纲 API 需要结构化/序列化接口。** `rssn-dim` 必须公开
   `DimTerm`/`DimArena`/`UnitRegistry` 的稳定访问器；Aevia 不解析
   `Answer.dimension` 文本。
6. **符号指数。** rssn 支持 `L^n`，Aevia 静态层不支持；`Dyn` 桥接。
7. **单位表统一。** `Units.SI` 从 `UnitRegistry` 生成；Aevia 不再维护更大
   的第二张表。缺失单位在 RSSN 注册，用户单位也注册到同一个 registry。
8. **仿射单位。** rssn 把 `degC` 转 K；Aevia 的 `Point` 是静态的，桥接时
   消解。
9. **`det`/`product` 的带量纲形式。** 静态不可表达；`Dyn` 逃逸。
10. **仿真参数是 JSON。** 场景 manifest 生成包装器；JSON schema 需要
    代码生成/校验，不能只靠手写。
11. **JIT 只支持 `Float` 标量。** 记录/变体/符号不进 JIT。
12. **`Term::as_tensor` 只支持数值。** 符号张量用嵌套 `list` 项，运行时
    自己遍历。
13. **预算与中断。** rssn 的预算在 `compute` 内；Aevia 需要在外层加超时/
    取消（协作式）。
14. **版本漂移。** rssn 正在架构迁移；Aevia 必须 pin 版本，并用一致性测试
    锁定行为。
15. **`compute` 的克隆是深拷贝。** `prepare` 克隆整个 `OpTable`（`Vec` +
    `HashMap`），单次 `simplify` 的成本与算子表大小相关，而不是请求大小。
    缓解：`OpTable` 用 `Arc` 共享，新增 `Session::prepare(config)` 复用
    克隆；符号张量分块 compute。
16. **`DimArena` 在克隆间共享。** `Graph::clone` 只克隆 `Arc`；在请求期
    `define_base_dimension` 会污染模板，并发请求的顺序会影响基维 id。
    缓解：基量纲只在模板构建期注册；请求期只读。
17. **`Term` 没有遍历/替换/自由符号 API。** `diff` 返回的闭包不能高效
    替换；`Real + Float` 的闭包性检查也拿不到自由符号。缓解：给 `Term`
    加 `children`/`substitute`/`free_symbols`，或让桥接层直接使用
    `graph::Graph`；否则 `diff` 每次调用都要重跑 `compute`。
18. **JIT 不支持分支和索引。** tape 的 `Select` 没有 lowering，也没有
    load/index 指令。缓解：当前 JIT 只支持无分支的 `Float` 表达式；`if`
    和张量索引由解释器执行，直到 RSSN 提供 lowering。
19. **`Solutions` 没有后端结构。** rssn 的 `solve` 返回根列表，重数和
    条件在求解器内部。缓解：`Solutions` 只保证 `roots`；`multiplicities`
    全填 1，`conditions` 为空，文档明确标注；RSSN 结构化结果就绪后补齐。
20. **ODE 方法/事件不齐。** `radau5` 没有 events；`rk4` 需要固定步数；
    `dopri5` 支持 events。缓解：`OdeOpts` 增加 `steps`（仅 Rk4）；
    events 只允许 Dopri5，其他方法报 `E0807`。
21. **`par_map` 对符号值不安全。** `TermId` 只在会话内有效；跨线程会
    静默用错项。缓解：`par_map` 要求 `Parallel a`；符号 `Real` 用
    `fork/merge`（RSSN 提供后），当前串行。
22. **`assuming` 只支持单元谓词。** rssn 的 `Facts` 没有等式/一般不等式。
    缓解：`assuming` 只支持 `x > 0`、`x >= 0`、`x != 0`、`x ∈ ℤ`；
    一般谓词走 `Rssn.assume` 动态接口，后端缺失报 `E1001`。
23. **没有 Kernel Manifest/签名/重载。** RSSN 的 `OpDescriptor` 只有
    arity；Aevia 无法生成类型化 façade。缓解：按根目录文档 §3.1/§3.3
    增加 manifest、`Signature`、`overloads`、`Stability`。
24. **没有动态 `Value`/`call` ABI。** Aevia 无法调用无签名算子。缓解：
    按 §3.2 增加 `Value`、`Kernel::call`、`OpError`。
25. **单位表私有且尺度是 `f64`。** Aevia 无法枚举/注册单位，精确尺度
    丢失。缓解：按 §3.5 抽出 `rssn-dim`，公开 `UnitRegistry`/`Scale`。
26. **`DimTerm` 不透明，`Dimension` 指数是字符串。** 无法可靠比较/
    序列化。缓解：公开结构化访问器，指数用 `BigRational`。
27. **`EvalFn` 是 fn pointer，不能捕获 Aevia 闭包。** 用户算子无法注册。
    缓解：增加 `EvalDyn`/`Plugin`/`HostCallback`（§3.4）。

### 10.8.1 缓解措施

- **一致性测试**：对每个标准算子，比较 Aevia 静态量纲与 rssn
  `Answer.dimension`；不一致即失败。
- **单位表校验**：启动时逐项比较基单位/派生单位/仿射单位。
- **回退路径**：JIT 失败回退解释器；符号求解失败返回 `Err`，不 panic。
- **预算默认值**：2 s / 1e6 重写；用户可调。
- **版本 pin**：`Cargo.toml` 固定 rssn 次版本；升级时跑全量 conformance。
- **桥接层隔离**：所有 rssn 调用集中在 `aevia-rssn`，上层不直接依赖
  `rssn` 类型。

## 10.9 一个端到端的调用链

```aevia
fn fall_time(h: Float[L]): Float[T] =
    sqrt(2 * h / g)
```

1. 类型检查：`h : Float[L]`，`g : Float[L/T^2]`，`2*h/g : Float[T^2]`，
   `sqrt : Float[T]`。
2. 调用 `fall_time(20.0 m)`：`20.0 m` 编译期折叠为 `Float` 值 20.0，带显示单位
   `m`。
3. 函数体：`2 * h` 构造 `mul(2, h)` 项；`/ g` 构造 `mul(..., pow(g,-1))`；
   `sqrt` 构造 `sqrt(...)` 项。这些都是 `Session::call`，不 `compute`。
4. 返回 `Float` 值（`g` 是 `Float` 常量，`2*h/g` 折叠为浮点数；
   `sqrt` 对 `Float` 永远走 f64，负数得 NaN，不会保持符号）。
5. `println` 调用 `show`：直接得到 `2.019`，按显示单位 `s` 打印。
6. 如果用户写 `approx(fall_time(20.0 m))`：`compute` 以 `Numeric` 目标运行，
   rssn 返回数值球，Aevia 取中点。

这个链条展示了 Aevia 的核心性能策略：**类型/量纲在编译期，算术在项构造，
重型求解才进 e-graph，数值热点走 f64/JIT**。

下一章用完整程序展示这些规则如何组合。



<!-- ===== 11-examples.md ===== -->


# 11 — 完整示例

这些程序不是片段，而是可以放进 `.aev` 文件、用 `aevia run` 运行或
`aevia check` 检查的完整模块。每个例子后面解释它用到的语言特性，以及
类型/量纲是如何推导的。

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
fall time from 20 m = 2.019 s
```

推导：

- `9.80665` 是十进制字面量，类型 `Float[1]`；乘以 `m/s^2` 得到
  `Float[L/T^2]`。
- `20.0 m` 是 `Float[L]`。
- `2 * h`：`2 : Int` 提升为 `Float`，结果 `Float[L]`。
- `/ g`：`L / (L/T^2) = T^2`。
- `sqrt`：指数除以 2，`T^2 → T`。
- 返回值与 `Float[T]` 统一。

`println` 用 `show`，按显示单位 `m`、`s` 打印。`h` 的显示单位是 `m`，
`fall_time` 的结果没有显式显示单位，打印时选择 `s`。

## 11.2 抛体运动：记录、ODE 与结构化状态

```aevia
module Projectile

open Units.SI
import Math { sqrt, sin, cos }

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
    let ts = linspace(0.0 s, 10.0 s, 1000)
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

输出（节选）：

```
f'(x) = 3*x^2 - 3
f'(2) = 9
g'(x) = exp(x)
roots = {2, 3}
∫₀¹ t² dt = 1/3
exp(t) ≈ 1 + t + t^2/2 + t^3/6 + t^4/24 + t^5/120 + O(t^6)
```

要点：

- `diff` 接受函数，不接受表达式。它创建同量纲的符号，应用函数，求导，
  再代回。因此 `diff(g)` 对任意 `g` 都成立。
- `df(2)` 是精确的 `9`，类型 `Real[1]`；要浮点，写 `approx(df(2))`。
- `solve`/`defint` 返回 `Result`，因为可能无解/不可积；`diff` 是 total 的，
  失败时保持未化简表达式。
- `series` 的结果带 `O(t^6)`；量纲仍是 `Real[1]`。

## 11.4 欧姆定律：带量纲的线性代数

```aevia
module Circuits

open Units.SI
import LinearAlgebra { linsolve, det }

fn main(): IO Unit = do
    let R = [[1.0, 2.0], [3.0, 4.0]] * ohm
    let I = [0.1, 0.2] * A

    let V = R @ I
    println("V = {V}")                  # [0.5 V, 1.1 V]

    let P = V * I
    println("P = {sum(P)}")             # 0.27 W

    match linsolve(R, V) with
    | Ok(I2) -> println("I = {I2}")     # 回到 [0.1 A, 0.2 A]
    | Err(e) -> println("solve failed: {e}")

    # det 只接受无量纲矩阵：
    # let d = det(R)                    # E0410
    let d = det(R / ohm)
    println("det(R/Ω) = {d}")
```

推导：

- `[[...]]` 是 `Tensor<2, Float>`；乘以 `ohm : Unit[Resistance]` 得到
  `Tensor<2, Float[Resistance]>`。
- `R @ I`：元素量纲 `Resistance * Current = Voltage`；结果
  `Tensor<1, Float[Voltage]>`。
- `V * I`：`Voltage * Current = Power`；`sum` 保持量纲。
- `linsolve(R, V)`：`Voltage / Resistance = Current`。
- `det(R)` 被拒绝，因为 `det` 的量纲是 `d^n`，`n` 是运行时阶数；`R / ohm`
  是无量纲矩阵，`det` 合法。

这个例子展示了“单位是编译期尺度”的好处：矩阵元素带单位，矩阵乘法自动
得到电压，不需要在运行时给每个元素贴标签。

## 11.5 温度：仿射点与温差

```aevia
module Thermo

open Units.SI

fn main(): IO Unit = do
    let T1 = 25.0 °C                 # Point[Θ]
    let T2 = point(300.0, K)         # Point[Θ]
    let dT = T2 - T1                 # Float[Θ]
    let T3 = T1 + 10.0 K             # Point[Θ]

    println("T1 = {T1}")
    println("T2 = {to(°C, T2)}")
    println("T3 = {T3}")
    println("ΔT = {dT}")

    # 下面两行是类型错误：
    # let bad1 = T1 + T2             # E0405
    # let bad2 = 2.0 * T1            # E0405
```

输出：

```
T1 = 25 °C
T2 = 26.85 °C
T3 = 35 °C
ΔT = 1.85 K
```

要点：

- `25.0 °C` 是仿射单位字面量，类型 `Point[Θ]`，值是 298.15 K。
- `300.0 K` 是线性单位字面量，类型 `Float[Θ]`，是温差，不是绝对温度。
  绝对温度用 `point(300.0, K)`。
- `Point - Point` 得到温差；`Point + 温差` 得到点；`Point + Point` 和
  `k * Point` 是错误。
- `to(°C, T2)` 只改显示单位；`T2` 的值仍是 K。

## 11.6 数据分析：`Table` 与动态列

```aevia
module Analysis

open Units.SI
import Data.Csv { read_csv, column, Table }
import Stats { mean, std }
import Plot { line, combine, plot, label }

fn main(): IO Unit = do
    r <- read_csv<{ time: Float[T], velocity: Float[L/T] }>("measurements.csv")
    match r with
    | Err(e) -> println("cannot read: {e}")
    | Ok(tbl) ->
        match (column(tbl, "time"), column(tbl, "velocity")) with
        | (Ok(t), Ok(v)) ->
            println("mean speed = {mean(v)}")
            println("std  speed = {std(v)}")
            let spec = label(line(t, v), "v(t)")
            plot(combine([spec]))
        | _ -> println("column type mismatch")
```

要点：

- `read_csv<ρ>` 是 `IO (Result<Table<ρ>, DataError>)`；`ρ` 是行类型，
  表头单位用于推断字段类型。`<-` 绑定结果，`match` 处理错误。
- `column(t, "time")` 在编译期用 `Table<ρ>` 的字段类型检查，运行时再做
  防御性校验；CSV 表头可以带单位（`time [s]`），没有时列被当作无量纲，
  显式 `ρ` 可以覆盖。
- `line` 接受带量纲的 `x`/`y` 张量，自动生成轴标签（`t / s`、
  `v / (m/s)`）；需要覆盖时用 `PlotOpts.xunit`/`yunit`。
- `combine([spec])` 构造 `PlotSpec`；`plot` 写 HTML/SVG。

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

fn linear_fit<d, e>(xs: Tensor<1, Real[d]>, ys: Tensor<1, Real[e]>)
    : { slope: Real[e/d], intercept: Real[e] } =
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

这些函数对任何量纲都成立；运行时没有单态化，量纲在类型检查后擦除。

## 11.9 错误示例：编译器如何拒绝

```aevia
let bad1 = 3 m + 2 s
let bad2 = sin(3 m)
let bad3 = (3 m)^n              # n : Int
let bad4 = 25.0 °C * 2.0
let bad5 = 1 / (0 m)
```

诊断（节选）：

```
error[E0401]: dimension mismatch
 --> bad.aev:1:12
  |
1 | let bad1 = 3 m + 2 s
  |            ^^^^^^^^^ expected Real[L], found Real[T]
  = left:  L
  = right: T
help: both operands of `+` must have the same dimension

error[E0403]: transcendental function requires a dimensionless argument
 --> bad.aev:2:12
  |
2 | let bad2 = sin(3 m)
  |                ^^^ found Real[L]
help: divide by a unit first, e.g. `sin(3 m / m)`

error[E0404]: dimensionful base with a runtime exponent
 --> bad.aev:3:12
  |
3 | let bad3 = (3 m)^n
  |            ^^^^^^^ base has dimension L, exponent is Int
help: use a literal exponent, or `pow_dyn` for a runtime dimension

error[E0405]: cannot scale an affine point
 --> bad.aev:4:12
  |
4 | let bad4 = 25.0 °C * 2.0
  |            ^^^^^^^^^^^^^ Point[Θ] * Float[1]
help: scale the temperature difference instead

error[E0801]: division by zero
 --> bad.aev:5:12
  |
5 | let bad5 = 1 / (0 m)
  |            ^^^^^^^^^ exact division by zero
```

这些错误中，`E0401`–`E0405` 是编译期；`E0801` 是运行时错误，但
`1/(0 m)` 在常量折叠时就能发现，编译器 MAY 提前报出。错误信息的目标是：
**指出量纲差异，并给出一个可执行的修正**，而不是只打印类型名。

## 11.10 一个“全特性”程序

```aevia
module Full

open Units.SI
import Math { exp }
import LinearAlgebra { linsolve }
import Plot { line, combine, plot }

fn kinetic(m: Float[M], v: Float[L/T]): Float[Energy] =
    m * v^2 / 2

fn main(): IO Unit = do
    # 1. 单位与量纲
    let m = 2.0 kg
    let v = 3.0 m/s
    println("E = {kinetic(m, v)}")

    # 2. 符号推导
    let x = symbol("x")
    println("f' = {diff(fn t => exp(-t^2))(x)}")

    # 3. 线性代数
    let A = [[2.0, 1.0], [1.0, 3.0]]
    let b = [1.0, 2.0]
    match linsolve(A, b) with
    | Ok(x) -> println("x = {x}")
    | Err(e) -> println("solve failed: {e}")

    # 4. 数据与绘图
    let ts = linspace(0.0 s, 1.0 s, 100)
    let vs = map(fn t => 3.0 m/s * exp(-value_in(s, t) / 0.5), ts)
    let spec = label(line(ts, vs), "v(t)")
    plot(combine([spec]))
```

这个程序把单位、符号、线性代数、张量、数据、绘图串在一起，展示了 Aevia
的目标：**一门语言覆盖从公式到图表的整个工作流**，而不是在多个库之间
搬运字符串。

下一章解释这些设计选择的理由、替代方案和已知限制。



<!-- ===== 12-rationale.md ===== -->


# 12 — 设计取舍、限制与路线图

这一章解释 Aevia 为什么长成这样，和现有方案比有什么不同，哪些地方是
刻意不做的，以及从设计到实现要经过哪些阶段。

## 12.1 为什么把量纲做成类型

科学计算中的错误，大多数不是“算法写错了”，而是“单位/量纲搞错了”：

- 把 `kW` 当成 `W`，结果差 1000 倍；
- 把 `°C` 当成 `K`，差 273.15；
- 把 `Hz` 当成 `rad/s`，差 `2π`；
- 把质量流量当成体积流量；
- 在经验公式里混用英制和马氏单位。

运行时单位库（Pint、Unitful）能在测试中抓到这些错误，但错误可能在生产
才出现。类型系统能在编译期抓到，而且能**指导推断**：`mean(xs)` 的量纲由
`xs` 决定，`var(xs)` 是 `xs` 的平方，`diff(f)` 是 `f` 除以自变量。这些
规则不是库函数能表达的，必须进入类型。

代价是类型系统更复杂。Aevia 的控制方式是：

- 量纲代数限制在自由阿贝尔群 + 有理指数，统一可判定；
- 不支持静态符号指数，避免不可判定；
- 静态不可表达时用 `Dyn` 显式逃逸，而不是发明不可靠的类型；
- 单位不进类型，避免类型级换算的组合爆炸。

## 12.2 为什么单位不进类型

如果 `m` 和 `ft` 是不同类型：

```
3 m + 4 ft          # 需要类型级换算
fn f<u: Unit>(x: Real[L, u]) -> ...
```

函数签名要量化单位，类型相等要带单位，换算要在类型层做。结果是：

- 类型错误信息变成“米 vs 英尺”，而不是“长度 vs 时间”；
- 单位表的变化会破坏类型兼容；
- 泛型函数要处理无限多的单位组合。

Aevia 的选择是：**类型只记量纲，单位是编译期尺度**。`3 km` 和 `3000 m`
是同一个类型、同一个值；需要显示时用 `to`。这牺牲了“同一量纲不同单位”
的类型区分，但换来了简单、可组合的类型系统。实践中的错误几乎都是量纲
错误，而不是“米 vs 英尺”。

## 12.3 为什么纯函数

- 纯函数可以安全地并行、缓存、求导、代入、化简。
- 符号计算要求表达式可替换；可变状态会破坏替换语义。
- 纯函数的类型/量纲规则更简单：没有“这个变量在什么时候被改”。
- IO 被隔离在 `IO`，库函数不会偷偷读文件/打印。

代价：

- 数值算法不能用原地循环；用尾递归 + 累加器，性能靠 TCO 和 COW。
- 某些算法（原地 LU、就地 FFT）需要运行时在唯一持有时优化；语义仍是纯的。
- 用户需要学习 `fold`/`map`/递归，而不是 `for`。

Aevia 用 `|>`、`map`、`fold`、模式匹配和尾调用把纯代码写得像数据流，
降低学习成本。

## 12.4 为什么严格求值

- 科学计算的性能可预测；没有 thunk 堆积和空间泄漏。
- 错误在发生的地方暴露，而不是在强制求值时。
- 与 rssn 的严格项构造一致。

符号计算不需要惰性：`symbol` 是值，表达式是项；需要化简时显式 `eval`。
惰性求值会让“什么时候跑重写”变得不可预测，反而不好调试。

## 12.5 为什么区分 `Real` 与 `Float`

- `Real`：精确有理数或符号表达式，适合推导、化简、精确结果。
- `Float`：IEEE double，适合数值计算、大数据、JIT。
- 十进制字面量默认 `Float`，因为写 `0.1` 的人通常想要浮点；写 `1/10`
  的人想要精确。
- 混合运算 `Float` 传染，符合科学计算惯例；编译器提示可能的有损提升。

如果只有一种数值类型：

- 全用 `Float`：`solve(x^2 - 2)` 得到 1.414...，失去精确性；符号推导
  需要另一套类型。
- 全用 `Real`：数值循环慢，BLAS/JIT 用不上；用户要手动 `to_float`。

两种类型是科学计算的“精确/近似”分界，值得显式。

## 12.6 为什么用 HM 而不是依赖类型

- HM + 量纲代数足以表达绝大多数科学公式；
- 推断是主要的易用性来源：用户不写类型签名；
- 依赖类型（shape-indexed、静态指数）会让 `reshape`、CSV、切片、广播
  的类型变得复杂，错误信息更难懂；
- 静态不可表达的情况用 `Dyn`，比“写不出来的依赖类型”更实用。

Aevia 不是证明助手；它要的是“大多数程序不用标注，标注了能检查”。

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

Aevia 的差异点：

1. 量纲是类型，单位不是；类型简单，单位表可扩展。
2. 符号与数值同一门语言；`diff`/`solve` 接受函数，不是字符串。
3. 纯函数 + `IO` 隔离；符号替换安全。
4. `Dyn` 显式处理静态不可表达的量纲。
5. 后端复用 rssn 的 e-graph、数值内核、JIT 和仿真，不重复造轮子。

## 12.8 设计边界（不是版本限制）

这些是**有意的设计边界**，不是“以后再说”的版本待办。每条都说明为什么
这样定，以及后端未实现时的表现（capability 诊断，而不是删语言特性）。

### 12.8.1 形状细化只做线性 `Nat`

`Tensor<[3,4],τ>` 支持形状检查，但类型级 `Nat` 只做字面量、`+`、`*` 和
线性等式；不做一般递归/证明。影响：需要复杂形状证明的代码要显式
`refine_shape`/`reshape`。绕法：`refine_shape` 返回 `Option`，或退到
`Tensor<n,τ>` 运行时检查。

### 12.8.2 静态符号指数不支持

`x^n`（`n : Int`，`x` 带量纲）不能静态类型化；`det`/`product` 的带量纲
形式同理。这是量纲代数的真实边界。绕法：`Dyn`；或先除以单位，做无量纲
计算，再装回量纲。

### 12.8.3 `Point` 的表示是 Float

`Point[d]` 可以是标量或张量元素，但表示是 `Float` + `UnitId`；符号绝对
温度用 `Real[Θ]` 的温差，不进入 `Point`。影响：符号热力学不能直接做
绝对温度代数。绕法：温差 + 参考点；或 `Dyn`。

### 12.8.4 不确定度不是核心类型

`Measurement<d>` 是标准库类型，只做独立误差传播；相关误差用
`Covariance<d,e>`。影响：实验数据分析的不确定度要显式管理。绕法：库类型；
不进入核心类型系统。

### 12.8.5 不做用户自定义效果/宏

`IO` 是唯一效果；没有 effect handlers、没有宏。这是有意的边界：扩展点
是 RSSN 算子、插件、`Rssn` 动态调用和 registry。影响：DSL/代码生成要用
插件，而不是语言内宏。绕法：`@op`、插件、`Rssn.dynamic`。

### 12.8.6 `Table` 行多态有运行时边界

`Table<ρ>` 是静态 schema；动态 CSV 返回 `Table<Dynamic>`，`to_table<ρ>`
在运行时校验。影响：未知 schema 的数据仍有运行时检查。绕法：读入时给
显式 `ρ`；或用 `Table<Dynamic>` + 校验。

### 12.8.7 并行符号值需要 fork/merge

rssn 的 `Session` 是单线程；符号值绑定会话。`par_map` 对 `Real` 用
`fork/merge`（RSSN 提供后），当前串行；数值张量并行。影响：符号并行的
启动成本高。绕法：符号阶段串行，数值阶段并行。

### 12.8.8 单位命名空间歧义

`ms` 可能是毫秒或米·秒；最长匹配优先。影响：少见的单位组合需要写 `m*s`。
绕法：显式 `*`；格式化器提示。

### 12.8.9 `Real` 吸收 `Float`

`Real + Float` 变成 `Real`，`Float` 被精确转换成有理数。影响：混合符号与
浮点会离开快速路径，产生大分子/分母。绕法：数值热路径全程用 `Float`；
需要精确时保留 `Real`；编译器提示 `W0302`（`Real` 路径变慢）。

### 12.8.10 设备/GPU/分布式是运行时目标

设备不是张量类型参数；`Device` 是运行时值。影响：类型系统不区分
CPU/GPU 张量，设备错误在运行时/能力检查时发现。绕法：`on(device, ...)`
作用域；`@jit(target=...)`；capability 诊断。这样避免设备类型组合爆炸。

### 12.8.11 没有形式化语义/证明

规范是文字 + 规则，没有 Coq/Lean 形式化。影响：实现可能有微妙的类型
系统 bug。绕法：conformance 测试、属性测试、与 rssn 的交叉检查。

### 12.8.12 `RateOf` 只覆盖数值状态

`RateOf<s>` 对标量、记录、元组、张量递归定义；含 `Int`/`Bool`/`String`
或函数字段的状态需要手动拆出参数。影响：混合状态要把常量参数放进闭包。
绕法：`rhs` 捕获参数，状态只保留 `Float` 字段。

## 12.9 已关闭的设计决策

这些问题不再留到未来版本；下面是最终决定。实现可以分阶段，但语言设计不再
分叉。

| # | 问题 | 决定 |
|---|---|---|
| 1 | `Point` 扩展到张量？ | 是；`Tensor<shape, Point[d]>`，元素级点算术 |
| 2 | `Angle` 成为基量纲？ | 否；`rad` 无量纲，`deg` 是尺度 |
| 3 | `Tensor` shape-indexed？ | 是，作为 refinement：`Tensor<[3,4],τ>`；`Nat` 线性算术 |
| 4 | `det` 默认返回 `Dyn`？ | 否；无量纲 `det` + `det_unit` |
| 5 | `Real`/`Float` 混合？ | `Real` 吸收 `Float`（精确优先）；热路径显式 `Float` |
| 6 | `Dyn` 错误？ | 算术 `panic` + `try_*`；`check` 返回 `Result` |
| 7 | `RateOf` 嵌套？ | 支持标量、记录、元组、张量，递归定义 |
| 8 | `Table` 静态 schema？ | 是，`Table<ρ>` 行多态；`Table<Dynamic>` 仍保留 |
| 9 | `@` 还是 `*` 矩阵乘？ | 保持 `@`；`*` 逐元素 |
| 10 | `%` 取模？ | 否；`mod`/`rem` |
| 11 | `let` 默认递归？ | 否；`fn` 递归，`let rec` 显式 |
| 12 | 顶层 IO？ | 否；`main`/REPL |
| 13 | 中心 registry？ | 是；内容寻址 + 签名 + capability，协议见 §13.7 |
| 14 | 用户量纲别名导出？ | 是；默认导出，`private` 可隐藏 |
| 15 | 单位换算类型级保证？ | 否；单位是尺度，量纲是类型 |
| 16 | GPU/分布式？ | 设备是运行时值；`Device`/`on`/`@jit(target)`，不是类型参数 |
| 17 | 稀疏？ | `Sparse<F,shape,τ>` 核心类型，`Layout` 类 |
| 18 | 用户效果/宏？ | 否；扩展点是算子/插件/registry |
| 19 | 不确定度？ | `Measurement`/`Covariance` 标准库类型 |
| 20 | 算子集成？ | 反射优先：Kernel Manifest + 动态 `Value` + 生成 façade；无手写绑定 |

## 12.10 实现路线图

这些阶段是**实现依赖顺序**，不是语言版本；语言设计不按 M0/M1 分叉。每个
阶段都必须保持第 13 章的 manifest/动态调用契约，避免“先做一个阉割版再
补”的历史包袱。

### M0 — 语法与类型核心（4–6 周）

- 词法/布局/解析/CST/AST；
- 名字解析、模块骨架；
- 类型/量纲代数、统一、泛化、默认化；`rssn-dim` 适配层；
- 形状细化（`Tensor<[3,4],τ>`）与 `Nat` 线性约束；
- 错误码/诊断/格式化器雏形；
- 验收：能检查第 11 章的所有例子（不运行）。

### M1 — 求值器与标量后端（4–6 周）

- Core IR、解释器、闭包、记录/变体、模式匹配；
- `Real`/`Float`/`Int` 值；
- Kernel Manifest 读取 + `Rssn` 模块生成（类型化 + `dynamic`）；
- rssn 桥：Session/Term/Config/compute；
- `eval`/`approx`/`simplify`/`diff`；
- 单位 registry、字面量折叠、`Point`；
- 验收：11.1–11.5 可运行；1000+ 算子 manifest 可见。

### M2 — 张量与线性代数（3–5 周）

- 张量存储、广播、索引/切片、COW；形状细化检查；
- `map`/`fold`/`sum`/`mean`/`std`；
- 线性代数内核（faer/rssn）；`Sparse<F,shape,τ>` 与 `Layout`；
- `zeros`/`linspace`/`reshape`；
- 验收：11.4、11.8 可运行。

### M3 — 符号微积分与方程（4–6 周）

- `diff`/`antiderivative`/`defint`/`limit`/`series`；
- `solve`/`nsolve`/`dsolve`；
- `sum`/`product`；
- 预算/中断/错误映射；
- 验收：11.3 可运行；与 rssn 的 conformance 测试。

### M4 — 模块、包、REPL、LSP（4–6 周）

- 模块/导入/可见性/接口文件；
- 包清单/锁文件/构建；
- 中心 registry 协议（索引/发布/yank/签名）；
- REPL/CLI/格式化；
- LSP 诊断/悬停/补全；
- 验收：多模块项目、`aevia test`、registry 发布/安装。

### M5 — 数据、绘图、仿真（3–5 周）

- CSV/NPY/JSON；`Table<ρ>` 行多态 schema；
- PlotSpec/HTML/SVG；
- `Device`/`on`/`to_device`；`simulate` + `Physics.*` 包装；
- 验收：11.6、11.7 可运行。

### M6 — JIT 与性能（4–6 周）

- `Term::compile` 集成；`Select`/索引 lowering；
- 插件算子 lowering；热点检测/缓存/回退；
- 基准/性能回归；
- 验收：JIT 热函数 ≥ 5× 解释器；张量运算达标；插件可 JIT 或回退。

### 首个稳定版验收标准

- 第 11 章所有例子可运行/可检查；
- 量纲错误全部在编译期（除 `Dyn`/数据边界）；
- 1000+ RSSN 算子通过 manifest 可见，无手写绑定；
- 与 rssn 的算子 conformance 测试通过；
- `rssn-dim`/`UnitRegistry` 往返一致；
- 插件/registry 安装/发布可用；
- 形状细化、`Point` 张量、`Table<ρ>`、`Sparse`、`Device` 至少各有一个
  端到端示例；
- 文档/教程/REPL 可用；
- 至少一个真实科学计算项目（例如 ODE/PDE/统计）端到端跑通。

## 12.11 成功标准

Aevia 成功的标志不是“功能多”，而是：

1. 一个物理/工程/数据科学用户能在一天内写出带单位、带符号推导、带数值
   求解的程序；
2. 量纲错误在编译期被抓住，错误信息能指导修正；
3. 数值热点的性能与 Python/NumPy 同量级；
4. 符号计算的能力覆盖常见微积分/方程/线性代数；
5. 代码可读，像数学，而不是像配置。

如果这五点做到，Aevia 就值得存在。

下一章是可扩展性与 RSSN 集成；之后是完整文法、诊断目录和审计变更记录。



<!-- ===== 13-extensibility.md ===== -->


# 13 — 可扩展性与 RSSN 集成

本章是规范的一部分，并且**优先于**其他章节中任何“暂不支持/以后评估”的
表述。Aevia 不按 `v1`/`v2` 分层：语言能力一次设计到位；后端尚未实现的
部分用 **capability 诊断**（`E1001`）报告，而不是删掉语言特性或让用户
等到下一个大版本。

本章的详细后端接口清单在项目根目录
[`RSSN-INTEGRATION-REQUIREMENTS.md`](../RSSN-INTEGRATION-REQUIREMENTS.md)。
这里定义 Aevia 侧的规范性契约。

---

## 13.1 三条公理

1. **单一算子注册表。** RSSN 的 `OpTable` 是唯一事实来源。Aevia 不维护
   “支持哪些算子”的清单；它读取 RSSN 的 **Kernel Manifest**，生成
   `Rssn` 模块。新算子只要在 RSSN 注册，Aevia 下一次构建就能看见。
2. **单一量纲/单位层。** `rssn-dim` 的 `DimTerm`/`DimArena`/`UnitRegistry`
   是运行时与编译期共享的规范层。Aevia 的 `Dim` 是它的类型级镜像；
   `Units.SI` 由 registry 生成，不维护第二张单位表。
3. **动态调用是正式能力。** 没有类型签名的算子仍可通过
   `Rssn.dynamic(key, args)` 调用；类型化 façade 是静态体验的增量，不是
   能力门槛。动态调用在运行时做类型/量纲检查，失败返回结构化 `OpError`。

这三条合起来解决两个长期问题：

- **1000+ 算子不能靠手写绑定。** 绑定会漂移、会漏、会随 RSSN 演进而腐烂。
- **新能力不能靠“下个版本”补。** 语言、manifest、插件 ABI 现在就定，
  后端按 capability 逐步补齐，用户不会遇到“语言升级才能用”的断崖。

---

## 13.2 Kernel Manifest 与生成的 `Rssn` 模块

### 13.2.1 Manifest

RSSN 在启动时提供确定性的 `KernelManifest`，至少包含：

```
KernelManifest
  rssn_version : String
  api_version  : Int
  manifest_hash: Bytes32
  features     : Features
  rule_sets    : [RuleSetInfo]
  ops          : [OpInfo]
  units        : [UnitDef]
  base_dims    : [BaseDimInfo]
```

`OpInfo` 必须包含：稳定键 `OpKey { rule_set, name, signature_hash }`、
别名、类别、arity、`overloads`、flags、binder、cost、eval 类型、是否
高阶、纯度、确定性、线程安全性、稳定性、since、文档、示例、量纲规则、
后端支持。完整 schema 见根目录文档 §3.1。

### 13.2.2 生成规则

Aevia 构建时：

1. 读取 manifest；校验 `api_version`（不兼容报 `E1004`）。
2. 对每个 `OpInfo`：
   - 有签名：生成 `Rssn.<name>` 类型化声明；
   - 无签名：生成 `Rssn.dynamic("<OpKey>", args)`；
   - 别名：生成 `Rssn.<alias>`；冲突时用 `Rssn.<rule_set>.<name>`。
3. 生成 `Rssn.Version`、`Rssn.ManifestHash`、`Rssn.Capabilities`。
4. 生成 LSP 补全、文档、签名存根。

Aevia 仓库**不允许**出现按算子名手写的绑定文件。只允许：

```
tools/gen_rssn_module.aev        # 生成器
rssn.signatures.overlay.toml     # 只补 RSSN 暂时缺的签名
```

CI 检查：生成器输出与提交的生成文件一致；manifest 快照变化必须显式更新。

### 13.2.3 命名空间

- 普通算子：`Rssn.sin`、`Rssn.integrate`。
- 规则集限定：`Rssn.Calculus.diff`、`Rssn.Units.convert`。
- 插件：`Rssn.<plugin>.<op>`。
- 动态：`Rssn.dynamic("calculus/diff", args)`。

名字冲突时，短名只保留**最稳定**的那个（`Stable` 优先，`since` 早者
优先）；其余必须用限定名。生成器在 manifest 中记录短名映射。

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
    | VUnit(Unit[1])
    | VQuantity(Quantity)
    | VTensor(TensorData)
    | VSparse(SparseData)
    | VRecord(Record)
    | VVariant(Variant)
    | VTerm(TermRef)
    | VFunction(Callback)
    | VDyn(Dyn<Real>)
    | VOpaque(Opaque)
```

`Value` 是**动态边界**的类型，不是核心语言的隐式子类型。核心语言仍然是
静态的；`Value` 只出现在 `Rssn.dynamic`、插件、FFI、反射 API 中。

### 13.3.2 调用

```aevia
Rssn.call    : (String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.dynamic : (String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.typed   : (String, Tensor<1, Value>, TypeRep) -> Result<a, OpError>
```

- `Rssn.call` 按 `OpKey` 或短名查找；短名歧义报 `E1002`。
- 没有签名/实验性/插件算子走 `dynamic`。
- `Rssn.typed` 在运行时把 `Value` 转成静态类型 `a`，做类型与量纲检查；
  失败返回 `OpError.Type`/`OpError.Dimension`。
- 高阶算子用 `Rssn.call_ho(key, args, fns)`；`fns` 是 `Callback`，可以
  包装 Aevia 闭包。

### 13.3.3 错误

`OpError` 是结构化变体：

```aevia
type OpError =
    | UnknownOp(String)
    | Arity(Int, Int)
    | Type(TypeRep, TypeRep, Int)
    | Dimension(DimError)
    | NotNumeric(String)
    | NotReduced(String)
    | Domain(String)
    | BudgetExceeded
    | Unsupported(String, String)   # backend, feature
    | Plugin(String)
```

`E1003` 是“没有匹配的算子/重载”，携带候选签名列表；`E1001` 是
“后端 capability 缺失”，携带 capability 名和最小 RSSN 版本。

---

## 13.4 重载、签名与解析

### 13.4.1 重载

RSSN 允许同名多签名。Aevia 的重载解析：

1. 按 arity 过滤；
2. 按参数类型/量纲匹配；
3. 多个候选时选最具体（`Tensor<[3,4],T>` 比 `Tensor<2,T>` 具体；
   `Float[L]` 比 `Real[d]` 具体）；
4. 仍歧义报 `E1002`；
5. 没有匹配时尝试 `Dynamic`；仍失败报 `E1003`。

### 13.4.2 签名覆盖

RSSN 的 1000+ 算子不要求一次写完签名。规则：

- 每个算子必须有 `Signature` 或 `dynamic_reason`；
- 没有签名的算子仍可用，但 Aevia 只生成 `dynamic` 声明；
- CI 的 `signature-audit` 报告覆盖率；覆盖率不达标不阻止构建，但阻止
  `Stable` 标记；
- 新算子用 `#[rssn_op]` 自动生成签名；旧算子用推断 + overlay。

这保证“能力完整”和“类型体验”可以分开推进，但不会有算子被静默遗忘。

---

## 13.5 插件与用户扩展

### 13.5.1 插件

插件不是新语法，而是**普通 Aevia 包 + 清单标记 + 属性**：

```toml
# aevia.toml
[plugin]
kind = "aevia"          # 或 "cdylib"
abi = 1
```

```aevia
module MyDomain

@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))

@rule("softplus(x) => ln(1 + exp(x))")
let softplus_rule = ()

dimension MyDim
unit myunit : Length = 0.5 m
```

编译后注册到 RSSN：

```aevia
Rssn.register_plugin("mydomain", plugin)
```

插件可以：

- 注册算子 + 签名 + eval 回调；
- 注册重写规则（declarative tier）；
- 注册量纲规则；
- 注册单位/基量纲；
- 注册 JIT lowering（可选）。

### 13.5.2 Aevia 函数作为 RSSN 算子

`@op` 把 Aevia 函数包装成 `HostCallback`，注册为 RSSN 算子。
这样用户可以用 Aevia 写领域算子，同时享受 RSSN 的符号重写/数值内核：

```aevia
@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))
```

限制：回调必须是纯的、线程安全的（或标记 `SessionBound`）；符号重写
规则必须显式注册，不能从函数体自动推导。

### 13.5.3 安全

- 插件必须签名；默认只加载 registry 信任的插件。
- 插件声明 capabilities（`fs`、`net`、`unsafe`）；未声明的能力在沙箱中
  被拒绝。
- 插件 ABI 版本不匹配报 `E1006`。

---

## 13.6 量纲与单位：统一到 `rssn-dim`

### 13.6.1 规范层

`rssn-dim` 提供：

- `DimTerm`：基量纲/变量的有理指数乘积，结构化访问 `factors()`/`vars()`；
- `DimArena`：基量纲/变量 interning，可快照/恢复；
- `UnitRegistry`：单位名、别名、精确尺度 `Scale { rat, pi }`、仿射
  offset、前缀、显示、类别、since；
- `Dimension`：可打印快照，但内部指数是有理数，不是字符串。

Aevia 的 `Dim` 是 `DimTerm` 的类型级镜像；两者的规范形、基名、指数运算
必须一致。编译器不复制单位表；`Units.SI` 是 registry 的生成视图。

### 13.6.2 单位声明

```aevia
dimension Currency
unit EUR : Currency = 1.08 USD
unit myunit : Length = 0.5 m
```

- `dimension` 注册到 `DimArena`；`unit` 注册到 `UnitRegistry`。
- 默认导出；`private` 只在本模块可见。
- 导出信息进入 manifest；下游包可以直接 `import` 并使用。
- 用户单位可以是仿射的（`unit °X : Temperature = 1 K offset 273.15`），
  但仿射单位只能单独使用。

### 13.6.3 `Dyn`

`Dyn<ρ>` 的运行时量纲是 `DimTerm`，不是字符串。`dyn`/`check` 是类型指导
原语：具体 `d` 在编译期物化为 `DimTerm`；泛型用 `dyn_with`/`check_with`
显式传入。`check` 返回 `Result`。

### 13.6.4 `Point` 与仿射

`Point[d]` 是静态包装；运行时是 `Float` + `UnitId`。`UnitRegistry` 的
`affine`/`offset` 是唯一事实来源。`Point` 的算术规则在 Aevia 层实现，
RSSN 的 `quantity`/`convert` 负责显示/换算。

---

## 13.7 中心包 registry

Aevia 现在就有中心 registry 的设计，不把它推到“以后”。registry 是
**内容寻址 + 签名 + 能力声明**的包索引。

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

```
GET /index.json                     # 全部包名 + 最新版本 + 摘要
GET /pkg/{name}/{version}.tar.zst   # 内容寻址的包
GET /pkg/{name}/{version}.sig       # 签名
GET /search?q=...                   # 搜索
POST /publish                       # 发布（签名）
POST /yank/{name}/{version}         # 撤回
```

索引项至少包含：name、version、content_hash、deps、aevia_range、
rssn_api、capabilities、operators、units、dimensions、stability、yanked。

### 13.7.3 可复现

- 包内容哈希覆盖源码、manifest、生成的 `Rssn` 模块签名；
- 构建不执行任意脚本；
- 依赖解析使用内容哈希 + semver；
- registry 镜像必须可验证签名。

---

## 13.8 稳定性与 capability（取代 v1/v2）

### 13.8.1 稳定性标记

```aevia
@stable
@experimental
@deprecated("use foo2")
```

- `Stable`：major 内签名/语义不变；
- `Experimental`：可改，但旧名至少保留一个 minor；
- `Deprecated`：至少一个 minor 的警告期。

### 13.8.2 Capability

后端能力用 capability 名描述：

```
core.symbolic
core.numeric
core.tensor
core.linalg
core.ode
core.pde
core.fft
core.stats
core.units
core.jit
core.jit.branch
core.jit.index
core.plugin
core.parallel_symbolic
core.gpu
core.distributed
```

Aevia 编译器在类型检查后检查 capability；缺失时：

- 编译期：如果静态可知（如 `@jit` 目标），报 `E1001`；
- 运行时：动态调用返回 `OpError.Unsupported`，Aevia 转成 `E1001`。

**不删语言特性**。例如 GPU 未实现时，`on(Gpu(0), ...)` 仍类型正确，运行
时报 `E1001("core.gpu")`；不会出现“这个语法只有 v2 才有”。

---

## 13.9 已决定的扩展设计

以下项目以前被标为“v2 再评估”；现在全部做出决定，并进入规范。

### 13.9.1 形状细化（shape refinement）

**决定：支持，作为 refinement，不是完整依赖类型。**

- `Tensor<n, τ>`：只带秩，形状存在（当前形式）。
- `Tensor<[n1, ..., nk], τ>`：形状细化；`n_i` 是类型级 `Nat`，可用
  `?` 表示未知。
- `Matrix<n, m, τ>` 是 `Tensor<[n, m], τ>` 的别名。
- 张量字面量自动获得细化形状；`zeros<[3,4]>()` 等构造器可显式指定。
- `matmul : Tensor<[m,k],τ> × Tensor<[k,n],τ> -> Tensor<[m,n],τ>`；
  秩-only 版本保留，运行时检查。
- `reshape<[n1..nk]>(a)`：如果 `a` 的形状静态已知，编译期检查元素数
  相等；否则返回 `Result`。`reshape_dyn` 总是运行时检查。
- 索引：细化形状 + 字面量索引 → 编译期越界检查；动态索引仍运行时检查。
- 细化 → 秩-only 是安全隐式转换；秩-only → 细化用 `refine_shape`
  返回 `Option`/`Result`。
- 类型级 `Nat` 支持字面量、`+`、`*` 和线性等式；不做一般递归/证明。
  这足以表达矩阵乘法、reshape、拼接，不需要完整依赖类型。

### 13.9.2 `Point` 张量

**决定：支持 `Tensor<shape, Point[d]>`。**

- 元素级 `+`/`-` 按点规则：`Point - Point -> Float[d]`，
  `Point + Float[d] -> Point`，`Point + Point` 报 `E0405`。
- `mean` 只接受 `Float`/`Real`；点的平均用 `centroid : Tensor<1,Point[d]> -> Point[d]`。
- 温度场可以直接是 `Tensor<[nx,ny], Point[Θ]>`；不再用“K 值 + 约定”。
- `Point` 张量的存储是 `Float` + `UnitId`；`Point` 的仿射偏移只在
  元素级运算时处理。

### 13.9.3 `Table` 行多态 schema

**决定：`Table<ρ>` 使用行多态静态 schema，动态 `Table` 仍保留。**

- `Table<ρ>`：`ρ` 是行类型，字段名 + 类型 + 量纲。
- `read_csv<ρ>(path)` 从表头单位推断字段类型；无法推断时报
  `DataError.Schema`，用户可给显式 `ρ`。
- `column` 是类型级字段查找：`column(t, "time")` 的结果类型由 `ρ` 的字段
类型决定。
- `select`/`join`/`rename` 保持行多态；新增字段要求 `lacks` 约束。
- 动态 CSV（未知 schema）返回 `Table<Dynamic>`；`to_table<ρ>` 做运行时
  校验。
- 这样数据管道和核心语言一样有静态 schema，不再只是“读入后赶紧转记录”。

### 13.9.4 设备、GPU、分布式

**决定：设备是运行时值，不是张量类型参数。**

- `Device = Cpu | Gpu(Int) | Distributed(Cluster)`。
- `Tensor` 类型不变；值带 `device` 元数据。
- `to_device : (Tensor<n,τ>, Device) -> IO (Tensor<n,τ>)`。
- `on : (Device, () -> a) -> IO a` 是作用域执行；纯计算可以 `on` 到
  设备，分配/传输是 IO。
- `@jit(target = Gpu(0))` 可指定 JIT 目标；没有 `core.gpu` 时运行时报
  `E1001`。
- 分布式张量用 `Sharded<Mesh, Tensor<...>>` 库类型；`Mesh` 是运行时
  拓扑，不是类型参数。这样不会为设备/分片生成类型组合爆炸。

### 13.9.5 稀疏与布局

**决定：`Sparse<F, shape, τ>` 是核心类型构造器，`F` 是格式类型。**

- 格式：`Csr`、`Csc`、`Coo`、`Block`、`Bcsr`；用户可注册新格式。
- `Layout` 类：`nnz`、`to_dense`、`from_dense`、`matmul`、`transpose`。
- `@` 按布局分派：dense×sparse、sparse×dense、sparse×sparse。
- `Sparse` 与 `Tensor` 之间显式转换；不隐式互转。
- 稀疏不是库补丁，而是类型系统的一部分；`shape` 细化同样适用。

### 13.9.6 效果与能力

**决定：不做用户自定义 effect handlers；`IO` 保持单一类型。**

- 扩展点是**算子/插件/registry**，不是语言级效果系统。
- `IO` 动作可以声明所需 **capability**（`fs`、`net`、`env`、`rand`、
  `gpu`）；这是运行时权限，不是类型参数。
- `with_cap(cap, thunk)` 在沙箱中执行；没有能力时 IO 动作失败。
- 这样避免 effect row 对全部标准库签名的侵入，也避免“v2 才有自定义
  效果”的断层。需要 DSL/代码生成时用插件/算子，不用宏。

### 13.9.7 宏与语法扩展

**决定：不提供用户宏；语法固定。**

- 扩展通过算子、插件、registry、`Rssn` 动态调用完成。
- 不引入 `macro_rules`/卫生宏/语法扩展，避免工具链分叉。
- 如果未来确实需要，必须以**独立、可选的预处理层**出现，不进入核心
  语法；当前规范不预留语法。

### 13.9.8 角度

**决定：`Angle` 不是基量纲；`rad` 无量纲，`deg` 是尺度。**

- `sin`/`cos` 接受 `Real[1]`；`deg` 字面量折叠为 `pi/180`。
- 不引入 `Angle` 基量纲，避免破坏 `exp(i*θ)`、`sin(ωt)` 等公式。

### 13.9.9 不确定度

**决定：`Measurement<d>` 是标准库类型，不是核心类型。**

- 独立误差传播：`Measurement<d> = { value: Float[d], sigma: Float[d] }`。
- 相关误差用 `Covariance<d,e>` 库类型；`linear_fit` 返回协方差。
- 核心语言只保证量纲；不确定度是库的领域模型。

### 13.9.10 其他已关闭的问题

| 问题 | 决定 |
|---|---|
| `det` 默认返回 `Dyn`？ | 否；无量纲 `det` + `det_unit` |
| `Real`/`Float` 混合？ | `Real` 吸收 `Float`（精确优先）；热路径显式 `Float` |
| `Dyn` 错误？ | 算术 `panic` + `try_*`；`check` 返回 `Result` |
| `RateOf` 嵌套？ | 支持标量、记录、元组、张量，递归定义 |
| `Table` 静态 schema？ | 是，`Table<ρ>` 行多态 |
| `@` 还是 `*` 做矩阵乘？ | 保持 `@`；`*` 逐元素 |
| `%` 取模？ | 否；`mod`/`rem` |
| `let` 默认递归？ | 否；`fn` 递归，`let rec` 显式 |
| 顶层 IO？ | 否；`main`/REPL |
| 中心 registry？ | 是，现在设计并实现协议 |
| 用户量纲别名导出？ | 是，默认导出，`private` 可隐藏 |

---

## 13.10 兼容性与迁移

- **语言版本**用 semver；不设“v1 语言/v2 语言”。major 才允许破坏性
  变更；minor 只加能力/算子/库；patch 只修 bug。
- **manifest 兼容**：`api_version` 不兼容 +1；Aevia 编译器 pin 范围；
  `manifest_hash` 变化使缓存失效，但旧签名仍可用到下一个 major。
- **算子弃用**：`Deprecated` 至少一个 minor 的警告；别名保留。
- **包 registry**：内容哈希 + 签名 + capability；yank 不删除内容，只
  阻止新解析。
- **迁移工具**：`aevia fix` 读取旧 manifest/签名，自动加限定名/别名；
  不强制用户重写代码。

---

## 13.11 错误码

| 代码 | 情况 |
|---|---|
| `E1001` | 后端 capability 缺失（`core.gpu`、`core.jit.branch` 等） |
| `E1002` | 算子/重载歧义 |
| `E1003` | 没有匹配的算子/重载 |
| `E1004` | RSSN `api_version` 不兼容 |
| `E1005` | manifest/签名哈希不匹配 |
| `E1006` | 插件 ABI/加载失败 |

这些错误属于**环境/后端**，不是用户程序类型错误；诊断必须给出
capability 名、最小 RSSN 版本和可执行的替代方案。



<!-- ===== appendix-a-grammar.md ===== -->


# 附录 A — 完整文法

本附录是规范性 EBNF。记法：

- `{ X }` 零次或多次；`[ X ]` 可选；`|` 选择。
- 终结符用 `"..."`；`NEWLINE`、`INDENT`、`DEDENT`、`EOF` 是布局记号
  （§1.4）。
- `INT`、`FLOAT`、`STRING`、`IDENT`、`UIDENT`、`UNIT_NAME`、`SUPERSCRIPT`、
  `LT_APP`、`ELLIPSIS` 是词法记号。
- `IDENT` 首字母小写（或 `_`）；`UIDENT` 首字母大写；`qname` 是限定名。

## A.1 布局

```
NEWLINE   ::= 每个逻辑行末尾
INDENT    ::= 缩进增加时
DEDENT    ::= 缩进减少时
```

- 括号 `()`、`[]`、`{}` 内部不产生 `NEWLINE`/`INDENT`/`DEDENT`。
- 空行和纯注释行不产生记号。
- 缩进必须匹配栈中的某一层；否则 `E0102`。
- 块和续行都由 `NEWLINE INDENT ... DEDENT` 表示：

```
block        ::= NEWLINE INDENT { block_stmt } expr NEWLINE? DEDENT
layout_group ::= NEWLINE INDENT expr NEWLINE? DEDENT
block_stmt   ::= let_decl | fn_decl
```

`block` 用于 `let`/`fn` 序列；`layout_group` 用于跨行表达式。解析器在
`NEWLINE INDENT` 后看到 `let`/`fn` 时选 `block`，否则选 `layout_group`。
`NEWLINE?` 允许内层块已经消费了行尾。

## A.2 程序与声明

```
program        ::= [ module_decl ] { import_decl } { top_decl } EOF

module_decl    ::= "module" module_path NEWLINE
module_path    ::= UIDENT { "." UIDENT }

import_decl    ::= "import" module_path [ "as" UIDENT ]
                   [ "{" name_list "}" ] NEWLINE
                 | "open" module_path [ "{" name_list "}" ] NEWLINE
name_list      ::= name { "," name }
name           ::= IDENT | UIDENT

top_decl       ::= { attribute } [ "private" ] decl
attribute      ::= "@" IDENT [ "(" [ expr { "," expr } ] ")" ] NEWLINE?
decl           ::= dimension_decl
                 | unit_decl
                 | type_decl
                 | class_decl
                 | instance_decl
                 | derive_decl
                 | fn_decl
                 | let_decl
                 | test_decl

dimension_decl ::= [ "private" ] "dimension" UIDENT [ "=" dim ] NEWLINE

unit_decl      ::= "unit" unit_name ":" dim "=" unit_rhs
                   [ "prefixable" ] NEWLINE
unit_rhs       ::= unit_scale [ unit_expr ] [ "offset" unit_scale unit_expr ]
unit_scale     ::= [ "-" ] number [ "/" number ]
                 | "pi" [ "/" number ]
unit_name      ::= UNIT_NAME | IDENT | qname

type_decl      ::= "type" UIDENT [ type_params ] "=" type_body NEWLINE?
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

- `type_body` 的单行变体 MUST 以 `|` 开头，以便与类型别名区分；缩进形式
  可以省略首个 `|`（但建议保留）。
- 声明末尾的 `NEWLINE?` 允许表达式是块（块已消费行尾）。
- `import M { ... }` 是选择性打开；`import M` 只绑定限定名。
- `open M { ... }` 是选择性打开。

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

match_expr     ::= "match" expr "with" NEWLINE INDENT { match_arm } DEDENT
match_arm      ::= "|" pattern [ "if" expr ] "->" expr NEWLINE?

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

**歧义说明**

- `LT_APP` 是紧邻标识符的 `<`（无空格）；`a < b` 的 `<` 前有空格，按比较
  解析。格式化器 MUST 在比较的 `<` 前保留空格。
- `record_body` 用 `with` 区分更新与字面量。
- `quantity_lit` 只在数值字面量后紧跟单位表达式时产生；`3 + 4` 不是。
- `match_arm`/`do_stmt` 的 `NEWLINE?` 允许块体已消费行尾。
- `qname` 以 `UIDENT` 开头；`Physics.Ising.run` 解析为 `qname` +
  `.run` 字段。

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

- 裸 `UIDENT` 是零参构造器模式；裸 `IDENT` 是绑定。
- 浮点字面量不是合法模式。

## A.5 类型

```
type           ::= [ "forall" type_param { "," type_param } "." ] fn_type
fn_type        ::= type_atom [ "->" type ]
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
                 | "(" type ")"

field_type     ::= IDENT ":" type
type_arg       ::= type | nat | dim | shape
format_name    ::= IDENT | UIDENT
shape          ::= nat | "?" | "[" [ shape_item { "," shape_item } ] "]"
shape_item     ::= nat | "?"
row_type       ::= type { "+" type }
nat            ::= INT | IDENT | nat "+" nat
rep            ::= "Int" | "Real" | "Float" | "Complex" | IDENT
```

- `Real` 不带方括号等价于 `Real[1]`；`Float`、`Complex` 同理。
- `Unit` 不带方括号等价于 `()`；`Unit[d]` 是单位值类型。
- `(A, B)` 是元组类型；`(A, B) -> C` 是多参函数类型。
- `Tensor` 的第一个参数是 `shape`（秩 `n` 或 `[n1,...,nk]`）；`Dyn` 的参数是
  `rep`；其他 `type_arg` 按 kind 解析。解析器在歧义时优先按 `shape`/`rep`
  解析 `Tensor`/`Dyn` 的第一个参数。
- `A * B` 不是合法类型；`*` 只出现在量纲中。
- `Tensor<0, τ>` 定义上等于 `τ`。
- `RateOf<s>` 是内建类型函数（§8.9）。

## A.6 量纲与单位

```
dim            ::= dim_factor { ( "*" | "/" | "·" ) dim_factor }
dim_factor     ::= dim_atom [ "^" exponent ]
dim_atom       ::= "1" | UIDENT | IDENT | "(" dim ")"

exponent       ::= [ "-" ] INT
                 | "(" [ "-" ] INT "/" INT ")"

unit_expr      ::= unit_factor { ( "*" | "·" | "/" ) unit_factor }
unit_factor    ::= unit_atom [ "^" exponent | SUPERSCRIPT+ ]
unit_atom      ::= unit_name | "(" unit_expr ")" | "%"
```

- `dim` 中 `UIDENT` 是基量纲/别名，`IDENT` 是量纲变量。
- `L/T^2` 左结合，等价于 `L/(T^2)`。
- `unit_name` 可以带前缀（`km`）或 `°C`。
- 单位表达式内不允许数字，指数除外。
- 仿射单位不能出现在乘积/幂中（语义检查 `E0508`）。

## A.7 运算符优先级

从低到高：

| 级别 | 运算符 | 结合性 |
|---|---|---|
| 1 | `\|>` `<\|` | 左（混用时按出现顺序） |
| 2 | `\|\|` | 右 |
| 3 | `&&` | 右 |
| 4 | `==` `!=` `<` `<=` `>` `>=` | 非结合 |
| 5 | `::` | 右 |
| 6 | `+` `-` | 左 |
| 7 | `*` `/` `·` `×` `÷` `@` | 左 |
| 8 | 前缀 `-` `+` `!` `√` | 前缀 |
| 9 | `^` | 右 |
| 10 | 类型应用、调用、索引、字段、`?`、后缀 `!` | 后缀 |

`..`/`..=` 只在索引中使用，不参与表达式优先级。

## A.8 词法记号（摘要）

```
IDENT          ::= lower XID_Continue { XID_Continue | "'" | "₀".."₉" }
UIDENT         ::= upper XID_Continue { XID_Continue | "'" | "₀".."₉" }
INT            ::= dec_int | "0x" hex { hex } | "0o" oct { oct } | "0b" bin { bin }
FLOAT          ::= dec_int "." dec_int [ exponent ]
                 | dec_int exponent
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

完整词法规则见第 01 章。`for`、`rec`、`in` 是上下文关键字，但仍不能作为
普通标识符。`forall` 只在类型开头（后跟 type_param 和 `.`）时是关键字；
它同时是标准库函数名。`return` 是 Prelude 函数，不是关键字。



<!-- ===== appendix-b-diagnostics.md ===== -->


# 附录 B — 诊断目录

诊断是语言体验的一部分。Aevia 的错误信息 MUST 包含：代码、主位置、期望/实际、
以及至少一条可执行的修正建议（如果能确定）。本附录列出错误码、严重级别和
消息格式。

## B.1 格式

```
error[E0401]: dimension mismatch
  --> src/kinematics.aev:7:13
   |
 7 |     sqrt(2 * h * g)
   |              ^^^^^ expected Real[T^2], found Real[L^2/T^2]
   |
   = left:  L^2/T^2
   = right: T^2
   = difference: L^2/T^4
help: did you mean `2 * h / g`?
```

- 第一行：`severity[code]: message`。
- `-->`：文件:行:列。
- 源码行 + 下划线标签；标签尽量短。
- `=`：附注（规范形、差异、相关声明）。
- `help:`：修正建议；最多两条，按置信度排序。
- 多位置错误用多个 `-->` 或 `= note: ...` 指向定义处。

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
| `E0105` | invalid character | 删除/替换 |
| `E0106` | invalid numeric literal | 检查下划线/指数 |
| `E0107` | invalid superscript sequence | 用 `^n` |
| `E0108` | expected a unit after numeric literal | 写 `3 * x` 或补单位 |

## B.4 语法错误（E02xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0201` | empty block | 加一个表达式 |
| `E0202` | syntax error | 检查附近记号 |
| `E0203` | ragged tensor literal | 使每层等长 |
| `E0204` | invalid pattern | 检查构造器/字段 |
| `E0205` | invalid operator section | 用 `(+)` 或 lambda |
| `E0206` | reserved word used as identifier | 改名 |
| `E0207` | ambiguous type application | 在 `<` 前加空格或加括号 |
| `E0208` | duplicate field | 删除重复字段 |
| `E0209` | invalid unit literal | 检查单位表达式 |
| `E0210` | unknown attribute | 检查属性名/参数 |

## B.5 类型错误（E03xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0301` | unbound name | 导入/拼写 |
| `E0302` | type mismatch | 检查标注/参数 |
| `E0303` | infinite type (occurs check) | 加标注/改递归 |
| `E0304` | ambiguous type | 加类型标注 |
| `E0305` | not a function | 检查调用 |
| `E0306` | missing record field | 补字段/改类型 |
| `E0307` | constructor arity mismatch | 补/删参数 |
| `E0308` | pattern type mismatch | 改模式 |
| `E0309` | non-exhaustive match | 补分支 |
| `E0310` | no instance for class | 导入实例/加实例 |
| `E0311` | overlapping instances | 合并/删除 |
| `E0312` | non-positive recursive type | 移到箭头右侧 |
| `E0313` | IO in pure context | 用 `do`/返回 IO |
| `E0314` | cannot infer rank | 加 `Tensor<n, ...>` 标注 |
| `E0315` | wrong number of arguments | 补/删参数 |
| `E0316` | duplicate definition | 改名/删除 |
| `E0317` | cannot derive instance | 手写实例 |

## B.6 量纲错误（E04xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0401` | dimension mismatch | 使两边同量纲 |
| `E0402` | dimension used as exponent | 指数必须无量纲 |
| `E0403` | transcendental function requires dimensionless argument | 除以单位 |
| `E0404` | dimensionful base with runtime exponent | 字面量指数/`pow_dyn` |
| `E0405` | invalid affine point operation | 用温差/`lerp` |
| `E0406` | bare `0` used where a dimensioned value is expected | 写 `0 m` |
| `E0407` | unknown dimension/alias | 声明/导入 |
| `E0408` | unsatisfiable dimension constraint | 检查公式 |
| `E0409` | dimension occurs check | 简化量纲 |
| `E0410` | dimensionful `det`/`product` requires `Dyn<ρ>` | 除以单位/`det_unit` |
| `E0411` | inconsistent tensor element dimensions | 统一元素量纲 |
| `E0412` | `Dyn` dimension check failed (runtime) | 检查运行时量纲 |

警告：

| 代码 | 消息 |
|---|---|
| `W0301` | unresolved dimension defaulted to 1 |
| `W0302` | `Real`/`Float` mix uses the exact `Real` path; performance hint |

## B.7 单位错误（E05xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0501` | unknown unit | 导入/拼写 |
| `E0502` | unit not imported | `open Units.SI` |
| `E0503` | prefix cannot be applied | 用显式单位 |
| `E0504` | affine offset dimension mismatch | 检查 offset |
| `E0505` | duplicate base unit | 删除重复 |
| `E0506` | unit definition cycle/scale not dimensionless | 检查定义 |
| `E0507` | prefix unit conflicts with explicit unit | 改名/删除 |
| `E0508` | affine unit in product/power | 单独使用 |
| `E0509` | `to` unit dimension mismatch | 换单位 |
| `E0510` | affine display on a temperature difference | 用 `delta_to` |

## B.8 模块错误（E06xx）

| 代码 | 消息 |
|---|---|
| `E0601` | module name does not match path |
| `E0602` | cyclic import |
| `E0603` | ambiguous opened name |
| `E0604` | orphan instance |
| `E0605` | invalid package manifest |
| `E0606` | dependency version conflict |
| `E0607` | module/package not found |
| `E0608` | missing/invalid entry point |

## B.9 效果错误（E07xx）

| 代码 | 消息 |
|---|---|
| `E0701` | `do` block requires `IO` |
| `E0702` | `?` used outside `Result`/`Option` |

## B.10 运行时错误（E08xx）

| 代码 | 消息 | 可恢复 |
|---|---|---|
| `E0801` | division by zero | 精确：否；浮点：IEEE |
| `E0802` | shape/index error | 否 |
| `E0803` | free symbols in `approx` | 是 |
| `E0804` | cannot decide symbolic condition | 否 |
| `E0805` | stack overflow | 否 |
| `E0806` | domain error | 是 |
| `E0807` | numerical method did not converge | 是 |
| `E0808` | budget exhausted | 是 |
| `E0809` | internal type/dimension invariant violated | 否 |
| `E0810` | IO error | 是 |
| `E0899` | panic | 否 |

## B.11 IO 错误（E09xx）

| 代码 | 消息 |
|---|---|
| `E0901` | file not found |
| `E0902` | permission denied |
| `E0903` | parse error in data file |
| `E0904` | unsupported format |
| `E0905` | plot rendering failed |

## B.12 警告（W0xxx）

| 代码 | 消息 |
|---|---|
| `W0101` | unused variable |
| `W0102` | unused import |
| `W0103` | shadowed binding |
| `W0201` | redundant match arm |
| `W0202` | discarded IO result |
| `W0203` | unreachable code |
| `W0301` | unresolved dimension defaulted to 1 |
| `W0302` | `Real`/`Float` mix uses the exact `Real` path; performance hint |
| `W0303` | unused symbol |
| `W0501` | display unit dropped |
| `W0601` | unused private declaration |

## B.13 消息写作规范

1. **说人话。** `expected Real[L], found Real[T]` 比 `type unification failed`
   好。
2. **指小范围。** 下划线覆盖最小错误子树，不要整行。
3. **给差异。** 量纲错误列出 `left`、`right`、`difference` 的规范形。
4. **给一个修正。** 只在置信度高时给 `help`；不要列五个猜测。
5. **不要重复。** 同一错误在多个位置只报一次，其余用 `note` 指向主位置。
6. **保留源码。** 错误信息中的源码行不截断运算符；必要时换行。
7. **可解析。** 代码和位置稳定，便于 IDE/CI 解析。

## B.14 示例：类型错误

```
error[E0302]: type mismatch
  --> src/analysis.aev:12:18
   |
12 |     let m = mean(xs) + 1
   |                      ^ expected Real[L], found Int
   |
   = `mean(xs)` has type Real[L]
help: use `1 m` if you meant one metre
```

## B.15 示例：单位错误

```
error[E0501]: unknown unit `kms`
  --> src/travel.aev:3:14
   |
 3 | let d = 3.2 kms
   |              ^^^ not a unit
   |
help: did you mean `km`?
```

## B.16 示例：运行时形状错误

```
error[E0802]: shape mismatch in `matmul`
  --> src/linalg.aev:8:13
   |
 8 |     let C = A @ B
   |             ^^^^^ left shape [3, 4], right shape [3, 2]
   |
   = matmul requires left columns = right rows
help: transpose B or reshape A
```

## B.17 扩展/后端错误（E10xx）

| 代码 | 消息 | 说明 |
|---|---|---|
| `E1001` | backend capability missing | 携带 capability 名与最小 RSSN 版本 |
| `E1002` | ambiguous operator/overload | 列出候选签名 |
| `E1003` | no matching operator/overload | 列出候选签名/动态入口 |
| `E1004` | incompatible RSSN api_version | 要求升级/降级 |
| `E1005` | manifest/signature hash mismatch | 重新生成 `Rssn` 模块 |
| `E1006` | plugin ABI/load failure | 检查插件签名/版本 |

这些是**环境/后端**错误，不是用户程序类型错误；诊断必须给出可执行的
替代方案（例如“安装带 `core.gpu` 的 RSSN”或“用 `Rssn.dynamic`”）。

下一附录记录规范的修订历史。



<!-- ===== appendix-c-changelog.md ===== -->


# 附录 C — 修订记录

本附录记录规范的每一次审计和实质性修改。修订号与 `README.md` 的
“Design Draft”状态对应。

## Revision 0 — 初始设计（2026-10-31）

- 建立语言定位：纯函数式、量纲即类型、符号/数值双相、rssn 后端。
- 完成 00–12 章与附录 A/B。
- 关键决定：
  - 量纲是类型，单位是编译期尺度；
  - `Int`/`Real`/`Float`/`Complex` 四种标量表示；
  - 十进制字面量默认 `Float`，整数默认 `Int`；
  - HM + 量纲代数 + 类型类 + 行多态；
  - 张量秩静态、形状动态；
  - `Point` 仿射点（标量、Float）；
  - `Dyn` 动态量纲逃逸；
  - 纯核心 + `IO`；
  - rssn 负责符号/数值/仿真，Aevia 负责语法/类型/运行时。

## Revision 1 — 第一轮审计（类型/语法/易用性）

第一轮发现并修复的主要问题：

1. **零不再量纲多态。** `0` 是 `Int`；零长度写 `0 m`。删除了
   “零乘任何值吸收量纲”的类型特例，增加 `E0406`。
2. **`Float` 严格是 f64。** 不再允许“符号 Float”；`Real + Float` 的结果
   改为 `Real`（精确优先），消除了“类型通过但运行时 panic”的洞。
3. **符号 Bool 改为 `Prop`。** `Bool` 只有 `true`/`false`；`Real` 比较
   返回 `Prop`；`if`/`match` 只接受 `Bool`；增加 `decide`/`assuming`/
   `by_cases`。
4. **`Dyn` 改为 `Dyn<ρ>`。** 保留表示；运行时量纲类型改名 `DimValue`；
   补 `unit_pow_dyn`；`check` 返回 `Scalar<ρ,d>`。
5. **`solve` 不再返回 `Set`。** 改为 `Solutions<ρ,d>`（根 + 重数 +
   条件）；`Set` 要求 `TotalOrd`，`Float` 不是 `TotalOrd`。
6. **`diff` 统一为一参返回函数。** 增加 `diff_at`；数值导数改名
   `numeric_deriv`；ODE 右端函数改名 `rhs`。
7. **`@` 按秩分派。** 2×2 `matmul`、2×1 `matvec`、1×2 `vecmat`、
   1×1 `dot`；修复 `A @ x` 示例。
8. **单位与张量算术补齐。** `Tensor * Unit`、`Tensor / Unit`；修复
   `0.02 / m`、`R / ohm`。
9. **`Point` 仿射语义收紧。** 仿射单位只接受 `Point`；`value_in`/
   `to` 对温差报 `E0510`；`Point + Scalar<ρ,Θ>` 明确允许。
10. **行统一加 lacks 约束。** 记录更新新增字段要求 `row lacks l`；
    重复字段 `E0208`。
11. **文法重写。** 布局改为 `NEWLINE INDENT ... DEDENT` 块/布局组；
    增加 `LT_APP`、`UNIT_NAME`、`SUPERSCRIPT`、`ELLIPSIS`、`PERCENT`；
    单行变体要求前导 `|`；lambda 支持 `fn x =>` 简写；`forall` 改为
    上下文关键字；`return` 改为 Prelude 函数。
12. **示例修正。** `fall_time` 统一 `Float`；`open Units.SI`；
    `linsolve`；`line` 直接接受带量纲张量。

## Revision 2 — 第二轮审计（后端/自洽/对抗）

第二轮发现并修复的主要问题：

1. **rssn 项图不带单位。** 静态单位在编译期折叠；`Real`/`Float` 值在宿主
   侧带显示单位；rssn 只看到无量纲数值/符号。避免了 `quantity` 无 eval
   和 `L`→升、`T`→特斯拉的文本歧义。
2. **量纲文本只用全名。** 桥接层使用 `Length`/`Mass`/`Time`/`Current`/
   `Temperature`/`Amount`/`Luminosity`，禁止单字母。
3. **数值答案的量纲取 `Answer.dimension`**，不从 `Answer.term` 重算。
4. **`compute` 克隆成本/`DimArena` 共享** 列入后端缺口；建议 `OpTable`
   用 `Arc`、新增 `prepare(config)`、基量纲只在模板期注册。
5. **`Term` 缺遍历/替换/自由符号 API** 列入缺口；`diff` 闭包需要
   `substitute` 或直接使用 `graph::Graph`。
6. **JIT 收缩承诺。** 当时只支持无分支的 `Float` 表达式；`if`/张量索引
   由解释器执行。
7. **`assuming` 收缩为单元谓词。** `x > 0`、`x >= 0`、`x != 0`、`x ∈ ℤ`；
   一般等式/不等式报 `E0804`。
8. **`Solutions` 后端降级。** 当时只保证 `roots`；`multiplicities` 填 1，
   `conditions` 为空，文档标注。
9. **ODE 方法/事件。** `OdeOpts` 增加 `steps`（Rk4）；events 只允许
   Dopri5；`RateOf` 递归定义标量/记录/张量。
10. **`par_map` 要求 `Parallel`。** 符号 `Real` 不并行，避免跨会话
    `TermId` 静默错误。
11. **一致性修复。** 删除残留 `OdeState`；`solve` 线性版本统一为
    `linsolve`；`deriv` 改名；`forall` 接受 `Prop`；删除 `E0703`；
    修正交叉引用/警告码；补 `E0316/E0317`、`E0406`、`DimValue` 等。
12. **标准库签名统一为元组多参。** 修复 `min/max`、`column`、`odeint`、
    `grad` 等柯里化写法；`mean/std/var` 统一 `div_rep(ρ)`。

## Revision 3 — 最终一致性

第三轮聚焦“修复是否真的闭合”，又关闭了以下问题：

1. **`Dyn` 的类型指导语义。** `dyn`/`check` 要求具体量纲，编译器把 `d`
   物化为 `DimValue`；泛型用 `dyn_with`/`check_with`。解决“类型擦除后
   运行时拿不到量纲”的矛盾。
2. **`by_cases` 限制为 `Real[d]`。** 避免 `Int`/`Bool` 分支无法构造符号
   `ite` 项。
3. **`Point + 温差` 只接受 `Float[Θ]`。** 精确温差必须先 `to_float`；
   `25 °C + 10 K` 报 `E0302`，提示写 `10.0 K`。
4. **`Show` 实例显式量纲多态。** `Show (Float[d])`、`Show (Tensor<n,a>)`、
   `Show (Solutions<ρ,d>)`、`Show (Point[d])`；否则 `20.0 m` 无法打印。
5. **绘图默认参数。** `line`/`scatter`/`hist` 使用默认 `PlotOpts`；
   `line_with` 接受完整记录；`label` 设置标签。示例不再依赖不完整的
   记录字面量。
6. **ODE 展平契约。** 字段声明顺序、深度优先、行主序；`Tensor<1,s>` 用
   `Boxed`；桥接层负责扁平 `f64` 与记录之间的转换。
7. **`Real` 的 `!=`。** 增加 `Prop` 版本；`assuming(x != 0, ...)` 可用。
8. **`Answer.dimension` 的 `Option`。** `None` 时回退静态量纲；数值路径
   用 `Advisory`/`Strict`。
9. **删除陈旧的 `quantity` 构造语句和 `L/T` 文本。** 静态项图无量纲；
   全名只用于 FFI/导出。
10. **`Float` 的 `sqrt` 永远走 f64**；删除“Float 保持符号”的错误描述。
11. **`inv` 统一返回 `Result`**；`Map` 不再声称依赖 `Hash`；`Solutions.
    conditions` 改为 `Tensor<1, String>`；`read_csv` 统一 `DataError`。
12. **JIT 触发条件去掉 `if`/索引**；与后端缺口一致。

## Revision 4 — 可扩展性与 RSSN 集成

这一轮不再把问题留到“v2”，而是把长期可维护性写进语言契约：

1. **反射优先的算子集成。** Aevia 不维护算子绑定表；读取 RSSN Kernel
   Manifest，生成 `Rssn` 模块。有签名 → 类型化；无签名 → `Rssn.dynamic`。
   1000+ 算子从第一天全部可见；新算子不需要改 Aevia 编译器。
2. **动态 `Value`/`call` ABI。** `Rssn.call`/`Rssn.dynamic` 是正式能力，
   不是降级路径；`OpError` 结构化，运行时检查类型/量纲。
3. **签名/重载/稳定键。** `OpKey { rule_set, name, signature_hash }`；
   `Signature`、`overloads`、`Stability`、`Since`、`Doc` 进入 manifest；
   CI 审计“每个算子有签名或 dynamic_reason”。
4. **插件/宿主回调。** `#[aevia::op]` 把 Aevia 函数注册为 RSSN 算子；
   插件可注册算子/重写/单位/量纲/JIT lowering。
5. **量纲/单位统一。** 抽出 `rssn-dim`：`DimTerm`/`DimArena`/`UnitRegistry`；
   精确 `Scale { rat, pi }`、仿射 offset、用户单位/基量纲注册；Aevia 的
   `Units.SI` 是 registry 的生成视图，不再有第二张表。
6. **中心 registry。** 内容寻址 + 签名 + capability；包清单/索引/发布/
   yank 协议现在就定。
7. **形状细化。** `Tensor<[3,4],τ>` 作为 refinement；类型级 `Nat` 只做
   线性算术；`Matrix` 是别名；细化 → 秩-only 安全隐式转换。
8. **`Point` 张量。** `Tensor<shape, Point[d]>` 支持元素级点算术；
   温度场不再用“K 值 + 约定”。
9. **`Table<ρ>` 行多态 schema。** `read_csv<ρ>`、`column` 类型级字段查找；
   `Table<Dynamic>` 保留给未知 schema。
10. **设备/GPU/分布式。** `Device` 是运行时值；`on`/`to_device`/
    `@jit(target)`；不是张量类型参数，避免类型组合爆炸。
11. **稀疏。** `Sparse<F,shape,τ>` 核心类型，`Layout` 类；`@` 按布局分派。
12. **效果/宏的边界。** 不做用户 effect handlers/宏；扩展点是算子/插件/
    registry；`IO` 保持单一类型，capability 是运行时权限。
13. **稳定性模型取代 v1/v2。** `@stable`/`@experimental`/`@deprecated`；
    后端缺失报 `E1001`，不删语言特性。

### 仍存在的风险（设计边界，不是版本待办）

- `Term` 的替换/自由符号 API 仍需 rssn 侧补齐；`diff` 闭包可能每次重跑
  `compute`。
- `compute` 的深拷贝和 `DimArena` 共享需要 rssn 侧优化；可用批处理规避。
- `Solutions` 的 `multiplicities`/`conditions` 在 RSSN 结构化结果就绪前
  可能只填默认值。
- `Point` 的表示是 Float + UnitId；符号绝对温度仍用温差/`Dyn`。
- 形式化语义/证明尚未做；用 conformance/属性测试/与 rssn 交叉检查。
- GPU/分布式/稀疏的**后端实现**可能晚于语言设计；缺失时报 capability，
  不删语法。


---

## 待办清单（审计后关闭）

- [x] 统一 `dim` / `dimension` 关键字。
- [x] 统一 `E0202` / `E0309` 非穷尽匹配错误码。
- [x] 修正 `forall` 测试语法。
- [x] 补 `point` 的签名。
- [x] 修正 `Tensor<1, Real[e>]` 等笔误。
- [x] 统一 `reshape<m>` 写法。
- [x] 检查所有示例的类型/量纲推导。
- [x] 检查单位表与 rssn 的一致性说明。
- [x] 检查 `Point` 与 `Float`/`Real` 的交互。
- [x] 检查 `RateOf` 的递归规则。
