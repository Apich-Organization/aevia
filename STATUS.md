# Implementation status

This file records what the implementation does today, chapter by chapter, and
where it deliberately differs from the design standard. It is kept honest: a
feature is "done" only when a test exercises it, and the test asserts semantics
rather than a rendered string. `PLAN.md` records what the implementation must
do and why.

Last update: after the conformance audit in `.workbuddy-ai/audit/` and the four
work streams that closed its findings (syntax, types, runtime, bridge) plus the
standard library, after the Core IR landed (W3), and after the slice step
(`.workbuddy-ai/audit/slice-step.md`). `cargo test --workspace` runs **790**
tests, all green.

## How to verify

```console
cargo test --workspace -- --test-threads=2   # 790 tests
cargo run -p aevia-cli -- rssn               # manifest audit
cargo run -p aevia-cli -- run examples/calculus.aev
```

## Chapter by chapter

### 01 — Lexical structure (done)

* UTF-8, BOM and shebang, `\n`/`\r\n`/`\r`, and a final line without a trailing
  newline now terminates with `NEWLINE`.
* Layout with `INDENT`/`DEDENT`, bracket suppression, blank and comment-only
  lines. An indentation that matches no enclosing level is `E0102` and the bad
  level is **not** pushed, so the following lines are not mis-attributed.
* Line comments, nested block comments, `##` doc comments.
* Integer literals in four bases with `_`; floats with exponents; exact decimal
  text preserved for `Real` contexts.
* Strings with escapes, interpolation, raw strings and `"""` blocks. Escapes are
  **decoded** in every form (they used to be stripped, so `"\t"` became `t`);
  raw strings stay literal.
* Identifiers compare in NFC (§1.5.1).
* Unit literals are decided by the parser; Unicode operators and superscripts
  are normalised.

### 02 — Syntax (done)

* Every production of appendix A parses. Declarations end with an expected
  `NEWLINE` rather than an optional one, so `let x = 1 let y = 2` is now an
  error instead of two declarations.
* `let … in` is a layout-group body again (it was mis-detected as a block and
  rejected). `instance … where` without a block is `E0103` and no longer steals
  the enclosing `DEDENT`.
* `Matrix<n,m,τ>`, `Sparse<fmt,shape,τ>`, row types with `+`, and the postfix
  `T[d]` are parsed; type arguments carry their kind, so `Vec<L>` is
  representable.
* Duplicate record fields are `E0208` in a literal, an update and a record type.
* Unknown attributes are `E0210`; `@jit`/`@op`/`@rule` reach the checker through
  `Decl::attribute`.
* Syntax-error recovery produces `ExprKind::Error`/`PatKind::Error`, which later
  phases skip, so one syntax error no longer fabricates `E0302`s.

### 03 — Type system (done)

* `Scalar<ρ,δ>`, tensors with rank-only and refined shapes, records with Rémy
  rows **and lacks-constraints**, nominal variants with per-compilation-unit
  ids, `IO`, `Point`, `Unit`, `Dyn`.
* HM inference with levels, bidirectional checking, generalisation at `let` and
  at top level. **Defaulting runs after generalisation, at module end** — it
  used to run before, which silently made every `<d>`-polymorphic function
  monomorphic and emitted a spurious `W0301`.
* **Weak variables** exist: `symbol`/`symfun` allocate a non-generalisable
  dimension variable, so one symbol keeps one dimension for the whole program.
* Representation polymorphism records `ρ ⊒ Int` bounds instead of binding, and
  the errors that used to be discarded with `let _ =` are reported. `^` with a
  negative literal exponent applies `div_rep`, so `2^-1` is `Real`.
* Recursive types are legal and positivity is checked (`E0312`). A `type` name
  is registered before its constructor fields are converted, so
  `type Tree<a> = | Leaf | Node(Tree<a>, a, Tree<a>)` works.
* `let rec` is honoured; a non-`rec` `let` cannot see itself; a top-level `let`
  has one type variable across both checking passes, so a forward reference is
  constrained rather than silently `∀a.a`.
* `Rssn.<op>` in call position is resolved by the lattice of §13.4.1 (arity →
  per-argument score → weakest → unique maximum; tie `E1002`, no match `E1003`)
  and the resolved `OpKey` is recorded in `CheckedModule::rssn_calls`. Outside
  call position it is `E0303`. It used to be typed as overload 0 and resolved
  again at run time.
* Indexing checks the index type and literal bounds against the refined shape,
  and keeps the refinement in the result. `Rank(n) ~ Refined` is rejected.
* **Type classes** are checked: `Scheme` carries constraints, `class`/`instance`
  register, resolution is head-matching with no backtracking, coherence
  (`E0311`) and the orphan rule (`E0604`) are enforced, and the built-in classes
  exist. The checker records the dictionary each call site needs in
  `CheckedModule::dictionaries`, and **the evaluator dispatches through it**: an
  `instance` body runs at the call site the checker resolved, chosen by the
  argument's runtime shape when the record was taken against a type variable.
  A user instance never replaces a prelude one, and a class method is a value
  (`map(pretty, xs)`). An instance head written as a record alias is resolved
  through `CheckedModule::aliases`: an alias has no nominal tag at run time, so
  `type Vec2 = { x, y }` and `{ x: Float, y: Float }` are the same head written
  two ways and must not be allowed to miss.
* A `fn` declaration is generalised **as soon as its body checks**, not at the
  end of the module. Waiting was too late: a call site in between had already
  unified the shared variable, so `fn ident(x) = x` was monomorphic and
  `ident("s")` after `ident(1)` was an error. A `fn` that calls a class method
  is now `Show a => a -> String` and dispatches per argument.

### 04 — Dimensions (done)

Canonical maps over base dimensions and dimension variables with exact rational
exponents; unification with an occurs check that is now reachable; dimension
generalisation; weak variables; diagnostics that carry `left`/`right`/`difference`
and a rendering that parses back to the same dimension.

### 05 — Units (done)

* The unit table is built from `manifest.units` (75 units); `UnitTable::si` is a
  test fixture kept equal to the manifest by a conformance test.
* Exact scales, SI prefixes, affine units (`°C`, `°F`), `%`, astronomy/CGS,
  and the spellings `Ω`/`Å`.
* `Units.Constants` is generated from `manifest.constants` (30 values, 18 exact).
  `hbar` is emitted as an exact expression rather than as 47 f64 digits.
* `unit`/`dimension` declarations, including user base dimensions. The unit and
  dimension tables are snapshotted **after** the user's declarations, so a
  `unit EUR …` is known at run time (it used to type-check and then be unknown).
* **Known gap:** `Da`, `kn`, `mph`, `acre`, `bit`, `B`, `KiB` exist in the
  specification but not in the kernel's registry, so they do not resolve.
  Adding them belongs in the kernel.

### 06 — Semantics (done)

* Strict, left-to-right evaluation; closures; first-match matching.
* **Exactness is a guarantee.** `Float ⊗ Exact → Real`, so `1.0 + 2` is exactly
  `3` and `1/3 + 0.5` is exactly `5/6`. `sqrt`/`exp`/`sin` of an exact argument
  with no exact value stay symbolic, so `sqrt(2)^2` is exactly `2`.
* **Structural equality** compares payloads: `Some(1) == Some(2)` is `false`,
  records compare field values (not renderings), tensors compare shape and
  elements, and functions have no `Eq`.
* **`show` is truthful and `repr` is distinct.** `show` uses the shortest
  round-trip float, so `0.1 + 0.2` prints `0.30000000000000004` — the same value
  its `==` sees. `repr` is round-trippable (exact rationals, quoted strings,
  constructor payloads). `to_latex`/`to_typst` render their own syntax.
* **IO is a description.** `assert`, `random` and `time_now` act when the action
  runs, not when it is built; `?` returns from the enclosing function and is
  `E0702` when it escapes an `IO` block (it used to exit 0 silently).
* `by_cases` builds an `ite` term; `assuming` rejects a false assumption;
  `forall` samples and decides; `decide` compares exact rationals; `approx_eq`
  checks dimensions; `gamma` is Γ, not log Γ; `zeros<Int>`/`ones<Int>` produce
  exact storage.
* `Solutions` carries `variable`, `roots`, `multiplicities`, `conditions` and
  `complete` — it is the kernel's own record, not a flattened root list, so
  `complete` is the solver's claim rather than a constant; `sum(f, a, b)`
  exists; `det`/`inv`/`linsolve` have an exact path
  for exact input (`det([[1,2],[2,4]])` is `0`, not `-0`).
* Tensors: construction, broadcasting, indexing, slicing, `@` by rank,
  reductions. Empty tensors are legal.
* **A slice step is real syntax.** `xs[a:b:s]` parses, types (the step must be
  an `Int`; a known `0` is `E0802` at check time, a computed one at run time)
  and lowers to the same view machinery, so a negative step shares the buffer
  instead of copying it: `xs[::-1]` is `reverse(xs)` with the same offset,
  stride and reversed axis, and `Rc::ptr_eq` holds. The length is the exact
  count `ceil((b - a) / s)` in the walk's direction, so a slice that passes its
  stop at once is empty (`xs[0:0:1]`, `xs[5:2:1]`) rather than one element.
  `a..b` keeps its node, its typing and its lowering, step absent.
* **Recursion is bounded by calls, not AST nodes**, so a legal flat 2100-term
  chain runs and a runaway recursion is `E0805` with a call stack. The `do`
  statement loop and `run_io` are iterative.
* **A tail call is a jump.** A call in tail position — including through an
  `if`, a `match` arm, a `let` chain or a `do` thunk — returns to the evaluator's
  loop instead of recursing, so a self-tail-recursive loop of 1,000,000
  iterations runs on a 2 MiB stack. A tail loop that never terminates is
  `E0805` by a tail-call budget (1,500,000) rather than by depth, because depth
  no longer grows.

### 07 — Modules and packages (done)

* A module is a file; `import A.B`, `import A.B as X`, `import A.B { x, y }`,
  `open A.B`; a cycle is `E0602`; a name list that names something unexported is
  an error; two `open`s bringing the same name in is `E0603`.
* `private` keeps a name out of the interface; a module gets its own scope at
  run time.
* **Packages exist.** `aevia.toml` declares `[package] name/version/entry` and
  path `[dependencies]`; `aevia.lock` is the resolved set, sorted and
  content-hashed, and re-resolving writes byte-identical bytes. `import A.B`
  resolves in the package's `src/`, then the entry's own directory, then a
  dependency's `src/`; `import Lib` with no sub-path names the dependency's
  *entry*, following its `entry` field. A file run with no manifest keeps the
  adjacent-directory root, so every pre-package program behaves as before.
* **No registry.** A version or git dependency is `E0605` naming the line, and
  `--help` says so; `aevia add` takes a path. Pretending to resolve a range
  would be worse than refusing one.

### 08 — Standard library (98.1 % of chapter 8)

`crates/aevia-std` is a real crate: `linalg`, `stats`, `special`, `dist`,
`calc`, `fft`, `interp`, `ode`, `pde`, `complex`, `number`, `geom`, `table`,
`sim`, and `spec` (which reads the name list out of `spec/08-stdlib.md`, so the
coverage test cannot drift from the spec).

313 names are declared in chapter 8: **307 implemented**, 2 documented
capability gaps that raise `E1001` naming the capability (`read_npy`/
`write_npy`, `io.binary`), and 4 blocked on the checker (`Rssn.typed`,
`Rssn.call_ho`, `Rssn.register`, `Rssn.register_plugin`). Every name is typed by
the checker *and* dispatched by the runtime; a name in one and not the other is
a bug the coverage test catches.

The prelude constants `pi`, `e`, `oo`, `nan` and `i` are values. `pi` and `oo`
are kernel constants and stay symbolic, so `2 * pi` is exact; a float `pi` would
be a wrong answer that looks right.

### 09 — Runtime

`aevia-dim → aevia-syntax → aevia-rssn → aevia-types → aevia-ir → aevia-core →
aevia-cli`, plus `aevia-std`. `aevia-ir` depends only on `aevia-syntax` and
`aevia-types`: `Value` lives in `aevia-core`, so an IR crate that depended on it
would be a cycle.

The REPL keeps **one interpreter per session** and appends declarations, so a
top-level `let` is not re-evaluated on every keystroke and `symbol("x")` keeps
its identity across lines.

**The Core IR of §9.2.2 is materialised.** `crates/aevia-ir` holds the IR and
the lowering from the typed AST; `aevia-core/src/eval_ir.rs` interprets it. The
IR is administrative normal form (every intermediate is a named slot, a call
takes atoms, an `if` tests an atom), closure-converted (a lambda captures an
ordered vector and a body reads it by index), and carries a span on every node
and a call site on every call, so `Rssn` overloads and class instances stay
pinned by span (PLAN D1, D3).

The AST evaluator is still there and still selectable: `Interp::with_mode`, or
`AEVIA_EVAL=ast` in the environment. It is a second implementation, not the
reference — the specification is — and `ir_parity.rs` compares the two: 104
programs (every `examples/**/*.aev`, every `tests/*.aev`, every program embedded
in the semantic tests, plus a curated corpus), 102 compared and all 102 agreeing
on output, values and error codes; the two skipped are fragments whose expected
outcome is a *compile* error, so neither evaluator ran. Both paths are run twice,
so a nondeterministic result on either side is skipped rather than counted as
agreement.

That the AST path was never the reference is not a rhetorical point. The
adversarial review found that it shared one mutable scope per block, so a
closure created before a later `let x = …` in the same block read the *later*
binding — the IR was lexically correct and the AST was wrong. Both paths now
extend the scope chain per binding, and three programs of that shape are in the
parity corpus.

Consequences that are now real rather than missing: a variable read is a slot
index instead of a `HashMap` chain; a tail call is a trampoline step instead of
a Rust frame; a call site can name its source location. Still missing: no boxing
decisions or drop points, and no JIT lowering hook, so `@jit` is accepted by the
parser and the checker and does nothing.

**Cost of a tail-call step.** A million-step tail loop
(`fn f(n) = if n == 0 then 0 else f(n - 1)`) runs in about 1.2 s in a release
build, roughly 1.2 µs per step. Two things were removed from that path: the
frame vector is reused across the trampoline instead of allocated per step, and
a parameter is *moved* into its slot instead of cloned (a 96-byte `Value`
carrying a `BigInt` was one allocation per parameter per iteration). The AST
path cannot run the same loop at all — it is `E0805` at depth 2048 — so there is
no before/after comparison to make; the remaining cost is the tree walk, and
getting past it needs a smaller `Value` and unboxed argument passing, which is
not a change to make casually.

### 10 / 13 — rssn backend (connected)

* `aevia-rssn` links `rssn 0.2.13` directly. One session per thread; handles
  carry a session id and are rejected when stale.
* The manifest is indexed once per process: 1369 operators (7 declared, 13
  overlay, 1349 derived), 75 units, 30 constants, capabilities.
* **Resolution is pinned.** The checker records the `OpKey` it proved for each
  `Rssn.<op>` call site; the evaluator looks it up by span and calls
  `call_operator(&key, …)`. An unknown key is `E1005` — the runtime never
  re-resolves and never falls back.
* Resolution errors are no longer swallowed: a tie is `E1002`.
* `convert_dim` maps `Any`/`Var` to a fresh dimension variable instead of
  `Dim::one()` ("any dimension" used to mean "dimensionless").
* `Val::Unit`/`Val::Quantity` cross as `Value::Unit`/`Value::Quantity`, not as
  opaque strings, so `Rssn.quantity(3.0, Units.m)` runs.
* A kernel failure is a typed error carrying the operator and the `OpError`
  variant. `Term::diff`/`substitute` are fallible; a failed differentiation can
  no longer become `0`.
* `aevia rssn --emit` writes `Rssn.aev`, `Units.aev`, `Constants.aev`; a test
  feeds them back through the parser and checker, and asserts the operator count
  equals `manifest.ops.len()`.
* Capability gaps raise `E1001("core.<capability>")` from one table derived from
  `Features`.

### Editor support (standalone)

Two crates, split so that the semantics are not bound to any editor:

* **`aevia-analysis`** answers questions about a document — diagnostics, hover,
  completion, definition, symbols, formatting — in its own types. It has no
  protocol vocabulary and no protocol dependency; `tests/boundary.rs` fails if
  `lsp-types`, `serde`, `tokio` or the word `textDocument/` ever appears in it.
  A playground, a CLI, a Zed extension or a Neovim plugin consumes this crate
  directly.
* **`aevia-lsp`** is the translation table: JSON-RPC over stdio with
  `Content-Length` framing, the `initialize`/`shutdown`/`exit` lifecycle,
  incremental sync, and one handler per request. It speaks **standard LSP
  3.17** — no `vscode-*` requests, no reliance on `workspace/configuration`, no
  client-specific special cases — so Zed, Neovim, Helix, Emacs and VS Code all
  connect without one.

Positions are converted exactly: LSP counts UTF-16 code units, and Aevia source
is full of `∫`, `°C`, `Ω` and `²`, so the mapping is tested against astral-plane
characters rather than assumed.

Hover answers for **any expression**, not only for a name. `CheckedModule` now
carries `expr_types`, a span → type table filled in `synth` and `check`; the
types are stored unresolved and resolved once after generalisation and
defaulting, so an entry can never name a variable that was later bound to
something else. That cost about 5 % of checking time on a synthetic corpus and
about 26 KB for a 5.9 KB program. Hover takes the innermost expression span
containing the cursor, falls back to the declared scheme for a name, and returns
nothing rather than a guess.

## Deliberate deviations

1. **A runaway *tail* loop is bounded by a budget, not by depth.** A tail call
   does not grow the stack, so `max_depth` cannot catch `fn down(n) = down(n +
   1)`. `Interp::max_tail_calls` (100,000,000, public and configurable) does, so
   the §9.3.4 guarantee — a legal program terminates, a runaway one is `E0805`
   with a call stack — holds on both paths. The budget is a *runaway guard*,
   not a semantic limit: a program that exceeds it is rejected even though it
   would terminate, which is why the default sits far above anything a real
   program does rather than just above the suite's longest loop (a 1.5 M
   default rejected a legal 1.6 M iteration count). `--budget` (W7) will make
   the bound explicit.
2. **Dictionaries are dispatched by runtime shape.** At a polymorphic call
   site the checker records a type variable, so the evaluator selects the
   instance by the argument; a record-alias head is resolved through the alias
   table. Selection is by shape, not by a passed dictionary value.
3. **Representation polymorphism is monomorphic** at the value level: the
   checker records bounds, but a value's representation is its `NumKind`.
4. **Symbolic results are dimensionless in the kernel.** Dimensions are a host
   concern (§10.3.6); a result's dimension comes from the static type and the
   argument.
5. **Tuples are records with numeric labels** internally.
6. **`Float` display** uses the shortest round-trip representation; `Exact`
   renders as a rational.
7. **Decimal literals default to `Real`** (exact decimal text), not `Float` as
   §3.10.1's table says. Reason: after type erasure the runtime cannot tell
   which representation a literal was *expected* to have, and converting the
   text to `f64` and back loses it (`0.1` becomes `3602879701896397/2^55`).
   Defaulting to the exact form preserves information; a `Float` context still
   instantiates `Float`. This should be recorded in the spec's changelog.

## Known gaps

| Area | Gap |
|---|---|
| Runtime | Tail calls are trampolined, but a *non-tail* recursion is still bounded by `max_depth` (2048) and relies on the 256 MiB thread for legal deep programs |
| Runtime | The IR does not yet make boxing or drop-point decisions, and `@jit` has no lowering hook |
| Runtime | Tensors have a dense `f64` path and **strided views**: a slice shares the buffer and carries an offset, one stride per dimension, and a direction per axis, so `reverse` of a vector is a view rather than a copy. **A slice step parses, types and lowers**: `xs[a:b:s]` multiplies the stride by the step and flips the axis for a negative one, so `xs[::-1]` is `reverse(xs)` as a view over the same buffer, and a zero step is `E0802` (static when the literal is known). `faer` backs `@`, `det`, `inv` and `linsolve` (12× on a 256×256 `f64` matmul) |
| Types | `Dyn` operations have a runtime (§4.9) but no checker signature (§3.11) |
| Types | Type-level `Nat` variables and linear equations (§3.5.1) |
| Types | `Dyn` in a `check<d>` position needs the checker to materialise `d` |
| Parser | `reshape<[3, 2]>`: an expression's `<…>` list parses types, not shape arguments (`type_arg`) |
| CLI | `clean`, `--out` for `rssn --emit`, and a `--locked` mode are absent |
| Kernel | `Solutions` crosses as the record §8.6 names, but the solver reports no multiplicities: `solve` returns distinct roots and `solve_structured` fills `Vec::new()`, so `multiplicities` is always empty for `solve`. A *symbolic* root list crosses as one opaque `list(...)` term (`Value::Tensor` cannot hold terms), which also makes `complete` `false` for e.g. `solve(fn t => sin(t) - 1/2)` although the solver did answer with a list |
| Kernel | Still open in `rssn`: `term_of_value` turns a `Value::Unit` into a bare symbol (the dimension is lost in the term graph); `CallCtx::substitutions`/`trace` are no-ops; `manifest.units` ignores the session registry; `DimExpr::from_term` is not `to_term`'s inverse. The rank-2 tensor and overload-lattice defects are fixed |

## Test coverage

Counts are per crate, from `cargo test --workspace`.

| Crate | Tests | Covers |
|---|---|---|
| `aevia-syntax` | 114 | lexer, layout, declarations, attributes, records, recovery, types |
| `aevia-types` | 106 | unification, rows, generalisation, classes, the soundness regressions |
| `aevia-dim` | 20 | dimension algebra, units, exact rationals |
| `aevia-ir` | 9 | ANF naming, capture order and threading, spans, tail positions, pinned call sites, `do` thunk scoping |
| `aevia-core` | 239 | semantics, exactness, IO, stack safety, tail calls, IR parity, classes, `Dyn`, shapes, modules, packages, tensor views and slice steps, the `faer` dispatch, the kernel bridge, standard-library coverage |
| `aevia-rssn` | 50 | manifest conformance, codegen, pinned keys, generated modules |
| `aevia-std` | 125 | linear algebra (with the `faer` backend), statistics, special functions, distributions, FFT, interpolation, ODE, PDE, tables |
| `aevia-analysis` | 58 | position mapping, diagnostics, hover, completion, definition, symbols, formatting, the boundary test |
| `aevia-lsp` | 21 | framing, lifecycle, incremental sync, every request, an end-to-end protocol script |
| `aevia-cli` | 20 | commands, options, the REPL session, package commands |
| `aevia-fmt` | 25 | idempotence and AST preservation over every example and test file |
| `aevia-doc` | 3 | `##` extraction, index, undocumented declarations |
| **total** | **790** | |

`tests/*.aev` are run with `aevia test`.

## Review history

**Audit round** (`.workbuddy-ai/audit/`) — four independent line-by-line audits
(syntax, types, runtime, bridge) against the normative chapters and against the
`rssn` tree. Findings closed in this round:

* syntax: declaration termination, `let … in`, dedent theft, escape decoding,
  duplicate fields, error-recovery placeholders, four missing type productions,
  inert attributes;
* types: defaulting order, weak variables, representation bounds, row lacks,
  per-unit ADT ids, recursive types, `let rec`, forward `let`, `Rssn.<op>`
  resolution, refinement through indexing, twenty diagnostics;
* runtime: exactness and promotion, `show`/`repr`, structural equality, IO
  laziness and `?`, `by_cases`/`assuming`/`forall`/`decide`/`gamma`/`zeros`,
  `Solutions`, `sum(f,a,b)`, exact `det`/`inv`, pinned keys, fallible `Term`,
  call-counted depth, iterative statement loop, incremental REPL;
* bridge: pinned resolution, dimension conversion, unit/quantity values, error
  channel, codegen duplicates and exact `hbar`, one capability table.
