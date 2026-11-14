//! Partial differential equations (§8.10).
//!
//! §8.10 fixes the four entry points and says the rest of the type rules live
//! in the `Pde` module documentation, so this is the smallest honest reading:
//! a regular rectangular grid, a field on it, and the classical finite
//! difference scheme for each equation. A spec that the scheme cannot honour
//! (an unstable time step, a grid that does not match the field) is an error,
//! never a silently wrong field.
//!
//! Every solver uses zero Dirichlet data on the boundary, which is what "no
//! boundary condition given" means for a first implementation; a spec carries
//! its own initial or source field and the interior is what changes.

use crate::NumericError;

/// A regular rectangular grid with spacing `h` on both axes.
pub use rssn::sim::fdm_nd::AdvectionSpec;
/// A regular rectangular grid with spacing `h` on both axes.
pub use rssn::sim::fdm_nd::Boundary1D;
/// A regular rectangular grid with spacing `h` on both axes.
pub use rssn::sim::fdm_nd::Boundary2D;
/// A regular rectangular grid with spacing `h` on both axes.
pub use rssn::sim::fdm_nd::Boundary3D;
/// A regular rectangular grid with spacing `h` on both axes.
pub use rssn::sim::fdm_nd::BoundaryCondition;
/// A regular rectangular grid with spacing `h` on both axes.
pub use rssn::sim::fdm_nd::Field;
/// A regular rectangular grid with spacing `h` on both axes.
pub use rssn::sim::fdm_nd::Field1D;
/// A regular rectangular grid with spacing `h` on both axes.
pub use rssn::sim::fdm_nd::Field3D;
/// A regular rectangular grid with spacing `h` on both axes.
pub use rssn::sim::fdm_nd::Grid;
/// A regular rectangular grid with spacing `h` on both axes.
pub use rssn::sim::fdm_nd::Grid1D;
/// A regular rectangular grid with spacing `h` on both axes.
pub use rssn::sim::fdm_nd::Grid3D;
/// A regular rectangular grid with spacing `h` on both axes.
pub use rssn::sim::fdm_nd::HeatSpec;
/// A regular rectangular grid with spacing `h` on both axes.
pub use rssn::sim::fdm_nd::PoissonSpec;
/// A regular rectangular grid with spacing `h` on both axes.
pub use rssn::sim::fdm_nd::TimeStepping;
/// A regular rectangular grid with spacing `h` on both axes.
pub use rssn::sim::fdm_nd::WaveSpec;

fn map_fdm_err(err: rssn::sim::fdm_nd::FdmError) -> NumericError {
    match err {
        | rssn::sim::fdm_nd::FdmError::Empty => NumericError::Empty,
        | rssn::sim::fdm_nd::FdmError::ShapeMismatch(s) => NumericError::ShapeMismatch(s),
        | rssn::sim::fdm_nd::FdmError::Domain(s) => NumericError::Domain(s),
        | rssn::sim::fdm_nd::FdmError::Singular => NumericError::Singular,
        | rssn::sim::fdm_nd::FdmError::NotConverged => NumericError::NotConverged,
    }
}

pub fn solve_poisson(spec: &PoissonSpec) -> Result<Field, NumericError> {
    rssn::sim::fdm_nd::solve_poisson(spec).map_err(map_fdm_err)
}

pub fn solve_heat(spec: &HeatSpec) -> Result<Vec<Field>, NumericError> {
    rssn::sim::fdm_nd::solve_heat(spec).map_err(map_fdm_err)
}

pub fn solve_wave(spec: &WaveSpec) -> Result<Vec<Field>, NumericError> {
    rssn::sim::fdm_nd::solve_wave(spec).map_err(map_fdm_err)
}

pub fn solve_advection(spec: &AdvectionSpec) -> Result<Vec<Field>, NumericError> {
    rssn::sim::fdm_nd::solve_advection(spec).map_err(map_fdm_err)
}

pub fn solve_poisson_1d(
    grid: Grid1D,
    source: &[f64],
    bc: &Boundary1D,
) -> Result<Field1D, NumericError> {
    rssn::sim::fdm_nd::solve_poisson_1d(grid, source, bc).map_err(map_fdm_err)
}

pub fn solve_poisson_with_bc(
    spec: &PoissonSpec,
    bc: &Boundary2D,
) -> Result<Field, NumericError> {
    rssn::sim::fdm_nd::solve_poisson_with_bc(spec, bc).map_err(map_fdm_err)
}

pub fn solve_poisson_3d(
    grid: Grid3D,
    source: &[f64],
    bc: &Boundary3D,
    iterations: usize,
    tolerance: f64,
) -> Result<Field3D, NumericError> {
    rssn::sim::fdm_nd::solve_poisson_3d(grid, source, bc, iterations, tolerance)
        .map_err(map_fdm_err)
}

pub fn solve_heat_1d(
    grid: Grid1D,
    alpha: f64,
    dt: f64,
    steps: usize,
    initial: &[f64],
    bc: &Boundary1D,
    stepping: TimeStepping,
) -> Result<Vec<Field1D>, NumericError> {
    rssn::sim::fdm_nd::solve_heat_1d(grid, alpha, dt, steps, initial, bc, stepping)
        .map_err(map_fdm_err)
}

pub fn solve_heat_with_bc(
    spec: &HeatSpec,
    bc: &Boundary2D,
    stepping: TimeStepping,
) -> Result<Vec<Field>, NumericError> {
    rssn::sim::fdm_nd::solve_heat_with_bc(spec, bc, stepping).map_err(map_fdm_err)
}

pub fn solve_heat_3d(
    grid: Grid3D,
    alpha: f64,
    dt: f64,
    steps: usize,
    initial: &[f64],
    bc: &Boundary3D,
    stepping: TimeStepping,
) -> Result<Vec<Field3D>, NumericError> {
    rssn::sim::fdm_nd::solve_heat_3d(grid, alpha, dt, steps, initial, bc, stepping)
        .map_err(map_fdm_err)
}

pub fn solve_wave_1d(
    grid: Grid1D,
    c: f64,
    dt: f64,
    steps: usize,
    initial: &[f64],
    velocity: &[f64],
    bc: &Boundary1D,
) -> Result<Vec<Field1D>, NumericError> {
    rssn::sim::fdm_nd::solve_wave_1d(grid, c, dt, steps, initial, velocity, bc).map_err(map_fdm_err)
}

pub fn solve_wave_with_bc(
    spec: &WaveSpec,
    bc: &Boundary2D,
) -> Result<Vec<Field>, NumericError> {
    rssn::sim::fdm_nd::solve_wave_with_bc(spec, bc).map_err(map_fdm_err)
}

pub fn solve_wave_3d(
    grid: Grid3D,
    c: f64,
    dt: f64,
    steps: usize,
    initial: &[f64],
    velocity: &[f64],
) -> Result<Vec<Field3D>, NumericError> {
    rssn::sim::fdm_nd::solve_wave_3d(grid, c, dt, steps, initial, velocity).map_err(map_fdm_err)
}

/// Solve the 3D wave equation with arbitrary supported face conditions.
pub fn solve_wave_3d_with_bc(
    grid: Grid3D,
    c: f64,
    dt: f64,
    steps: usize,
    initial: &[f64],
    velocity: &[f64],
    bc: &Boundary3D,
) -> Result<Vec<Field3D>, NumericError> {
    rssn::sim::fdm_nd::solve_wave_3d_with_bc(grid, c, dt, steps, initial, velocity, bc)
        .map_err(map_fdm_err)
}

pub fn solve_advection_1d(
    grid: Grid1D,
    v: f64,
    dt: f64,
    steps: usize,
    initial: &[f64],
    bc: &Boundary1D,
) -> Result<Vec<Field1D>, NumericError> {
    rssn::sim::fdm_nd::solve_advection_1d(grid, v, dt, steps, initial, bc).map_err(map_fdm_err)
}

pub fn solve_advection_with_bc(
    spec: &AdvectionSpec,
    bc: &Boundary2D,
) -> Result<Vec<Field>, NumericError> {
    rssn::sim::fdm_nd::solve_advection_with_bc(spec, bc).map_err(map_fdm_err)
}

pub fn solve_advection_3d(
    grid: Grid3D,
    velocity: (f64, f64, f64),
    dt: f64,
    steps: usize,
    initial: &[f64],
    bc: &Boundary3D,
) -> Result<Vec<Field3D>, NumericError> {
    rssn::sim::fdm_nd::solve_advection_3d(grid, velocity, dt, steps, initial, bc)
        .map_err(map_fdm_err)
}

pub use rssn::sim::pde_api::FemDegree;
pub use rssn::sim::pde_api::FemSolution;
pub use rssn::sim::pde_api::FvmScheme;

/// Solves the 1D reaction-diffusion BVP `-(k u_x)_x + c u = f(x)` on `[x0, x1]`
/// with `nx` simplex intervals using `P1` or `P2` Lagrange finite elements
/// (`rssn::sim::fem_simplex`) and boundary conditions `bc` (Dirichlet, Neumann, or Robin).
///
/// # Errors
///
/// Returns [`NumericError::Domain`] if the mesh, coefficients, or BCs are invalid.
pub fn solve_fem_1d<F>(
    bounds: (f64, f64),
    nx: usize,
    degree: FemDegree,
    k: f64,
    c: f64,
    source: F,
    bc: &Boundary1D,
) -> Result<FemSolution, NumericError>
where
    F: Fn(f64) -> f64 + Sync,
{
    rssn::sim::pde_api::solve_fem_1d(bounds, nx, degree, k, c, source, bc).map_err(map_fdm_err)
}

/// Solves the 2D elliptic PDE `-∇·(k ∇u) + c u = f(x, y)` on `[x0, x1] × [y0, y1]`
/// triangulated into `2 * nx * ny` triangles using `P1` or `P2` Lagrange finite
/// elements (`rssn::sim::fem_simplex`) and per-face boundary conditions `bc`
/// (Dirichlet, Neumann, or Robin).
///
/// # Errors
///
/// Returns [`NumericError::Domain`] if the mesh, coefficients, or BCs are invalid.
#[allow(clippy::too_many_arguments)] // Preserve the kernel adapter's explicit geometry and controls.
pub fn solve_fem_2d<F>(
    bounds: (f64, f64, f64, f64),
    nx: usize,
    ny: usize,
    degree: FemDegree,
    k: f64,
    c: f64,
    source: F,
    bc: &Boundary2D,
) -> Result<FemSolution, NumericError>
where
    F: Fn(f64, f64) -> f64 + Sync,
{
    rssn::sim::pde_api::solve_fem_2d(bounds, nx, ny, degree, k, c, source, bc).map_err(map_fdm_err)
}

/// Solves the 3D elliptic PDE `-∇·(k ∇u) + c u = f(x, y, z)` on a tetrahedral
/// box mesh `[lo, hi]` with `cells = [nx, ny, nz]` using `P1` or `P2` Lagrange
/// finite elements (`rssn::sim::fem_simplex`) and per-face boundary conditions `bc`.
///
/// # Errors
///
/// Returns [`NumericError::Domain`] if the mesh, coefficients, or BCs are invalid.
#[allow(clippy::too_many_arguments)] // Preserve the kernel adapter's explicit geometry and controls.
pub fn solve_fem_3d<F>(
    lo: [f64; 3],
    hi: [f64; 3],
    cells: [usize; 3],
    degree: FemDegree,
    k: f64,
    c: f64,
    source: F,
    bc: &Boundary3D,
) -> Result<FemSolution, NumericError>
where
    F: Fn(f64, f64, f64) -> f64 + Sync,
{
    rssn::sim::pde_api::solve_fem_3d(lo, hi, cells, degree, k, c, source, bc).map_err(map_fdm_err)
}

/// Solves a 2D elliptic PDE `-∇·(k ∇u) + c u = f(x, y)` on a triangle mesh with
/// adaptive Dörfler residual-based mesh refinement (`rssn::sim::fem_triangles`).
///
/// # Errors
///
/// Returns [`NumericError::Domain`] if the mesh or parameters are invalid.
#[allow(clippy::too_many_arguments)] // Preserve the kernel adapter's explicit geometry and controls.
pub fn solve_fem_adaptive_2d<F, G>(
    bounds: (f64, f64, f64, f64),
    nx: usize,
    ny: usize,
    k: f64,
    c: f64,
    source: F,
    dirichlet_bc: G,
    refinement_levels: usize,
    dorfler_theta: f64,
) -> Result<FemSolution, NumericError>
where
    F: Fn(f64, f64) -> f64,
    G: Fn(f64, f64) -> f64,
{
    rssn::sim::pde_api::solve_fem_adaptive_2d(
        bounds,
        nx,
        ny,
        k,
        c,
        source,
        dirichlet_bc,
        refinement_levels,
        dorfler_theta,
    )
    .map_err(map_fdm_err)
}

/// Solves a 1D scalar conservation law `u_t + f(u)_x = 0` on `num_cells` finite
/// volumes covering `[0, domain_size]` using `scheme` (`Upwind`, `LaxFriedrichs`,
/// `MusclMinmod`, or `MusclVanLeer`, leveraging `rssn::sim::physics_fvm` limiters
/// and fluxes) with periodic or transmissive boundary conditions.
///
/// Returns `(cell_centers, cell_averages)` at `t = steps * dt`.
///
/// # Errors
///
/// Returns [`NumericError::Domain`] for invalid grid, time step, or non-finite fluxes.
#[allow(clippy::too_many_arguments)] // Preserve the kernel adapter's explicit geometry and controls.
pub fn solve_fvm_conservation_1d<IC, FL, DFL>(
    num_cells: usize,
    domain_size: f64,
    dt: f64,
    steps: usize,
    periodic: bool,
    scheme: FvmScheme,
    initial: IC,
    flux: FL,
    max_wave_speed: DFL,
) -> Result<(Vec<f64>, Vec<f64>), NumericError>
where
    IC: Fn(f64) -> f64,
    FL: Fn(f64) -> f64,
    DFL: Fn(f64) -> f64,
{
    rssn::sim::pde_api::solve_fvm_conservation_1d(
        num_cells,
        domain_size,
        dt,
        steps,
        periodic,
        scheme,
        initial,
        flux,
        max_wave_speed,
    )
    .map_err(map_fdm_err)
}

/// Solves the 1D inviscid Burgers' equation `u_t + (u^2 / 2)_x = 0` using
/// `rssn::sim::physics_fvm::solve_burgers_1d` or high-resolution MUSCL-Hancock FVM.
///
/// # Errors
///
/// Returns [`NumericError::Domain`] for invalid grid or step parameters.
pub fn solve_fvm_burgers_1d<IC>(
    num_cells: usize,
    domain_size: f64,
    dt: f64,
    steps: usize,
    periodic: bool,
    scheme: FvmScheme,
    initial: IC,
) -> Result<(Vec<f64>, Vec<f64>), NumericError>
where
    IC: Fn(f64) -> f64,
{
    rssn::sim::pde_api::solve_fvm_burgers_1d(
        num_cells,
        domain_size,
        dt,
        steps,
        periodic,
        scheme,
        initial,
    )
    .map_err(map_fdm_err)
}

/// Solves the 1D Shallow Water Equations (`h_t + (hu)_x = 0`, `(hu)_t + (hu^2 + g h^2 / 2)_x = 0`)
/// using `rssn::sim::physics_fvm::solve_shallow_water_1d`.
///
/// Returns `(height_h, momentum_hu)` at `t = steps * dt`.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] if `initial_h.len() != initial_hu.len()`,
/// [`NumericError::Domain`] for non-positive parameters.
pub fn solve_fvm_shallow_water_1d(
    initial_h: &[f64],
    initial_hu: &[f64],
    dx: f64,
    dt: f64,
    steps: usize,
    gravity: f64,
) -> Result<(Vec<f64>, Vec<f64>), NumericError> {
    rssn::sim::pde_api::solve_fvm_shallow_water_1d(initial_h, initial_hu, dx, dt, steps, gravity)
        .map_err(map_fdm_err)
}

/// Solves 1D linear advection `u_t + a u_x = 0` using Nodal Discontinuous Galerkin
/// (`rssn::sim::dg::solve_dg_1d`) of polynomial degree `poly_degree >= 1` on `n_elem`
/// elements over `[x0, x1]` with periodic boundary conditions.
///
/// Returns `(lgl_nodes_x, nodal_values)` at `t = t_end`.
///
/// # Errors
///
/// Returns [`NumericError::Domain`] if parameters are invalid.
pub fn solve_dg_advection_1d<IC>(
    bounds: (f64, f64),
    n_elem: usize,
    poly_degree: usize,
    velocity: f64,
    t_end: f64,
    cfl: f64,
    initial: IC,
) -> Result<(Vec<f64>, Vec<f64>), NumericError>
where
    IC: Fn(f64) -> f64,
{
    rssn::sim::pde_api::solve_dg_advection_1d(
        bounds,
        n_elem,
        poly_degree,
        velocity,
        t_end,
        cfl,
        initial,
    )
    .map_err(map_fdm_err)
}

/// Solves the periodic Helmholtz / screened Poisson equation `-Δu + σ u = f`
/// in 1D, 2D, or 3D on a periodic box of lengths `lengths` and power-of-two grid
/// sizes `extents` using Fourier pseudospectral inversion (`rssn::sim::spectral::SpectralGrid`).
///
/// For `σ == 0.0` (pure periodic Poisson `-Δu = f`), the source `f` must have zero mean
/// and the returned solution has zero mean.
///
/// # Errors
///
/// [`NumericError::Domain`] if extents are not powers of two (`>= 4`) or if `σ == 0`
/// with non-zero mean source, [`NumericError::ShapeMismatch`] if `source.len()` does not match.
pub fn solve_spectral_helmholtz(
    extents: &[usize],
    lengths: &[f64],
    sigma: f64,
    source: &[f64],
) -> Result<Vec<f64>, NumericError> {
    rssn::sim::pde_api::solve_spectral_helmholtz(extents, lengths, sigma, source)
        .map_err(map_fdm_err)
}

/// Integrates a periodic reaction-diffusion or heat equation
/// `u_t = \alpha Δu - \sigma u + N(t, u)` in 1D, 2D, or 3D on a periodic Fourier
/// pseudospectral grid using 4th-order Exponential Time Differencing Runge–Kutta
/// (`rssn::sim::spectral::etdrk4`) with 2/3 dealiasing on the nonlinear term.
///
/// # Errors
///
/// [`NumericError::Domain`] for invalid grid extents or time span,
/// [`NumericError::ShapeMismatch`] if `initial.len()` does not match the grid size.
#[allow(clippy::too_many_arguments)] // Preserve the kernel adapter's explicit geometry and controls.
pub fn solve_spectral_etdrk4<NL>(
    extents: &[usize],
    lengths: &[f64],
    alpha: f64,
    sigma: f64,
    t_end: f64,
    steps: usize,
    initial: &[f64],
    nonlinear_phys: NL,
) -> Result<Vec<f64>, NumericError>
where
    NL: Fn(f64, &[f64]) -> Vec<f64> + 'static,
{
    rssn::sim::pde_api::solve_spectral_etdrk4(
        extents,
        lengths,
        alpha,
        sigma,
        t_end,
        steps,
        initial,
        nonlinear_phys,
    )
    .map_err(map_fdm_err)
}

/// Solves the 1D/2D/3D Helmholtz / Poisson equation `-Δu + σ u = f(x)` on a bounded
/// box `[lo, hi]` using Chebyshev–Gauss–Lobatto spectral collocation
/// (`rssn::sim::spectral::ChebGrid::solve_helmholtz`) with Dirichlet, Neumann, or
/// Robin boundary conditions.
///
/// Returns `(chebyshev_node_coords, nodal_values)`.
///
/// # Errors
///
/// Returns [`NumericError::Domain`] for invalid degrees (`< 2`), bounds, or BCs.
pub fn solve_chebyshev_helmholtz<F>(
    degrees: &[usize],
    lo: &[f64],
    hi: &[f64],
    sigma: f64,
    source: F,
    bc: &BoundaryCondition,
) -> Result<(Vec<[f64; 3]>, Vec<f64>), NumericError>
where
    F: Fn([f64; 3]) -> f64,
{
    rssn::sim::pde_api::solve_chebyshev_helmholtz(degrees, lo, hi, sigma, source, bc)
        .map_err(map_fdm_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(
        a: f64,
        b: f64,
        tolerance: f64,
    ) -> bool {
        (a - b).abs() <= tolerance
    }

    #[test]
    fn poisson_recovers_a_manufactured_solution() {
        // u = sin(pi x) sin(pi y) has lap(u) = -2 pi^2 u.
        let nx = 21;
        let ny = 21;
        let h = 1.0 / (nx as f64 - 1.0);
        let mut source = vec![0.0; nx * ny];
        let mut expected = vec![0.0; nx * ny];
        for y in 0..ny {
            for x in 0..nx {
                let (px, py) = (x as f64 * h, y as f64 * h);
                let value = (std::f64::consts::PI * px).sin() * (std::f64::consts::PI * py).sin();
                expected[y * nx + x] = value;
                source[y * nx + x] = -2.0 * std::f64::consts::PI.powi(2) * value;
            }
        }
        let field = solve_poisson(&PoissonSpec {
            grid: Grid { nx, ny, h },
            source,
            iterations: 20_000,
            tolerance: 1e-10,
        })
        .unwrap();
        // The discretisation error is O(h^2) ~ 2.5e-3.
        let worst = field
            .values
            .iter()
            .zip(expected.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f64, f64::max);
        assert!(worst < 0.01, "worst deviation {worst}");
    }

    #[test]
    fn heat_smooths_a_hot_spot() {
        let nx = 11;
        let ny = 11;
        let h = 0.1;
        let mut initial = vec![0.0; nx * ny];
        initial[5 * nx + 5] = 100.0;
        let snapshots = solve_heat(&HeatSpec {
            grid: Grid { nx, ny, h },
            alpha: 1.0,
            steps: 5,
            dt: 0.001,
            initial,
        })
        .unwrap();
        assert_eq!(snapshots.len(), 6);
        let centre = 5 * nx + 5;
        assert!(snapshots[5].values[centre] < snapshots[0].values[centre]);
    }

    #[test]
    fn an_unstable_heat_step_is_refused() {
        let nx = 5;
        let outcome = solve_heat(&HeatSpec {
            grid: Grid { nx, ny: nx, h: 0.1 },
            alpha: 1.0,
            steps: 1,
            dt: 1.0,
            initial: vec![0.0; nx * nx],
        });
        assert!(matches!(outcome, Err(NumericError::Domain(_))));
    }

    #[test]
    fn a_wave_pulse_travels() {
        let nx = 41;
        let ny = 5;
        let h = 0.05;
        let mut initial = vec![0.0; nx * ny];
        for y in 0..ny {
            for x in 15..20 {
                initial[y * nx + x] = 1.0;
            }
        }
        let snapshots = solve_wave(&WaveSpec {
            grid: Grid { nx, ny, h },
            c: 1.0,
            steps: 10,
            dt: 0.02,
            initial: initial.clone(),
            velocity: vec![0.0; nx * ny],
        })
        .unwrap();
        // The pulse splits into two halves that move apart.
        let left: f64 = snapshots[10]
            .values
            .iter()
            .enumerate()
            .filter(|(index, _)| index % nx < 15)
            .map(|(_, value)| value)
            .sum();
        assert!(left.abs() > 0.0);
    }

    #[test]
    fn advection_transports_a_bump() {
        let nx = 41;
        let ny = 5;
        let h = 0.05;
        let mut initial = vec![0.0; nx * ny];
        for y in 0..ny {
            for x in 5..10 {
                initial[y * nx + x] = 1.0;
            }
        }
        let snapshots = solve_advection(&AdvectionSpec {
            grid: Grid { nx, ny, h },
            vx: 1.0,
            vy: 0.0,
            steps: 10,
            dt: 0.01,
            initial,
        })
        .unwrap();
        let first = snapshots[0].values[2 * nx + 7];
        let later = snapshots[10].values[2 * nx + 9];
        assert!(close(first, 1.0, 1e-12));
        assert!(later > 0.0);
    }

    #[test]
    fn a_grid_of_two_points_is_refused() {
        let outcome = solve_poisson(&PoissonSpec {
            grid: Grid { nx: 2, ny: 2, h: 0.1 },
            source: vec![0.0; 4],
            iterations: 10,
            tolerance: 1e-8,
        });
        assert!(matches!(outcome, Err(NumericError::Domain(_))));
    }

    #[test]
    fn poisson_1d_2d_3d_support_nonzero_dirichlet_neumann_robin_and_periodic_bcs() {
        // 1. 1D Poisson with non-zero Dirichlet & Neumann:
        // u(x) = x^2 + 1 on [0, 1] => u''(x) = 2, u(0) = 1 (Dirichlet), u'(1) = 2 (outward Neumann at x=1).
        let nx = 51;
        let h = 1.0 / (nx - 1) as f64;
        let grid1d = Grid1D { nx, h };
        let src1d = vec![2.0; nx];
        let bc1d = Boundary1D {
            left: BoundaryCondition::Dirichlet(1.0),
            right: BoundaryCondition::Neumann(2.0),
        };
        let sol1d = solve_poisson_1d(grid1d, &src1d, &bc1d).unwrap();
        for (i, &val) in sol1d.values.iter().enumerate() {
            let x = i as f64 * h;
            let exact = x * x + 1.0;
            assert!((val - exact).abs() < 1e-10, "i={i}: {val} vs {exact}");
        }

        // 2. 1D Poisson with Robin BC at right endpoint:
        // u(x) = x^2 on [0, 1] => u(0) = 0, and at x=1: u(1) + u'(1) = 1 + 2 = 3 (alpha=1, beta=1, gamma=3).
        let bc_robin = Boundary1D {
            left: BoundaryCondition::Dirichlet(0.0),
            right: BoundaryCondition::Robin {
                alpha: 1.0,
                beta: 1.0,
                gamma: 3.0,
            },
        };
        let sol_robin = solve_poisson_1d(grid1d, &src1d, &bc_robin).unwrap();
        assert!((sol_robin.values[nx - 1] - 1.0).abs() < 1e-10);

        // 3. 2D Poisson with non-zero Dirichlet BC: u(x, y) = x + 2*y (harmonic: lap(u) = 0)
        let n2 = 11;
        let h2 = 1.0 / (n2 - 1) as f64;
        let g2 = Grid {
            nx: n2,
            ny: n2,
            h: h2,
        };
        let bc2d = Boundary2D {
            left: BoundaryCondition::DirichletProfile(
                (0..n2).map(|y| 2.0 * y as f64 * h2).collect(),
            ),
            right: BoundaryCondition::DirichletProfile(
                (0..n2).map(|y| 1.0 + 2.0 * y as f64 * h2).collect(),
            ),
            bottom: BoundaryCondition::DirichletProfile((0..n2).map(|x| x as f64 * h2).collect()),
            top: BoundaryCondition::DirichletProfile(
                (0..n2).map(|x| x as f64 * h2 + 2.0).collect(),
            ),
        };
        let sol2d = solve_poisson_with_bc(
            &PoissonSpec {
                grid: g2,
                source: vec![0.0; n2 * n2],
                iterations: 5000,
                tolerance: 1e-10,
            },
            &bc2d,
        )
        .unwrap();
        for y in 0..n2 {
            for x in 0..n2 {
                let exact = x as f64 * h2 + 2.0 * y as f64 * h2;
                assert!((sol2d.values[y * n2 + x] - exact).abs() < 1e-7);
            }
        }

        // 4. 3D Poisson on a 9x9x9 cube: u = sin(pi x) sin(pi y) sin(pi z), lap(u) = -3 pi^2 u
        let n3 = 9;
        let h3 = 1.0 / (n3 - 1) as f64;
        let g3 = Grid3D {
            nx: n3,
            ny: n3,
            nz: n3,
            h: h3,
        };
        let pi = std::f64::consts::PI;
        let mut src3 = vec![0.0; g3.len()];
        let mut exact3 = vec![0.0; g3.len()];
        for z in 0..n3 {
            for y in 0..n3 {
                for x in 0..n3 {
                    let v = (pi * x as f64 * h3).sin()
                        * (pi * y as f64 * h3).sin()
                        * (pi * z as f64 * h3).sin();
                    exact3[g3.index(x, y, z)] = v;
                    src3[g3.index(x, y, z)] = -3.0 * pi * pi * v;
                }
            }
        }
        let sol3 = solve_poisson_3d(
            g3,
            &src3,
            &Boundary3D::uniform(BoundaryCondition::zero_dirichlet()),
            3000,
            1e-9,
        )
        .unwrap();
        let err3 = sol3
            .values
            .iter()
            .zip(&exact3)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f64, f64::max);
        assert!(err3 < 0.03, "3D Poisson error: {err3}");
    }

    #[test]
    fn implicit_crank_nicolson_and_adi_heat_solvers_are_unconditionally_stable() {
        // Choose dt = 0.05 on h = 0.05 with alpha = 1.0 => r = alpha*dt/h^2 = 20.0 >> 0.25!
        // Explicit Forward Euler is rejected immediately, while BackwardEuler,
        // CrankNicolson, and ADI remain unconditionally stable and accurate!
        let nx = 21;
        let ny = 21;
        let h = 1.0 / (nx - 1) as f64;
        let grid = Grid { nx, ny, h };
        let pi = std::f64::consts::PI;
        let mut initial = vec![0.0; nx * ny];
        for y in 0..ny {
            for x in 0..nx {
                initial[y * nx + x] = (pi * x as f64 * h).sin() * (pi * y as f64 * h).sin();
            }
        }
        let spec = HeatSpec {
            grid,
            alpha: 0.1,
            steps: 20,
            dt: 0.05, // r = 0.1 * 0.05 / 0.0025 = 2.0 > 0.25 (8x above explicit limit!)
            initial,
        };
        let bc = Boundary2D::uniform(BoundaryCondition::zero_dirichlet());

        assert!(solve_heat_with_bc(&spec, &bc, TimeStepping::Explicit).is_err());

        let adi_snaps = solve_heat_with_bc(&spec, &bc, TimeStepping::Adi).unwrap();
        let t_end = spec.steps as f64 * spec.dt;
        let decay = (-2.0 * pi * pi * spec.alpha * t_end).exp();
        let center = (ny / 2) * nx + (nx / 2);
        let adi_center = adi_snaps.last().unwrap().values[center];
        assert!(
            (adi_center - decay).abs() < 5e-3,
            "ADI center {adi_center} vs exact {decay}"
        );

        // Also test 1D Crank-Nicolson with insulating Neumann BCs (conserves total thermal mass!)
        let g1 = Grid1D { nx: 21, h: 0.05 };
        let init1: Vec<f64> = (0..21)
            .map(|i| (pi * i as f64 * 0.05).cos() + 2.0)
            .collect();
        let snaps1 = solve_heat_1d(
            g1,
            1.0,
            0.05, // r = 20 >> 0.5
            10,
            &init1,
            &Boundary1D::uniform(BoundaryCondition::zero_neumann()),
            TimeStepping::CrankNicolson,
        )
        .unwrap();
        // Trapezoidal rule integral of u is conserved under homogeneous Neumann BCs
        let trapz = |v: &[f64]| -> f64 {
            0.5 * v[0] + v[1..v.len() - 1].iter().sum::<f64>() + 0.5 * v[v.len() - 1]
        };
        let m0 = trapz(&snaps1[0].values);
        let m_end = trapz(&snaps1.last().unwrap().values);
        assert!((m_end - m0).abs() < 1e-10, "mass {m0} vs {m_end}");

        // Also test 3D ADI heat solver at r = 0.72 >> 1/6 on smooth 3D eigenmode
        let g3 = Grid3D {
            nx: 7,
            ny: 7,
            nz: 7,
            h: 1.0 / 6.0,
        };
        let mut init3 = vec![0.0; g3.len()];
        for z in 0..g3.nz {
            for y in 0..g3.ny {
                for x in 0..g3.nx {
                    let (px, py, pz) = (x as f64 * g3.h, y as f64 * g3.h, z as f64 * g3.h);
                    init3[g3.index(x, y, z)] = (pi * px).sin() * (pi * py).sin() * (pi * pz).sin();
                }
            }
        }
        let snaps3 = solve_heat_3d(
            g3,
            1.0,
            0.02, // r = 0.72 > 1/6 (explicit limit)
            5,
            &init3,
            &Boundary3D::uniform(BoundaryCondition::zero_dirichlet()),
            TimeStepping::Adi,
        )
        .unwrap();
        let last3 = snaps3.last().unwrap();
        assert!(last3.values.iter().all(|v| v.is_finite() && *v >= -1e-12));
        let center3 = last3.at(3, 3, 3);
        assert!(center3 > 0.0 && center3 < 1.0);
    }

    #[test]
    fn fem_1d_2d_3d_and_adaptive_solvers_converge_to_exact_solutions() {
        // 1. 1D P2 FEM is exact (up to quadrature/solver tolerance) for quadratic u(x) = x * (1 - x):
        // -u''(x) = 2 on [0, 1], u(0) = 0, u(1) = 0.
        let fem1 = solve_fem_1d(
            (0.0, 1.0),
            8,
            FemDegree::P2,
            1.0,
            0.0,
            |_x| 2.0,
            &Boundary1D::default(),
        )
        .unwrap();
        let err1 = fem1.max_nodal_error(|[x, _, _]| x * (1.0 - x));
        assert!(err1 < 1e-11, "1D P2 FEM error on quadratic: {err1}");

        // 2. 2D P2 FEM on [0, 1]^2: u(x, y) = sin(pi x) sin(pi y), -lap(u) = 2 pi^2 u
        let pi = std::f64::consts::PI;
        let fem2 = solve_fem_2d(
            (0.0, 1.0, 0.0, 1.0),
            6,
            6,
            FemDegree::P2,
            1.0,
            0.0,
            |x, y| 2.0 * pi * pi * (pi * x).sin() * (pi * y).sin(),
            &Boundary2D::default(),
        )
        .unwrap();
        let err2 = fem2.max_nodal_error(|[x, y, _]| (pi * x).sin() * (pi * y).sin());
        assert!(err2 < 2e-3, "2D P2 FEM error: {err2}");

        // 3. 3D P2 FEM on [0, 1]^3 for manufactured solution u = sin(pi x) sin(pi y) sin(pi z)
        let fem3 = solve_fem_3d(
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [3, 3, 3],
            FemDegree::P2,
            1.0,
            0.0,
            |x, y, z| 3.0 * pi * pi * (pi * x).sin() * (pi * y).sin() * (pi * z).sin(),
            &Boundary3D::default(),
        )
        .unwrap();
        let err3 =
            fem3.max_nodal_error(|[x, y, z]| (pi * x).sin() * (pi * y).sin() * (pi * z).sin());
        assert!(err3 < 0.03, "3D P2 FEM error: {err3}");

        // 4. 2D Adaptive FEM with Dörfler refinement reduces error across refinement levels
        let fem_coarse = solve_fem_adaptive_2d(
            (0.0, 1.0, 0.0, 1.0),
            4,
            4,
            1.0,
            0.0,
            |x, y| 2.0 * pi * pi * (pi * x).sin() * (pi * y).sin(),
            |_x, _y| 0.0,
            0,
            0.5,
        )
        .unwrap();
        let fem_adap = solve_fem_adaptive_2d(
            (0.0, 1.0, 0.0, 1.0),
            4,
            4,
            1.0,
            0.0,
            |x, y| 2.0 * pi * pi * (pi * x).sin() * (pi * y).sin(),
            |_x, _y| 0.0,
            3,
            0.6,
        )
        .unwrap();
        let err_coarse = fem_coarse.max_nodal_error(|[x, y, _]| (pi * x).sin() * (pi * y).sin());
        let err_adap = fem_adap.max_nodal_error(|[x, y, _]| (pi * x).sin() * (pi * y).sin());
        assert!(fem_adap.values.len() > fem_coarse.values.len());
        assert!(
            err_adap < err_coarse && err_adap < 0.03,
            "2D adaptive FEM error {err_adap} vs coarse {err_coarse}"
        );
    }

    #[test]
    fn fvm_dg_and_spectral_solvers_conserve_mass_and_achieve_spectral_accuracy() {
        let pi = std::f64::consts::PI;

        // 1. FVM 1D Burgers with MUSCL Van Leer limiter (conserves total integral of u)
        let (_xs, u_burg) =
            solve_fvm_burgers_1d(64, 2.0 * pi, 0.01, 20, true, FvmScheme::MusclVanLeer, |x| {
                x.sin() + 0.5
            })
            .unwrap();
        let mean_u: f64 = u_burg.iter().sum::<f64>() / u_burg.len() as f64;
        assert!(
            (mean_u - 0.5).abs() < 1e-12,
            "FVM conservation drift: {mean_u}"
        );

        // 2. 1D Nodal Discontinuous Galerkin (DG, degree p=3) on periodic advection
        let (xs_dg, u_dg) = solve_dg_advection_1d(
            (0.0, 2.0 * pi),
            16,
            3,
            1.0,
            2.0 * pi, // one full period!
            0.3,
            |x| x.sin(),
        )
        .unwrap();
        let dg_err = xs_dg
            .iter()
            .zip(&u_dg)
            .map(|(&x, &u)| (u - x.sin()).abs())
            .fold(0.0f64, f64::max);
        assert!(dg_err < 1e-3, "DG p=3 full-period error: {dg_err}");

        // 3. Fourier Pseudospectral Helmholtz in 2D: -lap(u) + u = f on [0, 2*pi]^2
        // Exact u(x, y) = sin(x) cos(2y) => (-lap + 1) u = (1 + 4 + 1) u = 6 u.
        let n = 16;
        let l = 2.0 * pi;
        let mut src_spec = Vec::with_capacity(n * n);
        let mut exact_spec = Vec::with_capacity(n * n);
        for j in 0..n {
            for i in 0..n {
                let x = i as f64 * l / n as f64;
                let y = j as f64 * l / n as f64;
                let val = x.sin() * (2.0 * y).cos();
                exact_spec.push(val);
                src_spec.push(6.0 * val);
            }
        }
        let u_spec = solve_spectral_helmholtz(&[n, n], &[l, l], 1.0, &src_spec).unwrap();
        let spec_err = u_spec
            .iter()
            .zip(&exact_spec)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f64, f64::max);
        assert!(
            spec_err < 1e-12,
            "Fourier spectral Helmholtz error: {spec_err}"
        );

        // 4. Fourier ETDRK4 reaction-diffusion: u_t = u_xx (exact decay exp(-t) sin(x))
        let init_1d: Vec<f64> = (0..16).map(|i| (i as f64 * l / 16.0).sin()).collect();
        let u_etd = solve_spectral_etdrk4(&[16], &[l], 1.0, 0.0, 0.5, 20, &init_1d, |_t, u| {
            vec![0.0; u.len()]
        })
        .unwrap();
        let decay = (-0.5f64).exp();
        for (i, &val) in u_etd.iter().enumerate() {
            let exact = decay * init_1d[i];
            assert!((val - exact).abs() < 1e-12);
        }

        // 5. Chebyshev spectral collocation on [0, 1]: -u'' = pi^2 sin(pi x), u(0)=u(1)=0
        let (coords, u_cheb) = solve_chebyshev_helmholtz(
            &[12],
            &[0.0],
            &[1.0],
            0.0,
            |[x, _, _]| pi * pi * (pi * x).sin(),
            &BoundaryCondition::zero_dirichlet(),
        )
        .unwrap();
        let cheb_err = coords
            .iter()
            .zip(&u_cheb)
            .map(|(&[x, _, _], &u)| (u - (pi * x).sin()).abs())
            .fold(0.0f64, f64::max);
        assert!(
            cheb_err < 1e-10,
            "Chebyshev collocation degree 12 error: {cheb_err}"
        );
    }
    #[test]
    fn three_dimensional_heat_methods_have_their_actual_discrete_amplification() {
        let grid = Grid3D {
            nx: 4,
            ny: 4,
            nz: 4,
            h: 1.0,
        };
        let initial: Vec<_> = (0..64)
            .map(|i| {
                let phase = std::f64::consts::FRAC_PI_2;
                (phase * (i % 4) as f64).cos()
                    * (phase * ((i / 4) % 4) as f64).cos()
                    * (phase * (i / 16) as f64).cos()
            })
            .collect();
        for (method, dt, expected) in [
            (TimeStepping::Explicit, 0.1, 0.4),
            (TimeStepping::BackwardEuler, 1.0, 1.0 / 7.0),
            (TimeStepping::CrankNicolson, 1.0, -0.5),
            (TimeStepping::CrankNicolson, 2.0, -5.0 / 7.0),
            (TimeStepping::Adi, 1.0, 0.25),
        ] {
            let out = solve_heat_3d(
                grid,
                1.0,
                dt,
                1,
                &initial,
                &Boundary3D::uniform(BoundaryCondition::Periodic),
                method,
            )
            .unwrap();
            for (actual, original) in out[1].values.iter().zip(&initial) {
                assert!(
                    (actual - expected * original).abs() < 1e-8,
                    "{method:?}: {actual}"
                );
            }
        }
    }

    #[test]
    fn three_dimensional_poisson_enforces_mixed_neumann_boundary_data() {
        let grid = Grid3D {
            nx: 4,
            ny: 4,
            nz: 4,
            h: 1.0 / 3.0,
        };
        let mut bc = Boundary3D::uniform(BoundaryCondition::Neumann(0.0));
        bc.x_min = BoundaryCondition::Dirichlet(0.0);
        bc.x_max = BoundaryCondition::Neumann(1.0);
        let solution = solve_poisson_3d(grid, &[0.0; 64], &bc, 5000, 1e-10).unwrap();
        for i in 0..64 {
            assert!((solution.values[i] - (i % 4) as f64 / 3.0).abs() < 1e-7);
        }
    }
    #[test]
    fn three_dimensional_transport_updates_outflow_faces_and_periodic_wave_nodes() {
        let grid = Grid3D {
            nx: 4,
            ny: 4,
            nz: 4,
            h: 1.0 / 3.0,
        };
        let initial: Vec<_> = (0..64).map(|i| (i % 4) as f64 / 3.0).collect();
        let mut bc = Boundary3D::uniform(BoundaryCondition::Neumann(0.0));
        bc.x_min = BoundaryCondition::Dirichlet(0.0);
        bc.x_max = BoundaryCondition::Neumann(1.0);
        let out = solve_advection_3d(grid, (1.0, 0.0, 0.0), 0.1, 1, &initial, &bc).unwrap();
        assert!((out[1].values[3] - 0.9).abs() < 1e-12);
        assert!((out[1].values[15] - 0.9).abs() < 1e-12);
        let periodic = Boundary3D::uniform(BoundaryCondition::Periodic);
        let flat = vec![1.0; 64];
        let wave = solve_wave_3d_with_bc(grid, 1.0, 0.1, 0, &flat, &[0.0; 64], &periodic).unwrap();
        assert_eq!(wave.len(), 1);
        let wave = solve_wave_3d_with_bc(grid, 1.0, 0.1, 2, &flat, &[0.0; 64], &periodic).unwrap();
        assert!(
            wave.iter()
                .all(|field| field.values.iter().all(|v| (*v - 1.0).abs() < 1e-12))
        );
    }
}
