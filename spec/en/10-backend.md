# 10 — The rssn backend mapping

Aevia does not reimplement computer algebra. rssn provides the term representation, the rewriting engine, the dimension layer, numeric kernels, the JIT and simulation; Aevia provides syntax, type and dimension checking, closures, records, tensor shapes, modules, IO and the orchestration of the standard library. This chapter defines the contract between the two, including direct operator mappings, typed wrappers and their error behavior.

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
- `Term::eval(bindings)` evaluates numerically without rewriting; `Term::compile(inputs)` compiles a closed form with the active RSSN backend; `Term::as_tensor()` turns nested `list` terms into `(shape, Vec<f64>)`; `to_latex()`/`to_typst()`/`to_pretty()` typeset.
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
- **Generated at run time.** A module is rendered from the live manifest when `import Rssn_NumberTheory` is resolved (`codegen::namespace_source`), and no generated source is committed, so it cannot drift from the linked kernel. The committed overlay and coverage report `tools/rssn-api-audit/RSSN-COVERAGE.md` are checked by `bazel test //:contracts_test`. Use `bazel run //:refresh_kernel` to regenerate the checked-in artifacts after changing the contract.

**`Rssn_Numeric`.** rssn's `numeric` rule set (robust statistics, windows and spectra, wavelets, Krylov/sparse/low-rank/tensor decompositions, dimension-independent finite-difference solvers, complex special functions, expression operators) all have a `sig`, and none stays in the dynamic tier; user-facing interface and examples are in §8.19.1. The rules:

- Results are floating point, so signatures use `Float`/`Tensor<n, Float>`, **not** `Real` (`from_value<Real>` accepts only exact `Int`/`Rat`); integer-valued results (ranks, permutation indices) are likewise returned as `Float`.
- Method, wavelet and statistic parameters named by symbols have type `Real`; every call of `symbol("db2")` makes a new symbol, so bind each name once with `let` and reuse it.
- **One kernel operator may have several `sig`s, each with its own Aevia name** (the overlay writes `sig heat_fd as heat_fd_2d(..)`; a duplicate name is an error): since a signature has a single literal rank, dimension-independent operators are named by dimension count (`_2d`/`_3d`) and rank-polymorphic tensor operators by rank (`tensor_norm_<r>`, `tensordot_<ra>_<rb>_<k>`, etc., §8.19.1).
- **Decoding mixed lists.** The result of a tensor decomposition is a list of parts of different types. The bridge keeps it as a one-dimensional value with per-element dimension evidence, and `from_value<Tensor<1, Value>>` is the generic "list accessor": it hands out the parts along the first axis one by one as `Value` handles, and each is then decoded with `from_value<T>`. The decoders are Aevia source in the `Numeric` prelude (`hosvd3_from_value`, …), declared as decode-only types with `codec Hosvd3 - hosvd3_from_value` (`-` means no encoder; using such a type as a parameter is a coverage error).

Examples: `tests/domain_number_theory.aev`, `domain_combinatorics.aev`, `domain_finite_fields.aev`, `domain_numeric.aev`.

`Zmod` is a nominal residue type that stores its modulus at run time, so the same program can use
moduli it computes from inputs. Construct a residue with `Rssn_NumberTheory.zmod(value, modulus)` and
use `zmod_add`, `zmod_mul`, `zmod_inv`, or `zmod_pow`. Construction and arithmetic return
`Result<Zmod, String>`. Binary operations check that the moduli agree, and inversion checks that the
residue is invertible. For example, modulo 17, `zmod_pow(x, -1)` computes the inverse and
`zmod_mul(x, inverse)` returns the residue 1.

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

- `Config` is cached per rule set / assumptions / mode; terms are cached by structural hash; compiled functions by term hash + input names; the unit table and dimension aliases are compile-time constants. rssn also has a request-level answer cache and an optional sub-term lemma cache.
### 10.6.3 Batching

- Do not use `compute_all` for element-wise symbolic tensor operations (one request per element is costly): build terms with `map` and `simplify` once at the end.
- Numeric tensors use the `F64` path and never enter the term graph.

### 10.6.4 JIT

RSSN's `Term::compile` compiles a closed `Float` expression into a function. Its
`TieredBackend` interprets first and can compile in the background. Aevia lowers
eligible checked Core IR scalar functions to RSSN's LIR and compiles it with
Cranelift after warm-up, or on request with `@jit`. Native execution requires a
verified concrete activation with supported scalar representations. Effects,
unsupported programs and failed compilation keep the interpreter path.

The native toolchain enables Cranelift, wgpu and RSSN's NumPy I/O feature by
default. An explicit `@jit` in a build with no native JIT support reports
`E1001("core.jit")`.

Use `aevia devices` to inspect the linked features, current RSSN backend and
available selectors. `--rssn-backend NAME` sets the process default. In the
REPL, enter `:rssn-backend` to inspect it and `:rssn-backend auto` to restore
automatic selection. Aevia controls its own evaluator and matrix offload
policy independently with `--backend`.

On a host with Cranelift support, RSSN `auto` starts with the CPU tape and
compiles hot expressions in the background. Other hosts use the interpreter.
`interpreter` selects the tape path, `cpu` and `cpu-parallel` select explicit
CPU devices, and `tiered` and `cranelift` select compiled paths where supported.
`jit` is an alias for `cranelift`. Use an available device ID for a specific GPU,
or `gpu` for the first discovered wgpu adapter. An unavailable explicit
`--rssn-backend` selection exits with status 2.

For example, `aevia --rssn-backend cpu-parallel test examples/native_backend.aev`
uses the parallel CPU backend for RSSN compilation in the example. Source
code can select a backend for an individual program through the declared
`num_backend_named` and `num_backend_compile` operators. The `gpu` capability
is required to discover, select or use a GPU backend from source, including
when a GPU handle or compiled GPU program is reused in a narrower `with_cap`
scope.

### 10.6.5 Devices and data-parallel execution

Aevia's `Device` values identify a requested execution location. `device_of(tensor)`
returns its location. `to_device(tensor, device)` and `on(device, thunk)` are IO
actions. Both currently support `Cpu`. A `Gpu` or `Distributed` placement reports
`E1001` with capability `core.gpu` or `core.distributed`, respectively (§13.9.4),
even when the RSSN host device registry exposes a GPU.

RSSN's device layer exposes buffers, a numerical kernel IR, CPU reference and parallel
executors, and a WGSL backend behind `gpu-wgpu`. Embedding hosts can use it for batch
evaluation, array operations, and fixed-step ensembles. The Aevia interpreter's CPU
batch path accepts the pure scalar subset described in §9.6.1. Device enumeration,
launch fusion and explicit GPU buffer transfers are RSSN host APIs.

For large dense `Float` matrix products on the `@` path, Aevia's automatic
policy can select a compatible hardware GPU with native F64 in the RSSN registry.
The `gpu` capability must be available. Aevia keeps tensors on the host and uploads and
downloads the operands and result for each offloaded product. `--backend gpu`
also considers smaller products and the exact subset supported by wgpu. The CPU
path handles unsupported work and device failures.

The RSSN wgpu backend represents emulated F64 buffers as two F32 parts. Aevia
can use its F32 kernel only when all inputs are exactly representable integers
and the magnitude of any partial sum is bounded by 2^24, so rounding cannot
change the products or accumulated result. Inputs that can create negative zero
products use the host path. General `Float` products retain
binary64 arithmetic on the host unless the device offers it directly.

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

## 10.8 Kernel boundary invariants

- A term belongs to one RSSN session. Parallel work prepares or imports it on
  a thread with an appropriate session, and returned results are restored in
  input order (§10.5.1).
- The kernel manifest for that session determines operator availability, units,
  constant values and which values may enter the term graph. Generated bindings
  use that manifest's `api_version` and full `hash_hex()` when identifying the
  interface. The truncated `hash_u64()` is used for ABI reporting and display.
- An exact typed binding pins a supported signature. Derived or witnessed
  signatures are used only where the generator can state and verify their
  contract. The dynamic boundary preserves operation errors (§10.3.5).
- `Solutions.multiplicities` may be empty when multiplicity was not established.
  A solver reports completeness only when its method produced a complete
  solution set. Conditions accompany the solutions to which they apply.
- A compile-time dimension in Aevia may contain rational exponents (§4.12.3).
  Some RSSN hosts can also represent symbolic exponents. Dynamic interfaces
  must preserve their dimension evidence or report an error.
- Callback handles are scoped to their originating host call (§8.19).
  Simulation parameters may enter as a schema-checked record or through the
  generic JSON interface (§8.17). Optional native code and device capabilities
  are determined at the point of selection (§10.6).

### 10.8.1 Compatibility and validation

Use `aevia rssn --audit` to inspect binding coverage for the linked kernel.
Use `aevia rssn --describe OPERATOR` for one operator's signatures, dimension
rule and callable bindings. `aevia rssn --emit --out generated --namespaces`
writes the modules for that build. When a declaration has a verified typed
callback facade, use its emitted signature. For an explicit higher-order call,
`Rssn.call_ho` groups ordinary arguments and `to_callback` closures into separate
lists while retaining the signature's order within each list (§8.19).

Conformance tests compare source declarations and checked adapters, exercise
structured values and units, and verify the compiled evaluator against the
reference path. Runtime and symbolic budgets are separate (§6.9.1). Native
evaluators that cannot execute a supported code path can return to the interpreter.

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
