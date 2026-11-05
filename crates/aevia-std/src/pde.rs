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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// Points along x.
    pub nx: usize,
    /// Points along y.
    pub ny: usize,
    /// The spacing.
    pub h: f64,
}

impl Grid {
    fn check(&self) -> Result<(), NumericError> {
        if self.nx < 3 || self.ny < 3 {
            return Err(NumericError::Domain(
                "a PDE grid needs at least three points per axis".to_string(),
            ));
        }
        if self.h <= 0.0 || !self.h.is_finite() {
            return Err(NumericError::Domain(
                "a PDE grid needs a positive spacing".to_string(),
            ));
        }
        Ok(())
    }

    /// The number of points.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nx * self.ny
    }

    /// True when the grid has no points.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A field on a grid: `values[y * nx + x]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    /// The grid it lives on.
    pub grid: Grid,
    /// The samples, row-major.
    pub values: Vec<f64>,
}

/// The Laplacian of a field with zero boundary data, five-point stencil.
fn laplacian(
    grid: Grid,
    values: &[f64],
) -> Vec<f64> {
    let (nx, ny, h) = (grid.nx, grid.ny, grid.h);
    let mut out = vec![0.0; values.len()];
    for y in 1..ny - 1 {
        for x in 1..nx - 1 {
            let index = y * nx + x;
            out[index] =
                (values[index + 1] + values[index - 1] + values[index + nx] + values[index - nx]
                    - 4.0 * values[index])
                    / (h * h);
        }
    }
    out
}

/// A Poisson problem `∇²u = f` with zero boundary data.
#[derive(Clone, Debug, PartialEq)]
pub struct PoissonSpec {
    /// The grid.
    pub grid: Grid,
    /// The source term `f`.
    pub source: Vec<f64>,
    /// Maximum SOR sweeps.
    pub iterations: usize,
    /// The residual tolerance.
    pub tolerance: f64,
}

/// Solves `∇²u = f` by successive over-relaxation.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when the source does not fit the grid, and
/// [`NumericError::NotConverged`] when the sweep limit is reached.
pub fn solve_poisson(spec: &PoissonSpec) -> Result<Field, NumericError> {
    spec.grid.check()?;
    if spec.source.len() != spec.grid.len() {
        return Err(NumericError::ShapeMismatch(format!(
            "a {}x{} grid needs {} source values, found {}",
            spec.grid.ny,
            spec.grid.nx,
            spec.grid.len(),
            spec.source.len()
        )));
    }
    let (nx, ny, h) = (spec.grid.nx, spec.grid.ny, spec.grid.h);
    let mut u = vec![0.0; spec.grid.len()];
    let omega = 1.5;
    let tolerance = if spec.tolerance > 0.0 {
        spec.tolerance
    } else {
        1e-10
    };
    for _ in 0..spec.iterations.max(1) {
        let mut change = 0.0f64;
        for y in 1..ny - 1 {
            for x in 1..nx - 1 {
                let index = y * nx + x;
                let neighbour = (u[index + 1] + u[index - 1] + u[index + nx] + u[index - nx]) / 4.0;
                let target = neighbour - h * h * spec.source[index] / 4.0;
                let updated = u[index] + omega * (target - u[index]);
                change = change.max((updated - u[index]).abs());
                u[index] = updated;
            }
        }
        if change < tolerance {
            return Ok(Field {
                grid: spec.grid,
                values: u,
            });
        }
    }
    Err(NumericError::NotConverged)
}

/// A heat equation `u_t = α ∇²u`.
#[derive(Clone, Debug, PartialEq)]
pub struct HeatSpec {
    /// The grid.
    pub grid: Grid,
    /// The diffusivity.
    pub alpha: f64,
    /// How many time steps to take.
    pub steps: usize,
    /// The time step.
    pub dt: f64,
    /// The initial field.
    pub initial: Vec<f64>,
}

/// Solves the heat equation with an explicit scheme, returning every snapshot
/// including the initial one.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when the initial field does not fit, and
/// [`NumericError::Domain`] when `α dt / h²` exceeds the stability limit.
pub fn solve_heat(spec: &HeatSpec) -> Result<Vec<Field>, NumericError> {
    spec.grid.check()?;
    if spec.initial.len() != spec.grid.len() {
        return Err(NumericError::ShapeMismatch(
            "the initial field does not fit the grid".to_string(),
        ));
    }
    let ratio = spec.alpha * spec.dt / (spec.grid.h * spec.grid.h);
    if !(ratio.is_finite() && ratio <= 0.25) {
        return Err(NumericError::Domain(format!(
            "the explicit heat scheme needs `alpha*dt/h^2 <= 0.25`, found {ratio}"
        )));
    }
    let mut field = spec.initial.clone();
    let mut out = vec![Field {
        grid: spec.grid,
        values: field.clone(),
    }];
    for _ in 0..spec.steps {
        let lap = laplacian(spec.grid, &field);
        for index in 0..field.len() {
            field[index] += spec.alpha * spec.dt * lap[index];
        }
        out.push(Field {
            grid: spec.grid,
            values: field.clone(),
        });
    }
    Ok(out)
}

/// A wave equation `u_tt = c² ∇²u`.
#[derive(Clone, Debug, PartialEq)]
pub struct WaveSpec {
    /// The grid.
    pub grid: Grid,
    /// The wave speed.
    pub c: f64,
    /// How many time steps to take.
    pub steps: usize,
    /// The time step.
    pub dt: f64,
    /// The initial displacement.
    pub initial: Vec<f64>,
    /// The initial velocity.
    pub velocity: Vec<f64>,
}

/// Solves the wave equation with the leapfrog scheme.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when a field does not fit, and
/// [`NumericError::Domain`] when the CFL condition fails.
pub fn solve_wave(spec: &WaveSpec) -> Result<Vec<Field>, NumericError> {
    spec.grid.check()?;
    if spec.initial.len() != spec.grid.len() || spec.velocity.len() != spec.grid.len() {
        return Err(NumericError::ShapeMismatch(
            "the initial fields do not fit the grid".to_string(),
        ));
    }
    let courant = spec.c * spec.dt / spec.grid.h;
    if !(courant.is_finite() && courant.abs() <= std::f64::consts::FRAC_1_SQRT_2 + 1e-12) {
        return Err(NumericError::Domain(format!(
            "the leapfrog scheme needs `c*dt/h <= 1/sqrt(2)`, found {courant}"
        )));
    }
    let squared = courant * courant;
    let mut previous = spec.initial.clone();
    let mut current = spec.initial.clone();
    // The first step needs the velocity: u^1 = u^0 + dt v^0 + dt²/2 c² ∇²u^0.
    let lap = laplacian(spec.grid, &current);
    for index in 0..current.len() {
        current[index] += spec.dt * spec.velocity[index] + 0.5 * squared * lap[index];
    }
    let mut out = vec![
        Field {
            grid: spec.grid,
            values: previous.clone(),
        },
        Field {
            grid: spec.grid,
            values: current.clone(),
        },
    ];
    for _ in 1..spec.steps {
        let lap = laplacian(spec.grid, &current);
        let mut next = vec![0.0; current.len()];
        for index in 0..current.len() {
            next[index] = 2.0 * current[index] - previous[index] + squared * lap[index];
        }
        previous = current;
        current = next;
        out.push(Field {
            grid: spec.grid,
            values: current.clone(),
        });
    }
    Ok(out)
}

/// An advection equation `u_t + v·∇u = 0`.
#[derive(Clone, Debug, PartialEq)]
pub struct AdvectionSpec {
    /// The grid.
    pub grid: Grid,
    /// The velocity along x.
    pub vx: f64,
    /// The velocity along y.
    pub vy: f64,
    /// How many time steps to take.
    pub steps: usize,
    /// The time step.
    pub dt: f64,
    /// The initial field.
    pub initial: Vec<f64>,
}

/// Solves advection with the first-order upwind scheme.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when the field does not fit, and
/// [`NumericError::Domain`] when the CFL condition fails.
pub fn solve_advection(spec: &AdvectionSpec) -> Result<Vec<Field>, NumericError> {
    spec.grid.check()?;
    if spec.initial.len() != spec.grid.len() {
        return Err(NumericError::ShapeMismatch(
            "the initial field does not fit the grid".to_string(),
        ));
    }
    let (nx, ny, h) = (spec.grid.nx, spec.grid.ny, spec.grid.h);
    let cx = spec.vx * spec.dt / h;
    let cy = spec.vy * spec.dt / h;
    if !(cx.is_finite() && cy.is_finite()) || cx.abs() + cy.abs() > 1.0 {
        return Err(NumericError::Domain(format!(
            "the upwind scheme needs `(|vx|+|vy|)*dt/h <= 1`, found {}",
            cx.abs() + cy.abs()
        )));
    }
    let mut field = spec.initial.clone();
    let mut out = vec![Field {
        grid: spec.grid,
        values: field.clone(),
    }];
    for _ in 0..spec.steps {
        let mut next = field.clone();
        for y in 1..ny - 1 {
            for x in 1..nx - 1 {
                let index = y * nx + x;
                let dx = if cx >= 0.0 {
                    field[index] - field[index - 1]
                } else {
                    field[index + 1] - field[index]
                };
                let dy = if cy >= 0.0 {
                    field[index] - field[index - nx]
                } else {
                    field[index + nx] - field[index]
                };
                next[index] = field[index] - cx * dx - cy * dy;
            }
        }
        field = next;
        out.push(Field {
            grid: spec.grid,
            values: field.clone(),
        });
    }
    Ok(out)
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
}
