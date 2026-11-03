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

let g = 9.80665 m/s^2

fn fall_time(h: Float[L]): Float[T] =
    sqrt(2 * h / g)

fn main(): IO Unit = do
    let h = 20.0 m
    println("fall time from {h} = {fall_time(h)}")
```

```console
$ cargo run -p aevia-cli -- run examples/kinematics.aev
fall time from 20 m = 2.01962 s
```

## Building and running

```console
cargo build --release
cargo test --workspace
cargo run -p aevia-cli -- check examples/units.aev
cargo run -p aevia-cli -- run examples/symbolic.aev
cargo run -p aevia-cli -- run examples/calculus.aev
cargo run -p aevia-cli -- repl
cargo run -p aevia-cli -- rssn --emit generated/
```

The only requirements are a Rust toolchain (edition 2024) and the `rssn` kernel
checked out next to this repository. `aevia-rssn` links it directly, so the
operator set, the unit table and the physical constants all come from rssn's
Kernel Manifest — there is no second list to keep in step.

## What is where

| Crate | Responsibility | Spec |
|---|---|---|
| `crates/aevia-dim` | dimension algebra (exact rational exponents, unification with occurs check), unit registry with SI prefixes and affine scales | ch. 04, 05 |
| `crates/aevia-syntax` | UTF-8 lexer with the layout algorithm, parser for the complete grammar, AST, diagnostics renderer | ch. 01, 02, app. A/B |
| `crates/aevia-types` | `Scalar<ρ, δ>`, tensors, records with rows, nominal variants, HM inference with dimension unification, defaulting, exhaustiveness | ch. 03, 04 |
| `crates/aevia-rssn` | the kernel bridge: session, value conversion, manifest registry, `Rssn` module generation | ch. 10, 13 |
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
  `Units.Constants` are all generated from the Kernel Manifest, and symbolic
  values *are* rssn terms: an integral the local layer has no node for is
  carried by handle, not approximated.
* **Dimensions stop at the kernel boundary.** The term graph is dimensionless
  (§10.3.6); a result's dimension is restored from the static type and the
  argument, which is why `antiderivative(fn t: Real[L] => t)(2.0 m)` is `2 m^2`.

## Status

See [STATUS.md](STATUS.md) for the chapter-by-chapter state, the deliberate
deviations (type-class dictionaries, Core IR, `Rssn.invoke`) and the roadmap.

## License

Apache-2.0, matching the specification's reference backend.
