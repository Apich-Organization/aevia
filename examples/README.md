# Aevia examples

[简体中文](README.zh-CN.md)

Run a complete program with `aevia run examples/FILE.aev`. Use `aevia test examples/FILE.aev` for an example that contains assertions. The commands assume you are at the repository root.

| Start with | Learn |
|---|---|
| [Kinematics](kinematics.aev) and [units](units.aev) | Exact and approximate quantities, unit conversion and affine temperatures |
| [Tensors](tensors.aev) and [sparse tensors](sparse.aev) | Matrix products, reductions, storage formats |
| [Matrix operations](matrix_operations.aev) | Dense constructors (`fill`, `linspace`, `eye`, `diag`), `trace`, `tril`/`triu`, and `set` |
| [Tridiagonal spring chain](tridiagonal_spring_chain.aev) | Solve a measured system using only its diagonals |
| [Calculus](calculus.aev) and [symbolic expressions](symbolic.aev) | Differentiation, integration, limits and equations |
| [Modules](modules/main.aev) | Imports across a two-file project |
| [Calibration](sensor_calibration.aev) and [relaxation fitting](relaxation_fit.aev) | Dimensioned residuals, quality thresholds and fitting a model |
| [Plotting sensor measurements](plot_series.aev) | Shared axes, error bars, histograms and SVG, HTML and animated GIF export |
| [Simulation I/O and visualization](sim_io_visualization.aev) | Damped oscillator CSV/NPY export and SVG, HTML & animated GIF visualization |
| [Parallel computation](parallel.aev) | Ordered mapping, reductions and worker tasks |
| [Frequency analysis](frequency_analysis.aev) | Centered frequencies and reconstructing an odd-length, dimensioned signal from its half spectrum |
| [Kalman tracking](kalman_tracking.aev) | Filter and smooth measured observations, reconstruct a differenced signal, and compute rolling statistics |
| [Measured state-space simulation](measured_state_space.aev) | Discretise a measured model and simulate its continuous and discrete responses |
| [Polynomial root bounds](polynomial_root_bounds.aev) | Bound polynomial roots and compute Koebe growth bounds on the unit disk |
| [Zero-phase filtering](zero_phase_filter.aev) | Smooth a measured pressure pulse while preserving sample positions and units |
| [Measured vector updates](measured_vector_updates.aev) | Use a velocity and elapsed times to update positions in one vector call |
| [Measured mesh](measured_mesh.aev) | Triangulate measured coordinates, compute areas and volumes, and extract tetrahedron boundary faces |
| [Combinatorial probabilities](combinatorial_probabilities.aev) | Use exact counts for log binomial and multinomial coefficients and compute a beta quantile |
| [Modular polynomial products](modular_polynomials.aev) | Convolve polynomial coefficients and inspect exact generators and Bézout coefficients |
| [Exact dyadic arithmetic](dyadic_real.aev) | Compare and compute with explicitly rounded binary multiprecision values |
| [Exact modular vectors](exact_modular_vectors.aev) | Use full-width residues, modular dot products, batch inverses and wide modular powers |
| [Wide congruences](wide_congruences.aev) | Solve a generalized CRT system whose period exceeds 128 bits |
| [Exact matrix normal form](exact_matrix_normal_form.aev) | Normalize measured coefficients and recover the two Smith transforms |
| [Polynomial calibration](polynomial_calibration.aev) | Fit a measured trajectory, compose it with a clock calibration, and evaluate a batch of coordinates |
| [Numerical guide](NUMERICAL_GUIDE.md) | Detailed RSSN examples, adaptive solvers, stochastic trajectories and measured models |
| [Natural units](natural.aev) and [RSSN operators](rssn.aev) | Natural-unit dimension checks and the typed kernel interface |

The [book](https://aevia.apich.org/book?lang=en) uses small programs in order and includes exercises for changing inputs and verifying results.
