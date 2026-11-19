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
