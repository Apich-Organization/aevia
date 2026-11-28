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

The checker borrows imported interfaces and indexes direct module bindings by nominal identity. Each re-export uses the identity resolved for its declaration and forwards type and callable evidence from that source. Opening a name uses an indexed lookup for conflict detection, with diagnostics emitted in import order.

Incremental checks reuse syntax when the source text and file identifier match. Each load resolves the current import bindings, then combines fingerprints of the visible transitive interfaces. An edit that preserves a dependency's interface can reuse the importer's prior check. The editor retains a compiler cache with each of its four most recent program snapshots. Doctests in a file share its package roots and checked imports while compiling each snippet as a separate module.

The resolver keeps one reversible priority queue for pending dependencies. Installing or removing a version updates only requirements for the affected package. It builds candidate lists for the selected requirement and limits the amount of conflict detail it retains.

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

The tensor extent solver indexes deferred equations by the variables they still depend on. Binding a variable retries the affected equations in their original order. A deferred contradiction reports the equation that became inconsistent.

## Values, scheduling and effects

Lowering builds slot-oriented IR with explicit bindings and closure captures. A tail call replaces the active call through the trampoline. For wide calls that release the current frame, the evaluator indexes the final argument use of each local before moving values. Compact frames use a reusable slot buffer, while sparse arguments use an index proportional to the number of arguments. Both the IR evaluator and the typed AST evaluator use the same arithmetic, IO, pattern and kernel operations. Checked programs construct IO actions that the host runs at effect boundaries. Capabilities flow from the caller into source plugins and portable tasks. Mutable host objects and kernel handles belong to a session and thread.

During Boolean constant propagation, conditional arms reuse the incoming slot facts and roll back arm-local writes before the next arm. Statically dead arms and short-circuit operands take no optimization work.

Parallel evaluation uses interpreter replicas, because the regular value graph contains thread-local reference-counted objects. The CLI and library runner share the same immutable checked program with the replicas. Workers initialize their own scopes and reconstruct portable closures using the corresponding code identity, captures and supported evidence. Parallel mapping restores input order and reductions use fixed block boundaries. An operation that cannot run with portable values can take its ordinary interpreter path. Opt-in JIT code is restricted to eligible scalar functions with guards. Source closure identity and lexical bindings determine whether a cached native call remains applicable.

## Entry points for tooling

| Work | Source |
|---|---|
| Load one entry, a workspace or overlays | `aevia-core/src/driver.rs` |
| Checked exports, evidence and constraint propagation | `aevia-types/src/check.rs`, `infer.rs`, `check/` |
| Lower, optimize, evaluate | `aevia-ir/src/lower.rs`, `optimize.rs`, `aevia-core/src/eval_ir.rs` |
| Completion, rename, diagnostics and positions | `aevia-analysis/src` and `aevia-lsp/src/server.rs` |
| Runtime breakpoints, stepping and exception information | `aevia-core/src/debugger.rs` and `aevia-cli/src/debugger.rs` |

The editor analysis tracks edit overlays and positions in the protocol encoding. Source operations such as formatting and documentation derive from the syntax tree. Runtime debugging uses executing frames and spans, while run and test commands launch the toolchain against saved sources. This keeps a stale buffer from silently replacing the file selected for execution. [Debugging tutorial](/book/14-diagnostics-and-debugging?lang=en)

Repeated queries reuse outlines, semantic tokens and source indexes attached to the cached program snapshot. Clients can request semantic tokens for the whole document or for a visible range. The first cursor query builds the index it needs for the module's name occurrences, lexical scopes or checked expressions. Outline and navigation ranges share the module's line index. Formatting parses the buffer directly and skips loading and checking its imports. Document highlights use the same declaration identity as references and return only occurrences in the focused buffer. A change to a loaded input creates a new snapshot with its own indexes.
