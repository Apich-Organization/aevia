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

## 使用包

```console
aevia init model
cd model
aevia check --all
aevia run
aevia test
```

清单 `aevia.toml` 定义入口与依赖。[包指南](platform/manuals/book/zh/10-modules-and-packages.md)说明本地依赖、锁文件、文档和发布。[aevia-pkg](platform/README.md) 提供注册表访问和签名发布。

## 安装 beta 版

`v0.1.0-beta.1` 公开发布后，从 [GitHub 发布页](https://github.com/Apich-Organization/aevia/releases) 下载与操作系统和处理器架构对应的归档。用同名 `.sha256` 文件校验归档，解压后将 `aevia` 所在目录加入 `PATH`。归档同时包含 `aevia-lsp`、`aevia-pkg`、语言规范和示例。

在源码检出目录中运行安装脚本时，请显式指定 beta 版本。脚本默认查询稳定版。

```console
sh platform/install.sh --version 0.1.0-beta.1
```

```powershell
./platform/install.ps1 -Version 0.1.0-beta.1
```

## 从源码构建

使用 Rust 1.100 或更新版本。将 [RSSN](https://github.com/Apich-Organization/rssn/tree/v0.3) 放在本仓库旁，使 `aevia` 与 `rssn` 位于同一个父目录。选择与所构建 Aevia 版本配套的 RSSN 修订。

```console
cargo build --locked -p aevia-cli
cargo build --locked --manifest-path platform/Cargo.toml -p aevia-pkg
```

平台有独立 Cargo 工作区和[部署指南](platform/docs/DEPLOY.md)。语言工作区使用 `cargo test --locked --workspace` 验证。定向检查时选定所改 crate 或特定测试目标。`cargo +nightly fmt --all --check` 检查语言工作区的格式。

使用 `python3 tools/bundle_spec.py` 与 Pandoc 生成单页规范。使用 `aevia-stdlib-gen` 生成并检查标准库源码。`aevia doc --stdlib --json` 生成平台的双语标准库参考。添加 `--lang zh` 可以输出中文 Markdown 页面。

## 许可

[Apache License 2.0](LICENSE)。
