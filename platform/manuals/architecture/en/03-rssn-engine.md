# RSSN graph, rules and numerical backends

RSSN associates symbolic variants of an expression in an equivalence graph. Numerical algorithms also exist as Rust kernels that a host can call without building a symbolic term.

## Session and reduction

A session owns its term arena, rules and units. Hash consing gives equal nodes the same identity within a store. Rewriting unions classes, updates analysis facts and rebuilds affected parent relationships. Extraction walks the classes reachable from the requested term and chooses a result under the active cost model. Shape, sort and dimension evidence travel with the operation where the graph has enough information to derive them.

Standard sessions share the installed rules and the immutable manifest for the current process-wide unit table. With a session-local unit, the modifying session copies its template and rebuilds its own manifest. Aevia indexes operator names and aliases into one array of canonical manifest entries.

`Session::compute` prepares a private computation graph from the installed template, runs the reduction, and imports the answer. `compute_all` snapshots its cache inputs at entry. Workers prepare and run requests on independent graphs in batches sized by the active Rayon pool. The owning thread imports answers in input order. Cache identity includes the request and result-relevant configuration. Registration changes invalidate cached results tied to the old environment.

## Scheduling rules and kernels

Rules are grouped into tiers. The scheduler finds applicable rewrite runs and relevant numerical kernels by operator. Consecutive rewrites can search a frozen graph in parallel, then apply matches in stable order. Incremental search uses change epochs to avoid rebuilding assignments after unrelated edits. A kernel that opts into two-phase work extracts plain data, runs compute work independently and commits results in node order.

Budget and cancellation are distinct. Budget limits the work of a request. A cancellation token allows a caller to stop supported in-progress work. A cancelled result is not a reusable completed computation. Numerics commonly use separate limits for steps, panels, tolerances or callback evaluations.

## Numerical execution

Typed kernels cover dense and sparse linear algebra, quadrature, ODE/SDE/PDE integration, statistics, FFT and simulation. Direct algorithms own their validation and result types. Their term rules and typed ABI adapters translate arguments and outcomes at the boundary.

Native handles retain objects in the RSSN session under a shared memory limit. Mutable objects reserve capacity for future operations. Immutable AMG hierarchies, CSR and BSR matrices, and prepared Schur solvers charge their own allocated storage under individual size limits.

Optional lists of native handles accept an empty `Tensor<1, Value>`. Every element of a nonempty list must be a live handle of the declared kind.

Prepared plans retain the data shared by a family of calls. A wave plan keeps its grid and phases, a Levin plan keeps its quadrature nodes and weights, and a residual estimator keeps its finite-element geometry. Each call can supply a new wavefunction, amplitude or state. The manifest declares the operator and its user-facing argument contract. The adapter checks the shape and dimensions of sampled vectors before converting them to coherent values, or uses the explicit references of a normalized model.

`Rssn_Numeric.coherence` computes one-sided Welch coherence. `Rssn_Numeric.wavelet_packet` decomposes both branches of the wavelet transform and returns an equal-width matrix of leaf bands. `Rssn_Numeric.inverse_wavelet_packet` reconstructs the input from those bands. The typed `num_signal_*` operators expose the analytic signal, Hilbert transform, amplitude envelope and zoom FFT through the native numerical ABI.

CPU parallelism uses the active Rayon pool and workload thresholds. The global pool detects available CPUs and respects `RAYON_NUM_THREADS`. Aevia language workers start from the same default count and accept `AEVIA_JOBS` or `--jobs` overrides. Aevia reuses its worker pool when its thread count covers the work and stays within the caller's limit. RSSN calls on those workers use the enclosing pool. `kernels::reuse` stores shared immutable plans and leases exclusive scratch buffers under retention limits. Concurrent callers share construction of the same cached plan. FFT routines key plans by length and direction and detect CPU instructions at run time. Compiled CPU batches borrow intermediate `f64` buffers from the same reuse module.

Large optimized dense products use the active pool with row-major views of the input and output buffers. Large Welch computations partition complete frames into fixed blocks and then combine their partial spectra in order.

Compiled evaluation can use a tape with scalar, batched, automatic-differentiation or interval execution. LIR provides typed registers, branches, calls and a fallback exit. Cranelift compiles eligible work behind the JIT feature. The optional device backend adds an explicit buffer and launch model. Numeric algorithms that need reproducibility should treat precision, reduction order and backend choice as part of the experiment.

Native builds enable Cranelift, wgpu and RSSN's NumPy integration by default.
A thread-scoped RSSN backend takes precedence over a process selection, which
in turn takes precedence over automatic selection. With automatic selection,
a supported host starts with the CPU tape and JIT-compiles hot code. The
interpreter serves other hosts. Explicit device selection and the language's
matrix offload policy have separate controls and check GPU capability at use.
[Choose and inspect a backend](/book/12-parallel-and-profiling?lang=en)

## The two identities in the arena

A `NodeId` identifies a concrete term. The structural hash table interns an operator, payload and ordered concrete children so repeated subtrees share storage. A `ClassId` identifies an equivalence class. Union-find finds its canonical representative, and a linked member ring permits class merges without rewriting all concrete nodes. The congruence table compares operators over canonical child classes. A use list records parents affected by a merge. Rebuilding those parents can identify more congruent nodes while preserving existing term handles.

```text
Term creation → structural intern → concrete NodeId
                                    ↓
Rewrite or kernel proves equality → union classes → dirty parent use lists
                                          ↑                   ↓
                                   congruent parents ← rebuild canonical children
                                          ↓
                              cost extraction → concrete result
```

Facts travel with classes. Numeric witnesses, sort, dimensional and structural facts provide evidence to rule guards and kernels. A rule guard is part of the identity's domain. Missing evidence can keep the request symbolic. A rejected numerical domain is reported through the typed call boundary rather than passed to an algorithm with invalid inputs.

## Why the scheduler has stages

The scheduler starts from requested roots and limits search to reachable classes. Reduce tiers handle operators such as differentiation before normalization. Tree windows perform destructive simplifying passes inside small expressions. Exploratory identities can grow the graph, so they run behind budgets and rule back-off. A goal can stop a run early when extraction has a closed form or numeric witness. Matching a frozen snapshot can use parallel workers. Merging matches, performing unions and repairing congruence take place on the graph's owning thread in stable order.

The concrete expression and the equivalence class have different lifetimes. Extraction compares candidate costs over reachable classes and returns a concrete representative. A fixed-point stop describes what this rule set and budget reached. It is not a claim that every possible equivalent form has been enumerated. For numerical requests, the report separates the phase's result, stopping condition and available error or diagnostic information.

## Dispatch among symbolic, native and compiled paths

| Boundary | State and use |
|---|---|
| `src/api/mod.rs` and `src/graph/schedule.rs` | Session configuration, duplicated computation graphs, rewrite budgets and results |
| `src/graph/store.rs`, `facts.rs`, `extract.rs` | Structural and congruence stores, analysis facts and representative selection |
| `src/rules` | Term definitions, guarded identities and typed bridges to numerical methods |
| `src/kernels` and `src/sim` | Direct algorithms with sizes, tolerances, domain checks and result types |
| `src/backend/tape`, `lir.rs`, `lir_jit.rs`, `device` | Scalar/batched tapes, register code, native entry points and launch buffers |

An integrator that needs adaptive time stepping owns its work arrays and termination criteria. A graph routine may call it through a typed adapter. Compiled scalar code takes a separate register-and-buffer path. Choosing that path can change supported precision or execution resources. The backend feature and capability predicates therefore belong in a reproducible numerical run. [Call and ownership contracts](04-abi-and-persistence.md)
