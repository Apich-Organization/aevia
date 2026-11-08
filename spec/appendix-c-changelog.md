# 附录 C — 修订记录与已知实现边界

本附录记录 Aevia 语言规范与实现的演进历程、当前功能支持状态以及明确记录在案的未实现边界。

---

## 1. 规范演进总结

- **Revision 0 (2026-10-31)**：确立语言核心定位（纯函数式、量纲作为类型、静态秩与动态形状张量、符号/数值双相、RSSN 计算内核后端）。
- **Revisions 1–6 (2026-11-01 ~ 2026-11-04)**：完成双向类型检查、单位推导与仿射温标收敛、行多态与模式匹配、ODE/PDE 统一接口规范、严格诊断码（E0100–E1000）定义。
- **Revisions 7–10 (2026-11-05 ~ 2026-11-07)**：
  - **标准库就绪**：实现 §8.20 标准库模块层级（`Math`、`Units.SI`、`Calculus`、`LinearAlgebra`、`Sde` 等 37 个模块），支持全路径与解构导入。
  - **原生 JIT 支持**：完成 `aevia-core/src/jit.rs`，在 `feature = "jit"` 下将纯 Float/Bool 核心 IR 降译为 RSSN/Cranelift 机器码磁带与循环执行。
  - **Rayon 并行求值**：在 `aevia-core/src/parallel.rs` 中实现 `par_map` 多工作线程副本（Replica）求值，由 CLI `--jobs N` 设定线程池。
  - **二进制 I/O**：实现 `read_npy` / `write_npy` 张量数据持久化。
- **Revision 11 (2026-11-08)**：
  - 修复 `BigRational` 零底负幂 panic 与十进制超大幂次 DoS 隐患。
  - 增强词法分析器支持 Unicode 上标括号 `⁽` `⁾` 及严格符号语法。
  - 修复 LSP 传输层零长度消息死循环及 `did_change` 未跟踪文档解包崩溃。
  - 强化符号引擎 `0^(-n)` 奇点处理、分数根保护及除法渲染括号包裹。
  - 修复张量 Cons 维度一致性校验与测量值零除异常。
  - 完善 RSSN FFI 批量计算 `diagnostic_codes` 机器可读编码及确定性 `bincode-next` 序列化。
  - 修复稠密特征值求解器 0x0 与 1x1 边界条件。

---

## 2. 真实已实现能力与状态对齐

针对历史审计文档中可能存在的过时记录，当前真实代码状态说明如下：

1. **`par_map` 并行求值**：已实现。通过 Rayon 线程池与独立求值器副本（`Replica`）并行映射，保持确定性输入输出顺序，并通过 `--jobs N` 控制并发度。
2. **`@jit` 机器码生成**：已实现。在启用 `jit` feature 时，通过 `aevia-core/src/jit.rs` 将纯数值分支与自尾递归循环降级为 Cranelift 机器码执行；不支持的高阶与闭包结构安全回退至解释器。
3. **标准库模块导入**：已实现。37 个标准库 `.aev` 源码模块位于 `crates/aevia-std/stdlib/`，支持 `import Math { sqrt }` 与 `open Units.SI` 等语法。
4. **NPY 数据读写**：已实现。支持 `read_npy<rank>` 与 `write_npy` 的静态秩检验与文件数据读取。

---

## 3. Revision 12（2026-11-08）：Alpha 深度完善

本轮关闭了此前记录的五项缺口中的四项，并修正了多处使用即报错的功能。

1. **缓存（两层）。**
   - rssn `Session` 计算缓存（`api::cache`）：**请求级答案缓存**（默认开启；
     键为哈希共享存储里的请求结点、模板与配置指纹；命中与新运行完全一致；撞到
     墙钟上限 `Stop::Time` 的运行不缓存；注册插件/单位时整体失效）与**子项级
     引理缓存**（`CachePolicy::lemmas`，默认关闭：重请求子项 `integral(…)`/
     `diff(…)` 归约到的闭式在同一语境——除预算外的全部配置——下被记录，之后
     包含同一子项的请求在运行前就断言该等式）。`Session::cache_stats` 给出命中
     计数。
   - Aevia 按模块的磁盘接口缓存 `.aevi`（§9.11）：`AdtId` 不落盘，序列化时
     换成符号键与声明定义，加载时重新驻留并重映射；键覆盖源码、`FileId`、
     身份、导入绑定、导入接口摘要、能力集与编译器身份。
2. **块状稀疏存储 `Bcsr`/`Block`（§8.4.1）。** rssn 新增 `kernels::bsr::Bsr`
   （与 `Csr` 位级一致的并行 matvec、转置、乘法、块 Jacobi、块 ILU(0)、块大小
   选择）；Aevia 运行时 `SparseFormat::Bcsr`（块内允许零填充，`nnz` 计逻辑非零），
   `to_bcsr(A, br, bc)`/`block_size(A)`，`convert<Bcsr>` 等自动选块。
   `E1001("storage.sparse.block")` 删除。另：`cg_with` 预条件共轭梯度
   （`Precond`），对称正定矩阵的 `spsolve` 先走稀疏 Cholesky，稀疏切片。
3. **源码级算子打通（§13.5.2）。** `@op` 函数以检查得到的参数/结果量纲注册
   **量纲签名**（rssn `DimSignature` + 通用 `signature_rule`，按调用求解量纲
   变量）；模块绑定换成包装：数值实参执行函数体，符号实参构造算子结点，所以
   kernel 的 `diff`/化简/`@rule` 看到的是算子本身；`@rule` 在算子注册之后追踪，
   可以谈论源码算子；rssn 规则解析器接受 `ns/name` 算子名。`Rssn.dynamic` 结果
   的 `show` 显示所携带的值与量纲。
4. **非交换 SDE（§8.9.1）。** rssn 新增 Stratonovich 非交换 Milstein、无导数
   非交换 SRK（Itô/Stratonovich）以及经精确 Itô 漂移换算的自适应 Stratonovich；
   Aevia 中方法名选格式族与积分解释、噪声结构选交换/非交换内核，任意组合都能
   运行；Lévy 面积的 `terms`/`cells` 有默认值；`sde_order_with(method, structure)`；
   记录状态 + 一般噪声（`sde.general.record`）已支持。
5. **使用即报错的功能。** `symfun` 的应用（原为 `E1001`）构造未定函数项；新增
   `dsolve(eq, y, t[, 条件])`（§6.4.5 已规定但此前不存在）；`@jit(target = Cpu)`
   被接受，`Gpu(n)`/`Distributed(..)` 在检查期报 `E1001("core.gpu")` 等而非
   `E0210`。若干过时文档与帮助文本已修正（rssn API 文档中"无墙钟限制"、
   §8.20 的稀疏说明、"v1 不支持"措辞等）。单文件规范改由
   `tools/bundle_spec.py` 生成。
6. **长张量字面量。** 超过 128 个元素（带单位时 64 个）的张量字面量在运行时报
   `E0304`：每个元素的表示/量纲变量连成一条链，而解析在深度 128/64 处截断。
   变量链现在迭代跟随（深度上限只约束结构嵌套），且合并变量后变得相同的数值
   约束被去重，2000 元素字面量的检查从约 7 s 降到 0.6 s。
7. **rssn 极限。** `limit(sqrt(x+1) - sqrt(x), x, oo)` 不再停留为请求（`x = t^(-k)`
   的 Puiseux 代换，并在 `t` 空间数值核验）；同时修正 l'Hôpital 预判把"探针看来非零、
   实际极限为零"的分母直接相除的错误（此前隐藏地会给出
   `limit(sqrt(x)*(sqrt(x+1) - sqrt(x)), x, oo) = 0`，正确值为 1/2）。

## 4. 记录在案的未实现边界（Known Gaps）

1. **远程包注册表与 SemVer 依赖解析**：`aevia add` 只支持本地路径依赖；
   `version`/`git`/registry 依赖报 `E0605`。
2. **设备后端**：`Gpu`/`Distributed` 只是可构造的运行时值；`on`/`to_device`/
   `@jit(target = Gpu(..))` 报 `E1001("core.gpu")`/`E1001("core.distributed")`。
3. **稀疏值进入 RSSN 项图**与形状细化（只检查秩）仍是空白（§13.9.5）。
4. **SDE 事件/稠密输出**（`sde.events`）与解释器侧并行系综（闭包 `!Send`）。
5. **REPL 中 Ctrl-C 取消当前求值**（§9.10）。
