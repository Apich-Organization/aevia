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
