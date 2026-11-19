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
