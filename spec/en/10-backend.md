# 10 — The rssn backend mapping

Aevia does not reimplement computer algebra. rssn provides the term representation, the rewriting engine, the dimension layer, numeric kernels, the JIT and simulation; Aevia provides syntax, type and dimension checking, closures, records, tensor shapes, modules, IO and the orchestration of the standard library. This chapter defines the contract between the two: what maps directly, what needs a wrapper, and what the known gaps are.

## 10.0 Reflection first: no hand-written operator bindings

Aevia **does not maintain** binding tables such as `sin -> rssn::sin` or `det -> rssn::det`. At build time it reads rssn's **Kernel Manifest** and generates the `Rssn` module (contract in [13 — Extensibility](13-extensibility.md) §13.2):

```aevia
Rssn.sin(0.5)                       # numeric overload of a Derived operator: Real
Rssn.sin(x)                         # symbolic overload of a Derived operator: Dyn<term>
Rssn.det(M)                         # Overlay：Tensor<[n,n],Real> -> Real
Rssn.dynamic("myplugin_softplus", args)  # plugin/leaf: no signature
```

- `signature_source = Declared | Overlay | Derived` → typed declarations. `Derived` is a conservative signature inferred from registration data: `eval` → `(Real,…) -> Real` (a predicate gives `-> Bool`), and a non-leaf also gets `(Term,…) -> Term`; it does not distinguish `Tensor`/`Quantity`/`Int`, and IDEs mark it "conservatively inferred".
- `None` (leaf / non-callable) → a `dynamic` declaration. rssn validates at the **signature level** (`check_args`; an argument accepted by no overload is `OpError::Type`); the full type and dimension check belongs to the static checker or `Rssn.typed`.
- A new operator appears at the next build as soon as it is registered in rssn, with no change to the Aevia compiler.
- The generated source spells signatures exactly: dimension parameters are written as Aevia dimension variables (`Real[T]`, `Real[T^(1/2)]`), with variable names local to a declaration and **shared between parameters and return** — in `sde_wiener(a3: Real[T]): Tensor<2, Real[T^(1/2)]>`, `T` and `T^(1/2)` are the same variable. `Dyn<term>` remains only where a signature genuinely cannot be spelled: signatures the manifest itself declares as `Dyn(rep, any)`, rank-variable tensors (`transpose`/`rank`/`dot`/`norm`/`sum`; `Tensor<n, τ>` is `E0304`), type parameters, unknown-row `Table`, `Device`, variants, host opaque values and `Never`.
- The generated `Units.aev`/`Constants.aev`/`Rssn.aev` type-check with no diagnostics: for each declaration the generator emits only an `Rssn.invoke` with literal `op` + `key`; the checker validates those two literals against the pinned overload and types the result as that overload's declared result (with fresh dimension variables at each call). A non-literal key stays a dynamic `Value` and the run time answers `E1002`/`E1003`/`E1005`. The test `emit_typecheck` in `aevia-rssn` asserts this acceptance.
- Static overloads use `OpKey { rule_set, name, signature_hash }`; typed calls pin the overload through `Rssn.call_key`; dynamic calls go through `call_value`, choosing by the `Match` lattice when several registrations exist and reporting `E1002` on a tie (§13.4.1).

The kernel-side contract is in rssn's `docs/KERNEL.md`.

## 10.1 What rssn offers

```rust
let s = Session::new();
let x = s.sym("x");
let answer = s.compute(diff(sin(x) * cos(x), x), &Config::new())?;
```

- `Session` owns a hash-consed term graph; `Term<'s>` is a handle with a lifetime.
- `Config` specifies rule sets, the target phase (`Symbolic`/`Numeric`), bindings, assumptions, budgets, the dimension mode, dimension assumptions and custom base dimensions.
- `compute` copies the request into a clone of the template graph, runs e-graph rewriting, extracts the answer and imports it back into the session. `Answer` holds `term`, `value`, `error`, `reduced`, `dimension`, `diagnostics` and `report`.
- `Term::eval(bindings)` evaluates numerically without rewriting; `Term::compile(inputs)` compiles a closed form (the `jit` feature uses Cranelift); `Term::as_tensor()` turns nested `list` terms into `(shape, Vec<f64>)`; `to_latex()`/`to_typst()`/`to_pretty()` typeset.
- `sim::scenario::run(name, json)` runs a simulation; `sim::scenario::SCENARIOS` is the only source of scenario names (29 at present).
- `Session::register_unit(UnitDef)` registers a unit; `Session::manifest()` uses `KernelManifest::build_with_units`, so session-private units appear in that session's manifest.
- SDE: `rules::sde` exposes the callback-free `sde_wiener`/`sde_levy_areas` and the scheme metadata `sde_scheme_info(name) -> Tensor<1, Real>` as manifest operators (`features.sde`, capability `core.sde`). The solve loop needs host callbacks (`&Fn + Sync`) and is not in the manifest; Aevia's `sdeint_diag`/`sdeint_general` call `kernels::sde` on the host side.

What Aevia reuses directly:

| Capability | rssn module |
|---|---|
| Exact numbers (big integers / rationals / floats) | `graph::Number` |
| Term construction and hash-consing | `graph::Graph`, `Session::call_value`/`Session::parse` |
| Rewriting engine and rule sets | `graph::Engine`, `rules::*` |
| Calculus / algebra / equations / ODE / PDE | `rules::calculus`, `solve`, `ode`, `pde`, … |
| SDE noise primitives | `rules::sde` (manifest operators), `kernels::sde` (integrators) |
| Dimension inference, unit parsing | `graph::dim`, `rules::units` |
| Numeric kernels (linear algebra, FFT, ODE, optimisation, special functions) | `kernels::*` (`faer`, rustfft), `sim::integrators` |
| Simulation | `sim::scenario` |
| JIT | `backend::jit`, `Term::compile` |
| Typesetting | `io::latex`, `io::typst`, `io::pretty` |

What Aevia implements itself: lexing, parsing, layout and formatting; **compile-time** type and dimension checking; closures, records, variants, strings, sets and `IO`; the host representation of tensors, shape checking, broadcasting and slicing; modules, packages, the REPL and the LSP; orchestration and typed wrappers of the standard library; the run-time dimension check of `Dyn`.

## 10.2 Scalar mapping

### 10.2.1 Exact numbers

| Aevia | rssn |
|---|---|
| `Int` (arbitrary precision) | `Number::Int(BigInt)` |
| `Real` exact rational | `Number::Rat(BigRational)` |
| `Float` | `f64` / `Number::Float` |
| `Complex`, numeric | `num_complex::Complex64` (host) or an `a + b*i` term |
| `Complex`, symbolic | an `a + b*i` term |

- `Session::int`/`rational` accept only `i64`; for larger values use `Session::parse(&n.to_string())`.
- The `Exact(Number)` of `Real` maps directly and `Sym(TermId)` maps to an rssn term; the `F64` of `Float` does not enter rssn.
- The numeric path of `Complex` uses `Complex64`; the symbolic path builds `add(re, mul(im, i))`. The two paths are unified on the Aevia side because rssn's `Number` has no complex numbers.

### 10.2.2 Symbols

```rust
let name = format!("{base}#{id}");     // internal unique name
let term = session.sym(&name);
```

- Display and internal names are separate: the internal name of `symbol("x")` is `x#1` and it displays as `x`; the same symbol reuses the same `TermId` across `compute` calls.
- The static path's term graph is dimensionless and **no** dimension is passed for symbols; dimensions are the type checker's job. Full names are passed to `Config::dimension` only for FFI/export.
- Symbol assumptions (`positive`, `real`, `integer`) go in through `Config::assume`; `assuming` turns unary predicates (`x > 0`, `x >= 0`, `x != 0`, `x ∈ ℤ`) into `Facts`. **General equations and inequalities have no language entry point** (`Rssn` has no `assume` member and there is no `core.assume.general` capability row).

## 10.3 Operator mapping: a generated façade, not hand-written bindings

The tables below are **examples of the semantic mapping**, explaining how Aevia's syntactic sugar lands on rssn's structural operators and requests; the actual bindings are generated from the manifest.

### 10.3.1 Core arithmetic

| Aevia | rssn operator |
|---|---|
| `a + b` | `add(a, b)` |
| `a - b` | `add(a, mul(-1, b))` |
| `a * b` | `mul(a, b)` |
| `a / b` | `mul(a, pow(b, -1))` |
| `a ^ n` | `pow(a, n)` |
| `-a` | `mul(-1, a)` |
| `abs`/`sqrt`/`sin`/`exp`/`ln`/`atan2` | the same-named operator |

`Session::call_value(name, args)` resolves the overload, validates the arguments, builds the term and performs only the operator's own local rewriting (`add` is one n-ary flattening), with no global rewrite loop; whole expressions use `Session::parse`. `Kernel::call(&OpKey, …)` forwards to `call_key` and **pins** the overload `OpKey` points to; `Kernel::call_ho` likewise finds the registration by key.

### 10.3.2 Heavy requests

| Aevia | rssn request |
|---|---|
| `diff(f)` | `diff(term, ξ)` |
| `antiderivative(f)` | `integral(term, ξ)` |
| `defint(f,a,b)` / `limit(f,a)` / `series(f,a,n)` | `defint` / `limit` / `series` |
| `sum(f,a,b)` / `product(f,a,b)` | `sum(term, κ, a, b)` / `product(term, κ, a, b)` |
| `solve(f)` / `solve_complex(f)` / `nsolve(f,x0)` | `solve` / `solve_complex` / `nsolve` |
| `dsolve(eq,y,t)` | `dsolve(eq, y(t))` |
| `simplify(x)` / `approx(x)` | `compute(term, Config::symbolic())` / `compute(term, Config::numeric(tol))` |

### 10.3.3 Numeric kernels

| Aevia | rssn |
|---|---|
| `defint` (numeric) | `kernels::quadrature` |
| `odeint` | `kernels::ode` / `sim::integrators` |
| `sdeint_diag`/`sdeint_general` | `kernels::sde` (Aevia-side wrapper) |
| `fft` | `kernels::fft` (rustfft) |
| `eig`/`svd`/`qr`/`lu` | `kernels`/`faer` |
| `interp1`/`spline`, `minimize`, special functions | `kernels::*` |

Numeric kernels consume `f64`/`ndarray` directly and bypass the term graph; the `Float` tensor path prefers them.

### 10.3.4 Linear algebra

- `Float`/`Complex` matrices use rssn's numeric kernels (faer); small exact/symbolic `Real` matrices use `rules::linalg`; a large matrix that cannot be numericised returns `Err` or stays symbolic.
- The dimensionless restriction of `det` is in the Aevia layer (rssn's `det` can compute dimensioned matrices, but Aevia does not expose that overload); the dimension `e/d` of `linsolve(A,b)` is checked statically by Aevia, and rssn's answer dimension is used for validation.

### 10.3.5 Unknown and new operators

Any operator not in the generated façade goes through the dynamic call:

```aevia
let r = Rssn.dynamic("foo", [to_value(x), to_value(y)])
match r with
| Ok(v) -> ...
| Err(e) -> ...
```

Arguments and results are `Value`s; rssn checks the shape of the arguments against the signature, and the full type and dimension check stays on the Aevia side. After a new operator is registered, regenerating the `Rssn` module gives typed declarations; plugin and user operators need no compiler change; a failure returns `OpError`, which Aevia turns into `E1003`/`E1001`.

### 10.3.6 Units

**Aevia's term graph carries no units.** Unit literals are folded to coherent SI numbers at compile time; `Real`/`Float` values carry an optional display unit on the host side. rssn sees dimensionless numbers and symbolic expressions (`3 m + x` is `3 + x` in the term graph), and dimensions are guaranteed by static types.

The reason: rssn's `quantity` has no numeric eval, and `dimension_from_text` parses `L` as litre and `T` as tesla; putting units into the static term graph would break both the numeric path and the dimension path (more trade-offs in §12.2). Units appear at only two boundaries:

1. **Display / typesetting**: `show`/`to_latex` splice the number and the display unit on the host side; when rssn typesetting is needed, wrap with `quantity(v, "<coherent SI unit>")`, for output only.
2. **Export / FFI**: when writing CSV/NPY/JSON, unit metadata is attached on the host side.

A `Point` is converted to a `Float` in K before entering rssn; the run-time dimension of `Dyn<ρ>` is `rssn::dim::DimTerm`. `Units.*` is generated from `manifest.units` and no second table is kept; the start-up unit-table consistency check (§5.10) serves only display and export.

### 10.3.7 Simulation

```rust
let json = rssn::sim::scenario::run("ising", params)?;
```

`simulate(name, json)` calls it directly; the only source of scenario names is the kernel's `SCENARIOS`, and a host keeps no second list. The typed wrappers `Physics.*` (§8.17) are Aevia source modules that call `simulate` internally and convert between records and JSON; the `Rssn_Sim_<Name>` modules are generated on import from rssn's scenario catalogue (`rssn::sim::schema`, §8.17). `ising` returns a flat `Vec<i8>`, which the wrapper reshapes row-major by `width`/`height` into `Tensor<2, Int>`; `sim` errors are strings, which the wrapper maps to the `SimError` enum by prefix.

### 10.3.8 Domain modules: `Rssn_<Domain>`

The flat `Rssn.<op>` and the per-rule-set `Rssn_<rule_set>` follow the **kernel's** organisation and cannot say "`invmod` takes a modulus and returns a residue". Domain modules are an Aevia-side grouping, generated from the **manifest plus a small declarative overlay** (`crates/aevia-rssn/domains.overlay`):

- **Membership.** `domain`/`family` assign operators to 29 domains (`NumberTheory`, `Combinatorics`, `Group`, `FiniteField`, `Graph`, `Coding`, `Crypto`, `Topology`, `Numeric`, …) by `rule_set:glob`, first match wins. Every rule set MUST be claimed by some family; when the kernel adds a rule set or operator that the overlay does not place, the coverage check fails.
- **Three exposure tiers plus a witnessed tier.** *typed*: `sig` gives the Aevia type (`Result<T, OpError>`) and the wrapper is derived from the signature; *witnessed*: see below; *dynamic*: an automatically generated `Result<Value, OpError>` wrapper, so a new operator is callable with zero edits; *excluded*: `exclude`, with a mandatory reason. In a `typed`-mode domain an operator with no `sig`, `dynamic` or `exclude` is a coverage error; a `sig` whose parameter count disagrees with the manifest arity, or that names an operator that no longer exists, fails likewise.
- **The witnessed tier (automatic typing).** At the end of each engine run, the request (`factorint(360)`) and the answer (`list(list(2, 3), ...)`) sit in one equivalence class, which is itself a type judgement; the kernel records these judgements as "sort facts", clusters them by argument kind and then **replays** each witness call, producing `signatures.witnessed.json` (rssn `docs/KERNEL.md` §3a). Each overload in the manifest carries `overload_sources`. For a witnessed overload that has no `sig` but has a usable Aevia type, the domain module generates a wrapper of the same shape as the typed tier (`fn divisors(n: Int): Result<Tensor<1, Int>, OpError>`). Type mapping: `Int` → `Int`; rationals, symbolic expressions and symbols → `Real`; floats → `Float`; `Bool` → `Bool`; rectangular lists → `Tensor<rank, element>`; tuples, opaque objects and ragged lists stay in the dynamic tier. When an operator has several witnessed overloads, the one with the most evidence takes the operator name and the others get `_int`/`_real`/`_float` suffixes. A witnessed signature is **evidence, not a universal proof**: the parameter side may be narrower than what the operator actually accepts (`Rssn.dynamic` is always available), and the result is validated by `from_value<T>`, so going outside the observed range gives `Err`, never a wrongly typed value. `dynamic op | reason` can force an operator to stay in the dynamic tier.
- **Conversions are generic**: arguments `to_value(x)`, results `from_value<T>(v)`; rich types declare a pair of conversion functions with `codec Type to_fn from_fn` (one pair per type): `Perm` (0-based, ±1 at the boundary), `Graph`/`Group` (nominal types wrapping an opaque `Value` handle), `PermGroup`.
- **Error channel.** A kernel failure is `Err(OpError)`; a graph-backend operator whose argument leaves its domain returns an unreduced call term (`invmod(6, 15)`), which `from_value` reports as a `TypeMismatch` carrying the text of that term.
- **Diagnostic channel: why it did not reduce.** When a numeric operator rejects its arguments (shape mismatch, axis out of range, singular system, incompatible Neumann data, …) the kernel leaves only the unreduced call term. `Session::explain(term)` (`rssn::kernel::{Diag, DiagKind}`) replays the operator's evaluator on the calling thread and returns `Diag { op, arg, kind, message }`. When the result is exactly the operator's own unreduced call and `kind` is not `Symbolic`, the bridge (`Kernel::call_with_witnesses`, `call_key`, `call_key_with_witnesses`) returns `KernelError::from_diag`, with the stable code `rssn.diag.<kind>` (`arity`, `shape`, `domain`, `singular`, `incompatible`, `not_converged`, `unsupported`, `symbolic`, `unspecified`). `Rssn.dynamic` writes it in user form — function name, parameter name, position — inside `Err`: ``Err(rssn.diag.domain: tensordot_2_2_1: argument `axes_a` (#3): an axis is past the rank of the first tensor (rank 2): [2])``. Parameter names come from the overlay's `sig`; witnessed/dynamic wrappers give only a position (`argument #3`). `OpError` stays an opaque type: `"{e}"` gives `code: message`, and no constructor or `Rssn.*` member was added. At a **throwing** site (a typed call `Rssn.<op>(..)`) the same finding is `E0807` with the stable code in a note. Only operators of `rules::numeric::TABLES` record reasons.
- **Dimension evidence.** `sig` also declares "this operator is a pure function of dimensionless arguments"; rules the kernel declares `Dimensionless`, and `List/NonMetric/Compare/Angle` rules when the arguments are known to be dimensionless, give bare-number results `Scalar(1)` evidence; other rules (`Same`, `Sum`, `Custom`, …) are not trusted. A `list(...)` term result is read back as a tensor when trusted (equal-length rows merge into a higher-rank tensor; unequal ones stay a tensor of tensors).
- **Generated at run time.** A module is rendered from the live manifest when `import Rssn_NumberTheory` is resolved (`codegen::namespace_source`), and no generated source is committed, so it cannot drift from the linked kernel. Only the overlay and the coverage report `tools/rssn-api-audit/RSSN-COVERAGE.md` are committed; `cargo test -p aevia-rssn --test domains` regenerates and compares them (`AEVIA_BLESS=1` rewrites).

**`Rssn_Numeric`.** rssn's `numeric` rule set (robust statistics, windows and spectra, wavelets, Krylov/sparse/low-rank/tensor decompositions, dimension-independent finite-difference solvers, complex special functions, expression operators) all have a `sig`, and none stays in the dynamic tier; user-facing interface and examples are in §8.19.1. The rules:

- Results are floating point, so signatures use `Float`/`Tensor<n, Float>`, **not** `Real` (`from_value<Real>` accepts only exact `Int`/`Rat`); integer-valued results (ranks, permutation indices) are likewise returned as `Float`.
- Method, wavelet and statistic parameters named by symbols have type `Real`; every call of `symbol("db2")` makes a new symbol, so bind each name once with `let` and reuse it.
- **One kernel operator may have several `sig`s, each with its own Aevia name** (the overlay writes `sig heat_fd as heat_fd_2d(..)`; a duplicate name is an error): since a signature has a single literal rank, dimension-independent operators are named by dimension count (`_2d`/`_3d`) and rank-polymorphic tensor operators by rank (`tensor_norm_<r>`, `tensordot_<ra>_<rb>_<k>`, etc., §8.19.1).
- **Decoding mixed lists.** The result of a tensor decomposition is a list of parts of different types. The bridge keeps it as a one-dimensional value with per-element dimension evidence, and `from_value<Tensor<1, Value>>` is the generic "list accessor": it hands out the parts along the first axis one by one as `Value` handles, and each is then decoded with `from_value<T>`. The decoders are Aevia source in the `Numeric` prelude (`hosvd3_from_value`, …), declared as decode-only types with `codec Hosvd3 - hosvd3_from_value` (`-` means no encoder; using such a type as a parameter is a coverage error).

Examples: `tests/domain_number_theory.aev`, `domain_combinatorics.aev`, `domain_finite_fields.aev`, `domain_numeric.aev`.

**Language gap (not implemented)**: `let x: Zmod<17> = 5; x * x ^ -1` needs three language changes, all in `aevia-types`/`aevia-core`: (1) variant types accepting `Nat` parameters (currently `E0317`); (2) `Num`/`Fractional` classes with `add/sub/mul/neg/from_int/inv` methods, and binary operators dispatching to instances for user nominal types; (3) integer literals promoted through `from_int` by the expected type. Until then the modulus of `Zmod` is carried at run time and operations are written as functions (`zmod_mul(x, inv)`).

## 10.4 The dimension bridge

The only source of truth for dimensions and units is `rssn::dim` (`DimTerm`/`DimArena`/`UnitRegistry`). Aevia's `Dim` is its type-level mirror; `DimValue` at run time is a `DimTerm`, not a string.

### 10.4.1 Static to run time

**The static path passes no dimensions to rssn**: the type checker has already guaranteed dimensional correctness, symbols in the term graph are dimensionless, and `DimensionMode` is `Off` (or `Advisory`, for diagnostics only). Dimensions are handed over in only three situations:

- the run-time check of `Dyn<ρ>`: the host keeps a `DimTerm`, not a string;
- FFI/export: prefer `Config::dimension_term(symbol, DimExpr)` (no text parsing; unknown base names are interned). The plain-text entry `Config::dimension(symbol, text)` MUST be given **full names** (`Length`, `Mass`, `Time`, `Current`, `Temperature`, `Amount`, `Luminosity`) and **never single letters** (`L` is litre, `T` is tesla, `N` is newton, `J` is joule);
- the user explicitly requests rssn's dimension diagnostics: `Config::dimensions(Advisory)`.

A custom base dimension is declared with `Config::base_dimension(name)`; `prepare` calls `define_base_dimension` on the **cloned** graph. `Graph::clone` shares an `Arc<RwLock<DimArena>>` and a write detaches when `strong_count > 1` (copy on write), so registration during a request does not leak into the template or concurrent requests, and a request that defines no base dimension pays no copy cost. Never modify the template graph of a `Session` directly.

### 10.4.2 Run time to static

The dimension of a numeric answer is taken from `Answer.dimension`; do **not** re-infer it from `Answer.term`. `Answer.dimension` is an `Option`: it is `None` under `DimensionMode::Off`, in which case the static dimension is used; for a run-time dimension the numeric path MUST use `Advisory` or `Strict`. On the static path the answer's dimension is already known and `Answer.dimension` serves only an optional consistency check; a disagreement is the internal error `E0809`.

### 10.4.3 Dimension modes

| Situation | Mode |
|---|---|
| Ordinary code that passed static checking | `Off` (term graph has no dimensions) |
| `Dyn`, FFI, terms built at run time | `Strict`, with full names |
| User explicitly requests diagnostics | `Advisory` |

Aevia does not expose `bypass` to ordinary users; `Dyn<ρ>` is the explicit escape.

## 10.5 Threads and lifetimes

### 10.5.1 `Session` is not `Sync`

`Session` is internally a `RefCell`, and `Term<'s>` borrows the `Session`. The bridge keeps the kernel session **thread-local** (`kernel()` in `aevia-rssn`):

```rust
thread_local! {
    static KERNEL: RefCell<Option<Rc<Kernel>>> = const { RefCell::new(None) };
}

pub fn kernel() -> Rc<Kernel>;   // this thread's session, created on first use
```

- One session per thread; the worker threads of `par_map` each hold an evaluator replica and a session (§9.7.2).
- Symbolic values are not shared across threads: the kernel's `TermRef` carries a `SessionId` and a handle from another session is rejected (`OpError::ForeignTerm`); values that cannot cross threads fall back to sequential evaluation. The bridge does not use `Session::fork`/`merge` (primitives the kernel provides).
- `compute_all` runs in parallel within one session, but **the bridge does not use it**: batched requests are issued one after another; when parallelism is needed, assign sessions per thread on the host side.

### 10.5.2 Lifetime of terms

A `Term<'s>` is borrowed only during a `compute` call and converted to a `u32` id on return; `Session::term_by_id(id)` retrieves it, and ids are stable within a session; the answer term of `compute` has already been imported back into the session. The term graph only grows; the REPL's `:reset` rebuilds the session.

## 10.6 Performance strategy

### 10.6.1 Do not call `compute` for every operator

`compute` clones the template graph and runs rewriting; for an O(1) construction like `x + 1` it costs orders of magnitude more. The rules:

- Arithmetic and elementary functions: build terms with `Session::call_value` (local rewriting only), not `compute`.
- `simplify`/`eval`/`diff`/`integrate`/`solve`: `compute`.
- Printing does not simplify by default; the user asks with an explicit `simplify` or `:latex`.
- Comparison and `if`: a `Prop` is not evaluated; use `decide` or `assuming` when a decision is needed.

### 10.6.2 Caching

- `Config` is cached per rule set / assumptions / mode; terms are cached by structural hash; compiled functions by term hash + input names; the unit table and dimension aliases are compile-time constants. rssn also has a request-level answer cache and an optional sub-term lemma cache (appendix C, Revision 12).
### 10.6.3 Batching

- Do not use `compute_all` for element-wise symbolic tensor operations (one request per element is costly): build terms with `map` and `simplify` once at the end.
- Numeric tensors use the `F64` path and never enter the term graph.

### 10.6.4 JIT

`Term::compile` compiles a closed `Float` expression into a function; Aevia's hot functions take this path. `TieredBackend` interprets first and compiles in the background; if the JIT fails it falls back to the interpreter with no change of semantics. The default build has `features.jit = false`, and `@jit` MUST read `Features`, otherwise it reports `E1001("core.jit")`.

### 10.6.5 Devices: data-parallel execution

rssn has two independent execution axes:

| Axis | rssn abstraction | The question it answers |
|---|---|---|
| How one term becomes a function | `Backend` / `Compiled` | evaluate at one point |
| Where many rows run | `Device` / `Buffer` / `KernelIr` | evaluate on 10⁶ rows; integrate 10⁴ initial values |

The JIT (§9.6) belongs to the first axis and is **not** the GPU path; `@jit(target = Gpu(n))` selects a device on the second axis.

**Capability discovery.** Build level: manifest `features.devices` (`cpu`, `cpu-parallel`, plus `gpu-wgpu` when that feature is enabled) is a compile-time fact and may enter `manifest_hash`; `core.gpu` holds when `features.devices` contains any `gpu-*`. Run level: `rssn::backend::device::registry().list()` returns `DeviceInfo` (`id`, `kind`, `dtypes`, `ops`, `parallelism`, `unified_memory`, `deterministic`); having no adapter is not an error, just no GPU device. The Aevia-side wrapper is `aevia_rssn::device` (`available()`, `has_gpu()`, `resolve(Selector)`).

**`Device` to rssn device.**

| Aevia | rssn id | Note |
|---|---|---|
| `Cpu` | `cpu` | single-threaded; reference semantics |
| `Cpu` (hint `cpu_parallel`, `Selector::CpuParallel`) | `cpu-parallel` | rayon; **bit-identical** to `cpu` |
| `Gpu(n)` | the `n`-th `DeviceKind::Gpu` (`gpu:wgpu:n`) | if absent, `E1001("core.gpu")` with the number of registered GPUs in the message |
| `Distributed(c)` | none | `E1001("core.distributed")`; outside this design |

**Transfers are explicit.** `to_device(t, Gpu(n))` is one `upload` (an `F64` tensor is converted to `f32` on an `f32`-only device, which changes values); `device_of` reads the tensor's device tag; bringing a result back to the host needs a `download`, and there are no implicit transfers. `on(device, thunk)` is scoped execution: batch primitives inside `thunk` use that device and buffers are released when the scope ends.

**Precision contract.** The semantics of kernel IR is `f64`. `cpu` and `cpu-parallel` provide `F64` bit-identical to the interpreter (including NaN, `select`, and out-of-range `gather` reading NaN), extending §9.7.2 ("the thread count never changes any result") to devices. `gpu:*` (WGSL) provides `F32` and a **double-single `F64`**: WGSL has no `f64`, so the wgpu device represents `Dtype::F64` as a pair of `f32` values `(hi, lo)`, with about 48 bits of precision (on an RTX 3060 / Vulkan the relative error is `4e-15` for `+ - * /`, `1e-14` for `sqrt`, `7e-14` for `exp`; the device tests require `5e-12`). Limits: the exponent range is still that of `f32` (keep data within `1e-30 .. 1e30`), `sin`/`cos`/`tan` return NaN for `|x| > 1e6`, and `F32` and `F64` buffers cannot be mixed in one launch. So it is **not** IEEE double precision and is not bit-identical to the CPU. Aevia never silently turns a `Float` result into `f32`; code needing bit-exact results chooses `Cpu`/`CpuParallel`.

**Launch fusion.** rssn's `device::plan` fuses a chain of element-wise kernels into one launch (intermediate columns become registers and never reach device memory); fusion changes where values are kept, not how they are computed, so on deterministic devices it is bit-identical to the unfused plan. This is an explicit rssn-side API and is not yet exposed in Aevia.

**Functions that can be lowered to a device** follow the JIT gate of §9.6.1: no captures, `Float` scalar arithmetic, elementary functions, comparisons and `if` (both sides pure, evaluated eagerly and then `select`ed). Terms containing host calls (the `EvalFn` of gamma/erf/Bessel, C externs) cannot be lowered to a device, and `Device.map` falls back to the interpreter row by row, with values unchanged (on a CPU device).

**Batch primitives (design).**

```
Device.available : () -> IO (List DeviceInfo)       # registry snapshot
Device.map       : (Float -> Float, Tensor<[n],Float>) -> Tensor<[n],Float>
                                                     # on Cpu/CpuParallel; pure, deterministic
Device.map_on    : (Device, Float -> Float, Tensor<[n],Float>) -> IO (Tensor<[n],Float>)
Device.reduce    : (Reduce, Tensor<[n],Float>) -> Float   # Sum | Prod | Min | Max
Device.ensemble  : (OdeSystem, Tensor<[m,k],Float>, Float, Float, Int) -> Tensor<[m,k],Float>
                                                     # one program, k initial values; fixed-step RK4
```

Only the pure, bit-deterministic versions are promised on CPU devices; `map_on(Gpu(n), ..)` is `IO` because allocation and transfer are effects, and the result is subject to the device's precision.

**Status.** Implemented: the whole rssn device layer (registry, `Buffer`, `KernelIr`, CPU reference device, WGSL emitter, optional `gpu-wgpu`, `ensemble_rk4`, and `device::array` reductions / matmul / CSR product / stencil, etc.); the manifest `features.devices`; `aevia_rssn::device` and `jit::map_floats` in `aevia-core` (apply a pure `Float` function in batch on a CPU device). **Not implemented**: `Device.map`/`map_on`/`reduce`/`ensemble`/`available` as language primitives (entries are needed in the type table and the built-in table, and `Device.aev` regenerated); `to_device`/`on` on `Gpu(n)` still report `E1001("core.gpu")` (tensor values do not yet carry device buffer handles).

## 10.7 Error mapping

| rssn error | Aevia diagnostic |
|---|---|
| `ComputeError::Dimension(e)` | `E0401` or `E0412` (`Dyn`) |
| `ComputeError::NotNumeric(s)` | `E0803` (free symbol) or `E0806` (domain) |
| `ComputeError::Inconsistent(s)` | `E0809` (internal error) |
| `ComputeError::Rule(e)`, `UnknownOperator(s)` | `E0899` (internal error) |
| Budget exhausted | `E0808` |
| `BackendError` | `E0807` or `E0899` |

`Answer.diagnostics` become Aevia warnings; under `Advisory` mode they do not fail.

## 10.8 Known gaps and risks

**The numbers below are measurements; the live manifest (`aevia rssn`) is the only source of truth.** On `rssn 3.0.0-rc.1` (`api_version = 2`): 1620 operators (99 declared + 103 overlay + 1418 derived, none dynamic-only), 82 units, 30 constants (18 exact), `manifest_hash = d12fc2b7…`. rssn has implemented the Manifest, dynamic `Value`/`call_value`/`call_key`, overload resolution, 100% signature coverage, the physical-constant table, the cdylib plugin ABI, `rssn::dim`, custom base dimensions, `Config::dimension_term`, `fork/merge`, structured results and JIT lowering. Only the integration gaps that **still exist** are listed.

1. **`Session` is single-threaded.** Crossing threads needs `fork/merge`, not a shared session; the bridge currently shares no symbolic values across threads (§10.5.1).
2. **`compute` still clones the graph.** `OpTable` is an `Arc` (clones share it) and `DimArena` is copy-on-write, leaving the shallow-copy cost of the node table; `Session::prepare` is not public.
3. **The JIT is off by default** (see §10.6.4).
4. **The "not reported" semantics of `Solutions`.** An empty `multiplicities` means not reported and MUST NOT be filled with 1; the kernel fills it when it can prove it (a univariate rational polynomial whose roots are all exact numbers, via exact square-free decomposition); `conditions` holds only conditions declarable by construction; `complete` is distinguished by whether the result is a `list(...)`, and `Solutions::new` defaults to `false`. Aevia MUST NOT declare exhaustiveness unconditionally when constructing one.
5. **Constants are values, not units.** `manifest.constants` is not in the `UnitRegistry` and cannot take part in `quantity` parsing; `e` (elementary charge) has the same name as `Math.e` and MUST be qualified when both modules are `open`.
6. **100% typed, but only `Declared` + `Overlay` are exact.** `Derived` distinguishes only numeric `(Real,…)` from symbolic `(Term,…)` and not `Tensor`/`Quantity`/`Int`, so `Derived` MUST be marked "conservatively inferred".
7. **`assuming` is mostly unary predicates**; general equations and inequalities have no language entry point (§10.2.2).
8. **Symbolic exponents: the bridge round-trips, the language does not support them.** `DimExpr::to_term` interns `Length^n` as an atom and `try_from_term` is its exact inverse; Aevia's static type positions still accept only rational exponents (§4.12.3). Do not read "the backend can represent it" as "the language should support it".
9. **Simulation parameters are still JSON**; typed wrappers come from the `Physics.*` source modules or the generated `Rssn_Sim_<Name>` (§8.17).
10. **Budgets and interruption.** rssn's budget lives inside `compute`; Aevia's outer timeout/cancellation is cooperative and cannot kill.
11. **Version drift.** Aevia pins `api_version` + `manifest_hash` and runs conformance. The version-2 manifest adds Rust source-path metadata, and its binary layout is incompatible with version 1 with no version negotiation before decoding, so a host MUST know the producer version in advance. The manifest may still change during the rc cycle, which is why numbers in this document are measurements and not constants.
12. **`manifest_hash` is a bincode BLAKE3 and can be taken two ways.** `hash_hex()` (64 hex characters, which hosts SHOULD pin) and `hash_u64()` (the first 8 bytes read little-endian, for ABI/display only) are not interchangeable; JSON is for humans, and hashing or comparing JSON text is not allowed.
13. **Whether a `Value` can enter the term graph follows `manifest.value_kinds`** (10 of 17 variants can, 7 cannot, §13.3.1); sparse operations MUST go through `eval_dyn` or Aevia's local layout kernels. A generator MUST NOT hard-code this table.
14. **The callback and plugin bridges.** `core.callback` calls a checked source closure through a same-thread scoped handle, and the handle expires when the outer call returns (§8.19); `core.plugin.source` traces `@op` functions into kernel definitions and `@rule` into rewrites, registered as in-process plugins (§13.5.2).
15. **Units belong to a session.** A unit registered by `Session::register_unit` appears only in that session's manifest; the process-level `KernelManifest::build` gives a different unit table and hash, and Aevia MUST use `Session::manifest()`.
16. **SDE.** The manifest exposes only the noise primitives and the scheme metadata `sde_scheme_info` (strong order only; the weak order is the host-side `sde_order`, §8.9.1); the solve loop needs a `&Fn + Sync` host closure that the manifest's `Value` ABI cannot carry, so it stays on the host side (`aevia-std::sde`), and `sdeint_*` are not `Rssn` operators.

### 10.8.1 Mitigations

- **Conformance tests**: for every standard operator, compare Aevia's static dimension with rssn's `Answer.dimension`; any disagreement fails.
- **Unit-table validation**: at start-up compare base, derived and affine units one by one, against `manifest.units` only.
- **Fallback paths**: a JIT failure falls back to the interpreter; a failed symbolic solve returns `Err` and never panics.
- **Budget defaults**: 2 s / 1e6 rewrites; adjustable by the user.
- **Version pinning**: pin `api_version` + `hash_hex()`; run the full conformance suite on upgrade; operator and unit counts in documents never enter cache keys.
- **Bridge isolation**: every rssn call is concentrated in `aevia-rssn`, and upper layers do not depend on `rssn` types directly.

## 10.9 An end-to-end call chain

```aevia
fn fall_time(h: Float[L]): Float[T] =
    sqrt(2 * h / g)
```

1. Type checking: `h : Float[L]`, `g : Float[L/T^2]`, `2*h/g : Float[T^2]`, `sqrt : Float[T]`.
2. Calling `fall_time(20.0 m)`: `20.0 m` is folded at compile time to the `Float` value 20.0 with display unit `m`.
3. The body: `2 * h`, `/ g` and `sqrt` use plain f64 arithmetic on `Float` and never enter the term graph; if the argument were a symbolic `Real`, they would be `mul`/`pow`/`sqrt` terms built by `Session::call_value` (local rewriting only), still without `compute`.
4. `println` calls `show` and prints in the display unit `s`: `2.0196199771025523 s`.
5. `approx(x)` on a symbolic or exact value runs `compute` with a `Numeric` target; rssn returns a numeric ball and Aevia takes its midpoint.

This chain shows the core performance strategy: **types and dimensions at compile time, arithmetic at term construction, only heavy solving enters the e-graph, and numeric hot spots go to f64/JIT**.

The next chapter shows how these rules combine in complete programs.
