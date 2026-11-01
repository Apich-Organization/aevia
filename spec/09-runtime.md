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
