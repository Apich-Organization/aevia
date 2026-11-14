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
- `open` 只影响当前模块，不传递（没有 re-export）。
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
entry = "src/main.aev"      # 可选；默认 src/main.aev

[dependencies]
Units = "^1.2"                                        # 官方作用域：aevia/units
Plot = { package = "apich/plot", version = "~2.1" }   # registry 包
Kinematics = { path = "../kinematics" }               # 本地路径包
Dev = { path = "../dev", package = "apich/dev", version = "^0.3" }  # 本地开发用路径，发布后按版本
```

状态：清单读取器**只**读 `[package]`（键 `name`、`version`、`entry`）与 `[dependencies]`；其他表
（`[compat]`、`[capabilities]`、`[build]`、`[profile.*]`）与其他 `[package]` 键（`edition`、`authors`、
`description`）都是 `E0605`。说明、许可证、关键字、仓库地址在发布时由 `aevia publish` 的选项给出
（§9.9）。能力声明目前没有运行时权限检查，只有 `--caps`（§9.14）。`git` 依赖不存在（`E0605`）。

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
5. 实现是带冲突导向回跳的回溯求解器（不是 PubGrub：一个需求可由多个系列满足）。

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

状态：解析层允许同一包的不兼容系列共存，但每个包拥有各自依赖名字空间（按包的 `Roots` 查找）尚未实现，
所以共存须用不同导入根。

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

- `src/` 下的 `.aev` 都是模块，`main.aev` 是默认入口。`aevia test` 编译并运行 `tests/`。
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

- `test` 声明是 `Bool` 表达式；`aevia test` 运行所有测试。`approx_eq(a, b, tol)` 要求 `a`、`b`、`tol` 同量纲。
- 编译期错误测试用 `compile_fail("<source>")`（§8.13）：把源码片段交给编译器并要求**编译失败**；当前只支持
  不带导入的片段。`@compile_fail` 不是属性（未知属性 `E0210`）。属性测试用 `forall`（§8.13）。

### 7.5.5 文档

- `##` 文档注释附着到下一个声明，内容是 Markdown；`aevia doc` 提取类型签名、单位与例子，未文档化的声明
  在索引中报告。
- 文档注释中标 `aevia` 的代码块会被编译检查：`aevia test --doc [FILE…]` 抽取围栏块逐个编译；块首行可写
  `compile_fail` 或 `compile_fail EXXXX`。片段在自己的合成模块中编译，声明模块的 `import`/`open` 行被复制，
  但模块自身的声明不隐式进入片段，需要时显式导入。依赖或继承导入失败不满足 `compile_fail`。

## 7.6 版本与兼容性

- `edition` 决定语法/语义（目前清单不读取，§7.5.1）；库遵循 SemVer：破坏性变更升 major，新增 API 升 minor，
  修复升 patch。
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
