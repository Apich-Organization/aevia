# Language pipeline and tooling

Source travels through one front end so the command line and editor agree on the meaning of a file.

```text
source + package context
  → resolution → tokens and layout → AST
  → name, type, dimension and shape checking → CheckedModule
  → Core IR → evaluator → values, IO actions and diagnostics
```

## What the front end preserves

The lexer records source spans and the parser turns layout into module and expression structure. Name lookup, dimensional interpretation and overload selection depend on the import environment, so they take place in the checker. The module driver resolves imports by context and visits dependencies in post-order. The diagnostics can report a resolved cycle with its import edges, even if the same short module name exists in several packages. CLI, REPL and LSP provide different initial source sets and share this loader.

```text
Module graph in dependency order
    → declarations: dimensions, units, types
    → signatures: names and forward references
    → bodies: type, row, shape, effect and dimension constraints
    → defaulting/generalization → exports + typed side tables
                         │                  │
              imported by dependants       └→ lower checked code
```

Function evidence records requirements that cannot be executed until a generic argument is known. Class operations track dictionary choices, and RSSN calls can record a stable overload key. Array shapes and dimensions are carried by checked types, with validation at dynamic indices and host interfaces. Nominal constructors preserve declaration identity across module boundaries, even when two declarations use identical field names.

## Values, scheduling and effects

Lowering builds slot-oriented IR with explicit bindings and closure captures. A tail call replaces the active call through the trampoline. Both the IR evaluator and the typed AST evaluator use the same arithmetic, IO, pattern and kernel operations. Checked programs construct IO actions that the host runs at effect boundaries. Capabilities flow from the caller into source plugins and portable tasks. Mutable host objects and kernel handles belong to a session and thread.

Parallel evaluation uses interpreter replicas, because the regular value graph contains thread-local reference-counted objects. Transferable data has a portable representation. Workers reconstruct closures using the corresponding code identity, captures and supported evidence. Parallel mapping restores input order and reductions use fixed block boundaries. An operation that cannot run with portable values can take its ordinary interpreter path. Opt-in JIT code is restricted to eligible scalar functions with guards. Source closure identity and lexical bindings determine whether a cached native call remains applicable.

## Entry points for tooling

| Work | Source |
|---|---|
| Load one entry, a workspace or overlays | `aevia-core/src/driver.rs` |
| Checked exports, evidence and constraint propagation | `aevia-types/src/check.rs`, `infer.rs`, `check/` |
| Lower, optimize, evaluate | `aevia-ir/src/lower.rs`, `optimize.rs`, `aevia-core/src/eval_ir.rs` |
| Completion, rename, diagnostics and positions | `aevia-analysis/src` and `aevia-lsp/src/server.rs` |
| Runtime breakpoints, stepping and exception information | `aevia-core/src/debugger.rs` and `aevia-cli/src/debugger.rs` |

The editor analysis tracks edit overlays and positions in the protocol encoding. Source operations such as formatting and documentation derive from the syntax tree. Runtime debugging uses executing frames and spans, while run and test commands launch the toolchain against saved sources. This keeps a stale buffer from silently replacing the file selected for execution. [Debugging tutorial](/book/14-diagnostics-and-debugging?lang=en)
