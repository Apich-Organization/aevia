# Aevia Language Specification

**Single-file edition — Revision 10**

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

# Aevia Language Specification

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
RSSN **Kernel Manifest** and `rssn::dim` registry, and generates its `Rssn`
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
audit round. The current text is **Revision 10**, after six rounds of independent
audits plus the extensibility/backend-integration redesign, the live-kernel
alignment with **`rssn 3.0.0-rc.1`** (now **1381 operators, 32 rule sets, 24
overlays, 82 units**, with `features.sde`; see appendix C), and the second
implementation round: symbolic instance keys and module-exported classes/
instances/aliases, the §8.20 standard-library module tree (37 modules with the
loader fallback), typed SDE surface (§8.9.1) over the kernel's §SDE operators,
type-level `Nat` linear unification, and the tooling completion (doctests, bench
baselines, multi-module editor analysis). Revision 9 states the dispatch/extension
boundary explicitly (no runtime multimethods, no multi-parameter classes, no
per-type monomorphization; instances selected statically or by canonical
witnesses) and Revision 10 expands that into the implemented instance-key model.
There is **no v1/v2 language split**: every capability is designed in; a backend
that does not yet implement one reports a capability diagnostic (`E1001`) instead
of removing the language feature.

## A one-sentence summary

> Aevia makes the dimension of a quantity a type, the unit a compile-time scale,
> the formula a first-class value, and the numeric result a deliberate request.

## Quick taste

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
import Math { sqrt }

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
的人想要精确值。整数默认是 `Int`。`Real` 与 `Float` 混合运算时 `Real` 吸收 `Float`（精确优先）
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
唯一算子注册表，`rssn::dim` 是唯一量纲/单位层；Aevia 读取 Kernel Manifest，
生成 `Rssn` 模块（类型化 + `dynamic`）。新算子/新单位/新插件不需要改
Aevia 编译器。这样 1000+ 算子可以持续增长，而不是靠“下个版本支持”追赶。

## 0.5 十分钟导览

### 量纲与单位

```aevia
let d = 100 m                 # Real[L]
let t = 9.58 s                # Real[T]
let v = d / t                 # Real[L/T]
let E = 70 kg * v^2 / 2       # Real[M*L^2/T^2]

let bad = d + t               # error[E0401]: dimension mismatch
```

`70 kg` 是 `Real[M]`，`v^2` 是 `Real[L^2/T^2]`，乘积是 `Real[M*L^2/T^2]`。
不需要写 `Energy`；`Energy` 只是 `M*L^2/T^2` 的打印别名。

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
let M = [[1.0, 2.0], [3.0, 4.0]]      # Tensor<[2,2], Float>
let b = [5.0, 6.0]                    # Tensor<[2], Float>
let x = match linsolve(M, b) with
        | Ok(v) -> v
        | Err(_) -> [0.0, 0.0]        # 示例；实际应处理错误
let y = M @ x                         # matmul，等于 b

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
- 单位表达式内的 `*`/`·`/`/` 一直属于单位表达式，直到遇到数字或表达式边界。
  因此**要把一个量乘以变量或函数调用，必须先用括号结束单位字面量**：
  `(3 m) * x`、`(3.0 m/s) * exp(-t)`；写成 `3 m * x` 时 `m * x` 整体是单位
  表达式，`x` 不是单位就报 `E0501`。
- 单位表达式内不允许数字，因此 `3 m/2 s` 中单位部分只取 `m`，整个表达式
  解析为 `(3 m) / (2 s)`。
- `1/2 m` 解析为 `1 / (2 m)`，类型是 `Real[1/L]`；要写“半米”，写
  `(1/2) m`。格式化器 SHOULD 在这种模式上加括号。
- 括号只用于单位表达式：`3 (m/s)` 合法；`3 (2)` 不合法，报 `E0108`
  （数字后跟括号时括号内必须是单位表达式）；括号内单位写坏（`3 ()`）报
  `E0209`。

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

| 记号 | 含义 | 结合性/优先级（见 §2.6） |
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

`private` 可以加在 `type`、`dimension`、`unit`、`fn`、`let`、`class` 前，表示不导出。
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
type_decl ::= "type" UIDENT [ type_params ] [ "=" type_body ]
type_body ::= type
            | "|" constructor { "|" constructor }
            | NEWLINE INDENT { "|" constructor NEWLINE } DEDENT
constructor ::= UIDENT [ "(" [ type { "," type } ] ")" ]
```

- `type Name = T` 是**透明别名**：`Vec2` 与 `{ x: Real[L], y: Real[L] }`
  完全相同。
- `type Name`（省略 `= type_body`）是**抽象类型**，只能由标准库/插件提供
  构造器与操作；用户模块不能凭空实现它。`Table`/`Series`/`PlotSpec` 等
  内建类型用这种形式声明。
- `type Name = | C1(...) | C2(...)` 是**名义变体类型**；构造器是值/函数。
  单行变体 MUST 以 `|` 开头，以便与类型别名区分；缩进形式可以省略首个
  `|`（但建议保留）。
- 递归类型允许，但递归出现必须位于函数箭头的右侧、记录字段、元组或构造器
  参数中（正性检查，§3.6.4）。
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
attribute ::= "@" IDENT [ "(" [ attr_arg { "," attr_arg } ] ")" ] NEWLINE?
top_decl  ::= { attribute } [ "private" ] decl
attr_arg  ::= [ IDENT "=" ] expr
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
match_expr ::= "match" expr "with" match_body
match_body ::= NEWLINE INDENT { match_arm } DEDENT
             | NEWLINE { match_arm }          # 分支与 match 同列
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
- 只有裸运算符 `(+)` 或 lambda 是函数；部分应用 section（如 `(== 1)`、
  `(* 2)`）报 `E0205`，请写 `fn y => y == 1`。

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
`Option<a>` 的多态值。构造器名必须以大写 `UIDENT` 开头；这是语法要求。

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
        | [ expr ] ":" [ expr [ ":" [ expr ] ] ]
```

- 索引从 0 开始；负索引从末尾计。
- `..` 是半开区间，`..=` 是闭区间；省略端点表示到边界。
- `:` 是带步长切片的写法：`a[i:j:s]`。`i`、`j`、`s` 都可省略，省略的 `s`
  表示 1；`a[i:j]` 与 `a[i..j]` 是同一个节点。
- `::` 是 cons 运算符（§2.5），所以在表达式之后 `xs[1::2]` 仍解析为
  `xs[1 :: 2]`。带起始位置和步长的切片要写出终点（`xs[1:5:2]`）；起点之后
  立即省略终点只在没有起始位置时写作 `xs[::2]`。
- 在 `[...]` 内 `:` 是切片分隔符，所以带类型标注的下标要加括号：
  `xs[(i : Int)]`。
- `a[i]` 在 rank 1 上取元素，在 rank > 1 上取第 i 个切片（rank 减一）。
- `a[i, j]` 要求 rank 2；更多下标依此类推。
- 越界是运行时错误 `E0802`；编译器在索引为字面量时可以静态检查。
- 切片产生新张量（语义上是复制；运行时在可能时共享底层缓冲区）。负步长
  切片总是视图（§6.6.4.1）。

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
fn load(path: String): IO (Result<Table<Dynamic>, DataError>) =
    read_csv_dyn(path)
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
fn_type ::= type_postfix [ "->" type ]
type_postfix ::= type_atom { "[" IDENT "]" }
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
            | IDENT
            | "(" type ")"
field_type ::= IDENT ":" type
qname ::= UIDENT { "." UIDENT }
format_name ::= IDENT | UIDENT
shape ::= nat | "[" [ shape_item { "," shape_item } ] "]"
shape_item ::= nat | "?"
row_type ::= type { "+" type }
```

注意：类型级乘积 `A * B` **不**表示元组；元组用 `(A, B)`。`*` 只出现在
量纲表达式里。

- `Real` 不带方括号等价于 `Real[1]`；`Float`、`Complex` 同理。
- `Tensor<2, Float>` 的 `2` 是秩（rank shape）；`Tensor<[3,4], Float>` 是细化形状。
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

相邻的 `<...>` 只在 callee 是函数名（或其限定名）时是类型应用；
`3<Int>`、`f()<Int>` 这类歧义报 `E0207`，提示在 `<` 前加空格或给 callee
加括号。

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
| `a[i:j:s]` | `slice_step(a, i, j, s)` |

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

Format ::= Csr | Csc | Coo | Block | Bcsr
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
Tensor        : Shape -> Type -> Type       张量（秩或细化形状）
Matrix        : Nat -> Nat -> Type -> Type  = Tensor<[n,m], τ>
Sparse        : Format -> Shape -> Type -> Type  稀疏张量（§13.9.5）
Point         : Dim -> Type                  仿射点（Float 表示；可作张量元素）
Unit          : Dim -> Type                  单位值
Table         : Type -> Type                 行多态表；ρ 是封闭记录类型（§13.9.3）
Dynamic       : Type                        特殊记录类型：未知 schema（仅用于 Table<Dynamic>）
Device        : Type                         运行时设备（§13.9.4）
Set           : Type -> Type
Option        : Type -> Type
Result        : Type -> Type -> Type
IO            : Type -> Type
(->)          : Type -> Type -> Type         多参数函数是元组的糖
SymFun        : Dim -> Dim -> Type           符号函数（用于 dsolve）
Dyn           : Rep -> Type                  动态量纲标量（§4.9）
RateOf        : Type -> Type                  ODE 状态的时间导数类型（§8.9）
DiffusionOf   : Type -> Type                  SDE 状态的扩散系数类型（§8.9.1）
Solutions     : Rep -> Dim -> Type            符号解集合（§8.6）
```

`DiffusionOf<s>` 与 `RateOf<s>` 同构，但每个叶子的量纲除以 `T^(1/2)`：
布朗增量 `dW` 的量纲是 `T^(1/2)`，所以 `dX = a dt + G dW` 中 `G` 的每个
分量比 `X` 多一个 `T^(-1/2)`。

```
DiffusionOf<Scalar<ρ,d>>             = Scalar<ρ, d/T^(1/2)>
DiffusionOf<Tensor<n, τ>>            = Tensor<n, DiffusionOf<τ>>
DiffusionOf<{ l1: τ1, ..., ln: τn }> = { l1: DiffusionOf<τ1>, ..., ln: DiffusionOf<τn> }
```

若 `s = { x: Float[L] }`，则 `DiffusionOf<s> = { x: Float[L/T^(1/2)] }`；
标量状态 `Float[1]` 的 `DiffusionOf` 是 `Float[1/T^(1/2)]`。含
`Int`/`Bool`/函数字段的状态不合法，报 `E0302`（与 `RateOf` 相同）。这是
**对角**扩散（每个状态分量一个噪声通道）的类型；一般 `n × m` 噪声用
§8.9.1 的 `sdeint_general`，其扩散参数类型是
`Tensor<2, Scalar<ρ, d/T^(1/2)>>`。

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
PluginId   : Type                  # 注册后的插件 id（§13.5）
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

`∀` 量化类型变量（包括表示变量）、量纲变量、自然数变量和行变量。
`C` 包括类约束（如 `Show a`）以及数值约束：字面量可取的表示域、
`Join(ρ1, ρ2, ρout)`、`Divide(ρin, ρout)` 和依赖输入的私有字面量读取规则。
这些约束随方案、函数别名和模块接口保存；用户无需手写，编译器会重建。

实例化会同时重命名约束中的变量。运算产生的中间表示也需要独立实例，
但它们是计算结果，不会成为额外的可显式选择的 `<...>` 参数。

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

**`Symbol` 与 `Real[δ]` 的关系。** RSSN 的 `TypeExpr` 有一个独立的
`Symbol` 变体（自由符号，区别于 `String`），而 Aevia 把 `symbol("x")` 的
静态类型写成 `Real[δ]`（`δ` 弱）。两者是同一运行时值的两种视图：

- Aevia 的 `Symbol` **就是** `Real[δ]`，`δ` 是该符号的弱量纲变量；不新增
  第五种 `Symbol` 值类型；
- 边界上 `Value::Symbol` 对应 `TypeExpr::Symbol`；Aevia 生成 façade 时把
  RSSN 签名里的 `Symbol` 参数读作 `Real[δ]`（新鲜弱变量），把
  `Real[d]` 参数读作 `Dim(Real, d)`；
- 因此 `diff` 的 overlay 签名 `(Term, Symbol) -> Term` 在 Aevia 里读作
  `(Dyn<term>, Real[δ]) -> Dyn<term>`，弱变量在调用点统一。

`diff`/`solve` 现在正是这样声明的（`signatures.overlay.json` 的 13 条之一），
所以它们有精确的 `Symbol` 参数；`Symbol` 不再是只能写成 `Var("Symbol")`
的模糊类型。没有 overlay 的算子仍由 `Derived` 给出保守签名（`(Term,…) ->
Term`），那也足以生成 typed 声明。

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

未解决的表示保留有限的算术关系，而不是把结果直接等同于某个输入：
`Join(ρ1, ρ2, ρout)` 按上述四种具体表示的表约束三者，
`Divide(ρin, ρout)` 则约束整数除法的提升。普通偏序下界不能替代这些关系。

```aevia
fn half<r: Rep>(x: r) = x / 2
let exact = half<Int>(3)       # Real，精确的 3/2
let floating = half<Float>(3.0) # Float
```

`half` 只有输入 `r` 这个显式参数；调用者不能另选一个与除法规则冲突的结果表示。
关系在检查期和实际闭包的运行时泛型证明中都保留。

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

- `Tensor<n, τ>` 的 `n` 是秩；形状是运行时值 `[d1, ..., dn]`。它是
  `Tensor<[?, ..., ?], τ>`（n 个 `?`）的糖：`n` 是秩，extents 未知。秩错误是编译错误，形状
  错误在细化类型下是编译错误，否则是运行时 `E0802`。
- `Tensor<[n1, ..., nk], τ>` 的 `n_i` 是类型级 `Nat`，可用 `?` 表示未知。
  这是 `Tensor` 的**特殊类型形式**（`Tensor<shape, τ>`），不是把 shape
  列表当作普通类型参数；字面量自动获得细化形状，`zeros_of<[3,4]>()` 等构造器
  可显式指定。
- 类型级 `Nat` 支持字面量、`+` 和「乘以字面量」（`2 * n`）；不做一般
  递归/证明。形状 extent 中的 `Nat` 变量由 `<n: Nat>`（或未标 kind 的类型
  参数）绑定，是真正的类型级变量。统一时把等式化为线性形式
  `Σcᵢ·vᵢ + c = 0`：只有一个变量时解出非负整数（`n + 1 ~ 3` ⇒ `n = 2`，
  `2·n ~ 6` ⇒ `n = 3`，`n ~ 2·n` ⇒ `n = 0`）；多变量等式保留为待解约束，
  在后续绑定后重试；常数残差非零报 `E0302`（`type-level equation has no
  solution`）；两个变量相乘不是线性的，报 `E0304`。秩 `Tensor<n, τ>` 的
  `n` 仍须是具体整数（否则 `E0304`），refined shape 的 extent 可以是变量。
  没有 binder 的 shape 变量、以及实例头中的 Nat 变量都报 `E0304`。
  这足以表达矩阵乘法、reshape、拼接，不需要完整依赖类型。
- 细化 → 秩-only 是安全隐式转换；秩-only → 细化用
  `refine_shape(a) : Option<Tensor<[...],τ>>`（运行时检查）。
- `reshape<[n1, ..., nk]>(a)` 的签名始终返回
  `Result<Tensor<shape,τ>, ShapeError>`；形状静态已知时元素数在编译期
  检查、结果恒为 `Ok`，否则运行时检查。`reshape_dyn` 总是运行时检查。
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
Tensor<shape, Scalar<ρ,d>> * Unit[e]  : Tensor<shape, Scalar<ρ,d*e>>
Tensor<shape, Scalar<ρ,d>> / Unit[e]  : Tensor<shape, Scalar<ρ,d/e>>
Unit[e] * Tensor<shape, Scalar<ρ,d>>   : Tensor<shape, Scalar<ρ,e*d>>
```

**张量与张量的逐元素运算要求相同秩**。同秩内形状可以相等或某一维为 1
（广播）。细化类型在编译期检查；秩-only 在运行时检查。不做 NumPy 式跨秩
广播；这条限制让结果秩始终静态已知，也避免 `(3,1) + (1,4)` 这类静默错误。

同秩的 refined shape 与 rank-only shape 在**运算**中互相统一
（`Tensor<[2], V>` 与 `Tensor<1>, A>` 可以同时出现在 `*`/`+`/`@` 中，
refined → rank-only 是安全隐式转换）。但把 rank-only 值当作 refined
**注解**（`let x: Tensor<[3]> = zeros(4)`）仍需 `refine_shape` 的运行时
检查；直接标注是 `E0302`。

`Point` 元素张量 `Tensor<shape, Point[d]>` 的元素级规则：

```
Point - Point : Float[d]
Point + Float[d] : Point
Point + Point : E0405
centroid : Tensor<1, Point[d]> -> Point[d]
```

`mean` 只接受数值张量；点的平均用 `centroid`。

### 3.5.3.1 `@` 的秩分派

`@` 不是单一函数，而是按秩分派的矩阵乘法：

| 左 | 右 | 结果 | 等价函数 |
|---|---|---|---|
| `Tensor<2, Scalar<ρ,d>>` | `Tensor<2, Scalar<ρ,e>>` | `Tensor<2, Scalar<ρ,d*e>>` | `matmul` |
| `Tensor<2, Scalar<ρ,d>>` | `Tensor<1, Scalar<ρ,e>>` | `Tensor<1, Scalar<ρ,d*e>>` | `matvec` |
| `Tensor<1, Scalar<ρ,d>>` | `Tensor<2, Scalar<ρ,e>>` | `Tensor<1, Scalar<ρ,d*e>>` | `vecmat` |
| `Tensor<1, Scalar<ρ,d>>` | `Tensor<1, Scalar<ρ,e>>` | `Scalar<ρ,d*e>` | `dot` |
| `Tensor<[m,k], Scalar<ρ,d>>` | `Tensor<[k,n], Scalar<ρ,e>>` | `Tensor<[m,n], Scalar<ρ,d*e>>` | `matmul`（细化） |
| `Tensor<[m,k], Scalar<ρ,d>>` | `Tensor<[k], Scalar<ρ,e>>` | `Tensor<[m], Scalar<ρ,d*e>>` | `matvec`（细化） |
| `Tensor<[k], Scalar<ρ,d>>` | `Tensor<[k,n], Scalar<ρ,e>>` | `Tensor<[n], Scalar<ρ,d*e>>` | `vecmat`（细化） |

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
  `-n_i <= i < n_i`；动态索引仍在运行时检查（`E0802`）。
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

- 类**只有一个**类型参数；多参数类不在语言中（实例头语法无法表达多参数实例，
  求解器也只支持单参数）。`class Convert a b where ...` 报 `E0317`。
- 方法只有签名；默认实现由 `derive`、内置结构实现（Prelude 类）或插件提供。
- 实例头 `Class τ`，`τ` 是类型构造器应用；`Class a` 这样的裸类型变量头非法
  （`E0317`）。`τ` 中的变量隐式全称量化。
- 用户类：实例必须覆盖所有方法，缺失的方法在 `E0317` 中列出。
- Prelude 类（§3.8.4）是**钩子**而非封闭实现：实例可以只覆盖一部分方法，
  其余由内置结构实现提供（例如记录/元组的打印由内置打印器完成）。
- 类与实例的身份是**符号化**的：类键 `(定义模块路径, 类名)`，实例键再加规范
  头类型；`AdtId` 只是进程内见证，不进入模块接口或缓存。`private class` 只在
  定义模块可见；`private instance` 不允许（实例随模块导出，§7.4）。

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
- `Real` 不是 `Eq`：`x == y` 对 `Real` 返回 `Prop`（§3.8.6），不是 `Bool`。
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

`Show` 对上表的头是**结构性保证**，不是可选优化：没有用户实例时，内置
打印器直接渲染记录、元组、`Tensor`、`Point`、`Solutions`、`Dyn` 与单位值；
用户为**自己声明**的名义类型写的实例（或 `derive Show`）优先于内置实现。
函数没有 `Show` 实例，也没有结构性回退（`E0310`）。

内置类的实例集参与全局一致性（§7.4）。两个实例必须能由**运行时见证**区分
（§9.2.3）：见证相同即使用户写了两个静态上不同的头，也报 `E0311`。例如
`Tensor<1, Int>` 与 `Tensor<1, Real>` 都是精确存储，不能同时拥有同一类的实例。

### 3.8.5 派生

```
derive Eq, Show, Hash for Vec2
```

`derive` 只对**结构类**开放：`Eq`、`Ord`、`TotalOrd`、`Show`、`Hash`。派生
其他类（用户类、`Num` 等）报 `E0317`，消息指出失败的类。派生实例在**类型
所属模块**生成（孤儿规则同样适用，§7.4）。

编译器为记录/变体生成结构化实例。派生 `Ord` 按字段声明顺序做字典序。
派生不适用于函数字段（没有 `Eq`/`Show`/`Hash` 实例）；失败的字段类型与类名
出现在 `E0317` 中，而不是笼统的“不能派生”。

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
6. `Nat` 变量**不**默认成任何值；无法由约束解出时在检查期报 `E0304`
   （秩或 refined extent 无法推断）。
7. 行变量 → 空行（封闭记录）。

默认化只影响未被方案或词法环境量化的变量，并先传播所有数值约束。
`Literal` 约束选择合法的默认类型，不能把小数字面量读成 `Int`。

函数体的私有字面量若依赖泛型输入，会在这些输入确定后，优先按同一数值
约束分量中输入的算术 `join` 读取，再尝试其字面量默认表示。无关参数不参与；
调用者的期望结果也不能反过来另选这个读取。直接使用泛型参数的字面量
（如 `fn zero<r: Rep>(): r = 0`）保留自己的合法表示域。

实际构造并捕获的数据保持同一类型实例。函数别名传递构造要求，不能通过
对保存的闭包分别实例化而把同一个捕获数值同时解释成 `Int` 和 `Float`。

### 3.10.3 类型标注优先

如果表达式有期望类型，字面量按期望类型检查，不触发默认化：

```aevia
let x: Float[L] = 0.0 m    # 零长度，Float
let n: Int = 0             # 0 是 Int
let y = 0                  # 默认 Int
let z: Real[L] = 0 m       # 零长度，精确
```

仅含字面量的 `+`、`-`、`*`、`/` 表达式及正负号也接受这个局部上下文：
`let exact: Real = 0.1 + 2` 精确等于 `21/10`。这个规则不越过变量或调用；
已经建立的函数数值关系仍决定调用结果，例如 `fn f(x) = x + 1.0` 的
`f(1)` 是 `Float`，不能由调用者标注成 `Int`。

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

| 规范名 | 源码/显示别名 | 含义 |
|---|---|---|
| `Length` | `L` | 长度 |
| `Mass` | `M` | 质量 |
| `Time` | `T` | 时间 |
| `Current` | `I` | 电流 |
| `Temperature` | `Θ` | 热力学温度 |
| `Amount` | `Amt` | 物质的量 |
| `Luminosity` | `Lum` | 发光强度 |

- 基量纲是**生成元**，不是类型构造器；`L` 本身是一个量纲表达式。
- 用户可以用 `dimension Currency` 声明新的基量纲。源码声明的身份由定义模块的
  canonical package/source owner、模块路径和声明名共同确定；它不是全局裸名。
  两个包各自声明的 `Currency` 是不同生成元；同一声明经不同 import alias 引入仍相等。
- 基量纲名大小写敏感；**全名是规范名**（`Length`/`Mass`/`Time`/`Current`/
  `Temperature`/`Amount`/`Luminosity`），单字母只是源码/显示别名。跨 FFI、
  manifest 和 rssn::dim 一律用全名；单字母不得出现在边界。
- `1` 表示无量纲（空乘积）。

源码基量纲与宿主量纲分开：SI 七基继续使用既有规范名；RSSN/宿主的普通
`DimExpr::Base("Currency")` 仍表示宿主命名生成元，不会自动等同于任一源码
`dimension Currency`。源码名义生成元穿过桥接时使用带版本、包含完整 owner 与
声明名的规范编码；保留前缀下的普通宿主名字会转义，避免与名义编码碰撞。
此表示仍使用既有 RSSN `Base` 字符串载体，不改变 SI 的签名 hash 或二进制布局。
生成外观模块不能按显示名重声明外部 owner：此类算子签名使用动态路径；外部名义
量纲的单位和常量标注为不能生成并跳过，使用者须从其定义模块导入。

`A.Currency`、`Again.Currency` 是显示/查找提示，不参与生成元的相等、排序、hash
或 ADT 声明 schema。导入接口传递已解析的量纲，不能按导入别名重新创建生成元。
泛型 `d: Dim` 的实例化只替换量纲变量，保留所有名义生成元及其指数。

`private dimension` 只在定义模块可见。公共派生别名、函数签名和类型别名可以引用
私有量纲；接口保存其已解析语义，不向使用者开放私有名字，也不要求使用者重新
解析定义模块的私有上下文。导入本身不构成传递导出。

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
dimension MolarMass     = M/Amt
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
`graph::dim` 支持**单个**符号指数（`L^n`），因为它在项图上做推断；Aevia
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
DimValue       : Type        # = rssn::dim::DimTerm（基量纲 + 有理指数）
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

当前实现的 `DimValue` 保存可无损还原的规范量纲文本；自定义宿主基、名义 owner
和变量均有显式编码，SI 文本保持既有形式。`dim_of` → `dyn_with`/`check_with`
不经由显示名恢复身份。诊断和数值后缀使用可读别名；不能将这些显示文本当作
跨模块量纲身份证据。

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

0.2.13 起 rssn 的 `DimExpr::to_term` 也能把 `Length^n` intern 成原子，
所以 `Dyn` 逃逸路径不再在桥接层报错。这不改变语言决定：静态类型位置仍然
只接受有理指数，符号指数只能出现在 `Dyn<ρ>` 的运行时量纲里。

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
# 生成器为每个 dimension 在 Config 上声明一次 base_dimension(name)
dimension Currency                  # 自定义基量纲
unit USD : Currency = 1             # 基单位
unit EUR : Currency = 1.08 USD      # 派生单位

unit m : Length = 1 prefixable      # 基单位，允许 SI 前缀
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
  标准库；`Units.*` 是 `manifest.units` 的生成视图。

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

标准库 `Units.SI` 提供以下单位（节选）。

**单位名与量纲名是两个命名空间**，同一个拼写可以两边各有一个含义；解析器
按位置区分，不需要用户加限定：

| 拼写 | 单位位 | 量纲位 |
|---|---|---|
| `L` | 升（`liter`/`litre` 的别名，`prefixable`） | `Length` 的显示别名 |
| `T` | 特斯拉 | `Time` 的显示别名 |
| `G` | 高斯（`1e-4 T`） | 不是引力常数 |
| `h` | 小时 | 不是普朗克常数 |

`manifest.units` 里 `L` 与 `l` 同时存在（都是升），`L` 带
`aliases = ["liter", "litre"]`。规范名一律用全名
（`Length`/`Mass`/`Time`/...），单字母只用于源码/显示。

### 5.3.1 SI 基单位

| 单位 | 量纲 | 说明 |
|---|---|---|
| `m` | `L` | 米，prefixable |
| `kg` | `M` | 千克（基单位；`g` prefixable） |
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
| `week` | `T` | `604800 s` |
| `yr` | `T` | `31557600 s` |
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
| `bar` | `Pressure` | `100000 Pa` |
| `atm` | `Pressure` | `101325 Pa` |
| `mmHg` | `Pressure` | `133.322387415 Pa` |
| `cal` | `Energy` | `4.184 J` |
| `Wh` | `Energy` | `3600 J`（`kWh` 是 `k` 前缀形式） |
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
| `lbf` | `Force` | `4.4482216152605 N` |
| `psi` | `Pressure` | `6894.757293168 Pa` |
| `°C` | `Θ` | 仿射：`1 K offset 273.15 K` |
| `°F` | `Θ` | 仿射：`5/9 K offset 255.3722222222222 K` |
| `bit` | `1` | 信息，尺度 1 |
| `B` | `1` | `8 bit` |
| `KiB` | `1` | `1024 B` |

信息单位是否应该是无量纲？当前规范是；如果需要独立的 `Information` 基量纲，
用户可以 `dimension Information`。标准库不强制，因为大量公式把 bit 当
无量纲概率/熵使用。

> **rssn 3.0.0-rc.1**：`dimension Information` 与
> `unit bit2 : Information` 都可用，前提是在 `Config` 上声明
> `base_dimension("Information")`（`prepare` 在克隆图上定义它）。`Dim`
> 的内部表示是“7 个 SI 基 + 有序旁表”，`expr_to_dim` 接受任意基名。

### 5.3.4 单位模块

| 模块 | 内容 |
|---|---|
| `Units.SI` | 基单位、派生单位、前缀、常用非 SI |
| `Units.Imperial` | 英制长度/质量/体积/压强/温度 |
| `Units.Astronomy` | au、pc、ly、太阳/地球/木星质量、太阳半径（Mpc 由 `pc` 加前缀） |
| `Units.CGS` | cm、g、dyn、erg、Gal、P、St、G、Mx、Ba、Oe |
| `Units.Natural` | `c = 1`、`hbar = 1`、`eV` 系 |
| `Units.Constants` | 物理常数（§5.8） |

> **实现状态（按 rssn 3.0.0-rc.1 测量，2026-11-04）**：`manifest.units` 有
> **82** 项，覆盖 SI 基/派生、常见英制、`degC`/`degF`、角度、时间
> （`min`/`h`/`d`/`week`/`yr`）、能量/功率（`Wh`/`cal`/`eV`/`hp`）、信息
> 单位（`bit`/`B`/`KiB`），以及：
>
> - **天文**：`au`（精确）、`ly`、`pc`、`Rsun`、`Msun`、`Mearth`、`jup`；
> - **CGS**：`dyn`、`erg`、`Gal`、`P`、`St`、`G`（高斯，`1/10000 T`）、
>   `Mx`、`Ba`、`Oe`（scale 带 `pi^-1`）；
> - **§5.3.3 本轮补齐**：`Da`（别名 `u`）、`kn`、`mph`、`acre`、`bit`、
>   `B`、`KiB`——本轮之前 kernel 的注册表缺这七项。
>
> 因此 `Units.Astronomy` 与 `Units.CGS` 可以从 manifest 生成。`Mpc` 由
> `pc` 加前缀得到（`pc` 是 `prefixable`）；`kWh` 由 `k`+`Wh` 得到。
>
> `Units.Constants` 也从 `manifest.constants` 生成（30 条、18 条精确），
> 不再需要 Aevia 本地 overlay——单一事实来源没有例外。
>
> 每个 `UnitValue` 都带 `aliases`/`prefixable`/`category`/`since`，所以生成
> 单位表不需要再读 `UnitRegistry`。单位是 **session 私有**的：
> `Session::register_unit` 注册的单位只出现在该 session 的 manifest 里
> （`KernelManifest::build_with_units`），进程级表由 `build` 读取。

`Units.Natural` 的设计是“单位制”而不是“改常数”：它本应导出一组新的单位，
其中 `c`、`hbar` 的数值是 1，用户 `open Units.Natural` 后 `1 GeV` 等写法成立。
**当前实现状态**：kernel 没有 natural-unit registry，Aevia 也不允许在 manifest
之外维护第二张单位表（§5.10 第 1 条），所以这一构建里的 `Units.Natural`
是一个**文档性空模块**（`import Units.Natural` 能解析，但不导出单位）；`c`、
`hbar` 作为**物理常数的值**来自 `Units.Constants`，`GeV` 由 `G`+`eV` 前缀
得到。真正的 natural-unit 系统需要 kernel 增加 registry（记录为缺口），
在那之前不要声称 `open Units.Natural` 会改变 `c`/`hbar` 的数值。

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

> **实现状态（2026-11-05，分数指数打印已修复）**：没有显示单位时的 SI
> 单位拼写现在由 `Dim::to_si_symbol_string()` 渲染，分数指数打印正确：
> `T^(1/2)` → `s^(1/2)`、`T^(3/2)` → `s^(3/2)`、`1/T^(1/2)` →
> `1/s^(1/2)`、`L/T^2` → `m/s^2`。`wiener(seed, m, steps, dt)` 的输出元素
> 量纲是 `Float[T^(1/2)]`，现在打印为 `... s^(1/2)`；符号表达式的幂仍由
> `format_rational` 打印（`x^(1/2)`）。回归测试在 `aevia-core` 的
> `si_unit_text`/显示测试里（含真实 `wiener` 值）。早先“分数指数打印成
> `s^1`”的缺口已关闭。

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

`Units.Constants` 由 `manifest.constants` 生成（30 条，18 条精确、
12 条测量）。符号与 RSSN 完全一致，不改名：

| 常数 | SI 单位 | 类型 |
|---|---|---|
| `c` | `m/s` | 精确 |
| `g_n` | `m/s^2` | 精确 |
| `h` | `J*s` | 精确 |
| `hbar` | `J*s` | 精确 |
| `k_B` | `J/K` | 精确 |
| `N_A` | `1/mol` | 精确 |
| `e` | `C` | 精确 |
| `F` | `C/mol` | 精确 |
| `R` | `J/(mol*K)` | 精确 |
| `M_u` | `kg/mol` | 精确 |
| `T_tp` | `K` | 精确 |
| `atm` | `Pa` | 精确 |
| `G_0` | `S` | 精确 |
| `K_J` | `Hz/V` | 精确 |
| `R_K` | `ohm` | 精确 |
| `Phi_0` | `Wb` | 精确 |
| `K_cd` | `lm/W` | 精确 |
| `dnu_Cs` | `Hz` | 精确 |
| `G` | `m^3/(kg*s^2)` | 测量 |
| `alpha` | `1` | 测量 |
| `R_inf` | `1/m` | 测量 |
| `a_0` | `m` | 测量 |
| `epsilon_0` | `F/m` | 测量 |
| `mu_0` | `N/A^2` | 测量 |
| `mu_B` | `J/T` | 测量 |
| `m_e` | `kg` | 测量 |
| `m_p` | `kg` | 测量 |
| `m_n` | `kg` | 测量 |
| `m_u` | `kg` | 测量 |
| `sigma_SB` | `W/(m^2*K^4)` | 测量 |

- `exact = true` 的常数是**定义值或其精确派生**（来源见 `source`：
  SI 2019 定义、CGPM 1901/1954 等），没有 `uncertainty`；`exact = false`
  的是 CODATA 2022 测量值，带标准不确定度。
- 两类值在 manifest 里都是**精确有理数**（`ScaleValue`），不是 `Float`；
  `exact` 只表示“这是定义值”，不表示存储类型。只有换算成 `quantity`
  且尺度带 `pi` 时才降为 `Float`。
- 常数不是单位：它们有量纲、可参与运算，但不能出现在 `quantity(v, u)` 的
  单位位置，也不在 `UnitRegistry` 里。
- `pi`、`e`（自然常数）、`i` 在 `Math` 中。`Units.Constants.e` 是**基本
  电荷**，与 `Math.e` 同名：`open Units.Constants` 与 `Math.e` 同时可见
  时，`e` 必须限定（`Math.e` / `Units.Constants.e`）。`e_charge` 是
  `Units.Constants.e` 的别名，供不想限定的代码使用。
- 常数的不确定度：核心语言不携带不确定度；需要时用 `Measurement` 库类型
  （§8.14）。

`Units.Constants` 的每个条目是：

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

> **实现状态（2026-11-04）**：`E0501`/`E0504`/`E0505`/`E0506`/`E0508`/
> `E0509`/`E0510` 有实现且被诊断覆盖测试触发。`E0502`/`E0503`/`E0507`
> **当前不可达**：单位没有模块级命名空间（checker 的 units 是完整的
> kernel 表加本地声明，所以“未导入的单位”与内建单位无法区分），因此
> “未导入”“禁止前缀”“前缀/显式冲突”这三种状态都不存在；要触发它们需要
> 先有模块级单位可见性（loader + 每模块 units 表），不是检查器能单独补的。
> 在真正不可达时，编译器不会用别的错误去“假装”它们。

## 5.10 与 rssn 单位层的衔接

rssn 的 `rssn::dim` 提供 `DimTerm`/`DimArena`/`UnitRegistry`；`units` 规则集
只是 registry 的一个消费者。Aevia 的单位层是编译期视图，`Units.*` 由
`manifest.units`（`UnitValue`）生成，并且有 `Point` 类型。两者衔接方式：

1. **单一事实来源。** `Units.*` 是 `manifest.units` 的生成视图（`UnitValue`
   已带 `aliases`/`prefixable`/`category`/`since`）；`UnitRegistry` 只用于
   `resolve`/`format` 这类运行时服务。Aevia 不维护第二张表；用户
   `dimension`/`unit` 声明注册进同一个 registry，再回流到 manifest。
2. **项图不带单位。** `Real`/`Float` 值在宿主侧是相干 SI 数值；需要
   rssn 排版/导出时，才用 `quantity(v, "<相干 SI 单位>")` 包一层，仅用于
   输出，不进入 `compute`。
3. **仿射单位在桥接层消解。** `Point` 在进入 rssn 前转成 K 的 `Float`；
   `to(°C, ...)` 只发生在显示层。
4. **registry 往返。** 仅对**纯有理指数**，经 `Dimension::exact_factors`
   做 `DimTerm -> Dimension -> DimTerm` 精确往返；符号指数不支持往返，
   不得解析 `Dimension.factors` 文本。`UnitDef -> scale/offset -> UnitDef`
   必须精确往返。
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

F64(a) op F64(b)，或 F64 与 Int：
    -> 将 Int 提升为 F64，再计算 F64(IEEE op)

F64(_) 与 Exact(_) 或 Sym(_)：
    -> Real，先把 F64 精确转换成有理数，再走 Real 的规则
```

- `Real + Float` 的结果是 `Real`：`Float` 被精确转换成有理数，符号信息
  保留。不会因为符号而运行时失败。
- `Int` 有独立的整数表示；这里的 `Exact` 表示 `Real` 有理数，即使分母是 1
  也不等同于 `Int`。`Int` 与 `Float` 运算得到 `Float`，`Real` 与 `Float`
  运算仍得到精确的 `Real`，与 §3.4.1 的算术 `join` 一致。
- `Int / Int` 和整数的负次幂产生 `Exact` 有理数，即使结果恰好为整数。
  源码 `1.0 + 2` 的无标注混合字面量默认规则仍把两者读成 `Real`；
  它和已物化的 `Int` 值参与浮点运算是不同的读取场景。
- `0 / 0`、`1 / 0` 在精确相位是错误 `E0801`；在浮点相位是 NaN/Inf
  （IEEE）。
- `sqrt(-1)` 在 `Real` 相位构造符号项 `sqrt(-1)`；`Real` 的符号函数是
  **形式**的，不自动变成复数。需要复数时用 `complex_sqrt` 或 `Complex` 类型。

### 6.3.3 符号项的构造

`build(op, args)` 调用 rssn 的算子构造（`Session::call_value`），不立即运行
全局重写引擎。这样 `x + 1` 是 O(1) 的项构造，不会为每个 `+` 触发一次 e-graph 搜索。

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
- **精确（`Real`）相位的扩展实数运算是部分的。** 除本节明确给出的边界值
  （`ln(0) = -oo`、`ln(oo) = oo`、`exp(oo) = oo`、`exp(-oo) = 0`）外，含
  `oo` 的 `+`、`-`、`*`、`/`、`^` 一律保留为符号项（按原式打印，例如
  `oo - oo`、`oo*0`、`oo/oo`），既不静默折叠成有限数，也不报错；未定义
  形式（`oo - oo`、`oo + (-oo)`、`oo * 0`、`oo / oo`、`oo ^ 0` 等）同样
  保留，`-oo` 的符号不会丢失。需要有限答案时请在浮点相位计算：`Float`
  按 IEEE 给出 `inf`/`nan`（`nan != nan`，`-0.0 == 0.0`）。指数为无穷的
  形式（`0 ^ oo`、`oo ^ oo`）要求有限有理指数，报 `E0402`。这是刻意的
  保守选择：精确相位不发明 NaN/未定义值，也不把未定义形式当作已化简。

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

> **实现状态（2026-11-05）**：穷尽性/冗余臂（`E0309`/`W0201`）分析是有界
> 递归，**模式嵌套深度上限 24**（`Checker::PATTERN_DEPTH_BOUND`）；超过
> 上限时按“已覆盖”处理——它只能**抑制**一条穷尽性/冗余臂诊断，绝不会凭空
> 产生诊断（不会误报）。`or-pattern` 与空枚举（`type Never = |` 形式）在
> 当前语法中不可表达；若出现零构造器类型，零个分支即穷尽（语义保留）。

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
  1×1 → `dot`（§3.5.3.1）。

#### 6.6.4.1 步长

`a[i:j:s]` 是带步长的切片。`s` 是**整数**表达式：不是 `Float`，也不是带量
纲的量（步长是位置的个数，没有量纲，也没有小数部分）。非整数步长是类型错误
`E0302`。

- 省略 `i` 或 `j` 沿用默认端点；省略 `s` 表示 1。`a[i:j]` 与 `a[i..j]` 是
  同一个节点，行为完全相同。
- 端点省略时按步长的方向解释：`s > 0` 时 `i` 默认 0、`j` 默认该维长度；
  `s < 0` 时 `i` 默认最后一个下标、`j` 默认第一个下标**之前**。所以
  `xs[::-1]` 是 `reverse(xs)`，`xs[::-2]` 是 `xs[4], xs[2], xs[0]`。
- 显式端点中的负数仍从末尾计（与索引一致），并截断到合法范围。终点是半开
  的：`xs[4:0:-1]` 是 `xs[4], xs[3], xs[2], xs[1]`，不含下标 0。

长度是下标个数的**精确值**：

```
s > 0:  len = ceil((b - a) / s)      满足 a ≤ i < b
s < 0:  len = ceil((a - b) / |s|)    满足 a ≥ i > b
```

**空情形**：第一步就越过 `b` 的切片是空切片，不是一元切片。`xs[0:0:1]`、
`xs[5:2:1]`、`xs[2:0:1]`、`xs[0:3:-1]` 都为空，且不 panic。

`s == 0` 永不推进：字面量已知时是静态错误 `E0802`，否则是运行时错误
`E0802`。实现不做除零，也不进入无限循环。

负步长产生**视图**，不是拷贝：同一底层缓冲区、不同的 offset、该轴反向
（`View { strides, reversed }`）；正步长只是把该轴的 stride 乘以 `s`。因此
`xs[::-1]` 与 `reverse(xs)` 共享同一缓冲区（`Rc::ptr_eq`），`m[:, ::-1]` 只
翻转第二轴。视图的元素类型仍是 `Float`：缓冲区是 `f64`。

### 6.6.5 线性代数

- `F64`/`C128` 的 `matmul`、`solve`、`inv`、`eig`、`svd`、`qr`、`lu` 走
  rssn 的数值内核（faer/ndarray）。
- `Exact`/`Boxed` 的小矩阵走 rssn 的符号线性代数规则；大矩阵若无法数值化，
  返回 `Err` 或保持符号形式。
- `det` 只接受无量纲；带量纲用 `det_unit` 返回 `Dyn<ρ>`。
- 奇异矩阵 `solve` 返回 `Err(SingularMatrix)`；`inv` 同理。
- 特征值可能是复数；`eig` 返回 `Complex`，即使输入是实数。

### 6.6.6 归约

- `sum` 空张量 = 0；`prod` 空张量 = 1；`mean` 空张量：`Float` 返回
  `NaN`，`Real` 报 `E0802`；`min`/`max` 空张量报 `E0802`。
- `sum(a, axis)` 保留秩（该轴大小为 1，keepdims）；`sum(a)` 返回标量。
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
  multiplicities: Tensor<1, Int>, conditions: Tensor<1, Condition> }`。它不要求
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
read_csv_dyn : String -> IO (Result<Table<Dynamic>, DataError>)
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

rssn 的 `compute` 接受 `Budget`（外层迭代、重写匹配上限、节点数、时间上限、
窗口/beam 参数等）。**实际默认值**（`rssn::graph::schedule::Budget::default`，
2026-11-04 实测）：`max_iterations = 24`、`max_nodes = 20_000`、
`match_limit = 1_000`、`reduce_rounds = 32`、`patience = 3`、
`time_limit = None`（不设墙钟上限）；其余字段见内核源码。预算耗尽报
`E0808`；`eval`/`simplify` 终止并返回错误（或 `panic`，取决于 API）。重型
请求 `integrate`/`solve` 返回 `Result`，预算错误是 `Err`。

**实现状态**：本实现尚未提供语言级的 `with_budget(seconds, rewrites, thunk)`
函数（该调用形式不在当前语法/内建中）；Aevia 的 `--budget` 是**尾调用预算**
（§9.3.4），不是符号预算。符号预算目前取内核默认值，需要调整时通过内核
`Config`/`Budget` API（embedding 层）完成。`Rssn.budget(n)` 诊断提示中的
入口也属于后续补齐项。这是已记录的实现缺口，不是规范删除。

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
- 非尾递归受调用深度限制：默认 2048 次调用（`Interp::max_depth`），
  超过报 `E0805` 并带调用栈；解释器运行在 `--max-stack <MiB>` 指定的
  OS 线程栈上（默认 256 MiB），合法的深递归因此比默认 8 MB 的线程能跑
  更深。
- 尾调用不再增长深度，因此一个**不终止**的尾循环由“连续尾调用预算”
  限制：默认 100,000,000 次（`Interp::max_tail_calls`，`--budget` 修改），
  超过报 `E0805` 并带调用栈。这是 runaway 保护，不是语义上限（§9.3.4）。
- 表达式嵌套深度另有上限（默认 16,384 个节点），超过同样是 `E0805`。
- `panic` 打印调用栈（源码位置 + 函数名）。

## 6.10 确定性与并行

- 纯求值完全确定；相同输入得到相同输出（浮点 NaN 规范化）。
- `par_map(f, xs)` 并行映射，结果顺序与输入一致；线程数不影响结果。
  要求 `Parallel a`（仅 `Int`/`Float` 及其张量）；符号 `Real` 不并行。
  **实现状态**：本实现的求值器是单线程的，`par_map` 顺序执行但保持上述
  顺序语义（见 §9.7.2）。
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
- 文件可以写 `module Thermo.IdealGas`；导入时声明名 MUST 与模块的源路径
  一致，不一致报 `E0601`。包依赖的导入别名属于导入者的名字空间，不改变
  定义处模块名；包内相对源路径名及自身包名限定的路径均可使用。直接运行的
  入口文件没有导入路径上下文，不执行这项声明名检查。
- 一个文件只能有一个模块；不支持一个文件多个模块。
- 模块名区分大小写。

> **实现状态（2026-11-05）**：声明名检查区分源路径与导入别名。
> `Library` 包的 `src/Model.aev` 可声明 `module Model` 或
> `module Library.Model`；使用者通过 `Renamed.Model` 导入时，声明无需改名。
> `import Renamed` 使用依赖自身清单的 `entry`，支持自定义入口。
> 直接运行入口仍豁免声明名检查；独立程序所导入的文件仍检查其声明名。
>
> 模块查找与执行均遵守定义包的作用域：依赖里的 `import Helper` 查找该依赖
> 的 `src/Helper.aev`，模块闭包捕获实际导入边的值。应用的同名 `Helper`
> 不会覆盖库的引用。加载节点以 `(canonical 文件, owning package)` 去重；
> 同一包的别名、菱形导入及符号链接共享检查结果，同一物理源属于不同包时
> 分别解析/检查。外部 `src` 符号链接保留显式包归属，循环按完整节点报
> `E0602`；缓存命中也重新验证每条逻辑导入路径的声明名。

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

> **实现状态（2026-11-05，类型名导入已闭合）**：`import`/`open` 可以引入
> 该模块导出的**值名**（函数、常量、单位）、**名义变体构造器**与**透明
> 类型别名**（`import A { Pair }`、`open Physics.Ising` 后直接写 `Params`
> 都通过检查；`type Pair = { … }` 的定义随接口导出并在导入者展开）。
> 限定名（`A.Pair`/`Physics.Ising.Params`）也可用，但不再是结构别名类型
> 注解的唯一写法。`import`/`open` 两种形式都验证过，含 `open` 引入别名
> 后直接使用。

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
- 单独编译：每个模块导出**接口**：类型与构造器、值签名、单位、**类声明、
  实例、别名定义**。别名导出 MUST 携带定义（展开体），否则导入者无法展开
  `type Vec2 = { x: Float, y: Float }`（只导出名字会得到 `E0301`）。
- 类与实例以符号键导出：类键 `(定义模块路径, 类名)`，实例键再加规范头类型；
  别名先展开再取键。`AdtId` 只是进程内见证，不进入接口。
  接口可以缓存为 `.aevi` 文件；缓存失效按源码哈希、依赖接口哈希和能力集。
- **路径包的名义身份**同时包含包的 canonical 源目录（连同包名/版本）与
  包内相对源文件路径；可读的 `module` 名单独保留。同名同版本的两个本地包
  因源目录不同而拥有不同的 ADT/类/实例键；同一包的两个导入别名共享身份。
  同一物理源可由不同包拥有；归属沿依赖边与包内导入传递，不依赖 canonical
  文件是否仍位于包目录前缀。宿主可用 `SourceIdentity::for_package` 为外部源
  明确指定包；同一物理源的未保存缓冲会提供给其所有包实例。
  这是本地 path 源的身份规则，不是 registry 身份协议或 `.aevi` 缓存实现。
  检查器、接口、实例体 mangling、运行时见证使用同一个完整键。
- **独立源与虚拟会话**也有稳定来源身份。物理源优先 canonical 路径；未创建
  文件按最深已有祖先与规范后缀确定身份，普通保存不会换键；`file://` URI 与
  物理路径使用相同规则。`compile` 与模块加载识别同一 owning package，清单
  自定义 `entry` 即使位于 `src/` 外也归该包。宿主可显式传入虚拟来源身份；
  REPL 每个会话复用自身 URI，追加声明及 `:type`/`:core`/`:load` 不改变它，
  `:reset` 开始新会话身份。名义身份不使用 `FileId`、整文件内容哈希或每次
  检查的随机 nonce。
- **ADT 声明 schema**独立于 source 身份及检查 revision：规范编码只覆盖该
  声明可达的名义类型图，包含构造器名字/顺序、字段的已展开类型、参数种类及
  alpha 规范化的量纲绑定；递归引用按图节点编码。无关源码编辑或绑定改名
  不改变 schema，字段/构造器/可达类型结构改变会改变它。未完成的声明不声称
  跨检查相等；已完成注册不可原地改写，确切 `AdtId` 仍是构造器见证。
  检查器统一、实例头与运行时见证共享 schema 身份；此编码尚不是持久化接口
  格式，也没有把类声明签名纳入 schema 协议。
- IDE 缓存同时比较解析根图的身份（含包名/版本、依赖别名与入口）；只改清单
  而不改 `.aev` 文件也会触发重新加载。未变图仍可命中缓存。
- 每个已检查模块携带该次检查分配的构造器见证（包括私有类型）。旧程序在
  同名模块重新检查后仍使用自身的构造器；`private type` 的构造器不进入公开
  接口。显式导入一个公开类型时，其公开构造器也进入相同导入作用域。
- 模块只合并自身依赖闭包的接口，不因另一个模块恰好较早检查而获得其接口；
  直接导入的透明别名按实际 `as`/包别名前缀绑定。
- **本地运行时元数据与公开接口分离（2026-11-05）**：私有类及其本地实例仍
  保留确切运行时 key；实例体按原声明 `Span` 关联，不依赖 class registry 的
  枚举顺序或跨类的序号。公开接口继续隐藏私有类/实例。类方法作为值传递或被
  闭包捕获时，AST 与 IR 保留完整 `ClassKey`；IR 使用显式
  `ClassMethodRef`，不按方法字符串重新猜类。普通受约束函数仍执行自身函数体。
- 公开透明别名携带已解析的模板，私有 helper alias 的类型/量纲参数在定义处
  展开，因此消费者无需看到 helper 名字。`private` alias 与导入者使用的
  alias 不会自动进入本模块导出表；解析依赖闭包仍用于语义/一致性检查，但
  名字只通过当前模块的显式 import binding 进入作用域。
- 没有隐式全局实例：实例只从导入的模块可见。为避免“实例丢失”，
  标准库的常用实例在 Prelude 中导入。

## 7.4 类型类实例与模块

- 身份是**符号化**的：类键 `(定义模块路径, 类名)`；实例键 `(定义模块路径,
  类键, 规范头类型)`。规范头类型由类型构造器与结构参数构成，别名先展开。
  `AdtId` 只是进程内见证，不用于接口、缓存或跨进程身份。
- 导入一个模块即导入它的类、实例与别名定义。导入的实例参与**全局一致性**
  检查（重叠 `E0311`，消息指出两个模块），但不再重复孤儿规则检查——它在
  定义处已经通过。
- 孤儿规则：实例必须定义在类型所属模块或类所属模块；否则 `E0604`。派生
  实例（`derive`）同样适用，在类型所在模块生成。
- 实例不因 `private` 隐藏；它们随模块导出，`private instance` 不允许。
  `private class` 只在定义模块可见，其实例也不进入接口。
- 运行时只在**量化的调用点**按见证分派（§9.2.3）；检查器必须能证明候选实例
  的见证两两不相交，否则在检查期报 `E0311`/`E0310`，绝不按运行时形状猜测。

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

> **实现状态（2026-11-04）**：清单解析、`target`/`entry`、`--locked`、内容哈希与
> path 依赖已实现；**`version`/`git` 依赖与 registry 未实现**（分别报 `E0605`/
> 无 registry 客户端，`aevia add/update` 只管理本地清单/锁文件）。上面的
> `git`/`version` 示例是设计目标，不是当前可用的依赖形式；`[capabilities]` 目前
> 只被解析/保留，没有运行时权限检查（§9.14）。

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

> **path 包实现（2026-11-05）**：模块根与锁文件从同一个已验证的包图生成。
> 包节点以 canonical 目录去重，导入别名是图的边；同一包的每个别名都进入锁
> 文件。当前仍保留程序范围内的传递依赖别名表：不同包争用同一导入根报
> `E0606`，尚不支持不同包各自为同名依赖选择不同版本。
>
> 锁文件路径相对于根包，依赖内容摘要覆盖 canonical 清单、`src/**/*.aev`
> 和入口文件，因此改依赖的 `entry` 或依赖边也使 `--locked` 失效。源目录中
> 的符号链接循环报 `E0607`。`aevia add` 先解析完整候选图，再暂存清单和锁；
> 普通 I/O 安装失败会恢复原清单，失败的解析不会改写输入。该回滚不承诺机器
> 崩溃时两个文件之间的原子提交。
>
> 模块初始化按包含 `FileId` 的完整 `Span` 安装数值字面量表示、动态量纲检查、
> RSSN overload key、类字典与表达式类型，重复安装同一模块不追加重复记录；
> host registration 列表在模块切换时替换。单位字面量和单位值使用定义源文件
> 的单位表，同名私有单位不会被另一个包的定义替换。

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

> **实现状态（2026-11-05）**：`aevia run file.aev` 与 `aevia run --example
> name` 都可用。`--example` 在包的 `examples/<name>.aev` 按名解析，拒绝
> 路径分隔符（名字不是路径），找不到时会**列出可用的示例**，并且与直接给
> 文件互斥（`--example` 和 file 只能给一个）；测试
> `cli.rs::run_example_finds_the_packages_examples_directory` 覆盖。`aevia
> test` 支持 `.aev` 文件/目录，以及 `--doc` 的 doctest 模式。

### 7.5.4 测试

```aevia
# tests/fall_time.aev
open Kinematics

test "fall time from 20 m" =
    approx_eq(fall_time(20.0 m), 2.019 s, 0.001 s)

test "dimension rejection" =
    # 编译期错误不能用 test 表达；用 compile_fail 指令
    ...
```

- `test` 声明是 `Bool` 表达式；`aevia test` 运行所有测试。
- `approx_eq(a, b, tol)` 要求 `a`、`b` 同量纲，`tol` 同量纲。
- 编译期错误测试用 `compile_fail("<source>")`（§8.13）：它把源码片段交给
  编译器并要求**编译失败**；片段里的导入由调用模块的导入表提供（实现当前
  只支持不带导入的片段，文档化的限制）。`@compile_fail` 属性不是本语言的
  属性（未知属性报 `E0210`）。
- 属性测试用 `forall`（§8.13）。

### 7.5.5 文档

- `##` 文档注释附着到下一个声明，内容是 Markdown。
- `aevia doc` 生成 HTML；类型签名、单位、例子自动提取；未被文档化的声明
  在索引中报告。
- 文档注释中的代码块标 `aevia` 会被编译检查（doctest）：`aevia test --doc
  [FILE…]` 抽取围栏块并逐个编译；块首行可写 `compile_fail` 或
  `compile_fail EXXXX`。片段在自己的合成模块中编译，声明模块的
  `import`/`open` 行会被复制，但模块自身的声明不在作用域内（单文件编译器
  的已知限制）。

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
`ones`/`fill` 是**内建可变参数**：`n` 由 `Int` 实参的个数决定，不是用户
可声明的函数；类型检查器特殊处理。`reshape<[n1, ..., nk]>(a)` 的目标形状
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
`E0314`。

CSV 表头可以带单位：`time [s]`、`velocity [m/s]`。`read_csv<ρ>` 解析
表头单位；`column(t, "time")` 在编译期用 `ρ` 的字段类型检查，运行时再做
一次防御性校验。没有表头单位时，列被当作无量纲；显式 `ρ` 可以覆盖，
但运行时校验失败报 `DataError.DimensionMismatch`。

> **实现状态（2026-11-05，第 8 章 323/323 已闭合）**：
> `read_csv`/`write_csv`/`read_json`/`write_json` 与
> Table/column/select/filter/join 已实现；**NPY 也在本地实现**（不再依赖
> kernel 的 `io.binary`）：`read_npy`/`write_npy` 有本地 NPY 编解码与执行
> runner，实测写读往返为 `write=Ok(())`、`read=Ok([[1, 2], [3, 4]])`。
> NPY 细节：写 version 1.0；读 1.0/2.0/3.0 头，dtype `'<f8'`（写）与
> `'<f8'`/`'<f4'`/`'|f8'`/`'|f4'`/`'=f8'`/`'=f4'`（读；f4 提升为 Float），
> rank 0/1/2（含空数组），只支持 C order。错误按附录 B：`E0901` 文件不存在、
> `E0902` 非法参数（非 dense/rank 不对的写入）、`E0903` 数据损坏/截断或头
> 10 KiB、`E0904` 不支持的格式（Fortran order、大端/非浮点 dtype、错误
> 扩展名、不支持的版本）、`E0905` 绘图。**类型层面 v1 的精确边界**：
> `read_npy` 固定返回 `Tensor<2, Float>`（rank-variable shape 未实现；
> `read_npy<1>` 不改变结果秩，省略 `<n>` 也不报 `E0314`）；`write_npy`
> 接受任何 tensor（checker 只要求是 tensor；rank 0/1/2 由运行时处理，
> 不支持的存储/格式是 `E0904`）。因此第 8 章的声明名**没有**
> capability gap（coverage 的 `chapter_eight_has_no_capability_gaps` 通过；
> 共 **323/323** 个名字——`args` 计入后由 322 变 323）。
> **IO 错误分类 E0901–E0905 已落地**（本节的读者/写者与 §8.12 的绘图动作
> 都遵守）：`E0901` 文件不存在、`E0902` 权限拒绝或非法参数、`E0903` 数据
> 文件解析失败（CSV/JSON 格式错误、NPY 损坏/截断）、`E0904` 不支持的
> 格式/扩展名（例如 `read_csv("data.npy")`、`read_json("data.csv")`、或
> `read_npy`/`write_npy` 给错扩展名，在真正读文件之前就返回）、`E0905`
> 绘图渲染/保存失败。这些分类作为 `Result` 的 `Err` 文本（`E090x: …`）或
> 致命 IO 动作诊断出现，由 `aevia-core/tests/io_taxonomy.rs` 断言
> （`E0901`/`E0903`/`E0904`/`E0905` 有直接用例；`E0902` 由
> `NotFound`/`PermissionDenied` 映射）。

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
Rssn.call_ho       : (String, Tensor<1, Value>, Tensor<1, Callback>)
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

- `Rssn.typed` 可用：它执行动态调用并接受一个 `TypeRep`。当前 `type_rep()`
  产出的 `TypeRep` 只有名字（`kind = "opaque"`），所以运行时只能校验第三个
  参数确实是 `TypeRep`，不能按它逐字段校验返回值；静态保证来自类型检查器。
- `Rssn.call_ho` 的类型正确，但**非空**回调列表不可构造：Aevia 闭包不满足
  kernel `HostCallback` 的 `Send + Sync`，因此运行时报
  `E1001("core.callback")`。空列表等价于一次动态调用，会正常执行。
- `Rssn.register`/`Rssn.register_plugin` 的类型正确，但 Aevia 源码无法构造
  `Plugin` 值：源码级 `@op` 打包（§13.5.2）未实现，因此运行时报
  `E1001("core.plugin.source")`。Rust host 插件仍可经嵌入 API
  （`Session::register`）注册。
- 上述两个名字是 capability 表里的行、`enabled = false`；缺的是 Aevia 侧
  的值构造，不是 kernel 接口。

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
> （`crates/aevia-std/stdlib/**.aev`，**37** 个模块，含 §8.9.1 的 `Sde`；
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
> `SdeAdaptiveOpts`/`SdeOpts`/`SdePath`）；`Physics.Ising`/`Physics.Geodesic`
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
> `crates/aevia-std/stdlib/Units/Constants.aev` 头注释记录了生成时的
> manifest hash；manifest 变化后必须用
> `cargo test -p aevia-std --test stdlib_modules -- --ignored regenerate`
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

> **增量会话（2026-11-05）**：REPL 复检累计源码后安装完整 checked evidence，
> 保持已执行声明的游标，实例体按其声明源位置绑定，只初始化新增声明。
> 新增类型的构造器使用
> 该次检查的确切见证，后续数值字面量、量纲检查与实例调用也使用新证据。
> 已检查的闭包经词法作用域保留该次 `ModuleEvidence`：表达式类型、字面量
> 表示、量纲检查、RSSN 调用键、字典、构造器引用和单位表。相同 source 的
> 后续检查不会改写旧闭包的证据；AST 与 IR 尾调用均按目标闭包切换并恢复
> 上下文。第一类方法值同样捕获词法作用域，实例实现使用本 revision 的代码
> 模板和弱环境引用；导入作用域沿无环模块图保留对应依赖的环境。
> 不同解释器会话（含 reset）的已检查闭包以 `E0809` 拒绝执行。普通变量读取
> 不能从当前注解改写既有 ADT 参数证据或为空张量制造元素 schema；源码构造
> 路径与宿主显式检查 API 才能建立相应证明。这不是任意未检查宿主值的热替换
> 协议，也不声称进程级 ADT 注册表已实现垃圾回收。

> **动态 ABI 容器（2026-11-05）**：检查后的 `Tensor<1, Value>` 保持一维
> boxed 元素，不因元素恰好是张量而堆叠为高阶张量。
> `[to_value(a), to_value(b)]` 始终携带两个 ABI 参数，即使 `a`、`b` 是同形或
> 异形张量；普通数值嵌套字面量仍使用高阶张量规则。AST 与 IR 读取同一份
> 源位置类型证据作此决定，动态调用不再丢失张量参数边界。

**见证表。** 类型类分派只读取下列见证。没有见证的类型不能用于量化的运行时
分派（检查期拒绝，§3.8.4/§7.4）：

| 类型形状 | 运行时见证 |
|---|---|
| `Int` / `Real` / `Float` / `Complex` 标量 | `NumKind`（整数/精确/浮点/复数） |
| `Tensor<n, τ>` | rank + 存储类（`Exact`/`Float`/`Complex`） |
| 名义类型 `T<...>` | `(定义模块, 名字)` + 参数见证递归 |
| 记录 | 规范标签集合（排序后；开放行见下） |
| `Bool` / `String` / `()` / `Prop` | 各自的标签 |
| `Point` / `Unit` 值 / 函数 / `Dyn` | **无见证**（不能按见证分派） |

- **具体头类型**：检查器记录精确实例键（类 + 规范头），Core IR 携带该键，
  解释器直接执行对应实例体；不重新解析、不按形状猜测。
- **量化头类型**：只有当候选实例的见证两两不相交时才允许运行时按见证选择；
  否则检查期报 `E0311`/`E0310` 并给出帮助。见证不是全的：`Tensor<1, Int>`
  与 `Tensor<1, Real>` 的存储类相同，两个实例不能共存。
- 记录见证是**结构**的：不同模块中同标签集合的记录类型不可区分，因此不能
  各写一个实例；名义类型以模块+名字区分（两个模块同名的 `Shape` 是不同类型）。
- 没有运行时多方法分派，也没有按类型/量纲的单态化：`mean<L>` 与 `mean<T>`
  MUST 是同一份代码（§4.4.4、§12.8.13）。表示专用化是按操作、带标签守卫的
  快速路径，不复制泛型函数。

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
  算术只构造项（`Session::call_value`），不调用 `compute`。

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

对 `Dyn<ρ>`，不比较静态量纲；把宿主保存的 `rssn::dim::DimTerm` 作为运行时量纲。

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
- 插件：`Rssn.register`（别名 `register_plugin`）把算子/重写/单位注册进当前 RSSN 会话；
  Aevia 通过 manifest 反射看到它们，不需要重新编译。

## 9.6 JIT

### 9.6.1 触发条件与门控

先读 manifest 的 `Features`：

- `features.jit = false`：`@jit` 报 `E1001("core.jit")`；函数只解释执行。
- `features.jit_branch = false`：含 `if`/`match` 的函数不 JIT。
- `features.jit_index = false`：含张量索引的函数不 JIT。

满足门控后：

- 函数只含 `Float` 标量、算术、初等函数；按上面的 flag 允许 `if`/索引；
- 被调用超过阈值（默认 1000 次）或显式 `@jit`；
- 输入/输出类型固定（无符号、无 `Dyn`、无记录/变体）。

### 9.6.2 流程

1. 把 Core IR 降成 rssn 项（`Float` 标量表达式）。
2. 仅在 `features.jit` 为真时调用 `Term::compile(&["x", "y"])` 得到
   `Compiled`；否则不进入此路径。
3. 缓存键：项结构哈希 + 输入名 + `manifest_hash` + 后端版本。
4. 调用时直接走机器码；失败回退解释器。

rssn 的 `TieredBackend` 可以先解释、后台编译；Aevia 复用这个策略。

### 9.6.3 限制

- 符号值、记录、变体、闭包不进入 JIT。
- `Select`/`Extern1`/`Gather` 已 lowering，但受 `jit_branch`/`jit_index`
  与构建 feature 控制；默认构建 `features.jit=false`。
- `Real` 的精确算术不进入 JIT；需要时先 `to_float`。
- JIT 的结果与解释器 MUST 数值一致（同一 IEEE 语义）；不一致是 bug。

## 9.7 内存与并发

### 9.7.1 内存

- 张量：`Arc` + COW；切片共享；唯一持有时原地。
- 闭包：`Rc`；递归闭包用弱引用/断链；周期收集器。
- 符号项：rssn 的 hash-consed 图；项 id 不跨会话。
- 会话级 arena 在 REPL 中增长；`:reset` 释放。

### 9.7.2 并发

- 纯函数可以并行；`par_map(f, xs)` 的结果顺序确定（与输入顺序一致）。
- **实现状态**：当前解释器的求值器是单线程的，`par_map` 在本实现中**顺序执行**
  且保持顺序语义；线程数不影响结果这一保证仍然成立。真正的并行实现需要
  `Rssn` 会话级并行或把求值器移出单线程（记录为性能缺口，见 STATUS）。
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

> **实现状态（2026-11-04）**：多行输入、`:ast`/`:core`/`:time`/`:latex` 与
> Ctrl-D 已实现；**Ctrl-C 取消当前求值尚未实现**（默认控制台处理会终止
> 进程；协作式取消需要在求值器里加中断标志，属后续工作）。

## 9.9 CLI

```
aevia run [--example NAME] [file] [-- args...]  # 运行可执行包/文件/示例
aevia repl                         # 交互式
aevia check [--all]                # 类型/量纲检查
aevia fmt [--check]                # 格式化
aevia test [--filter pat] [--doc]  # 运行测试 / 编译文档中的 aevia 代码块
aevia doc [--open]                 # 生成文档
aevia bench [--filter pat] [--iterations N] [--warmup N]
            [--save-baseline FILE] [--compare-baseline FILE] [--threshold RATIO]
aevia init name                    # 新建包
aevia add dep                      # 添加依赖
aevia update                       # 更新锁文件
aevia clean                        # 清理构建产物
```

常用 flag：

```
--release          优化模式（当前解释器实现无 JIT/构建缓存，接受并提示“无效果”）
--jobs N           并行度（同上：当前实现单进程解释执行，接受并提示“无效果”）
--max-stack MiB    解释器线程的栈上限（默认 256）
--budget CALLS     连续尾调用预算（默认 100,000,000；不是秒，见 §9.3.4）
--out PATH         输出路径（`rssn --emit`、`doc` 使用）
--color auto|always|never
--locked           拒绝修改 `aevia.lock`（`update`/`add`）
```

> **实现状态（截至本轮）**：`clean` 删除 `.aevia/`（当前实现不写构建缓存，
> 因此通常只报告“没有可清理的内容”）；§9.11 的 `.aevi` 接口缓存尚未实现，
> 每次编译都重新检查全部模块。这是已记录的性能/增量编译缺口，不是语义
> 差异。
>
> **程序参数（2026-11-05 已实现）**：`aevia run [file] -- args…` 只把 `--`
> **之后**的字符串交给程序；程序用 `args()` 读取（§8.1.8），返回值是
> rank-1 `Tensor<1, String>`，无参数时为空张量（`[]`）。`--` 之前的文件路径
> 和 flag 不是程序参数；没有 `--` 时 `args()` 为 `[]`。`check`/`test`/
> `bench` 不转发参数。测试见 `cli.rs` 的 `run` 用例（`-- a b` 与空 `--`）。
>
> **`run --example`（已实现）**：`aevia run --example NAME` 在包的
> `examples/NAME.aev` 解析（名字不是路径：拒绝路径分隔符），找不到时列出
> 可用示例，且 `--example` 与直接给文件互斥；测试
> `cli.rs::run_example_finds_the_packages_examples_directory`。

## 9.10 LSP

- 诊断：类型错误、量纲错误、单位错误、未使用绑定、非穷尽匹配。
- 悬停：类型、量纲、单位、文档。
- 补全：模块名、函数、单位、字段、构造器。**字段补全**：在 `.` 之后只列
  接收者记录类型的字段（不混入通用名字）；**关键字补全**：只在表达式位置
  提供 `do`/`let`/`match`/`with`，不污染类型标注或 `import` 行。
- 跳转：定义、类型定义、单位声明。
- 引用：`textDocument/references`——同一名字在已加载程序里的全部使用，
  `context.includeDeclaration` 遵守；局部绑定/参数只在本文件，顶层声明、
  导入/限定名、类型与构造器、本文档声明的记录字段可跨文件/跨模块。
- 重命名：`textDocument/prepareRename` + `textDocument/rename`——返回
  `WorkspaceEdit`（`documentChanges`），可改局部绑定、顶层声明、被导入且
  在本程序内使用的名字、本文档声明的记录字段；拒绝关键字/预置名/单位名/
  不归本文档编辑的导入名，以及非法或与同作用域名字冲突的新名（返回明确
  错误而不是部分编辑）；编辑后的文本应仍可重新检查通过。
- 签名帮助：`textDocument/signatureHelp`——光标在调用括号内时给出函数名、
  活动参数位置与可用重载（来自已加载程序，含导入声明）；触发字符 `(`/`,`，
  retrigger `,`。**诚实的限制**：活动重载按 arity 选择（`Rssn.<op>` 的
  pinned key、类方法按检查期 dictionary 是例外）；参数标签显示的是**类型**
  而不是参数名；文档只来自声明上方的 `##`（内建函数没有声明文档）。
- 格式化：`aevia fmt` 的同一引擎。
- 代码动作：添加类型标注、把 `degC` 转成 `K`、提取 `let`、修正量纲。
- 语义高亮：类型、量纲、单位、算子来源；没有宏，语法高亮是固定的。

LSP 复用 `aevia-syntax`/`aevia-types`，不启动运行时；`check` 模式即可。
文件已保存时，分析通过 `aevia_core::driver` 加载其导入图（包根沿用
`aevia.toml`），诊断/悬停/补全/跳转按当前文件的 `LoadedModule` 回答，跨
模块跳转返回定义模块的文件 URI；未保存缓冲区也保留导入图（
`driver::load_with_roots_and_source` 覆盖入口源码），无法解析/虚拟文档才
回退为单文件分析。分析结果按入口内容哈希 + 依赖文件 `(len, mtime)` 做有界
LRU 缓存（默认 4 个程序），任何被导入文件的变化都会失效对应条目。
`initialize` 返回的**精确能力列表**（`aevia-lsp` 的 `capabilities()`）：

```json
{
  "positionEncoding": "utf-16",
  "textDocumentSync": { "openClose": true, "change": 2, "save": false },
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
      "unsavedBuffers": "entry-overlay, imports resolved from disk"
    },
    "analysisCache": {
      "strategy": "entry content hash + file mtime/length",
      "capacity": 4
    }
  }
}
```

`aevia-lsp/tests/protocol.rs` 断言这份 capability 表（只声明已实现的行为；
`workspace/symbol` 在已加载程序的全模块上工作、无文档时空列表）。
`references`/`rename` 的请求级测试在 `aevia-lsp/tests/protocol.rs`，数据级
测试在 `aevia-analysis/tests/rename.rs`；`signatureHelp` 的数据级测试在
`aevia-analysis/tests/signature.rs`。

## 9.11 构建与包管理

- 包图拓扑排序；并行编译无依赖的模块。
- 接口文件 `.aevi` 缓存类型/量纲/实例/单位；哈希失效。
- 增量：源码哈希 + 依赖接口哈希 + 编译器版本。
- 锁文件保证可复现；`--locked` 禁止更新。
- 构建产物在 `.aevia/target/`；不提交。
- 中心 registry 是默认源；协议见 §13.7。registry 索引是内容寻址 + 签名 +
  capability 声明；`aevia add`/`publish`/`yank` 都走同一协议。

> **实现状态（2026-11-04）**：本实现**不做**`.aevi` 接口缓存、不并行编译、
> 不实现 registry 客户端：依赖只有 path 依赖（`version`/`git`/registry 依赖
> 报 `E0605`），`aevia add/update` 只改本地清单/锁文件。`check --all` 已改为
> 单遍加载（共享依赖只检查一次），`load_with_roots_and_source` 支持脏缓冲区
> 保留多模块分析；`.aevia/target/` 仍不写。接口缓存与 registry 是已记录
> 的实现缺口（见 PLAN 夜间计划），不是语义删除。

## 9.12 测试与基准

- 单元测试：`test` 声明；`aevia test [--filter PAT]`。
- 属性测试：`forall` + `Arbitrary`。
- Doctest：`aevia test --doc [FILE…/DIRS…]` 抽取 `##` 注释中所有标 `aevia`
  的围栏代码块并逐个编译。块首行可写 `compile_fail`（要求编译失败）或
  `compile_fail EXXXX`（要求失败且包含该代码）；不写标记要求编译通过。
  每个片段作为独立合成模块编译：声明模块的 `import`/`open` 行会被复制，
  但模块自身的声明不在作用域内（单文件编译器限制）。任何失败退出码为 1；
  没有可检查的块或过滤后为空也退出 1。抽取器在 `aevia-doc::doctest`。
- 编译失败测试：`compile_fail("<source>") : String -> Bool` 在隔离环境中
  编译片段（无导入、无调用模块声明，仅 Prelude），产生至少一个错误时返回
  `true`；它**不**检查错误码——错误码由 doctest 的 `compile_fail EXXXX`
  标记断言。
- 黄金测试：REPL 输出/错误信息快照。
- 基准：`bench` 声明；每项报告 N 次迭代的中位数、均值与 95% 置信区间
  （默认 N=100，`--iterations`/`--warmup` 可调）。`--save-baseline FILE`
  写 JSON（`format=1`、`tool`/`version`、`profile=debug|release`、
  `iterations`、`warmup`、每项 `median_ns`/`mean_ns`/`ci_ns`）；
  `--compare-baseline FILE --threshold RATIO`（默认 0.05，可写 `5%`）按
  中位数比较，超出阈值报 `REGRESSION` 并以 1 退出；profile 不一致以 2
  退出；只在一侧出现的 workload 会被报告但不计为回归。协议：只比较
  release 构建、同机、median of N。

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

> **实现状态（2026-11-04）**：`fs`/`net`/`env`/`rand`/`gpu` capability 的
> **运行时权限检查尚未实现**（`with_cap` 不在语言/内建中）；文件/网络访问
> 目前受进程权限约束。`gpu`/`distributed` 等后端能力缺失时按 §13.8 报
> `E1001`。符号预算、栈上限、尾调用预算真实存在（§6.9.1/§9.3.4）；
> “内存上限可配置”目前只有解释器线程栈上限。沙箱化是已记录的实现缺口。

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
Rssn.sin(0.5)                       # Derived 的数值重载：Real
Rssn.sin(x)                         # Derived 的符号重载：Dyn<term>
Rssn.det(M)                         # Overlay：Tensor<[n,n],Real> -> Real
Rssn.dynamic("myplugin_softplus", args)  # 插件/叶子：没有签名
```

- `signature_source = Declared | Overlay | Derived` → 类型化声明；
- `Derived` 是“由注册信息推出”的保守签名：`eval` → `(Real,…) -> Real`
  （谓词 `-> Bool`），非叶子另加 `(Term,…) -> Term`；它不区分
  `Tensor`/`Quantity`/`Int`，所以 IDE 要标“保守推断”；
- `None`（叶子/无调用）→ `dynamic` 声明；RSSN 会做**签名级**校验
  （`check_args`，参数不被任何 overload 接受时报 `OpError::Type`），但
  不做 Aevia 的完整类型/量纲检查，那由静态检查器或 `Rssn.typed` 负责；
- 新算子只要在 RSSN 注册，下一次构建就出现，不需要改 Aevia 编译器；
- `aevia rssn --emit` 的**源码拼写现在也是精确的**（task-31）：维度参数写成
  Aevia 的量纲变量（`Real[T]`、`Real[T^(1/2)]`、`Tensor<2, Real[T^(1/2)]>`、
  `Unit[T]`），变量名在声明内局部（`T`、`T2`…）并**在参数与返回间共享**
  ——`sde_wiener(a3: Real[T]): Tensor<2, Real[T^(1/2)]>` 里的 `T` 与
  `T^(1/2)` 是同一个变量。`Dyn<term>` 只保留在**确实无法拼写**的地方：
  manifest 自己声明为 `Dyn(rep, any)` 的签名（`quantity`/`convert` 的值参数
  与结果、以及 derived 签名）、**秩变量张量**（`transpose`/`rank`/`dot`/
  `norm`/`sum`；Aevia 声明需要具体秩，`Tensor<n, τ>` 是 `E0304`）、类型
  参数、未知行 `Table`、`Device`、变体、host opaque、`Never`。生成的
  `Units.aev`/`Constants.aev` 无条件通过 check；`Rssn.aev` 也通过——生成器
  对每条声明只发**字面量** `op` + `key` 的 `Rssn.invoke`，checker 按 pinned
  overload 校验这两个字面量并把结果类型定为该 overload 的声明结果（每次调用
  新维度变量；`Rssn.sde_wiener` → `Tensor<2, Real[T^(1/2)]>`）。非字面量
  key 保持动态 `Value`，运行时答 `E1002`/`E1003`/`E1005`；checker 不重新
  解析或执行该 key（见 §13.2.2）。这套验收在**干净进程**里由
  `aevia-rssn` 的 `emit_typecheck`（`the_emitted_modules_type_check_with_no_diagnostics`）
  断言：emit 后三个生成模块 **0 diagnostics**；`cargo test -p aevia-rssn`
  为 78 项通过；
- 静态重载用 `OpKey { rule_set, name, signature_hash }`；typed 调用走
  `Rssn.call_key(key, …)` 钉死重载；dynamic 调用走 `call_value`，多注册时
  按 `Match` 格选、并列报 `E1002`（§13.4.1）。`resolve_overload` 只用于
  “先解析再缓存”，且只缓存 `Best::Unique` 的结果。

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
  Cranelift（0.137）。
- `Term::as_tensor()` 把嵌套 `list` 项转成 `(shape, Vec<f64>)`。
- `Term::to_latex()`/`to_typst()`/`to_pretty()` 输出排版。
- `sim::scenario::run(name, json)` 运行物理仿真；
  `sim::scenario::SCENARIOS` 是场景名的唯一来源（2026-11-04 测量 29 个）。
- `Session::register_unit(UnitDef)` 把单位注册进该 session 的 registry；
  `Session::manifest()` 用 `KernelManifest::build_with_units` 生成，所以
  session 私有单位会出现在 manifest 里。
- **SDE manifest 面**：`rules::sde` 把三个无回调算子暴露成 manifest 算子
  （两个噪声原语 `sde_wiener`/`sde_levy_areas`，加方案元数据
  `sde_scheme_info(name) -> Tensor<1, Real>`）；`features.sde` 为
  true 时宿主可用 capability `core.sde`。SDE **求解循环**需要 host 回调
  （`&Fn + Sync`），不在 manifest 里（kernel CO-03）；Aevia 的
  `sdeint_diag`/`sdeint_general` 在宿主侧调用 `kernels::sde`。

Aevia 直接复用的部分：

| 能力 | rssn 模块 |
|---|---|
| 精确数（大整数/有理数/浮点） | `graph::Number` |
| 项构造与 hash-consing | `graph::Graph`、`Session::call_value`/`Session::parse` |
| 重写引擎与规则集 | `graph::Engine`、`rules::*` |
| 微积分/代数/方程/ODE/PDE | `rules::calculus`、`solve`、`ode`、`pde` 等 |
| SDE 噪声原语 | `rules::sde`（manifest 算子）、`kernels::sde`（积分器） |
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
  `x ∈ ℤ`）转成 `Facts`。**一般等式/不等式目前没有语言入口**：`Rssn`
  的固定成员里没有 `assume`（§8.19），运行时也没有对应 arm；后端也没有
  `core.assume.general` 能力行。所以 `assuming` 之外的假设不可用，不是
  “传给 RSSN 后返回 E1001”。（把一般假设暴露成 `Rssn.assume` 是待定的
  扩展，需要同时定义 capability 行。）

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

`Session::call_value(name, args)` 解析重载、校验参数，再构造项并只做该算子
自身的局部重写（`add` 就是一次 n 元展平），不跑全局重写循环；整段表达式
用 `Session::parse`。`Kernel::call(&OpKey, …)` 转发 `call_key`，**钉死**
`OpKey` 指向的重载，不再只按 name 分发；`Kernel::call_ho` 同样按 key 找
注册。Aevia 的 `+` 在 `Real`/`Float` 符号路径上就是一次
`call_value("add", ...)`。

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
| `sdeint_diag`/`sdeint_general` | `kernels::sde`（Aevia 侧包装；噪声原语 `sde_wiener`/`sde_levy_areas` 与方案元数据 `sde_scheme_info` 是 manifest 算子） |
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
let r = Rssn.dynamic("foo", [to_value(x), to_value(y)])
match r with
| Ok(v) -> ...
| Err(e) -> ...
```

- `Rssn.dynamic` 的参数/返回值是 `Value`；RSSN 会按签名检查参数形态
  （不匹配报 `OpError::Type`），完整类型/量纲仍由 Aevia 侧负责；
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
`rssn::dim::DimTerm`。启动时做单位表一致性检查（§5.10），但只用于显示/
导出；`Units.*` 从 `manifest.units` 生成，不维护第二张表。

### 10.3.7 仿真

```rust
let json = rssn::sim::scenario::run("ising", params)?;
```

Aevia 的 `simulate(name, json)` 直接调用；场景名的唯一来源是 kernel 的
`sim::scenario::SCENARIOS`（本次测量 29 个，含 `ising_wolff`），宿主不手写
第二份清单。`Physics.*` 的类型化包装器是**Aevia 源模块**（§8.17），内部调用
`simulate` 并把记录与 JSON 互相转换；kernel 目前的场景参数仍是 JSON，
`ScenarioSchema`（DESIGN.md §9 的设计）尚未实现，所以包装器不是从“场景
manifest”自动生成的——新增场景需要新增一个包装模块。`ising` 返回扁平的
`Vec<i8>`，包装器按 `width`/`height` 行主序 reshape 成 `Tensor<2, Int>`；
`sim` 的错误是字符串，包装器按前缀映射成 `SimError` 枚举。

## 10.4 量纲桥

量纲/单位的唯一事实来源是 RSSN 的 `rssn::dim`（`DimTerm`/`DimArena`/
`UnitRegistry`）。Aevia 的 `Dim` 是它的类型级镜像；`DimValue` 在运行时就是
`rssn::dim::DimTerm`，不是字符串。桥接细节见根目录文档 §3.5。

### 10.4.1 静态到运行时

**静态路径不把量纲传给 rssn。** Aevia 的类型检查器已经保证量纲正确；
项图里的符号是无量纲的，rssn 的 `DimensionMode` 设为 `Off`（或
`Advisory` 仅用于诊断）。这避免了文本歧义和 `quantity` 无 eval 的问题。

只有以下场景需要把量纲交给 rssn：

- `Dyn<ρ>` 的运行时检查：宿主保存 `rssn::dim::DimTerm`，不依赖字符串；
- FFI/导出：优先用 `Config::dimension_term(symbol, DimExpr)`（不解析文本，
  未知基名会被 intern）。只有纯文本入口
  `Config::dimension(symbol, text)` 才必须传**全名**（`Length`、`Mass`、
  `Time`、`Current`、`Temperature`、`Amount`、`Luminosity`），**绝不使用
  单字母**（`L` 是升，`T` 是特斯拉，`N` 是牛顿，`J` 是焦耳）。静态路径
  不传量纲，所以这不影响正常程序。
- 用户显式请求 rssn 的量纲诊断：`Config::dimensions(Advisory)`。

自定义基量纲用 `Config::base_dimension(name)` 声明。它只是把名字放进
`Config`；`prepare` 在**克隆出来的**图上调用 `define_base_dimension`。
`Graph::clone` 共享 `Arc<RwLock<DimArena>>`，写入时 `with_dim_arena_mut`
看到 `strong_count > 1` 就 detach（写时复制），所以请求期注册不会漏进
模板或并发请求，不定义基量纲的请求也不付拷贝成本。

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

`Session` 内部是 `RefCell`，`Term<'s>` 借用 `Session`。Aevia 的桥把内核会话
放在**线程本地**（`aevia-rssn` 的 `kernel()`）：

```rust
thread_local! {
    static KERNEL: RefCell<Option<Rc<Kernel>>> = const { RefCell::new(None) };
}

pub fn kernel() -> Rc<Kernel>;   // 当前线程的会话，首次使用时创建
```

- 每个线程一个内核会话；`par_map` 的“工作线程”因此会各自创建会话（当前
  `par_map` 顺序执行，见 §9.7.2，所以实际只用一个会话）。
- 符号值跨线程传递时，kernel `TermRef` 带 `SessionId`，别的会话的 handle 会被
  拒绝（`OpError::ForeignTerm`）；当前桥不跨线程共享符号值，也不使用
  `Session::fork`/`merge`——`fork/merge` 只是 kernel 提供的原语。
- `compute_all` 在单会话内并行，但**当前 Aevia 桥没有用它**：批量请求按顺序
  逐个调用；需要并行时应在宿主侧按线程分配会话，而不是假设桥已经这样做。

### 10.5.2 项的生命周期

- `Term<'s>` 只在 `compute` 调用期间借用；返回后转成 `u32` id。
- `Session::term_by_id(id)` 取回；id 在会话内稳定。
- `compute` 的答案项已经导入回会话，id 有效。
- `Session` 的项图只增不减；REPL `:reset` 重建会话。

## 10.6 性能策略

### 10.6.1 不要为每个算子调用 `compute`

`compute` 会克隆模板图并运行重写；对 `x + 1` 这种 O(1) 构造，代价高几个
数量级。Aevia 的规则：

- 算术/初等函数：`Session::call_value` 构造项（只做局部重写），不 `compute`。
- `simplify`/`eval`/`diff`/`integrate`/`solve`：`compute`。
- 打印：默认不化简；用户显式 `simplify` 或用 `:latex`（可配置轻量规范化）。
- 比较/`if`：`Prop` 不求值；需要判定时 `decide` 或 `assuming`。

### 10.6.2 缓存

- `Config` 按规则集/假设/模式缓存；避免每次重建。
- 项按结构哈希缓存；重复子表达式复用。
- 编译函数按项哈希 + 输入名缓存。
- 单位表/量纲别名编译期常量。

### 10.6.3 批处理

- `compute_all` 可以在一个会话内并行准备/运行/导入多个独立请求；**当前
  Aevia 桥没有使用它**，批量请求按顺序逐个调 `compute`。这是已记录的
  性能缺口，不是语义差异。
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

## 10.8 已知缺口与风险（适配后）

**下面的数字是测量（2026-11-04，`rssn 3.0.0-rc.1`），live manifest 是唯一
事实来源。** RSSN 已实现 Manifest、动态
`Value`/`call_value`/`call_key`、重载解析、**100% 签名覆盖**（Declared/
Overlay/Derived）、物理常数表、cdylib 插件 ABI、`rssn::dim`、自定义基量纲、
`Config::dimension_term`、`Term` 遍历/替换、`fork/merge`、结构化结果和
JIT lowering。本轮实测：1382 算子 / 32 rule set / 32 精确（7 declared +
25 overlay）/ 1350 derived / 82 单位 / 30 常数（18 精确）/
`manifest_hash = 8d2c3764…`。下面只列**仍然存在**的集成缺口；已关闭的旧
缺口不再重复（构建阻塞、`UnitValue` 元数据、重载只按 arity、
`dimension_term` 缺失、`TypeExpr::Symbol` 缺失、`Solutions.complete`
硬编码、ODE `dimensions` 丢字段、CODATA 常数缺失、自定义基量纲单位注册、
cdylib 未实现、单位表缺 §5.3.3 的七项，均已关闭）。

1. **`Session` 单线程。** `Session` 仍是 `RefCell`；跨线程要用
   `fork/merge`，不是共享会话。`par_map` 对符号值必须 fork/merge。
2. **`compute` 仍克隆图。** `prepare` 仍 `graph.clone()`；`OpTable` 已经是
   `Arc<OpTable>`（clone 共享），`DimArena` 写时复制，所以剩下的是节点表
   的浅拷贝成本。`Session::prepare` 仍未公开。
3. **基量纲必须经 `Config` 声明。** `DimArena` 是写时复制：clone 共享，
   第一次写才 detach，所以 `prepare` 里的 `define_base_dimension` 是隔离
   的；但如果直接改 `Session` 的模板图，就会影响后续所有请求。Aevia 只用
   `Config::base_dimension`，不碰模板。
4. **JIT 默认关闭。** `Select`/`Extern1`/`Extern2`/`Gather` 已 lowering
   （Cranelift 0.137，由 `jit` feature 门控），但默认构建
   `features.jit=false`；`@jit` 必须读 `Features`，否则报
   `E1001("core.jit")`（checker 侧门控）。
5. **`Solutions` 的两个“未报告”语义。** `multiplicities` 为空表示未报告，
   不能填 1。但 kernel 现在**在能证明时填它**：一元有理多项式 + 全部根是
   精确数时，用精确 square-free 分解给重数；`conditions` 只填构造上可声明
   的条件（如 `-b/a` 的 `a != 0`）。`complete` 在 `solve_structured` 里按
   “是否 `list(...)`”区分；`Solutions::new` 默认 `false`，要声明穷尽得显式
   `.complete()`。Aevia 自己构造时不要无条件声明穷尽。
6. **ODE 事件的方法差异已缩小。** `Dopri5`、`Radau5`、`BDF`、
   `Rosenbrock23` 与定步长（`solve_fixed_events`，覆盖 Euler/Heun/RK4）
   都能定位事件；差别只剩“定步长需要 `steps`”和事件定位的精度。
7. **常数是值，不是单位。** `manifest.constants` 有 30 条（18 条精确），
   `Units.Constants` 可从它生成；但它们不在 `UnitRegistry` 里，不能当单位
   参与 `quantity` 解析。`e` 是基本电荷，与 `Math.e` 同名，`open` 两个模块
   时 `e` 必须限定。
8. **100% typed，但只有 32 个精确。** 7 declared + 25 overlay 是精确的；
   1350 个 `Derived` 只区分“数值 `(Real,…)` / 符号 `(Term,…)`”，不区分
   `Tensor`/`Quantity`/`Int`。Aevia 可以给全部 1382 个生成 typed 声明，
   但必须把 `Derived` 标成“保守推断”，否则 `det` 只会是
   `(Term) -> Term`（它是 HEAVY 请求，没有数值重载）。
9. **`assuming` 仍以单元谓词为主。** 一般等式/不等式**没有语言入口**
   （`Rssn` 没有 `assume` 成员，见 §10.2.2）；后端也没有
   `core.assume.general` 能力行。不是“走动态入口后返回 E1001”。
10. **复数表示。** `Value::Complex` 有 `Exact`/`Float`，但 `Number` 仍
    没有复数；符号复数靠 `i` 项，数值复数靠 `Complex64`。两条路径需在
    Aevia 侧统一。
11. **符号指数：桥接可往返，语言仍不支持。** `DimExpr::to_term` 把
    `Length^n` intern 成原子；`DimExpr::try_from_term` 是 term 可表示量纲的
    精确逆，`from_term` 是全函数版（读不回给 `Any`，不静默丢因子）。
    Aevia 的静态类型位置仍只接受有理指数（§4.12.3）。不要把“后端能表示”
    误读成“语言该支持”。
12. **仿真 JSON schema。** `SimResult` 有结构化结果，但场景参数仍是
    JSON；kernel 的 `ScenarioSchema`（DESIGN.md §9 的设计）尚未实现，所以
    Aevia 的 `Physics.*` 是源模块包装器，不能从 manifest 自动生成；新增
    场景需要新增包装模块。
13. **预算/中断。** rssn 的预算在 `compute` 内；Aevia 外层超时/取消是
    协作式的，不能强杀。
14. **版本漂移。** `api_version=1`、`rssn 3.0.0-rc.1`；Aevia 必须 pin
    `api_version` + `manifest_hash`，并跑 conformance。rc 周期内 manifest
    仍可能变化，所以文档数字是测量而不是常量。
15. **`manifest_hash` 是 bincode BLAKE3，有两种取法。**
    `hash_hex()` 是 64 个 hex 字符（本轮测量 `8d2c37640f409249…`），宿主
    应该 pin 它；`hash_u64()` 是前 8 字节按**小端**读出的 u64
    （`4992400f64372c8d`），只用于 ABI/显示。两者不可互换。JSON 只供人读，
    不能对 JSON 文本做哈希/比较。
16. **自定义基量纲要经 `Config` 声明。** `expr_to_dim` 已接受任意基名，
    但 `DimTerm` 只能为 `DimArena` 里已有的基名生成；Aevia 对每个
    `dimension` 发 `Config::base_dimension(name)`，`prepare` 会在克隆图上
    定义它。不要直接改模板图的 arena。
17. **`Value` 的可入项图性按 `manifest.value_kinds` 走。**
    `term_of_value` 对 `Sparse`/`Record`/`Variant`/`Function`/`Opaque` 返回
    `Unsupported`；`Complex` 仅虚部为 0 时可入图；`Unit` 是定义不是数
    （17 个变体中 10 个可入图、7 个不可），要过边界用
    `quantity(value, unit)`。稀疏运算必须走 `eval_dyn` 或 Aevia 本地布局
    内核。生成器不要硬编码这张表。
18. **两个 Aevia 侧 bridge capability 缺口。** `core.callback`：Aevia 闭包
    不满足 kernel `HostCallback` 的 `Send + Sync`，`Rssn.call_ho` 的非空
    回调列表报 `E1001`；`core.plugin.source`：Aevia 源码 `@op` 打包未实现，
    `Rssn.register`/`register_plugin` 报 `E1001`（Rust host 插件仍经嵌入 API
    注册）。两者是 capability 表里的行，`enabled = false`；缺的是 Aevia 侧
    的值构造，不是 kernel 接口。
19. **单位属于 session。** `Session::register_unit` 注册的单位只出现在该
    session 的 manifest（`KernelManifest::build_with_units`）；进程级
    `KernelManifest::build` 给出不同的单位表与 hash。插件注册的单位只属于
    安装该插件的 session，Aevia 必须用 `Session::manifest()`。
20. **SDE：manifest 只暴露噪声原语 + 方案元数据，求解循环保持 Rust-only。**
    本轮 `Features`
    新增 `sde: bool`（true），宿主 capability 名 `core.sde`；
    `sde_wiener`/`sde_levy_areas` 是 overlay 精确签名（`RootOf{2,3}` /
    `Same(1)`），**`sde_scheme_info(name) -> Tensor<1, Real>`** 是本轮新增的
    第三个 SDE manifest 算子（方案元数据：强阶 + Itô/交换/自适应/Lévy 面积
    四个 flag，dim rule `NonMetric`；无回调，所以能进 manifest），typed 声明
    随 manifest 自动出现；它是 **manifest-only 的元数据查询**，不是求解
    路径（求解循环仍 Rust-only），且**只给强阶、没有弱阶**——弱阶在宿主侧
    `sde_order`（返回 `{strong, weak}`）里，两者互补。它覆盖 kernel 的
    8 个方案（6 个定步长/Stratonovich + 2 个自适应行，snake_case 名字）。
    生成源对维度变量的签名现在也是精确的：`sde_wiener` 的声明是
    `(Int, Int, Int, Real[T]) -> Tensor<2, Real[T^(1/2)]>`、
    `sde_levy_areas` 是 `(Int, Real[T], Tensor<1, Real>, Int) ->
    Tensor<1, Real[T]>`（声明内共享同一维度变量映射）；`Dyn<term>` 只为
    manifest 自己开放的类型（`Dyn(rep, any)`、秩变量张量、类型参数、未知行
    Table、Device、变体、opaque、Never）保留，见 §10.0。manifest 仍是唯一
    精确来源。
    求解器需要 `&Fn + Sync` host 闭包，manifest 的 `Value` ABI 传不了，
    所以 kernel 的 CO-03 把求解循环、ensemble 统计、host-callback drift 和
    `FnMut` adapter 留在宿主侧；Aevia 的 `sdeint_*` 不是 `Rssn` 算子。
    **宿主侧已落地**：`aevia-std::sde` 实现六种方法与 fixed/adaptive 求解，
    `aevia-core` 负责 dispatch/记录展平（`sde_runtime` 10/10 通过，
    源码级 `sde_language` 5/5 通过），`core.sde` 在本构建 enabled；
    **源码级策略记录也已验证可用**（`Sde.Fixed`/`Sde.Adaptive` +
    `Sde.Diagonal`/`Sde.General(m)` 通过检查并跑出路径，见 §13.2.2 状态）；
    记录状态 + 一般噪声在语言层是 checker 的 `E0302`，运行时的
    `E1001("sde.general.record")` 只保留给宿主层直接调 builtin 的路径。
    记录顺序/一致性见 §10.3.7。
    `manifest_hash` 变化一方面因为 `Features` 多了这个字段，另一方面本轮
    加了 `sde_scheme_info`（hash 从 `8ed92a22…` 变为 `8d2c3764…`）。

### 10.8.1 缓解措施

- **一致性测试**：对每个标准算子，比较 Aevia 静态量纲与 rssn
  `Answer.dimension`；不一致即失败。
- **单位表校验**：启动时逐项比较基单位/派生单位/仿射单位，并且只以
  `manifest.units`（本次测量 82 项）为准。
- **回退路径**：JIT 失败回退解释器；符号求解失败返回 `Err`，不 panic。
- **预算默认值**：2 s / 1e6 重写；用户可调。
- **版本 pin**：pin `api_version` + `hash_hex()`；升级时跑全量 conformance；
  不把文档里的算子/单位计数写进缓存键。
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
   `sqrt` 构造 `sqrt(...)` 项。这些都是 `Session::call_value`（只做局部
   重写），不 `compute`。
4. 返回 `Float` 值：`g` 是 `Float` 常量，`2*h/g` 折叠为浮点数；`sqrt` 对
   `Float` 永远走 f64，负数得 NaN，不会保持符号。
5. `println` 调用 `show`：直接得到 `2.01962…`，按显示单位 `s` 打印。
6. 如果用户写 `approx(fall_time(20.0 m))`：`compute` 以 `Numeric` 目标运行，
   rssn 返回数值球，Aevia 取中点。

> **规范 vs 当前实现的偏差（记录在附录 C）**：上面的 4–5 是**设计语义**
> ——静态 `Float` 的值在运行时就是 f64。当前实现把十进制**量纲字面量**在
> 编译期折叠为精确有理数（`20.0 m` 的值是精确的 20），即使静态类型是
> `Float[L]`；于是 `sqrt` 看到精确参数、保持符号，`fall_time(20.0 m)` 打印
> 为 `sqrt(800000/196133) s`，`approx(...)` 才给出数值。这是实现偏差，不是
> 本章语义。

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
    let r = [[1.0, 2.0], [3.0, 4.0]] * ohm
    let i = [0.1, 0.2] * A

    let v = r @ i
    println("V = {v}")                  # [0.5 V, 1.1 V]

    let p = v * i
    println("P = {sum(p)}")             # 0.27 W

    match linsolve(r, v) with
    | Ok(i2) -> println("I = {i2}")     # 回到 [0.1 A, 0.2 A]
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

这个例子展示了“单位是编译期尺度”的好处：矩阵元素带单位，矩阵乘法自动
得到电压，不需要在运行时给每个元素贴标签。

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
        | (Ok(t), Ok(v)) -> do
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

这些函数对任何量纲都成立；运行时没有单态化，量纲在类型检查后擦除。

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
    let x = symbol("x")
    println("f' = {diff(fn t => exp(-t^2))(x)}")

    # 3. 线性代数
    let m = [[2.0, 1.0], [1.0, 3.0]]
    let b = [1.0, 2.0]
    match linsolve(m, b) with
    | Ok(x) -> println("x = {x}")
    | Err(e) -> println("solve failed: {e}")

    # 4. 数据与绘图
    let ts = linspace(0.0 s, 1.0 s, 100)
    let vs = map(fn t => (3.0 m/s) * exp(-(t / s) / 0.5), ts)
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
- 混合运算 `Real` 吸收 `Float`（精确优先），符合符号计算；热路径显式 `Float`。

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

rssn 的 `Session` 是单线程；符号值绑定会话。`par_map` 对 `Real` 用 `fork/merge`（RSSN 0.2.13 已提供）；数值张量并行。影响：符号并行的
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

### 12.8.13 分派与扩展边界

Aevia 不做多重分派，也不做按类型或量纲的单态化。前者是语义决定，后者是
工程决定，理由不同。

- **没有运行时多方法。** 方法调用在检查期解决：具体头类型记录精确实例键
  （类 + 规范头）；只有量化头类型允许按运行时见证在**已证明两两不相交**的
  候选实例之间选择。见证不能区分的实例在检查期被拒绝（§3.8.4、§9.2.3）。
  这样“选哪个实例”仍然是一个静态问题，运行时只是执行检查器已经证明的
  选择，不做回溯或按值猜测。
- **不单态化。** 单态化按类型复制泛型代码：它放大编译时间与代码体积，破坏
  单独编译（每个调用类型组合都要重新生成），并且与类型擦除的目标
  （§9.2.3、§4.4.4）以及量纲多态直接冲突——`mean<L>` 与 `mean<T>` 必须是
  同一份代码。单态化的正确性收益也不存在：类型错误在检查期已经排除。
- **表示专用化不是单态化。** 按操作的、带标签守卫的表示专用化保留泛化路径，
  不复制泛型函数、不改变语义；它在测量与分配基线之后才进入性能计划
  （§9.13）。
- **扩展点是显式的**：ADT、记录行、模块/导入、`derive`（受限结构集，
  §3.8.5）、`@op`/`@rule`、插件与注册表、`Dyn` 逃逸。模块接口导出类型、
  构造器、类、实例与**别名定义**（§7.1.3/§7.3；导入者可直接展开
  `type Vec2 = { x: Float, y: Float }`——task-22 已闭合）。没有隐式全局实例、
  没有类型类默认方法（Prelude 类的“部分覆盖”来自内置结构实现，不是用户
  可写的默认方法）、没有宏/引用/拼接（§12.8.5）。

代价是：用户不能给既有类型添加类型类实例（孤儿规则），分布、单位这类
“有参数的量纲族”要用名义类型 + 内建重载表达（§8.7.1），而不是多参数类。
这些缺口是设计边界，不是版本限制。

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
- 类型/量纲代数、统一、泛化、默认化；`rssn::dim` 适配层；
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
- `rssn::dim`/`UnitRegistry` 往返一致；
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
2. **单一量纲/单位层。** `rssn::dim` 的 `DimTerm`/`DimArena`/`UnitRegistry`
   是运行时与编译期共享的规范层。Aevia 的 `Dim` 是它的类型级镜像；
   `Units.*` 由 `manifest.units` 生成，不维护第二张单位表。
3. **动态调用是正式能力。** 没有类型签名的算子仍可通过
   `Rssn.dynamic(name, args)` 调用；类型化 façade 是静态体验的增量，不是
   能力门槛。RSSN 在动态路径上做**签名级**校验（`check_args`），但不做
   量纲/类型推导；`Dyn<term>` 兜底让这条校验对多数 derived 算子很弱，
   完整检查仍由 Aevia 静态检查器或 `Rssn.typed` 负责。

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
  units        : [UnitValue]
  base_dimensions : [BaseDimInfo]
  plugins      : [PluginInfo]
  constants    : [ConstantValue]
  value_kinds  : [ValueKindInfo]
```

`OpInfo` 的实际字段：`keys: [OpKey]`（每个 overload 一个，`keys[0]` 是
主重载）、`name`、`aliases`、`category`、`arity`、`overloads`、
`signature_source`、`flags`、`binder`、`cost`、`eval`、`higher_order`、
`purity`、`determinism`、`thread_safety`、`stability`、`since`、`doc`、
`examples`、`dim_rule`、`backend_support`、`dynamic_reason`、`rule_set`、
`jit`。完整 schema 见根目录文档 §3.1。

`manifest_hash` 是确定性 bincode 的 BLAKE3；`to_json()` 只供人读。
`units` 是 `UnitValue`，带
`name`/`display`/`dimension`/`scale`/`offset`/`affine`/`aliases`/
`prefixable`/`category`/`since`，生成 `Units.*` 只读 manifest 即可。
`KernelManifest::build_with_units(graph, sets, plugins, &registry)` 从调用方
的 registry 读单位，`build(...)` 从进程级表读；`Session::manifest()` 用前者，
所以 `Session::register_unit` 注册的单位会进入**该** session 的 manifest，
不会泄漏到别的 session。

`value_kinds: [ValueKindInfo]` 描述每个 `Value` 变体能否入项图
（`in_term_graph`）以及不能时的替代做法；生成器按它决定 façade 的参数
位置能不能接受该类型（§13.3.1）。本次测量 17 个变体中 10 个
`in_term_graph = true`。

`constants: [ConstantValue]` 是物理常数表（30 条、18 条精确），字段
`name`/`symbol`/`value: ScaleValue`/`dimension: DimExpr`/`unit`/`exact`/
`uncertainty`/`source`；`Units.Constants` 由它生成（§13.6.5）。

`features.plugin_kinds` 说明这个构建能加载哪种插件（默认
`["in-process"]`；开 `plugin-cdylib` 后多一个 `"cdylib"`）。
`features.sde`（本轮新增）说明 `sde` rule set 已安装、SDE 噪声原语
（`sde_wiener`/`sde_levy_areas`）与方案元数据算子 `sde_scheme_info` 可从
manifest 调用；宿主侧的 capability 名是 `core.sde`。`Features` 是 manifest
的一部分，新增字段/算子会改变 `manifest_hash`（本轮 hash 从 `1920638b…`
变为 `8ed92a22…`，随后因新增 `sde_scheme_info` 变为 `8d2c3764…`）。

### 13.2.2 生成规则

Aevia 构建时：

1. 读取 manifest；校验 `api_version`（不兼容报 `E1004`）。
2. 对每个 `OpInfo`：
   - `signature_source = Declared | Overlay | Derived`：生成
     `Rssn.<name>` 类型化声明（**每条 overload 一个**）；
   - `signature_source = None`：生成 `Rssn.dynamic`；
   - 别名：生成 `Rssn.<alias>`；冲突时用 `Rssn.<rule_set>.<name>`。
3. typed 声明同时记住 `OpInfo.keys` 里对应的 `OpKey`，调用必须钉死该
   overload，静态选择与运行时执行不会分歧。公开形式是
   `Rssn.call_key(key, …)`；当前生成器生成等价的
   `Rssn.invoke(name, signature_hash, args)`（同一钉死语义，错误通道不同：
   生成的 `fn` 必须返回声明类型，所以 kernel 失败会 raise 而不是返回
   `Err`）。
4. 生成 `Rssn.Version`、`Rssn.ManifestHash`、`Rssn.Capabilities`、
   `Rssn.ValueKinds`、`Rssn.Constants`、`Rssn.Units`。
5. 生成 LSP 补全、文档、签名存根；存根里记录每条声明的来源。

当前实测覆盖（`rssn 3.0.0-rc.1`，2026-11-05；live manifest 是唯一事实
来源）：1382 个算子 **100% 有签名**（32 个 rule set），0 unexplained。
来源是 **7 declared + 25 overlay + 1350 derived**；overload 分布是 1201 个
1 条、178 个 2 条、2 个 3 条、1 个 5 条（共 1568 条签名）。这些数字是
**测量**，不是要写死进生成器的常量。

**精度分档**（生成器必须区分）：

| 来源 | 数量 | 精度 | 生成什么 |
|---|---|---|---|
| `Declared` | 7 | 精确 | typed |
| `Overlay` | 25 | 精确 | typed |
| `Derived` | 1350 | 形态级 | typed，但只有 `Real`/`Bool`/`Term` |
| `None` | 0 | 无（叶子/无调用） | `dynamic` |

`Derived` 的规则是“由注册信息推出”，不是猜名字：

| 注册信息 | 推出的签名 |
|---|---|
| 有 `eval` | `(Real, …) -> Real` |
| 有 `eval` + `PREDICATE` | `(Real, …) -> Bool` |
| 非叶子 | 另加 `(Term, …) -> Term` |
| `Binder` 在第 k 位 | 第 k 个参数是 `Symbol` |
| `Arity::Variadic` | 尾参数变长 |
| `OpFlags::LEAF` | 无签名（`DynamicReason::OpaquePayload`） |

因此 `sin` 有两条重载：`(Real) -> Real` 与 `(Term) -> Term`（它有 `eval`）。
`det` 是 `HEAVY` 请求、没有 `eval`，所以没有 overlay 时**只有一条**
`(Term) -> Term`——连数值形态都没有，`det(matrix)` 的静态类型会是
`Dyn<term>`；overlay 把它补成 `(Tensor<[n,n],Real>) -> Real`。这就是为什么
“100% typed”不等于“100% 精确”：1350 个算子的签名不区分
`Tensor`/`Quantity`/`Int`，Aevia 的 IDE 应把它们标成“保守推断”。

overlay 也是**维度类型参数**的载体：`sde_wiener`/`sde_levy_areas` 用
`T` 这类维度参数把 `sqrt(h)`（`RootOf{degree:2,index:3}`）和
`Tensor<1, Real[T]>` 写进签名，derivation 推不出这些信息。manifest 里的
签名是精确的，bridge 的 conformance 测试断言 `sde_wiener` 的元素量纲恰是
“一个维度变量上的 `^(1/2)`”；checker 与 `registry().resolve` 都按精确
签名工作。**生成源的拼写现在也是精确的**（task-31）：`render_dim` 把维度
参数写成 Aevia 的量纲变量，变量名在**声明内局部**（`T`、`T2`…）并跨参数
与返回共享——`aevia rssn --emit` 产出的 `Rssn.sde_wiener` 是
`(Int, Int, Int, Real[T]) -> Tensor<2, Real[T^(1/2)]>`，`sde_levy_areas` 是
`(Int, Real[T], Tensor<1, Real>, Int) -> Tensor<1, Real[T]>`；不再有
“open dimension 只能写 `Dyn<term>`”的限制。`Dyn<term>` 只为**确实无法
拼写**的类型保留：manifest 自己开放的 `Dyn(rep, any)`（`quantity`/`convert`
的值参数与结果、derived 签名）、**秩变量张量**（`transpose`/`rank`/`dot`/
`norm`/`sum`；声明需要具体秩）、类型参数、未知行 `Table`、`Device`、变体、
host opaque、`Never`。生成的 `Units.aev`/`Constants.aev` 无条件通过 check；
`Rssn.aev` 也通过——**字面量 pinned key 的结果类型化**：生成器只发
`Rssn.invoke(op, key, …)` 且 `op`/`key` 都是字面量，checker 按 manifest 的
pinned overload 校验并给出该 overload 的声明结果类型
（`Rssn.sde_wiener` → `Tensor<2, Real[T^(1/2)]>`、`Rssn.det` → `Real`…），
每次调用取新的维度变量；**非字面量、未知或歧义的 key** 保持动态 `Value`，
诊断由运行时给出：`E1005`（未知/不匹配的 `signature_hash`）、`E1002`
（歧义的注册/重载）、`E1003`（动态参数或非函数值）。checker 只按 live
manifest 校验两个字符串字面量 key，绝不重新解析或执行它；映射用的是
manifest 自己的签名，不是渲染文本。这套“emit 后 0 diagnostics”的验收在
干净进程里由 `aevia-rssn` 的 `emit_typecheck` 断言
（`the_emitted_modules_type_check_with_no_diagnostics`；
`cargo test -p aevia-rssn` 78 项通过）。新增的
`sde_scheme_info(name) -> Tensor<1, Real>` 没有开放维度，生成源同样是精确
的。**SDE 求解循环则不在 manifest 里**（host 回调，kernel CO-03），宿主侧
`sdeint_*` 不经过 `Rssn` 算子通道；`sde_scheme_info` 是本轮
新增的**元数据算子**（强阶 + Itô/交换/自适应/Lévy 面积四个 flag，dim rule
`NonMetric`，无回调，故能进 manifest）。它覆盖 kernel 的 8 个方案
（snake_case 名，含 2 个自适应行），是 **manifest-only** 的查询，只给强阶、
没有弱阶——弱阶仍由宿主侧 `sde_order`（§8.9.1）给出，两者互补。

> **实现状态（2026-11-05）**：SDE 的宿主侧求解器（`sdeint_diag`/
> `sdeint_general`/`wiener`/`sde_order`/`sde_ensemble_seed`）已实现并有
> 10/10 集成测试（`sde_language` 源码级 5/5）；`Sde` 模块 re-export 这五个
> 入口并声明选项 ADT，checker 已实现 `RateOf`/`DiffusionOf`。能力行
> `core.sde` 在本构建为 enabled（`Rssn.Capabilities` 可见）。**源码级
> 策略构造已验证可用**：`Sde.Fixed({ structure = Sde.Diagonal, method =
> Sde.EulerMaruyama, … })` 通过检查并跑出路径（`Fixed` 与 `Adaptive`、
> `Diagonal` 与 `General(m)` 都验过）；名义变体构造器可以从 `Sde` 导入。
> 透明类型别名（`type X = { … }`）的类型名现在也能经 `import { X }`/`open`
> 引入（task-22 已闭合，例如 `Physics.Ising.Params` 可直接注解；限定名
> 同样可用，§7.2.2/§8.20），因此 SDE 选项 ADT 与其它结构别名类型没有剩余
> 导入缺口。`sde_wiener`/`sde_levy_areas` 的 manifest 签名始终是精确的。

Aevia 仓库**不允许**出现按算子名手写的绑定文件，也**不保留**
`signatures.overlay.json` 的副本。只允许：

```
tools/gen_rssn_module.aev        # 生成器
```

`signatures.overlay.json` 是 RSSN 的构建输入，overlay 在 RSSN 侧已并入
`OpInfo.overloads`；宿主从 manifest 读到的就是应用后的精确签名，
`signature_source` 只用于标注。

CI 检查：生成器输出与提交的生成文件一致；manifest 快照变化必须显式更新。

### 13.2.3 命名空间

- 普通算子：`Rssn.<name>`（`Declared|Overlay|Derived` 都生成 typed；
  只有 `None` 与插件算子走 `Rssn.dynamic`）。
- 规则集限定：`Rssn.<rule_set>.<name>` 只是命名空间；运行时仍按 name/arity。
- 插件：`Rssn.<plugin>.<op>`。
- 动态：`Rssn.dynamic("diff", args)   # args: Tensor<1, Value>`。RSSN 的
  `call_value` 只按 name/alias 查找，**不支持** `rule_set/name` 限定名；
  限定名是 Aevia 的命名空间，不是运行时键。要钉死重载用
  `Rssn.call_key(key, args)`，`key` 来自 `Rssn.overloads(name)` 或
  `OpInfo.keys`。

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

`Value` 是**动态边界**的类型，不是核心语言的隐式子类型。核心语言仍然是
静态的；`Value` 只出现在 `Rssn.dynamic`、插件、FFI、反射 API 中。

**可入项图性**：这张表由 `manifest.value_kinds` 给出，Aevia 不维护第二份。
本次测量（17 个变体）是：

- `in_term_graph = true`（10 个）：`VInt`/`VReal`/`VFloat`/`VBool`/
  `VString`/`VSymbol`/`VQuantity`/`VTensor`/`VTerm`/`VDyn`；
- `in_term_graph = false`（7 个）：`VComplex`（仅虚部为 0 时降为 `Float`
  入图，否则 `Unsupported("complex literals")`）、`VUnit`（单位是定义不是
  数：`term_of` 拒绝它，过边界用 `quantity(value, unit)`；只有已声明的单位
  参数位置才直接收 `Value::Unit`）、`VSparse`/`VRecord`/`VVariant`/
  `VFunction`/`VOpaque`（`term_of_value` 返回 `Unsupported`）。

`false` 的变体只能喂给有 `eval_dyn` 的算子，或作为返回值；生成器不能假设
所有 `Value` 都能参与重写。

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

- `Rssn.call` 是 `Rssn.dynamic` 的别名，只按短名/别名查找；不支持
  `rule_set/name` 限定名。短名歧义报 `E1002`。
- `Rssn.call_key` 是“不解析、直接执行”的入口；它收 name +
  `signature_hash`（32 hex 字符），两者合起来就是 `OpKey` 的可写形式。
  `signature_hash` 全零表示“该算子没有声明重载”。key 与当前 manifest 不符
  时报 `E1005`（提示重新生成模块）；kernel 侧参数偏离钉住签名时报
  `UnknownOp`，在 Aevia 边界变成错误值。
- `Rssn.resolve_overload` 暴露运行时选择结果，供缓存/诊断用：成功返回
  `"rule_set.name@signature_hash"`；并列与“没有签名接受”都报
  `OpError::Ambiguous`（后者 `count` 是签名总数）。
- `Rssn.overloads` 返回每条 overload 的渲染签名文本。
- 参数用 `to_value(x)` 注入；返回值用 `from_value(v)` 转回静态类型。
- 没有签名（`SignatureSource::None`）的算子、插件算子、实验性入口走
  `dynamic`。标准集当前没有 `None`，所以 `dynamic` 在标准集上只是插件
  通道。
- `Rssn.typed` **可达且可用**：它执行动态调用并接受一个 `TypeRep`。当前
  `type_rep()` 产出的 `TypeRep` 只有名字（`{ name = "TypeRep", kind =
  "opaque" }`），所以运行时只能校验第三个参数确实是 `TypeRep`，不能按它
  逐字段校验返回值；类型/量纲保证仍来自 Aevia 类型检查器。它不会把
  kernel 错误变成诊断，错误仍是 `Err(OpError)`。
- 高阶算子用 `Rssn.call_ho(name, args, fns)`；类型正确，但**非空** `fns`
  在当前构建不可构造（Aevia 闭包不满足 kernel `HostCallback` 的
  `Send + Sync`）：运行时报 `E1001("core.callback")`。空列表就是一次
  动态调用。
- `Rssn.register`/`Rssn.register_plugin` 类型正确，但 Aevia 源码无法构造
  `Plugin` 值（源码级 `@op` 打包未实现）：运行时报
  `E1001("core.plugin.source")`；Rust host 插件仍经嵌入 API 注册
  （§13.5.1）。这两个名字是 capability 表里 `enabled = false` 的行，缺的是
  Aevia 侧的值构造。

### 13.3.3 错误

`OpError` 是结构化变体：

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

`E1003` 是“没有匹配的算子/重载”，携带候选签名列表；`E1001` 是
“后端 capability 缺失”，携带 capability 名和最小 RSSN 版本。

---

## 13.4 重载、签名与解析

### 13.4.1 重载

RSSN 允许同名多签名，判定规则在 `kernel::overload` 里：

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

- 每个参数单独分类，整个调用的分数是**最弱**的那个参数；
- `Int -> Real`、`Int|Real|Float -> Complex` 是 `Widened`，反向不成立；
- 同构造器但元素/量纲未被签名钉死是 `Shaped`；类型变量和 `Dyn` 是
  `Generic`；
- `best` 内部并列最高 → `Ambiguous`，不静默取第一个；
- `score_types`/`best_types` 与 `score`/`best` 共用同一个取分折叠：Aevia
  在编译期解析一次，得到的数与 kernel 运行时会得到的是同一个。

**当前标准集的实际情况**（本次测量）：1382 个算子分布在 1382 个唯一名字
上，每个名字只有 1 个注册；但**每个注册内部有 1–5 条重载**（1201 个 1 条、
178 个 2 条、2 个 3 条、1 个 5 条），因为有 `eval` 的算子同时有数值与符号
两条，overlay 再补精确签名。所以要把两件事分开：

- **注册选择**（`resolve_op`）在 `fitting.len() == 1` 处直接返回，对标准集
  不做评分；跨注册并列只可能由插件触发。
- **重载选择**（`resolve_overload` → `overload::best`）对注册内的 1–5 条
  签名**真的执行**：`sin(0.5)` 选 `(Real) -> Real`，`sin(x)`（`x` 是
  `Symbol`）选 `(Term) -> Term`。但实参本身是 `Term`/`Dyn` 时，数值重载
  的 `Real` 也会以 `(_, Dyn(_)) → Generic` 通过，两条同分 → `Ambiguous`。
  所以 `resolve_overload` 只适合具体类型的实参；符号表达式用
  `OpInfo.keys` 的 key。

`call_value` 按 name/arity 选注册，然后调 `check_args`：没有任何 overload
接受参数时返回 `OpError::Type`。但符号重载的 `Dyn<term>` 对任何值都是
`Generic`，所以 1350 个 derived 算子几乎总能通过——这条检查只在签名全部
具体（`eq`/`lit`/`pow`）时才真正拦得住。它不在数值/符号之间做选择（那由
算子的 `eval` 决定）。

Aevia 的静态解析规则（与 RSSN 的格同构，供 Aevia 自己的重载/插件使用）：

1. 按 arity 过滤；
2. 逐参数分类，取最弱；
3. 分数最高且唯一 → 选中；
4. 并列最高 → `E1002`；
5. 没有匹配 → 尝试 `Dynamic`；仍失败 → `E1003`。

运行时三条入口，职责不同：

- `Session::call_key(&OpKey, …)`（固有方法）**不重新解析**，按
  `rule_set` + `signature_hash` 找注册；它钉死重载，并要求这些参数能被
  钉住签名接受（`overload::score(pinned, args).is_some()`），否则报
  `UnknownOp`。
- `Session::call_value(name, …)` 按 name/arity 找注册；多个候选时用
  `overload::score` 选最具体，并列报 `OpError::Ambiguous`；参数不被任何
  overload 接受时报 `OpError::Type`（`check_args`）。
- `Kernel::call(&OpKey, …)` 转发 `call_key`，因此**也钉死**重载；通过
  `dyn Kernel` 传递不会丢信息。

`Session::resolve_overload(name, args)` 用于“先解析、再缓存”：

- `best` 为 `Unique` → 返回带 `signature_hash` 的 key，可直接给 `call_key`；
- 算子完全没有签名 → 返回零 hash 的 key，`call_key` 接受它；
- 并列 → `Err(OpError::Ambiguous)`，**不会**给出不可执行的零 hash key；
- 没有任何签名接受这些参数 → 也返回 `Err(OpError::Ambiguous)`（`count` 是
  签名总数）；它不重新推导单个签名的拒绝理由。

所以 `resolve_overload` 的返回值总是可以安全地交给 `call_key`。

`OpKey.signature_hash` 是 `Signature::hash()` 的前 16 字节（确定性 bincode
的 BLAKE3），伪造/过期/参数偏离的 key 返回 `UnknownOp`。

### 13.4.2 签名覆盖

覆盖率是 100%：本次测量 1382 个算子都有签名（`0 unexplained`，32 个
rule set）。签名有四个来源，优先级从高到低：

1. `Declared`：`OpDescriptor::new(...).signature(...)`，算子自己写（7 个）；
2. `Overlay`：`signatures.overlay.json` 里人工写的修正（25 个；在 RSSN 侧
   并入 manifest，Aevia 不持有副本）；
3. `Derived`：`kernel::infer::derive` 从注册信息推出（1350 个）；
4. `None`：叶子/无调用形式，`dynamic_reason` 说明原因（标准集没有）。

规则：

- `Derived` 是**推导**，不是猜测：它只读 `eval`/`PREDICATE`/`Binder`/
  `Variadic`/`LEAF`，不读算子名；所以它是“保守但可靠”的。
- 精度不够时写 overlay，而不是改推导器。overlay 只应该出现在“这条签名
  必须精确”的算子上（张量、符号、单位参数）。
- CI 的 `signature-audit` 报告覆盖率与来源分布；`Derived` 占比高不阻止
  构建，但阻止把对应算子标成“精确”。
- 插件用 `Registry::register_op` 声明签名；`Registry` 拒绝“既无签名又无
  `dynamic_reason`”的注册。

这保证“能力完整”和“类型精度”可以分开推进，但不会有算子被静默遗忘。

---

## 13.5 插件与用户扩展

### 13.5.1 插件

插件不是新语法，而是**普通 Aevia 包 + 清单标记 + 属性**：

```toml
# aevia.toml
[plugin]
kind = "aevia"          # 进程内；对应 RSSN 的 PluginKind::InProcess
abi = 1
```

`kind` 的两个取值对应 RSSN 的 `PluginKind`：

| `[plugin] kind` | RSSN | 加载方式 |
|---|---|---|
| `"aevia"` / `"in-process"` | `PluginKind::InProcess` | `Session::register(Box<dyn Plugin>)` |
| `"cdylib"` | `PluginKind::Cdylib` | `Session::load_plugin(path)`（需 `plugin-cdylib`） |

`cdylib` 的 C ABI 是**字节缓冲**：入口符号 `rssn_plugin_v1`，参数与返回值
都是 bincode 编码的 `Vec<Value>`/`Result<Value, String>`，不是
`#[repr(C)]` 的 `Value` 镜像。`cdylib::load` 会校验 `PLUGIN_ABI` 和
`RssnPluginDesc`；`Session::load_plugin` = `load` + `register`。

这个模块由 `plugin-cdylib` feature 控制（默认关闭，`full` 才包含），因为
加载动态库会在宿主进程里执行任意代码、没有沙箱。Aevia 在加载前必须查
`manifest.features.plugin_kinds`：没有 `"cdylib"` 时报
`E1001("core.plugin.cdylib")`，而不是让加载器抛一个底层错误。

```aevia
module MyDomain

@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))

@rule("softplus(x) => ln(1 + exp(x))")
let softplus_rule = ()

dimension MyDim
unit myunit : Length = 0.5 m
```

编译后注册到 RSSN。Aevia 的 `Rssn.register` 是 `Session::register(Box<dyn Plugin>)` 的包装；
`Rssn.register_plugin` 是同一函数的别名：

```aevia
Rssn.register(plugin)          # -> Result<PluginId, PluginError>
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

`@op("mydomain/softplus")` 里的 `mydomain/` 是**算子名的一部分**（插件
命名空间），不是 `rule_set/name` 限定符；运行时 `call_value` 仍按完整
字符串查找，不能拆成 `rule_set` + `name`。

### 13.5.3 安全

- 插件必须签名；默认只加载 registry 信任的插件。
- 插件声明 capabilities（`fs`、`net`、`unsafe`）；未声明的能力在沙箱中
  被拒绝。
- 插件 ABI 版本不匹配报 `E1006`。

---

## 13.6 量纲与单位：统一到 `rssn::dim`

### 13.6.1 规范层

`rssn::dim` 提供：

- `DimTerm`：基量纲/变量的有理指数乘积，结构化访问
  `factors(&arena)`/`vars(&arena)`；
- `DimArena`：基量纲/变量 interning，可快照/恢复；
- `UnitRegistry`：单位名、别名、精确尺度 `Scale { rat, pi }`、仿射
  offset、前缀、显示、类别、since；
- `Dimension`：**可打印快照**，指数是字符串；精确比较/计算用 `DimTerm`，
  不要解析 `Dimension.factors`。

Aevia 的 `Dim` 是 `DimTerm` 的类型级镜像；两者的规范形、基名、指数运算
必须一致。编译器不复制单位表；`Units.*` 是 `manifest.units` 的生成视图
（`UnitValue` 已带别名/前缀/类别/since，不必再读 `UnitRegistry`）。本次
测量 `manifest.units` 有 82 项。单位属于 **session**：
`Session::register_unit(UnitDef)` 写该 session 的 registry，
`KernelManifest::build_with_units` 才把它写进 manifest；进程级
`KernelManifest::build` 看到的是进程表。插件注册的单位只属于安装插件的
session。

结构化入口是 `Config::dimension_term(symbol, DimExpr)`：它不解析文本，未知
基名会被 intern，结构化假设在文本假设之后应用（同名时结构化胜）。
`DimExpr::to_term` 写 term；`DimExpr::try_from_term` 是 term 可表示量纲的
精确逆（`Length^n` 读回 `Length^n`）；`from_term` 是全函数版，读不回时给
`DimExpr::Any` 而不是错误量纲。

`OpInfo.dim_rule` 的 `DimRuleInfo` 本轮新增 `RootOf { degree, index }`：
结果量纲是**第 `index` 个参数**量纲的 `degree` 次根。旧 `Root(n)` 固定取
第 0 个参数的根，表达不了 `sde_wiener(seed, m, steps, h)` 的
`sqrt(h)`（`h` 是第 4 个参数）；它的 manifest wire form 是
`{"root_of":{"degree":2,"index":3}}`。宿主读 `dim_rule` 时应按变体匹配，
不要假设只有 `Root`/`Same`/`Product`。

自定义基量纲由 `Config::base_dimension(name)` 声明（幂等）：它把基名送进
`DimArena`，`Dim` 的内部表示从固定的 7 个 SI 指数扩成“7 个 SI + 有序
旁表”，`expr_to_dim` 接受任意基名。这是 `unit EUR : Currency` 能成立的
前提——`dimension_term` 只能为 arena 里已有的基名产出 `DimTerm`。

### 13.6.2 单位声明

```aevia
dimension Currency            # 自定义基量纲
unit USD : Currency = 1       # 基单位
unit EUR : Currency = 1.08 USD
unit myunit : Length = 0.5 m  # 内置基量纲
```

生成器为每个 `dimension` 在 `Config` 上声明一次
`base_dimension("<Name>")`，然后才注册单位；否则单位注册成功但 `DimTerm`
无法生成（arena 里没有该基名）。

- `dimension` 注册到 `DimArena`（编译期经 `DimExpr`，运行时经
  `Config::dimension_term` / `Graph::assume_dimension_value`）；`unit`
  注册到 `UnitRegistry`。
- 默认导出；`private` 只在本模块可见。
- 导出信息进入 manifest；下游包可以直接 `import` 并使用。
- 用户单位可以是仿射的（`unit °X : Temperature = 1 K offset 273.15`），
  但仿射单位只能单独使用。
- 自定义基量纲：`Config::base_dimension` 声明基名，`expr_to_dim` 接受任意
  基名，`Dim` 用旁表保存非 SI 指数；所以 `dimension Currency` +
  `unit EUR : Currency` 都可用。`features.user_base_dimensions` 是
  **硬编码 true**（不是 feature 门控），所以每个构建都支持；唯一按构建
  变化的是 `jit*` 与 `plugin_kinds`。
- 基名经 `Config::base_dimension(name)` 声明即可：`prepare` 在**克隆的**
  图上 `define_base_dimension`；`DimArena` 是写时复制，第一次写就 detach，
  所以不会污染模板或并发请求（§13.6.6）。

### 13.6.3 `Dyn`

`Dyn<ρ>` 的运行时量纲是 `DimTerm`，不是字符串。`dyn`/`check` 是类型指导
原语：具体 `d` 在编译期物化为 `DimTerm`；泛型用 `dyn_with`/`check_with`
显式传入。`check` 返回 `Result`。

### 13.6.4 `Point` 与仿射

`Point[d]` 是静态包装；运行时是 `Float` + `UnitId`。`UnitRegistry` 的
`affine`/`offset` 是唯一事实来源。`Point` 的算术规则在 Aevia 层实现，
RSSN 的 `quantity`/`convert` 负责显示/换算。

### 13.6.5 物理常数

`manifest.constants` 是 `Vec<ConstantValue>`（30 条、18 条精确）：

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

`Units.Constants` 由它生成，规则：

- 符号原样使用（`c`、`g_n`、`h`、`hbar`、`k_B`、`N_A`、`e`、`G`、
  `alpha`、`R`、`F`、`sigma_SB`、`m_e`、`m_p`、`m_n`、`a_0`、`R_inf`、
  `mu_0`、`epsilon_0`、`mu_B`、`M_u`、`m_u`、`T_tp`、`atm`、`G_0`、`K_J`、
  `R_K`、`Phi_0`、`K_cd`、`dnu_Cs`）；
- `exact = true` 的常数没有 `uncertainty`；`exact = false` 的有；
- 常数是**值**，不是单位：不能出现在 `quantity(v, u)` 的单位位置；
- `e` 是基本电荷，与 `Math.e` 同名。`open Units.Constants` 与 `Math.e`
  同时可见时，`e` 必须限定（`Math.e` / `Units.Constants.e`）。

### 13.6.6 线程与 arena 隔离

`DimArena` 在模板里是 `Arc<RwLock>`，`Graph::clone` **共享**它；写入时
`with_dim_arena_mut`/`define_base_dimension` 先看 `Arc::strong_count`，大于 1
就克隆一份再写（写时复制）。因此：

- `Config::base_dimension(name)` 是安全路径：`prepare` 在克隆图上
  `define_base_dimension`，第一次写就 detach，请求之间互不影响；
- 不定义任何基量纲的请求共享模板的 arena，不付拷贝成本；
- 不要直接对 `Session` 的模板图调用 `define_base_dimension`——那会让后续
  请求都看到这个基名；
- 跨线程仍用 `Session::fork`/`merge`，不共享 `Session`（它是 `RefCell`）。

---

## 13.7 中心包 registry

Aevia 现在就有中心 registry 的设计，不把它推到“以后”。registry 是
**内容寻址 + 签名 + 能力声明**的包索引。

> **实现状态（2026-11-04）**：**设计已完成，实现尚未开始。** 当前包管理只
> 支持 **path 依赖**（`aevia.toml` 的 `[dependencies]` + `aevia.lock`）；
> registry 依赖、git 依赖和 `publish`/`yank`/`aevia add <version>` 都会报
> `E0605`（`crates/aevia-core/src/manifest.rs` 明确“there is no registry”）。
> 本节描述的是待实现的协议，不是可用功能；`spec/09 §9.11` 的状态说明与
> 本节一致。

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

后端能力用 capability 名描述。**名字的唯一来源是一张 capability 表**：
它由 Kernel Manifest 的 `Features` 派生，`Rssn.Capabilities` 只列出
`enabled = true` 的行（`aevia-rssn` 的 `Capabilities::table()`）。当前构建
的完整表（本次测量）：

```
core.dynamic_call
core.jit                    # false：默认构建无 jit feature
core.jit_branch             # false（同上）
core.jit_index              # false（同上）
core.parallel_symbolic
core.higher_order
core.structured_results
core.sde                   # 由 features.sde 派生；SDE 噪声原语可用
core.callback              # false：Aevia 闭包不是 Send + Sync
core.plugin.source         # false：源码级 @op 打包未实现
core.plugin
core.plugin.in-process
core.plugin.cdylib         # 仅 plugin-cdylib 构建
dim.exact_units
dim.affine_units
dim.user_base_dimensions
```

本构建 `Rssn.Capabilities` 实际列出的 enabled 行（2026-11-04 实测）：
`core.dynamic_call`、`core.parallel_symbolic`、`core.higher_order`、
`core.sde`、`core.structured_results`、`dim.exact_units`、
`dim.affine_units`、`dim.user_base_dimensions`、`core.plugin`、
`core.plugin.in-process`——jit 三行与两个 host-bridge 行是 `false`，不会出现
在 `Rssn.Capabilities` 里。

另有**规范声明、但当前 manifest 没有对应 `Features` 字段**的名字；它们可以
出现在诊断里，`Rssn.Capabilities` 不会列出：`core.symbolic`、`core.numeric`、
`core.tensor`、`core.linalg`、`core.ode`、`core.pde`、`core.fft`、
`core.stats`、`core.units`、`core.assume.general`、`core.gpu`、
`core.distributed`、`io.binary`，以及**存储类稀疏**的命名缺口
`storage.sparse`（§13.9.5：v1 用 CSR 记录，不是运行时存储类）。

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
- 张量字面量自动获得细化形状；`zeros_of<[3,4]>()` 等构造器可显式指定。
- `matmul : Tensor<[m,k],τ> × Tensor<[k,n],τ> -> Tensor<[m,n],τ>`；
  秩-only 版本保留，运行时检查。
- `reshape<[n1, ..., nk]>(a)` 始终返回 `Result`；形状静态已知时元素数在
  编译期检查、结果恒为 `Ok`，否则运行时检查。`reshape_dyn` 总是运行时检查。
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
- 动态 CSV（未知 schema）返回 `Table<Dynamic>`；`Dynamic` 是特殊记录类型，
  不是普通字段可投影的行。`to_table<ρ>` 做运行时校验。
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

> **实现状态（2026-11-04）**：`Device` 值、`to_device`/`on`/`device_of` 已
> 类型化；没有 `core.gpu` 时 `to_device`/`on` 报 `E1001("core.gpu")`，与
> 设计一致。`@jit` 当前**只接受零参数**（`@jit(target = …)` 报
> `E0210("jit takes no arguments")`），JIT 目标选择尚未实现；`Gpu`/
> `Distributed` 也没有后端，只是可构造的运行时值。

### 13.9.5 稀疏与布局

**决定：`Sparse<F, shape, τ>` 是核心类型构造器，`F` 是格式类型。**

- 格式：`Csr`、`Csc`、`Coo`、`Block`、`Bcsr`；当前是封闭集合，用户
  自定义格式要等 RSSN 开放 `Format` kind（当前插件只能注册算子/单位）。
- `Layout` 类（设计上）：`nnz`、`to_dense`、`from_dense`、`matmul`、
  `transpose`；当前实现是内建函数，不是可实例化的类型类。
- `@` 按布局分派：dense×sparse、sparse×dense、sparse×sparse。
- `Sparse` 与 `Tensor` 之间显式转换；不隐式互转。RSSN 的 `TypeExpr::Sparse`
  只有 `rank`，所以 Aevia 的 `Sparse<F,shape,τ>` 在边界降为 rank + 运行时
  shape；细化形状由 Aevia 自己检查。
- 稀疏不是库补丁，而是类型系统的一部分；`shape` 细化同样适用。

> **实现状态（2026-11-05）**：`Sparse` 类型构造器与 `from_dense`/`to_dense`/
> `nnz` 已实现（CSR **记录**往返，stdlib 测试覆盖）；**v1 没有运行时
> `Storage::Sparse` 值类型**——`from_dense` 返回 CSR 字段的记录，checker 的
> `TyKind::Sparse` 是类型层镜像；kernel 的 `Value::Sparse` 过桥时被 densify。
> `matmul`/`transpose` 的稀疏版本与 `@` 的 dense/sparse 布局分派**尚未
> 实现**，以命名 capability gap **`storage.sparse`** 记录（不是静默遗漏；
> 本轮的 scoping 见 `.workbuddy-ai/design/storage-sparse-scope.md`）；当前
> `@` 只分派 dense 张量。格式仍是封闭集合。在真正的存储类/算法落地前，
> 不要声称运行时存在 `Sparse` 值类型。

### 13.9.6 效果与能力

**决定：不做用户自定义 effect handlers；`IO` 保持单一类型。**

- 扩展点是**算子/插件/registry**，不是语言级效果系统。
- `IO` 动作可以声明所需 **capability**（`fs`、`net`、`env`、`rand`、
  `gpu`）；这是运行时权限，不是类型参数。
- `with_cap(cap, thunk)` 在沙箱中执行；没有能力时 IO 动作失败。
- 这样避免 effect row 对全部标准库签名的侵入，也避免“v2 才有自定义
  效果”的断层。需要 DSL/代码生成时用插件/算子，不用宏。

> **实现状态（2026-11-04）**：`with_cap` 与 fs/net/env/rand/gpu 的运行时
> 权限检查**尚未实现**；当前可用的 capability 判定是 §13.8.2 那张由 kernel
> `Features` 派生的表（`core.*`/`dim.*`），它只控制后端能力（如 `core.jit`、
> `core.sde`），不控制文件/网络访问。§9.14 的沙箱承诺是设计目标。

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
| 中心 registry？ | 是；设计完成，实现待做（当前只有 path 依赖，§13.7） |
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
| `E1001` | 后端 capability 缺失（`core.gpu`、`core.jit_branch` 等） |
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

dimension_decl ::= [ "private" ] "dimension" UIDENT [ "=" dim ] NEWLINE

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

match_expr     ::= "match" expr "with" match_body
match_body     ::= NEWLINE INDENT { match_arm } DEDENT
                 | NEWLINE { match_arm }          # 分支与 match 同列
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

`:` 切片（§6.6.4.1）的第三个分量是整数步长：`a[i:j:s]`。`::` 是 cons 运算
符，所以起点之后的 `::` 不是两个切片冒号——`xs[1::2]` 是 `xs[1 :: 2]`；省略
终点又要给步长时，起点必须省略（`xs[::2]`）或终点必须写出（`xs[1:5:2]`）。

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
| `E0108` | expected a unit after numeric literal | 写 `3 * x` 或补单位；`3 (2)` 的括号内必须是单位表达式 |

## B.4 语法错误（E02xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0201` | empty block | 加一个表达式 |
| `E0202` | syntax error | 检查附近记号 |
| `E0203` | ragged tensor literal | 使每层等长 |
| `E0204` | invalid pattern | 检查构造器/字段 |
| `E0205` | invalid operator section | 用 `(+)` 或 lambda |
| `E0206` | reserved word used as identifier | 改名 |
| `E0207` | ambiguous type application | 在 `<` 前加空格或给 callee 加括号 |
| `E0208` | duplicate field | 删除重复字段 |
| `E0209` | invalid unit literal | 检查单位表达式 |
| `E0210` | unknown attribute | 检查属性名/参数 |

## B.5 类型错误（E03xx）

| 代码 | 消息 | 典型修复 |
|---|---|---|
| `E0301` | unbound name | 导入/拼写 |
| `E0302` | type mismatch | 检查标注/参数；类型级 `Nat` 方程无解也报此码 |
| `E0303` | infinite type (occurs check) | 加标注/改递归 |
| `E0304` | ambiguous type | 加类型标注；未能解出的 `Nat` 变量、非线性 `Nat` 方程、无 binder 的 shape 变量也报此码 |
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
| `E0408` | unsatisfiable dimension constraint | 检查公式（保留码：当前量纲代数是自由阿贝尔群上的指数线性统一，非零变量残差总可解，故实现中不可达——见 `diagnostics_coverage` 的证明） |
| `E0409` | dimension occurs check | 简化量纲（保留码：当前统一不会产生 occurs 状态，实现中不可达——同上） |
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
| `E0502` | unit not imported | `open Units.SI`（保留码：当前实现单位表全局、没有模块级单位命名空间） |
| `E0503` | prefix cannot be applied | 用显式单位 |
| `E0504` | affine offset dimension mismatch | 检查 offset |
| `E0505` | duplicate base unit | 删除重复 |
| `E0506` | unit definition cycle/scale not dimensionless | 检查定义 |
| `E0507` | prefix unit conflicts with explicit unit | 改名/删除（保留码：同 E0502 的模块级命名空间缺口） |
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
| `W0602` | deprecated name used: `@deprecated("...")` on the declaration (§2.2.7/§13.8.1) |

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
    conditions` 改为 `Tensor<1, String>`（Revision 6 又改为
    `Tensor<1, Condition>`）；`read_csv` 统一 `DataError`。
12. **JIT 触发条件去掉 `if`/索引**；与后端缺口一致。

## Revision 4 — 可扩展性与 RSSN 集成

这一轮不再把问题留到“v2”，而是把长期可维护性写进语言契约：

1. **反射优先的算子集成。** Aevia 不维护算子绑定表；读取 RSSN Kernel
   Manifest，生成 `Rssn` 模块。有签名 → 类型化；无签名 → `Rssn.dynamic`。
   1000+ 算子从第一天全部可见；新算子不需要改 Aevia 编译器。
2. **动态 `Value`/`call` ABI。** `Rssn.call`/`Rssn.dynamic` 是正式能力，
   不是降级路径；`OpError` 结构化。RSSN 只选注册、不校验签名；类型/
   量纲检查由 Aevia 静态检查器或 `Rssn.typed` 负责。
3. **签名/重载/稳定键。** `OpKey { rule_set, name, signature_hash }`；
   `Signature`、`overloads`、`Stability`、`Since`、`Doc` 进入 manifest；
   CI 审计“每个算子有签名或 dynamic_reason”。
4. **插件/宿主回调。** `@op` 把 Aevia 函数注册为 RSSN 算子；
   插件可注册算子/重写/单位/量纲/JIT lowering。
5. **量纲/单位统一。** 抽出 `rssn::dim`：`DimTerm`/`DimArena`/`UnitRegistry`；
   精确 `Scale { rat, pi }`、仿射 offset、用户单位/基量纲注册；Aevia 的
   `Units.*` 是 `manifest.units` 的生成视图（Revision 6 起），不再有第二张表。
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

### 仍存在的风险（Revision 5 时点）

- `compute` 的深拷贝（以及当时 `DimArena` 的共享）仍需 RSSN 优化；可用
  批处理规避。（Revision 7 起 `Graph::clone` 已深拷贝 arena。）
- `Solutions.multiplicities` 空表示“未报告”，不是“全 1”；`complete`
  目前硬编码 true，不能当证明。
- `Point` 的表示是 Float + UnitId；符号绝对温度仍用温差/`Dyn`。
- 形式化语义/证明尚未做；用 conformance/属性测试/与 rssn 交叉检查。
- GPU/分布式/稀疏的**后端实现**可能晚于语言设计；缺失时报 capability，
  不删语法。
- `Term` 替换/自由符号、`fork/merge`、结构化结果已在 0.2.13 实现；
  Revision 4 的对应风险已关闭。


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

## Revision 5 — 对齐 RSSN 0.2.13 实际实现

RSSN 的适配已经落地，这一轮把规范从“设计契约”改成“实际 API 契约”：

1. **Manifest 实际字段**：`KernelManifest` 有 `features`/`rule_sets`/`ops`/
   `units: Vec<UnitValue>`/`base_dimensions`/`plugins`；`manifest_hash` 是
   确定性 bincode 的 BLAKE3，JSON 只供人读。
2. **实际入口名**：`Session::manifest() -> Arc`、`call_value`、`call_ho`、
   `register(Box<dyn Plugin>)`；`OpInfo.keys`（不是 `key`）；`Value::Symbol`
   （不是 `VString`）；`TypeExpr::Dim(Box<TypeExpr>, DimExpr)`（不是
   `Real(dim)`）。
3. **实际覆盖**：rssn 0.2.13 有 1369 个算子、30 个 rule set、799 有签名
   （58.4%，其中 7 declared / 792 inferred）、570 个有 `dynamic_reason`、
   0 unexplained；Aevia 的可信 typed façade 只有 7 个，其余 1362 个走
   `Rssn.dynamic`。（这是 Revision 5 时点的数字；Revision 7 起 1369 个
   全部有签名。）
4. **量纲/单位**：`rssn::dim` 公开 `DimTerm`/`DimArena`/`Dimension`/
   `UnitRegistry`/`UnitDef`/`Scale`；`DimTerm::factors(&arena)` 返回
   `(String, BigRational)`；`Dimension` 仍是可打印字符串快照。
5. **Term/线程**：`Term` 借 `&Session`，`TermRef` 是 owned handle；
   `children`/`substitute`/`free_symbols`/`to_value`/`handle` 已实现；
   `fork/merge` 已实现。
6. **结构化结果**：`Solutions` 的 `multiplicities` 空表示“未报告”，不是
   “全 1”；`OdeSolutionView`/`SimResult` 已实现。
7. **JIT**：`Select`/`Extern1`/`Gather` 已 lowering；默认构建
   `features.jit=false`，`@jit` 必须报 `E1001("core.jit")`。
8. **当时唯一的 P0 构建阻塞**：`signatures.overlay.json` 被 `.gitignore`
   的 `**/*.json` 排除，干净 clone 下 `include_str!` 失败。（Revision 6
   时已由 RSSN 修复。）
9. **章节更新**：13 章、10.8、08.6/8.11/8.18/8.19、README 与根目录
   `RSSN-INTEGRATION-REQUIREMENTS.md` 已按实际 API/数值重写。

## Revision 6 — 对齐 RSSN `0852123a`（重载解析、Value 边界、单位扩充）

RSSN 上游又推进了一轮，这一版把规范里“等 RSSN 支持”的几处**关闭**，并把
新增的契约写进去。审计：`rssn 0.2.13`、`api 1`、1369 算子 / 30 rule set /
799 有签名（7 declared、792 inferred）/ 570 explained / 0 unexplained /
75 单位 / 7 基量纲。（Revision 7 起改为 1369 全部有签名。）`manifest_hash` 的 32 字节十六进制以
`ee077387a6aba5e4` 开头；`signature-audit` 显示的
`e4a5aba6877307ee` 是它的 `hash_u64`（小端），不是前缀。

1. **重载解析落地。** 新增 `kernel::overload`：`Match` 格
   （`Exact > Widened > Shaped > Generic > No`）、`score`、`best`；
   `Session::resolve_overload`/`call_key`/`overloads`；`OpError::Ambiguous`。
   规范 13.4.1 从“运行时只按 arity”改成“静态解析与 RSSN 的格同构，typed
   调用用 `call_key` 钉死”。`E1002` 现在有真实的运行时来源。
2. **Value 边界成为契约。** `manifest.value_kinds: [ValueKindInfo]` 给出每个
   `Value` 变体的 `in_term_graph` 与替代做法；13.3.1 改为“读 manifest，
   不维护第二份”。
3. **单位表扩充到 75 项并带元数据。** `UnitValue` 新增
   `aliases`/`prefixable`/`category`/`since`；天文（`au/ly/pc/Rsun/Msun/
   Mearth/jup`）与 CGS（`dyn/erg/Gal/P/St/G/Mx/Ba/Oe`）已注册，
   `Units.Astronomy`/`Units.CGS` 可从 manifest 生成；CODATA 常数仍缺。
4. **结构化量纲入口。** `Config::dimension_term`、`Graph::with_dim_arena`、
   `Graph::assume_dimension_value`；未知基名（`Currency`）在符号路径上可用。
   `dimension` 可用，`unit` 仍受限（`expr_to_dim` 只认 7 个内置基名）。
5. **`TypeExpr::Symbol`。** 自由符号不再是 `Var("Symbol")`；03 章补充
   `Symbol ≡ Real[δ]`（`δ` 弱）的对应规则。
6. **结构化结果语义修正。** `Solutions.complete` 不再硬编码 true：
   返回 `list(...)` 才 true，单根为 false；`OdeSolutionView.dimensions`
   写入 record。
7. **构建阻塞关闭。** `signatures.overlay.json` 已入库，`build.rs` 在缺失
   时写空 overlay 到 `$OUT_DIR`；干净 clone 可构建。
8. **章节更新**：13（manifest/value_kinds/overload/call_key/units）、
   10.8（剩余缺口重排）、08.6/8.19（Solutions/Rssn API）、05.3（单位
   状态）、03.3（Symbol）、README 与根目录
   `RSSN-INTEGRATION-REQUIREMENTS.md`。
9. **重载的“实际生效范围”写清楚。** 当时标准集 1369 个名字各只有 1 个
   注册、至多 1 个 overload，`score`/`best` 不执行；`call_value` 不校验
   签名，且无候选匹配时会退回第一个 arity 匹配。`resolve_overload` 非
   `Best::Unique` 时返回零 hash 的不可执行 key；`Kernel::call(&OpKey)`
   只按 name 分发。（后两条在 Revision 8 被上游改掉：`resolve_overload`
   并列报 `Ambiguous`，`Kernel::call` 转发 `call_key`。）这三条写进
   13.4.1 与根文档 §1.3/§3.3.1。
   （Revision 7 起已变：每个注册 1–3 条重载，`best` 在注册内执行。）
10. **`Solutions.conditions` 改为 `Tensor<1, Condition>`**，`Condition`
    是 `{ kind, text, expression? }`；`SimResult` 记录含 `schema` 字段。
11. **单位表以 `manifest.units` 为唯一来源**（不是 `UnitRegistry` 的视图）；
    `UnitValue` 带 `aliases`/`prefixable`/`category`/`since`。
12. **`manifest_hash` 的两种显示**：32 字节十六进制以
    `ee077387a6aba5e4` 开头；`signature-audit` 的
    `e4a5aba6877307ee` 是 `hash_u64`（小端），不能当哈希前缀 pin。

## Revision 7 — 对齐 RSSN `111b71d0`（100% 签名、常数、自定义基量纲、cdylib）

这一轮上游把上一版剩下的三个大缺口都补了，规范随之从“7 个可信 typed +
动态兜底”改成“全部 typed + 精度分档”。

1. **签名覆盖 100%。** `SignatureSource::Inferred` 改名为 `Derived`，
   语义也变了：它不再是从名字猜，而是从注册信息推——有 `eval` →
   `(Real,…) -> Real`（谓词 `-> Bool`），非叶子另加 `(Term,…) -> Term`，
   `Binder` 位置是 `Symbol`，变长参数是 `Variadic`。1369 个算子全部有
   签名：7 declared + 13 overlay + 1349 derived。
2. **精度分档写进规范。** “全部 typed”不等于“全部精确”：只有 20 个是
   精确的，1349 个是形态级（不区分 `Tensor`/`Quantity`/`Int`）。生成器必须
   标注来源，IDE 把 `Derived` 显示为“保守推断”。13.2.2 与 10.8 都按这个
   重写。
3. **重载真正生效。** 有 `eval` 的算子现在有 2 条重载（数值/符号），
   177 个算子双重重载、2 个三重重载；`resolve_overload` 会按参数类型选。
   注册内并列被它吞成零 hash key，`OpError::Ambiguous` 仍只由多注册
   （插件）触发。`call_value` 仍是注册级 arity 选择，不校验签名。
4. **物理常数进 manifest。** `constants: [ConstantValue]`（30 条、18 条
   精确、12 条带不确定度）。05.8 的常数表按 manifest 符号重写
   （`e` 是基本电荷、`g_n` 是标准重力、`epsilon_0`/`mu_0` 是测量值）；
   `Units.Constants` 不再需要 Aevia 本地 overlay。
5. **自定义基量纲可用。** `Dim` 变成“7 个 SI 基 + 有序旁表”，
   `expr_to_dim` 接受任意基名，`Config::base_dimension(name)` 把基名送进
   `DimArena`；`unit EUR : Currency` 不再是 `E1001`。规范要求经 `Config`
   声明，`prepare` 在克隆图上定义（`Graph::clone` 深拷贝 arena）。
6. **cdylib 插件 ABI。** `PluginKind { InProcess, Cdylib }`，
   `kernel::cdylib`（`rssn_plugin_v1`、bincode 字节缓冲、`RssnBuf`/
   `RssnHost`/`RssnOpEntry`/`RssnPluginDesc`），`Session::load_plugin`，
   `plugin-cdylib` feature 控制；`features.plugin_kinds` 让宿主在加载前
   拒绝。13.5.1 补了 ABI 表。
7. **移除已关闭的缺口**：10.8 删掉 CODATA 缺失、自定义基量纲注册失败、
   cdylib 未实现；13.6 删掉 `E1001("core.units.custom_base")`；
   新增 13.6.5（常数）与 13.6.6（线程/arena）。
8. **`manifest_hash` 更新**为 `9f1613fde0f150ce…`（显示值
   `ce50f1e0fd13169f` 是 `hash_u64` 小端）。（Revision 8 起又是
   `98fd35fa…` / `f03babb1fa35fd98`。）

## Revision 8 — 对齐 RSSN `95099fa0`（动态路径收口、COW arena、ODE 事件）

这一轮上游没有改语言契约，但把上一版剩下的四个“运行时不可靠”点收口了，
其中两个直接推翻 Revision 7 的结论。

1. **`call_value` 现在校验参数。** `resolve_op` 调用 `check_args`：没有
   任何 overload 接受参数时返回 `OpError::Type`，并指出第一个不被接受的
   参数；只有“所有候选都没有签名”时才放行。所以 dynamic 路径不再是
   “类型错误静默执行”。
2. **`Kernel::call(&OpKey, …)` 改为转发 `call_key`。** 它现在**钉死**重载，
   不再只按 `key.name` 分发。Revision 7 里“trait 会丢钉死”的说法作废。
3. **`resolve_overload` 并列时返回 `OpError::Ambiguous`。** 它不再返回
   零 hash 的不可执行 key；零 hash 只留给“完全没有签名”的算子。因此它的
   返回值总是可以安全交给 `call_key`。Revision 7 里“被静默吞成零 hash”
   的说法作废。
4. **`Solutions::new` 默认 `complete = false`**，新增 `.complete()` 构造器。
   “未报告”和“已穷尽”现在是两个显式状态。
5. **`KernelManifest::hash_hex()`** 给出 64 个 hex 字符，宿主应该 pin 它；
   `hash_u64()` 只是前 8 字节小端。Revision 7 的“pin 32 字节哈希”有了
   正式 API。
6. **`DimArena` 改为写时复制。** `Graph::clone` 共享 arena，
   `with_dim_arena_mut`/`define_base_dimension` 写入前 detach；
   `Graph::with_dim_arena` 同时改名 `with_dim_arena_mut`（`&mut self`）。
   Revision 7 的“深拷贝”描述作废，但隔离结论仍成立。
7. **符号指数桥接打通。** `DimExpr::to_term` 把 `Length^n` intern 成原子
   （`DimTerm::base_symbolic`/`var_symbolic`）。Aevia 的**语言**仍拒绝静态
   符号指数（§4.12.3），04 章补了一句“桥接能表示 ≠ 语言该支持”。
8. **ODE 事件覆盖扩展。** `Dopri5`/`Radau5`/`BDF`/`Rosenbrock23` 与定步长
   （`solve_fixed_events`）都能定位事件；10.8 的第 6 条从“只覆盖部分方法”
   改成“方法差异只剩定步长需要 `steps`”。
9. **`manifest_hash` 更新**为 `98fd35fab1ab3bf0…`（`hash_u64`
   `f03babb1fa35fd98`）。

## Revision 9 — 对齐 RSSN 3.0.0-rc.1、并发核验与派发/扩展边界（2026-11-04）

这一轮做三件事：把规范从“0.2.13 契约”升级到**实测的 3.0.0-rc.1**；把上一轮
审计后仍与实现不符的条目改正（含以核验结果为准的规范修正）；把“派发/
扩展边界”这一长期含糊的设计写成明确的规范决定。原则不变：**不删语言
特性**，后端/实现缺失用 capability 诊断；不以“v2”推脱。

### 1. 后端对齐：rssn 3.0.0-rc.1（实测）

- `rssn 3.0.0-rc.1`、`api_version = 1`；**1379 个算子 / 31 个 rule set**；
  签名来源 **7 declared + 22 overlay + 1350 derived**（1565 条 overload：
  1198×1、178×2、2×3、1×5）；**82 个单位**（本轮新增 7 个，见 §5）；
  30 条常数（18 条精确）；value kinds 17 个（10 个可进项图）；
  `SCENARIOS` 29 个（含 `ising_wolff`）；`features.jit = false`。
- `manifest_hash.hash_hex()` =
  `1920638b60fbdae5717552cf577fb41690473129868aebce5de02ae7580c04d9`。
  宿主 pin 64 个 hex 字符；`hash_u64()` 只是显示便利。**不再 pin 任何
  计数/哈希常量**：Aevia 的 conformance 测试直接读活的 manifest（算子数、
  来源分布、单位表、常数表、能力标志、哈希），manifest 变化时以测试失败
  提示重新生成，而不是让文档与代码各自漂移。
- overlay 从 13 个算子扩到 **22**：原 13 个（`diff`/`solve`/`taylor`/
  `quantity`/`convert`/`det`/`trace`/`norm`/`transpose`/`rank`/`dot`/
  `cross`/`identity`）加 `and`/`or`/`xor`/`not`/`implies`/`iff`/`fftconv`/
  `interpolate`/`sum`。**精确签名 29 条**（7+22），其余 1350 条是形态级
  `Derived`。
- 这期间上游收口的接口事实（详见根目录 `RSSN-INTEGRATION-REQUIREMENTS.md`
  的实测段落）：重载只有**一条解析路径**（`call_value` 校验实参；
  `resolve_overload` 只返回可执行 key、并列报 `Ambiguous`；`call_key` 不再
  重解析且会校验钉死的 overload；`Kernel::call` 转发 `call_key`）；
  `Session::register_unit` 与 `KernelManifest::build_with_units`（单位随
  session 拥有，插件单位不污染别的 session）；`score`/`score_types` 与
  `best`/`best_types` 共用同一折叠；`DimExpr::try_from_term` 是 `to_term`
  的精确逆，`from_term` 是全函数并回答 `Any` 而不是静默错误量纲；
  `ValueKind::Unit` 不再入项图；稀疏改用 faer（`sprs-rssn` 移除）；
  Cranelift 0.137（`jit` feature，默认关）；`Solutions` 记录、
  `CallCtx::substitutions`/trace 都已生效。
- **Aevia 侧不再维护 `signatures.overlay.json` 镜像**：bridge 直接消费
  manifest 携带的 overlay；仓库里没有第二份签名表。§4.1/§4.2 与 13.2 已按
  此改写。
- **JIT 语义**：默认构建 `features.jit = false`，因此 `@jit` 在检查期报
  `E1001("core.jit")`（不再是“被接受但不做事”）。JIT 的代码生成仍未实现，
  属已记录的实现缺口。

### 2. 规范修正（以实测行为为准）

- **§6.9.3 / §9.3 调用与资源界限**：非尾递归受**调用深度 2048**限制
  （`Interp::max_depth`），解释器线程栈由 `--max-stack <MiB>` 指定
  （默认 256 MiB，不是规范旧文的 8 MB）；不终止的尾循环由**连续尾调用
  预算**（默认 100,000,000，`--budget`）限制；表达式嵌套另有 16,384 节点
  上限。三者超限都是 `E0805` 并带调用栈。这是“合法程序终止、runaway 被
  报告”的保证在新尾调用实现下的精确形式。
- **§9.9 CLI**：`--max-stack` 的单位是 **MiB**，`--budget` 的单位是**尾
  调用次数**（不是秒）；`--release`/`--jobs` 在当前解释器实现中没有效果，
  CLI 明确提示而不是静默忽略；`--out`、`--locked`、`clean`、`lsp` 已实现。
  `--locked` 只校验锁文件，不涉及缓存。
- **§9.11 构建缓存**：当前实现**不写** `.aevi`/`.aevia/target` 接口缓存，
  每次编译重新检查全部模块；`clean` 诚实地删除可能存在的 `.aevia/` 并
  通常报告“没有可清理的内容”。增量编译是已记录的性能缺口（见 §9 末尾）。
- **§6.10 / §9.7.2 `par_map`**：语义保证（结果顺序与输入一致、线程数不影响
  结果）不变；**当前求值器单线程，`par_map` 顺序执行**，明确记录为实现
  偏差。真正的并行需要会话级并行或线程化求值器。
- **§1.8.1 单位字面量邻接**：补写“单位表达式内的 `*`/`·`/`/` 一直属于单位
  表达式，要把量乘以变量或函数调用必须先用括号结束单位字面量”，例如
  `(3.0 m/s) * exp(-t)`；`3 m * x` 会把 `m*x` 当单位表达式（`x` 不是单位时
  `E0501`）。这是既有规则的可执行化说明，不是新规则。
- **§11 完整示例**：修正了示例自身的错误——变量绑定用大写（按 A.4 那是
  零参构造器模式）；多语句 `match` 分支体需要 `do`（它是 IO 动作序列）；
  `(3 m)^n` 需要先绑定 `n : Int`；量的乘法需要括号结束单位字面量。示例的
  类型/量纲推导与输出同步修正。**注意**：`spec/11` 依赖标准库模块
  （`import Math { sqrt }`、`open Units.SI`），而 §8.20 的模块层级本轮尚未
  实现，导入仍是 `E0607`；§8.17 的 `Physics.*` 类型化包装器同样未实现。
  `spec/11`、`spec/08` 里已如实标注这一实现状态，不假装已可运行。
- **附录 B**：新增 **`W0602` deprecated name used**（`@deprecated` 使用点
  警告）。其余“已文档但尚未可触发”的代码继续列在 STATUS 的已知缺口里，
  不在规范里假装可达。

### 3. 本轮新增的语言与运行时能力

- **类型类之外的 `Dyn` 检查器**（§3.11/§4.9）：`dyn`/`check`/`dyn_with`/
  `check_with`/`dim_of`/`value_of`/`pow_dyn`、`DimValue`、`DimError` 与
  `Dyn` 的 `+`/`-`/`*`/`/`/`^`/`sqrt` 规则全部有检查器签名；`dyn`/`check`
  在调用点把具体量纲物化并记录（`dyn_calls`），运行期据此做 `E0412` 校验；
  `Dyn` 与 `Scalar` 之间没有隐式转换（显式 `check`）。`Show`/`Eq` 对
  `Dyn` 可用（§4.9.4）。
- **属性成为真语义**（§2.2.7/§9.6.1/§13.8.1）：`@jit` 按 `core.jit`
  门控；`@stable`/`@experimental`/`@deprecated` 被记录，使用 deprecated 名字
  警告 `W0602`；`@op`/`@rule` 按 `core.plugin.source` 门控（本构建缺失，
  报命名 capability），不再静默无效。源码级插件包装（§13.5.2）仍未实现。
- **`Rssn` 动态接口的四个入口可达**（§8.19）：`Rssn.typed` 端到端可用
  （返回 `Result<Value, OpError>`，`TypeRep` 是不透明运行时类型，`from_value`
  做窄化——这是本实现的诚实静态形式，而不是虚假的 `Result<a, OpError>`）；
  `Rssn.call_ho` 的空回调表转发到内核、非空回调报
  `E1001("core.callback")`（Aevia 闭包不是 `Send+Sync`）；
  `Rssn.register`/`register_plugin` 报 `E1001("core.plugin.source")`
  （源码级插件打包未实现；宿主 Rust 插件走嵌入 API）。这两个 capability
  是本轮新增的“实现能力”名。
- **张量与单位的算术**（§3.5.3/§4.5.2）：`Tensor * Unit`、`Unit * Tensor`、
  `Tensor / Unit` 对字面量张量也成立（元素表示不被强制改变）；同秩张量的
  逐元素 `*`/`/` 与张量 `^` 字面量指数按规范实现。
- **ODE 的 `RateOf`**（§8.9）：`odeint`/`odeint_with`/`solve_ivp` 的右端
  按内建类型函数 `RateOf<s>` 检查（标量除以 T、张量/记录递归；`Int`/
  `Bool`/函数字段报 `E0302`）；右端必须是双参。`OdeOpts<s>` 目前只做记录
  字面量的字段检查（不完全按声明记录检查），是记录在案的遗留项。
- **Chapter 8 的名称补齐**：`par_map`（`Parallel` 内建类；Int/Float 及其
  张量；符号 `Real` 报 `E0310`）与 `rng : Int -> (() -> Float)`（纯、确定性
  生成器）进入 Prelude；`set`/`sum(a, axis)`/`at` 的秩 overload 补齐；
  单位名与函数名在调用位置正确分命名空间（`at(t, 0)` 是函数，`3 at` 是
  单位）；`f()` 支持零参函数值应用。**当前 chapter 8 覆盖：315 个名字，
  实现 + 命名 capability gap 已闭合**（`read_npy`/`write_npy` 仍报
  `E1001"io.binary"`；blocked 为 0）。
- **运行时/IR（§6、§9.3-§9.5）**：`reshape` 在连续视图上保持视图（否则
  复制）；`sum` 对 dense 与 boxed Float 使用 Kahan/pairwise；空张量归约按
  元素相位定义（Float 空 `sum=0.0`、`prod=1.0`、`mean=NaN`，Real 空
  `mean=E0802`）；混合字面量张量按 `join` 选择 f64 存储；`EvalError` 栈
  保留最内层失败帧与其调用点（§9.3.4）；`approx` 对定义域外形式表达式
  报 `E0806`（不是 `Ok(NaN)`）；`decide` 接受精确比较擦除后的 Bool 形式；
  `Dyn` 算术/`check` 的运行期校验见 §3；`par_map`/`rng` 运行期 arm 已实现
  （`par_map` 顺序执行）。
- **工具链（§9.8-§9.12）**：REPL 补 `:ast`/`:core`/`:time`/`:latex`；CLI
  补 `clean`、`lsp [--stdio]`、`rssn --emit --out`、`--locked`、`test
  --filter`、`check --all`、`doc --open`；LSP 补 code actions（加类型标注、
  温度单位换算、提取 let、修量纲错误）与 semantic tokens，诊断可带未使用
  绑定；`bench` 报中位数 + 95% CI 并提示构建 profile；`doc` 报告未文档化
  声明。编辑器仍按 §9.10 的“标准 LSP 3.17、协议无关语义层”分层。
- **内核（D:/rssn，本轮）**：§5.3.3 的 7 个单位注册进内核单位表
  （`Da`/`u`、`kn`、`mph`、`acre`、`bit`、`B`/`byte`、`KiB`；精确尺度；
  `bit`/`B`/`Da` 可加前缀）；**`Solutions.multiplicities`/`conditions`**
  不再是恒空：只在解确为“一元有理多项式、根为精确数”时按无平方分解填
  重数（无理/符号根保持“未报告”），`conditions` 填解本身给出的
  `NonZero` 条件（参数在分母），`complete` 是求解器的真实回答；18 条
  kernel-requirements 审计项逐条复核（含 4 条此前无直接测试的项）无回归；
  内核文档（KERNEL/DESIGN/DIMENSIONS/checklist/archive）同步到实测数字。

### 4. 记录在案的实现偏差（不是“以后再说”，是当前契约）

1. **十进制“量字面量”的精确折叠。** 裸小数默认 `Float`（f64），但带单位
   的十进制量（如 `3.2 km`、`0.1 m`）在编译期按**精确有理数**折叠；其静态
   表示仍默认 `Float[L]`，在精确上下文（与 Int/Real 混算）走精确路径，
   纯 Float 上下文才落成 f64。因此 `0.1 m * 3 == 0.3 m` 为真，而
   `0.1 + 0.2` 仍是 f64 的 `0.30000000000000004`。§3.10.1 的“精确十进制
   转换”规则不变，但“`3.0 m` 的值 3.0”要读作“精确 3，在 Float 运算处
   转 f64”。这是实现为保证精确性做的选择，记录在此以免被当成 bug。
   **相关已知缺陷**：静态 `Float` 与运行期表示目前可能不一致
   （`fn f(x: Float): Float = x + 1` 返回精确 3），这是类型健全性问题，
   不是该偏差的许可；修复方向是拒绝 Real→Float 或在字面量降级处按记录的
   表示强制转换（见 STATUS 已知缺口）。
2. **`par_map` 顺序执行**（见 §2）。
3. **`Rssn.typed` 的静态类型是 `Result<Value, OpError>`**，不是规范理想的
   `Result<a, OpError>`：本构建的 `TypeRep` 没有类型见证，虚构一个新鲜
   `a` 会重新引入 `∀a.a`；窄化由 `from_value` 完成。`call_ho`/`register`
   是“类型正确但 capability 门控”。三者都是诚实的能力边界，不是静默
   降级。
4. **类实例派发仍按运行时形状**，跨模块的类/实例/别名导出尚未实现
   （§7.1.3/§7.4 未兑现）；这是已复现的正确性缺陷（多原语头实例会静默
   互相覆盖、跨模块实例不可见），已列入下一轮的**最高优先级**工作，
   并配套“实例键 + 见证派发”的设计（见 §5）。
5. **没有 `@jit` 代码生成**、没有 `.aevi` 接口缓存、没有真正并行的
   `par_map`、没有标准库模块层级（§8.20）与 `Physics.*` 包装器；这些是
   实现缺口，不是规范删除的特性（capability/诊断按 §13.8 报告）。
6. **单一 RNG 语义未统一**：语言侧 `rng`/`sample_n` 与核侧 SDE 使用的
   RNG 流不同；当前承诺是**同构建可复现**，不是跨平台逐位一致。

### 5. 派发与扩展边界（新增 §12.8.13 的规范决定）

这一轮把长期含糊的问题写成明确决定（理由见 §12.8.13 与设计评审记录）：

- **不做运行时多方法派发**。实例由类型静态选定；类型被擦除后，运行期只
  按**规范见证**（ADT 的模块+名字身份、`NumKind`、张量秩+存储类、记录
  字段集合、Bool/String/Unit/Prop）选择实例；见证冲突是**检查期**的
  `E0311`/`E0310`，不是运行期错误。函数、`Dyn`、高阶值没有见证，不能用
  实例头部。
- **不做多参数类/superclass/关联类型/实例上下文/通用默认方法/用户数值类
  重载**。§3.8.1 的多参数类句子删除；§8.7.1 的 `class Distribution dist d`
  改写为名义 `Distribution<d>` 加内建 overload（v1 的用户自定义分布关闭，
  记录为扩展性损失，若将来需要，用 `RateOf` 式类型函数或专门的泛型类设计，
  而不是把类系统扩成关系型）。
- **不做按类型或按量纲的单态化**。类型/量纲擦除后，`mean<L>` 与 `mean<T>`
  必须是同一份代码（§4.4.4、§9.2.3、§11.8）；性能来自调用路径/表示特化/
  张量内核，而不是为每个类型生成代码。`expr_types` 可用于**表示**特化，
  但必须先修好静态 Float/运行期表示的偏差。
- **扩展点是**：ADT、记录、模块、结构化 `derive`（封闭集合
  `{Eq, Ord, TotalOrd, Show, Hash}`）、RSSN 算子/插件、`Dyn`、包 registry；
  不是宏、不是 `quote`/AST 片段、不是构建脚本（§12.8.5、§13.9.7、§9.14）。
- **`Show` 是保证也是钩子**：§3.8.4 列出的结构值（记录、`Solutions` 等）
  在没有用户实例时由内建结构打印兜底（保证）；用户可为自己的名义类型写
  实例覆盖（钩子），实例体在检查期按替换后的方法签名检查。
- **下一轮工作顺序**（记录在 PLAN）：① 实例/类/别名的模块接口 +
  `Class@?` 静态全局缺陷修复 + 见证派发（正确性）；② 单遍 `check --all`
  （~10×，不依赖缓存）；③ 最小 SDE（定步长对角噪声 Euler–Maruyama）；
  ④ 性能 stage-now（分配计数门、帧复用、字面量预解析、启动延迟）；
  ⑤ 增量编译接口缓存（先符号身份，后缓存）。

## Revision 10 — 第二轮实施：派发键、标准库模块树、SDE、Nat 与工具链（2026-11-05）

本轮把 Revision 9 列出的工作顺序中的前四项实施到可测试状态，并完成了设计评审
后的边界决定。原则不变：静态解析与运行期一致，能力缺失用诊断而不是删特性。

### 1. 派发与复用（正确性优先）

- **实例身份改为符号键。** `ClassKey`/`InstanceKey`/`AdtKey` 由「定义模块 + 名字
  + 规范化头部（标量表示、张量秩/形状、ADT 结构参数、记录标签集）」构成，
  `AdtId` 只作为进程内见证；运行期和检查器用同一个 `InstanceKey::mangle()`
  生成实例体全局名，修掉了 `Class@?::method` 让所有原语头实例互相覆盖的
  静默错误。`DictTarget` 把调用点钉死到 `Instance(key)` / `Witness{candidates}`
  / `Builtin(ShowStructural)`，IR 的 `CallKind::Dictionary` 携带 target，AST 与
  IR 解释器同规则派发。
- **类/实例/别名随模块接口导出**（§7.1.3/§7.4）：`ModuleInterface` 携带
  `classes`/`instances`/`alias_defs`，driver 按拓扑序传给
  `check_module_with_interfaces`；导入实例合并进 ClassTable（重叠仍 `E0311`，
  孤儿规则不重复检查），`private class` 只在本地，`private instance` 非法；
  别名的定义随接口导出，导入方可以展开它。同一类型名在两个模块里现在是两个
  不同的名义类型（构造器按定义模块绑定）。
- **见证派发。** 具体头部静态钉死；量化调用点按规范见证（ADT 模块+名字、
  `NumKind`、张量秩+存储类、记录标签集、Bool/String/Unit/Prop）选择；见证
  冲突是检查期 `E0311`/`E0310`，不再运行期猜形状。函数、`Dyn`、高阶值没有
  见证。`rows_overlap`/张量 `heads_overlap` 修正为按字段数/秩比较。
- **`Show` 语义**：§3.8.4 列出的结构值（记录、`Dyn`、单位值等）在没有用户实例
  时由内建结构打印兜底（保证）；用户可为自己的名义类型写实例覆盖（钩子），
  实例体按替换后的方法签名检查。`derive` 封闭到
  `{Eq, Ord, TotalOrd, Show, Hash}`，`E0317` 点名缺哪个字段/实例；多参数类声明
  报 `E0317`（§3.8.1 的多参数句子删除）。
- **`Distribution` 改为名义 `Distribution<d>` ADT**（§8.7.1），v1 不开放用户
  自定义分布，这个扩展性损失被记录而不是隐藏。

### 2. 标准库模块树（§8.20）

- 37 个模块源（29 个由 §8 章节生成 + 8 个手写：`Units/*` 六个、
  `Physics/Ising`、`Physics/Geodesic`；新增 `Sde`）经 `include_str!` 嵌入
  `aevia-std`，`aevia_std::stdlib` 暴露 `source`/`is_module_name`/`module_paths`；
  加载器在普通根失配时回退到嵌入源（合成 `stdlib/A/B.aev` 路径参与后序/环检测，
  `SourceMap` 显示 `stdlib://A/B.aev`），本地文件优先。
- `import Math { sqrt }`、`open Units.SI`、`import Data.Csv { read_csv, column,
  Table }`、`import Physics.Ising` 从用户源码可用；`spec/11` 的九个完整程序
  **全部 9/9** 通过类型检查（11.2 的 `RateOf` 被导入再导出遮蔽、11.4 的
  refined/rank-only 形状单向统一是两个真实检查器缺陷，已修复）。
- `Units/*` 是文档性模块：单位名本来就是 ambient `Unit[d]` 值，不导出绑定
  （§5.3.4）；`Units.Constants` 由 live manifest 生成。`Physics.Ising` 有 4×4
  端到端运行测试。
- 第 8 章覆盖率：**322 个名字，320 实现，2 个具名 `io.binary` 缺口
  （`read_npy`/`write_npy`），0 blocked，100% 有归属**。

### 3. SDE（内核 + 语言）

- **内核**新增 `rules::sde`（第 32 个 rule set）与两个无回调算子：
  `sde_wiener(seed, m, steps, h) -> Tensor<2, Real[T^(1/2)]]`（`DimRule::RootOf
  { degree: 2, index: 3 }`，取第 4 个参数的平方根；manifest 线格式
  `{"root_of":{"degree":2,"index":3}}`）与
  `sde_levy_areas(seed, h, dw, terms) -> Tensor<1, Real[T]]`；新增
  `Features.sde = true`；`SdeError` 带数据并实现 `Display`，`SdePath` 增加
  provenance 字段；求解器循环（EM/Milstein 族/SRK/Heun/自适应/非交换/集合）
  保持为 Rust API，因为回调无法越过 manifest ABI（内核清单 CO-03）。
- **语言侧**：新 `Sde` 模块与 §8.9.1 规范——`SdeMethod`（名称编码 Itô/
  Stratonovich）、`SdeStructure`（Diagonal / General(m) /
  GeneralNoncommutative(m)）、`SdeFixedOpts<T>`/`SdeAdaptiveOpts<T>`/
  `SdeOpts<T>`、`SdePath<s>`，入口 `sdeint_diag`/`sdeint_general`/`wiener`/
  `sde_order`/`sde_ensemble_seed`。`DiffusionOf<τ>` 内建类型函数（叶子量纲
  除以 `T^(1/2)`，与 `RateOf` 同构）。定步长可交换路径使用语言 RNG 流
  （与 `rng(seed)` 一致，`wiener` 与路径消费同一组增量），非交换/自适应
  使用内核 xoshiro 流；只承诺同构建可复现。错误沿用
  `E0802/E0806/E0807/E1001`（`core.sde`、`sde.noncommutative`、
  `sde.interpretation`、`sde.general.record`），不新增附录 B 码。
- `aevia-rssn` 自动吸收 manifest：`Rssn.sde_wiener`/`Rssn.sde_levy_areas` 类型化
  声明、`core.sde` 能力行、`T^(1/2)` 元素维度 conformance 测试；kernel 实测
  **1381 算子 / 32 rule set / 1567 overload（7 declared + 24 overlay + 1350
  derived）/ hash `8ed92a22…`**。
- 明确推迟：events/根查找、dense output、记录状态 + 一般噪声、非均匀定步长
  网格、用户自定义噪声结构、manifest 求解器算子；`aevia rssn --emit` 对开放
  维度参数仍保守地拼成 `Dyn<term>`（manifest/检查器精确，生成源码拼写是已知
  限制）。

### 4. 类型级 Nat 线性统一

- 形状 extent 支持真正的类型级 `Nat` 变量（`ty::Nat = Lit | Var | Add | Mul`，
  `Shape::Refined(Vec<Option<Nat>>)`），`Scheme.nats` 参与量化/实例化/默认化；
  统一把方程化为线性形式，单变量解出非负整数（`n+1~3` ⇒ 2、`2n~6` ⇒ 3）、
  多变量等式保留重试、常数残差 `E0302`、非线性 `E0304`。`reshape`/`zeros_of`/
  matmul/索引界限按解出的 extent 检查；实例头不允许 Nat 变量（`E0304`）。
- 前端诊断补齐：`E0108`（数字后括号非单位）、`E0205`（部分运算符 section）、
  `E0207`（callee 非名字的相邻类型应用）、`E0203` ragged 字面量、`W0101`/
  `W0103`/`W0601`/`W0602`；其余码在 `diagnostics_coverage.rs` 中有明确处置
  （运行期/加载器拥有，或带证明的 unreachable 理由）。

### 5. 工具链、测试与性能

- `aevia test --doc`：从 `##` 注释抽取 `aevia` 围栏块，逐块在合成模块中编译；
  块首行 `compile_fail` / `compile_fail EXXXX` 断言失败与错误码；`@compile_fail`
  属性删除（`E0210`）；`compile_fail("<source>")` 保持隔离片段契约。
- `aevia bench` 增加 `--iterations/--warmup`、`--save-baseline`/
  `--compare-baseline`/`--threshold`（默认 5%），JSON baseline 带
  profile/version/median/mean/CI，跨 profile 拒绝比较，回归退出码 1。
- LSP/analysis：文件已保存时通过 `driver` 加载导入图，诊断/悬停/补全/跳转按
  当前 `LoadedModule` 回答，跨模块跳转到定义文件；未保存缓冲区回退单文件并
  在能力中声明；新增 code actions、semantic tokens、typeDefinition、单位跳转、
  `W0101`/`E0309` 透传。缓存与 workspace/symbol 属后续优化。
- `aevia check --all` 改为单遍加载（`driver::load_all_with_roots` +
  `LoadStats`），共享依赖只检查一次；`load_with_roots_and_source` 让脏缓冲区
  保留多模块分析。
- 性能：字面量按检查器记录的表示物化（修掉「静态 `Float`、运行期精确值」的
  相位错位）；`eval_ir` 不再在每次返回时克隆整个调用栈，浮点字面量在 lowering
  时解析一次；Kahan/Neumaier 求和与单元素显示单位修正；分配计数门、符号项
  结构键、启动阶段剖析、Rep 特化 IR、JIT 均按计划后置（不做整体单态化）。
- 文档：`STATUS`/`PLAN`/本附录按实测数字更新；单文件规范版重新生成；
  `RSSN-INTEGRATION-REQUIREMENTS` 与 spec/10/13 对齐新的 manifest 事实与
  `Features.sde`；内核 `KERNEL/DESIGN/DIMENSIONS/checklist/archive` 同步。

### 6. 仍然开放（记录，不隐藏）

- SDE 源码级选项 ADT 依赖「别名/ADT 声明的 kind 推断」（`type Normal<d>` 的
  同类问题）；在它落地前 `sdeint_*` 只能从运行期测试构造选项。
- 增量编译 `.aevi` 接口缓存（先单遍化，缓存 v0 待做）；`aevia rssn --emit`
  开放维度拼写；稀疏张量运行期（`Value::Sparse`/`@` 布局分派）未实现，
  当前只有 `from_dense`/`to_dense`/`nnz` 记录级内建；JIT 代码生成未实现。
- `Spec/11` 依赖的 §8.7.1 `Normal<d>` 静态声明同样等待 kind 推断修复。



### Revision 10 实施补记（2026-11-05 夜 → 傍晚）

第二轮计划完成后又做了一轮逐模块硬化，全部带测试：

- **类型层**：§6.6 穷尽性/有用性改为有界矩阵算法（嵌套载荷、字面量、记录、guard、
  空枚举；W0201 冗余分支；深度上界 24 只会抑制诊断、不会凭空报错）；形状细化在
  matmul/索引/字面量切片/reshape/transpose 中保留；Nat 求解器用 `i128` 线性式，
  覆盖 `n+n~4`、任一侧乘字面量、负解 E0302、大整数、晚绑定后才矛盾（E0302）与
  真正歧义（声明末 E0304）；导入类型/构造器/透明记录别名与泛型别名/ADT 的
  kind 推断（`<d: Dim>`/`<n: Nat>`/`<r: Row>` 及从 body 推断）；属性实参必须是
  编译期常量（E0210）；`min_all`/`max_all` 接受任意秩；调用位置内建函数优先于
  同名单位值（`min` vs minute、`dyn` vs dyne），并保住 `dyn` 的动态量纲特化。
- **标准库与诊断**：§8 签名审计（309 条声明，2 条记录的松散 scheme，18 条人工
  类型/顺序发现全部修复或有主）；`div_rep` 结果细化（`inv`/`linsolve`/`lstsq`/
  `mean`/`var`/`std`/`cov`/`corr`）、`complex_sqrt` 的 `d^(1/2)` 显示、`rank`/
  `shape` 张量类型、NPY 秩放宽（`write_npy` 任意张量、`read_npy` v1 秩 2）、
  `decide` 的 Bool 重载、Measurement 算术；本地 NPY 编解码（第 8 章 323/323，
  0 缺口）；E0601、`args()`、E0901–E0905；精确相位扩展实数采取部分语义
  （`oo*0`/`oo-oo`/`oo/oo` 保持符号项，Float 按 IEEE）。
- **后端与工具链**：`aevia rssn --emit` 用声明内共享的维度变量精确拼写
  `Real[T]`/`Real[T^(1/2)]`/`Unit[T]`，其余 `Dyn<term>` 原因逐条记录；字面量键
  `Rssn.invoke(op, key, …)` 按 manifest 钉死的 overload 定型（每次调用新鲜的维度
  变量；非字面量/未知键保持动态，E1002/E1003/E1005 仍是运行期诊断），因此
  `--emit` 后 `check Rssn/Units/Constants` 各 0 诊断；编辑器侧补
  `references`/`rename`/`prepareRename`（跨文件 WorkspaceEdit、拒绝规则）、
  `signatureHelp`、字段/关键字补全、`workspace/symbol` 与内容哈希+ mtime 的
  分析缓存；`run -- args`、`run --example`。
- **性能**：`sum(a, axis)` 与整张量 `min_all`/`max_all` 的 dense f64 内核
  （512×512 axis sum：525,336 → 7 次分配、~449 ms → ~12.9 ms；min_all ~105 ms →
  ~5.2 ms，debug）；分配计数门、结构性 `Term` 键、`Rc<str>`、惰性内核常量播种
  （空程序启动 61.6 → 8.6 ms）。
- **内核**：Rader 素数长度 FFT + 批量线程化变换、`tan`/`atan2` 区间规则、C API
  维度错误码、三角形自适应求积、HOSVD/Tucker、`sde_scheme_info` 元数据算子
  （manifest **1382 ops / 32 rule set / 25 overlay / 1568 overloads**，hash
  `8d2c3764…654a`）。
- **最终权威套件**：`cargo test --workspace --no-fail-fast -- --test-threads=2`
  → **1266 passed / 0 failed / 5 ignored**（89 个测试目标，exit 0）；单文件规范
  版同步重新生成为 Revision 10。仍开放的项目逐条列在 `STATUS.md` 的
  “Known gaps”（稀疏运行期、`.aevi` 缓存/并行编译、registry、沙箱、JIT、
  非字面量 invoke、KN-06 等）。
