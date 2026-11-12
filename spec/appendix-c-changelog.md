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
- **Revision 12（数值与类型系统缺口，track 3）**：
  - **形状细化（§3.5.1/§3.5.3）**：`Nat` 求解器由线性形式升级为多项式形式——变量的乘积（`n * m`）作为不透明单项式保留，可与同一乘积相消（`n*m ~ m*n`），一个因子被解出后代回成线性方程；推断变量（泛型被调用方的新实例）可与任意表达式消去（`a ~ n + 1`、`a ~ b`），修复了"泛型函数在另一个带符号 extent 的泛型函数里被调用时报 `E0304`"。**声明的 extent 是刚性的**：两个不同声明变量（`n`、`m`）不会被悄悄合并，报 `E0302`（原来是含糊的 `E0304`）；仍可被字面量或表达式解出（`n + 1 ~ 3`）。逐元素 `+ - * /` 实现规范早已承诺的 1-extent 广播（`[3,1] + [1,4] = [3,4]`、`[n,1] + [1,m] = [n,m]`），此前类型检查器把它当作形状错误。诊断带源码名字（`n + 1 = n`、`shape mismatch: expected [n, 2], found [n, 3]`）。实现 `refine_shape<[..]>(a) : Option<Tensor<..>>`（规范早有，实现缺失）：形状分支的写法。
  - **稀疏（§8.4.1）**：第一类稀疏值、CSR/CSC/COO/BSR、转换、`@`、转置、加减、Hadamard、标量积、`spsolve`/`cg`/`cg_with` 此前已完备；新增非对称 Krylov 求解器 `gmres_with`/`bicgstab_with`；`sparse @ dense` 按行并行且逐位确定；更正"形状只检查秩"的过时说法。
  - **ODE（§8.9.0）**：`ode_solve`（连续输出 `at`、多事件含方向与终止）、`odeint_order`（任意阶，逐阶量纲追踪）、`ode_bvp`（单步打靶）、`AutoSwitch`（Dopri5 → Radau 按步数预算切换）。
  - **SDE（§8.9.1）**：`sde_at`/`sde_resample`/`sde_interp`（线性弦的连续输出与 saveat）、`sde_events`（方向/终止）、`sde_ensemble_diag/general`（确定播种，经并行副本，结果与线程数无关）；原"v1 不做 events、dense output、ensemble runner"改为已实现。
  - **并行（§9.7.2）**：审计 `par_map`/`par_reduce`/`par_tabulate` 的确定性，把稀疏 `@` 与 SDE 集合接入同一机制。
  - **遗留缺口**：多重打靶/配置法 BVP；SDE Brownian 桥稠密输出与积分中途的终止事件；`AutoSwitch` 无特征值估计；`gmres/bicgstab` 不支持精确元素。

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

## 4. Revision 13（2026-11-07 ~ 2026-11-09）：信任边界与规则收紧

本轮改变的是几条语言规则，并把此前只存在于实现中的部分写入规范正文。下列条目
在对应章节里以“Revision 13”标注。

1. **单位表达式的空白规则（§1.8.1）。** 此前单位表达式内的运算符一直延续到数字
   或表达式边界，`3 m * x` 是单位 `m*x`；现在空白是单位与算术的分界，紧贴写法
   `3 m*x` 仍是单位表达式。规范自己的导览 `70 kg * v^2 / 2` 因此可以通过名字解析。
2. **未解出的量纲变量作指数的底数（§3.4.3）。** `diff(fn t => t^t)` 中 lambda
   参数的量纲仍是推断变量时，检查器令其为 `1`（与 `sin(t)` 让 `t` 无量纲同一条
   规则），不再报 `E0404`。
3. **声明的量纲参数是刚性的（§4.4.1）。** `fn f<d>` 的函数体不能替调用者选定 `d`；
   违反时报 `E0318`。
4. **`show` 保留 `Float` 的相位（§6.12.3）。** 整数值的 `Float` 带小数点（`10.0`），
   精确的 `10` 与浮点的 `10.0` 不再显示得一样。
5. **`Set<τ>` 有了值与操作（§8.1.5.1）。** 此前只有类型。`set_insert` 等返回新集合，
   集合相等与插入顺序无关。
6. **运行时信任边界（§9.2.3，规范性）。** 运行时执行检查器的证明而不重新证明：
   已检查的源调用不重新验证实参，已检查的构造器应用不遍历载荷，宿主边界处仍完整
   验证，字面量按（位置、证据、模块）记忆。结果不变，工作量下降一到两个数量级。
7. **字面量折叠（§3.10.4）取代 Revision 9 记录的偏差。** 一个字面量只舍入一次，
   闭合的字面量算术树是编译期常量，`1.0 + 2` 是 `Float`。
8. **单位制（§5.11）写入规范正文。** `module M with units natural`（还有 `planck`、
   `atomic`、`geometrized`）按单位制约化模块里写出的量纲，值始终是诚实的 SI 量；
   `to_system`/`from_system<D>` 显式换算；`E0511`、`E0512`。实现与附录 B 的诊断码早已
   存在，章节正文此前缺失，现补全。
9. **文档对齐。** §9.9 的命令与 flag 对齐实际 CLI（`watch`、`lsp`、`rssn`、`pkg`、
   `--caps`、`--no-cache`、`--jobs` 已实现）；已删除的 `RSSN-INTEGRATION-REQUIREMENTS.md`
   的引用改指 rssn 仓库的 `docs/KERNEL.md`；`spec/README.md` 的修订号与 kernel 计数更新。

## 4.1 Revision 14（2026-11-10）：rssn 绑定自动化

1. **领域模块（§10.3.8）。** 新增 `Rssn_<Domain>`：manifest 加声明式 overlay
   （`crates/aevia-rssn/domains.overlay`）按领域生成带类型的模块；转换泛型化，错误走
   `Result`；覆盖检查在算子既无绑定也无排除时失败；报告
   `tools/rssn-api-audit/RSSN-COVERAGE.md` 由测试生成并比对。
2. **维度证据。** 无量纲的 kernel 规则与 overlay `sig` 让派生签名算子的裸数结果可以
   `from_value<Int>`；`list` 项结果读回为张量；未约化项的错误信息带上该项。
3. **丰富类型。** `Zmod`、`Perm`（0 起始）、`Graph`、`Group`、`PermGroup`。
4. **仍缺：** `Zmod<n>`/`GF<p>` 的类型级模数与运算符重载（§10.3.8 列出所需的语言改动）。

## 4.2 Revision 15（2026-11-10）：包平台与依赖解析

1. **`aevia-resolve`（新 crate，§7.5.1.1/§7.5.1.2）。** SemVer 2.0 `Version`（预发布、build、
   全序）、`VersionReq`（`^ ~ = > >= < <=`、通配、`>=1.2, <2`、预发布规则）、解析器（带冲突导向回跳的回溯，
   偏好锁定版本再取最高、yank 仅在已锁定时可选、每兼容系列至多一个版本、环检测、确定性、推导树式的失败说明）、
   registry 稀疏索引读取器（无依赖的严格 JSON 子集）。测试含与穷举搜索的 2000 轮随机对照。
2. **清单与锁（§7.5.1/§7.5.2）。** `Units = "^1.2"`、`{ package = "owner/name", version = … }`、path+version
   并存；`E0605` 严格性不变（git/未知键仍拒绝）；锁新增 `checksum`，registry 来源为 `registry+aevia`；
   仅 path 的锁逐字节不变。解析经本地缓存目录实现（`DirRegistry`），构建只读校验过的本地副本。
3. **平台（`platform/`）。** 索引的 `deps` 现在记录 `package`/`req`/可选 `import`；发布时校验：不得有纯 path 依赖、
   不得依赖自身、依赖必须存在且有未 yank 的版本满足需求；版本允许预发布、拒绝 build 元数据；发布与 yank
   各自在一个事务内（行、依赖行、归档文件、日志链）；索引带 `ETag`；下载校验存储字节的摘要；新增
   `POST /api/v1/resolve`、`GET /api/v1/crates/{owner}/{name}/dependents` 与包页“被依赖”表。
4. **客户端。** `aevia add owner/name[@req] [--as]`、`aevia update [owner/name…]`、`aevia fetch`、`aevia tree`
   （`--offline`）；`aevia-pkg` 新增 `index`、`add@req`、`update`、`vendor`/`fetch`、`tree`、`lock [--check]`。
5. **仍缺：** 按包的依赖名字空间（同一程序中同名导入根指向不同版本，仍是 `E0606`）；`[patch]` 式覆盖；
   仅 path 包的传递 registry 依赖由 `aevia` 解析但 `aevia-pkg tree/lock` 只读根清单；
   `aevia-pkg` 对 registry 以外的镜像没有 `[source]` 替换；解压后的源树只靠校验标记保证完整性（无逐文件摘要）。

## 4.3 Revision 16（2026-11-12）：见证式自动类型化

1. **witnessed 档（§10.3.8）。** 领域模块不再只有"手写 `sig`"与"`Result<Value, OpError>`"两档。
   rssn 内核把引擎自己的等式判断（请求与答案同处一个等价类）记录为 sort 事实，经完整引擎重放、
   聚类取并、变异探测后生成 `signatures.witnessed.json`；Aevia 从 manifest 的
   `overload_sources` / `refinements` 为没有 `sig` 的算子生成同形的类型化包装器。
   dynamic 档从 1298 个算子降到 871 个，新增 453 个自动类型化算子，没有一条签名是手写的。
2. **不透明句柄。** 结构化内核对象（图、群、稀疏矩阵等，以构造子为头的项）在见证签名中映射为
   `Value`：可以传回内核、可以对它做带类型的查询（`sp_rank(s): Result<Int, OpError>`），除此之外不可见。
3. **量纲证据。** 见证重载在全部参数已知无量纲时给出无量纲结果证据；内核声明为
   `NonMetric`/`Container` 的见证算子不要求参数证据（句柄没有量纲）。
4. **新内核能力随之可用**：`Rssn_LinearAlgebra` 中的精确/符号稀疏矩阵（`sp_*`），
   `Rssn_Calculus` 中的 D-finite 函数判定（`holonomic_zero`、`holonomic_equal`、`holonomic_ode` 等）。
   端到端示例见 `tests/domain_witnessed.aev`。
5. **仍缺：** 见证签名的参数名是位置名（`a0`、`a1`）；元组与参差列表结果仍留在 dynamic 档；
   同一算子多个见证重载以 `_int`/`_real`/`_float` 后缀区分，而非类型类重载。

## 5. 记录在案的未实现边界（Known Gaps）

1. **包依赖的边界**：registry 依赖、SemVer 解析和带校验的本地副本已实现（Revision 15）；
   `git` 依赖不存在；按包的依赖名字空间、`[patch]` 覆盖、镜像 `[source]` 替换尚未实现。
2. **设备后端**：`Gpu`/`Distributed` 只是可构造的运行时值；`on`/`to_device`/
   `@jit(target = Gpu(..))` 报 `E1001("core.gpu")`/`E1001("core.distributed")`。
3. **稀疏值进入 RSSN 项图**与形状细化（只检查秩）仍是空白（§13.9.5）。
4. **SDE 事件/稠密输出**（`sde.events`）与解释器侧并行系综（闭包 `!Send`）。
5. **REPL 中 Ctrl-C 取消当前求值**（§9.10）。
