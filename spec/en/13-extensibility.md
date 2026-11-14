# 13 — Extensibility and the rssn integration

This chapter is part of the specification and **takes precedence over** any "unsupported for now / to be evaluated later" wording elsewhere. Aevia has no `v1`/`v2` layering: language capabilities are designed once and in full, and whatever the backend has not yet implemented is reported as a **capability diagnostic** (`E1001`) rather than by removing a language feature.

The detailed backend contract is in rssn's `docs/KERNEL.md` (Kernel Manifest, dynamic-call ABI, plugins), `docs/DIMENSIONS.md` and `docs/CALLBACKS.md`; this chapter defines the normative contract on the Aevia side.

---

## 13.1 Three axioms

1. **A single operator registry.** rssn's `OpTable` is the only source of truth. Aevia keeps no list of "which operators are supported"; it reads the **Kernel Manifest** and generates the `Rssn` module. A new operator registered in rssn is visible at Aevia's next build.
2. **A single dimension/unit layer.** `rssn::dim`'s `DimTerm`/`DimArena`/`UnitRegistry` is the canonical layer shared by run time and compile time; Aevia's `Dim` is its type-level mirror, and `Units.*` is generated from `manifest.units` with no second unit table.
3. **Dynamic calls are a first-class capability.** An operator without a type signature can still be called through `Rssn.dynamic(name, args)`; the typed façade is an increment to the static experience, not a precondition for capability. rssn validates on the dynamic path at the **signature level** (`check_args`) and does no dimension or type inference; the full check belongs to Aevia's static checker or `Rssn.typed`.

Together these answer two things: **thousands of operators cannot be covered by hand-written bindings** (they drift, miss things and rot), and **new capabilities cannot wait for "the next version"** (the language, the manifest and the plugin ABI are fixed now, and the backend fills in capabilities step by step).

---

## 13.2 The Kernel Manifest and the generated `Rssn` module

### 13.2.1 The manifest

At start-up rssn provides a deterministic `KernelManifest`:

```
KernelManifest
  rssn_version : String
  api_version  : Int
  manifest_hash: Bytes32
  features     : Features
  rule_sets    : [RuleSetInfo]
  ops          : [OpInfo]
  units        : [UnitValue]
  base_dimensions : [BaseDimInfo]
  plugins      : [PluginInfo]
  constants    : [ConstantValue]
  value_kinds  : [ValueKindInfo]
```

- `OpInfo`: `keys: [OpKey]` (one per overload; `keys[0]` is the primary overload), `name`, `aliases`, `category`, `arity`, `overloads`, `signature_source`, `flags`, `binder`, `cost`, `eval`, `higher_order`, `purity`, `determinism`, `thread_safety`, `stability`, `since`, `doc`, `examples`, `dim_rule`, `backend_support`, `dynamic_reason`, `rule_set`, `jit`. The full schema is in §3.1 of the documents at the root of the rssn repository.
- `manifest_hash` is the BLAKE3 of deterministic bincode; `to_json()` is for humans only.
- `units: [UnitValue]` carries `name`/`display`/`dimension`/`scale`/`offset`/`affine`/`aliases`/`prefixable`/`category`/`since`; generating `Units.*` needs to read only this. `KernelManifest::build_with_units(graph, sets, plugins, &registry)` reads units from the caller's registry and `build(...)` from the process-level table; `Session::manifest()` uses the former, so a unit registered by `Session::register_unit` enters **that** session's manifest only.
- `value_kinds: [ValueKindInfo]` describes whether each `Value` variant can enter the term graph (`in_term_graph`) and the alternative when it cannot (§13.3.1).
- `constants: [ConstantValue]` is the table of physical constants, with fields `name`/`symbol`/`value: ScaleValue`/`dimension: DimExpr`/`unit`/`exact`/`uncertainty`/`source`; `Units.Constants` is generated from it (§13.6.5).
- `features.plugin_kinds` says which plugin kinds the build can load (default `["in-process"]`; `plugin-cdylib` adds `"cdylib"`); `features.sde` says that the SDE noise primitives and the scheme-metadata operator can be called from the manifest (host capability name `core.sde`). `Features` is part of the manifest: adding a field or operator changes `manifest_hash`.

### 13.2.2 Generation rules

At build time:

1. Read the manifest and check `api_version` (an incompatible one is `E1004`).
2. For each `OpInfo`: `signature_source = Declared | Overlay | Derived` → generate a typed `Rssn.<name>` declaration (**one per overload**); `None` → `Rssn.dynamic`; an alias → `Rssn.<alias>`, or `Rssn.<rule_set>.<name>` on a clash.
3. A typed declaration remembers the corresponding `OpKey` from `OpInfo.keys` and calls pin that overload, so static selection and run-time execution cannot diverge. The public form is `Rssn.call_key(key, …)`; the generator actually emits the equivalent `Rssn.invoke(name, signature_hash, args)` (the same pinning semantics with a different error channel: a generated `fn` MUST return its declared type, so a kernel failure raises instead of returning `Err`).
4. Generate `Rssn.Version`, `Rssn.ManifestHash`, `Rssn.Capabilities`, `Rssn.ValueKinds`, `Rssn.Constants` and `Rssn.Units`.
5. Generate LSP completions, documentation and signature stubs, recording the source of each declaration.

**Coverage is a measurement, not a constant.** On `rssn 3.0.0-rc.1` (`aevia rssn` gives the live figures) all 1620 operators have signatures (99 declared + 103 overlay + 1418 derived). The precision tiers the generator MUST distinguish:

| Source | Precision | What is generated |
|---|---|---|
| `Declared` | exact | typed |
| `Overlay` | exact | typed |
| `Derived` | shape-level | typed, but only `Real`/`Bool`/`Term` |
| `None` | none (leaf / non-callable) | `dynamic` |

`Derived` is "inferred from registration data", not name guessing:

| Registration data | Inferred signature |
|---|---|
| has `eval` | `(Real, …) -> Real` |
| has `eval` + `PREDICATE` | `(Real, …) -> Bool` |
| not a leaf | additionally `(Term, …) -> Term` |
| `Binder` at position k | the k-th parameter is a `Symbol` |
| `Arity::Variadic` | the trailing parameter is variadic |
| `OpFlags::LEAF` | no signature (`DynamicReason::OpaquePayload`) |

So `sin` has two overloads, `(Real) -> Real` and `(Term) -> Term`; `det` is a `HEAVY` request without `eval`, so without an overlay it has only `(Term) -> Term` and the static type of `det(matrix)` would be `Dyn<term>`; the overlay supplies `(Tensor<[n,n],Real>) -> Real`. Hence "100% typed" is not "100% exact": `Derived` does not distinguish `Tensor`/`Quantity`/`Int`, and IDEs mark it "conservatively inferred".

Overlays are also the carrier of **dimension type parameters**: `sde_wiener`/`sde_levy_areas` use a dimension parameter such as `T` to write `sqrt(h)` (`RootOf{degree:2,index:3}`) and `Tensor<1, Real[T]>` into the signature, which derivation cannot infer. The spelling of the generated source is exact (§10.0): the `Rssn.sde_wiener` produced by `aevia rssn --emit` is `(Int, Int, Int, Real[T]) -> Tensor<2, Real[T^(1/2)]>`. `Dyn<term>` remains only for types that genuinely cannot be spelled (§10.0).

**Typing a literal pinned key.** The generator emits only `Rssn.invoke(op, key, …)` with literal `op` and `key`; the checker validates these two strings against the pinned overload in the live manifest and gives that overload's declared result type (`Rssn.det` → `Real`), with fresh dimension variables at each call. A **non-literal, unknown or ambiguous key** stays a dynamic `Value`, with the diagnostic produced at run time: `E1005` (unknown or mismatched `signature_hash`), `E1002` (ambiguity), `E1003` (a dynamic argument or non-function value). The checker never re-parses or executes a key.

The SDE solve loop is not in the manifest (host callbacks, kernel CO-03); the host-side `sdeint_*` do not go through the `Rssn` operator channel, and `sde_scheme_info` is a manifest-only metadata query (strong order only; the weak order is the host-side `sde_order`, §8.9.1).

**Repository rule.** The Aevia repository MUST NOT contain per-operator hand-written binding files, and keeps no copy of `signatures.overlay.json` (it is an rssn build input, already merged into `OpInfo.overloads`). Only the generators are allowed (`aevia rssn --emit` in `aevia-rssn`; the generated views of the standard library are written by `aevia-stdlib-gen`). CI checks: generator output equals the committed generated files, and a change of the manifest snapshot MUST be updated explicitly.

### 13.2.3 Namespaces

- Ordinary operators: `Rssn.<name>` (`Declared|Overlay|Derived` are all typed; only `None` and plugin operators use `Rssn.dynamic`).
- Rule-set qualification: `Rssn.<rule_set>.<name>` is only a namespace; the run time still goes by name/arity. rssn's `call_value` looks up only by name/alias and does **not** support `rule_set/name`; to pin an overload use `Rssn.call_key(key, args)` with a `key` from `Rssn.overloads(name)` or `OpInfo.keys`.
- Plugins: `Rssn.<plugin>.<op>`.
- Domain modules: `import Rssn_NumberTheory` (`Rssn_<Domain>`, CamelCase) gives wrappers grouped by mathematical domain with types and a `Result` channel (§10.3.8); `Rssn_<rule_set>` remains grouped by kernel and stays available.

On a name clash, a short name keeps only the **most stable** one (`Stable` first, then earlier `since`); the others must be qualified. The generator records the short-name map in the manifest.

---

## 13.3 Dynamic values and dynamic calls

### 13.3.1 `Value`

```aevia
type Value =
    | VInt(Int)
    | VReal(Real[1])
    | VFloat(Float[1])
    | VComplex(Complex[1])
    | VBool(Bool)
    | VString(String)
    | VSymbol(String)                 # a free symbol, not a String
    | VUnit(Unit[1])
    | VQuantity(Quantity)
    | VTensor(TensorData)             # shape + row-major Values
    | VSparse(SparseData)
    | VRecord(Record)
    | VVariant(Variant)
    | VTerm(TermRef)                  # owned handle
    | VFunction(Callback)
    | VDyn(Dyn<Real>)
    | VOpaque(Opaque)
```

`Value` is the type of the **dynamic boundary**, not an implicit subtype in the core language; it appears only in `Rssn.dynamic`, plugins, FFI and the reflection API.

**Term-graph admissibility** comes from `manifest.value_kinds`, and Aevia keeps no second copy. Currently (17 variants):

- `in_term_graph = true` (10): `VInt`/`VReal`/`VFloat`/`VBool`/`VString`/`VSymbol`/`VQuantity`/`VTensor`/`VTerm`/`VDyn`;
- `in_term_graph = false` (7): `VComplex` (lowered to a `Float` only when the imaginary part is 0, otherwise `Unsupported("complex literals")`), `VUnit` (a unit is a definition, not a number; cross the boundary with `quantity(value, unit)`; only declared unit-parameter positions take a `Value::Unit` directly), `VSparse`/`VRecord`/`VVariant`/`VFunction`/`VOpaque`.

A variant with `false` can only be fed to operators that have `eval_dyn` or serve as a return value; a generator MUST NOT assume every `Value` can take part in rewriting.

### 13.3.2 Calls

```aevia
Rssn.call    : (String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.dynamic : (String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.typed   : (String, Tensor<1, Value>, TypeRep) -> Result<Value, OpError>
Rssn.overloads       : String -> Tensor<1, String>
Rssn.resolve_overload: (String, Tensor<1, Value>) -> Result<String, OpError>
Rssn.call_key        : (String, String, Tensor<1, Value>) -> Result<Value, OpError>
Rssn.Constants       : Tensor<1, String>     # constant symbols; values are in the generated Constants module
Rssn.Units           : Tensor<1, String>     # unit names; values are in the generated Units module
```

- `Rssn.call` is an alias of `Rssn.dynamic` and looks up only by short name/alias; an ambiguous short name is `E1002`.
- `Rssn.call_key` "runs without resolving": it takes a name + `signature_hash` (32 hex characters), which together are the writable form of an `OpKey`; all zeros means the operator has no declared overload. A key that does not match the current manifest is `E1005` (regenerate the module); an argument that departs from the pinned signature on the kernel side is `UnknownOp`, which becomes an error value at the Aevia boundary.
- `Rssn.resolve_overload` exposes the run-time selection for caching and diagnostics: on success it returns `"rule_set.name@signature_hash"`; both a tie and "no signature accepts" are `OpError::Ambiguous`. `Rssn.overloads` returns the rendered signature of every overload.
- Arguments are injected with `to_value(x)` and results converted back to a static type with `from_value(v)`. Operators without signatures, plugin operators and experimental entries use `dynamic`; the standard set currently has no `None`, so on it `dynamic` is only the plugin channel.
- `type_rep<T>()` keeps the concrete type resolved by the checker; it is an opaque value that an ordinary record cannot forge. After the dynamic call, `Rssn.typed` checks representation, shape and dimensions against that witness; a mismatch or missing evidence returns `Err(OpError)`. A successful result is still a `Value`, validated and converted through `from_value<T>`; a generic not made concrete by a run-time type dictionary cannot produce a `TypeRep`.
- Higher-order operators use `Rssn.call_ho(name, args, fns)`; `to_callback` builds an opaque callback from a checked source closure, with the limits of §8.19. A callback runs synchronously on the original thread and expires when the outer call ends; calls across threads and re-entry in the same scope are rejected, and callback errors come back through `OpError`; an empty list is just a dynamic call.
- `Rssn.register`/`Rssn.register_plugin` accept a host `Plugin`; source plugins are published by `@op`/`@rule` and `load_plugin` (§13.5.2). `core.plugin.source` is enabled in builds with in-process plugins; `core.callback` means the restricted callback bridge is available.

### 13.3.3 Errors

```aevia
type OpError =
    | UnknownOp(String)
    | Arity(Int, Int)
    | Type(TypeRep, TypeRep, Int)
    | Ambiguous(String, Int)         # name, number of ties
    | Dimension(DimError)
    | NotNumeric(String)
    | NotReduced(String)
    | Domain(String)
    | BudgetExceeded
    | Unsupported(String, String)   # backend, feature
    | Plugin(String)
    | ForeignTerm(String, String)   # expected session, got session
    | Codec(String)
    | Host(String)
    | Internal(String)
```

`E1003` is "no matching operator/overload" and carries the list of candidate signatures; `E1001` is "backend capability missing" and carries the capability name and the minimum rssn version.

---

## 13.4 Overloads, signatures and resolution

### 13.4.1 Overloads

rssn allows several signatures under one name, decided in `kernel::overload`:

```aevia
type Match = | No | Generic | Shaped | Widened | Exact   # ordered
matches_type : (TypeExpr, TypeExpr) -> Match
matches      : (TypeExpr, Value) -> Match
score        : (Signature, Tensor<1, Value>) -> Option<Nat>
score_types  : (Signature, Tensor<1, TypeExpr>) -> Option<Nat>
best         : (Tensor<1, Signature>, Tensor<1, Value>) -> Best
best_types   : (Tensor<1, Signature>, Tensor<1, TypeExpr>) -> Best
type Best    = | None | Unique(Int) | Ambiguous(Tensor<1, Int>)
```

- Each argument is classified separately, and the score of the whole call is that of the **weakest** argument.
- `Int -> Real` and `Int|Real|Float -> Complex` are `Widened` and the reverse does not hold; the same constructor with element/dimension not pinned by the signature is `Shaped`; type variables and `Dyn` are `Generic`.
- A tie at the top inside `best` gives `Ambiguous`; it never silently takes the first.
- `score_types`/`best_types` share the same score fold as `score`/`best`: Aevia resolves once at compile time and gets the same number the kernel would get at run time.

In the standard set each name has one registration, but **each registration has 1–5 overloads** (an operator with `eval` has both a numeric and a symbolic one, and an overlay adds exact signatures). Two things must be kept apart:

- **Registration selection** (`resolve_op`) returns directly at `fitting.len() == 1` and does no scoring on the standard set; a cross-registration tie can come only from plugins.
- **Overload selection** (`resolve_overload` → `overload::best`) really runs over the signatures within a registration: `sin(0.5)` selects `(Real) -> Real` and `sin(x)` (`x` a `Symbol`) selects `(Term) -> Term`. But when the argument itself is a `Term`/`Dyn`, the numeric overload also passes as `Generic`, and the two tie → `Ambiguous`; so `resolve_overload` suits only arguments of concrete type, and for symbolic expressions use a key from `OpInfo.keys`.

`call_value` selects a registration by name/arity and then calls `check_args`: when no overload accepts the arguments it returns `OpError::Type`. But the `Dyn<term>` of the symbolic overload is `Generic` for any value, so derived operators nearly always pass — the check really blocks only when all signatures are concrete (`eq`/`lit`/`pow`); it does not choose between numeric and symbolic (the operator's `eval` decides).

Aevia's own static resolution rule (isomorphic to rssn's lattice, for Aevia's own overloads and plugins):

1. filter by arity; 2. classify each argument and take the weakest; 3. a unique highest score is selected; 4. a tie at the top is `E1002`; 5. no match → try `Dynamic`; still failing → `E1003`.

Three run-time entry points:

- `Session::call_key(&OpKey, …)` **does not re-resolve**: it finds the registration by `rule_set` + `signature_hash`, pins the overload, and requires the arguments to be accepted by the pinned signature (`overload::score(pinned, args).is_some()`), otherwise `UnknownOp`.
- `Session::call_value(name, …)` finds the registration by name/arity; with several candidates it picks the most specific with `overload::score` and reports a tie as `OpError::Ambiguous`; arguments accepted by no overload are `OpError::Type`.
- `Kernel::call(&OpKey, …)` forwards to `call_key` and therefore also pins the overload; passing through `dyn Kernel` loses nothing.

`Session::resolve_overload(name, args)` is for "resolve first, then cache": a `Unique` `best` returns a key with `signature_hash` that can be handed straight to `call_key`; an operator with no signature at all returns a zero-hash key (which `call_key` accepts); a tie or no accepting signature is `Err(OpError::Ambiguous)` and **never** yields an unexecutable zero-hash key. So the return value can always be handed to `call_key` safely. `OpKey.signature_hash` is the first 16 bytes of `Signature::hash()` (the BLAKE3 of deterministic bincode); a forged, stale or argument-mismatched key returns `UnknownOp`.

### 13.4.2 Signature coverage

Coverage is 100% (`aevia rssn` reports `0 dynamic-only`). The sources, in decreasing priority:

1. `Declared`: `OpDescriptor::new(...).signature(...)`, written by the operator itself;
2. `Overlay`: a manual correction in `signatures.overlay.json` (merged into the manifest on the rssn side; Aevia keeps no copy);
3. `Derived`: inferred by `kernel::infer::derive` from registration data;
4. `None`: a leaf or non-callable form, with `dynamic_reason` stating why (the standard set has none).

Rules: `Derived` reads only `eval`/`PREDICATE`/`Binder`/`Variadic`/`LEAF` and never the operator name, so it is conservative but reliable; when precision is insufficient write an overlay rather than change the inferrer (overlays belong only on operators whose signature must be exact: tensor, symbolic and unit parameters); CI's `signature-audit` reports coverage and source distribution, and a high share of `Derived` does not block the build but blocks marking those operators "exact"; plugins declare signatures with `Registry::register_op`, and `Registry` rejects a registration with neither signature nor `dynamic_reason`. This lets "complete capability" and "type precision" advance separately, without any operator silently forgotten.

---

## 13.5 Plugins and user extension

### 13.5.1 Plugins

A plugin is not new syntax but an **ordinary Aevia package + a manifest marker + attributes**:

```toml
# aevia.toml
[plugin]
kind = "aevia"          # in-process; corresponds to rssn's PluginKind::InProcess
abi = 1
```

| `[plugin] kind` | rssn | Loaded by |
|---|---|---|
| `"aevia"` / `"in-process"` | `PluginKind::InProcess` | `Session::register(Box<dyn Plugin>)` |
| `"cdylib"` | `PluginKind::Cdylib` | `Session::load_plugin(path)` (needs `plugin-cdylib`) |

The C ABI of `cdylib` is a **byte buffer**: the entry symbol is `rssn_plugin_v1`, and arguments and results are bincode-encoded `Vec<Value>`/`Result<Value, String>`, not a `#[repr(C)]` mirror of `Value`; `cdylib::load` checks `PLUGIN_ABI` and `RssnPluginDesc`, and `Session::load_plugin` = `load` + `register`. The module is controlled by the `plugin-cdylib` feature (off by default, included only in `full`) because loading a dynamic library runs arbitrary code inside the host process with no sandbox. Aevia MUST check `manifest.features.plugin_kinds` before loading: without `"cdylib"` it reports `E1001("core.plugin.cdylib")` instead of letting the loader throw a low-level error.

```aevia
module MyDomain

@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))

@rule("softplus(x) => ln(1 + exp(x))")
let softplus_rule = ()

dimension MyDim
unit myunit : Length = 0.5 m
```

`Rssn.register` (alias `register_plugin`) wraps `Session::register(Box<dyn Plugin>)`: `Rssn.register(plugin)  # -> Result<PluginId, PluginError>`. A plugin can register operators + signatures + eval callbacks, rewrite rules, dimension rules, units and base dimensions, and (optionally) a JIT lowering.

### 13.5.2 Aevia functions as rssn operators

`@op` publishes an Aevia function as an rssn operator and `@rule` publishes a rewrite rule:

```aevia
@op("mydomain/softplus")
fn softplus(x: Real[1]): Real[1] = ln(1 + exp(x))

@rule("mydomain/sinh-exp")
fn sinh_exp(x: Real[1]) = (sinh(x), (exp(x) - exp(-x)) / 2)
```

**Decision: tracing, not callbacks.** The kernel's extension language is its own term language: an operator is a definition `name(a, b) := body` and a rule is a rewrite `name: lhs => rhs`. An Aevia function becomes one by **tracing**: call the function with fresh symbols carrying the checked dimension of each parameter, and the term it builds is the definition body. This needs no second compiler and inherits everything the kernel can do — once `mydomain/softplus` is a definition, the kernel can call it, differentiate it, simplify and integrate through it, and evaluate it numerically, which an opaque host callback cannot do (a callback also cannot cross threads, because the `Rc` interpreter is not `Send`).

- The price is checked: a function whose result depends on control flow over its arguments (`if x > 0`, a `match` on a value) or that performs IO cannot be traced into a term, and registration reports `E1006` with the reason.
- A rule function returns a pair `(lhs, rhs)` and its parameters are the rule's pattern variables; `lhs` MUST be an operator application (it is locally simplified while tracing, so `x * 0` is not a legal left-hand side).
- Publication happens after the module's function bindings and before the evaluation of top-level `let`s, so top-level bindings can already call it through `Rssn.dynamic("mydomain/softplus", …)`. Each loaded module registers an in-process plugin `aevia/<module>#n` belonging to the current thread's kernel session.
- **Source operators are first-class operators.** Publication order: first trace each `@op` into a kernel definition and register the checked parameter/result dimensions as a **dimension signature** (`Registry::register_dim_signature`: parameters and result are monomials in signature variables, solved from the argument dimensions at each use; `demo/area(r)` for `fn area(r: Real[d]): Real[d^2]` is `dim(r)^2` in the kernel's dimension inference); then replace the function's binding in the module by a wrapper: when **all arguments are numeric** (or closed terms without free symbols) it runs the Aevia body, and with **symbolic arguments** it builds the operator node `demo/softplus(x)`, whose dimension comes from the signature, and an argument dimension that conflicts with the signature is `E0401`; only then are `@rule`s traced, so a rule can talk about source operators. When `diff`/`simplify`/`integral` act on an operator node, the kernel sees the operator, its definition and all rules about it. If the kernel rejects a plugin, the original binding is restored.
- **Dynamic loading**: `load_plugin(path) : String -> IO (Result<Int, String>)` loads, checks and evaluates a source file (including its imports) at run time, publishes its `@op`/`@rule`s and returns the number published; it needs the `fs` capability.

The `mydomain/` in `@op("mydomain/softplus")` is **part of the operator name** (the plugin namespace), not a `rule_set/name` qualifier; the kernel's definition head allows `/`-separated identifier segments, and `call_value` looks up the full string.

### 13.5.3 Security

- Plugins MUST be signed; by default only plugins the registry trusts are loaded.
- A plugin declares capabilities (`fs`, `net`, `unsafe`); undeclared capabilities are denied in the sandbox.
- A plugin ABI version mismatch is `E1006`.

---

## 13.6 Dimensions and units: unified on `rssn::dim`

### 13.6.1 The canonical layer

`rssn::dim` provides `DimTerm` (a product of base dimensions/variables with rational exponents, with structured access `factors(&arena)`/`vars(&arena)`), `DimArena` (interning, with snapshot/restore), `UnitRegistry` (unit names, aliases, exact scale `Scale { rat, pi }`, affine offset, prefixes, display, category, since), and `Dimension` (a **printable snapshot** with string exponents; use `DimTerm` for exact comparison and computation and never parse `Dimension.factors`).

Aevia's `Dim` is the type-level mirror of `DimTerm`; the canonical form, base names and exponent arithmetic of the two MUST agree. The compiler copies no unit table; `Units.*` is a generated view of `manifest.units` (82 entries at present). Units belong to a **session**: `Session::register_unit(UnitDef)` writes that session's registry, and only `build_with_units` writes it into the manifest; the process-level `build` sees the process table; units registered by a plugin belong only to the session that installed it.

The structured entry point is `Config::dimension_term(symbol, DimExpr)`: it parses no text, unknown base names are interned, and structured assumptions are applied after textual ones (the structured one wins on a name clash). `DimExpr::to_term` writes a term; `try_from_term` is the exact inverse for term-representable dimensions (`Length^n` reads back as `Length^n`); `from_term` is the total version and yields `DimExpr::Any` rather than a wrong dimension when it cannot read back.

The `DimRuleInfo` of `OpInfo.dim_rule` has `RootOf { degree, index }`: the result dimension is the `degree`-th root of the dimension of the **`index`-th argument** (the older `Root(n)` always took argument 0 and could not express the `sqrt(h)` of `sde_wiener(seed, m, steps, h)`); its manifest wire form is `{"root_of":{"degree":2,"index":3}}`. A host reading `dim_rule` MUST match on the variant and not assume only `Root`/`Same`/`Product`.

A custom base dimension is declared by `Config::base_dimension(name)` (idempotent): it sends the base name into the `DimArena`, the internal representation of `Dim` grows from a fixed seven SI exponents to "seven SI + an ordered side table", and `expr_to_dim` accepts any base name; this is what makes `unit EUR : Currency` work (`dimension_term` can produce a `DimTerm` only for base names already in the arena).

### 13.6.2 Unit declarations

```aevia
dimension Currency            # custom base dimension
unit USD : Currency = 1       # base unit
unit EUR : Currency = 1.08 USD
unit myunit : Length = 0.5 m  # built-in base dimension
```

For each `dimension` the generator declares `base_dimension("<Name>")` once on the `Config` and only then registers units; otherwise unit registration succeeds but no `DimTerm` can be produced.

- `dimension` registers in the `DimArena` (at compile time through `DimExpr`, at run time through `Config::dimension_term` / `Graph::assume_dimension_value`); `unit` registers in the `UnitRegistry`.
- Exported by default; `private` is visible only in the module; export information enters the manifest and downstream packages can `import` it directly.
- A user unit may be affine (`unit °X : Temperature = 1 K offset 273.15`), but an affine unit can be used only on its own.
- `features.user_base_dimensions` is hard-coded `true` (every build supports it); only `jit*` and `plugin_kinds` vary by build.

### 13.6.3 `Dyn`

The run-time dimension of `Dyn<ρ>` is a `DimTerm`, not a string. `dyn`/`check` are type-directed primitives: a concrete `d` is materialised as a `DimTerm` at compile time; generics pass it explicitly with `dyn_with`/`check_with`; `check` returns a `Result`.

### 13.6.4 `Point` and affine units

`Point[d]` is a static wrapper; at run time it is a `Float` + `UnitId`. The `affine`/`offset` of the `UnitRegistry` are the only source of truth. The arithmetic rules of `Point` are implemented in the Aevia layer, and rssn's `quantity`/`convert` handle display and conversion.

### 13.6.5 Physical constants

`manifest.constants` is a `Vec<ConstantValue>` (30 entries at present, 18 exact):

```rust
pub struct ConstantValue {
    pub name: String,        // "speed of light in vacuum"
    pub symbol: String,      // "c"
    pub value: ScaleValue,   // exact rational, coherent SI
    pub dimension: DimExpr,
    pub unit: String,        // "m/s"
    pub exact: bool,
    pub uncertainty: Option<ScaleValue>,
    pub source: String,      // "SI 2019 definition" / "CODATA 2022"
}
```

`Units.Constants` is generated from it: symbols are used as is (`c`, `g_n`, `h`, `hbar`, `k_B`, `N_A`, `e`, `G`, `alpha`, `R`, `F`, `sigma_SB`, `m_e`, `m_p`, `m_n`, `a_0`, `R_inf`, `mu_0`, `epsilon_0`, `mu_B`, `M_u`, `m_u`, `T_tp`, `atm`, `G_0`, `K_J`, `R_K`, `Phi_0`, `K_cd`, `dnu_Cs`); an `exact = true` constant has no `uncertainty`; a constant is a **value**, not a unit, and cannot appear in the unit position of `quantity(v, u)`; `e` is the elementary charge and has the same name as `Math.e`, so when both are visible it MUST be qualified (`Math.e` / `Units.Constants.e`).

### 13.6.6 Threads and arena isolation

In the template, the `DimArena` is an `Arc<RwLock>` that `Graph::clone` **shares**; on a write, `with_dim_arena_mut`/`define_base_dimension` sees `Arc::strong_count > 1` and clones before writing (copy on write). Therefore: `Config::base_dimension(name)` is the safe path (`prepare` defines it on the cloned graph, so requests do not affect each other); requests that define no base dimension share the template arena and pay no copy; never call `define_base_dimension` on a `Session`'s template graph directly; and across threads still use `Session::fork`/`merge` and never share a `Session` (a `RefCell`).

---

## 13.7 The central package registry

The registry is a package index that is **content-addressed, signed and capability-declaring**. The service is implemented in `platform/` (`aevia-registry-*` and `aevia-pkg`) and deployed as `aevia.apich.org`.

**Status**: implemented — `aevia add owner/name[@req]`, `aevia update`, `aevia fetch`, `aevia tree` (all with `--offline`) and `aevia pkg publish/yank/search/verify`; dependency resolution is done by `aevia-resolve` (SemVer 2.0, backtracking solver, lock files with `checksum`, §7.5.1/§7.5.2); git dependencies and unknown manifest keys remain `E0605`. Known gaps are in appendix C, Revision 15.

### 13.7.1 Package manifest

```toml
[package]
name = "aevia-thermo"
version = "0.3.0"
edition = "2026"
license = "MIT"
content_hash = "blake3:..."

[deps]
aevia-std = "^1.0"

[compat]
aevia = ">=1.0, <2.0"
rssn_api = "1"

[capabilities]
requires = ["fs"]
provides = ["thermo"]

[operators]
exports = ["thermo/enthalpy", "thermo/entropy"]

[units]
exports = ["J", "kJ", "cal"]

[dimensions]
exports = ["Entropy"]
```

### 13.7.2 Protocol

Logical operations (HTTP paths follow `/api/v1` of `platform/`):

| Operation | Description |
|---|---|
| Read the index | all package names + latest versions + summaries; carries an `ETag` |
| Download package / signature | a content-addressed archive; the digest of the stored bytes is verified on download |
| Search | `aevia pkg search` |
| Publish (signed) | checks: no path-only dependencies, no self-dependency, every dependency exists and has a non-yanked satisfying version; pre-release versions allowed, build metadata rejected; publication runs in one transaction |
| Yank | deletes no content, only blocks new resolution |
| Resolve / dependents | server-side dependency resolution; lists the dependents of a package |

An index entry contains at least: name, version, content_hash, deps (`package`/`req`/optional `import`), aevia_range, rssn_api, capabilities, operators, units, dimensions, stability, yanked.

### 13.7.3 Reproducibility

The package content hash covers the source, the manifest and the signature of the generated `Rssn` module; builds run no arbitrary scripts; dependency resolution uses content hashes + semver; registry mirrors MUST be verifiable by signature.

---

## 13.8 Stability and capabilities (replacing v1/v2)

### 13.8.1 Stability markers

```aevia
@stable
@experimental
@deprecated("use foo2")
```

`Stable`: signature and semantics unchanged within a major; `Experimental`: may change, but the old name is kept for at least one minor; `Deprecated`: a warning period of at least one minor.

### 13.8.2 Capabilities

Backend abilities are described by capability names. **The only source of names is the capability table**: it is derived from the manifest's `Features`, and `Rssn.Capabilities` lists only rows with `enabled = true` (`Capabilities::table()` of `aevia-rssn`). The full table:

```
core.dynamic_call
core.jit                    # false: the default build has no jit feature
core.jit_branch             # false (as above)
core.jit_index              # false (as above)
core.parallel_symbolic
core.higher_order
core.structured_results
core.sde                    # derived from features.sde
core.callback               # same-thread scoped callbacks; concrete structured signatures
core.plugin.source          # source-level @op/@rule/load_plugin (needs in-process plugins)
core.plugin
core.plugin.in-process
core.plugin.cdylib          # plugin-cdylib builds only
dim.exact_units
dim.affine_units
dim.user_base_dimensions
```

The default build's `Rssn.Capabilities` actually lists twelve rows (all except the three `jit*` rows and `plugin.cdylib`). There are also names that are **declared by the specification but have no corresponding `Features` field in the manifest**; they can appear in diagnostics but are not listed by `Rssn.Capabilities`: `core.symbolic`, `core.numeric`, `core.tensor`, `core.linalg`, `core.ode`, `core.pde`, `core.fft`, `core.stats`, `core.units`, `core.assume.general`, `core.gpu`, `core.distributed`, `io.binary`.

`storage.sparse` (§13.9.5/§8.4.1) is Aevia's own storage class, is enabled, and is not published by the manifest's `Features`, so it does not appear in `Rssn.Capabilities`.

After type checking the Aevia compiler checks capabilities; when one is missing: statically known (such as a `@jit` target) → `E1001` at compile time; a dynamic call → `OpError.Unsupported` at run time, which Aevia turns into `E1001`. **No language feature is removed**: when the GPU is unimplemented, `on(Gpu(0), ...)` is still well-typed and reports `E1001("core.gpu")` at run time.

---

## 13.9 Decided extension designs

### 13.9.1 Shape refinement

**Decision: supported, as a refinement and not full dependent types.**

- `Tensor<n, τ>` carries only a rank; `Tensor<[n1, ..., nk], τ>` is a shape refinement where each `n_i` is a type-level `Nat`, with `?` for unknown; `Matrix<n, m, τ>` is an alias of `Tensor<[n, m], τ>`.
- Tensor literals get a refined shape automatically; `zeros_of<[3,4]>()` and similar can state one explicitly. `matmul : Tensor<[m,k],τ> × Tensor<[k,n],τ> -> Tensor<[m,n],τ>`; the rank-only version is kept and checked at run time.
- `reshape<[n1, ..., nk]>(a)` always returns a `Result`: when shapes are statically known the element count is checked at compile time and the result is always `Ok`, otherwise it is checked at run time; `reshape_dyn` always checks at run time.
- Indexing: a refined shape plus a literal index gives a compile-time bounds check; dynamic indices are still checked at run time.
- Refined → rank-only is a safe implicit conversion; rank-only → refined uses `refine_shape`, returning an `Option`/`Result`.
- Type-level `Nat` supports literals, `+`, `*` and linear equations, with no general recursion or proof, which suffices for matrix products, reshape and concatenation.

### 13.9.2 `Point` tensors

**Decision: `Tensor<shape, Point[d]>` is supported.** Element-wise `+`/`-` follow the point rules (`Point - Point -> Float[d]`, `Point + Float[d] -> Point`, `Point + Point` is `E0405`); `mean` accepts only `Float`/`Real`, and the mean of points uses `centroid : Tensor<1,Point[d]> -> Point[d]`; a temperature field can be a `Tensor<[nx,ny], Point[Θ]>` directly; storage is `Float` + `UnitId`, and the affine offset is handled only in element-wise operations.

### 13.9.3 Row-polymorphic `Table` schemas

**Decision: `Table<ρ>` uses a row-polymorphic static schema, and the dynamic `Table` is kept.** `ρ` is a row type (field names + types + dimensions); `read_csv<ρ>(path)` infers field types from header units and reports `DataError.Schema` when it cannot; `column(t, "time")` is a type-level field lookup; `select`/`join`/`rename` stay row-polymorphic, and new fields require a `lacks` constraint; dynamic CSV returns `Table<Dynamic>` and `to_table<ρ>` validates at run time (details in §8.11).

### 13.9.4 Devices, GPU, distributed

**Decision: a device is a run-time value, not a tensor type parameter.**

- `Device = Cpu | Gpu(Int) | Distributed(Cluster)`; the `Tensor` type is unchanged, and values carry `device` metadata.
- `to_device : (Tensor<n,τ>, Device) -> IO (Tensor<n,τ>)`; `on : (Device, () -> a) -> IO a` is scoped execution (allocation and transfer are IO).
- `@jit(target = Gpu(0))` names the JIT target; without `core.gpu` it reports `E1001`. Distributed tensors use the library type `Sharded<Mesh, Tensor<...>>`, with `Mesh` a run-time topology. This avoids a combinatorial explosion of types for devices and shards.

**Status**: the `Device` value and `to_device`/`on`/`device_of` are typed; without `core.gpu`, `to_device`/`on` report `E1001("core.gpu")`. `@jit(target = Cpu)` is equivalent to `@jit`; `@jit(target = Gpu(n))` and `Distributed(..)` are accepted by the language and report `E1001("core.gpu")`/`E1001("core.distributed")` at check time; any other argument is `E0210`. In attribute arguments a "constructor applied to constants" (`Gpu(0)`) counts as a compile-time constant (§2.2.7). `Gpu`/`Distributed` have no backend and are only constructible run-time values (the rssn-side device layer is in §10.6.5).

### 13.9.5 Sparse and layouts

**Decision: `Sparse<F, shape, τ>` is a core type constructor and `F` is a format type.**

- Formats: `Csr`, `Csc`, `Coo`, `Bcsr` (`Block` is a synonym); the set is currently closed, and user-defined formats wait until rssn opens the `Format` kind (plugins can currently register only operators and units).
- The `Layout` class (in the design): `nnz`, `to_dense`, `from_dense`, `matmul`, `transpose`; the current implementation is built-in functions, not an instantiable type class.
- `@` dispatches on layout (dense×sparse, sparse×dense, sparse×sparse); only explicit conversions exist between `Sparse` and `Tensor`. rssn's `TypeExpr::Sparse` has only a `rank`, so `Sparse<F,shape,τ>` is lowered at the boundary to rank + run-time shape; refined shapes are checked and propagated by Aevia itself.

**Status**: implemented and `storage.sparse` is enabled — the run-time `Value::Sparse`, the four layout dispatches, `Bcsr`, preconditioned `cg_with`, non-symmetric Krylov solvers, slicing and rank-N contraction; details, exactness and error codes are in §8.4.1. **Remaining gaps**: the format set is still closed; sparse values cannot enter the rssn term graph.

### 13.9.6 Effects and capabilities

**Decision: no user-defined effect handlers; `IO` stays a single type.** The extension points are operators, plugins and the registry, not a language-level effect system. An `IO` action may declare the **capabilities** it needs (`fs`, `net`, `env`, `rand`, `gpu`) — a run-time permission, not a type parameter; `with_cap(cap, thunk)` runs in a sandbox, and an IO action fails when the capability is missing. This avoids effect rows intruding on every standard-library signature. Use plugins and operators for DSLs and code generation, not macros.

**Status**: `with_cap(caps, thunk)` and `aevia run --caps` are implemented, with semantics in §9.14 (capability sets narrow per block, nesting takes the intersection, a missing one is `E0902`). They are a different thing from the backend capability table of §13.8.2: the latter describes what the backend can do, the former what the program is allowed to do.

### 13.9.7 Macros and syntax extension

**Decision: no user macros; the syntax is fixed.** Extension is by operators, plugins, the registry and `Rssn` dynamic calls; `macro_rules`, hygienic macros and syntax extensions are not introduced, to avoid forking the toolchain. If one is ever really needed, it MUST appear as a separate, optional pre-processing layer outside the core syntax; the specification reserves no syntax for it.

### 13.9.8 Angles and uncertainty

- **`Angle` is not a base dimension**: `rad` is dimensionless and `deg` is a scale; `sin`/`cos` take `Real[1]` and a `deg` literal folds to `pi/180`; no `Angle` base dimension is introduced, to avoid breaking formulas like `exp(i*θ)` and `sin(ωt)`.
- **`Measurement<d>` is a standard-library type, not a core type**: independent error propagation is in §8.14; correlated errors use the library type `Covariance<d,e>`; the core language guarantees only dimensions.

### 13.9.9 Other closed questions

See the decision table in §12.9 (`det`, `Real`/`Float` mixing, `Dyn` errors, nested `RateOf`, `Table` schemas, `@`/`*`, `%`, `let` recursion, top-level IO, the registry, export of dimension aliases).

---

## 13.10 Compatibility and migration

- **Language versions** use semver, with no "v1 language / v2 language": only a major may break; a minor adds only capabilities, operators and library; a patch only fixes bugs.
- **Manifest compatibility**: an incompatible `api_version` is +1; the Aevia compiler pins a range; a change of `manifest_hash` invalidates caches, but old signatures stay usable until the next major.
- **Operator deprecation**: `Deprecated` has a warning period of at least one minor; aliases are kept.
- **Package registry**: content hash + signature + capabilities; a yank deletes no content and only blocks new resolution.
- **Migration**: users are not forced to rewrite code; deprecated operators keep aliases and old signatures stay usable within the pinned `api_version`. There is currently no automatic migration command.

## 13.11 Error codes

| Code | Situation |
|---|---|
| `E1001` | backend capability missing (`core.gpu`, `core.jit_branch`, …) |
| `E1002` | ambiguous operator/overload |
| `E1003` | no matching operator/overload |
| `E1004` | incompatible rssn `api_version` |
| `E1005` | manifest/signature hash mismatch |
| `E1006` | plugin ABI/load failure; failure to trace an `@op`/`@rule` |

These errors belong to the **environment/backend** and are not type errors in the user's program; a diagnostic MUST give the capability name, the minimum rssn version and an actionable alternative.
