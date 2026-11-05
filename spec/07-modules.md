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
  `E0601`。独立程序（无包、入口文件无路径上下文）与嵌入的标准库模块不受
  此检查约束。
- 一个文件只能有一个模块；不支持一个文件多个模块。
- 模块名区分大小写。

> **实现状态（2026-11-05）**：检查已实现——导入一个模块时，把该文件声明的
> `module` 名与其导入路径比对，不一致报 `E0601`（消息含两侧名字与“声明名
> 必须与导入路径一致”的 note）。豁免与上面一致：**入口文件**（manifest 的
> `entry` 指向的文件，或 `aevia run file.aev` 里直接运行的文件——它们没有
> 相对 `src/` 的导入路径）、**独立文件**（无包）和**嵌入的标准库模块**不
> 检查；实测声明 `module OtherName` 的包入口与独立文件都能正常运行。

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
