# aevia

Aevia is a pure functional language for scientific computing in which
**dimensions are part of the type system**. The compiler rejects `3 m + 2 s`,
`sin(3 kg)` and `exp(1 m)` before any code runs; units are compile-time scales;
symbolic and numeric evaluation are two phases of the same expression.

This repository is the implementation of the [design standard](spec/README.md):
a lexer and parser for the full grammar, a dimension-aware type checker, an
interpreter with tensors and IO, and a manifest-driven bridge to the
[`rssn`](../rssn) kernel.

```aevia
module Kinematics

## `h` is a length by its type, so `sqrt(2 * h / g)` is checked before it runs.
## `h` and `g` are exact quantities, so the answer is exact; `approx` is the
## explicit step from the exact answer to a number (§0.5, §6.3.1).
let g = 9.80665 m/s^2

fn fall_time(h: Real[L]): Real[T] =
    sqrt(2 * h / g)

fn main(): IO Unit = do
    let h = 20.0 m
    println("fall time from {h} = {fall_time(h)}")
    match approx(fall_time(h)) with
    | Ok(t) -> println("numerically       = {t}")
    | Err(e) -> println("approx failed: {e}")
```

```console
$ cargo run -p aevia-cli -- run examples/kinematics.aev
fall time from 20 m = sqrt(800000/196133) s
numerically       = 2.0196199771025523 s
```

The first line is the exact answer: `sqrt` of an exact value that has no exact
root stays symbolic (§6.3.1) and `Real` is exact rational arithmetic. The
second line runs the numeric phase; it converts to `Float` and is therefore
subject to floating-point rounding in the last digits.

## Building and running

```console
cargo build --release
cargo test --workspace
cargo run -p aevia-cli -- check examples/units.aev
cargo run -p aevia-cli -- run examples/symbolic.aev
cargo run -p aevia-cli -- run examples/calculus.aev
cargo run -p aevia-cli -- repl
cargo run -p aevia-cli -- rssn --audit
cargo run -p aevia-cli -- rssn --emit generated/ --namespaces
```

The test profile keeps source line tables for backtraces while omitting full
variable and type debug data. This bounds disk and linker-memory costs when
many integration tests link the RSSN kernel. For debugger inspection of a
particular test, use `CARGO_PROFILE_TEST_DEBUG=2 cargo test -p <crate> --test <name>`.
Use a shared `CARGO_TARGET_DIR` and run Cargo jobs in sequence when working on
both repositories; separate feature combinations can rebuild the kernel.

The requirements are Rust 1.97 or newer (edition 2024) and the `rssn` kernel
(`3.0.0-rc.1`, `api_version = 2`) checked out next to this repository.
`aevia-rssn` links it directly, so the operator set, the unit table and the
physical constants all come from rssn's Kernel Manifest — there is no second
list to keep in step. `cargo run -p aevia-cli -- rssn` prints the live version,
API version, manifest hash, operator/signature-source counts and unit/constant
counts. `rssn --audit` classifies each overload as an exact typed binding,
a dynamic/approximate signature, or unavailable and explains the limits.
`--emit DIR --namespaces` additionally generates modules grouped by kernel
rule set: exact signatures use pinned keys; other entries use the dynamic
value-list ABI and return its `Result` error channel. The manifest only covers
registered operators, and callback-based Rust kernels still require a host
callback bridge.

The kernel-facing contract (what the manifest carries, which API is stable, and
which gaps remain) is [RSSN-INTEGRATION-REQUIREMENTS.md](RSSN-INTEGRATION-REQUIREMENTS.md).

## What is where

| Crate | Responsibility | Spec |
|---|---|---|
| `crates/aevia-dim` | dimension algebra (exact rational exponents, unification with occurs check), unit registry with SI prefixes and affine scales | ch. 04, 05 |
| `crates/aevia-syntax` | UTF-8 lexer with the layout algorithm, parser for the complete grammar, AST, diagnostics renderer | ch. 01, 02, app. A/B |
| `crates/aevia-types` | `Scalar<ρ, δ>`, tensors, records with rows, nominal variants, HM inference with dimension unification, defaulting, exhaustiveness | ch. 03, 04 |
| `crates/aevia-rssn` | the kernel bridge: session, value conversion, manifest registry, `Rssn` module generation | ch. 10, 13 |
| `crates/aevia-std` | the standard library: linear algebra, statistics, special functions, distributions, FFT, interpolation, ODE/SDE/PDE, tables, simulation | ch. 08 |
| `crates/aevia-ir` | the Core IR: ANF, closure conversion, per-node spans and call sites, lowering from the typed AST | §9.2.2 |
| `crates/aevia-core` | values, tensors, closures, IO actions, the symbolic layer on top of rssn, the AST and IR interpreters, built-ins, the module loader | ch. 06, 07, 08 |
| `crates/aevia-cli` | `run`, `check`, `test`, `repl`, `rssn` | ch. 09 |

`examples/` holds runnable programs; `tests/` holds conformance tests in Aevia
itself.

## Design decisions worth knowing

These are the places where the implementation had to choose, and the reasoning:

* **Units never enter a type.** `3 km` and `3000 m` have the same type
  `Real[L]` and the same value; only the display unit differs. This is the
  central rule of §0.4 and it is what keeps type equality decidable.
* **Dimensions are exact.** Exponents are `BigRational`, so `L^(2/4)` is `L^(1/2)`
  and unit scales such as `0.3048 m` convert exactly.
* **Literals are representation-polymorphic, not dimension-polymorphic.** `1`
  may become `Int`, `Real`, `Float` or `Complex`, but only `1 m` is a length
  (§3.10.1). The checker enforces this and suggests `1 m` in the diagnostic.
* **`Real` and `Float` are different types.** `Float` is always an `f64`;
  mixing it with `Real` promotes to `Real` (exact), never the other way.
* **The runtime keeps witnesses.** Types are erased, but a scalar carries its
  dimension and display unit, a tensor its shape, and a variant its constructor
  tag, so `show`, `to`, `value_in` and pattern matching work without types.
* **The kernel is the only operator registry.** `Rssn.sin`, the unit table and
  `Units.Constants` are all generated from the Kernel Manifest (1382 operators,
  82 units and 30 constants at the time of writing), and symbolic values *are*
  rssn terms: an integral the local layer has no node for is carried by handle,
  not approximated.
* **Source operators are kernel operators.** `@op("ns/name")` publishes a
  function as a kernel definition with a dimension signature taken from its
  checked type; on symbolic arguments the function then builds the operator
  node, so `@rule`s, `diff` and `simplify` see the operator rather than its
  expansion (§13.5.2).
* **Checked modules are cached on disk.** `aevia check`/`run` keep a `.aevi`
  file per module under `.aevia/cache/modules`; process-local ADT ids are
  replaced by symbolic keys and remapped on load (§9.11.3). `--no-cache`
  bypasses it and `aevia clean` removes it.
* **Dimensions stop at the kernel boundary.** The term graph is dimensionless
  (§10.3.6); a result's dimension is restored from the static type and the
  argument, which is why `antiderivative(fn t: Real[L] => t)(2.0 m)` is `2 m^2`.

## Status

See [STATUS.md](STATUS.md) for the chapter-by-chapter state, the deliberate
deviations (type-class dictionaries, Core IR, `Rssn.invoke`) and the roadmap;
the design standard's revision history is in
[appendix C](spec/appendix-c-changelog.md). All nine complete programs in
chapter 11 of the standard now type-check (and the runnable ones run); the
standard library's module tree is embedded in `crates/aevia-std/stdlib/` and the
loader falls back to it when no package root resolves a module, so
`import Math { sqrt }`, `open Units.SI` and `import Physics.Ising` work from user
source (a local file still wins); chapter 8's names also remain reachable
through the Prelude, which is why the quick start needs no imports. SDE support
landed end to end: the kernel exposes the noise primitives
`sde_wiener`/`sde_levy_areas` and the manifest-only metadata query
`sde_scheme_info` as operators (capability `core.sde`); `sde_scheme_info` gives
the strong order plus Itô/commutative/adaptive/Lévy-area flags for the kernel's
8 schemes but has no weak-order column — `sde_order` in Aevia supplies
`{strong, weak}`. `Sde`/the Prelude provide the typed solver surface
(`sdeint_diag`, `sdeint_general`, `wiener`, `sde_order`, `sde_ensemble_seed`);
the solver loops stay host-side because they need Aevia closures.

The alpha round (spec appendix C, Revision 15) added:

* **Unit systems** (§5.11): `module Hep with units natural` (also `planck`,
  `atomic`, `geometrized`) reduces the dimensions written in the module — a
  length is an inverse energy — while every value stays an honest SI quantity,
  so values cross module boundaries without reinterpretation. `to_system` /
  `from_system<D>` convert explicitly; `Units.Natural` and friends are generated
  from the kernel manifest, with `c = hbar = 1` produced by the reduction itself.
* **Literal folding without the old deviation** (§3.10.4): a literal is rounded
  once, closed literal trees are compile-time constants evaluated in their
  static representation, and `1.0 + 2` is the spec's `Float`.
* **Parallel evaluation and tasks** (§9.7): `par_map`, `par_tabulate` and
  `par_reduce` run on shared-nothing interpreter replicas (`--jobs N`) and
  never change a value; `spawn`/`await` run IO concurrently with deterministic
  output. `AEVIA_PARALLEL_TRACE=1` explains every sequential fallback.
* **Source plugins** (§13.5.2): `@op`/`@rule` functions are traced into kernel
  definitions and rewrites (so the kernel can differentiate and evaluate them);
  `load_plugin(path)` loads a plugin file at run time.
* **Incremental checking** (§9.11): `aevia watch file.aev` re-checks only the
  modules whose source or imported interfaces changed.
* **Fresnel integrals** (§8.2.2): `fresnel_s`/`fresnel_c`, numerically and
  through the kernel's calculus (derivatives, antiderivatives of quadratic
  phases, definite integrals, Meijer-G).

## License

Apache-2.0, matching the specification's reference backend.
