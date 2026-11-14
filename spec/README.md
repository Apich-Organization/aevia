# Aevia 语言规范

**Aevia — 设计标准**

Aevia 是一门面向科学计算的纯函数式语言，**量纲是类型系统的一部分**。一个量不是带运行时标签的数字，而是类型里记着物理量纲的值。编译器在任何代码运行之前就拒绝
`3 m + 2 s`、`sin(3 kg)` 和 `exp(1 m)`。

语言建立在 `rssn` 计算机代数与科学计算引擎之上：符号与数值求值是同一个表达式的两个相位；单位、量纲推断、重写、数值内核、JIT 和物理仿真由后端提供。Aevia 不维护第二份算子表或单位表：
它读取 rssn 的 **Kernel Manifest** 与 `rssn::dim` 注册表，并由它们生成 `Rssn` 模块。见 [13 — 可扩展性](13-extensibility.md) 与 rssn 仓库的 `docs/KERNEL.md`。

> 本文档是**规范性设计**：文法完整，类型规则明确，运行时架构固定，每个后端依赖都已标出。示例与取舍说明是资料性的。

---

## 阅读指南

| 章节 | 内容 | 规范性 |
|---|---|---|
| [00 — 总览](00-overview.md) | 目标、非目标、原则、十分钟导览 | 是（目标） |
| [01 — 词法结构](01-lexical.md) | 源码编码、字面量、单位、布局 | 是 |
| [02 — 语法](02-syntax.md) | 声明、表达式、模式、模块 | 是 |
| [03 — 类型系统](03-types.md) | 种类、类型、推断、类型类、效果 | 是 |
| [04 — 量纲](04-dimensions.md) | 量纲代数、类型规则、多态、`Dyn` | 是 |
| [05 — 单位](05-units.md) | 单位声明、前缀、仿射点、常数 | 是 |
| [06 — 求值语义](06-semantics.md) | 求值、值、符号/数值相位、张量 | 是 |
| [07 — 模块、包与可见性](07-modules.md) | 命名空间、可见性、包、测试 | 是 |
| [08 — 标准库](08-stdlib.md) | Prelude 与库签名 | 基本是 |
| [09 — 运行时](09-runtime.md) | 流水线、解释器、REPL、CLI、LSP、性能 | 是（架构） |
| [10 — rssn 后端映射](10-backend.md) | 与 rssn 的映射、桥接设计、缺口与风险 | 是 |
| [11 — 完整示例](11-examples.md) | 从运动学到仿真的完整程序 | 否 |
| [12 — 设计取舍、限制与路线图](12-rationale.md) | 替代方案、取舍、设计边界、实现进展 | 否 |
| [13 — 可扩展性与 RSSN 集成](13-extensibility.md) | rssn 反射、生成的 `Rssn`、插件、registry、形状/设备/稀疏决定 | 是 |
| [附录 A — 完整文法](appendix-a-grammar.md) | 完整 EBNF | 是 |
| [附录 B — 诊断目录](appendix-b-diagnostics.md) | 错误码与消息格式 | 是 |
| [附录 C — 修订记录](appendix-c-changelog.md) | 修订历史与已知边界 | 资料性 |

后端接口契约在 rssn 仓库的 `docs/KERNEL.md`（Kernel Manifest、动态调用 ABI、插件、单位与量纲）和 `docs/DIMENSIONS.md` 中；这是 Aevia 规范成立的前提，不是可选的实现细节。
实现的 crate 划分见仓库根目录的 [ARCHITECTURE.md](../ARCHITECTURE.md)。每章另有英文版（`spec/en/`），写作与术语规则见 `spec/STYLE.md`。

## 规范用语

**必须**（MUST）、**不得**（MUST NOT）、**应该**（SHOULD）、**可以**（MAY）按 RFC 2119 理解。符合规范的实现必须接受本规范测试语料中的每个程序，必须拒绝每个标记为类型错误的程序，
并产生规定类别的诊断（具体措辞不是规范性的）。

## 设计状态

这是**设计标准**，不是冻结的实现。标为规范性的章节意在按文实现；[附录 C](appendix-c-changelog.md) 记录每次修订与已知缺口。当前文本是 **Revision 23**，对齐 **`rssn 3.0.0-rc.1`**
（`api_version = 2`）。manifest 的规模是测量而不是常量，以 `aevia rssn` 的 live 输出为准（该版本为 1620 个算子、82 个单位、30 个常数）。分派与扩展遵循第 13 章的边界：没有运行时多方法、
没有多参数类、没有按类型的单态化；实例在检查期静态选定或由规范见证选定。

**没有 v1/v2 的语言分层**：每项能力都已设计到位；后端尚未实现的能力报 capability 诊断（`E1001`），而不是删掉语言特性。

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
