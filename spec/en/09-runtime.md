# 09 — Runtime

The runtime turns source into values, values into rssn terms and terms into results. Its goals
are **fast start-up, fast interaction, fast numeric hot spots and controllable symbolic
work**. This chapter defines the components, the data flow, the threading model and the
performance boundaries.

## 9.1 Components

| Crate | Responsibility |
|---|---|
| `aevia-dim` | dimension algebra, unit table, unit systems; no dependency on the parser or the runtime |
| `aevia-syntax` | lexing, layout, parsing, AST, diagnostic rendering |
| `aevia-types` | kinds, types, dimension unification, inference, type classes, exhaustiveness, `.aevi` persistence |
| `aevia-ir` | Core IR and lowering from the checked AST; data only, no dependency on `aevia-core` |
| `aevia-rssn` | the rssn bridge: `Session`, conversion between the two value models, Kernel Manifest, `Rssn` module generation, JIT pass-through |
| `aevia-std` / `aevia-stdlib-model` | the standard library (Aevia source plus Rust numeric kernels) / the source of truth for its generation |
| `aevia-core` | values, tensors, closures, IO, the interpreters, built-ins, the symbolic layer, the module loader (`driver`/`manifest`), the disk cache, parallelism, the optional JIT |
| `aevia-fmt` / `aevia-doc` | the formatter / `##` documentation and doctest extraction |
| `aevia-analysis` / `aevia-lsp` | editor-independent semantic queries / the LSP adapter (the only crate that knows JSON-RPC) |
| `aevia-resolve` | version requirements and dependency resolution (§7.5.1.2), a pure crate |
| `aevia-cli` | the `aevia` executable |

Dependency direction: `dim → syntax → rssn → types → ir → core`; `cli` and `lsp` depend on
everything above them. The dimension algebra in `types` is pure Rust and needs no rssn run
time; `core` uses the backend through `aevia-rssn`.

## 9.2 Compilation pipeline

```
source .aev → lexing + layout → token stream (with INDENT/DEDENT) → parsing → AST
  → name resolution (module paths, unit resolution) → type/dimension checking, class resolution, exhaustiveness → checked AST
  → lowering → Core IR (ANF) → interpretation / optional JIT → values
```

### 9.2.1 AST

The AST keeps source positions for the formatter and the LSP; desugaring (`|>`, `√`, `!`, `@`,
the `_` placeholder, `do`, record update, tensor literals, indexing) happens before lowering.
Unit literals are resolved during checking: name → unit table → scale, dimension and affine
flag.

### 9.2.2 Core IR

```
core ::= let x = core in core | lambda(params, core) | app(core, [core])
       | if(core, core, core) | match(core, [arm]) | prim(op, [core])
       | construct(tag, [core]) | project(core, field) | update(core, [(field, core)])
       | tensor_lit([core]) | index(core, [index]) | slice(core, [slice])
       | do([stmt]) | return(core) | literal(value) | var(x)
```

Core IR is in ANF: every intermediate result is bound to a name, which helps closure
conversion, the JIT and debugging. Pattern matching is kept, and the interpreter matches by
tag and field.

### 9.2.3 Type erasure and the trust boundary

Dimensions and types are erased in Core IR, but **run-time witnesses** are kept: the optional
display unit of a `Real`/`Float`, the `is_point` flag of a `Point`, the run-time dimension of a
`Dyn`, and the shape and storage type of a tensor. Erasure lets `mean<L>` and `mean<T>` share
code; witnesses are used only for display, `Dyn` and FFI checks.

**Trust boundary (normative).** The runtime executes the checker's proofs; it does not
re-prove them:

1. **A checked source call does not re-validate its arguments.** If the callee's activation
   evidence is *closed* (the signature and all Type/Rep/Dim/Nat/Row projections are
   variable-free), the runtime uses it directly, without traversing arguments or unifying the
   signature again. If it is not closed, the projections the checker recorded at that call site
   are substituted first; only if it is still open are the remaining binders solved from the
   arguments' run-time witnesses (for example a tensor length known only at run time). Data is
   used only to solve unknowns the static evidence left open, never to re-check a closed
   signature.
2. **A checked constructor application does not traverse its payload.** A saturated
   `Cons(x, acc)` is the primitive `Construct` in Core IR; nominal identity is fixed at
   lowering and the type witness is the checked type at that position (substituted by the
   activation evidence, memoised per position).
3. **Full validation at boundaries.** Where a value's origin is unknown it is still validated:
   arguments of an Aevia closure called directly by a host (an embedder, a kernel callback,
   the `Rssn` dynamic ABI), the error payload propagated by `?`, and results of
   `from_value` and IO decoding. A built-in such as `map` or `fold` calling a callback counts
   as a host call and takes the "not closed" path of rule 1.
4. **Literals are constants.** The value of a numeric or unit literal depends only on its
   position and the activation evidence and is memoised by (position, activation evidence,
   module); a unit expression is not re-resolved on each evaluation.

These rules change no program's result, only the amount of work: a 10^5-step simulation over a
dimensioned record dropped from 22.7 s to 0.9 s, and building and traversing a 2×10^4-element
list from over 100 s to 0.3 s (optimised build).

A checked closure keeps the `ModuleEvidence` of its check through lexical scope (expression
types, literal representations, dimension checks, RSSN call keys, dictionaries, constructor
references, the unit table); a later check of the same source does not rewrite an older
closure's evidence, and a checked closure from a different interpreter session (including REPL
`:reset`) is refused with `E0809`. An explicit generic selection carries the origin of the
actual declaration and its Type/Rep/Dim/Nat/Row projections in declaration order (phantom
parameters are kept); higher-order value flows that cannot be proved stay conservative, and
this is not a provenance analysis for arbitrary rank-N polymorphism. A native self tail call
first verifies that the global still points at the same canonical closure and compares the
five concrete projections one by one; if that fails it returns to the interpreter trampoline.

Dynamic-ABI containers: a checked `Tensor<1, Value>` keeps one-dimensional boxed elements and
is not stacked when elements happen to be tensors; `[to_value(a), to_value(b)]` always carries
two ABI arguments.

**Witness table.** Type-class dispatch reads only these witnesses; a type without a witness
cannot be used for quantified run-time dispatch (rejected at check time, §3.8.4/§7.4):

| Type shape | Run-time witness |
|---|---|
| `Int` / `Real` / `Float` / `Complex` scalar | `NumKind` (integer / exact / float / complex) |
| `Tensor<n, τ>` | rank + storage class (`Exact`/`Float`/`Complex`) |
| nominal type `T<...>` | `(defining module, name)` + recursive argument witnesses |
| record | canonical label set (sorted) |
| `Bool` / `String` / `()` / `Prop` | their own tags |
| `Point` / `Unit` value / function / `Dyn` | **no witness** |

- **Concrete head type**: the checker records the exact instance key (class + canonical head);
  Core IR carries the key and the interpreter runs that instance body directly.
- **Quantified head type**: run-time selection by witness is allowed only if the candidate
  instances' witnesses are pairwise disjoint; otherwise the check reports `E0311`/`E0310`.
  Witnesses are not total: `Tensor<1, Int>` and `Tensor<1, Real>` have the same storage class,
  so two such instances cannot coexist.
- A record witness is **structural**: record types with the same label set are
  indistinguishable and cannot each have an instance; nominal types are told apart by module
  and name.
- There is no run-time multiple dispatch and no per-type or per-dimension monomorphisation:
  `mean<L>` and `mean<T>` MUST be the same code (§4.4.4, §12.8.13). Representation
  specialisation is a per-operation, tag-guarded fast path.

## 9.3 Interpreter

### 9.3.1 Environments

A local environment is an immutable persistent frame (`Rc`); a closure captures its
environment frame; the global environment holds module exports, the Prelude, the unit table
and class-instance dictionaries. Recursive closures are resolved through scope bindings and use
`Rc` environments; there is **no** session-level cycle collector. The parent chain of an
exclusively owned Scope is released iteratively; reclaiming arbitrary closure cycles is not
promised.

### 9.3.2 Calls

- Arguments are evaluated left to right; a built-in is a Rust function pointer plus an arity,
  and a user function is a closure.
- A self tail call uses a trampoline (`app` in tail position returns `TailCall` and the main
  loop replaces the current frame), so the Rust stack does not grow.
- The bounds on non-tail recursion are in §6.9.3 (`E0805`).

IO dispatch uses an explicit work stack; a Core IR `do` is one shared slot frame plus a
sequential step table, and no closure is created per suffix. Flat statement sequences and IO
tail recursion in the last action do not accumulate dispatch continuations in proportion to
statement count. `Interp::max_io_steps` (default 100,000,000) bounds dispatch steps and
`max_io_depth` (library default 2048, relaxed to 10⁶ by the CLI) bounds pending continuations;
exceeding either is `E0805`, and each `run_io` resets the counters. The CLI's `--budget` sets
both the tail-call and the IO-step budget; it is not a time or memory limit.

The control-flow marker of `?` carries the real function or IO boundary the checker certified;
failure propagation rebuilds `Err`/`None` at the target's success type, and an ordinary
run-time error is never mistaken for a success at a function boundary. A deferred action keeps
the check evidence of its definition; running an old action after a session change or reset
reports `E0809`. Built-in abstract errors of the Prelude such as `ParseError` and `IOError`
still use a text payload; `?` can still propagate them, but they are safely refused when
round-tripped through an opaque `Value`; a same-named ADT written in source is not covered by
this compatibility.

### 9.3.3 Pattern matching

A constructor pattern compares the tag and matches the payload recursively; a record pattern
looks fields up by name and is open; a rank-1 tensor pattern matches by length and elements, and
`..rest` produces a `SharedSlice` (every storage representation can be shared, and consecutive
tail operations point at the same original tensor with no chain of nested views); a literal is
compared exactly; a guard is evaluated after matching and MUST be a concrete `Bool`.

The AST and IR paths share an explicit work stack and a staged binding table: bindings are
committed only after all structural checks succeed, and a failure does not modify an existing
scope. Constructor evidence is kept with the lexical module revision and validates the
declaration schema, ordinal, name and field count. Storage work of element-by-element tail
recursion over a tensor is linear. The static exhaustiveness matrix handles at most 8192
states and 128 levels of expansion; when the budget runs out it conservatively reports `E0309`
and suggests `_`.

### 9.3.4 Errors and the stack

Every Core IR node carries a source position. The error object:

```
EvalError {
    code: E08xx,
    message: String,
    span: Span,
    stack: Vec<Frame>,   # function name + call site
    notes: Vec<String>,
}
```

`panic` prints the stack; a `Result` error is not printed and is propagated by `?`. Stack and
tail-call budgets are in §6.9.3.

## 9.4 The symbolic bridge

### 9.4.1 Session and Term

rssn's `Session` owns a term graph, and `Term<'s>` is a handle with a lifetime; `Session` is
built on `RefCell` and is not `Sync`. Policy:

- **One `Session` per thread** (`thread_local!` or the worker pool); terms kept across calls are
  stored by `u32` id, and `TermId` carries a `session_id` to prevent cross-session misuse.
- `compute` clones the template graph, which is acceptable for heavy requests and unacceptable
  for every `+`; so arithmetic only builds terms (`Session::call_value`) and never calls
  `compute` (§6.3.3).

### 9.4.2 Symbol table

```
SymbolicCtx {
    session: Session,
    symbols: HashMap<SymbolKey, TermId>,   # user symbol -> term
    display: HashMap<TermId, String>,      # display names
    config_cache: HashMap<ConfigKey, Config>,
    term_cache: LruCache<TermHash, TermId>,
}
```

- `symbol("x")` produces an internally unique name `x#n`. Symbols on the static path are
  dimensionless; only FFI and export pass the full name to `Config::dimension`, and the
  dimension of a `Dyn<ρ>` is kept by the host.
- `Config` is cached by (rule set, assumptions, dimension mode, budget); `term_cache`
  de-duplicates by structural hash.

### 9.4.3 Request construction

| Aevia | rssn request |
|---|---|
| `diff(f)` | `diff(term, ξ)` |
| `antiderivative(f)` | `integral(term, ξ)` |
| `defint(f,a,b)` | `defint(term, ξ, a, b)` |
| `limit(f,a)` | `limit(term, ξ, a)` |
| `solve(f)` | `solve(term, ξ)` |
| `sum(f,a,b)` / `product` | `sum(term, κ, a, b)` / `product(term, κ, a, b)` |
| `series` | `series(term, ξ, a, n)` |
| `dsolve` | `dsolve(eq, y(t))` |
| `simplify` | `compute(term, Config::symbolic())` |
| `approx` | `compute(term, Config::numeric(tol).bind(...))` |

`ξ` and `κ` are fresh symbols; `a` and `b` are evaluated terms or numbers. `compute` returns an
`Answer`; `Answer.term` is the answer term and `Answer.dimension` is the dimension rssn
inferred.

### 9.4.4 Dimension validation

After each `compute`:

1. Static path: the term graph is dimensionless, the checker already knows the answer's
   dimension, and no run-time comparison is made.
2. Numeric path: `Answer.dimension` is authoritative (do **not** re-infer it from
   `Answer.term`: the term of a Numeric target is a dimensionless floating literal); if it is
   `None` (`DimensionMode::Off`) fall back to the static dimension.
3. If the optional diagnostic is on and `Answer.dimension` disagrees with the static
   expectation, report `E0809` and print the request, the answer and both dimensions.
4. Convert `Answer.term` back to an Aevia value.

For `Dyn<ρ>` no static dimension is compared; the host-held `DimTerm` is the run-time
dimension.

### 9.4.5 Numeric path

- `Float` scalar arithmetic goes straight to f64 without entering rssn; `sin`, `exp` and the
  like use Rust `libm` or rssn kernels.
- `approx` of a `Real` uses `Target::Numeric`; the result carries an error ball and Aevia takes
  the midpoint.
- `F64` tensor operations use ndarray/faer; `Exact`/`Boxed` use the element-wise scalar path.

## 9.5 Tensor runtime

- Storage: `Arc<TensorData>`; a slice is a view (offset + strides) and `reshape` is a view when
  contiguous and a copy otherwise. There are no writes: `update`/`map` produce new tensors,
  reusing storage in place when the reference is unique. Semantics are in §6.6.
- Element-wise `F64` loops are unrolled 4/8-way; `sum` uses pairwise/Kahan summation.
  `matmul`/`solve`/`eig`/`svd`/`qr`/`lu` call rssn's numeric kernels (faer); `fft` calls rustfft;
  `interp`/`spline` call rssn kernels.
- Shape checks run before each operation and errors name both shapes and the operation;
  refined shapes are checked at compile time, rank-only shapes at run time.
- Devices: a tensor carries `device` metadata; `to_device` and `on(device, ...)` transfer and
  scope execution. CPU is the default; GPU and distributed are run-time targets, not type
  parameters.
- Sparse: `Sparse<F,shape,τ>` shares `@` dispatch with dense tensors; the `Layout` class
  provides `nnz`, `to_dense`, `from_dense`, `matmul` and `transpose`.
- Plugins: `Rssn.register` (alias `register_plugin`) registers operators, rewrites and units into
  the current RSSN session; Aevia sees them through manifest reflection with no recompilation.

## 9.6 JIT

### 9.6.1 Triggers and gates

The manifest's `Features` are read first:

- `features.jit = false`: `@jit` is `E1001("core.jit")` and the function is only interpreted;
- `features.jit_branch = false`: a function containing `if`/`match` is not JIT-compiled;
- `features.jit_index = false`: a function containing tensor indexing is not JIT-compiled.

Past the gates the function must also: contain only `Float` scalars, arithmetic and elementary
functions (with `if`/indexing as the flags allow); be called more than a threshold (default
1000) times or carry an explicit `@jit`; and have fixed input and output types (no symbols,
no `Dyn`, no records or variants).

### 9.6.2 Procedure

1. Lower Core IR to rssn's LIR (`rssn::backend::lir`): a typed register machine with basic
   blocks, block parameters, direct (possibly recursive) calls, checked integer operations,
   bounds-checked slices and *bail* exits. Each activation (argument kinds + result kind +
   literal specialisation) produces two LIR functions: the entry form returns
   `(status, result, next arguments, remaining fuel)` and the value form returns only the
   result, for recursive calls in value position.
2. Only when `features.jit` is true is it compiled by Cranelift (`rssn::backend::lir_jit`).
3. Cache key: function identity + argument kinds + result kind + literal specialisation.
4. Before entering machine code, check that every self-call site still points at this
   activation and that the built-in names it uses still resolve to built-ins (not shadowed).
   On a bail the interpreter re-executes from the same arguments (the machine code is pure, so
   redoing is unobservable).

### 9.6.3 Semantics and limits

- Compilable: capture-free pure functions whose parameters and result are dimensionless
  `Float`, `Bool` or 64-bit `Int`; `if`/`match` (literals, wildcards, bindings, guards) really
  branch; self tail calls become jumps with fuel checks, handing the trampoline back to the
  interpreter when fuel runs out; self calls in value position become native recursive calls
  (depth limit 512, beyond which it bails).
- `Int` `+ - *` are overflow-checked; on overflow the code bails and the interpreter completes
  with arbitrary precision. Mixing `Int` and `Float` promotes as in §6.3.2.
- Elementary functions (`sqrt`, `cbrt`, `exp`, `ln`/`log`, `log2`, `log10`, trigonometric and
  hyperbolic functions and their inverses, `√`) are guarded: any case in which the interpreter
  would not simply return `f(x)` (domain errors, the extended-real special values of §6.3.4,
  `±oo`, `ln(0)`) bails. `sqrt` and `cbrt` use `powf(1/n)` like the interpreter and agree bit
  for bit.
- Symbolic values, records, variants, closures, tensors, exact numbers and units do not enter
  the JIT.
- JIT results MUST agree bit for bit with the interpreter; a case that cannot guarantee this
  MUST bail, and a disagreement is a bug.
- `map_floats` (the device seam) still lowers to a straight-line tape, because a GPU needs a
  program without control flow.

Optional build: `cargo build -p aevia-cli --features jit` enables RSSN's Cranelift backend
(Rust 1.97 or newer). It is off by default, and `@jit` is then `E1001`. Reference test:
`tests/jit_native.aev`.

### 9.6.4 Device targets

`@jit` stays a scalar, CPU, machine-code path. Data-parallel execution on `Cpu`, `CpuParallel`
and `Gpu(n)` is a separate mechanism (rssn `Device`, §10.6.5); a function that qualifies for
`@jit` also qualifies for the device batch path.

- `@jit(target = Cpu)` equals `@jit`; `target = Gpu(n)` selects the **batch** device path, and
  without `core.gpu` the check reports `E1001("core.gpu")`.
- The device path and the JIT share one lowering (Core IR → SSA) and one gate (§9.6.1); they
  differ only in backend: the JIT compiles a one-point function, the device launches one
  kernel over `rows` rows.
- `CpuParallel` agrees bit for bit with `Cpu` and the thread count changes no result (§9.7.2);
  `Gpu(n)` is `f32` and agrees only within rounding error, so it does not enter pure
  functions or bit-deterministic primitives.
- If the device is unavailable, the function cannot be lowered or the device fails, evaluation
  falls back to the interpreter row by row and the values are unchanged;
  `AEVIA_PARALLEL_TRACE=1` prints the reason.

## 9.7 Memory and concurrency

### 9.7.1 Memory

Tensors are `Arc` + copy-on-write, slices share, and storage is reused in place when the
reference is unique; closures are `Rc` (no cycle collector, §9.3.1); symbolic terms are rssn's
hash-consed graph and term ids do not cross sessions; a REPL session arena grows and `:reset`
releases it.

### 9.7.2 Parallel evaluation: shared-nothing replicas

- Pure functions may run in parallel; the result order of `par_map(f, xs)` is fixed and **the
  thread count changes no result**.
- **Shared-nothing replica design.** The interpreter is built on `Rc`, and converting the
  runtime to `Arc` would make every program pay for the few parallel ones. So each worker
  thread holds a **replica**: a host-installed `Replica` factory rebuilds the interpreter from
  the same source on that thread (once per thread, then reused). Threads exchange **portable
  values** (`Portable`): numbers with dimension and display unit, strings, tensors, records,
  variants, constructors and IR closures.
- **Closures cross threads as code identity plus captured data.** A lambda is "the function's
  source position plus its captured values"; the position is a **module identity** (logical
  source path + owning package) plus a byte offset (`FileId` is assigned per load and does not
  cross threads), and the replica resolves it back to the same function through the
  same-named module.
- **Guarantee: parallelism never changes a value.** When a value cannot cross threads (a
  symbolic term may hold a handle of the thread's session, a kernel handle, an IO action, a
  closure with activation-time generic evidence), when there is no replica factory, under
  `--jobs 1`, or when a worker fails, the primitive falls back to sequential evaluation, and the
  sequential recomputation gives the value or error the program would have given (with
  position and call stack). `AEVIA_PARALLEL_TRACE=1` prints the reason for each fallback.
- **Determinism.** Chunk boundaries depend only on the input length, not on the thread count.
  `par_reduce(f, zero, xs)` requires `f` to be associative and `zero` an identity; it folds
  fixed chunks of 1024 elements and merges the chunk results in order, so a floating-point
  reduction is **bit-identical** on any number of cores and in the sequential fallback.
- Primitives: `par_map`, `par_tabulate(n, f)`, `par_reduce`. `--jobs N` (or `AEVIA_JOBS`) sets
  the worker count, defaulting to the available parallelism; replicas do not nest parallelism.
  Each worker thread has one kernel `Session`.
- **Parallel numeric kernels.** `sparse @ dense` (including `Bcsr`) runs in fixed 256-row blocks,
  each output row accumulated sequentially by one work item, bit-equal to the sequential
  kernel; `sde_ensemble_diag/general` runs through `par_map` replicas, and the member seed
  `ensemble_seed(seed, i)` depends only on `(seed, i)`. Deliberately not parallel: the time
  steps of a single ODE/SDE path, Krylov iterations and `tabulate`. A value with a closure field
  (the `at` of `ode_solve`) cannot cross threads and falls back to sequential.

### 9.7.3 Concurrent tasks

```
spawn : (() -> IO a) -> IO (Task a)
await : Task a -> IO a
pure  : a -> IO a
```

- `spawn(f)` runs `f()` on a worker thread and returns a `Task`; `await(t)` waits and fetches
  the result.
- **A task's console output is buffered and written at `await`, in await order**, so program
  output is deterministic however tasks interleave; other effects such as files happen directly
  on the task's thread.
- If the thunk cannot cross threads the task runs on this thread at `spawn` (also buffered), so
  `spawn` is always available. A run-time error on a worker is reported at `await` (not
  re-run); a task result MUST be portable data, otherwise `await` reports `E0811`; a task can
  be awaited only once (the second `await` is `E0811`).
- `do` itself still runs sequentially; concurrency appears only explicitly through tasks.

## 9.8 REPL

```
$ aevia repl
Aevia — dimensions are types. Type `:help` for commands, `:quit` to exit.
> let g = 9.80665 m/s^2
> sqrt(2 * 20.0 m / g)
> :type sqrt
sqrt : forall t0, d0. Scalar<ρ0, d0> -> Scalar<ρ0, d0^(1/2)>
```

| Command | Effect |
|---|---|
| `:type e` | show the type |
| `:dims` | list the base dimensions (`Length`, `Mass`, `Time`, `Current`, `Temperature`, `Amount`, `Luminosity`) |
| `:units [d]` | list units, optionally for one dimension |
| `:ast e` / `:core e` | show the AST / Core IR |
| `:time e` | evaluate and report the time taken |
| `:latex e` | render the value as LaTeX |
| `:load f` | load declarations from a file |
| `:reset` | clear the session (starts a new session identity, §7.3) |
| `:quit` | exit |

Multi-line input (indented blocks) and Ctrl-D to exit are supported; declarations accumulate in
input order and a repeated definition shadows the old one. After re-checking the accumulated
source the REPL installs the complete check evidence, keeps a cursor over already executed
declarations and initialises only the new ones; constructors of a newly added type use the
exact witnesses of that check.

Status: cancelling the current evaluation with Ctrl-C is **not implemented** (the default
console handling terminates the process; cooperative cancellation needs an interrupt flag in the
evaluator).

## 9.9 CLI

```
aevia run [--example NAME] [file.aev] [-- args…]   run the package entry, a file or an example (program arguments follow `--`)
aevia check [--all] [file.aev…]                    type/dimension check without running
aevia watch [file] [--once | --runs N]             check repeatedly, re-checking only changed modules (§9.11.1)
aevia test [--filter PAT] [file.aev]               run `test` declarations
aevia test --doc [--filter PAT] [files…]           compile the aevia blocks of `##` docs (§9.12)
aevia repl                                         interactive session (§9.8)
aevia fmt [--check] [files…]                       format source, the package's src/ by default
aevia doc [--open] [--out DIR] [files…]            write Markdown API docs, default docs/
aevia bench [--filter PAT] [--iterations N] [--warmup N] [--save-baseline FILE]
            [--compare-baseline FILE] [--threshold RATIO] [files…]
aevia init <name>                                  create a package: aevia.toml, src/main.aev, aevia.lock
aevia add <path|owner/name[@req]> [--as NAME] [--offline]   add a path or registry dependency and re-resolve the lock
aevia update [owner/name…] [--offline]             re-resolve aevia.lock to the newest compatible versions
aevia fetch [--offline] [--force]                  download and verify the registry packages aevia.lock names
aevia tree [--offline]                             print the dependency tree
aevia clean [--registry]                           remove `.aevia/` (`.aevia/registry` stays unless asked)
aevia lsp [--stdio]                                language server (§9.10)
aevia rssn [--audit] [--emit [--out DIR] [--namespaces]]   audit a Kernel Manifest / generate the Rssn module (chapter 13)
aevia login                                        bind a signing key to your registry account (browser)
aevia publish [--no-verify] [--dry-run] [options…] check, format-check, sign and upload this package
aevia search <query>      aevia info <owner/name>  search the registry / show a package page
aevia yank <owner/name@version> [--undo]           withdraw or restore a version
aevia verify <owner/name@version>                  check a download against its signature
aevia docs [owner/name[@version]] [--open]         print (or open) the address of a package's documentation site
aevia pkg <command>                                every registry-client command (`key`, `status`, `lock`, …), forwarded to aevia-pkg
aevia --version
```

Global flags (allowed anywhere on the command line):

| Flag | Effect |
|---|---|
| `--max-stack MiB` | the interpreter thread's stack (default 1024) |
| `--budget STEPS` | tail-call/IO step budget before `E0805` (default 100,000,000; not seconds, §9.3.4) |
| `--caps LIST` | grant only these capabilities: `fs,net,env,rand,gpu` (empty list = sandbox everything; default all, §9.14) |
| `--jobs N` | worker threads for parallel evaluation (§9.7.2) |
| `--no-cache` | `check`/`run` neither read nor write the disk caches (§9.11) |
| `--out PATH` | output path (`rssn --emit`, `doc`) |
| `--color auto\|always\|never` | diagnostic colouring |
| `--locked` | refuse to rewrite an out-of-date `aevia.lock` (`update`/`add`/…) |
| `--release` | accepted but reported as having no effect (the interpreter has no optimisation profile; JIT: §9.6) |

- **Program arguments**: `aevia run [file] -- args…` hands only the strings **after** `--` to the
  program, which reads them with `args()` (§8.1.8, a `Tensor<1, String>`; `[]` when there are
  none). `check`, `test` and `bench` do not forward arguments.
- **Registry commands**: `login`, `search`, `info`, `yank`, `verify`, `key`, `status`, `lock` and
  `index` are forwarded to `aevia-pkg`; `add`, `update`, `fetch` and `tree` are native to the
  toolchain (they edit `aevia.toml` and `aevia.lock` and call the client only for the network
  part); `publish` is native too: it first runs `aevia check --all` and `aevia fmt --check` with
  this binary and, on failure, refuses before a key is touched (`--no-verify` skips this), then
  hands over to `aevia-pkg publish`. If `aevia-pkg` is not on `PATH`, point `AEVIA_PKG` at a
  built binary or run `cargo install --path platform/crates/aevia-pkg`.
- **Options of `aevia-pkg publish`**: `--key FINGERPRINT`, `--description TEXT`, `--license SPDX`,
  `--keyword K` (up to five), `--repository URL`, `--dry-run` (print the statement and the
  checksum, sign nothing) and `--allow-dirty`. There are no API tokens: every authenticated
  action is a detached signature, made with the publisher's own OpenPGP or SSH key, over a
  short-lived server-issued statement that names one archive, carries a single-use nonce and is
  valid for five minutes.
- `aevia docs` only prints the documentation address
  (`<registry>/docs[/<owner>/<name>[/<version>]]`; the registry comes from `--registry`,
  `AEVIA_REGISTRY` or the default `https://aevia.apich.org`); `--open` opens a browser when
  stdout is a terminal.

Status: the "packages" paragraph of `aevia --help` still says "there is no registry", which
contradicts registry dependencies and should be deleted; a prebuilt `aevia` binary older than
`publish`/`login`/`search`/`docs` does not list them in its help.

## 9.10 LSP

Supported capabilities:

- Diagnostics: type, dimension and unit errors, unused bindings, non-exhaustive matches.
- Hover: type, dimension, unit, documentation.
- Completion: module names, functions, units, constructors; **field completion** after `.` lists
  only the fields of the receiver's record type; **keyword completion** offers
  `do`/`let`/`match`/`with` only in expression position.
- Navigation: definition and type definition; `textDocument/references` lists every use of the
  same name in the loaded program (honouring `context.includeDeclaration`; a local binding stays
  within its file, while top-level declarations, imported and qualified names, types and
  constructors, and record fields declared in this document can span modules).
- Rename: `prepareRename` + `rename` return a `WorkspaceEdit` carrying document versions; they
  can rename local bindings, top-level declarations, imported names used in this program and
  record fields declared in this document, and refuse keywords, predefined names, unit names,
  imported names not edited by this document, and invalid or clashing new names (with an
  explicit error and never a partial edit).
- Signature help: inside call parentheses it gives the function name, the active parameter and
  the available overloads; trigger characters `(` and `,`, retrigger `,`. Limits: the active
  overload is chosen by arity; parameter labels show **types**, not parameter names;
  documentation comes only from the `##` above a declaration.
- Formatting: the same engine as `aevia fmt`. Code actions: add a type annotation, turn `degC`
  into `K`, extract a `let`, fix a dimension. Semantic highlighting: types, dimensions, units,
  operators. `workspace/symbol` lists the declarations of all modules of the loaded program.

The LSP reuses `aevia-syntax` and `aevia-types` and starts no runtime. For a saved file,
analysis loads its import graph through `aevia_core::driver` (the package root follows
`aevia.toml`), and cross-module navigation returns the defining module's file URI; unsaved
buffers keep the import graph too (one snapshot over all open physical buffers), so an
imported module not yet saved to disk still takes part in resolution; a virtual document
without a package root falls back to single-file analysis. A bounded LRU cache (default 4
programs) compares the actual source contents read, the existence and physical identity of
import candidates and the package-resolution context; it does not scan unrelated directories
and does not rely on `(len, mtime)`. When an imported buffer changes, diagnostics of the
affected open importers are republished. The exact capabilities `initialize` returns:

```json
{
  "positionEncoding": "utf-16",
  "textDocumentSync": { "openClose": true, "change": 2, "save": true },
  "hoverProvider": true,
  "completionProvider": { "resolveProvider": false },
  "definitionProvider": true,
  "typeDefinitionProvider": true,
  "documentSymbolProvider": true,
  "documentFormattingProvider": true,
  "codeActionProvider": true,
  "workspaceSymbolProvider": true,
  "referencesProvider": true,
  "renameProvider": { "prepareProvider": true },
  "signatureHelpProvider": {
    "triggerCharacters": ["(", ","],
    "retriggerCharacters": [","]
  },
  "semanticTokensProvider": {
    "legend": {
      "tokenTypes": ["type", "dimension", "unit", "operator", "function", "variable", "enumMember"],
      "tokenModifiers": []
    },
    "full": true,
    "range": false
  },
  "experimental": {
    "multiModuleAnalysis": {
      "savedFiles": true,
      "unsavedBuffers": "snapshot overlays for all open physical buffers"
    },
    "analysisCache": {
      "strategy": "observed source contents + import candidates + package resolution",
      "capacity": 4
    }
  }
}
```

`aevia-lsp/tests/protocol.rs` asserts this capability table (only implemented behaviour is
declared).

## 9.11 Build and package management

- The package graph is topologically sorted; the lock file makes builds reproducible (§7.5.2);
  build artefacts live in `.aevia/` and are not committed; the central registry is the default
  source (protocol: §13.7), and `add`/`publish`/`yank` use the same protocol.

### 9.11.1 In-process incremental checking

Checking a module is a function of its source text, `FileId`, the **checked interfaces** of the
modules it imports, its import bindings, its module identity and the check options.
`CheckCache` stores `CheckedModule` and diagnostics keyed by a structural fingerprint of these
inputs: a module whose inputs are all unchanged is not re-checked, and because the fingerprint
includes the imported checked interfaces, **an edit that does not change an interface does not
re-check its importers** (early cutoff). `FileId` is a position in the `SourceMap` and is the
same on every load while the import graph is unchanged; a cache entry is reused only under the
same `FileId`, so every span in a reused interface points into the text it was checked
against.

`aevia watch <file> [--once|--runs N]` checks repeatedly with one session cache and reports
"how many checked, how many reused" each round. The standard library and unchanged package
modules are checked once per session; while waiting it watches every input the load observed
(files read, and import candidates that did not exist then but would change resolution if they
appeared).

### 9.11.2 Cross-process whole-program cache

`aevia check <file>` records, in `.aevia/cache/` beside the entry, every input the load
observed (files read with their text hashes, import candidates that did not exist), the
compiler identity (the executable with its size and modification time), the rendering style and
the result. A later `check` whose inputs all still match replays the output and exit code
directly; any change (an edit, a new module that would be imported, a rebuilt compiler, a
different colour style) is a miss.

### 9.11.3 Per-module disk cache (`.aevi`)

One file per checked module, at
`.aevia/cache/modules/<module path>-<16 hex digits of the key>.aevi` beside the entry; standard
library modules are cached too, while the kernel namespace generated on the fly from the RSSN
manifest and check results containing errors are not. File layout: magic `AEVI`, format version
(`FORMAT_VERSION`, little-endian u32), a 128-bit key, the body length and an FNV-1a checksum,
and the body (`bincode` written by `persist::encode`: the entry's `FileId`, the interface
digest, diagnostics and the **complete** `CheckedModule`). Writes go to a temporary file and are
renamed; a missing, truncated, checksum-failing or key- or version-mismatched file is a silent
miss and is overwritten; at most 4 revisions per module name are kept.

- The **key** is a stable 128-bit hash (two-lane FNV-1a, not `DefaultHasher`) of: the module
  source, `FileId`, the module path and the nominal identity the loader gives (including package
  origin), import bindings, the **interface digest of every module in the dependency closure**,
  the check options (capability set) and the compiler identity (`aevia-core` version,
  `FORMAT_VERSION`, the RSSN manifest hash, the executable's size and modification time). In
  the interface digest every ADT is replaced by its symbolic definition, so it contains no
  `AdtId`. Hence an edit that leaves the interface unchanged re-checks only the edited module,
  an interface change re-checks all dependants, and a change of capability set, import bindings
  or compiler build is a miss.
- **`AdtId` does not enter the file (§7.3).** On encoding, each `AdtId` that occurs gets an
  in-file index and the ADT table records its full definition (`AdtKey` = module path + name +
  declaration schema, parameters, parameter kinds, constructors and field types, with cycles
  expressed by index); the reserved `Option`/`Result`/`Ordering` (0/1/2) are written as is. On
  decoding, the ADT table is first **re-interned** into this process's registry (a complete type
  with the same `AdtKey` reuses its `AdtId`, otherwise one is created; host and opaque types
  are `intern`ed by name) and only then is the body decoded.
- An entry is reused only under the same `FileId`, which is in both the key and the file.
  `LoadStats` gains `disk_hits` and `disk_writes`; the entry points are
  `driver::load_with_disk_cache` and `driver::load_incremental_with_disk_cache`
  (`DiskCache::beside(entry)`).

Status: known limits — each file carries the complete unit table and dimension environment and
is larger than the interface; hashing the interface digests of the dependency closure grows
linearly with closure size on large graphs; `watch`, the LSP and `test` do not use the disk
cache yet; old-revision cleanup is the heuristic "keep 4 per module name" with no global
capacity limit.

## 9.12 Tests and benchmarks

- Unit tests: `test` declarations, `aevia test [--filter PAT]`. Property tests: `forall` +
  `Arbitrary`.
- Doctests: `aevia test --doc [FILE…/DIRS…]` extracts every fenced block tagged `aevia` from
  `##` comments and compiles each. The first line of a block may be `compile_fail` (compilation
  must fail) or `compile_fail EXXXX` (it must fail and include that code); without a marker the
  block must compile. Each snippet is its own synthetic module: the declaring module's
  `import`/`open` lines are copied, imports resolve against the declaring file's package and
  adjacent directories, and the module's own declarations do not enter the snippet implicitly;
  snippets use in-memory source and write no temporary file; a failing dependency or inherited
  import does not satisfy an expected compile failure. `--locked` checks the package of each
  checked document, not the caller's working directory. Any failure exits 1; no checkable block,
  or an empty result after filtering, also exits 1. The extractor is `aevia-doc::doctest`.
- Compile-failure tests: `compile_fail("<source>") : String -> Bool` compiles a fragment in an
  isolated environment (no imports, the Prelude only) and is `true` when it yields at least one
  error; it does **not** check the error code, which the doctest marker `compile_fail EXXXX`
  asserts.
- Golden tests: snapshots of REPL output and error messages.
- Benchmarks: `bench` declarations; each reports the median, mean and 95% confidence interval of
  N iterations (default N=100; `--iterations`/`--warmup`). `--save-baseline FILE` writes JSON
  (`format=1`, `tool`/`version`, `profile=debug|release`, `iterations`, `warmup`, and per item
  `median_ns`/`mean_ns`/`ci_ns`); `--compare-baseline FILE --threshold RATIO` (default 0.05, may
  be written `5%`) compares medians and, above the threshold, reports `REGRESSION` and exits 1; a
  profile mismatch exits 2; a workload present on one side only is reported but not counted as
  a regression. Protocol: compare only release builds, on the same machine, median of N.

## 9.13 Performance targets

| Scenario | Target |
|---|---|
| start-up (empty program) | < 50 ms |
| REPL input to result (simple expression) | < 100 ms |
| `Float` tensor element-wise operations | same order as NumPy (≤ 2×) |
| `Float` matrix multiplication | same order as faer (≤ 1.2×) |
| simplifying a small symbolic expression | < 10 ms |
| heavy integration/solving | governed by the budget |
| JIT hot function | ≥ 5× the interpreter |

These are design targets, not specification guarantees; implementations should track them in
benchmarks.

## 9.14 Security and sandboxing

- There is no `unsafe` user code; file and network IO happen only through `IO` actions;
  capabilities (`fs`, `net`, `env`, `rand`, `gpu`) are checked by the runtime and are not type
  parameters (§13.9.6). The symbolic budget prevents unbounded simplification and the stack
  limit prevents unbounded recursion.
- A package build executes no arbitrary script and there is no `build.aev`; when code generation
  is needed use plugins or operators, not build scripts or macros.

```
with_cap : (String, () -> IO a) -> IO a
```

- `with_cap(caps, thunk)` runs the action `thunk()` builds while keeping only the capabilities
  listed in `caps` (comma-separated: `"fs"`, `"fs,rand"`; `""` grants nothing). Nested uses
  intersect, and the outer set is restored when the block ends, including by error.
- Actions that need a capability: file reads and writes, NPY, `plot`/`save` (`fs`), `random()`
  (`rand`). Without the capability a data action returning `Result` gives
  `Err("E0902: permission denied: …")` and any other action terminates with `E0902`; an unknown
  capability name is `E0302`.
- A program has every capability by default; `aevia run --caps fs,rand` starts with the given set
  and `--caps ""` sandboxes completely.

Status: `fs` and `rand` are actually checked; the standard library currently has no `net`, `env`
or `gpu` actions, so those names are accepted and take part in intersection. A missing backend
capability is `E1001` (§13.8). `[capabilities]` in the manifest is not read (§7.5.1). "A
configurable memory limit" exists today only as the interpreter thread's stack limit.

## 9.15 Error reporting

All diagnostics share the format of appendix B.1: code, message, primary location, label, notes
and help. A dimension error carries the canonical forms and the difference; a type error
carries the expected and actual types and, where possible, the "most likely fix"; a parse error
carries the expected token and the nearest legal repair.

The next chapter defines the concrete mapping between Aevia and rssn: which capabilities are
used directly, which need a wrapper and which are known gaps.
