# Aevia changelog · 变更日志

The entries cover the language toolchain, the package registry and their documentation. 本文记录语言工具链、包注册中心及其文档的变化。[Language specification](spec/en/README.md) · [语言规范](spec/README.md)

## [0.1.0-beta.1]

### Scientific computing

- The checked Core IR interpreter handles typed operator dispatch, explicit tail calls and dimension evidence. Integer division, remainders, `gcd`, `lcm` and `to_int` preserve arbitrary precision. Exact rational inputs keep exact remainders. `range` accepts arbitrary-precision endpoints and checks the result size before allocating. Floating-point tensor execution can use vectorized and parallel kernels.
- The RSSN Kernel Manifest drives Aevia adapters for dimensioned values, numerical callbacks, structured model results and session-owned objects.
- The module cache encodes checked interfaces and types with native bincode-next traits and a schema fingerprint. The toolchain rebuilds incompatible entries.
- Dependency resolution eliminates candidate lists early where another package already has fewer choices. The module loader indexes resolved dependency edges and includes exact re-export origins in cache keys. Editor queries reuse lazily built module indexes for each checked snapshot.
- Generated RSSN interfaces expose new signal processing, weighted graph and low-rank algorithms. Dense and sparse numeric dispatch uses the active CPU pool's size.
- The numerical library supports batched distribution CDF queries, periodic Helmholtz solves and finite-field byte operations. The callback bridge places callbacks at the argument positions declared in the kernel manifest.
- `Interpolate.interp2Batch` prepares one grid for all query points. Each query axis keeps its own coordinate dimension, and the result keeps the grid's value dimension.
- Forward real FFTs borrow contiguous Float input. The prepared RSSN inverse real FFT reuses a fixed length across spectra, returns measured samples, and reports the sample and bin counts separately.
- `Math.NumberTheory.mod_pow` computes exact modular powers. `Stats.Distributions.dist_quantile` returns quantiles with the distribution's units, including support endpoints.
- RSSN bindings expose fixed-step ODE final states, sampled equatorial Kerr–Newman trajectories, finite-field Betti numbers, fused regression gradients, and seeded Ising observations. [Numerical examples](examples/NUMERICAL_GUIDE.md)
- The transverse magnetic FDTD operator returns the measured final electric field and elapsed time at the requested step. [Numerical examples](examples/NUMERICAL_GUIDE.md)
- The two-dimensional Schrödinger operator can prepare phases for repeated signed-time propagation with measured complex amplitudes. `Pde.solve_heat` accepts the `adi` method for a fixed-boundary two-dimensional field. [Numerical examples](examples/NUMERICAL_GUIDE.md)
- Triangular finite-element operators can prepare a P1 or P2 mesh for repeated measured diffusion–reaction solves. The Euler and shallow-water configuration accepts the `Hlle` flux. [Numerical examples](examples/NUMERICAL_GUIDE.md)
- Aevia can reuse a prepared Cartesian Q1 Poisson operator across measured sources or a prepared two-dimensional fast multipole tree across source strengths. Chain accelerations retain physical units and follow input body order. [Numerical examples](examples/NUMERICAL_GUIDE.md)
- Prepared exact gas Riemann solutions accept single or batched velocities and return measured density, velocity and pressure. [Numerical examples](examples/NUMERICAL_GUIDE.md)
- Prepared periodic DG advection advances a measured nodal field. Checked explicit Euler, midpoint and Heun accept signed time steps and return the measured state at the requested endpoint. [Numerical examples](examples/NUMERICAL_GUIDE.md)
- Prepared hyperelastic material points reuse a fixed deformation gradient and return measured stress, stiffness and energy density. Molecular-dynamics neighbour builds support sparse, large periodic boxes. [Numerical examples](examples/NUMERICAL_GUIDE.md)
- Mixed tensor selectors keep the extents of surviving axes. Slice indices, bounds and steps keep integer precision at runtime. Dense tensor views check their shape and storage boundaries before indexing or materializing the tensor. Dense constructors validate output size, `diag` preserves the element representation, and `linspace` preserves endpoints across large finite intervals.
- Square matrix determinants, inverses and solves keep exact operands exact at every size. Floating-point factorizations reject non-finite input, and eigenvalue and Cholesky routines validate both matrix extents. Floating-point transposes of any rank share their backing storage. Reshaping checks every runtime extent and returns `Result`. Tensor sequence operations retain the element type, including for empty selections.
- Floating-point mean and standard deviation stay finite where the result is representable across extreme scales. FFT axes use exact integer indices. The inverse real transform restores the requested extent even with empty untransformed axes. SVG, HTML and GIF render lines, scatter plots, error bars, histograms and heatmaps using the same coordinate geometry.
- Source rules preserve complete symbolic parameter names when publishing rewrites to RSSN. The source plugin interpreter inherits the caller’s active capabilities.
- CSV and TSV readers preserve header units as metadata and return unsupported-format errors through their IO result. Table joins index the right table once and preserve the order of matching rows.
- Type-level natural numbers retain exact coefficients through constraint solving. Sparse tensor contraction requires distinct axes on each operand, including axes computed at runtime.
- Generic record updates preserve open rows and enforce field exclusions across function aliases and module interfaces. The interpreter reuses uniquely owned records when applying an update.
- Parallel tasks honor the configured worker count. Transfers preserve constructor provenance, tensor schemas and explicitly selected generic closures. Workers enforce the submitter’s active capability scope and report cancellation consistently through `await`.
- Parallel workers initialize from the checked program shared by the main interpreter. Large record declarations and documentation blocks parse without repeatedly scanning their accumulated fields or text.
- The worker pool grows with the amount of independent work, up to the configured limit. Parallel reductions reuse decoded functions within a batch and retain fixed block boundaries.
- `ParMap` shares contiguous floating-point tensor storage across workers and preserves dimensions, display units and input order.
- `Map2` and `ZipWith` read tensor inputs as each callback needs them, including when the first callback fails.
- Worker defaults follow CPU availability and Rayon configuration. Requests can reuse an existing pool when its workers satisfy the requested limit and workload.
- Measured RSSN calls include prepared Newton–CG, grid transfers and tape column selection. Generated numerical help and the source coverage audit read the running kernel manifest.
- NPY input decodes Fortran layouts directly to row-major storage. Large arrays with three or more varying axes can use the active RSSN CPU pool. Wide record patterns with ignored fields use the same exhaustiveness rules as narrow patterns.
- Parallel maps and tabulations can return homogeneous Float batches in dense storage. Fixed-signature native RSSN calls can borrow contiguous Float inputs and return dense tensors with their dimensions.

- Elementwise Float addition, subtraction and multiplication share RSSN's SIMD vector kernels. The [measured update example](examples/measured_vector_updates.aev) applies a velocity correction to a vector of positions.
- RSSN polynomial models can fit a measured axis and evaluate a batch of queries after composition. The [calibration example](examples/polynomial_calibration.aev) shows how Aevia tracks time and distance through both models.

- The measured [Kalman tracking](examples/kalman_tracking.aev), [state-space response](examples/measured_state_space.aev) and [tridiagonal spring chain](examples/tridiagonal_spring_chain.aev) workflows use RSSN's typed interfaces.
- Generated PDE event calls accept typed callbacks in their declared positions and check the event result's units. Exact [dyadic arithmetic](examples/dyadic_real.aev), [hyperbolic charts](examples/hyperbolic_charts.aev) and [Weierstrass periods](examples/weierstrass_periods.aev) show named results and numerical contracts in source.

### Toolchain and registry

- The language server reuses line indexes, document outlines and semantic tokens. Clients can request semantic highlighting for a visible range. Document highlights use declaration identity, and formatting edits use the open text.
- Package resolution searches the sorted version catalogue by constraint interval. Registry documentation reuses its HTML sanitization policy and renders compiler module summaries and visibility directly from the extracted module.
- On Windows, concurrent package-cache refreshes keep the previous index available while retrying transient file replacement errors.
- The lint fixer preserves condition evaluation when simplifying identical branches. SARIF reports declare their column coordinate units. Registry HEAD requests no longer increment download counts.
- The lint fixer combines edits reached through different paths to the same file and preserves expression grouping and comments. The opt-in `constant-condition` check reports Boolean literals used as `if` conditions.
- The CLI, REPL, language server, formatter and lint tools share the language front end. Compatible editors can inspect live source overlays, run and test files, open a REPL and debug with breakpoints, stepping and variable inspection.
- Local path dependencies accept `aevia add --as` aliases. The command rejects surplus package arguments before writing the manifest or lockfile.
- Completion uses the local type of a shadowed name. Signature help keeps the active argument accurate while editing nested calls or string arguments. The draft release workflow includes versioned VS Code and JetBrains extension archives.
- The registry serves signed releases, package search, generated documentation and a transparency log. GitHub publisher and organization authorization uses stable provider account IDs.
- The standard library reference, book, architecture manual and specification have English and Chinese editions. The registry supports documentation hot reload and standalone HTML downloads. Operators can enable runnable book examples through a separate rootless Podman worker.

### Installation and upgrades

- The installer and CLI toolchain manager verify SHA-256 and GitHub release attestations. The latest stable release, dated nightly builds and exact versions install side by side. Activate a version for the language CLI, package client and language server together.
- Build from source with Rust 1.100 or later and the paired RSSN revision. Release archives contain `aevia`, `aevia-lsp` and `aevia-pkg`. Linux archives also contain the optional playground worker.
- The Rust embedding API compares `Int` values by numeric value across small and arbitrary-precision representations. `Int::as_i64`, `Int::is_zero` and `Int::is_one` are runtime methods.
- For an existing registry, review the GitHub account binding migration and keep the database and package archive backups together. [Deployment guide](platform/docs/DEPLOY.md)

### 科学计算

- 并行工作线程和主解释器使用同一份已检查程序。解析字段较多的记录及较长的文档注释时，避免重复扫描已处理的字段或文本。

- 经过检查的 Core IR 解释器处理类型化算子分派、显式尾调用和量纲依据。整数除法、余数、`gcd`、`lcm` 和 `to_int` 保持任意精度。精确有理数输入保留精确余数。`range` 接受任意精度端点，并在分配前检查结果大小。浮点张量运算可以使用矢量化与并行内核。
- Aevia 根据 RSSN 内核清单生成适配器，覆盖带量纲值、数值回调、结构化模型结果和会话所属对象。
- 模块缓存用 bincode-next 原生编码和模式指纹保存已检查的接口与类型。工具链会重建不兼容的缓存项。
- 依赖解析可提早排除候选项更多的包。模块加载器索引已解析的依赖边，缓存键包含各再导出声明的准确来源。编辑器查询复用同一检查快照中按需建立的模块索引。
- 从 RSSN 清单生成的接口增加信号处理、带权图和低秩分解算法。稠密与稀疏数值任务按当前 CPU 线程池大小调度。
- 数值库支持批量分布累积概率查询、周期 Helmholtz 求解和有限域字节运算。回调桥按内核清单声明的位置传入回调。
- `Interpolate.interp2Batch` 为所有查询点一次准备网格，分别校验两条查询轴的坐标量纲，并保留网格值的量纲。
- 实数正向 FFT 可直接借用连续 Float 输入。RSSN 预备逆实数 FFT 可对多组频谱复用同一固定长度，返回带量纲的样本，并分别报告样本数和频率分量数。
- `Math.NumberTheory.mod_pow` 计算精确模幂。`Stats.Distributions.dist_quantile` 返回保留分布量纲的分位数，包括支撑区间端点。
- RSSN 绑定公开定步长 ODE 末态、按步距采样的 Kerr–Newman 赤道轨道、有限域 Betti 数、融合回归梯度及带种子的 Ising 在线观测。[数值示例](examples/NUMERICAL_GUIDE.zh-CN.md)
- 横磁 FDTD 可返回请求步数结束时带量纲的电场和经过时间。二维 Schrödinger 求解可对带量纲复振幅的有符号时间推进复用相位。`Pde.solve_heat` 的 `adi` 方法支持固定边界的二维温度场。[数值示例](examples/NUMERICAL_GUIDE.zh-CN.md)
- 三角有限元可为重复的带量纲扩散反应求解准备 P1 或 P2 网格。欧拉与浅水配置支持 `Hlle` 通量。[数值示例](examples/NUMERICAL_GUIDE.zh-CN.md)
- Aevia 可在多组带量纲源项间复用预备笛卡尔 Q1 Poisson 算子，在多组源强度间复用预备二维快速多极子树。链式动力学的加速度保留物理单位和输入物体顺序。[数值示例](examples/NUMERICAL_GUIDE.zh-CN.md)
- 预备精确气体 Riemann 求解支持输入单个或多个速度，分别返回带量纲的密度、速度和压力。[数值示例](examples/NUMERICAL_GUIDE.zh-CN.md)
- 预备周期 DG 平流可推进带量纲的节点场。受检的显式 Euler、中点法及 Heun 方法接收有符号步长，并在请求时间返回带量纲状态。[数值示例](examples/NUMERICAL_GUIDE.zh-CN.md)
- 预备超弹性材料点可复用固定变形梯度，返回带量纲的应力、材料切线和能量密度。分子动力学近邻构建支持稀疏的大周期盒。[数值示例](examples/NUMERICAL_GUIDE.zh-CN.md)
- 张量混合索引保留剩余轴的大小。切片下标、端点和步长在运行时保持整数精度。密集张量视图在索引或物化前检查形状和存储边界。密集张量构造函数检查输出大小，`diag` 保留元素表示，`linspace` 在跨度很大的有限区间上保留端点。
- 精确方阵的行列式、逆矩阵和线性求解在任意阶数下保持精确。浮点分解拒绝非有限输入，特征值和 Cholesky 例程检查矩阵的两个轴。任意秩浮点张量的转置共享底层存储。动态重塑会检查各轴长度并返回 `Result`。张量序列操作保留元素类型，包括结果为空的选择。
- 浮点均值和标准差在结果可表示时可处理极端数值。FFT 使用精确整数轴索引，即使未变换的轴为空，实数逆变换也会恢复指定的轴长。SVG、HTML 与 GIF 使用同一套坐标几何绘制线图、散点图、误差棒、直方图和热图。
- 源码规则向 RSSN 发布重写时保留完整的符号参数名称。 源码插件解释器继承调用方的有效能力。
- CSV 和 TSV 读取保留表头单位元数据，并经 IO 结果返回格式错误。表连接仅为右表建一次索引，同时保留匹配行的顺序。
- 类型级自然数在约束求解中保留精确系数。稀疏张量收缩要求每侧的轴不重复，包括运行时求得的轴。
- 泛型记录更新保留开放行，并通过函数别名和模块接口传递字段排除约束。解释器更新唯一拥有的记录时会复用其存储。
- 并行任务使用配置的线程数。跨线程传输保留构造器来源、张量类型和显式特化的泛型闭包。工作线程遵守任务提交时的权限范围，`await` 统一报告任务取消。
- `ParMap` 的工作线程共享连续浮点张量存储，并保留量纲、显示单位和输入次序。
- `Map2` 与 `ZipWith` 按回调需要读取张量输入，首个回调失败时也遵守这一顺序。
- 默认线程数根据可用 CPU 和 Rayon 配置选择。现有线程池满足本次工作量与调用方上限时可直接复用。
- 带量纲的 RSSN 调用包括预备 Newton–CG、网格传递和计算带选列。生成的数值帮助与源码覆盖率核验读取运行内核的同一份清单。
- NPY 输入可将 Fortran 布局直接解码到行主序。非单例轴达到三个或更多的大型数组可以使用当前 RSSN CPU 池。含忽略字段的宽记录模式按常规完备性规则检查。
- 并行映射和制表可以用稠密存储返回同质 Float 批次。固定签名的 RSSN 原生调用可以借用连续的 Float 输入，并返回带量纲的稠密张量。

- Float 张量的逐元素加、减、乘共用 RSSN 的 SIMD 向量内核。[带量纲向量更新示例](examples/measured_vector_updates.aev)展示如何把速度修正应用到一组位置。
- RSSN 多项式模型可以拟合带量纲横轴、复合模型和批量求值。[校准示例](examples/polynomial_calibration.aev)展示 Aevia 如何跟踪两个模型的时间和距离量纲。

- [Kalman 跟踪](examples/kalman_tracking.aev)、[状态空间响应](examples/measured_state_space.aev)和[三对角弹簧链](examples/tridiagonal_spring_chain.aev)使用 RSSN 的带量纲强类型接口。
- 生成的 PDE 事件接口按声明位置接收强类型回调，并检查事件结果的量纲。[精确二进制算术](examples/dyadic_real.aev)、[双曲坐标](examples/hyperbolic_charts.aev)与[魏尔斯特拉斯周期](examples/weierstrass_periods.aev)示例展示了命名结果和数值契约。

### 工具链与注册中心

- 语言服务器复用行索引、文档大纲与语义标记，并按声明标识提供文档内高亮。格式化直接处理打开的文本。
- 包解析器在有序版本目录中二分定位约束区间。注册中心文档复用 HTML 清理策略，并直接采用编译器提取的模块摘要与可见性标记。
- Windows 上的包缓存并发刷新遇到临时文件替换冲突时会重试，期间保留可读取的上一份索引。
- lint 自动修复在合并相同分支时保留条件求值。SARIF 报告声明源码列坐标单位。注册中心的 HEAD 请求不会增加下载次数。
- CLI、REPL、语言服务器、格式化和 lint 工具共用语言前端。适配编辑器可检查未保存的源码、运行和测试文件、打开 REPL，并使用断点、单步和变量检查功能。
- `aevia add --as` 支持为本地路径依赖设置别名。命令在写入清单或锁文件前拒绝多余的包参数。
- 补全根据局部作用域识别同名绑定。编辑嵌套调用或字符串实参时，签名帮助会跟随当前参数。发布草稿包含带版本号的 VS Code 与 JetBrains 扩展归档。
- 注册中心提供签名发布、包搜索、自动生成的文档和透明日志。GitHub 发布者和组织授权使用提供方的稳定账户 ID。
- 标准库参考、教程、架构手册与规范均提供中文和英文版本。注册中心支持文档热重载与独立 HTML 下载。运维人员可通过独立的无根 Podman 工作进程启用教程示例执行。

### 安装与升级

- 安装器和 CLI 工具链管理器验证 SHA-256 和 GitHub 发布证明。最新稳定版、每日构建和指定版本可同时安装。切换版本同时影响语言 CLI、包管理命令和语言服务器。
- 源码构建需要 Rust 1.100 或更新版本，以及配套的 RSSN 提交。发布归档包含 `aevia`、`aevia-lsp` 和 `aevia-pkg`。Linux 归档还包含可选的示例执行工作进程。
- Rust 嵌入 API 中，`Int` 按数值比较机器整数和任意精度表示。`Int::as_i64`、`Int::is_zero` 和 `Int::is_one` 是运行时方法。
- 升级现有注册中心前，按步骤审核 GitHub 账户绑定迁移，并配套保留数据库与包归档备份。[部署指南](platform/docs/DEPLOY.zh.md)

## [0.1.0-rc.1]

The release candidate establishes the dimension-typed language, RSSN bridge and local development tools. The release notes above cover the transition to the beta architecture.

该候选版本建立带量纲类型的语言、RSSN 桥接和本地开发工具。上方发布说明概述向 beta 架构的迁移。
