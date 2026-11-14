# Aevia architecture

This document describes how the implementation is organised. The *language* is
defined by the design standard in [`spec/`](spec/README.md); this file says which
crate implements which part of it, how a program travels from text to a value,
and where the boundary with the [`rssn`](../rssn) kernel lies. Where a rule is
argued in the standard, the section is cited as `§N.M`.

## Overview

```
 source text (.aev)
      │
      ▼
 aevia-syntax ── lexer + layout algorithm ── parser ── AST ── diagnostics
      │                  (units kept as syntax: aevia-dim::UnitExpr)
      ▼
 aevia-types ─── HM inference with dimension / representation / Nat / row
      │          unification, classes, exhaustiveness, unit resolution
      │          (resolves `Rssn.*` through the Kernel Manifest)
      ▼
 CheckedModule ──► .aevi disk cache (aevia-core::disk_cache, §9.11)
      │
      ▼
 aevia-ir ────── Core IR: ANF, closure-converted, a span on every node
      │
      ▼
 aevia-core ──── interpreter (Core IR loop; the typed-AST walker is kept as
      │          the semantic reference), values, tensors, IO, symbolic layer,
      │          built-ins, module loader, parallel replicas, optional JIT
      │
      ├──► aevia-std ───── numeric and data kernels behind chapter 8
      └──► aevia-rssn ──── the only crate that names an rssn type
                 │
                 ▼
               rssn (Kernel Manifest, Session, rules, kernels, units)

 tools on the same front end:
   aevia-fmt (formatter) · aevia-doc (docs, doctests) · aevia-analysis ──► aevia-lsp
   aevia-cli (the `aevia` binary: run, check, watch, test, repl, fmt, doc, bench, …)
```

The central design decision is that **Aevia owns no operator list, unit table or
constant table**. The checker, the runtime and the generated `Rssn` module all
read the rssn Kernel Manifest, and symbolic values *are* rssn terms. Everything
else follows from keeping that boundary narrow.

## Crates

| Crate | Responsibility | Depends on |
|---|---|---|
| `aevia-dim` | Dimension algebra (exact rational exponents, unification with an occurs check, aliases), the unit table with SI prefixes and affine scales, unit systems (§5.11). No dependency on parser or runtime. | — |
| `aevia-syntax` | UTF-8 lexer with the layout algorithm, parser for the complete grammar, AST, spans, diagnostics renderer, AST fingerprints. | `aevia-dim` |
| `aevia-rssn` | The kernel bridge: the live `Session`, conversion between the two value models, the Kernel Manifest indexed (`registry`), `Rssn` module generation (`codegen`), pinned-key bindings, JIT passthrough. | `rssn`, `aevia-dim`, `aevia-syntax` |
| `aevia-types` | `Scalar<ρ, δ>`, tensors, records with rows, nominal variants, type classes, Hindley–Milner inference with dimension unification, literal defaulting, exhaustiveness, resolution of `Rssn.*` signatures, `.aevi` persistence. | `aevia-dim`, `aevia-syntax`, `aevia-rssn` |
| `aevia-ir` | The Core IR of §9.2.2 (ANF, closure conversion, tail positions, spans, call sites) and lowering from the typed AST. Data only; it must not depend on `aevia-core`. | `aevia-dim`, `aevia-syntax`, `aevia-types` |
| `aevia-stdlib-model` | The source of truth for standard-library generation, independent of generated files. | — |
| `aevia-std` | The standard library: embedded §8.20 module tree (`stdlib/*.aev`), and the numerics behind chapter 8 (linear algebra on `faer`, statistics, distributions, FFT, interpolation, ODE/SDE/PDE, sparse, tables, geometry, special functions). Knows nothing about runtime values. | `aevia-stdlib-model`, `rssn` |
| `aevia-core` | Values, tensors, closures, IO actions; the interpreters; built-ins; the symbolic layer on top of rssn (`sym`); the module loader (`driver`, `manifest`); the `Rssn` module (`rssn_module`); source plugins (`@op`/`@rule`, `plugin_source`); the disk cache; parallel evaluation (`parallel`); the optional JIT (feature `jit`). | `aevia-dim`, `aevia-syntax`, `aevia-types`, `aevia-ir`, `aevia-rssn`, `aevia-std` |
| `aevia-fmt` | A formatter over the AST whose output is re-parsed and compared by fingerprint (semantics-preserving) and re-formatted (idempotent); on failure the file is left untouched. | `aevia-syntax`, `aevia-dim` |
| `aevia-doc` | Markdown API pages from `##` doc comments; extraction of `aevia` code blocks for `aevia test --doc`. Also used by the registry platform. | `aevia-syntax`, `aevia-fmt` |
| `aevia-analysis` | Editor-independent semantic queries (diagnostics, hover, completion, definitions, symbols, code actions, semantic tokens) over a document or a whole import graph. No JSON and no LSP vocabulary — a test fails the build if one appears. | `aevia-syntax`, `aevia-types`, `aevia-core`, `aevia-fmt`, `aevia-rssn` |
| `aevia-lsp` | The language-server adapter: JSON-RPC framing, document store, protocol translation. The only crate that knows what an editor client is. | `aevia-analysis` |
| `aevia-cli` | The `aevia` binary: `run`, `check`, `watch`, `test`, `repl`, `fmt`, `doc`, `bench`, `init`, `add`, `update`, `clean`, `lsp`, `rssn`, `pkg`. | most of the above |

`aevia-std` has dev-dependencies on `aevia-core`, `aevia-types` and `aevia-rssn`
(coverage tests only); `aevia-rssn` has dev-dependencies on `aevia-syntax` and
`aevia-types`. Neither edge exists in the normal build graph.

Tools (`tools/`): `aevia-stdlib-gen` regenerates and checks the embedded
standard-library sources and their embedding table; `rssn-api-audit` compares
the live kernel manifest with the public Rust API inventory
([README](tools/rssn-api-audit/README.md)); `aevia-prof` is a sampling profiler
for Windows that needs no administrator rights; `bundle_spec.py` regenerates
the single-file specification.

## From text to a value

1. **Loading** (`aevia-core::driver`). A module is a file; its name is its path
   under the package's `src/`. The loader walks imports post-order, so a module
   is always checked after the modules it imports, reports cycles, and resolves
   path dependencies through `aevia.toml` / `aevia.lock` (`aevia-core::manifest`;
   only path dependencies exist in the toolchain). The standard library is
   embedded in `aevia-std` and used when no package root provides a module.
2. **Lexing and parsing.** `aevia-syntax` produces the AST and keeps a unit
   expression such as `m/s^2` as syntax, so a file may use `3 km` before the unit
   is declared (§1.8.1).
3. **Checking.** `aevia-types` infers types, representations (`Int`, `Real`,
   `Float`, `Complex`), dimensions and tensor ranks together; a dimension
   mismatch is a type error. Units never enter a type: `3 km` and `3000 m` have
   the same type (§0.4). Checked modules are cached per module as `.aevi` files
   under `.aevia/cache/modules` (keys cover the source, the imported interfaces,
   the capability set and the compiler build); process-local ADT ids are replaced
   by symbolic keys on save and re-interned on load (§9.11.3).
4. **Lowering.** `aevia-ir` turns the typed AST into the Core IR. Types are
   erased, but the checker's evidence (resolved class instances, pinned `Rssn`
   overloads, dimension witnesses) stays keyed by span.
5. **Evaluation.** `aevia-core` runs the IR: blocks are straight-line statements
   over a slot vector, closures capture an ordered environment, a tail call is a
   jump. The typed-AST walker (`eval.rs`) is retained as the semantic reference.
   The runtime trusts the checker's proof instead of re-proving it at each call
   (the trust boundary of §9.2.3), but keeps *witnesses* — a scalar carries its dimension
   and display unit, a tensor its shape, a variant its constructor tag — so
   `show`, `to`, `value_in` and pattern matching work without types.
6. **Numbers and symbols.** `Real` is exact rational arithmetic; `Float` is an
   `f64`. A `Real` function of exact arguments stays symbolic when it has no
   exact value (`sqrt(800000/196133)`), and `approx` is the explicit step to a
   number (§6.3.1). Symbolic values are rssn terms handled by `aevia-core::sym`
   through `aevia-rssn`.

## The rssn boundary

`aevia-rssn` is the only crate that names an rssn type. It links `rssn` by
path (`../rssn`, `default-features = false`), so the repositories are checked
out side by side.

* **Manifest as the single registry.** `Rssn.sin`, `Units.Constants` and the
  unit table are generated from the manifest (`aevia rssn --emit DIR --namespaces`).
  Domain modules (`import Rssn_NumberTheory`, `Rssn_FiniteField`, `Rssn_Group`, ...) are rendered at
  import time from the manifest plus `crates/aevia-rssn/domains.overlay` (typed signatures, codecs,
  exclusions). An operator without a curated signature is typed from the kernel's *witnessed*
  overloads when it has one that Aevia can state (signatures the kernel mined from its own runs and can
  replay; rssn `docs/KERNEL.md` §3a), and is dynamic otherwise; a test fails when an operator is neither bound nor excluded and keeps
  `tools/rssn-api-audit/RSSN-COVERAGE.md` current (spec §10.3.8). Witnessed wrappers take parameter
  names derived from the Aevia type (`n`, `x`, `xs`, `m`, `t3`, `flag`, `v`; a repeated kind is numbered),
  since the kernel records none.
* **Numeric operators.** `Rssn_Numeric` carries the operators of rssn's `numeric` rule set
  (robust statistics, windows and spectra, wavelets, Krylov/sparse/low-rank solvers, tensors, sampled
  functions, complex special functions, finite-difference solvers, expression operators such as `ode_solve`):
  67 curated `sig` lines, 8 dynamic ones with a reason. Their answers are floating point, so signatures say
  `Float`, not `Real`, and a symbol that names a method or wavelet is a `Real`. A `sig` is also what gives a
  result its dimensionless evidence; a dynamic `Value` from a rank-polymorphic or dimension-free operator
  cannot be decoded into a `Float` tensor. The kernel's dimension-free names (`heat_fd`/`poisson_fd` take the
  space dimension from the nesting depth of the data; `solve_heat_equation` and the unsuffixed affine
  transforms `translation`, `scaling`, `shear`, `reflection`, `rotation`) are the rule: the `_1d`/`_2d`/`_3d`
  spellings are aliases, and `Rssn_Graphics` claims the unsuffixed names. A signature has one rank, so the
  finite-difference wrappers are one-dimensional.
* **Devices.** `aevia_rssn::device` wraps rssn's device registry. The wgpu device provides `F64` as
  double-single arithmetic (`DeviceInfo::emulated_f64`; pairs of `f32`, relative error about `1e-14`,
  `f32` exponent range) and `device::plan` fuses chains of launches (a fused plan returns the same bits as
  the unfused one). The language-level `Device` primitives and `Gpu(n)` are not wired up yet (spec §10.6.5).
  Exact signatures use pinned `OpKey`s; other entries use the dynamic value-list
  ABI and return its `Result` error channel. `aevia rssn --audit` classifies each
  overload as an exact typed binding, a dynamic/approximate signature or
  unavailable.
* **Dimensions stop at the kernel boundary.** The term graph is dimensionless
  (§10.3.6); a result's dimension is restored from the static type and the
  arguments, which is why `antiderivative(fn t: Real[L] => t)(2.0 m)` is `2 m^2`.
* **Source operators.** `@op("ns/name")` publishes a function as a kernel
  definition with a dimension signature taken from its checked type; `@rule`
  publishes a rewrite. The function is *traced* with fresh symbols, so the kernel
  can differentiate, simplify and integrate through it (§13.5.2). A function whose
  result depends on its arguments through control flow or IO cannot be traced and
  is rejected with `E1006`.
* **Callbacks.** Higher-order kernel operators receive interpreter-owned
  callbacks through the kernel's checked callback bank, in a thread-affine scope.
  The stochastic solvers stay host-side because they need Aevia closures.
* **JIT.** With feature `jit`, a `@jit` function is lowered to rssn's LIR
  (`rssn::backend::lir`, a typed register machine with real branches, loops
  and calls) and compiled with Cranelift. `Float`, `Bool` and 64-bit `Int`
  activations compile: `if`/`match` branch for real, every self tail call is a
  jump with a fuel check (exhausted fuel hands the trampoline back to the
  interpreter), a self call in value position is a native recursive call,
  `Int` `+ - *` are checked, and the elementary functions (`sqrt`, `exp`,
  `ln`, the trigonometric and hyperbolic families, ...) are guarded so that
  every case in which the interpreter would not just return `f(x)` (a domain
  error, an extended-real special value, an overflow) *bails* and the
  interpreter repeats the pure call. A builtin is used only while the name
  still resolves to it. Effects, captures, exact numbers, units and tensors
  stay interpreted; `map_floats` keeps the straight-line tape lowering that
  devices need.
* **Simulations.** `import Rssn_Sim_<Name>` renders a typed module for an rssn
  simulation scenario from the kernel's scenario catalog
  (`rssn::sim::schema`, `aevia-rssn::sim`): `Params` and `Output` records,
  a variant type per method-name field (`Rssn_Sim_PoissonMultigrid.W`),
  `defaults()` from the scenario's own defaults, and `run(p)`. A new scenario,
  parameter or method in rssn appears in Aevia without a line written here.

The kernel-facing contract is documented in the rssn repository:
[`docs/KERNEL.md`](../rssn/docs/KERNEL.md) (manifest, dynamic ABI, plugins,
units), [`docs/DIMENSIONS.md`](../rssn/docs/DIMENSIONS.md) and
[`docs/CALLBACKS.md`](../rssn/docs/CALLBACKS.md). The language-side rules are
chapters 10 and 13 of the standard.

## Concurrency

Evaluation is single-threaded by default. `par_map`, `par_tabulate` and
`par_reduce` (§9.7.2) run on shared-nothing interpreter replicas
(`aevia-core::parallel`; `--jobs N`) and never change a value; `spawn`/`await`
run IO concurrently with deterministic output. An rssn `Session` is not `Sync`
and interpreter closures are not `Send`, so the interpreter and its callbacks are
thread-affine and a closure-driven ensemble is not parallelised implicitly. `AEVIA_PARALLEL_TRACE=1`
explains every sequential fallback.

## Tooling

* **CLI.** `aevia --help` is the reference; chapter 9.9 of the standard
  documents it. `check` and `run` keep the disk caches; `watch` re-checks only
  the modules whose source or imported interfaces changed; `--caps` restricts
  `fs,net,env,rand,gpu`; `--no-cache` bypasses the caches.
* **Editor.** `aevia lsp` hosts `aevia-lsp` over stdio. All semantic answers come
  from `aevia-analysis`, which loads a file and its import graph through the same
  driver the compiler uses.
* **Docs and doctests.** `aevia doc` writes Markdown from `##` comments;
  `aevia test --doc` compiles every `aevia` block (a `compile_fail` first line
  must fail to compile).
* **Registry client.** `aevia pkg …` forwards to `aevia-pkg` from the
  [platform](platform/README.md).

## Platform

`platform/` is a separate Cargo workspace: the package registry, the
documentation site (including the rendered specification) and the signed-publish
client. It reuses `aevia-doc`, `aevia-syntax` and `aevia-fmt` by path and never
links the rssn kernel. Its own architecture is in
[`platform/docs/ARCHITECTURE.md`](platform/docs/ARCHITECTURE.md).

## Testing

* Unit and doc tests in every crate (`cargo test --workspace`); the standard
  library's coverage test derives its expected names from chapter 8
  (`aevia-std::spec`, `crates/aevia-core/tests/stdlib_names.txt`).
* Conformance programs in Aevia itself: `tests/*.aev` (run with `aevia test`)
  and runnable programs in `examples/`.
* `aevia-stdlib-gen --check` guards the generated standard library;
  `rssn-api-audit --check` guards the kernel API inventory.
* Test profile: line tables only (no full debug info), to bound disk and linker
  memory when many integration tests link the rssn kernel. Use a shared
  `CARGO_TARGET_DIR` and run Cargo jobs in sequence when working on both
  repositories.

## CI/CD

The rssn and aevia repositories share one pipeline design (`.github/workflows`):

* **`ci.yml`** — rustfmt (nightly), clippy `-D warnings`, tests on Linux and Windows, the
  platform workspace against a real PostgreSQL, contract checks (`aevia-stdlib-gen --check`,
  `rssn-api-audit --check`, the single-file specification regenerated and diffed), rustdoc and a
  build of the registry container image. One `CI success` job summarises them for branch protection.
* **`security.yml`** — CodeQL, dependency review on pull requests, RustSec advisories, weekly.
* **`release.yml`** — on a tag `vX.Y.Z` equal to the workspace version: `aevia` and `aevia-pkg`
  binaries for three platforms, a GitHub release carrying the specification, and the registry image
  pushed to GHCR. Deploying the image is manual ([platform/docs/DEPLOY.md](platform/docs/DEPLOY.md)).

The workflows check out rssn next to this repository; the kernel revision is the repository variable
`RSSN_REF` (default `main`).

## Ownership of numerical extensions

RSSN owns numerical algorithms, including automatic differentiation, quadrature,
optimization, decompositions, interpolation, transforms, statistics and geometry.
Time evolution and discretized fields belong to RSSN's `sim` modules. New Aevia
standard-library APIs adapt values, shapes, callbacks and errors; they must call
RSSN instead of implementing another solver. Exact Aevia rational semantics and
language value conversion remain the responsibility of the language layer.

There are two coverage contracts. The live Kernel Manifest is exposed by the
flat `Rssn` facade and generated `Rssn_<Domain>` modules. Public Rust source APIs
are inventoried separately because helpers, generics and opaque state are not
necessarily callable ABI operators. Source-derived composable codecs register
reviewed raw-number APIs from `rssn/tools/rssn-api-gen/codec-policy.tsv`; record
schemas come from `structured-policy.tsv`. Rust checks the calls and codec types,
and the manifest supplies Aevia bindings automatically. Dimension policies are
explicit semantic choices, not guesses inferred from a function name.

Run `sh tools/sync-kernel.sh` after reviewing kernel API changes. This regenerates
RSSN bindings, structured codecs, Aevia's source inventory, manifest coverage and
standard-library files. CI checks these artifacts. Test the changed numerical
families and the generated-language integration target first; the full scientific
suite is a separate, expensive validation step.
