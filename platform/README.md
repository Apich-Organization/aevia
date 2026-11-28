# Aevia platform

The platform serves the package registry, hosted package documentation, the language specification, and the Aevia Book. The separate Cargo workspace includes the `aevia-pkg` client and the registry’s Rust server, PostgreSQL queries, documentation renderer, and HTML views.

| Read | Open |
| --- | --- |
| Learn the language | [The Aevia Book](manuals/book/en/01-getting-started.md) · [Interactive edition](https://aevia.apich.org/book?lang=en) |
| Look up a library, function, or unit | [Standard library](stdlib-docs/stdlib.md) · [Hosted package docs](https://aevia.apich.org/docs) |
| Find or publish a package | [Registry](https://aevia.apich.org/crates) · [CLI guide](https://aevia.apich.org/cli) |
| Understand the system | [Language specification](../spec/en/README.md) · [Architecture](manuals/architecture/en/05-registry.md) |
| Operate or integrate the registry | [Deployment](docs/DEPLOY.md) · [HTTP API](docs/API.md) · [Security](docs/SECURITY.md) |

## Install the language tools

The release installers require GitHub CLI with `gh release verify-asset` and a session from `gh auth login`. Run the installer for your operating system from the repository root. These examples select a particular beta release.

```sh
sh platform/install.sh --version 0.1.0-beta.1
```

```powershell
.\platform\install.ps1 -Version 0.1.0-beta.1
```

The default channel is `stable`. Use `--channel nightly` or `-Channel nightly` to select a daily build. Linux and macOS install to `~/.aevia`. Windows installs to `%LOCALAPPDATA%\aevia`. Set `AEVIA_HOME` to override the default, or pass `--home` on Linux or macOS and `-HomeDirectory` on Windows. Use `--no-activate` or `-NoActivate` to install alongside your active toolchain. Use `--no-path` or `-NoPath` to skip PATH and shell completion changes. [Linux and macOS installer](install.sh) · [Windows installer](install.ps1)

## Develop the registry

From the repository root, run `bazel run //:platform_dev -- run` to start the registry at `http://127.0.0.1:8080`. Set `AEVIA_BIND` and `AEVIA_BASE_URL` to use another listener and public origin. The script prefers Podman and also supports Docker. It starts a PostgreSQL 18 container named `aevia-dev-db` on `127.0.0.1:55432`. The registry stores local archives in `platform/target/packages` and reads the specification from the repository’s `spec/`. Use `bazel run //:platform_dev -- up` to start only the database and print the environment exports. The existing `platform/scripts/dev.sh` command forwards to this target.

[`Cargo.toml`](Cargo.toml) declares the minimum supported Rust version. The platform builds the parser, formatter, documentation, dimension, and dependency resolution crates from the language workspace. Build the registry image from this checkout with `bazel run //:registry_image -- --tag localhost/aevia/registry:0.1.0`.

```sh
bazel test //:platform_unit_test
AEVIA_TEST_DATABASE_URL=postgres://aevia:aevia@127.0.0.1:55432/aevia \
    bazel test --test_env=AEVIA_TEST_DATABASE_URL //:platform_test
```

The database integration tests create a separate schema for each test and require PostgreSQL 18 and permission to create schemas. The integration target creates a disposable OpenPGP signing key for publish tests.

## 中文

平台提供包注册中心、托管包文档、语言规范和 Aevia 教程。`platform/` 是独立的 Cargo 工作区，包含 `aevia-pkg` 客户端、Rust 服务端、PostgreSQL 查询、文档生成和 HTML 页面。

| 所需内容 | 入口 |
| --- | --- |
| 学习语言 | [Aevia 教程](manuals/book/zh/01-getting-started.md) · [交互版](https://aevia.apich.org/book?lang=zh) |
| 查询库、函数或单位 | [标准库概览](../spec/08-stdlib.md) · [托管包文档](https://aevia.apich.org/docs?lang=zh) |
| 查找或发布包 | [注册中心](https://aevia.apich.org/crates?lang=zh) · [命令行指南](https://aevia.apich.org/cli?lang=zh) |
| 了解系统设计 | [语言规范](../spec/README.md) · [架构](manuals/architecture/zh/05-registry.md) |
| 运维或集成注册中心 | [部署](docs/DEPLOY.zh.md) · [HTTP API](docs/API.zh.md) · [安全](docs/SECURITY.zh.md) |

### 安装语言工具

发布安装脚本需要支持 `gh release verify-asset` 的 GitHub CLI，并要求提前运行 `gh auth login`。在仓库根目录执行上面的 Linux、macOS 或 Windows 命令即可安装指定 beta 版本。默认渠道为 `stable`。可用 `--channel nightly` 或 `-Channel nightly` 选择每日构建。

Linux 和 macOS 的默认安装目录是 `~/.aevia`，Windows 的默认目录是 `%LOCALAPPDATA%\aevia`。可设置 `AEVIA_HOME`，或分别通过 `--home` 和 `-HomeDirectory` 指定目录。使用 `--no-activate` 或 `-NoActivate` 可并行安装其他版本并保留当前版本为活动工具链。使用 `--no-path` 或 `-NoPath` 可跳过 PATH 和 Shell 补全设置。 [Linux 和 macOS 安装脚本](install.sh) · [Windows 安装脚本](install.ps1)

### 开发注册中心

在仓库根目录运行 `bazel run //:platform_dev -- run`，即可通过 `http://127.0.0.1:8080` 访问注册中心。如需使用其他监听地址或公开访问地址，可设置 `AEVIA_BIND` 和 `AEVIA_BASE_URL`。脚本优先使用 Podman，也支持 Docker。它会在 `127.0.0.1:55432` 启动名为 `aevia-dev-db` 的 PostgreSQL 18 容器。注册中心将本地包归档写入 `platform/target/packages`，并从仓库的 `spec/` 读取语言规范。仅需数据库时，运行 `bazel run //:platform_dev -- up`，命令会输出环境变量设置。原有 `platform/scripts/dev.sh` 会转发到该 Bazel 目标。

[`Cargo.toml`](Cargo.toml) 声明了最低 Rust 版本。平台复用语言工作区中的解析、格式化、文档、量纲及依赖解析 crate。在仓库根目录运行 `bazel run //:registry_image -- --tag localhost/aevia/registry:0.1.0` 构建注册中心镜像。

```sh
bazel test //:platform_unit_test
AEVIA_TEST_DATABASE_URL=postgres://aevia:aevia@127.0.0.1:55432/aevia \
    bazel test --test_env=AEVIA_TEST_DATABASE_URL //:platform_test
```

数据库集成测试需要 PostgreSQL 18 和创建模式的权限，每项测试使用独立模式。集成测试目标会为 OpenPGP 发布测试创建一次性签名密钥。
