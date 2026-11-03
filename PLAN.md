# Aevia — implementation plan

This file is the engineering plan for turning the current tree into a conforming
implementation of the design standard in `spec/`. It is written after a
line-by-line audit of all six crates against chapters 01–13 and against the
`rssn` tree in `../rssn` (the post-migration architecture, `rssn 0.2.13`).
The audit reports are in `.workbuddy-ai/audit/`.

`STATUS.md` describes what the code does. This file describes what it must do
and why. Where the two disagree, this file wins.

---

## 0. Diagnosis

The tree is not a skeleton — 25k lines of real code, and the dimension algebra,
the layout algorithm, the HM core and the kernel calls are genuine. But four
advertised pillars are facades, and several semantics are wrong in ways that
tests cannot see because the tests assert on rendered strings.

| Area | State |
|---|---|
| Dimension algebra, unit table | real |
| Lexer, layout | real |
| HM inference, rows, variants | real but unsound in five places |
| Type classes | **absent** (`Scheme` has no constraint field) |
| Representation polymorphism | **absent** (constraints are unification, errors discarded) |
| `Dyn` | type exists, **no constructor or operation** |
| Weak variables (`symbol`) | **absent** |
| Recursive types | **impossible** (ctors converted before interning) |
| Core IR | **absent** (interpreter walks the typed AST) |
| Standard library | **absent** (~200 of ~300 chapter-8 names unbound) |
| Packages (`aevia.toml`) | **absent** |
| `Rssn.<op>` static/runtime agreement | **broken** (checker types overload 0, runtime resolves again) |

Two defects are worse than the admitted gaps because they silently delete
features the spec and the tests claim:

1. **Dimension polymorphism is erased.** `default_type` runs *before*
   `generalize` at every `let` (`check.rs:388`, `694`), so every `<d>`-polymorphic
   function is monomorphic and every one of them emits a spurious `W0301`.
2. **`Rssn.invoke` is `∀a.a`** (`check.rs:1548`), and as a value it is literally
   `Type::Var(0)` (`check.rs:2864`), aliasing the module's first variable.

Everything else follows from the reports. The point of this plan is that these
are *not* features to add later — they are the semantics of programs users will
write in week one, and every one of them is a breaking change once code exists.

---

## 1. Design decisions that cannot be revisited

Each decision below is forced by the standard, or is a choice this plan makes
now because the alternative becomes a breaking change later. None of them is
"we will do it properly in a future version".

### D1 — Static resolution equals runtime resolution

`Rssn.<op>` in **call position** is resolved by the checker using the lattice of
§13.4.1 (arity filter → per-argument classification → weakest score → unique
maximum), and the resolved `OpKey` is **pinned into the Core IR**. The runtime
executes that key and never re-resolves. A tie is `E1002`; no match is `E1003`.

`Rssn.<op>` **not** in call position is an error (`E0303`) unless every overload
of that name has the same type — an overloaded name is not a first-class value,
and pretending otherwise is how the current unsoundness arose.

*Why now:* the alternative (type it as overload 0) is what the tree does today,
and any program written against it breaks when it is fixed.

### D2 — Core IR is materialised

`crates/aevia-ir` implements the ANF core of §9.2.2 and lowering from the typed
AST. The interpreter runs the IR, not the AST. Consequences that are the reason
for doing it: call sites carry spans (stack traces, §9.3.4), tail calls are
trampolined (§9.3.2), closures are converted to flat frames instead of
`Rc<Scope>` chains, `@jit` has a lowering hook (§9.6), and the depth guard
counts calls instead of AST nodes.

*Why now:* the IR is internal, so it is not a source-compatibility risk — but
every semantic guarantee that depends on it (traces, tail calls, no stack
overflow on legal programs) is observable, and observable behaviour is what
freezes.

### D3 — Type classes are dictionary-passing

`Scheme` gains a constraint list. `class`/`instance`/`derive` build dictionaries;
resolution is head-matching with no backtracking (§3.8.2); coherence `E0311` and
the orphan rule `E0604` are enforced. `Eq`/`Ord`/`TotalOrd`/`Show`/`Hash`/`Num`/
`Fractional`/`Floating`/`Container`/`Sized` are the built-in classes, and `Real`
has no `Eq` instance (comparison yields `Prop`, §3.8.6).

*Why now:* instance sets are a global, coherent namespace. Adding dictionaries
after libraries define instances changes what "the instance for `Show Int`" means.

### D4 — Representation constraints are constraints

`join(ρ, Int)` records `ρ ⊒ Int` rather than binding, `div_rep` applies to `^`
with negative literal exponents, and the seven `let _ = constrain_rep(...)` sites
report. Schemes quantify over rep variables with their bounds.

*Why now:* today `fn f(a: Int): Int = a + 2.0` types as `Int`; fixing it later
changes the type of every numeric expression.

### D5 — Rows have lacks-constraints

Row unification carries `lacks` side conditions, record update keeps the tail,
duplicate literal fields are `E0208`.

*Why now:* a lacks-constraint changes the meaning of every row-polymorphic
signature; a library published against today's permissive rows cannot be kept.

### D6 — Nominal identity is per module

`AdtId` is allocated per compilation unit; the name-keyed global registry goes
away. Recursive types are legal and positivity is checked (`E0312`).

*Why now:* two modules that both declare `type Shape` currently share one id.
That is a name-resolution bug that leaks into every serialised interface.

### D7 — `symbol` allocates a weak variable

Weak variables do not generalise; they default at module end (`W0301` if
unresolved). A symbol therefore keeps one dimension for the whole program.

*Why now:* the entire symbolic story rests on it, and the current default-to-`1`
behaviour makes `symbol` unusable with units.

### D8 — Exactness is a language guarantee, and `show` is truthful

`Float ⊗ Exact → Real` (§6.3.2); `sqrt`/`exp`/`sin` of a non-representable exact
argument stay symbolic (§6.3.1); `Exact` renders as a rational, `Float` as the
shortest round-trip, `repr` is a distinct function (§6.12.3); structural equality
compares payloads, not renderings.

*Why now:* every test in the tree is blind to exactness because `show` hides it.
Fixing `show` after users write golden files is breaking.

### D9 — Tensors are unboxed and strided

`Arc<TensorData>` with shape, offset and strides; an `f64` fast path; `faer` for
`matmul`/`det`/`inv`/`eig`/`svd`; Kahan summation; exact path for small `Exact`
matrices; slicing yields views (§6.6.4, §9.5).

*Why now:* the current representation is 3–4 orders of magnitude off §9.13 and
cannot express a view at all. Both are internal, but the *observable* part —
`det` of an exact matrix returning a float — is not.

### D10 — The standard library is code, not a table

`crates/aevia-std` holds `Prelude`, `Math`, `Units.SI`, `Calculus`, `Linalg`,
`Stats`, `Data`, `Plot`, `Sim` as Aevia sources plus a small intrinsic layer.
Chapter 8 is the contract; a name in chapter 8 that does not resolve is a bug.

*Why now:* `pi` currently evaluates to a constructor that prints `"pi"`. A
language whose constant `pi` is not a number has no users to break.

---

## 2. Work breakdown

Ordering is **dependency order**, not language versioning. Nothing in this list
is deferred because a release is coming; the ordering exists only because
`aevia-core` cannot compile against a checker that does not yet expose weak
variables.

### W1 — Syntax (`crates/aevia-syntax`)

1. `expect_newline` at every declaration end; reject `let x = 1 let y = 2`.
2. Layout: `let … in` as a group body; validate `INDENT`/`DEDENT` everywhere;
   never steal an enclosing dedent; `E0102` does not push the bad level.
3. Missing type productions: `Matrix`, `Sparse`, `row_type` with `+`,
   `T[d]` postfix, kind-carrying type arguments (`Vec<L>`).
4. Decode escapes in `"""` strings (today `\t` becomes `t`).
5. Attributes: `@op`/`@jit`/`@rule` reach the checker or are `E0210`.
6. Duplicate record fields in a literal or a row type are `E0208`.
7. Recovery placeholders must not reach the checker (`UnitLit`/`Wildcard`).
8. `E0601`: module name must match its path.

### W2 — Types (`crates/aevia-types`)

1. Defaulting moves after generalisation, to module end (D4, §3.10).
2. Weak variables for `symbol`/`symfun` (D7).
3. Rep constraints and `div_rep` (D4).
4. Row lacks-constraints, update tail, duplicate labels (D5).
5. Per-module `AdtId`, recursive types, positivity (D6).
6. `let rec` semantics; top-level forward `let` unification.
7. Type classes and dictionaries (D3).
8. `Rssn.<op>` call-site resolution and pinned key (D1).
9. Index type and static bounds; refined shapes survive indexing.
10. `Dyn` constructor and operations (§3.11, §4.9).
11. Diagnostics: emit the 20 documented codes that never fire; `E0406` for
    `3 m + 0`; `E0405` for `Point / scalar`; `E0316` for duplicate `fn`.

### W3 — IR (`crates/aevia-ir`, new)

Lowering from typed AST to ANF core; closure conversion; per-node spans; call
sites; trampoline. (D2)

### W4 — Runtime (`crates/aevia-core`)

1. Exactness and promotion (D8); `sqrt` of exact stays symbolic.
2. Structural equality for variants, records, tensors (D8).
3. `show`/`repr` split; shortest round-trip.
4. IO: actions built lazily; `assert` deferred; `random` deferred; real
   `time_now`; `?` returns from the enclosing function, not the closure.
5. `by_cases`/`assuming` build `ite`; `forall` samples and decides; `decide`
   without f64 rounding; `gamma` is Γ, not log Γ; `zeros<Int>`.
6. `sum(f, a, b)`, exact `det`/`inv`, `Solutions` with `variable`/`complete`.
7. Tensors on `Arc<TensorData>` with strides and an `f64` path (D9).
8. Iterative `do`/`run_io`; guarded `show`/`match`/`Term`; call-counted depth.
9. REPL: one interpreter per session, incremental declarations.

### W5 — Standard library (`crates/aevia-std`, new)

Chapter 8 in full, including FFT, interpolation, ODE, distributions, complex,
number theory, and the table/JSON/CSV layer. (D10)

### W6 — Bridge (`crates/aevia-rssn`, `aevia-core/src/rssn_module.rs`)

1. Call-site overload resolution with a pinned key; delete the re-resolving
   fallback and the swallowed `Err(_)`.
2. Variadic ABI: spread a list argument into positional values.
3. `convert_dim`: `Any`/`Var` become fresh dimension variables, not `1`.
4. `Unit`/`Quantity` map to `Value::Unit`/`Value::Quantity`, never `Opaque`.
5. `Foreign` failures raise `E0899`/`E0807`; `diff` never returns `0` on error.
6. Codegen: fix the `m_u` duplicate, emit `hbar` exactly, type-check the
   generated modules in a test, assert `manifest.ops.len()`.
7. One capability table derived from `Features`; `E1001` names the capability.

### W7 — Packages and tooling

`aevia.toml`/`aevia.lock`, package roots, `E0601`/`E0604`; `fmt`, `doc`, `bench`,
`init`, `add`; `--max-stack`, `--budget`, `--out`, `--color`; `aevia-lsp` over
the same checker.

---

## 3. Rules of engagement

1. **No language feature is removed to make a deadline.** A missing backend
   capability is `E1001` with the capability name. A missing *implementation* is
   a bug, not a diagnostic.
2. **Tests assert semantics, not renderings.** No new test may assert on `show`
   output unless the test is about `show`. Tests inspect value kind, tensor
   storage, or an error code.
3. **Every diagnostic code the spec documents is reachable from a test.**
4. **A fix lands with the test that would have caught the bug.**
5. **Concurrency:** builds use `CARGO_BUILD_JOBS=4`, tests use
   `--test-threads=2`, `RAYON_NUM_THREADS=2`. The machine has 16 cores and
   `rssn` is itself parallel; four agents at full width oversubscribes it.

---

## 4. Definition of done

* `cargo test --workspace` is green, and the suite contains a negative test for
  every soundness hole in `.workbuddy-ai/audit/*.md` §2.
* Every program in `spec/11-examples.md` type-checks and runs.
* Every chapter-8 name resolves.
* `aevia rssn --emit` output type-checks in a test, and its operator count equals
  `manifest.ops.len()`.
* `STATUS.md` contains no claim that a test does not exercise.

---

## 5. Progress

Verified by `cargo test --workspace` (769 tests) and by running every
`examples/*.aev`.

| Work stream | State |
|---|---|
| W1 syntax | **done** — 111 tests |
| W2 types (items 1–10) | **done** — 98 tests. Classes are checked *and* dispatched, including a record-alias head; a `fn` is generalised when its body checks, so `fn f(x) = show(x)` is `Show a => a -> String`. `Dyn` has a runtime but no checker signatures |
| W3 Core IR | **done** — `crates/aevia-ir` (ANF, closure conversion, spans, call sites) + `aevia-core/src/eval_ir.rs` (trampoline); 9 lowering tests, a 104-program differential harness (both paths run twice) and a 1,000,000-iteration tail-call test at 2 MiB of stack. The AST path stays selectable as a second implementation; the reference is the specification |
| W4 runtime (items 1–9) | **done** — item 7 landed as a dense `f64` storage class (245× on elementwise `+`, 130× on `sum` at 100k elements, exact tensors still boxed) **and** strided views: a slice shares the buffer, carrying an offset and one stride per dimension |
| W5 standard library | **98.1 %** of chapter 8 — 307 of 313 names; 2 capability gaps, 4 blocked |
| W6 bridge | **done**; the kernel-side P0 defects (rank-≥2 tensors, overload rank/dimension lattice, pinned-overload validation, signature hash) are fixed in `rssn` |
| W7 packages and tooling | **done** — `aevia.toml`/`aevia.lock` with content hashes and package roots, `init`/`add`/`update`, `fmt` (idempotent, AST-preserving), `doc`, `bench`, the `--max-stack`/`--budget`/`--color` options, and the editor split: `aevia-analysis` (semantic core, protocol-free) + `aevia-lsp` (standard LSP 3.17 over stdio) |

Remaining work, in dependency order:

1. **A slice step.** `reverse` reaches the reversed view, but `xs[::-1]` does
   not parse: `Parser::slice` has no step component. The runtime is there; the
   syntax is a language change and has not been made.
2. **The rest of the kernel list.** P1/P2 of
   `.workbuddy-ai/audit/kernel-requirements.md`: a unit that crosses into the
   term graph loses its dimension, `CallCtx` traces and substitutions are
   no-ops, `manifest.units` ignores the session registry, `DimExpr::from_term`
   is not `to_term`'s inverse.
