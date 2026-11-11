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
  **已实现**：`check`/`run` 为每个已检查模块写一个 `.aevi`（键与格式见
  §9.11.3），写入时每个 `AdtId` 被换成该类型的符号定义，读回时重新驻留。
- **路径包的名义身份**同时包含包的 canonical 源目录（连同包名/版本）与
  包内相对源文件路径；可读的 `module` 名单独保留。同名同版本的两个本地包
  因源目录不同而拥有不同的 ADT/类/实例键；同一包的两个导入别名共享身份。
  同一物理源可由不同包拥有；归属沿依赖边与包内导入传递，不依赖 canonical
  文件是否仍位于包目录前缀。宿主可用 `SourceIdentity::for_package` 为外部源
  明确指定包；同一物理源的未保存缓冲会提供给其所有包实例。
  这是本地 path 源的身份规则，不是 registry 身份协议。
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
Units = "^1.2"                                        # 官方作用域：aevia/units
Plot = { package = "apich/plot", version = "~2.1" }   # registry 包
Kinematics = { path = "../kinematics" }               # 本地路径包
Dev = { path = "../dev", package = "apich/dev", version = "^0.3" }  # 本地开发用路径，发布后按版本

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

> **实现状态（2026-11-10）**：清单解析、`target`/`entry`、`--locked`、内容哈希、
> path 依赖与 **registry 依赖（版本需求、解析、锁、校验后的本地副本）** 已实现，见
> §7.5.1.1 与 §7.5.2；**`git` 依赖不存在**（报 `E0605`）。`[compat]`、
> `[capabilities]`、`[build]`、`[profile.*]` 示例是设计目标：当前清单读取器只读
> `[package]` 与 `[dependencies]`，其余表报 `E0605`（`[capabilities]` 目前也没有运行时
> 权限检查，§9.14）。

- `name` 是包名，`snake_case`；导入时用依赖键（导入根）。
- `version` 是 SemVer 2.0：`major.minor.patch`，可带 `-pre-release`；发布版本不得带
  `+build` 元数据（两个仅 build 不同的版本优先级相同，registry 无法区分）。
- `edition` 是语言版本（`2026`）。
- 依赖有三种来源：registry、path，或两者并存（`path` 用于本地开发，`version` 用于发布后
  的使用者）。不存在 git 依赖。registry 是内容寻址 + 签名 + capability 声明的，不是
  “以后再加”的附属品。
- `target` 为 `library` 时不需要 `main`；为 `executable` 时必须有
  `main : IO Unit`。
- `entry` 默认 `src/main.aev`。

#### 7.5.1.1 依赖声明与版本需求

`[dependencies]` 的键是**导入根**（`UpperCamelCase`，不含 `-`），值有以下形式：

| 写法 | 含义 |
|---|---|
| `Units = "^1.2"` | 官方作用域包 `aevia/units`，需求 `^1.2`。包名由键转 `snake_case`（`UnitsExtra` → `aevia/units_extra`）；缩写键（`FFT`）要显式写 `package`。 |
| `Plot = { package = "apich/plot", version = "~2.1" }` | 指定 `owner/name` 的 registry 包。 |
| `Lib = { path = "../lib" }` | 本地路径包。 |
| `Dev = { path = "../dev", package = "apich/dev", version = "^0.3" }` | 两者并存：本包从源码构建时用 `path`；本包作为 registry 包被使用时（`path` 目录不存在）只用 `package`/`version`。**发布的清单不得含纯 `path` 依赖**——使用者没有那个目录——服务器在发布时拒绝（`E_MANIFEST`）。 |

`version` 是**版本需求**（Cargo 的语法与含义）：

| 写法 | 含义 |
|---|---|
| `1.2.3`、`^1.2.3` | `>=1.2.3, <2.0.0`（裸版本即 `^`） |
| `^0.2.3` / `^0.0.3` | `>=0.2.3, <0.3.0` / 恰为 `0.0.3` |
| `~1.2.3`、`~1.2`、`~1` | `>=1.2.3, <1.3.0`、`<1.3.0`、`<2.0.0` |
| `1.*`、`1.2.x` | 通配 |
| `>=1.2, <2`、`=1.2.3`、`>1`、`<=1.2` | 比较子，逗号表示合取 |
| `*` | 任意**正式**版本 |

预发布版本（`1.0.0-rc.1`）只有当需求中某个比较子写出了**同一** `major.minor.patch` 的
预发布时才满足；`^1`、`*` 永远不会选中预发布版本，因此 `aevia update` 不会悄悄采用候选
版。可含内联表中的逗号（`">=1.2, <2"`），行尾 `#` 注释被忽略。

#### 7.5.1.2 解析：每个兼容系列至多一个版本

解析器是 `aevia-resolve`（纯 crate，无 I/O，无依赖；工具链与 registry 共用）。语义：

1. 满足所有需求；每个选中版本的依赖也被满足；选中版本之间**无依赖环**（环报错并列出环）。
2. **每个包的每个兼容系列至多选一个版本**。兼容系列：`>=1.0` 取主版本（`1.x`），`0.y`
   取 `0.y`，`0.0.z` 取 `0.0.z`（即 `^` 的含义）。因此 `1.4` 与 `1.9` 不会同时出现，
   而 `1.x` 与 `2.x` 可以共存，各自有自己的名义类型（§7.3：身份含包名/版本/目录）。
   共存的前提是两条边用**不同的导入根**；程序范围的导入根表仍然只允许一个根名对应一个
   包，否则 `E0606`（消息列出争用的两个版本）。
3. 在所有解中偏好**已锁定**的版本，其余取**最高**的兼容版本；`yanked` 的版本只在锁
   文件已经包含它时才可被选中（yank 不破坏已有构建，也不影响新解析）。
4. 结果与索引的迭代顺序无关（确定性）；不可满足时给出推导树式的说明：哪个需求无法满足、
   尝试过哪些候选及各自失败原因（连续相同失败折叠成区间）。
5. 实现是带冲突导向回跳的回溯求解器（不是 PubGrub，因为一个需求可由多个系列的“槽”满足，
   与“每包一个版本”的 PubGrub 项不兼容）。测试包括与穷举搜索的随机对照与回跳的复杂度
   断言。

registry 包的索引 `deps` 只记录 `package`、`req` 和（与默认导入根不同时）`import`。

### 7.5.2 锁文件

`aevia.lock` 记录每个依赖的精确版本、来源和摘要。构建 MUST 使用锁文件；`aevia
update` 更新。锁文件保证可复现构建；registry 包必须带内容哈希，yank 不删除内容。

格式（`version = 1`，字段与顺序固定，包按 `(source, import)` 排序，因此未变的解析产生
逐字节相同的文件）：

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

registry 工作流：

- 索引：`.aevia/registry/index/<owner>/<name>`（`$AEVIA_REGISTRY_DIR` 可共享）是
  `GET /api/v1/index/{owner}/{name}` 的原文。解析只读这些文件；缺失时 `aevia
  add/update/tree` 调用 `aevia-pkg index` 下载（`--offline` 禁止，并报告缺哪个包），
  编译器本身从不联网。
- 源码：`aevia fetch`（= `aevia-pkg vendor`）下载归档，校验摘要与 OpenPGP/SSH 签名后解包
  到 `.aevia/registry/src/<owner>/<name>/<version>/`，并写入 `.aevia-pkg-checksum`。
  构建 MUST 只读这些目录，且拒绝没有该标记或标记与索引摘要不符的目录（`E0607`），
  也拒绝清单依赖与索引记录不一致的目录（`E0605`）。
- `aevia update` 无参数时忽略锁取最新；`aevia update owner/name…` 只移动指定的包，其余
  保持锁定。`--locked` 下任何变化都是错误。`aevia tree` 打印 path 与 registry 包的依赖树。
- `aevia clean` 保留 `.aevia/registry`（不是构建产物）；`aevia clean --registry` 才删除。

> **path 包实现（2026-11-05）**：模块根与锁文件从同一个已验证的包图生成。
> 包节点以 canonical 目录去重，导入别名是图的边；同一包的每个别名都进入锁
> 文件。当前仍保留程序范围内的传递依赖别名表：不同包争用同一导入根报
> `E0606`。解析层允许同一包的不兼容系列共存（§7.5.1.2），但程序范围的导入根表仍要求
> 一个根名只指向一个包，所以共存要求两条依赖边用不同的导入根；让每个包各自拥有自己的
> 依赖名字空间（含 `Roots` 的按包查找）尚未实现。
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
  `import`/`open` 行会被复制，依赖按声明文件所属包及相邻目录解析。
  模块自身的声明不隐式进入片段；需要它们时显式导入声明模块。片段仅作为内存
  源码加载，不创建临时源文件。依赖或继承导入失败不满足 `compile_fail`。

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
| `E0606` | 依赖版本冲突（需求不可满足、依赖环、导入根争用） |
| `E0607` | 找不到模块/包（含：registry 索引未缓存、包未 `fetch` 或校验标记不符） |
| `E0608` | 入口缺失或签名错误 |

下一章给出标准库的签名。标准库是语言的一部分：单位、数学、线性代数、
统计、微积分、数据、绘图和仿真都在这里。
