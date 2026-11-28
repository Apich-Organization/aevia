# Numerical computing in Aevia

Run an example's executable assertions with:

```sh
bazel run //:aevia -- test examples/dimensioned_numerics.aev
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
| `complex_coordinate_maps.aev` | Dimensionless stereographic coordinates, finite inverse projection and both inverse Joukowsky branches |
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

`Rssn_Special.riemann_sphere_project` returns a record with `x`, `y` and `z`
fields. `riemann_sphere_unproject` takes those coordinates and returns
`Ok(None)` at the north pole and `Err` for a point off the unit sphere.
`joukowsky_inverse` returns `plus` and `minus` branches ordered by the
principal square root. The maps take dimensionless complex coordinates.
`cayley_transform` is the Möbius map on the complex plane with its pole at
`-i`. Use `uhp_to_disk` for the checked open hyperbolic chart.

`Rssn_Special.complex_jacobi_p` and the `complex_*` orthogonal polynomial
functions take dimensionless parameters and points with integer degrees
from 0 through 4096. `complex_spherical_harmonic` takes integer order
and the colatitude and azimuth in radians. These functions return
`Result<Complex, OpError>`. [Runnable example](complex_special_functions.aev)

The same module provides Weierstrass sigma and zeta for two complex
half-periods. Sigma is zero at lattice points and zeta returns a pole error.
The implementation reduces the lattice basis, caches theta coefficients, and
restores the quasiperiodic terms for points outside the centered cell.

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

For an explicit complex branch cut, import `Rssn_Special` and call
`arg_branch(z, theta0)`, `log_cut(z, theta0)`, `sqrt_cut(z, theta0)`, or
`pow_cut(z, w, theta0)`. The angle is in radians. On the cut, the argument
takes the upper endpoint of `(theta0, theta0 + 2π]`. The logarithm and power
return a domain error at zero. The square root accepts physical quantities.
For example, `sqrt_cut(complex(4.0 m^2, 0.0 m^2), 0.0)` returns a complex
length with a real part of approximately `-2.0 m`.

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
For a certified exponential, use `Rssn_Multiprecision.exp_dyadic(value, precision)`
for an exact input or `exp_ball(interval, precision)` for a closed interval.
Both return a dyadic midpoint and an outward radius in a typed `Result`.

The typed `Rssn_Multiprecision` facade exposes `dyadic_from_int` with explicit
rounding and `dyadic_from_float` and `dyadic_to_float` for finite binary64
exchange. `add_balls`, `subtract_balls` and `multiply_balls` round outward
and keep a portable midpoint and radius.
[Exact dyadic example](dyadic_real.aev)

For complex intervals, `Rssn_Multiprecision.DyadicRectangle` uses the same
exact dyadic balls as its `real` and `imag` fields. Use
`make_complex_rectangle(real, imag)` to combine two balls or
`lift_real_ball(real)` for a real interval. For a lifted interval
`[1, 3]`, `complex_rectangle_norm_squared(rectangle, 64)` encloses
`[1, 9]` and `reciprocal_complex_rectangle(rectangle, 64)` encloses its
reciprocal. The operations return `Result` for invalid records,
precision bounds and division through the origin.

Use `Rssn_FiniteField.poly_mul_ntt_mod(a, b, p)` for exact coefficient
convolution modulo an odd prime. Its transform length is the next power
of two of the product length and must divide `p - 1`.
`Rssn_NumberTheory.extended_gcd_u64` returns named Bézout coefficients.
[Modular polynomial example](modular_polynomials.aev)

`Rssn_Numeric.CsrMatrix` carries ordinary CSR row pointers, zero-based
column indices and measured entries. Use `csr_transpose`,
`csr_matmul_dense` and `csr_matmul_sparse` for rectangular matrices.
For least squares use `csr_lsqr` with a damping value in the matrix entry
dimension. Set it to zero for the undamped solve. The solution retains
right-hand-side units divided by matrix entry units.
[Structural CSR fixture](../tests/domain_krylov_csr.aev)


For a censored sample, `Rssn_Statistics.kaplan_meier(time, event, confidence)`
returns event times, risk counts, survival probabilities and confidence
bounds. Pass the record to `median_survival` to obtain an optional value
in the same time dimension. `fit_cox` accepts dimensionless covariates and
the `"breslow"` or `"efron"` tie method. Pass its fitted record to
`predict_cox` for relative hazards.
[Survival models fixture](../tests/domain_survival_models.aev)


For repeated transforms, obtain a plan with `Rssn_FiniteField.ntt_plan(p, length)`.
Pass its result to `ntt_transform(values, plan, false)` for the forward
transform and use `true` for the normalized inverse. Values are ordinary
integer residues. The same plan can be serialized as its modulus and
length. The [NTT fixture](../tests/domain_finite_field_ntt.aev) checks
the exact spectrum and inversion with the Goldilocks prime.

`Rssn_Multiprecision.named_dyadic_constant("pi", 64)` returns a certified
nearest-even dyadic. Use `enclose_dyadic_constant("pi", 64)` to keep the
rounding error in the result or `floor_pi_times_power_of_two(8)` for
the exact integer 804. `dyadic_constant_names()` lists the supported names.

Use `Rssn_Multiprecision.ln_dyadic`, `log2_dyadic`, `log10_dyadic`
and `exp2_dyadic` for exact dyadic inputs. For existing closed intervals,
use `ln_ball`, `log2_ball`, `log10_ball` or `exp2_ball`.
All return outward enclosures. A logarithm requires a strictly positive
input. `scale_decimal_dyadic_in` rounds an exact product by a power of ten
in the chosen direction.

Use `Rssn_Multiprecision.sin_cos_dyadic(angle, precision)` to compute
both outward enclosures in one reduction. Angles are in radians.
`sin_ball`, `cos_ball` and `tan_ball` take a closed dyadic interval.
`tan_ball` returns an error when the cosine enclosure includes zero.

Use `Rssn_Multiprecision.sinh_dyadic`, `cosh_dyadic` or
`tanh_dyadic` for an exact real input. Their `sinh_ball`, `cosh_ball`
and `tanh_ball` variants enclose the image of a closed dyadic interval.
For `cosh_ball`, a zero crossing includes the exact minimum of one.

Use `Rssn_Multiprecision.atan2_dyadic(y, x, precision)` for a principal
angle or `atan2_ball` for a closed coordinate rectangle. A rectangle that
contains the origin or touches the negative real cut from above or below
returns a domain error. `dyadic_rectangle_argument` uses the same corner
bounds for a complex rectangle and excludes the non-positive real axis.

Use `Rssn_Multiprecision.asin_dyadic` or `acos_dyadic` for exact
inputs, and `asin_ball` or `acos_ball` for closed intervals. Both
domain endpoints are accepted. An interval outside [-1, 1] returns a
domain error.

Use `Rssn_Multiprecision.powi_dyadic(base, exponent, precision)` for a
signed integer exponent and `power_dyadic_ratio(base, numerator, denominator, precision)`
for an exact rational exponent. For example, raising −8 to 2/6 encloses
−2 because the reduced denominator is three. The interval variants check
the domain over the entire input interval.

Use `Rssn_Multiprecision.complex_rectangle_modulus` for the range of
magnitudes in a closed dyadic rectangle. `exp_complex_rectangle` and
`ln_complex_rectangle` return closed dyadic rectangles. The exponential
requires an imaginary midpoint within ±2^20 radians. The principal
logarithm returns a domain error if the input meets the non-positive
real axis. See [the complex rectangle example](../tests/domain_mp_complex_exp_log.aev)
for exact records and error handling.

The same type is accepted by `sin_complex_rectangle`, `cos_complex_rectangle`,
`tan_complex_rectangle` and the corresponding `sinh`, `cosh` and `tanh`
variants. For an imaginary-axis input `iy`, `tan_complex_rectangle` returns
a rectangle on the imaginary axis enclosing `i * tanh(y)`. This also works
for large finite `y`. The [trigonometry fixture](../tests/domain_mp_complex_trigonometry.aev)
shows the exact record syntax.

For principal roots, use `sqrt_complex_rectangle`. The square root of −4
is 2i. `principal_power_complex_rectangle` takes a separate exponent
rectangle and recognizes exact signed 64-bit integer point exponents.
Its general logarithmic route requires the base rectangle to avoid the
non-positive real axis. See [the root and power fixture](../tests/domain_mp_complex_root_power.aev).

Use `Rssn_Multiprecision.asin_complex_rectangle` and its five companion
inverse functions for principal inverse values. For instance, both
`acos_complex_rectangle(point_one, 32)` and
`acosh_complex_rectangle(point_one, 32)` enclose zero. Atan returns
`Err` at the imaginary unit. See the
[complex inverse fixture](../tests/domain_mp_complex_inverse.aev).

To carry exact bounds through a calculation, use
`Rssn_Multiprecision.enclose_expression("sqrt(81) + hypot(3, 4) + cbrt(-8)", 24)`.
It returns a dyadic midpoint and radius. For a function with a known global
Lipschitz bound on your input interval, use `widen_ball_lipschitz` with
the exact mapped midpoint and the nonnegative bound. The
[enclosure fixture](../tests/domain_mp_enclosures.aev) shows both calls and
their domain errors.

The certified `erf_complex_rectangle` and `erfc_complex_rectangle`
operators evaluate the complex error functions over dyadic rectangles.
For example, `erfc_complex_rectangle(point_twenty, 128)` keeps the
small nonzero tail at the real point 20. The
[error-function fixture](../tests/domain_mp_error_functions.aev) builds
the input rectangle and checks the returned dyadic enclosure.

Use `Rssn_Multiprecision.bessel_j_complex_rectangle(value, order, 128)`
for principal Bessel J. Exact integer order permits a negative real
argument. A noninteger order uses the principal branch and returns
`Err` for a rectangle that meets the non-positive real cut. The
[Bessel fixture](../tests/domain_mp_bessel.aev) shows the origin limits,
branch checks and typed dyadic inputs.

The same module provides `gamma_complex_rectangle`,
`log_gamma_complex_rectangle` and `digamma_complex_rectangle`. Gamma
and digamma accept negative real arguments between poles. Log-gamma uses
the analytic branch real on positive arguments and returns `Err` on
the non-positive real cut. See the
[gamma-family fixture](../tests/domain_mp_gamma_family.aev) for exact
factorials, the branch cut and pole errors.

Use `riemann_zeta_complex_rectangle(order, 128)` across the critical
strip and into the supported left half-plane. For polylogarithms, pass a
complete dyadic rectangle strictly inside the unit disk to
`polylog_complex_rectangle(value, order, 128)`. The returned radius
accounts for the operand ranges and the bounded series tail. See the
[Dirichlet-function fixture](../tests/domain_mp_dirichlet.aev).

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

Pass the same portable record to `num_control_discrete_n_states`,
`num_control_discrete_n_inputs` and `num_control_discrete_n_outputs`
to query its extents. `num_control_discrete_poles` returns complex eigenvalues,
and `num_control_discrete_is_stable` checks for poles strictly inside the
unit circle. The controllability and observability matrices retain the
units of `b` and `c`, respectively. These diagnostics require a discrete
model, whose state matrix `a` is dimensionless.

For dimensionless complex Taylor data, `Rssn_Special.pade_complex` returns
the numerator and denominator as a record. Reuse it with
`Rssn_Special.eval_pade_complex` to evaluate at several complex points.
`recenter_taylor_complex` moves a Taylor polynomial through a sequence of
centers, and `wynn_epsilon_complex` accelerates a sequence of partial sums.
[Runnable continuation tests](../tests/domain_analytic_continuation.aev)

`Rssn_Special.holonomic_order`, `continue_holonomic` and `holonomic_monodromy`
accept dimensionless polynomial coefficient rows in ascending powers. Row
`j` multiplies the `j`th derivative, and the initial state lists derivatives
starting with the function value. Supply a closed path for monodromy.
`borel_pade_complex` uses a deterministic upper lateral integral when its
rational Borel transform has a pole on the positive integration axis.

`num_continuation_laurent_fft` accepts a complex callback, a center and a
positive radius of the same dimension, negative and positive orders, a
sample count and an evaluation budget. Its coefficients use the local
variable `(z - center) / radius`, retain the callback's output units and
start at `first_power`. The record reports the radius and samples used.
[Runnable examples](../tests/domain_holonomic_borel_laurent.aev)

`num_native_irfft_plan_new` prepares an inverse real FFT for a fixed real
output length. Reuse the plan for successive spectra with
`num_native_irfft_plan_apply`, which preserves the spectrum's physical
dimension. `num_native_irfft_plan_output_len` reports the real sample
count, and `num_native_irfft_plan_spectrum_len` reports the required number
of nonnegative frequency bins. For example, both four and five real
samples require three bins. [Runnable example](native_kernel_families.aev)

`Rssn_Optimizer.solve_linear_program` minimizes a linear objective with finite
lower bounds and optional upper bounds. Use `upper_present` to select which
upper entries apply. Use `Le`, `Ge` or `Eq` for each constraint row. If the
coordinate, objective and constraint dimensions are D, E and C, objective
coefficients have dimension E/D, constraint coefficients have C/D, and
right-hand sides have C. Provide positive references with dimensions D,
E and C. The result carries the status, measured coordinates and optional
objective. `simplex_budgeted` assumes nonnegative variables and accepts
`Dantzig`, `SteepestEdge` or `Bland` pricing with a shared pivot budget for
both phases. [Runnable LP examples](../tests/domain_linear_programming.aev)

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

`Rssn_ComplexPolynomial` takes dimensionless complex coefficients in ascending order. `durand_kerner` and `aberth_ehrlich` return one candidate per degree with individual relative backward errors and the largest difference in the reconstructed normalized coefficients. For a nonzero constant the candidate tensor is empty. `laguerre` takes a complex seed and returns an optional candidate and its residual. The zero polynomial and nonconvergence return errors. Backward error quantifies the polynomial perturbation near the supplied candidate. A repeated or ill-conditioned root can have appreciable coordinate error even when this residual is small.

`Rssn_RiemannHilbert` uses fixed two-by-two complex tensors. Matrix addition and subtraction require matching entry dimensions. Products multiply the dimensions, inverses carry reciprocal units, and the Frobenius norm retains the original units. `evaluate` reuses a dimensionless collocation record containing increasing nodes in `(-1, 1)`, positive quadrature weights, and an `n × 2 × 2` tensor of jump densities. Evaluation accepts complex points off the closed arc `[-1, 1]`. `Rssn_numerical_callback.num_riemann_hilbert_solve_typed` builds this record from a dimensionless matrix jump callback. It checks the identity at the two endpoints and returns the actual number of callback evaluations.

The scalar `num_riemann_hilbert_cauchy_checked_typed`, `num_riemann_hilbert_plemelj_checked_typed` and `num_riemann_hilbert_factor_checked_typed` report absolute error estimates, termination and callback counts. Cauchy and Plemelj preserve the density dimension. Factorization samples the continuous jump phase from its principal value at `-1` and reports the error both before and after exponentiation. `num_complex_cauchy_fft_derivatives_checked_typed` uses powers of the local coordinate `(z - center) / radius`, so its derivative tensor retains a common output dimension. The doubled sampling grid gives an empirical error estimate.

For a measured contour and analytic callbacks, `Rssn_numerical_callback` exposes `num_complex_root_circle_moments_typed`, `num_complex_root_circle_winding_typed` and `num_complex_root_circle_rouche_samples_typed`. The moments are powers of `(z - center) / radius`; the result records the difference between the two sampling grids. Winding requires both grids to approach the same integer. Rouché reports the strict inequality at the sampled points. The callbacks receive coherent coordinates, and the record gives the actual callback count.

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

`Rssn_ComplexDynamics` uses the dimensionless quadratic map `z² + c`.
The escape functions report the crossing step, an `escaped` flag and an
optional potential estimate. Budget exhaustion leaves the potential absent.
`quadratic_orbit` includes the starting point. `quadratic_multiplier` checks
the requested orbit closure before reporting its numerical class, and
`quadratic_periodic_points` returns candidates and residuals for the requested
primitive period. `external_ray` takes its angle in turns and returns a point
with a relative Böttcher-coordinate residual.

For user-defined maps, `Rssn_numerical_callback.num_complex_dynamics_newton_basin_checked_typed` accepts the map and its derivative with explicit coordinate and residual scales. A singular derivative or an exhausted iteration count yields `root = None`. An exhausted callback budget yields an error. `num_complex_dynamics_koenigs_checked_typed` checks fixed-point closure and returns a truncated coordinate estimate, the measured closure error, and actual work counts.

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


The checked complex stencils in `Rssn_numerical_callback` derive both
Wirtinger estimates, Cauchy–Riemann and conformality diagnostics, a Cartesian
Laplacian, local magnification and rotation, polar residuals, or a complex-step
derivative. The Cartesian coordinate step has the input dimension. The polar
stencil takes separate radial and dimensionless angular steps. Results carry
the derivative dimensions, and the real analytic complex step samples once.
The first-order Cartesian routines share four samples. A collapsed rounded
step or exhausted callback budget returns an error.
[Runnable tests](../tests/domain_conformal_stencil.aev).

Use `Rssn_Univalent` for coefficient and derivative diagnostics on the normalized
unit disk. Supply Taylor coefficients in ascending order beginning with the
constant term. The Grunsky matrix and its test vector use complex tensors.
Bieberbach, Grunsky and Becker evaluate finite numerical conditions. Loewner
advances one radial step for a fixed driving angle in radians.
[Runnable tests](../tests/domain_univalent.aev).

The typed numerical callbacks also estimate the Schwarzian derivative on one
64-sample circle and run sampled Nehari, starlikeness, or convexity diagnostics.
The Schwarzian result has inverse coordinate-squared units. These finite
diagnostics carry explicit callback budgets and can reject poorly resolved inputs.
[Runnable tests](../tests/domain_univalent_sampled.aev).

The conformal kernels in `Rssn_Special` fit a homogeneous Möbius record from
three point pairs, evaluate a Bergman expansion from zero-padded coefficient
rows, and evaluate a prepared Schwarz–Christoffel map inside the upper
half-plane. The checked integral returns convergence, absolute error, and
sample count for the requested tolerance and panel budget.
[Runnable tests](../tests/domain_conformal_kernels.aev).

Prepare a Szegő quadrature once and reuse its boundary points, Cauchy weights,
and center for the Szegő kernel, Bergman kernel, and Riemann-map evaluations.
The checked map from the center follows a straight path inside the sampled
boundary. The two callback variants take explicit sample and callback budgets.
[Runnable tests](../tests/domain_conformal_szego.aev).

The Riemann covering helpers report packed ramification cycles and zero-based
sheet permutations for normalized algebraic curves. The local infinity loop
uses the coordinate `1/z`. For numerical monodromy, coefficients index powers of
`w` by row and powers of `z` by column, and the last path vertex connects to the
first. A path touching an unresolved branch point returns an error. The
measured branch-jump callback separately takes its normal angle and step.
[Runnable tests](../tests/domain_riemann_covering.aev).

For reproducible circular sampling, use
`num_contour_residue_infinity_fixed_typed` with a requested sample count and
callback budget that covers the minimum of 32 nodes. The adaptive contour,
principal-value and residue methods also return an absolute error estimate
and convergence flag. Their two components reuse complex callback samples.
[Runnable tests](../tests/domain_contour_compatibility.aev).

`Rssn_Contour` builds open and closed paths in one coordinate dimension.
Each segment has entries in `starts` and `ends`. The zero-based
`arc_segments` indices identify arcs, with matching entries in `arc_centers`,
`arc_radii`, `theta_start` and `theta_end`. Angles are bare radians and the
sign of an arc's sweep gives its orientation. `bromwich` and `hankel` set
`closed` to `false`. `indented_semicircle` requires room for every specified
pole and builds its geometry in a unit-radius frame before restoring units.


`Rssn_Contour.deform` constructs an open path between two measured complex
endpoints. Pass vectors of poles and ray origins, a positive pole clearance,
and one angle in bare radians per ray origin. Up to 64 poles and rays are
allowed in total. A path crossing a ray can travel around the branch point
on the same sheet. If the bounded construction cannot verify clearance or
disjointness, the result is an error. Use the generated
`Rssn_numerical_callback.num_contour_integrate_deformed_checked_typed` to
construct and integrate in one call, or reuse the returned path.
[Runnable tests](../tests/domain_contour_deformation.aev).

`Rssn_numerical_callback.num_contour_integrate_checked_typed` takes one of
these paths, coordinate and value reference scales, an absolute tolerance,
a panel limit per component and a total callback limit. The final argument
is a complex-valued callback. The result contains `value`, an estimated
absolute `error`, `evaluations` and `converged`. Panel exhaustion can return
a finite estimate with `converged = false`. Callback failures, nonfinite
samples and exhausting the separate host callback limit produce errors.

`num_analysis_levin_plan_new` prepares nodes and weights for a fixed phase,
its derivative and the frequency. `num_analysis_levin_plan_nodes` returns
the physical nodes. Use `num_analysis_levin_plan_integrate_samples` for
samples in that order, or `num_analysis_levin_plan_integrate` for a compiled
amplitude. The complex result has amplitude units times coordinate units.
[Runnable example](physical_functions.aev)

`Rssn_Special` provides principal-branch Bessel `J` and `I` with complex
orders, Anger `J` and Weber `E` with real orders, and Kelvin functions with
named `ber`, `bei`, `ker` and `kei` fields. All arguments are dimensionless.
The exponential integral `E_n` and principal Legendre `Q_n` accept integers
from 0 to 4096. A signed imaginary zero selects the boundary value of
the cut. Singular and nonfinite evaluations report an error.

The typed complex root callbacks in `Rssn_numerical_callback` accept a
measured coordinate reference and an independent residual reference.
`num_complex_root_muller_typed` takes three guesses. The Halley variant
takes one guess and callbacks for the residual and both coordinate
derivatives. Homotopy follows a known zero from dimensionless time 0 to 1
using coordinate and time derivatives. A successful result includes the
root in the input coordinate units and the actual callback evaluation count.

`num_complex_root_delves_lyness_typed` estimates the zeros of a pole-free
function inside a circle from sampled function values and first derivatives.
Its radius serves as the coordinate reference. The returned roots retain
the circle's units. For a verified enclosure, use the whole-disk certification
operators.

The `num_saddle_*_typed` operators in `Rssn_numerical_callback` share
a physical saddle record. `num_saddle_find_typed` takes a complex guess,
coordinate reference, normalized slope tolerance and callback budget.
The phase is dimensionless, with first and second derivatives in inverse
coordinate units. The leading term, order-two correction and traced
integral return amplitude times coordinate units. For order two, the phase
and amplitude must be analytic on a disk with radius one tenth of the
coordinate reference. The traced integral takes a measured half length
and an absolute tolerance in the output units.

`Rssn_ComplexDisk` represents a closed Euclidean disk as `[real, imag, radius]`.
The coordinates and radius share one physical dimension. `make` takes a
complex center and a nonnegative radius, and `point` creates a zero-radius
disk from its real and imaginary coordinates. `contains` treats uncertain
boundary membership as possible, while `inside_strictly` certifies inclusion.
`mul` and `divide` combine the units of the operands. `divide` returns
`Ok(None)` if the denominator disk cannot be separated from zero.

Use `variable` or `constant` to start a dimensionless first-order disk jet.
Its `value` and `derivative` fields are both enclosing disks. The `jet_*`
functions compose arithmetic and propagate the first derivative. `matvec`,
`matmul` and `solve` use disk coordinates on the last axis. `solve` returns
`Ok(None)` if it cannot certify a solution enclosure.

The elementary disk jet functions enclose both the principal value and its
first derivative. For example, `jet_acosh` uses the two principal square
roots of `z - 1` and `z + 1` separately in the derivative. A branch cut or
an enclosure that cannot be certified yields `Ok(None)` for the optional
jet functions. `sec`, `csc`, `cot`, `sech`, `csch` and `coth` likewise
return `Ok(None)` when their denominator cannot be separated from zero.
The elementary exponential evaluation uses a bounded reduction; its input
disk must have modulus at most `2^20` to obtain a finite enclosure.

The `erf`, `gamma`, `log_gamma` and `bessel_j` functions enclose
the whole normalized disk. A pole, branch cut or unsupported finite
range can yield `Ok(None)`. For proof-producing workflows, use the typed
`num_complex_disk_certify_*_typed` operators in `Rssn_numerical_callback`.
Their callbacks must enclose the whole disk or box. The scalar Krawczyk
test needs two calls, the system test needs three, and the Rouché test
needs two evaluations per arc.

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

The [numerical transforms fixture](../tests/domain_numerical_transforms.aev)
uses checked Mellin, Hilbert and inverse Laplace callbacks from
`Rssn_numerical_callback`, and Borel and Meijer G from
`Rssn_NumericalTransforms`. Supply dimensionless coordinates and an
absolute tolerance in the callback output unit. Finite quadrature errors
exclude omitted tails, and inverse Laplace errors compare two resolutions.

## Analyze a prepared SISO transfer

`Rssn_Control` takes coefficient vectors in descending powers and uses a
common normalized time and frequency scale. The same model can be evaluated,
combined with another model, or passed to step and margin analysis. A proper
model is required for a step response. Release the model after its final use.

```aevia
import Rssn_Control as Control

test "stable first-order transfer" =
    match Control.transfer_new_proper([2.0], [1.0, 1.0]) with
    | Ok(model) -> Control.is_stable(model) == Ok(true) && Control.release_transfer(model) == Ok(true)
    | Err(_) -> false
```

[Transfer-function examples and checks](../tests/domain_control_transfer.aev)

`Rssn_Control.simulate_pid` works on normalized errors, a positive sample
interval, and optional output limits. `pid_response` uses a prepared proper
transfer model. For discrete identification, pass an input vector and a
sample-by-output tensor to `identify_era`. Use `era_matrices` to inspect the
result or `impulse_response` to sample it. [PID and ERA tests](../tests/domain_control_trajectories.aev)

`Rssn_Control.prepare_mpc` builds a bounded discrete plan. A
`mpc_closed_loop` call returns the initial state and successive controlled
states. Keep the plan for additional trajectories and call `release_mpc`
when finished. [MPC examples and checks](../tests/domain_control_mpc.aev)

## Exact decimal exchange

Parse base-ten text with `Rssn_Multiprecision.decimal_to_dyadic(text, precision)`.
For example, parsing `"1.25000000001"` to two binary bits yields exactly
`1.5`. Use `dyadic_to_decimal(value, digits)` to write it back or
`round_decimal_text(text, digits, "nearest")` for a decimal-to-decimal
rounding such as `"1.3"` at two significant digits. To retain the exact
input in an interval, compose `enclose_decimal` with `format_decimal_ball`.

### Prepared objectives and scalar root searches

Use `Rssn_Optimizer.sqp_solve` for a normalized sparse nonlinear program.
The evaluation callback returns a named packet with the objective,
gradient, constraints and Jacobian. A separate callback forms the
upper-triangular Lagrangian Hessian from the current point and multipliers.
Use zero-based sparse triplets. Values are dimensionless normalized
coordinates and equality rows precede `g(x) <= 0` rows. The returned
multipliers and KKT residual make it possible to inspect the solve.
The callback interface takes up to 128 coordinates and 128 constraints,
with up to 16,384 triplets in each sparse record.
[Typed SQP fixture](../tests/domain_sparse_sqp.aev)

`Rssn_Optimizer` prepares built-in objective functions, normalized tapes
and regression models for repeated runs. An immutable tape uses unit-scale
references and an explicit output slot. `prepare_checked` reports nonfinite
tape evaluations as errors. A prepared problem can own a box, reuse its
objective with new bounds, and evaluate its cost and gradient at a
normalized point. `minimize` takes a solver name, an initial point, and
explicit iteration, evaluation and seed controls. The result reports the
best point, cost, native termination and evaluation count.

`regression_problem` uses the stored normalization of the predictor and
response. For physical residuals, pass coefficients and a response-dimension
reference to `residuals`. The result contains fitted minus observed responses
in coherent units. The typed root calls in `Rssn_numerical_vector` accept
scalar callbacks, an ordered bracket, a positive tolerance, and evaluation
limits. Newton and Halley can use a supplied derivative. See the
[prepared optimizer examples](../tests/domain_optimizer.aev).

The source API inventory and callable-manifest report are separate:
`tools/rssn-api-audit/inventory.tsv` records public Rust APIs, declared adapters
and resolved aliases;
`tools/rssn-api-audit/RSSN-COVERAGE.md` records language reachability. A Rust
helper is not counted as callable merely because it appears in the inventory.
Regenerate contracts, adapters and inventories with `bazel run //:refresh_kernel`.

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
bazel run //:aevia -- test examples/native_execution.aev
```

## Triangulate a measured planar domain

`Rssn_Meshgen.advancing_front` fills a simple polygon using a target edge
spacing. `Rssn_Meshgen.constrained_delaunay` keeps zero-based segment endpoint
pairs as chains of triangulation edges. Crossing constraints can introduce
additional vertices. Both operations take a positive reference length close
to the coordinate extent and an explicit output-vertex budget between 3 and
4096. The output includes measured coordinate pairs and zero-based triangle
triples. [Planar meshing examples and checks](../tests/domain_planar_meshgen.aev)

## Work with a measured CAD mesh

`Rssn_Cad.mesh_new` takes a three-column tensor of lengths and a three-column
tensor of zero-based triangle indices. `signed_volume` returns a measured
volume for a closed consistently wound surface. Pass a unit length when
reading or writing in-memory STL, and weld the result before checking its
adjacency. [CAD mesh examples and checks](../tests/domain_cad_mesh.aev)
`read_step_text` takes planar STEP solids with straight edges and returns one
measured handle per solid using the coordinate units declared in the text.
To load a file in Aevia, call `read_file(path)` with file-read access and pass
the returned text to `Rssn_Cad.read_step_text`. Language bindings can read the
file with their host APIs and pass the text to `num_cad_step_read_text`.

## Reuse a measured grid or integrate a compiled function

`Rssn_Interpolation.prepare_bilinear(xs, ys, cells)` retains a rectilinear
grid. Pass strictly increasing axes and a cell tensor with shape `[len(ys),
len(xs)]`. The two axes and the cells may use independent units.
`interpolate(grid, x, y, cell_unit)` clamps coordinates outside the
sampled range and returns a value in the cell dimension. Reuse the handle for
later queries. [Measured grid example](../tests/domain_interpolation.aev)

`Rssn_ParallelQuadrature.gauss_legendre` and `clenshaw_curtis` integrate a
compiled scalar function over measured bounds. The integrand witness supplies
the integrand dimension and the result has the product of both dimensions.
The compiled function's work budget must cover the quadrature samples.
`sum_compensated` keeps the vector's dimension and uses a deterministic
compensated sum. [Fixed quadrature examples](../tests/domain_parallel_quadrature.aev)

Run `aevia devices` to see available RSSN backend selectors.
[The measured-backend example](native_backend.aev) uses the active selector. Run
it with `--rssn-backend cpu` or `--rssn-backend cranelift` to compare execution.

Use `Rssn_Multiprecision.cbrt_dyadic(value, precision, rounding)` to round a
real cube root in a chosen direction. `cbrt_dyadic_enclosure`,
`hypot_dyadic_enclosure` and `sqrt_ball` return outward dyadic balls.
`dyadic_limits()` reports the supported precisions and binary exponents.

`object_binding!` derives schemas, codecs, arity and registration from typed
adapter declarations. Source-derived scalar/record bindings use reviewed unit
policies. `bazel test //:rssn_api_audit_test` checks the inventory snapshot
and callable coverage from one scan. A re-export inherits coverage when Rust
source resolution proves it is the same public item. The inventory also records
macro and re-export fingerprints for review.

## Reuse an exact modular reducer

`Rssn_Modular` exposes typed prepared reducers for unsigned machine-word
arithmetic. Use Barrett when the modulus can be even and Montgomery to compose
operations on encoded residues under an odd modulus. Inputs and outputs are
exact Aevia integers. Release a plan after its final use to free its session
slot.

```aevia
import Rssn_Modular as Mod

test "multiply modulo a composite" =
    match Mod.barrett_new(12) with
    | Ok(plan) ->
        let product = Mod.barrett_mul(plan, 7, 7)
        product == Ok(1) && Mod.release_barrett(plan) == Ok(true)
    | Err(_) -> false
```

The domain suite also exercises 128-bit reduction and failures for stale handles
or noncanonical encoded residues. [Modular reducer tests](../tests/domain_modular.aev)

### Markov chain Monte Carlo

`Rssn_Mcmc` provides explicitly seeded PCG32 streams and convergence
diagnostics. Pass one measured chain to `autocorrelation`,
`effective_sample_size` or `geweke`. Pass a tensor with one chain per row to
`split_rhat`. The rows must share a physical dimension. All diagnostics
return dimensionless values. Release a deterministic stream with
`Rssn_Mcmc.release` when it is no longer needed.

For sampling, use the typed `num_mcmc_*_typed` entry points in
`Rssn_numerical_stats`. They take normalized coordinates, a seed and a
bounded draws/burn-in/thinning configuration. Supply a target expressed in
log-density units to Metropolis–Hastings, its gradient to Hamiltonian Monte
Carlo, or a coordinate callback to Gibbs. The Gibbs callback receives the
zero-based coordinate index, the current state and a uniform draw. Samples
are returned by row alongside proposal and acceptance counts. Gibbs returns
an empty log tensor unless it receives a joint log target. See the
[deterministic regression examples](../tests/domain_mcmc.aev).

Rssn_Statrs.prepare takes a distribution name and its exact parameter count. Use the same prepared handle for densities, moments, and tail queries. The sf method evaluates the upper tail directly. Release a handle with release_distribution.


## Exact sparse matrices and prepared prime fields

`Rssn_SparseExact.field` validates an unsigned 64-bit prime once for a
session-owned field. Reuse the field for arithmetic, reduction and batched
inversion. A denominator divisible by the prime makes `reduce_matrix`
return `None`. The same applies to a zero residue in `batch_inverse`.
`reducer` returns an independent `Rssn_Modular` Barrett handle.

`rational_rref` and `prime_rref` return a reduced sparse matrix and its
zero-based pivot columns. `rcm_order` returns both directions of the
cached ordering for a square pattern. For `solve_dixon_multi`, place one
right-hand side per row. Solutions keep this row convention and exact
rational coefficients. A singular system returns `None`. See the
[sparse exact examples](../tests/domain_sparse_exact.aev).

The five `Rssn_Special.complex_carlson_*` calls work with dimensionless complex
arguments. For example, use `complex_carlson_rj(x, y, z, p)` with nonnegative
real parts for `x`, `y` and `z`, and a positive real part for `p`. When `p`
equals one of the first three arguments, the corresponding `R_D` uses the
same branch. Singular arguments return `Err`. `R_G(x, 0, 0)` is finite and
equals half the principal square root of `x`.

Use `Rssn_Special.to_polar` and `from_polar` with measured complex
coordinates. The radius has the coordinate dimension, and the principal angle
is in radians. `complex_abs_squared` squares the coordinate dimension.
`unwrap_phase` accepts measured samples away from zero. With dimensionless
samples, `continue_complex_root` follows the selected root branch as the path
winds around zero. Sample closely enough that the phase changes by less than
π between adjacent samples.

`Rssn_Special.barycentric_interpolate` accepts measured abscissas and
ordinates. Repeated queries on the same small grid reuse the node weights
without retaining the ordinates. For dimensionless polynomials in descending
coefficient order, `polynomial_power` uses an integer exponent and
`polynomial_gcd` uses a relative coefficient tolerance.

`Rssn_Statistics.fit_glm` takes dimensionless samples, a family and link,
optional observation weights, and explicit iteration and tolerance controls.
It returns named fit diagnostics and stores the link in the model.
`predict_glm(model, rows)` reuses that link for response means. Give both
operations design rows without an intercept column.
