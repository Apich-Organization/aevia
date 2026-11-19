# RSSN graph, rules and numerical backends

RSSN associates symbolic variants of an expression in an equivalence graph. Numerical algorithms also exist as Rust kernels that a host can call without building a symbolic term.

## Session and reduction

A session owns its term arena, rules and units. Hash consing gives equal nodes the same identity within a store. Rewriting unions classes, updates analysis facts and rebuilds affected parent relationships. Extraction walks the classes reachable from the requested term and chooses a result under the active cost model. Shape, sort and dimension evidence travel with the operation where the graph has enough information to derive them.

`Session::compute` prepares a private computation graph from the installed template, runs the reduction, and imports the answer. `compute_all` prepares on the owning thread, executes jobs on independent graphs, and finishes in input order. Sharing immutable configuration across workers keeps mutable graph state local. Cache identity includes the request and result-relevant configuration. Registration changes invalidate cached results tied to the old environment.

## Scheduling rules and kernels

Rules are grouped into tiers. The scheduler finds applicable rewrite runs and relevant numerical kernels by operator. Consecutive rewrites can search a frozen graph in parallel, then apply matches in stable order. Incremental search uses change epochs to avoid rebuilding assignments after unrelated edits. A kernel that opts into two-phase work extracts plain data, runs compute work independently and commits results in node order.

Budget and cancellation are distinct. Budget limits the work of a request. A cancellation token allows a caller to stop supported in-progress work. A cancelled result is not a reusable completed computation. Numerics commonly use separate limits for steps, panels, tolerances or callback evaluations.

## Numerical execution

Typed kernels cover dense and sparse linear algebra, quadrature, ODE/SDE/PDE integration, statistics, FFT and simulation. Direct algorithms own their validation and result types. Their term rules and typed ABI adapters translate arguments and outcomes at the boundary.

Compiled evaluation can use a tape with scalar, batched, automatic-differentiation or interval execution. LIR provides typed registers, branches, calls and a fallback exit. Cranelift compiles eligible work behind the JIT feature. The optional device backend adds an explicit buffer and launch model. Numeric algorithms that need reproducibility should treat precision, reduction order and backend choice as part of the experiment.
