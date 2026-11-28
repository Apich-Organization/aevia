# Aevia and RSSN system architecture

Aevia is a dimension-checked scientific language. RSSN is its symbolic and numerical engine and also exposes its own Rust and C APIs. The platform distributes the toolchain, indexes signed packages, and serves the specification, library reference and manuals.

| Boundary | Input | Output |
|---|---|---|
| Aevia front end | UTF-8 source and resolved imports | Typed modules, dimension evidence and diagnostics |
| Aevia runtime | Checked module, arguments and execution options | Values, IO effects and diagnostics |
| RSSN kernel | Terms or typed values, operator signatures and configuration | Reduced terms, numerical values and structured results |
| Registry | Verified package release and metadata | Versioned archives, index, transparency log and generated docs |
| Book execution service | A short standalone program | Bounded output, errors and exit status |

## Repository and dependency structure

The projects are sibling directories for source builds. The language Cargo workspace contains the compiler, tools and adapters. The platform has its own Cargo workspace and lockfile, with dependencies on selected language crates for parsing, package resolution and documentation. The parser used to publish a package's docs is the language parser. The production registry can therefore serve docs without linking the numerical engine.

RSSN owns operator identity and scientific metadata. Aevia owns syntax, language-level typing and runtime behavior. The platform owns the package and deployment protocols. A new RSSN operator usually needs a type or a documented dynamic boundary, rather than a special case in the language parser.

## Choose the chapter by entry point

- [Language pipeline](02-language-toolchain.md) follows a file from source to execution and an editor request back to source.
- [RSSN engine](03-rssn-engine.md) follows a term through hashing, rewriting and extraction.
- [Kernel contracts](04-abi-and-persistence.md) covers the manifest, dimensions and binary compatibility.
- [Registry](05-registry.md) covers packages, documentation and execution isolation.
- [Operations](06-operations.md) covers deployment, backups and serving multiple document editions.
- [Quality and delivery](07-quality-and-delivery.md) covers continuous validation and release artifacts.

The [language specification](/spec?lang=en) is the reference for user-visible semantics. The manuals describe implementation boundaries and operational behavior.

## Runtime and process boundaries

```text
Source file or editor buffer
    → resolution + parser → checked module → Core IR → interpreter
                                                      ↕ checked values
                                                 RSSN session
Browser / aevia-pkg
    → HTTPS origin → Axum registry → PostgreSQL catalogue + durable archives
                          ├→ package documentation queue
                          ├→ mounted documentation snapshots
                          └→ private worker → disposable rootless container
```

The top and bottom flows share package and documentation syntax through path dependencies. They meet at the signed archive and generated interfaces. The web registry never evaluates a published package when it renders that package's documentation. The book runner compiles and executes only the source submitted to its separate endpoint. A developer's interpreter holds local capabilities and kernel objects, while the runner starts a new restricted process for each program.

The CLI starts with one entry file and a module resolution context. The LSP adds source overlays for buffers that have changed in the editor. They share checking, which keeps diagnostic codes and span interpretation consistent. DAP is a runtime inspection protocol. A file can have valid language-server diagnostics and still raise an error when it is executed. The two protocols surface different parts of the same program.

## Authoritative artifacts

| Artifact | Authority | Consumer |
|---|---|---|
| Language behavior | Bilingual specification, checked against executable cases | CLI, editors and users |
| Operator signatures and availability | Installed RSSN tables and generated manifest | Aevia bridge, C hosts and documentation |
| Package identity and bytes | Version record, signed statement and archive digest | Resolver, downloads and transparency log |
| First-party help | Mounted source and generated standard-library catalog | Web editions, standalone HTML and Markdown readers |
| Release identity | Git commit, target triple, lockfiles and immutable attachment | Installer and deployment operator |

A generated artifact carries the representation its consumer needs. The module cache can be discarded and recomputed, a downloaded release can be reverified against its attestation, and a package database restore must be paired with the corresponding archive store. Keeping these lifetimes separate prevents a cache policy from becoming a persistence obligation.

## Source map

Start in `crates/aevia-core/src/driver.rs` for file execution and `crates/aevia-lsp/src/server.rs` for editor requests. Follow `crates/aevia-rssn/src/kernel.rs` at the language boundary and RSSN's `src/api/mod.rs` or `src/kernel/call.rs` on the other side. The platform entry point is `platform/crates/aevia-registry-server/src/lib.rs`. Its database operations, Markdown renderer and HTML views live in separate workspace crates. [Registry internals](05-registry.md) · [Deployment and lifecycle](06-operations.md)
