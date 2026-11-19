# Aevia changelog · 变更日志

The entries cover the language toolchain, the package registry and their documentation. 本文记录语言工具链、包注册中心及其文档的变化。[Language specification](spec/en/README.md) · [语言规范](spec/README.md)

## [0.1.0-beta.1]

### Scientific computing

- The checked Core IR interpreter handles typed operator dispatch, explicit tail calls and dimension evidence. Integer division, remainders, `gcd`, `lcm` and `to_int` preserve arbitrary precision. Exact rational inputs keep exact remainders. `range` accepts arbitrary-precision endpoints and checks the result size before allocating. Floating-point tensor execution can use vectorized and parallel kernels.
- The RSSN Kernel Manifest drives Aevia adapters for dimensioned values, numerical callbacks, structured model results and session-owned objects.
- The module cache encodes checked interfaces and types with native bincode-next traits and a schema fingerprint. The toolchain rebuilds incompatible entries.
- Mixed tensor selectors keep the extents of surviving axes. Slice indices, bounds and steps keep integer precision at runtime. Dense tensor views check their shape and storage boundaries before indexing or materializing the tensor. Dense constructors validate output size, `diag` preserves the element representation, and `linspace` preserves endpoints across large finite intervals.
- Square matrix determinants, inverses and solves keep exact operands exact at every size. Floating-point factorizations reject non-finite input, and eigenvalue and Cholesky routines validate both matrix extents. Floating-point transposes of any rank share their backing storage. Reshaping checks every runtime extent and returns `Result`. Tensor sequence operations retain the element type, including for empty selections.
- Floating-point mean and standard deviation stay finite where the result is representable across extreme scales. FFT axes use exact integer indices. The inverse real transform restores the requested extent even with empty untransformed axes. SVG, HTML and GIF render lines, scatter plots, error bars, histograms and heatmaps using the same coordinate geometry.
- Source rules preserve complete symbolic parameter names when publishing rewrites to RSSN. The source plugin interpreter inherits the caller’s active capabilities.
- CSV and TSV readers preserve header units as metadata and return unsupported-format errors through their IO result. Table joins index the right table once and preserve the order of matching rows.
- Type-level natural numbers retain exact coefficients through constraint solving. Sparse tensor contraction requires distinct axes on each operand, including axes computed at runtime.
- Generic record updates preserve open rows and enforce field exclusions across function aliases and module interfaces. The interpreter reuses uniquely owned records when applying an update.
- Parallel tasks honor the configured worker count. Transfers preserve constructor provenance, tensor schemas and explicitly selected generic closures. Workers enforce the submitter’s active capability scope and report cancellation consistently through `await`.

### Toolchain and registry

- The lint fixer preserves condition evaluation when simplifying identical branches. SARIF reports declare their column coordinate units. Registry HEAD requests no longer increment download counts.
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

- 经过检查的 Core IR 解释器处理类型化算子分派、显式尾调用和量纲依据。整数除法、余数、`gcd`、`lcm` 和 `to_int` 保持任意精度。精确有理数输入保留精确余数。`range` 接受任意精度端点，并在分配前检查结果大小。浮点张量运算可以使用矢量化与并行内核。
- Aevia 根据 RSSN 内核清单生成适配器，覆盖带量纲值、数值回调、结构化模型结果和会话所属对象。
- 模块缓存用 bincode-next 原生编码和模式指纹保存已检查的接口与类型。工具链会重建不兼容的缓存项。
- 张量混合索引保留剩余轴的大小。切片下标、端点和步长在运行时保持整数精度。密集张量视图在索引或物化前检查形状和存储边界。密集张量构造函数检查输出大小，`diag` 保留元素表示，`linspace` 在跨度很大的有限区间上保留端点。
- 精确方阵的行列式、逆矩阵和线性求解在任意阶数下保持精确。浮点分解拒绝非有限输入，特征值和 Cholesky 例程检查矩阵的两个轴。任意秩浮点张量的转置共享底层存储。动态重塑会检查各轴长度并返回 `Result`。张量序列操作保留元素类型，包括结果为空的选择。
- 浮点均值和标准差在结果可表示时可处理极端数值。FFT 使用精确整数轴索引，即使未变换的轴为空，实数逆变换也会恢复指定的轴长。SVG、HTML 与 GIF 使用同一套坐标几何绘制线图、散点图、误差棒、直方图和热图。
- 源码规则向 RSSN 发布重写时保留完整的符号参数名称。 源码插件解释器继承调用方的有效能力。
- CSV 和 TSV 读取保留表头单位元数据，并经 IO 结果返回格式错误。表连接仅为右表建一次索引，同时保留匹配行的顺序。
- 类型级自然数在约束求解中保留精确系数。稀疏张量收缩要求每侧的轴不重复，包括运行时求得的轴。
- 泛型记录更新保留开放行，并通过函数别名和模块接口传递字段排除约束。解释器更新唯一拥有的记录时会复用其存储。
- 并行任务使用配置的线程数。跨线程传输保留构造器来源、张量类型和显式特化的泛型闭包。工作线程遵守任务提交时的权限范围，`await` 统一报告任务取消。

### 工具链与注册中心

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
