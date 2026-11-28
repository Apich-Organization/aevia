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
| `native_backend.aev` | Session-owned interpreter compilation, mixed physical reference scales, measured columns and unit rejection |
| `native_algebra_models.aev` | Portable Float/exact matrices, normalized polynomial calculus, spline/Chebyshev methods, native factors and checked approximation callbacks |
| `native_kernel_families.aev` | Coding, finite fields, permutations, topology, exact integral homology, measured persistence, fractals and robust statistics |
| `native_execution.aev` | Mixed-unit tape values/gradients, physical device columns and geometrized relativity |
| `native_discrete_objects.aev` | Stateful random streams, exact groups/Lie models, measured graphics and complex enclosures |
| `native_linear_tensors.aev` | Explicit tensor-train and compressed linear algebra objects |
| `native_mechanics.aev` | Native finite-element materials, models and physical stress |
| `native_jacobian.aev` | Native Jacobian providers and checked structured callbacks |
| `native_ode_objects.aev` | Mixed position/velocity state, retained native trajectories and solver budgets |
| `native_particle_engines.aev` | Native MD geometry, SI particle energy, measured multigrid, GPE normalization and split-step wave norms |
| `native_flow_conservation.aev` | DG/FV conservation and prepared exact gas Riemann sampling with physical component references |
| `native_continuum_models.aev` | BEM geometry, periodic Helmholtz batches, Ricci curvature, mixed-unit gas fluxes, hyperelastic stresses and finite elements |
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

`Rssn_numerical_vector` exposes `num_signal_hilbert`,
`num_signal_analytic_signal`, `num_signal_analytic_envelope`, and
`num_signal_zoom_fft`. The same module provides biased autocovariance
(`num_signal_autocovariance`), autocorrelation (`num_signal_autocorrelation`),
and Yule–Walker partial autocorrelation (`num_signal_partial_autocorrelation`).
Each takes a sample tensor and returns the lags from zero through the given
maximum. For samples measured in metres, autocovariances are in square metres
and both correlations are dimensionless.

`aevia rssn --describe num_signal_autocovariance` displays the documentation,
overloads, and Aevia binding names from the running kernel.

`Rssn_numerical_vector.num_signal_filtfilt(b, a, samples)` applies a
dimensionless filter forward and backward with odd-reflected edges. It
preserves the length and units of `samples`; invalid coefficients or an
undefined steady state produce a domain error.
[Pressure example](zero_phase_filter.aev)

Use `Rssn_numerical_vector.num_cauchy_sharp_root_bound` for a complex
coefficient vector in ascending order. For strictly positive real
coefficients, `num_enestrom_kakeya_bounds` returns a record with `lower`
and `upper` radii. The coordinate is dimensionless, so any common
coefficient units cancel. `Rssn_numerical_special` provides the three
Koebe bounds for the unit disk.
[Root bounds example](polynomial_root_bounds.aev)

Use `Rssn_numerical_special.num_mp_real_round` to create a dimensionless
`{mantissa, exponent, precision}` record with exact integer fields.
Compare records exactly using `num_mp_real_compare`. Addition,
subtraction and multiplication each take a result precision in bits
and a rounding mode, such as `"nearest"` (ties to even) or `"up"`.
[Exact dyadic example](dyadic_real.aev)

Use `Rssn_FiniteField.poly_mul_ntt_mod(a, b, p)` for exact coefficient
convolution modulo an odd prime. Its transform length is the next power
of two of the product length and must divide `p - 1`.
`Rssn_NumberTheory.extended_gcd_u64` returns named Bézout coefficients.
[Modular polynomial example](modular_polynomials.aev)

`num_timeseries_kalman_filter` and `num_timeseries_kalman_smooth` accept
linear state-space models whose states share one dimension and whose
observations share another. The filter returns state means, covariance
matrices and log likelihood. Its final argument sets the positive
observation reference scale for the log likelihood. The smoother returns
one state mean per observation. The same module provides
`num_timeseries_integrate` to undo repeated first differences.
Use `num_timeseries_rolling_moments(samples, window, ddof)` for complete
windows of a measured series. It returns `mean` in the sample units and
`variance` in squared sample units. Pass `ddof=0` for population variance
or `ddof=1` for sample variance with a window of at least two samples.
[Measured tracking example](kalman_tracking.aev)

Use `Rssn_numerical_matrix.num_control_c2d_zoh(a, b, c, d, dt)`
to discretise a linear state-space model with a measured sample period.
Pass the returned record directly to
`num_control_simulate_discrete(model, x0, inputs)`. For a nonuniform
time grid, use `num_control_simulate_continuous(a, b, c, d, x0, inputs, times)`.
The output starts with the observation of the initial state, including
the direct input term. [Measured simulation example](measured_state_space.aev)

`num_native_irfft_plan_new` prepares an inverse real FFT for a fixed real
output length. Reuse the plan for successive spectra with
`num_native_irfft_plan_apply`, which preserves the spectrum's physical
dimension. `num_native_irfft_plan_output_len` reports the real sample
count, and `num_native_irfft_plan_spectrum_len` reports the required number
of nonnegative frequency bins. For example, both four and five real
samples require three bins. [Runnable example](native_kernel_families.aev)

`num_stats_simple_linear_regression` accepts predictor and response tensors
with independent dimensions. The slope has response units divided by
predictor units, and the intercept has response units. The kernel can rescale
extreme data when the unscaled moments overflow or underflow.
[Runnable example](native_kernel_families.aev)

`num_polynomial_model_try_long_division` divides descending-coefficient
polynomials with the same coordinate reference. It allows leading zero
coefficients in the divisor and reports a zero divisor as an error.
Quotient coefficients have dividend units divided by divisor units, and
the remainder coefficients retain dividend units.
[Runnable example](native_algebra_models.aev)

`num_native_categorical_new` prepares finite nonnegative relative weights.
Reuse the table with one session random stream or several. The
`num_native_categorical_categories` count includes zero-weight entries, and
`num_native_categorical_sample` returns a zero-based index in the original order.
`num_native_categorical_fill` draws a tensor in the same order as successive
scalar calls. [Runnable example](native_discrete_objects.aev)

`num_grid_simplex_positive_fraction` returns the fraction of a segment,
triangle or tetrahedron where a linear level set is positive. Supply two,
three or four dimensionless vertex values. The edge ratios preserve
representable thin fractions and handle opposite finite values whose
difference exceeds the Float range. [Runnable example](native_continuum_models.aev)

`num_analysis_homotopy_system_scaled` takes normalized coefficients,
exponent rows, equation offsets, and physical references for roots and
residuals. Paths are returned in start order. A finite-path flag and
offsets identify the paths with root coordinates. The numerical interface
accepts at most six variables, 4,096 terms, degree 16 per equation, and
256 paths. [Runnable example](native_kernel_families.aev)

`num_analysis_wavelets_dwt_plan_new` prepares single-level analysis and
synthesis at fixed length, wavelet and boundary mode. `num_analysis_wavelets_dwt_plan_analyze`
returns measured approximation and detail bands. `num_analysis_wavelets_dwt_plan_synthesize`
requires bands of equal physical dimension and restores the original signal
length. `num_analysis_wavelets_dwt_plan_signal_len` and
`num_analysis_wavelets_dwt_plan_coeff_len` report the required lengths.
[Runnable example](native_kernel_families.aev)

The native graph functions include Bellman–Ford, DAG shortest paths, maximum
flow, multi-source BFS, strong-component condensation, and undirected bridges
and articulation points. The generated `Rssn_Numeric` module includes Welch
coherence and forward/inverse wavelet packets.

`num_native_ordering_metrics` takes CSR adjacency offsets and neighbour
indices along with a zero-based permutation that maps new positions to old
vertices. It returns integer `half_bandwidth` and `profile` fields. Each edge
is treated as undirected, and repeated entries leave both metrics unchanged.

`num_descriptive_histogram_edges` counts observations into user-supplied
bins. Edges must be finite, strictly increasing, and measured in the same
dimension as the observations. An observation on an interior edge goes in
the bin to its right. The last bin includes the highest edge.

`num_matfun_powm_psd` takes a dimensionless symmetric positive
semidefinite matrix and a finite real exponent. Positive exponents preserve
zero eigenvalues. Negative exponents require positive definiteness. The
zeroth power is the identity on admissible inputs.

`num_kdtree_nearest_batch` takes a portable k-d tree and a matrix with
one query per row. The result holds zero-based indices in query order and
distances in the model's coordinate dimension. The tree is reconstructed once
for the batch. [Runnable example](portable_models.aev)

`num_native_real_roots_find_roots_in` searches a finite closed interval for
the real roots of a normalized polynomial. Its bounds, absolute tolerance and
positive coordinate reference share the same dimension. The sorted result
preserves that dimension and includes roots at either endpoint.
[Runnable example](native_discrete_objects.aev)

`num_finite_field_gf256_scale` multiplies a byte vector by one coefficient.
`num_finite_field_gf256_mul_add` adds the scaled source to an equal-length
destination by XOR. Both use the same GF(256) polynomial as Reed–Solomon.
[Runnable example](native_kernel_families.aev)

`num_analysis_convergence_levin_condition_numbers` reports one dimensionless
value per available order of the Levin transform. Terms can share a physical
dimension. Use the exact variant names `U`, `T`, or `V`. An infinite value
marks an unavailable or unbounded condition number.

`num_analysis_ode_fixed_final` evaluates a retained native RHS tape on a
fixed grid and returns the final state with each component's physical units.
Choose method `0` for Euler, `1` for Heun or `2` for classical RK4. The
evaluation budget must cover the number of steps times 1, 2 or 4, respectively.
[Runnable example](native_ode_objects.aev)

`num_analysis_ode_etdrk4_diag` takes one inverse-time linear rate per
component and a retained tape for the nonlinear rate. Its real exponential
weights preserve representable steady states on highly dissipative steps.
[Runnable example](native_ode_objects.aev)

`num_native_geodesic_model_equatorial_timelike` prepares a future-directed
equatorial Kerr–Newman orbit. The physical frame uses geometric lengths for
mass, spin and charge. `num_native_geodesic_model_run_kerr_newman_geodesic_sampled`
keeps coordinates at the requested positive step stride, including the initial
and final reached points.
[Runnable example](native_execution.aev)

`num_native_ising_measure_seeded` reports energy, absolute magnetization
and response per site after thermalization. Specify the sampling stride
and exact seed. A physical frame takes the temperature in kelvin. The
output includes the final spins and number of complete measurement
intervals. Binder is optional when all magnetizations vanish.
[Runnable example](native_particle_engines.aev)

`num_native_spin_potts_heat_bath_probabilities_d1`,
`num_native_spin_potts_heat_bath_probabilities_d2`, and
`num_native_spin_potts_heat_bath_probabilities_d3` read the conditional color
probabilities at a zero-based site. The result is dimensionless and ordered
by color. A probability query leaves the spins and random stream intact.
[Runnable example](native_particle_engines.aev)

`num_native_fmm_prepare2d` fixes positions and open-boundary FMM options.
`num_native_fmm_plan_count2d` returns the required number of strengths.
`num_native_fmm_evaluate2d` accepts new strengths and their positive reference.
The field has one row per input position and units of source per length.
`num_analysis_nbody_chain_accelerations` reads three measured acceleration
components per body in its original input order.
[Runnable examples](native_particle_engines.aev)

`num_native_engine_neighbours_build_d3` indexes populated cells for sparse
periodic boxes. `num_native_engine_compute_forces_d3` uses the same neighbour
search and retains the physical domains of the force and energy results.
[Runnable example](native_particle_engines.aev)

`num_native_gpe_run_ground_state` returns the probability density in inverse
square length on the specified positive grid. A zero step count or zero
imaginary-time increment returns the normalized initial density.
[Runnable example](native_particle_engines.aev)

`num_native_quantum_schrodinger_plan_new` prepares a fixed physical potential
on a periodic two-dimensional grid. Give lengths, signed time increment,
reduced Planck constant, mass, and the potential in their SI dimensions.
`num_native_quantum_schrodinger_plan_evolve` reuses those phases and returns
the signed time and final inverse-length complex amplitudes in x-fast order.
Zero steps or zero time increment preserve the starting wavefunction.
[Runnable example](native_particle_engines.aev)

`num_grid_quad_element` computes an 8 by 8 Q4 plane-stress stiffness matrix
from counterclockwise vertices and a mechanical reference. A common
translation of the vertices does not change the element. The entry
reference has force per displacement units.
[Runnable example](native_continuum_models.aev)

`num_grid_hyper_prepare_point` holds the constitutive data for a fixed
dimensionless deformation gradient. `num_grid_hyper_point_piola`,
`num_grid_hyper_point_tangent` and `num_grid_hyper_point_energy_density`
return pressure-valued stress, stiffness and energy per reference volume.
Use `num_grid_hyper_point_tangent_action` for a stress increment in a given
dimensionless gradient direction. [Runnable example](native_continuum_models.aev)

`num_backend_call_batch_columns` accepts one measured rank-one tensor per
compiled input, in declared symbol order, along with the row count. Each
column keeps its own unit. The compiled program returns a single ordered
measured vector, using the active CPU pool when the work is large enough.
[Runnable example](native_backend.aev)

`num_native_sse_new` prepares a quantum XXZ spin sampler with a seed, energy
reference and inverse temperature in inverse energy units. `num_native_sse_sweep`
advances one sweep with a work budget, and `num_native_sse_sample` reads the
current expansion order and the total and staggered magnetization. Budget exhaustion during a sweep
invalidates its handle. The same seed reproduces trajectories for this version.
[Runnable example](native_particle_engines.aev)

`num_native_mm_compute_density_and_forces` recomputes density, pressure and
forces in the legacy SPH reduced units. `num_native_mm_state` returns the
current particle rows in input order. Position, velocity and force each have
two components. [Runnable example](native_particle_engines.aev)

`num_native_md_morse_radial` takes a reference frame, a separation, an energy
depth, an inverse length and an equilibrium separation. It returns `energy`
and `radial_force` with their physical dimensions. Energy is zero at
equilibrium. A positive force points toward increasing separation.
[Runnable example](native_particle_engines.aev)

Use `num_native_engine_rdf_merge_d1`, `num_native_engine_rdf_merge_d2` or
`num_native_engine_rdf_merge_d3` to combine independently accumulated radial
distribution frames. Both accumulators use the same bins, cutoff and
reference frame. Each frame contributes its own ideal-gas reference based on
its particle count and volume. The result has measured radii and
dimensionless density ratios. [Runnable example](native_particle_engines.aev)

`num_analysis_pde_method_of_lines_1d_timed` advances a one-dimensional field
using `Radau5` or `Bdf1` through `Bdf5`. Supply the two endpoint kinds and
compiled programs in left-to-right order. Each endpoint program takes a
measured time. A `Dirichlet` program returns the field value and a `Neumann`
program returns the outward derivative in field units per length. Use
`num_analysis_pde_field_data` to read the measured field and final time.
[Runnable example](native_continuum_models.aev)

`num_native_epd_goursat_samples` solves the Euler–Poisson–Darboux
Goursat problem from two sampled characteristic traces. Provide strictly
increasing coordinates for each trace and target rows `[rho, sigma]`. Both
traces must agree at their shared corner and use the same field dimension.
The result keeps that dimension and the original target order.
[Runnable example](native_epd_goursat.aev)

`num_grid_solve_helmholtz_multigrid` solves a structured Helmholtz problem
with a compiled spatial source in field units per squared length. Choose `V`,
`W` or `Fmg` in the controls. `num_grid_multigrid_result` gives the normalized
field, its physical reference and the relative residual history. Multiply the
field data by `value_reference` to restore the physical values.
[Runnable example](native_continuum_models.aev)

`num_fv_config_euler` and `num_fv_config_shallow_water` configure finite-volume
fluxes. Set the `riemann` option to `"Hlle"` for a two-wave HLLE flux with
Einfeldt wave-speed estimates. The options record also chooses the spatial
reconstruction and CFL number. [Runnable example](native_flow_conservation.aev)

`num_dg_advection_prepare1d` prepares the Legendre–Gauss–Lobatto weights for
one periodic mesh and measured velocity. `num_dg_advection_step_prepared1d`
accepts a measured nodal field and a time increment within the CFL bound. It
returns the advanced field in element order with the same dimension.
[Runnable example](native_flow_conservation.aev)

`num_grid_gas_prepare_riemann` fixes two normalized ideal-gas states
`[density, velocity, pressure]` in a mechanical reference frame.
`num_grid_gas_sample_riemann` and `num_grid_gas_sample_riemann_batch` accept
measured self-similar velocities `x/t` and return density, velocity and pressure
with their individual dimensions. Samples inside a vacuum have zero density
and pressure with velocity equal to `x/t`. [Runnable example](native_flow_conservation.aev)

`num_triangle_prepare` retains triangular geometry and quadrature for a fixed
P1 or P2 degree. `num_triangle_solve_prepared` accepts a diffusion–reaction
problem with the same physical references and returns a triangular solution.
Each call evaluates that problem's coefficients, source and boundary values.
`num_triangle_prepared_dofs` includes both free and prescribed nodes.
[Runnable example](native_continuum_models.aev)

`num_grid_fem_cartesian_prepare` retains the Q1 stiffness spectrum for a
uniform unit square or cube with zero boundary values. The physical side
length is the frame's length reference. `num_grid_fem_cartesian_solve_prepared`
evaluates a source in field per length squared once at each quadrature point.
`num_grid_fem_cartesian_nodal_shape` lists extents including the boundary;
the returned nodal data vary fastest along x. [Runnable example](native_continuum_models.aev)

`num_grid_bem_prepare3` prepares a dense interior Laplace solve for a surface
with fixed per-panel boundary kinds. `num_grid_bem_plan_panel_count` reports
the required length for `num_grid_bem_solve3_prepared`. Each coefficient uses
the potential reference or the flux reference (potential per length) for
its panel. `num_grid_bem_eval3` returns the measured interior potential.
For a closed body in an exterior domain, `num_grid_bem_exterior` solves the
decaying Laplace problem and `num_grid_bem_eval3_exterior` evaluates its
measured potential. The exterior flux follows the normal pointing out of the
body. Each evaluator checks the solution's domain.
[Runnable example](native_continuum_models.aev)

`num_analysis_symplectic_integrate_sampled` advances every step of a separable
system and saves the initial state, each cadence-aligned state, and the final
state. It returns step indices and independently measured positions and velocities.
[Runnable example](native_ode_objects.aev)

`num_analysis_physics_explicit_euler` accepts `Forward`, `Midpoint` or `Heun`
and an explicit signed time increment. It shortens the final step to the
requested time and restores each state component in its physical dimension.
Forward uses one tape evaluation per step. Midpoint and Heun use two.
[Runnable example](native_ode_objects.aev)

`num_grid_sm_advance_advection_diffusion_3d_modes` advances existing Fourier
coefficients in x-fastest order with the spectral Forward Euler method.
Coefficients use the unnormalized forward FFT convention. Use the supplied
frame to normalize spacing, velocity, diffusivity and the time step. The
result retains the field reference. [Runnable example](native_continuum_models.aev)

`num_grid_fourier_helmholtz_batch` solves several periodic forcings with one
shifted Fourier symbol. Give one row per forcing in field per length squared
and a shift in inverse length squared. Multiply the result's flattened
`data` by `value_reference` to recover measured fields in input order.
`num_native_geometry_ricci_curvature` evaluates a symmetric metric callback
with one finite-difference stencil to return both the Ricci tensor and scalar.
Their dimensions are inverse coordinate squared and inverse coordinate squared
per metric unit, respectively. [Runnable examples](native_continuum_models.aev)

`num_grid_legacy_fdm_solve_wave_dirichlet_sampled` advances a 2D or 3D wave
with homogeneous Dirichlet faces and measured initial displacement and rate.
It keeps step zero, cadence-aligned steps and the final step for signed time
increments. Each retained row keeps the displacement's unit.
[Runnable example](native_continuum_models.aev)

`num_analysis_yee_plane_wave_sample` reads the normal-incidence TF/SF auxiliary
line of a Yee solver. It returns electric and magnetic amplitudes in separate
units and records both times. The magnetic sample is a half-cell ahead of the
electric sample. [Runnable example](yee_plane_wave.aev)

`num_native_fdtd_model_run_final` advances the legacy transverse magnetic model
and returns the measured electric field on the `(width, height)` grid and the
elapsed time. It accepts positive grid extents, including one-cell axes. The
length and wave-speed references specify the time step `L/(2c)`, and the source
uses a zero-based grid index and a measured frequency. Zero steps return the
unexcited field. `num_native_fdtd_model_run` keeps observations after steps 1,
6, 11 and so on. [Runnable example](native_continuum_models.aev)

`num_grid_cnm_solve_heat_equation_2d_cn_adi_fixed_boundary` reuses one
tridiagonal factorization per axis for its two-dimensional heat solve. The
initial field supplies the four fixed boundary edges. Data, spacing,
diffusivity and time step use the grid reference frame.
[Runnable example](native_continuum_models.aev)

`num_ns_divergence_summary` reports maximum absolute and RMS divergence,
active fluid area or volume, and signed outward flux. The divergence norms
use the fluid measure of each cut cell. Flux is area per time in 2D and
volume per time in 3D. [Runnable example](native_continuum_models.aev)

`num_linear_simplex_map_new` prepares a simplex from normalized vertices
and a positive coordinate reference. `num_linear_simplex_map_volume` returns
its unsigned physical measure. `num_linear_integrate_simplex_map` reuses the
map across compiled fields and returns the integral and error in field
units times the simplex measure. [Runnable example](native_continuum_models.aev)

`Rssn_numerical_special` exposes `num_special_ext_carlson_rg`,
`num_special_ext_jacobi_amplitude`, and `num_special_ext_jacobi_elliptic_batch`.
The batch output has one row per input argument and columns `sn`, `cn`, `dn`.
`num_analysis_coordinates_cartesian_to_spherical` and
`num_analysis_coordinates_spherical_to_cartesian` convert fixed triples.
Use the same reference length for Cartesian coordinates and the radius. The
polar angle starts at +z and the azimuth starts at +x, both in radians.
[Runnable example](physical_functions.aev)

`num_analysis_complex_scalar_method_recip` computes a complex reciprocal
with a scaled fallback for extreme components. Its output dimension is the
inverse of the input's dimension. [Runnable example](native_kernel_families.aev)

`num_fresnel_sc_interval` returns the sine and cosine Fresnel integrals between
two bounds. It preserves small differences between adjacent or far-field
endpoints and accepts improper limits. [Runnable example](physical_functions.aev)

`num_special_elliptic_pi_plan_new` prepares the third-kind elliptic integral
at fixed characteristic and parameter. Read its complete value with
`num_special_elliptic_pi_plan_complete`, or supply amplitudes in radians to
`num_special_elliptic_pi_plan_eval` and `num_special_elliptic_pi_plan_eval_many`.
The batch follows input order. [Runnable example](physical_functions.aev)

`num_special_ext_bessel_in_log_abs` returns the natural logarithm of the
magnitude of the integer-order modified Bessel function. It stays finite
for large finite arguments even when the scaled `num_special_ext_bessel_in_scaled`
value underflows. A nonfinite logarithm, such as at zero argument and nonzero
order, produces a domain error. [Runnable example](physical_functions.aev)

`num_native_cheb_from_values` constructs a portable Chebyshev interpolant
from 1 to 65 samples at second-kind nodes, starting at the upper endpoint.
The endpoints share a coordinate dimension and samples carry an independent
ordinate dimension. Query it with `num_native_cheb_eval`.
[Runnable example](physical_functions.aev)

For known discontinuities, `num_analysis_quadrature_gk_breakpoints_scaled`
accepts a normalized interval and interior breakpoints with physical input and
output references. `num_grid_dimensional_solve_transport_sampled_nd` advances
all time steps and retains only the requested snapshots and the endpoint.
`num_grid_physics_fvm_burgers_nd_with_boundaries` evolves cell averages in a
velocity-valued frame. Give each axis a `Periodic` or `Outflow` boundary and
keep the time step within the per-axis CFL bound specified in the operator
reference. A fully periodic grid conserves the sum of cell averages.
[Runnable example](native_flow_conservation.aev)

`num_linear_bsr_matmul_dense` multiplies a block sparse operator by a rank-two
array whose columns are right-hand sides. The result has one row per operator
row and one column per right-hand side. Its reference scale and dimension are
the products of the two inputs.
[Runnable example](native_linear_tensors.aev)

Use `num_linear_sparse_native_lu_solve_many`,
`num_linear_sparse_native_cholesky_solve_many`, or
`num_linear_sparse_native_solve_auto_many` to solve several right-hand sides
with one sparse factorization. The automatic method returns the selected
algorithm and the solution array. The reference scale and physical dimension
are those of the right-hand sides divided by the matrix.

`num_linear_amg_general_precondition_many` applies one general AMG cycle
from zero for each row. All rows share the hierarchy and workspace, and
their dimensions are those of the right-hand sides divided by the operator.
`num_linear_amg_general_workspace_len` reports the number of scratch slots
for one cycle. [Runnable example](native_linear_tensors.aev)

`num_linear_train_inner_workspace_len` reports the temporary storage for
the compressed inner product of two equal-shape tensor trains.

`num_linear_sylvester_plan_new` prepares `A X + X B = C` from two square
native matrices of the same coefficient dimension. `num_linear_lyapunov_plan_new`
uses one Schur decomposition for `A X + X A^T = C`. Use
`num_linear_sylvester_plan_solve` with either plan and each right-hand side.
The output uses the right-hand side's dimension divided by the coefficients'
dimension. [Runnable example](native_linear_tensors.aev)

`num_native_ewald_reciprocal_plan_new` prepares the reciprocal wave pairs at
fixed cell, screening and cutoff. `num_native_ewald_reciprocal_plan_evaluate`
reuses them for any configuration with the same cell and reference frame,
returning energy, per-particle force and virial.
[Runnable example](native_particle_engines.aev)

`num_native_sph_density_plan_new_d1`, `num_native_sph_density_plan_new_d2`
and `num_native_sph_density_plan_new_d3` prepare neighborhoods from the
current particle positions and boundaries up to a maximum smoothing length.
`num_native_sph_density_plan_evaluate_d1`, `num_native_sph_density_plan_evaluate_d2`
and `num_native_sph_density_plan_evaluate_d3` accept updated masses and
smoothing lengths. The output is mass per length to the spatial dimension
in the original particle order. [Runnable example](native_particle_engines.aev)

`num_analysis_pic_assignment_new` prepares periodic PIC interpolation weights
for fixed measured positions. `num_analysis_pic_assignment_deposit` returns
mesh charge per unit length. `num_analysis_pic_assignment_gather` samples a
field at the same particle positions and preserves its physical dimension.
[Runnable example](native_particle_engines.aev)

Use `num_native_split_linear_plan_new` prepares fixed-potential propagation on
one to three periodic axes of any positive size within the grid budget.
`num_native_split_prepare_linear` reuses a centered solver's grid and
references. `num_native_split_linear_plan_size` reports the flattened size.
`num_native_split_linear_plan_evolve` shares the cached phases across wave
functions and preserves amplitude units. A negative timestep reverses the
propagation. [Runnable example](native_particle_engines.aev)

`num_functional_l2_grid_new` prepares trapezoidal weights for a measured
coordinate axis. `num_functional_l2_grid_norm` returns the ordinate dimension
times the square root of the axis dimension. `num_functional_l2_grid_norm_and_normalize`
computes both results in one reduction. The normalized ordinate has the inverse
square-root axis dimension. A zero-measure input has no normalized ordinates,
and an overflowing norm has no representable scalar result.
[Runnable example](physical_functions.aev)

`num_analysis_stochastic_jump_sample_batch` draws consecutive marks from one
stream. Rows follow draw order and each column has the mark dimension. Empty
or failed batches preserve the stream state. [Runnable example](sde_adaptive_jump.aev)

`num_analysis_sobol_point_at` reads a point by its unsigned 32-bit index
without advancing the sequence. `num_analysis_sobol_seek` changes the
position of a retained generator. The next draw advances by one, and the
largest index wraps to the origin. [Runnable example](native_discrete_objects.aev)

`num_analysis_levin_plan_new` prepares nodes and weights for a fixed phase,
its derivative and the frequency. `num_analysis_levin_plan_nodes` returns
the physical nodes. Use `num_analysis_levin_plan_integrate_samples` for
samples in that order, or `num_analysis_levin_plan_integrate` for a compiled
amplitude. The complex result has amplitude units times coordinate units.
[Runnable example](physical_functions.aev)

`num_native_ball_values_and_jacobian_at_terms` computes a polynomial system's
values and Jacobian together at a point. `num_native_ball_eval_value_and_jacobian_terms`
returns both enclosures over a complex box. Values carry the residual dimension,
and Jacobian entries carry residual divided by coordinate dimensions.
[Runnable example](native_discrete_objects.aev)

`num_analysis_series_sum_terms` takes a measured prefix and a finite term
budget. The sum estimate and its heuristic absolute error share the sample
dimension, and `terms` reports the actual number consumed. An estimate is
`None` when it is unavailable. [Runnable example](physical_functions.aev)

`num_linear_csc_transpose_matvec` computes a transposed sparse product directly
from CSC columns. `num_linear_csc_matvec_into` and
`num_linear_csc_transpose_matvec_into` reuse a destination vector and its
physical reference. Errors leave the destination unmodified.
[Runnable example](native_linear_tensors.aev)

`num_grid_fem_residual_estimator_new` keeps simplex geometry for a fixed space.
Use `num_grid_fem_residual_estimator_indicators` for constant coefficients or
`num_grid_fem_residual_estimator_indicators_var` for variable ones. Each result
has normalized cell indicators and a physical reference of field units times
length to the power `(dimension + 2)/2` divided by time.
[Runnable example](native_continuum_models.aev)

`num_linear_csc_norms` obtains the Frobenius, induced 1-norm and induced
infinity norm together. Each field carries the matrix entry dimension.
[Runnable example](native_linear_tensors.aev)

`num_stats_kruskal_wallis` compares independently sized groups with one
measurement dimension. Pass flat values and zero-based CSR offsets. The
rank statistic, chi-square right-tail probability and degrees of freedom
are dimensionless. [Runnable example](physical_functions.aev)

`num_combinatorics_bell_batch` returns a Bell number for each index in input
order. Each count is `Some` when it fits in `Float` and `None` on overflow.
`num_number_theory_prime_sqrt_plan_new` checks one prime modulus and prepares
its square-root method. `num_number_theory_prime_sqrt_plan_modulus` returns
that modulus. `num_number_theory_prime_sqrt_plan_sqrt` returns the smaller
root or `None` for a nonresidue. [Runnable example](native_discrete_objects.aev)

`num_native_multivector_mul_central` multiplies a Cl(3,0) multivector by a
scalar and pseudoscalar factor. The two factor coefficients share a physical
dimension, and each result coefficient uses the product of dimensions.
[Runnable example](native_discrete_objects.aev)

`num_brent_with_bracket` returns the root, final ascending bracket and actual
callback count. The root, bracket and absolute tolerance share the domain
dimension, and the callback can return a different residual dimension.
[Runnable example](native_ode_objects.aev)

`num_native_multilinear_grid_new` prepares a rectilinear grid for repeated
queries on unequal axis lengths and with independent axis units. Values are
flat with the first axis varying fastest. `num_native_multilinear_grid_eval`
accepts one coordinate per axis and returns the stored ordinate dimension.
[Runnable example](physical_functions.aev)

`num_robust_huber_location_with_scale` fits a Huber location using a positive
clipping scale in the sample's units and a dimensionless tuning constant.
[Runnable example](physical_functions.aev)

`num_dual_powi_with_second_derivative` reports the primal value, first
derivative and second directional derivative of a normalized integer power
along `val + t * deriv` at `t = 0`. [Runnable example](native_autodiff.aev)

`num_native_hessian_diagonal_scaled` evaluates coordinate-wise second
derivatives of a scalar callback. `num_native_hessian_diagonal_with_steps_scaled`
accepts one positive measured step per coordinate. Both return the output
unit divided by the square of the coordinate unit.
[Runnable example](native_autodiff.aev)

`num_analysis_newton_cg_tape_plan_new` retains a tape snapshot and one objective
output. Call `num_analysis_newton_cg_tape_plan_minimize` with a new initial
point, parameters and options to reuse its solver buffers. The record keeps
each coordinate's units, the objective units and the evaluation counts.
[Runnable example](native_autodiff.aev)

`num_linear_tensordot` contracts specified paired axes of two native arrays.
It preserves the order of their remaining axes and multiplies the physical
references. [Runnable example](native_linear_tensors.aev)

`num_native_lie_from_matrices` constructs a Lie algebra from exact square
matrices spanning a space closed under the commutator. `num_native_lie_bracket`
returns exact coefficients in the original basis.
[Runnable example](native_discrete_objects.aev)

`num_native_tape_column_gather` prepares a tape that selects one source
column per row. Pass a dimensionless offset column first to
`num_native_tape_eval_columns`, followed by the measured source columns.
Offsets are zero-based and truncated toward zero. All source columns and
the result share the prepared physical dimension.
[Runnable example](native_execution.aev)

`num_analysis_polynomial_antidifference_new` compiles ascending coefficients
at a measured origin and positive step. `num_analysis_polynomial_antidifference_eval`
returns the prepared function at a point. `num_analysis_polynomial_antidifference_difference`
uses the interval width directly for nearby points and preserves coefficient
units. [Runnable example](physical_functions.aev)

`num_native_sum_indefinite_sum` keeps the term's physical dimension, and
`num_native_sum_indefinite_product` multiplies positive terms divided by the
explicit value reference. Both accept a measured start and step, and check
lattice work against the caller's evaluation budget before sampling.
[Runnable example](physical_functions.aev)

`num_linear_randomized_svd` uses a seeded sketch to approximate the leading
singular triplets. Multiply normalized entries of `s` by
`singular_value_reference` to recover singular values in the matrix's units.
[Runnable example](native_linear_tensors.aev)

`num_linear_geometric_level_size` counts interior points at a zero-based
level. Pass the fine level to both `num_linear_geometric_restrict` and
`num_linear_geometric_prolong`. Restriction uses full weighting toward the
adjacent coarser grid, and prolongation interpolates back with homogeneous
Dirichlet boundaries. Both keep the field's dimension and reference scale.
[Runnable example](native_linear_tensors.aev)

`num_analysis_hypergeometric_hyp2f1_cplx` evaluates the Gauss function at
`z = 1` for positive real `a` and `b` when `c > a + b`. The endpoint
calculation avoids forming the potentially overflowing Gamma factors.
[Runnable example](physical_functions.aev) · [NIST formula](https://dlmf.nist.gov/15.4.E20)

`num_special_ln_bessel_k` takes a dimensionless real order and a positive
dimensionless argument. It returns the natural logarithm of the modified
Bessel function of the second kind, including values whose magnitude is
too small for `num_special_bessel_k` to represent.
[Runnable example](physical_functions.aev)

`num_analysis_meijer_meijer_g_real` evaluates the dimensionless real Meijer
G function. With orders `(1, 0)`, an empty upper parameter list and one
lower parameter `b`, it combines the power and exponential factors of
`z^b exp(-z)` on the nonnegative real axis and retains representable
subnormal results.
[Runnable example](physical_functions.aev)

`num_special_exact_chebyshev_tu_mod` returns the residues of `T_n(x)` and
`U_n(x)` as `item0` and `item1`. The degree and modulus can use the full
unsigned 64-bit range, and `x` is a signed 64-bit integer. A zero modulus
returns `None`. [Runnable example](native_kernel_families.aev)

`num_analysis_holonomic_expression_series` returns exact coefficients of a
holonomic expression in ascending degree, up to the requested truncation.
The integer power case uses a truncated recurrence. Undefined series return
`None`. Requests beyond the native term budget return an error.
[Runnable example](native_kernel_families.aev)

`num_topology_persistent_homology` accepts measured filtration values and
ragged simplices as zero-based offsets and vertex indices. A repeated simplex
enters at its earliest filtration value. The result keeps the filtration
dimension and identifies essential intervals with a Boolean mask.
[Runnable example](native_kernel_families.aev)

`num_complex_winding_number` counts turns around zero for a complex polygon
with a common physical dimension and joins the last vertex to the first.
Counterclockwise turns count positively. A path through zero yields a
domain error. [Runnable example](native_kernel_families.aev)

`num_norm` and `num_distance` preserve the coordinate dimension and scale
intermediate squares when needed. `num_normalize` returns a dimensionless
direction for finite nonzero input even when the original vector's norm
exceeds Float range. [Runnable example](native_kernel_families.aev)

`num_reflect` preserves the input vector's dimension and takes a
dimensionless unit normal. It can retain finite reflected components even
when the intermediate correction overflows.
[Runnable example](native_kernel_families.aev)

`num_linear_arnoldi` returns a dimensionless Krylov basis and a normalized
Hessenberg matrix and residual `beta`. Multiply `hessenberg` and `beta` by
`operator_reference` to recover the operator's units.
[Runnable example](native_linear_tensors.aev)

`num_native_qs_factor_pair` takes a dimensionless integer and a quadratic
sieve limits handle. It returns a pair of verified nontrivial factors in
ascending order, or `None` if the sieve finds no pair within the limits.
[Runnable example](native_discrete_objects.aev)

`num_native_number_exact_falling_factorial` counts ordered selections of
`k` distinct members from `n`. It returns an exact integer, including zero
for an impossible selection and one for a valid empty selection.
[Runnable example](native_kernel_families.aev)

`num_permgroup_power_exact` raises a zero-based permutation to a signed
exact integer power. A negative exponent applies the inverse permutation.
The native operator checks that all images form a bijection.
[Runnable example](native_kernel_families.aev)

`num_analysis_regression_value_and_gradient` evaluates a retained linear
regression in one residual pass. For predictors in D and responses in E,
the loss has units E². The intercept derivative has units E and each slope
derivative has units E·D, in coefficient order.
[Runnable example](native_algebra_models.aev)

`num_homology_chain_betti_mod_p` returns one Betti number per degree from
a retained chain complex. The coefficient field uses an exact unsigned
64-bit prime. Characteristic two can give different results from odd
characteristics, as the real projective plane example shows.
[Runnable example](native_kernel_families.aev)

`num_inverse_erfc` accepts the entire Float probability range from zero
through two. Logarithmic refinement keeps the deep tail accurate down to
the smallest positive Float. `num_analysis_special_complex_lambert_w_complex`
takes an integer branch and resolves the two sides of its negative real cut.
[Runnable example](native_kernel_families.aev)

`num_native_rs_encoder_new` prepares one Reed–Solomon parity length for
reuse. `num_native_rs_encoder_encode` returns the original bytes followed
by parity, and `num_native_rs_encoder_parity_symbols` reads the stored
parity length. A whole codeword fits in 255 bytes.
[Runnable example](native_kernel_families.aev)

`num_native_affine_ifs_new` prepares dimensionless affine maps with
nonnegative relative weights. Reuse the handle with
`num_native_affine_ifs_generate` and pass the starting point, retained
point count, burn-in count, and exact unsigned 64-bit seed. The result
has two columns and excludes the starting point.
[Runnable example](native_kernel_families.aev)

`num_analysis_complex_scalar_method_sqrt` returns the principal complex
root, including representable small components near the real axis. The
result carries the square root of the input dimension.
[Runnable example](native_kernel_families.aev)

`num_native_graphics_prepared_triangle_new` prepares three vertices using a
positive reference in their common coordinate dimension.
`num_native_graphics_prepared_triangle_intersect` tests a single ray, and
`num_native_graphics_prepared_triangle_intersect_parallel` tests up to 4,096
origins with a shared dimensionless direction. An absent upper bound leaves
the ray interval unbounded above. The batch `hit_mask` identifies the rows
with intersections, whose ray parameters and points keep the coordinate
dimension. Normals are dimensionless.
[Runnable example](native_discrete_objects.aev)

`num_native_dd_hypot` computes a norm from two double-double handles with
the same dimension. `num_native_cdd_norm_dd` computes a double-double
modulus from a complex double-double handle. For either result,
`num_native_dd_parts` returns the separately measured high and low
components as `item0` and `item1`. `num_native_cdd_recip` returns a complex
double-double handle in the inverse dimension and rescales extreme components.
[Runnable example](native_discrete_objects.aev)

`num_distributions_weibull_quantiles` evaluates lower-tail probabilities in
input order, and `num_distributions_weibull_inverse_survival` accepts the
upper-tail probability directly. Results retain scale units. The native
Float bindings report a domain error when a quantile is infinite.
[Runnable example](physical_functions.aev)

`num_native_overlap_save_plan_new` stores the spectrum of a measured filter.
Use `num_native_overlap_save_plan_apply` for one recording, or
`num_native_overlap_save_plan_apply_batch` for flat recordings and zero-based
row offsets. Repeated offsets describe empty rows, and the result reports its
own offsets. Output values have sample units times tap units.
[Runnable example](physical_functions.aev)

`num_interp_ext_pchip_many` or `num_interp_ext_akima_many` for a tensor of
query coordinates. Queries share the knots' dimension. The returned samples
retain the ordinates' dimension and the order of the queries. An online
covariance accumulator accepts `num_native_online_covariance_push_frequency_batch`
with one dimensionless, nonnegative integer count per row. The total count is
the sum of the frequencies, as if each row had been repeated.

`num_analysis_sde_drift_implicit_sampled` advances every step over the supplied
Brownian increments. It keeps the initial state, states at the requested step
stride, and the final state. The state dimensions and callback budget are the
same as for the full path.

`num_number_ext_mobius_sieve` evaluates the Möbius function at consecutive
integers starting from zero. `num_number_ext_solve_linear_congruence` returns
the least nonnegative solution and its period, or `None` if no solution exists.

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
Their kind and session are checked on every call. A successful
`num_native_release` invalidates every copy. Release dependent variables and
gradients before releasing their tape so the parent release can succeed. Independently owned
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
report `Unsupported` when their RSSN feature is disabled. Default builds include Cranelift, wgpu and NumPy file support. Run the native
execution examples with the regular CLI.

```sh
cargo run -p aevia-cli -- test examples/native_execution.aev
```

Run `aevia devices` to see available RSSN backend selectors.
[The measured-backend example](native_backend.aev) uses the active selector. Run
it with `--rssn-backend cpu` or `--rssn-backend cranelift` to compare execution.

`object_binding!` derives schemas, codecs, arity and registration from typed
adapter declarations. Source-derived scalar/record bindings use reviewed unit
policies. `cargo run -p rssn-api-audit -- --verify` checks the inventory snapshot
and callable coverage from one scan. A re-export inherits coverage when Rust
source resolution proves it is the same public item. The inventory also records
macro and re-export fingerprints for review.
