# Language pipeline and tooling

Source travels through one front end so the command line and editor agree on the meaning of a file.

```text
source + package context
  → resolution → tokens and layout → AST
  → name, type, dimension and shape checking → CheckedModule
  → Core IR → evaluator → values, IO actions and diagnostics
```

## Front end and loading

`aevia-resolve` provides the dependency model. `aevia-core::driver` walks the import graph and orders modules by their dependencies. A module in a dependency resolves its own imports in that dependency's context. The loader tracks file identities and reports cycles along the resolved graph. Embedded standard-library sources and the materialized RSSN namespace enter through the same checking boundary as local sources.

`aevia-syntax` retains spans and leaves unit expressions for later resolution. `aevia-dim` implements exact exponent algebra and the unit environment. `aevia-types` relates representations, dimensions, rows and tensor shapes during checking. A checked module retains evidence about selected class instances, dimension witnesses and kernel overloads for later stages.

## Evaluation and performance

`aevia-ir` lowers checked expressions to explicit bindings and closure environments. The evaluator uses slots for local values and explicit tail positions. The typed-AST evaluator provides a reference for behavior comparison. Batching and parallel primitives can reuse typed paths across work items. Observable output order and reduction semantics belong to the language contract.

Most `Int` operations can use inline signed integers. Values promote to arbitrary precision when required. Dense floating-point tensor paths separate dispatch from per-element arithmetic. Exact rationals and symbolic expressions remain exact until the user requests approximation. With the JIT feature, eligible scalar functions lower through RSSN to native code and fall back to the evaluator when a runtime guard fails.

## Editor and documentation interfaces

`aevia-analysis` turns compiler data into hover, completion, rename and other semantic queries. `aevia-lsp` manages JSON-RPC, open-file overlays and publication of diagnostics. Formatter and documentation tools use the syntax tree. The formatter checks syntax-tree fingerprints to protect program meaning. The documentation tool uses `##` comments and extracts fenced blocks for checking through the CLI.
