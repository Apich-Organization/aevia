# Numerical computing in Aevia

Run an example's executable assertions with:

```sh
cargo run -p aevia-cli -- test examples/dimensioned_numerics.aev
```

The examples call RSSN kernels through Aevia's generated modules. Physical
quantities are converted to coherent units before numerical work; input unit
relationships are checked, and result tensors and record fields retain their
physical dimensions.

| Example | What it demonstrates |
| --- | --- |
| `dimensioned_numerics.aev` | PCHIP, normalized polynomial fitting, OLS with velocity coefficients, Schur/matrix square roots, physical reverse AD, Romberg and scaled L-BFGS |
| `physical_functions.aev` | Generated SI physics contracts, sampled functional inner products/norms/projection and copied-input percentile |
| `option_numerics.aev` | Native typed Option bandwidths, statistical result fields and geometric intersections with units |
| `native_autodiff.aev` | Native Dual callbacks and scoped reverse tapes, physical derivatives, shared work budgets and reference-scale invariance |
| `native_backend.aev` | Session-owned interpreter compilation, mixed physical reference scales, measured scalar/batch calls and unit rejection |
| `native_algebra_models.aev` | Portable Float/exact matrices, normalized polynomial calculus, spline/Chebyshev methods, native factors and checked approximation callbacks |
| `native_kernel_families.aev` | Coding, finite fields, permutations, topology, exact integral homology, measured persistence, fractals and robust statistics |
| `native_execution.aev` | Mixed-unit tape values/gradients, physical device columns and geometrized relativity |
| `native_discrete_objects.aev` | Stateful random streams, exact groups/Lie models, measured graphics and complex enclosures |
| `native_linear_tensors.aev` | Explicit tensor-train and compressed linear algebra objects |
| `native_mechanics.aev` | Native finite-element materials, models and physical stress |
| `native_jacobian.aev` | Native Jacobian providers and checked structured callbacks |
| `native_ode_objects.aev` | Mixed position/velocity state, retained native trajectories and solver budgets |
| `native_particle_engines.aev` | Native MD geometry, SI particle energy, measured multigrid, GPE normalization and split-step wave norms |
| `native_flow_conservation.aev` | DG/FV and AMR conservation with physical component references |
| `native_continuum_models.aev` | BEM geometry, mixed-unit gas fluxes, hyperelastic stress scales, Poisson elements and quadrilateral stiffness |
| `portable_models.aev` | Opaque k-d-tree/B-spline adapters, measured nearest distances, independent spline coordinates/values, scaled RBF and clamped slopes |
| `ode_adaptive_geometric.aev` | Requested RK4, Verlet, Ruth4, implicit midpoint and Gauss–Legendre methods with error control, dense output and terminal events |
| `ode_adaptive_dae.aev` | Residual and semi-explicit DAEs with explicit dimensions and nondimensionalization scales |
| `ode_adaptive_dde.aev` | Delay equations, history and accepted-step interpolation |
| `pde_adaptive_heat.aev` | Explicit, backward Euler, Crank–Nicolson and ADI diffusion on 1D–3D grids |
| `pde_adaptive_wave_event.aev` | Separate displacement/velocity dimensions and terminal events |
| `pde_adaptive_advanced.aev` | FEM theta methods, FVM reconstructions, DG SSPRK3 and spectral ETDRK4 |
| `sde_adaptive_implicit.aev` | Adaptive implicit Brownian methods and reproducible paths |
| `sde_adaptive_jump.aev` | Adaptive jump diffusion with preserved arrival times and marks |
| `resampling.aev` | Bootstrap/jackknife callbacks with independent sample/statistic units, seeds and budgets |
| `sparse_spectral_dimensions.aev` | Lanczos, Arnoldi, shift-invert, AMG and physical eigenvalue/residual dimensions |
| `sde_adaptive_levy.aev` | Stable Lévy noise with an explicit reference-time convention |

Concrete numerical functions use generated `Rssn_numerical_vector`,
`Rssn_numerical_matrix`, `Rssn_numerical_stats` and `Rssn_numerical_special`
modules. Their exact source signatures are generated from the kernel manifest.
A typed numerical function returns its declared result directly; a numerical
failure becomes an interpreter error. Generic callback operations also expose
`*_typed` aliases returning `Result<..., OpError>`, preserving callback errors
and explicit work limits. Opaque or runtime-shape-dependent operations use the
checked `Value` interface and `from_value<T>` for explicit recovery.
Exact native zero-argument operations use `f()` and return `Result`; importing
their module does not run constructors or probe optional backends.

Polynomial coefficients for a physical coordinate generally have different
units at each degree. `num_polyfit_normalized` instead fits powers of
`x/coordinate_scale`, keeping every coefficient in ordinate units. Scaled AD
and nonlinear solvers use the same reference-scale convention. For mixed
coordinates, Hessian entry `(i, j)` has output/(input_i input_j) dimensions.
A multivariate log density additionally needs an explicit reference measure; use
`num_multivariate_normal_log_pdf_scaled`. The physical density itself carries
coordinate units to the negative vector dimension.

Portable opaque models store versioned defining data, not pointers. Query
adapters validate the data and reconstruct the native model; repeated queries
currently incur that reconstruction cost. This supports safe portable values
without claiming a persistent cached tree.

Native frame APIs make normalized coefficients explicit. `num_grid_frame_new`
takes positive Length, Time and field references; `num_grid_mechanical_references`
takes Length, Time, Mass and Temperature references. Plain `Float` coefficients
and arrays documented as normalized use those reference scales and reject
measured inputs. Results such as `{ normalized, reference }` represent their
product; the stored normalized array itself is dimensionless. Mixed physical
components use separate references rather than one tensor element dimension.
Reduced acoustics use density Mass/Length^d and pressure Energy/Length^d;
2D hyperelastic energies are per unit out-of-plane thickness.

Native frames, backends, solvers and reverse-AD objects use bounded session-owned
handles.
Their kind and session are checked on every call; successful release invalidates
every copy. A parent release is rejected while dependent handles remain alive;
release dependent variables and gradients before a tape. Independently owned
compiled snapshots retain and account for their code separately. Temporary
reverse-callback handles expire when the callback finishes. Compiled backend
inputs use one positive reference per coordinate, allowing independent physical
dimensions without putting mixed quantities in a numeric tensor.

Standard `ode_solve` supports heterogeneous state records and tuples.
`OdePhysicalOpts<s>` accepts an absolute-tolerance value with the same shape
and physical leaf dimensions as the state; each component is normalized by its
own positive tolerance before entering the kernel. The native tape/trajectory interfaces also preserve independent component
references through checked `Value` cells; their initial conditions, rate units,
parameters and result leaves are checked separately. Homogeneous numeric
vectors keep one element dimension.

Adaptive methods expose tolerances, minimum/maximum steps and finite work
budgets. Tightening tolerance controls temporal error on the chosen spatial
mesh; it does not eliminate spatial discretization error. Adaptive geometric
methods retain their requested formula, but changing their step size generally
loses fixed-step symplecticity. Stochastic rejection reuses a bounded, coupled
noise discretization; exhausting its finest resolution returns an error rather
than accepting an inaccurate step or drawing replacement noise. Lévy methods
only offer Brownian higher-order corrections when alpha equals two.

The source API inventory and callable-manifest report are separate:
`tools/rssn-api-audit/inventory.tsv` records public Rust APIs, declared adapters
and resolved aliases;
`tools/rssn-api-audit/RSSN-COVERAGE.md` records language reachability. A Rust
helper is not counted as callable merely because it appears in the inventory.
Regenerate contracts, adapters and inventories with `tools/sync-kernel.sh`.

Native generic and opaque interfaces use explicit specializations: Float/exact
matrices, constant dimensions, sampled initializers, finite provider recipes,
owned compiled functions, and authenticated session objects. The manifest
describes each specialization and its finite shape/work bounds; a Rust generic
signature does not imply support for arbitrary Aevia types or dimensions.
Mutable slice/storage interfaces return or update checked owning values; raw
pointers and borrowed Rust layouts are not language values. JIT/GPU operations
report `Unsupported` when their RSSN feature is disabled. Ordinary examples
use default features. Optional backend and NPY file adapters can be compiled
from the normal CLI with:

```sh
cargo run -p aevia-cli --features jit,gpu-wgpu,npy -- test examples/native_execution.aev
```

`npy` enables NPY file support. Backend flags enable the corresponding adapters;
actual backend and GPU device availability are checked separately at runtime.

`object_binding!` derives schemas, codecs, arity and registration from typed
adapter declarations. Source-derived scalar/record bindings use reviewed unit
policies. `cargo run -p rssn-api-audit -- --coverage-check` independently checks
that every inventoried callable has an adapter; a re-export inherits coverage
only when Rust source resolution proves it is the same public item. Macro and
re-export fingerprints remain separately tracked review evidence.
