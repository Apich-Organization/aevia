# Aevia platform

The platform serves the package registry, hosted package documentation, the language specification, and the Aevia Book. The separate Cargo workspace includes the `aevia-pkg` client and the registry’s Rust server, PostgreSQL queries, documentation renderer, and HTML views.

| Read | Open |
| --- | --- |
| Learn the language | [The Aevia Book](manuals/book/en/01-getting-started.md) · [Interactive edition](https://aevia.apich.org/book?lang=en) |
| Look up a library, function, or unit | [Standard library](stdlib-docs/stdlib.md) · [Hosted package docs](https://aevia.apich.org/docs) |
| Find or publish a package | [Registry](https://aevia.apich.org/crates) · [CLI guide](https://aevia.apich.org/cli) |
| Understand the system | [Language specification](../spec/en/README.md) · [Architecture](manuals/architecture/en/05-registry.md) |
| Operate or integrate the registry | [Deployment](docs/DEPLOY.md) · [HTTP API](docs/API.md) · [Security](docs/SECURITY.md) |

To develop locally, run `./scripts/dev.sh run` from `platform/`. It starts the development PostgreSQL container and runs the registry on `http://127.0.0.1:8080`. The Rust toolchain is pinned by `Cargo.toml`. The platform compiles the parser and documentation crates from the language workspace. It does not compile the scientific kernel to render pages.

```sh
cargo test --locked --workspace
AEVIA_TEST_DATABASE_URL=postgres://aevia:aevia@127.0.0.1:55432/aevia \
    cargo test --locked -p aevia-registry-server --test api
```

The database integration tests create a separate schema for each test and require PostgreSQL 18 and permission to create schemas. To exercise the OpenPGP publish path, supply a disposable `GNUPGHOME` with a signing key.

## 中文

平台提供包注册中心、托管包文档、语言规范和 Aevia 教程。`platform/` 是独立的 Cargo 工作区，包含 `aevia-pkg` 客户端、Rust 服务端、PostgreSQL 查询、文档生成和 HTML 页面。

| 所需内容 | 入口 |
| --- | --- |
| 学习语言 | [Aevia 教程](manuals/book/zh/01-getting-started.md) · [交互版](https://aevia.apich.org/book?lang=zh) |
| 查询库、函数或单位 | [标准库概览](../spec/08-stdlib.md) · [托管包文档](https://aevia.apich.org/docs?lang=zh) |
| 查找或发布包 | [注册中心](https://aevia.apich.org/crates?lang=zh) · [命令行指南](https://aevia.apich.org/cli?lang=zh) |
| 了解系统设计 | [语言规范](../spec/README.md) · [架构](manuals/architecture/zh/05-registry.md) |
| 运维或集成注册中心 | [部署](docs/DEPLOY.zh.md) · [HTTP API](docs/API.zh.md) · [安全](docs/SECURITY.zh.md) |

在 `platform/` 运行 `./scripts/dev.sh run` 可启动本地 PostgreSQL 并运行注册中心，访问地址是 `http://127.0.0.1:8080`。`Cargo.toml` 指定 Rust 工具链版本。平台复用语言工作区的解析和文档生成 crate，渲染页面时无需编译科学计算内核。

```sh
cargo test --locked --workspace
AEVIA_TEST_DATABASE_URL=postgres://aevia:aevia@127.0.0.1:55432/aevia \
    cargo test --locked -p aevia-registry-server --test api
```

数据库集成测试需要 PostgreSQL 18 和创建模式的权限，每项测试使用独立模式。测试 OpenPGP 发布时，请使用带有签名密钥的临时 `GNUPGHOME`。
