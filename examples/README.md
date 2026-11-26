# Aevia examples

[简体中文](README.zh-CN.md)

Run a complete program with `aevia run examples/FILE.aev`. Use `aevia test examples/FILE.aev` for an example that contains assertions. The commands assume you are at the repository root.

| Start with | Learn |
|---|---|
| [Kinematics](kinematics.aev) and [units](units.aev) | Exact and approximate quantities, unit conversion and affine temperatures |
| [Tensors](tensors.aev) and [sparse tensors](sparse.aev) | Matrix products, reductions, storage formats |
| [Matrix operations](matrix_operations.aev) | Dense constructors (`fill`, `linspace`, `eye`, `diag`), `trace`, `tril`/`triu`, and `set` |
| [Calculus](calculus.aev) and [symbolic expressions](symbolic.aev) | Differentiation, integration, limits and equations |
| [Modules](modules/main.aev) | Imports across a two-file project |
| [Calibration](sensor_calibration.aev) and [relaxation fitting](relaxation_fit.aev) | Dimensioned residuals, quality thresholds and fitting a model |
| [Plotting sensor measurements](plot_series.aev) | Shared axes, error bars, histograms and SVG, HTML and animated GIF export |
| [Simulation I/O and visualization](sim_io_visualization.aev) | Damped oscillator CSV/NPY export and SVG, HTML & animated GIF visualization |
| [Parallel computation](parallel.aev) | Ordered mapping, reductions and worker tasks |
| [Numerical guide](NUMERICAL_GUIDE.md) | Detailed RSSN examples, adaptive solvers, stochastic trajectories and measured models |
| [Natural units](natural.aev) and [RSSN operators](rssn.aev) | Natural-unit dimension checks and the typed kernel interface |

The [book](https://aevia.apich.org/book?lang=en) uses small programs in order and includes exercises for changing inputs and verifying results.
