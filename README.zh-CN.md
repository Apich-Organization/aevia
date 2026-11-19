# Aevia

[English](README.md)

Aevia 是一门面向科学计算的函数式语言，将量纲纳入类型系统。程序运行前即可发现 `3 m + 2 s` 之类的错误。RSSN 提供符号变换、数值算法和物理单位元数据。

## 快速开始

[从源码构建 `aevia`](#从源码构建)，也可参阅[在线安装指南](https://aevia.apich.org/cli?lang=zh)。用 `aevia --version` 确认安装，然后运行 `aevia` 进入交互提示符。

```text
2 m + 30 cm
:type 2 m + 30 cm
```

运行[运动学模型](examples/kinematics.aev)，体验以文件为入口的程序。

```console
aevia check examples/kinematics.aev
aevia run examples/kinematics.aev
```

该模型比较下落时间的精确表达式和浮点近似。改变高度，查看两种形式如何变化。

| 资料 | 用途 |
|---|---|
| [Aevia 教程](platform/manuals/book/zh/01-getting-started.md) · [交互版](https://aevia.apich.org/book?lang=zh) | 通过完整练习和可运行示例学习 |
| [标准库概览](spec/08-stdlib.md) · [在线参考](https://aevia.apich.org/stdlib?lang=zh) | 查找名称、签名、物理量纲和示例 |
| [语言规范](spec/README.md) · [单页 HTML](Aevia-Language-Specification.html) | 查询语法、类型规则和语言语义 |
| [示例](examples/README.zh-CN.md) | 运行完整程序或复现数值验证 |
| [编辑器](editors/README.md) | 配置诊断、导航、任务及受支持的调试功能 |
| [架构](ARCHITECTURE.md) · [变更日志](CHANGELOG.md) | 了解实现边界与版本变化 |

用 `aevia docs --section book --lang zh --open` 打开在线教程，添加 `--download` 获取独立 HTML 的地址。用 `aevia doc --stdlib --lang zh` 生成中文标准库参考。`aevia <command> --help` 说明命令及其选项。

Aevia 自动探测语言并行求值可用的 CPU。`--jobs` 设置单次运行的线程数，`AEVIA_JOBS` 可供当前 shell 会话使用。`RAYON_NUM_THREADS` 控制默认 Rayon 池，未指定 Aevia 线程数时也用作语言工作线程数。 [并行求值与性能测量](platform/manuals/book/zh/12-parallel-and-profiling.md)

文本客户端可从[文档索引](https://aevia.apich.org/llms.txt?lang=zh)读取本地化 Markdown 章节和生成的标准库目录。

## 使用包

```console
aevia init model
cd model
aevia check --all
aevia run
aevia test
```

清单 `aevia.toml` 定义入口与依赖。[包指南](platform/manuals/book/zh/10-modules-and-packages.md)说明本地依赖、锁文件、文档和发布。[aevia-pkg](platform/README.md) 提供注册表访问和签名发布。

## 安装工具链

安装支持 `gh release verify-asset` 的 [GitHub CLI](https://cli.github.com/)，并运行 `gh auth login` 登录。安装脚本先验证 SHA-256 校验和及 GitHub 发布证明，再安装 `aevia`、`aevia-lsp` 和 `aevia-pkg`。脚本会配置 PATH 和 shell 命令补全。

脚本默认安装最新稳定版。等 `v0.1.0-beta.1` 发布后，可显式指定该版本进行安装。

```console
curl -fsSL https://aevia.apich.org/install.sh | sh -s -- --version 0.1.0-beta.1
```

```powershell
& ([scriptblock]::Create((irm 'https://aevia.apich.org/install.ps1'))) -Version 0.1.0-beta.1
```

在 macOS 和 Linux 上使用 `--channel nightly`，在 Windows 上使用 `-Channel nightly`，即可安装最新每日构建。使用 `--version nightly-YYYY-MM-DD` 或 `-Version nightly-YYYY-MM-DD` 可以固定到特定日期的构建。同样的选项适用于源码目录中的 [Unix 安装脚本](platform/install.sh)和 [PowerShell 安装脚本](platform/install.ps1)。

首次安装后，可在终端直接管理工具链。

```console
aevia toolchain install nightly --no-activate
aevia toolchain list
aevia toolchain run nightly-YYYY-MM-DD --version
aevia toolchain update stable
```

用 `list` 显示的每日构建日期替换示例中的日期。运行 `aevia toolchain use VERSION` 可切换到已安装的版本。运行 `aevia toolchain update` 可跟进当前的稳定版或每日构建频道。固定到特定版本后，该版本会保持启用，直至主动切换。

如需手动安装，从 [GitHub 发布页](https://github.com/Apich-Organization/aevia/releases) 下载与操作系统及处理器架构对应的归档。用同名 `.sha256` 文件比对校验和，并运行 `gh release verify-asset TAG ARCHIVE --repo Apich-Organization/aevia` 核验发布证明。解压后，将可执行文件所在目录加入 PATH。归档还包含语言规范和示例。

## 从源码构建

使用 Rust 1.100 或更新版本。将 [RSSN](https://github.com/Apich-Organization/rssn/tree/v0.3) 放在本仓库旁，使 `aevia` 与 `rssn` 位于同一个父目录。选择与所构建 Aevia 版本配套的 RSSN 修订。

```console
cargo build --locked -p aevia-cli
cargo build --locked --manifest-path platform/Cargo.toml -p aevia-pkg
```

平台有独立 Cargo 工作区和[部署指南](platform/docs/DEPLOY.zh.md)。语言工作区使用 `cargo test --locked --workspace` 验证。定向检查时选定所改 crate 或特定测试目标。`cargo +nightly fmt --all --check` 检查语言工作区的格式。

使用 `python3 tools/bundle_spec.py` 与 Pandoc 生成单页规范。使用 `aevia-stdlib-gen` 生成并检查标准库源码。`aevia doc --stdlib --json` 生成参考页面及 JSON 索引。用 `--lang en` 生成仓库中的参考文件，用 `--lang zh` 输出中文页面。 [双语参考检查](tools/check_stdlib_docs.py)

## 许可

[Apache License 2.0](LICENSE)。
