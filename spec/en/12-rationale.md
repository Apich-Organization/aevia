# 12 — Design trade-offs, limits and roadmap

This chapter explains why Aevia looks the way it does, how it differs from existing approaches, what is deliberately left out, and how far the implementation has come. It is informative.

## 12.1 Why dimensions are types

Most errors in scientific computing are not "the algorithm is wrong" but "the unit or dimension is wrong": `kW` taken for `W` (off by 1000), `°C` taken for `K` (off by 273.15), `Hz` taken for `rad/s` (off by `2π`), a mass flow taken for a volume flow. Run-time unit libraries (Pint, Unitful) catch these in tests, but the error may only surface in production. A type system catches them at compile time and can also **guide inference**: the dimension of `mean(xs)` is that of `xs`, `var(xs)` is its square, and `diff(f)` is `f` divided by the independent variable. A library function cannot express these rules; they must be in the types.

The price is a more complex type system. Aevia keeps it in check by:

- limiting dimension algebra to a free abelian group with rational exponents, which is uniformly decidable;
- not supporting static symbolic exponents, which would be undecidable;
- using the explicit escape `Dyn` when something cannot be expressed statically, instead of inventing unsound types;
- keeping units out of types (§12.2), avoiding a combinatorial explosion of type-level conversions.

## 12.2 Why units are not in types

If `m` and `ft` were different types, function signatures would have to quantify over units, type equality would carry units, and conversion would happen at the type level: type errors would read "metres vs feet" rather than "length vs time"; changes to the unit table would break type compatibility; generic functions would have to handle unboundedly many unit combinations.

Aevia's choice: **a type records only the dimension, and a unit is a compile-time scale**. `3 km` and `3000 m` have the same type and the same value; use `to` when display is needed. This gives up the type-level distinction between different units of one dimension, in exchange for a simple, composable type system; in practice, errors are almost always dimension errors, not "metres vs feet".

## 12.3 Why pure functions

Pure functions can be safely parallelised, cached, differentiated, substituted and simplified; symbolic computation requires expressions to be replaceable, and mutable state breaks substitution; dimension rules are simpler too (no "when was this variable changed"). IO is confined to `IO`, so library functions never read files or print behind your back. The costs: numeric algorithms cannot use in-place loops and use tail recursion with an accumulator, with performance coming from tail-call elimination and copy-on-write; some algorithms (in-place LU, in-place FFT) are optimised at run time when the buffer is uniquely held, while the semantics stay pure; users learn `fold`/`map`/recursion instead of `for`. `|>`, `map`, `fold`, pattern matching and tail calls let pure code read like a data flow.

## 12.4 Why strict evaluation

Performance is predictable, with no thunk build-up or space leaks; errors surface where they happen; it matches rssn's strict term construction. Symbolic computation does not need laziness: a `symbol` is a value, an expression is a term, and simplification is requested explicitly with `eval`; laziness would make "when does rewriting run" unpredictable.

## 12.5 Why `Real` and `Float` are separate

- `Real`: exact rationals or symbolic expressions, for derivation, simplification and exact results.
- `Float`: IEEE double, for numeric computation, big data and the JIT.
- A decimal literal defaults to `Float` (someone who writes `0.1` usually wants a float; someone who writes `1/10` wants exactness); in mixed arithmetic `Real` absorbs `Float` (exactness first), and hot paths use `Float` explicitly.

A single numeric type would be bad either way: with only `Float`, `solve(x^2 - 2)` yields 1.414…, losing exactness, and symbolic derivation would need another type system; with only `Real`, numeric loops are slow, BLAS and the JIT cannot be used. Exact versus approximate is the dividing line of scientific computing and deserves to be explicit.

## 12.6 Why HM and not dependent types

HM plus dimension algebra is enough for the vast majority of scientific formulas; inference is the main source of usability (users write no signatures); dependent types (shape-indexed, static exponents) would complicate the types of `reshape`, CSV, slicing and broadcasting and make error messages harder to read; for what cannot be expressed statically, `Dyn` is more practical than a dependent type that cannot be written. Aevia is not a proof assistant: most programs need no annotations, and annotations, when given, are checked.

## 12.7 Comparison with existing approaches

| Approach | Dimensions | Units | Symbolic | Pure | Inference | Assessment |
|---|---|---|---|---|---|---|
| F# units of measure | types | types | no | no | yes | pioneer of typed dimensions; units in types, generics are complex |
| Julia Unitful | run time | run time | partial | no | no | good ecosystem; errors at run time |
| Python Pint/Astropy | run time | run time | no | no | no | easy to use; weak performance and typing |
| Frink | run time | run time | no | no | no | a unit calculator, not a general language |
| Mathematica | run time | run time | yes | partial | no | strong symbolics; dimensions are not types |
| Haskell units | types | types | no | yes | yes | expressive; steep learning curve |
| Squants | types | types | no | yes | yes | a Scala library; units in types |
| **Aevia** | **types** | **compile-time scale** | **yes** | **yes** | **yes** | dimension types + symbolic/numeric phases + pure functions |

What sets Aevia apart: dimensions are types and units are not; symbolic and numeric work live in one language, and `diff`/`solve` take functions rather than strings; pure functions with `IO` isolation make symbolic substitution safe; `Dyn` handles dimensions that cannot be expressed statically; the backend reuses rssn's e-graph, numeric kernels, JIT and simulation.

## 12.8 Design boundaries (not version limits)

These are **deliberate design boundaries**, not "later" to-dos. Each says why it was drawn there and how it behaves when the backend lacks support (a capability diagnostic, never removal of a language feature).

| # | Boundary | Effect and workaround |
|---|---|---|
| 12.8.1 | Shape refinement covers only linear `Nat` (literals, `+`, `*`, linear equations), with no general recursion or proof | Complex shape proofs need an explicit `refine_shape`/`reshape`, or a fall back to run-time checks with `Tensor<n,τ>` |
| 12.8.2 | Static symbolic exponents are unsupported: `x^n` (`n : Int`, `x` dimensioned), dimensioned `det`/`product` | A real boundary of dimension algebra; use `Dyn`, or divide by a unit, compute dimensionlessly and restore the dimension |
| 12.8.3 | `Point[d]` is represented as `Float` + `UnitId`; symbolic absolute temperatures do not enter `Point` | Symbolic thermodynamics cannot do absolute-temperature algebra directly; use differences plus a reference point, or `Dyn` |
| 12.8.4 | Uncertainty is not a core type: `Measurement<d>` does independent error propagation only, correlated errors use `Covariance<d,e>` | Uncertainty in experimental data must be managed explicitly; it is a library type |
| 12.8.5 | No user-defined effects or macros: `IO` is the only effect; the extension points are rssn operators, plugins, `Rssn` dynamic calls and the registry | Use `@op`, plugins and `Rssn.dynamic` for DSLs and code generation |
| 12.8.6 | `Table<ρ>` is a static schema; dynamic CSV returns `Table<Dynamic>` and `to_table<ρ>` validates at run time | Give an explicit `ρ` when reading, or use `Table<Dynamic>` plus validation |
| 12.8.7 | Symbolic values are bound to an rssn session and cannot cross threads: the `par_map` workers each hold a replica, and values that cannot cross threads fall back to sequential evaluation (§9.7.2) | Keep the symbolic phase serial and the numeric phase parallel |
| 12.8.8 | Unit-name ambiguity: `ms` may be milliseconds or metre-seconds; the longest match wins | Write an explicit `*` for rare combinations; the formatter hints |
| 12.8.9 | `Real` absorbs `Float`: `Real + Float` becomes `Real` and the `Float` is converted exactly to a rational | Mixing symbolic and float leaves the fast path and produces large numerators and denominators; keep hot paths `Float` throughout; the compiler hints `W0302` |
| 12.8.10 | Devices/GPU/distributed are run-time targets: `Device` is a run-time value, not a tensor type parameter | The type system does not distinguish CPU and GPU tensors, and device errors surface at run time or at the capability check; use `on(device, ...)` and `@jit(target=...)`, avoiding a type combinatorial explosion |
| 12.8.11 | No formal semantics or proofs: the specification is prose plus rules, with no Coq/Lean formalisation | Rely on conformance tests, property tests and cross-checks against rssn |
| 12.8.12 | `RateOf<s>` covers only numeric state (recursively over scalars, records, tuples, tensors) | For states with `Int`/`Bool`/`String`/function fields, put the constant parameters in the `rhs` closure |

### 12.8.13 Dispatch and extension boundary

Aevia has neither multiple dispatch nor per-type or per-dimension monomorphisation. The former is a semantic decision, the latter an engineering one.

- **No run-time multimethods.** Method calls are resolved at check time: a concrete head type records an exact instance key (class + canonical head); only a quantified head type may choose, by a run-time witness, among candidate instances **proved pairwise disjoint**, and instances that no witness can distinguish are rejected at check time (§3.8.4, §9.2.3). "Which instance" therefore remains a static question; the run time only executes the selection the checker has proved, with no backtracking and no guessing by value.
- **No monomorphisation.** Monomorphisation duplicates generic code per type, which inflates compile time and code size, breaks separate compilation, and conflicts directly with type erasure (§9.2.3, §4.4.4) and dimension polymorphism — `mean<L>` and `mean<T>` MUST be the same code; nor is there a correctness gain, since type errors are already excluded at check time. Per-operation, tag-guarded **representation specialisation** keeps the generic path, duplicates no function and changes no semantics; it enters the performance plan only after a measured baseline (§9.13).
- **The extension points are explicit**: ADTs, record rows, modules/imports, `derive` (a restricted set of structures, §3.8.5), `@op`/`@rule`, plugins and the registry, and the `Dyn` escape. Module interfaces export types, constructors, classes, instances and alias definitions (§7.1.3/§7.3). There are no implicit global instances, no default methods in classes, and no macros, quotation or splicing.

The cost: users cannot add type-class instances to existing types (the orphan rule); "parameterised dimension families" such as distributions and units are expressed with nominal types plus built-in overload tables (§8.7.1) rather than multi-parameter classes.

## 12.9 Closed design decisions

These are final decisions; implementation may proceed in stages, but the language design no longer forks.

| # | Question | Decision |
|---|---|---|
| 1 | Extend `Point` to tensors? | Yes; `Tensor<shape, Point[d]>` with element-wise point arithmetic |
| 2 | Make `Angle` a base dimension? | No; `rad` is dimensionless and `deg` is a scale |
| 3 | Shape-indexed `Tensor`? | Yes, as a refinement: `Tensor<[3,4],τ>`; linear `Nat` arithmetic |
| 4 | Should `det` return `Dyn` by default? | No; dimensionless `det` + `det_unit` |
| 5 | Mixing `Real`/`Float`? | `Real` absorbs `Float`; hot paths use explicit `Float` |
| 6 | `Dyn` errors? | arithmetic `panic` + `try_*`; `check` returns `Result` |
| 7 | Nested `RateOf`? | scalars, records, tuples, tensors, defined recursively |
| 8 | Static `Table` schema? | Yes, row-polymorphic `Table<ρ>`; `Table<Dynamic>` kept |
| 9 | `@` or `*` for matrix product? | keep `@`; `*` is element-wise |
| 10 | `%` for modulo? | No; `mod`/`rem` |
| 11 | Is `let` recursive by default? | No; `fn` is recursive, `let rec` is explicit |
| 12 | Top-level IO? | No; `main`/REPL |
| 13 | Central registry? | Yes; content-addressed + signatures + capabilities, protocol in §13.7 |
| 14 | Export user dimension aliases? | Yes; exported by default, `private` hides |
| 15 | Type-level guarantee of unit conversion? | No; units are scales, dimensions are types |
| 16 | GPU/distributed? | Devices are run-time values: `Device`/`on`/`@jit(target)` |
| 17 | Sparse? | `Sparse<F,shape,τ>` as a core type, with a `Layout` class |
| 18 | User effects/macros? | No |
| 19 | Uncertainty? | `Measurement`/`Covariance` standard-library types |
| 20 | Operator integration? | Reflection first: Kernel Manifest + dynamic `Value` + generated façade; no hand-written bindings |

## 12.10 Implementation progress

Implementation proceeds in dependency order and is not a language version; every stage keeps the manifest and dynamic-call contract of chapter 13. History by revision is in appendix C.

| Stage | Contents | Status |
|---|---|---|
| M0 | lexing, layout, parsing; name resolution; type and dimension algebra; shape refinement; diagnostics and formatter | implemented; all nine complete programs of chapter 11 check |
| M1 | Core IR, interpreter, closures/records/variants/pattern matching; Kernel Manifest and `Rssn` generation; the rssn bridge; units and `Point` | implemented |
| M2 | tensor storage/broadcasting/slicing; linear-algebra kernels; `Sparse` | implemented (`Bcsr`, Krylov solvers, slicing) |
| M3 | `diff`/`antiderivative`/`defint`/`limit`/`series`/`solve`/`sum`/`product`; budgets and error mapping | implemented |
| M4 | modules/packages/lock files; registry protocol and client; REPL/CLI/formatter; LSP | implemented; gaps in appendix C, Revision 15 |
| M5 | CSV/NPY/JSON; `Table<ρ>`; Plot; `Device`; `simulate` and `Physics.*` | implemented (Plot has only the HTML/SVG renderer; no GPU backend is wired in) |
| M6 | JIT (`jit` feature); plugin operators; benchmarks | partial: the JIT is off by default; see §9.6 |

**Acceptance criteria for a first stable release**: every example of chapter 11 checks and runs; every dimension error is a compile-time error (except at `Dyn` and data boundaries); operators in the manifest need no hand-written bindings; conformance tests against rssn pass; `rssn::dim`/`UnitRegistry` round-trip consistently; plugins and the registry work; shape refinement, `Point` tensors, `Table<ρ>`, `Sparse` and `Device` each have an end-to-end example; at least one real ODE/PDE/statistics project runs end to end.

## 12.11 Success criteria

1. A physics, engineering or data-science user can write, in a day, a program with units, symbolic derivation and numeric solving;
2. dimension errors are caught at compile time, and the messages guide the fix;
3. numeric hot spots perform on the same order as Python/NumPy;
4. symbolic computation covers common calculus, equations and linear algebra;
5. the code is readable, like mathematics rather than like configuration.

The next chapter covers extensibility and the rssn integration; after it come the complete grammar, the diagnostics catalogue and the revision log.
