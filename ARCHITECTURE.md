# System architecture · 系统架构

The [Aevia and RSSN architecture manual](https://aevia.apich.org/architecture?lang=en) follows programs through the compiler, RSSN kernel, package registry, and release pipeline. The same chapters are available as [standalone HTML](https://aevia.apich.org/architecture/download?lang=en).

[Aevia 与 RSSN 架构手册](https://aevia.apich.org/architecture?lang=zh)介绍语言工具链、RSSN 内核、包注册表和交付流程，也提供[独立 HTML](https://aevia.apich.org/architecture/download?lang=zh)。

Sources are in [platform/manuals/architecture](platform/manuals/architecture). The [language specification](spec/en/README.md) defines observable language behavior.

RSSN numerical behavior is implemented in the Rust kernels. The kernel manifest carries the signatures and dimensions consumed by Aevia's `Rssn` bindings. The language implementation and package platform use separate Cargo workspaces. [Kernel bridge](platform/manuals/architecture/en/03-rssn-engine.md) · [ABI and persistence](platform/manuals/architecture/en/04-abi-and-persistence.md) · [API inventory](tools/rssn-api-audit/README.md)

数值行为由 RSSN 的 Rust 内核实现。Aevia 的 `Rssn` 绑定从内核清单读取签名与量纲。语言实现和包平台分别使用两个 Cargo 工作区。 [内核桥接](platform/manuals/architecture/zh/03-rssn-engine.md) · [ABI 与持久化](platform/manuals/architecture/zh/04-abi-and-persistence.md) · [API 清单](tools/rssn-api-audit/README.md)
