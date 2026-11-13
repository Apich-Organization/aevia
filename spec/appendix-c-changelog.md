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
5. **仍缺：** 见证签名的参数名由类型推出而非来自内核；元组与参差列表结果仍留在 dynamic 档；
   同一算子多个见证重载以 `_int`/`_real`/`_float` 后缀区分，而非类型类重载。

## 4.4 Revision 17（2026-11-12）：数值算子组、维度无关名字、GPU `f64`

1. **`Rssn_Numeric` 的数值算子组（§8.19.1、§10.3.8）。** rssn 的 `numeric` 规则集新增 73 个算子（manifest 从 1502 增至 1590：numeric +73、pde +9、discrete 的仿射名字 +6）。
   稳健统计、窗与谱、小波、Krylov/稀疏/低秩/张量/函数、有限差分求解器、复变特殊函数、表达式算子
   （`ode_solve`、`ode_symplectic`、`ode_imex`、`ode_etdrk4`、`oscillatory_integral`、`anderson_fixed_point`、
   `newton_cg_minimize`）进入 `Rssn_Numeric`：67 条手写 `sig`（含原有的 `fftconv`、`interpolate`），8 个 dynamic
   并写明理由（秩多态张量、返回混合列表的分解）。typed 档由此从 172 增至 239 个算子。
   两条约定：结果类型是 `Float` 而不是 `Real`（内核只编码浮点；`from_value<Real>` 不接受它）；方法/小波/统计量是
   符号，类型 `Real`，且每个名字只 `let` 一次（`symbol("db2")` 每次都造新符号）。
2. **维度无关的名字。** `heat_fd`/`wave_fd`/`poisson_fd`/`advect_fd` 的空间维数是场数据的嵌套深度；符号 PDE 为
   `solve_heat_equation`/`solve_wave_equation`/`solve_laplace_equation`/`solve_poisson_equation`；仿射变换为
   `translation`/`scaling`/`shear`/`reflection`/`rotation`（`_2d`/`_3d` 是别名；`Rssn_Graphics` 认领不带后缀的名字，
   该域从 38 增至 44 个算子）。有限差分的 typed 包装器是一维形式，二维/三维见 §4.5（`_2d`/`_3d` 形式）。
3. **GPU `f64`（§10.6.5）。** rssn 的 wgpu 设备以双单精度提供 `Dtype::F64`（约 48 位，`f32` 的指数范围，
   `DeviceInfo::emulated_f64`）；新增启动融合规划器 `device::plan`（`Plan::fuse`，融合与未融合逐位相同）。
   Aevia 侧 `Device`/`to_device`/`on` 对 `Gpu(n)` 仍报 `E1001("core.gpu")`，融合规划器尚未暴露为语言原语。
4. **见证包装器的参数名** 由类型推出（`n`、`x`、`xs`、`m`、`t3`、`flag`、`v`，同类重复时编号），不再是 `a0`、`a1`。
5. **测试。** 新增 `tests/domain_numeric.aev`（45 个 `test`，封闭形式核对），由 `aev_suites` 与
   `aevia test` 运行。覆盖报告 `tools/rssn-api-audit/RSSN-COVERAGE.md` 已重新生成。
6. **仍缺（Revision 18 已补二维/三维与分解）：**
   dynamic 档的数值结果不能 `from_value` 成 `Float`（无量纲证据）；manifest 里只有一个 `tensor_outer`，属于 `geometry` 规则集（`Rssn_DifferentialGeometry`），`numeric` 的同名实现不在 `Rssn_Numeric` 中出现。

## 4.5 Revision 18（2026-11-13）：按秩/维数命名的多签名，数值算子全部 typed

1. **一个算子多条 `sig`。** `domains.overlay` 的 `sig op as name(..)` 现在可对同一内核算子重复出现，各用自己的 Aevia 名字
   （`Overlay.sigs` 由 `op -> SigDef` 改为 `op -> Vec<SigDef>`，同名重复报错；渲染、覆盖检查与覆盖报告逐签名处理；
   只有一条 `sig` 的 overlay 不变）。新增 `codec Type - from_fn`：只解码的类型。
2. **有限差分的 2D/3D。** `heat_fd`/`wave_fd`/`poisson_fd`/`advect_fd` 保持一维含义，新增 `_2d`/`_3d` 形式
   （`Tensor<2, Float>`、`Tensor<3, Float>` 场）。
3. **秩多态张量算子。** `tensor_norm_1..4`、`tensor_inner_1..4`、`tensordot_<ra>_<rb>_<k>`（27 个）；分解 `hosvd_r`、
   `hooi_r`、`cp_als_r`、`tt_svd_r`、`tt_dense_r`（r = 2、3、4）返回前言里的记录 `Hosvd<r>`、`Hooi<r>`、`CpFit`、`TensorTrain`。
   `Rssn_Numeric` 的 75 个算子全部是 typed，dynamic 档为空。
4. **混合列表与列表访问器。** 桥的 `lift_list` 接受由张量和数混合的列表（保留逐元素量纲证据）；`from_value<Tensor<1, Value>>`
   成为通用访问器，把任何内核列表（或张量的第一个轴）拆成 `Value` 句柄，再逐个 `from_value<T>`。
5. **测试。** `tests/domain_numeric.aev` 新增 2D/3D 的 poisson、heat、wave、advect 封闭形式核对，`tensordot_2_2_2` 等于 Frobenius
   内积，秩 3 张量范数，`hosvd_3`/`hooi_3`/`cp_als_3`/`tt_svd_3`+`tt_dense_3` 重构。覆盖报告已重新生成。
6. **仍缺：** 秩超过 4 的张量算子、`tensordot` 结果秩超过 4 的组合、分解的秩 1 与秩 5–6 形式仍走 `Rssn.dynamic`（结果是句柄，
   可用 `from_value<Tensor<1, Value>>` 手工拆开）；符号 PDE 名字仍维度无关。

## 4.6 Revision 19（2026-11-13）：语言侧补全——秩完整、Result 组合子、诊断建议、格式化

1. **秩完整的张量操作（§8.1.5、§8.4）。** 规范把 `sum`/`prod`/`mean`/`var`/`std`/`norm`/`normalize`/`map`/`map2`/`zip_with`/
   `flatten`/`transpose` 写成 `Tensor<n, …>`，而类型表只有秩 1（部分有秩 2）：`sum(matrix)` 是 `E0302`，运行时却早已按秩无关实现。
   现在秩 2–4 都有重载（`rank_overloads`），`sum(a, axis)` 的 keepdims 形式覆盖秩 1–4；`transpose` 在秩 ≥ 2 上反转轴序
   （运行时新增秩 ≥ 3 的实现，细化形状同样反转）。
2. **`Result` 的组合子（§8.1.6）。** `map`、`and_then`、`unwrap_or` 增加 `Result` 重载（此前只对 `Option`，`Result` 只有 `map_err`）；
   新增 `is_ok`、`is_err`、`ok`（`Result → Option`）、`ok_or`（`Option → Result`）。
3. **列表与数值辅助。** `first`/`last`（`Option`）、`cumsum`/`cumprod`（保留量纲）、`argmax`/`argmin`；`round(x, n)` 保留 `n` 位小数；
   `is_nan`/`is_finite`/`is_infinite`（`mean([])` 与 `ln(0) - ln(0)` 产生的 NaN 此前无法判断）。
4. **诊断建议。** `Float[m]`（把单位写在量纲位置）的 `E0407` 现在指出 `m` 是单位、量纲是 `L`；拼错的量纲、拼错的单位
   （`E0501`，含带前缀的拼写）给出 "did you mean"；记录没有某个字段（`E0306`）且没有近似名时列出现有字段。
5. **格式化器保留空行。** `aevia fmt` 此前丢掉 `do`/布局块里语句之间的空行（仓库自己的示例都过不了 `fmt --check` 的原因之一）；
   现在作者留下的空行折叠为一行并保留，结果稳定。
6. **测试。** `tests/stdlib_ergonomics.aev`（18 个 `test`）；`aevia-fmt` 的 `blank_lines_between_statements_are_kept`；
   `aevia-types` 的重载计数断言更新。
7. **仍缺：** `reverse`/`filter` 等仍只有秩 1（运行时对矩阵 `reverse` 是展平，语义另议）；`fft` 只接受复数张量（实数用 `rfft`）；
   二维以上的 FFT 没有。（以上三项已在 Revision 20 解决。）

## 4.7 Revision 20（2026-11-13）：FFT 族补全、秩 ≥ 2 的序列操作、单位建议

1. **FFT 族（§8.8）。** `fft`/`ifft`/`fft_norm` 现在接受 `Complex`、`Float`、`Int` 张量（实数、整数提升为虚部 0，结果为复数，
   量纲保持）。新增多维变换（行列分解，沿每个变换轴逐一做一维变换，秩 1 到 4）：`fft2`/`ifft2`（最后两轴）、`fftn`/`ifftn`
   （全部轴）、`fft_axis`/`ifft_axis`（一个轴）、`fftn_axes`/`ifftn_axes`（任意轴列表，负轴从末尾数，越界或重复 `E0802`）、
   `ifft_norm`、`fftn_norm`/`ifftn_norm`；实数变换 `rfft2`/`rfftn` 与 `irfft2`/`irfftn`（`irfft*` 另有带 `Int` 的形式，
   给出奇数原长度）；`fftshift`/`ifftshift`（任意元素类型）与 `*_axis`；`rfftfreq`。`fftfreq`/`rfftfreq` 的结果量纲仍是
   采样间隔的倒数（`fftfreq(4, 0.5 s)` 是 `Hz`）。
2. **归一化修正。** `fft_norm(x, Forward)` 此前错用 `1/√N`（与 `Unitary` 相同）；按文档应为正向 `1/N`、逆向 1。现已修正，
   并按被变换的总点数计算（N 维时为各轴长度之积）。
3. **秩 ≥ 2 的序列操作（§8.1.5）。** 秩 `n ≥ 2` 的张量是沿第一轴的"行"序列：`reverse` 翻转第一轴并保持形状（**语义变化**：
   此前运行时对矩阵 `reverse` 是展平成向量，而类型表只有秩 1，所以该调用在检查期就是 `E0302`，只有直接调用运行时的
   代码会受影响；仓库内没有依赖展平的代码）。新增 `reverse_axis(t, k)`；`filter` 的谓词接受一行并保持秩；`any`/`all`/`find`、
   `first`/`last`、`take`/`drop`、`concat`（沿第一轴堆叠，其余轴必须一致）同样按行。秩 1 的行为不变；`f64` 张量上的 `reverse`/
   `reverse_axis` 仍是视图。
4. **单位建议的并列决胜。** `kmm` 过去建议 `klm`（千流明），因为 `m` 与 `lm` 同样是一次编辑，按字典序 `lm` 在前。现在先比较编辑
   距离，再偏好"包含最小"的编辑（候选是输入删去/加上若干字符，`kmm` 是 `km` 多了一个字母），再偏好常用单位，最后才是字典序；
   纯删除即使只剩一个字符也接受（`mm` → `m`）。
5. **测试。** `tests/fft_and_sequences.aev`（39 个 `test`：与教科书 2-D DFT 对拍、各轴子集往返、归一化、半谱往返（含奇数长度）、
   shift、频率量纲、行语义的序列操作）；`aevia-std` 的 `fft` 单元测试（N 维对拍、往返、`Forward`/`Unitary`、shift 置换、
   `rfftfreq`）；`aevia-core/tests/stdlib.rs` 的轴/长度错误与秩 3 变换；`aevia-types/tests/unit_suggestions.rs` 与
   `closest_unit` 单元测试。`stdlib_names.txt` 与生成的 `FFT.aev`/`Prelude.aev` 已更新。
6. **仍缺：** 秩超过 4 的变换与序列操作；`fold`/`sort`/`zip`/`enumerate` 在秩 ≥ 2 上仍是元素级（展平）语义；
   `fftn_axes` 没有对应的实数形式（`rfftn` 取全部轴）。

## 4.8 Revision 21（2026-11-13）：kernel 诊断进入 `Err(OpError)`；`Rssn_Numeric` 补全上一轮算子

1. **诊断通道（§10）。** rssn 新增 `Diag`/`DiagKind` 与 `Session::explain`/`apply_diag`。桥在算子留下自己的未约化调用时用
   `explain` 取原因（符号参数除外），`KernelError` 新增 `diag` 字段与 `from_diag`/`describe`；`Rssn.dynamic` 的 `Err` 现在是
   `rssn.diag.<kind>: <包装器名>: argument `<参数名>` (#<位置>): <kernel 的消息>`，包装器名取调用栈最内层帧，参数名取
   `domains.overlay`（`domains::wrapper_params`）。`OpError` 保持不透明、没有新构造子或 `Rssn.*` 成员，既有 `match` 不变，
   代码即 `"{e}"` 的前缀。类型化的 `Rssn.<op>(..)` 调用位置抛出 `E0807`，附注为稳定代码。
2. **新增 `Rssn_Numeric` 签名（11 个新名字族，`Rssn_Numeric` 的 typed 档 75 → 86）。** `poisson_fd_bc`、`heat_fd_bc`、`wave_fd_bc`
   （各有 `_2d`/`_3d`；边界种类 `kind: Real` 是符号 `dirichlet`/`neumann` 或 `d`/`n` 字母模式，如 `sparse_solve` 的 `method`；
   `boundary` 是单个数，逐轴/逐面的数值列表走 `Rssn.dynamic`）、`advect_fd_upwind`（`_2d`/`_3d`）、`ifft_abs`、
   `tensor_contract_2/3/4`、`tucker_dense_2/3/4`、`cp_dense_2/3/4`、`erf_complex`、`fresnel_s_complex`、`fresnel_c_complex`。
   纯 Neumann 的 `poisson_fd_bc` 数据不平衡时是 `rssn.diag.incompatible`，不会给出错误的场。
   rssn 签名 overlay 里只有秩变量的返回类型（`tensordot`、`tensor_contract`、`tucker_dense`、`cp_dense`）没有难倒绑定生成器：
   平铺 `Rssn` 门面把它们留在动态档，域模块由 `sig` 逐秩给出，`emit_typecheck` 等测试全部通过。
3. **测试。** `crates/aevia-rssn/tests/diagnostics.rs`（5 个：`from_diag`、`describe`、包装器参数名、`tensordot` 轴越界的
   `rssn.diag.domain` 与参数位置、合法调用不受影响）；`tests/domain_numeric.aev` 新增 14 个 `test`（`poisson_fd_bc` 的
   Dirichlet 等于 `poisson_fd`、不平衡 Neumann 为 `incompatible`、平衡数据有解、混合模式；`heat_fd_bc`/`wave_fd_bc` 常数场；
   `advect_fd_upwind`；`advect_fd_2d` 速度长度错误指向 `` `velocity` (#1) ``；`tensordot_2_2_1` 轴越界指向 `` `axes_a` (#3) ``；
   `tensor_contract`；`tucker_dense_3`/`cp_dense_3` 与 `hosvd_3`/`cp_als_3` 互逆；`ifft_abs`；`erf`/Fresnel）。
   `tools/rssn-api-audit/RSSN-COVERAGE.md` 已重新生成（manifest 1620 个算子，curated typed 258）。
4. **仍缺：** 只有 `rules::numeric::TABLES` 的算子记录原因，其余重型算子无原因（保持未约化项）；witnessed/dynamic 包装器只给
   参数位置；`poisson_fd_bc` 等的逐轴/逐面数值列表没有 typed 形式；`OpError` 没有 `code(e)`/`message(e)` 访问器
   （需要新的 `Rssn.*` 成员与检查器支持，留待检查器一侧的改动）。

## 5. 记录在案的未实现边界（Known Gaps）

1. **包依赖的边界**：registry 依赖、SemVer 解析和带校验的本地副本已实现（Revision 15）；
   `git` 依赖不存在；按包的依赖名字空间、`[patch]` 覆盖、镜像 `[source]` 替换尚未实现。
2. **设备后端**：`Gpu`/`Distributed` 只是可构造的运行时值；`on`/`to_device`/
   `@jit(target = Gpu(..))` 报 `E1001("core.gpu")`/`E1001("core.distributed")`。
3. **稀疏值进入 RSSN 项图**与形状细化（只检查秩）仍是空白（§13.9.5）。
4. **SDE 事件/稠密输出**（`sde.events`）与解释器侧并行系综（闭包 `!Send`）。
5. **REPL 中 Ctrl-C 取消当前求值**（§9.10）。
