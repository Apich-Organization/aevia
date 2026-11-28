# 用 Bazel 构建 Aevia 和 RSSN

[English](BAZEL.md)

如果可执行文件名为 `bazelisk`，可用它替换示例中的 `bazel`。在 Windows 上，如果默认卷无法创建目录联接，请用 `--output_user_root` 指向 NTFS 卷。

两个仓库各自保留模块和锁文件。RSSN 可以独立构建。Aevia 的 Bazel 模块默认读取同级的 `../rssn`，因此未提交的 API 和实现改动也会进入本地构建。CI 和发布继续检出工作流解析并验证的 RSSN 提交。两个仓库使用 Bazel 7.7.1、Python 3.11+ 和 Cargo 清单声明的 Rust 工具链。

```text
work/
  aevia/    MODULE.bazel
  rssn/     MODULE.bazel
```

可从 Aevia 选择任意已有的 RSSN 工作树。Bazel 会把声明的输入放到一个稳定、并列的本地镜像中，保留 Cargo 路径依赖的语义。

```sh
bazel test --override_module=rssn=/work/rssn-worktree //:joint_test
```

| 命令 | 用途 |
| --- | --- |
| `bazel build //:toolchain` | 构建 CLI 和 LSP |
| `bazel run //:aevia -- --version` | 运行 Bazel 构建的 CLI |
| `bazel run //:aevia -- rssn --manifest-json > manifest.json` | 导出实际链接的 RSSN 清单，供审计与绑定工具使用 |
| `bazel run @rssn//:refresh_bindings_from_manifest -- --manifest manifest.json` | 用导出的清单刷新实际链接版本的各语言接口 |
| `bazel run //:check` | 检查 Rust 工作区，省去测试可执行文件的链接 |
| `bazel test //:test` | Aevia Rust 主测试、安装和工具链测试、契约检查 |
| `bazel test //:joint_test` | Aevia 和 RSSN 主测试及两边的契约检查 |
| `bazel test //:integration_test` | 两个仓库的全特性、数据库、编辑器和语言绑定集成测试 |
| `bazel test //:rust_all_features_test` | Aevia 全特性 Rust 测试 |
| `bazel test //:joint_all_features_test` | RSSN 和 Aevia 的全特性 Rust 测试 |
| `bazel test //:platform_scripts_test` | 模拟部署与开发脚本测试、平台依赖镜像校验 |
| `bazel test //:platform_test` | PostgreSQL 平台测试和无编译器特性的文档包测试 |
| `bazel test //:editor_config_test //:editor_protocol_test` | 编辑器配置、Tree-sitter 解析器以及 LSP/DAP 协议测试 |
| `bazel test //:editor_kate_test //:editor_zed_runnables_test //:editor_zed_wasm_test` | Kate 高亮、Zed 查询及 WASI 扩展 |
| `bazel test //:editor_package_test` | VS Code 和 JetBrains 扩展打包及内容验证 |
| `bazel run //:editor_packages` | 生成可分发的编辑器扩展，默认写入 `dist/editor-extensions/` |
| `bazel test @rssn//:bindings_test` | RSSN 的四语言绑定测试 |
| `bazel test //:clippy_test //:platform_clippy_test` | 对语言和平台工作区执行严格 Clippy |
| `bazel test //:release_inputs_test` | 验证发布源码溯源和 Linux ABI 检查 |
| `bazel run //:refresh_kernel` | 按顺序刷新 RSSN 生成代码、Aevia API 清单、标准库源和覆盖报告 |
| `bazel run //:refresh_stdlib` / `bazel run //:refresh_spec` | 分别刷新标准库源或规格文档 |

运行平台数据库测试时，请设置 `AEVIA_TEST_DATABASE_URL` 指向可供测试的数据源。测试自行建立临时 GPG 密钥。编辑器目标根据所选测试使用 Node.js、PyYAML、Vim、Emacs、Helix、`ksyntaxhighlighter6` 或 `tree-sitter`。Zed WASI 测试需要 `wasm32-wasip2` 和 Clippy。打包需要 npm、Java 21 和 Gradle。RSSN 的 header、manifest、witness 和绑定更新目标也可以从 Aevia 使用，例如 `bazel run @rssn//:refresh_witness` 或 `bazel build @rssn//:manifest`。更新命令只在生成成功后发布指定文件，并会检查生成期间是否有人改动同一文件。

## 发布与测速

```sh
bazel run --config=release-max //:package_release -- \
  --distribution 0.1.0-beta.1 --rssn-commit "$(git -C ../rssn rev-parse HEAD)"
```

Bazel 在构建发布二进制文件前运行 RSSN 的 LLVM 流水线探针。发布启用 Aevia 和 RSSN 的全部特性、LLVM 插件、fat LTO 和单个 CGU。x86-64 使用包含 AVX2/FMA 的 x86-64-v3，AArch64 使用 Advanced SIMD（NEON）。目标在 `dist/` 生成并验证发行包与校验文件，运行包内的 CLI、LSP 和 DAP，并检查 NumPy `.npy` 读写、Cranelift JIT、AST 和 IR。Linux 需要 `mesa-vulkan-drivers` 提供 Lavapipe，以验证 wgpu 适配器。CI 从干净的 Aevia 和 RSSN 工作树构建。打包时检查指定的提交，并核对各二进制构建用的源码快照。两种 Linux 架构的包均包含 registry 和 playground。Bazel 用同一份原生发布二进制构建并验证 registry 镜像，CI 附上两种镜像的归档，并为版本发布生成多架构镜像清单。

`bazel run --config=release-max //:editor_packages` 打包 VS Code、JetBrains、Zed 的 WASI 扩展，以及 Emacs、Helix、Kate、Sublime Text、Tree-sitter 和 Vim 的支持文件。CI 把测试过的编辑器包及校验文件附到发布。

Linux 发布运行器使用 Ubuntu 24.04。打包目标通过 `readelf` 检查发布的二进制文件所需的 glibc 版本，要求 2.39 或更早。

只需 CLI 和 LSP 时可构建 `//:toolchain`，只需 registry 客户端时可构建 `//:platform_cli`。可用 `bazel run //:release_smoke -- --archive /path/to/aevia.tar.gz` 复验发行包。`//:package_release_from_binaries` 接收 `--toolchain-bin` 和 `--platform-bin`，对现有二进制文件运行相同的打包和验证步骤。发布构建需要与 `rustc -vV` 完全匹配的宿主 LLVM SDK，可用 `--repo_env=RSSN_LLVM_CONFIG=/path/to/llvm-config` 指定。

Windows 构建机还需用 `--repo_env=RSSN_LLVM_IMPORT_LIBRARY` 指定宿主 LLVM C++ API 的导入库。需要选用兼容工具链时，可传入 `--repo_env=RUSTC` 与 `--repo_env=CARGO`。CI 可通过 `RSSN_RELEASE_LLVM_CONFIGS`、`RSSN_RELEASE_LLVM_IMPORT_LIBRARIES`、`RSSN_RELEASE_RUSTCS` 和 `RSSN_RELEASE_CARGOS` 按 Rust 目标三元组配置路径。

`--config=release` 使用 ThinLTO 和 16 个 CGU。发布工作流使用 `--config=release-max` 启用 fat LTO 和单 CGU。`--config=release-local` 用于测量关闭 LLVM 插件和 LTO 的 O3 构建。通过 `--define=rust_target=<目标三元组>` 和 `--repo_env=RSSN_TARGET_ENV=/absolute/path/to/toolchains.json` 为交叉编译配置目标链接器、SDK 和 C 工具链。Rust 测试可通过配置的 Cargo target runner 执行。安装器和协议测试需在可原生运行对应产物的平台上运行。

`--define=rssn_timings=1` 记录 Cargo 各任务的耗时，`--profile=/path/profile.gz` 记录 Bazel action 耗时。默认将本机增量状态放到 `.bazel-cache/`，Windows 可用 `--repo_env=BAZEL_SCI_CACHE=C:/b/a` 缩短路径。跨平台工具链配置、编译缓存和对照命令见 RSSN 仓库的 `docs/BAZEL.zh.md`。
