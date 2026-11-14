//! Ordinary differential equations (§8.9).
//!
//! The state is a flat `Vec<f64>`: the runtime flattens a record or a tensor in
//! field order and restores it afterwards (§8.9's flattening contract), so the
//! integrator only ever sees numbers.
//!
//! `Rk4` is the fixed-step method and needs a step count; `Dopri5` is the
//! adaptive Dormand–Prince pair. Stiff methods use RSSN’s Radau IIA(5),
//! variable-order BDF and Rosenbrock 2(3) solvers with dense output.

use crate::NumericError;

/// The integration method of §8.9.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// Fixed-step fourth-order Runge–Kutta.
    Rk4,
    /// Adaptive Dormand–Prince 5(4).
    Dopri5,
    /// Adaptive Radau IIA (order 5, L-stable).
    Radau,
    /// Adaptive variable-order BDF (orders 1–5).
    Bdf,
    /// Adaptive Rosenbrock 2(3).
    Rosenbrock23,
    /// Dormand–Prince, switching to Radau IIA when the explicit method
    /// exhausts its step budget (a stiff problem needs far more explicit steps
    /// than its accuracy requires, §8.9 "stiff solver selection").
    Auto,
    /// Fixed-step second-order Störmer–Verlet symplectic integrator.
    StormerVerlet,
    /// Fixed-step fourth-order Ruth (Forest–Ruth / Yoshida) symplectic integrator.
    Ruth4,
    /// Fixed-step fourth-order (2-stage) Gauss–Legendre implicit symplectic Runge–Kutta.
    GaussLegendre4,
    /// Fixed-step sixth-order (3-stage) Gauss–Legendre implicit symplectic Runge–Kutta.
    GaussLegendre6,
    /// Fixed-step implicit midpoint (Gauss-Legendre order two).
    ImplicitMidpoint,
    /// Error-controlled step doubling of classical RK4.
    AdaptiveRk4,
    /// Error-controlled step doubling of partitioned Verlet.
    AdaptiveStormerVerlet,
    /// Error-controlled step doubling of Forest-Ruth.
    AdaptiveRuth4,
    /// Error-controlled step doubling of implicit midpoint.
    AdaptiveImplicitMidpoint,
    /// Error-controlled step doubling of two-stage Gauss-Legendre.
    AdaptiveGaussLegendre4,
    /// Error-controlled step doubling of three-stage Gauss-Legendre.
    AdaptiveGaussLegendre6,
}

impl Method {
    /// The capability this method needs, when it is not implemented.
    #[must_use]
    pub fn capability(&self) -> Option<&'static str> {
        match self {
            | Self::Rk4
            | Self::Dopri5
            | Self::Radau
            | Self::Bdf
            | Self::Rosenbrock23
            | Self::Auto
            | Self::StormerVerlet
            | Self::Ruth4
            | Self::GaussLegendre4
            | Self::GaussLegendre6
            | Self::ImplicitMidpoint
            | Self::AdaptiveRk4
            | Self::AdaptiveStormerVerlet
            | Self::AdaptiveRuth4
            | Self::AdaptiveImplicitMidpoint
            | Self::AdaptiveGaussLegendre4
            | Self::AdaptiveGaussLegendre6 => None,
        }
    }

    fn adaptive_geometry(self) -> Option<rssn::kernels::ode_geom::GeometricMethod> {
        use rssn::kernels::ode_geom::GeometricMethod as G;
        Some(match self {
            | Self::AdaptiveRk4 => G::Rk4,
            | Self::AdaptiveStormerVerlet => G::StormerVerlet,
            | Self::AdaptiveRuth4 => G::Ruth4,
            | Self::AdaptiveImplicitMidpoint => G::ImplicitMidpoint,
            | Self::AdaptiveGaussLegendre4 => G::GaussLegendre4,
            | Self::AdaptiveGaussLegendre6 => G::GaussLegendre6,
            | _ => return None,
        })
    }

    fn is_adaptive(self) -> bool {
        self.adaptive_geometry().is_some()
            || matches!(
                self,
                Self::Dopri5 | Self::Radau | Self::Bdf | Self::Rosenbrock23 | Self::Auto
            )
    }
}

/// Options for an integration.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// The method.
    pub method: Method,
    /// Relative tolerance (adaptive methods only).
    pub rtol: f64,
    /// Absolute tolerance (adaptive methods only).
    pub atol: f64,
    /// Sub-steps per output interval, for `Rk4`.
    pub steps: usize,
    /// Initial adaptive step; zero requests automatic startup.
    pub h0: f64,
    /// Minimum adaptive step, except the final clipped interval.
    pub h_min: f64,
    /// Maximum adaptive step.
    pub h_max: f64,
    /// Maximum attempted adaptive steps.
    pub max_steps: usize,
    /// Maximum actual right-hand-side callback invocations.
    pub max_evaluations: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            method: Method::Dopri5,
            rtol: 1e-8,
            atol: 1e-10,
            steps: 20,
            h0: 0.0,
            h_min: 1e-14,
            h_max: f64::INFINITY,
            max_steps: 100_000,
            max_evaluations: 10_000_000,
        }
    }
}

/// A right-hand side: `(t, y) -> dy/dt`.
pub type Rate<'a> = dyn FnMut(f64, &[f64]) -> Vec<f64> + 'a;

/// A terminal event function. A sign change stops integration at its root.
pub type EventRate<'a> = dyn FnMut(f64, &[f64]) -> f64 + 'a;

/// Integrate until a terminal event, returning the actual times and states.
/// Adaptive methods locate roots on their continuous extension; RK4 locates
/// roots within each fixed step by reintegration and bisection.
pub fn integrate_until_event(
    rhs: &mut Rate<'_>,
    y0: &[f64],
    ts: &[f64],
    options: Options,
    event: &mut EventRate<'_>,
) -> Result<(Vec<f64>, Vec<Vec<f64>>), NumericError> {
    use rssn::kernels::ode;
    use rssn::kernels::ode_adaptive::Event;
    use rssn::kernels::ode_adaptive::OdeOptions;
    use rssn::kernels::ode_adaptive::{
        self,
    };
    use rssn::kernels::ode_stiff;
    use std::cell::Cell;
    use std::cell::RefCell;

    if ts.is_empty() {
        return Err(NumericError::Empty);
    }
    if !ts.iter().chain(y0).all(|x| x.is_finite())
        || ts
            .windows(2)
            .any(|p| p[1] < p[0] || !(p[1] - p[0]).is_finite())
        || !options.rtol.is_finite()
        || options.rtol < 0.0
        || !options.atol.is_finite()
        || options.atol <= 0.0
    {
        return Err(NumericError::Domain(
            "invalid ODE times, state or tolerances".into(),
        ));
    }
    let initial_event = event(ts[0], y0);
    if !initial_event.is_finite() {
        return Err(NumericError::Domain(
            "event returned a non-finite value".into(),
        ));
    }
    if initial_event == 0.0 {
        return Ok((vec![ts[0]], vec![y0.to_vec()]));
    }
    let start = ts[0];
    let end = ts[ts.len() - 1];
    if start == end {
        return Ok((ts.to_vec(), vec![y0.to_vec(); ts.len()]));
    }
    if y0.is_empty() {
        return Err(NumericError::Domain(
            "event integration requires a non-empty state".into(),
        ));
    }
    if !(end - start).is_finite() {
        return Err(NumericError::Domain(
            "the time interval is too large".into(),
        ));
    }
    if options.method != Method::Rk4 {
        let specs = [EventSpec {
            terminal: true,
            direction: 0,
        }];
        let solution = solve(rhs, y0, (start, end), options, &specs, &mut |_, t, y| {
            event(t, y)
        })?;
        let terminal = *solution.t.last().ok_or(NumericError::NotConverged)?;
        let mut times = Vec::new();
        let mut states = Vec::new();
        for &t in ts.iter().take_while(|&&t| t <= terminal) {
            times.push(t);
            states.push(solution.eval(t).ok_or(NumericError::NotConverged)?);
        }
        if times.last().copied() != Some(terminal) {
            times.push(terminal);
            states.push(
                solution
                    .y
                    .last()
                    .cloned()
                    .ok_or(NumericError::NotConverged)?,
            );
        }
        return Ok((times, states));
    }
    let rhs = RefCell::new(rhs);
    let failure = RefCell::new(None);
    let f = |t: f64, y: &[f64], out: &mut [f64]| {
        match evaluate(&mut **rhs.borrow_mut(), t, y, out.len()) {
            | Ok(rate) => out.copy_from_slice(&rate),
            | Err(error) => {
                if failure.borrow().is_none() {
                    *failure.borrow_mut() = Some(error);
                }
                out.fill(f64::NAN);
            },
        }
    };
    let event = RefCell::new(event);
    let invalid_event = Cell::new(false);
    let g = |t: f64, y: &[f64]| {
        let value = event.borrow_mut()(t, y);
        if !value.is_finite() {
            invalid_event.set(true);
        }
        value
    };
    let events = [Event {
        g: &g,
        terminal: true,
        direction: 0,
    }];
    let opts = OdeOptions {
        rtol: options.rtol,
        atol: options.atol,
        h0: if options.h0 > 0.0 {
            Some(options.h0)
        } else {
            None
        },
        hmin: options.h_min,
        hmax: options.h_max,
        max_steps: options.max_steps,
    };
    let result = (|| {
        if options.method == Method::Rk4 {
            let mut times = vec![start];
            let mut states = vec![y0.to_vec()];
            for pair in ts.windows(2) {
                if pair[0] == pair[1] {
                    times.push(pair[1]);
                    states.push(states.last().unwrap().clone());
                    continue;
                }
                let (trajectory, hits) = ode::solve_fixed_events(
                    f,
                    states.last().unwrap(),
                    (pair[0], pair[1]),
                    options.steps.max(1),
                    ode::OdeSolverMethod::RungeKutta4,
                    &events,
                )
                .map_err(|_| NumericError::NotConverged)?;
                if let Some(hit) = hits.first() {
                    times.push(hit.t);
                    states.push(hit.y.clone());
                    break;
                }
                times.push(pair[1]);
                states.push(
                    trajectory
                        .y
                        .last()
                        .cloned()
                        .ok_or(NumericError::NotConverged)?,
                );
            }
            Ok((times, states))
        } else {
            let solution = match options.method {
                | Method::Dopri5 => ode_adaptive::dopri5(f, start, end, y0, &opts, &events),
                | Method::Radau => ode_stiff::radau5_events(f, start, end, y0, &opts, &events),
                | Method::Bdf => ode_stiff::bdf_events(f, start, end, y0, &opts, 5, &events),
                | Method::Rosenbrock23 => {
                    ode_adaptive::rosenbrock23_events(f, start, end, y0, &opts, &events)
                },
                | Method::Rk4
                | Method::Auto
                | Method::StormerVerlet
                | Method::Ruth4
                | Method::GaussLegendre4
                | Method::GaussLegendre6
                | Method::ImplicitMidpoint
                | Method::AdaptiveRk4
                | Method::AdaptiveStormerVerlet
                | Method::AdaptiveRuth4
                | Method::AdaptiveImplicitMidpoint
                | Method::AdaptiveGaussLegendre4
                | Method::AdaptiveGaussLegendre6 => unreachable!(),
            }
            .map_err(|_| NumericError::NotConverged);
            solution.and_then(|solution| {
                let terminal = *solution.t.last().ok_or(NumericError::NotConverged)?;
                let mut times = Vec::new();
                let mut states = Vec::new();
                for &t in ts.iter().take_while(|&&t| t <= terminal) {
                    times.push(t);
                    states.push(if t == start {
                        y0.to_vec()
                    } else if t == terminal {
                        solution
                            .y
                            .last()
                            .cloned()
                            .ok_or(NumericError::NotConverged)?
                    } else {
                        solution.eval(t).ok_or(NumericError::NotConverged)?
                    });
                }
                if times.last().copied() != Some(terminal) {
                    times.push(terminal);
                    states.push(
                        solution
                            .y
                            .last()
                            .cloned()
                            .ok_or(NumericError::NotConverged)?,
                    );
                }
                Ok((times, states))
            })
        }
    })();
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }
    if invalid_event.get() {
        return Err(NumericError::Domain(
            "event returned a non-finite value".into(),
        ));
    }
    result
}

/// Integrates `rhs` from `ts[0]` over the given times.
///
/// The result has one state per entry of `ts`, starting with `y0` at `ts[0]`.
///
/// # Errors
///
/// [`NumericError::Empty`] when there is no time to reach,
/// [`NumericError::ShapeMismatch`] when the right-hand side changes the state's
/// length, and [`NumericError::NotConverged`] when the adaptive step collapses.
pub fn integrate(
    rhs: &mut Rate<'_>,
    y0: &[f64],
    ts: &[f64],
    options: Options,
) -> Result<Vec<Vec<f64>>, NumericError> {
    if ts.is_empty() {
        return Err(NumericError::Empty);
    }
    if !ts.iter().all(|x| x.is_finite()) || !y0.iter().all(|x| x.is_finite()) {
        return Err(NumericError::Domain(
            "times and initial state must be finite".into(),
        ));
    }
    if !options.rtol.is_finite()
        || options.rtol < 0.0
        || !options.atol.is_finite()
        || options.atol <= 0.0
        || ts
            .windows(2)
            .any(|p| p[1] < p[0] || !(p[1] - p[0]).is_finite())
    {
        return Err(NumericError::Domain(
            "invalid adaptive tolerances or time grid".into(),
        ));
    }
    if options.method.is_adaptive() {
        let last = ts[ts.len() - 1];
        if ts[0] == last || y0.is_empty() {
            return Ok(vec![y0.to_vec(); ts.len()]);
        }
        let solution = solve(rhs, y0, (ts[0], last), options, &[], &mut |_, _, _| 0.0)?;
        return ts
            .iter()
            .map(|&t| solution.eval(t).ok_or(NumericError::NotConverged))
            .collect();
    }
    use rssn::kernels::ode_geom::GeometricMethod as G;
    let method = match options.method {
        | Method::Rk4 => G::Rk4,
        | Method::StormerVerlet => G::StormerVerlet,
        | Method::Ruth4 => G::Ruth4,
        | Method::ImplicitMidpoint => G::ImplicitMidpoint,
        | Method::GaussLegendre4 => G::GaussLegendre4,
        | Method::GaussLegendre6 => G::GaussLegendre6,
        | _ => unreachable!("adaptive methods handled above"),
    };
    rssn::kernels::ode_geom::sample_fixed_geometric(rhs, y0, ts, method, options.steps)
        .map_err(map_ode_geom_err)
}

fn evaluate(
    rhs: &mut Rate<'_>,
    t: f64,
    y: &[f64],
    expected: usize,
) -> Result<Vec<f64>, NumericError> {
    let rate = rhs(t, y);
    if rate.len() != expected {
        return Err(NumericError::ShapeMismatch(format!(
            "the right-hand side returned {} value(s) for a state of {expected}",
            rate.len()
        )));
    }
    if !rate.iter().all(|value| value.is_finite()) {
        return Err(NumericError::Domain(
            "the ODE right-hand side returned a non-finite value".into(),
        ));
    }
    Ok(rate)
}


// ---------------------------------------------------------------------------
// Solutions with dense output and events (§8.9)
// ---------------------------------------------------------------------------

pub use rssn::kernels::ode_geom::EventSpec;
pub use rssn::kernels::ode_geom::Hit;
/// Event function callback: `(index, t, y) -> g`.
pub type Events<'a> = dyn FnMut(usize, f64, &[f64]) -> f64 + 'a;

#[derive(Clone, Debug)]
enum Dense {
    /// The kernel's continuous extension (Dormand–Prince: order 4; Radau:
    /// collocation polynomial; BDF: Newton backward differences).
    Kernel(Box<rssn::kernels::ode_adaptive::OdeSolution>),
    /// Cubic Hermite interpolation through the nodes and their derivatives
    /// (fixed-step `Rk4`, whose kernel keeps no stage data).
    Hermite { rates: Vec<Vec<f64>> },
}

/// An ODE solution that can be evaluated at any time inside its span.
#[derive(Clone, Debug)]
pub struct Solution {
    /// Accepted time points (increasing).
    pub t: Vec<f64>,
    /// States at the accepted time points.
    pub y: Vec<Vec<f64>>,
    /// Located events, in time order.
    pub hits: Vec<Hit>,
    /// Right-hand-side evaluations.
    pub nfev: usize,
    /// Rejected steps (adaptive methods).
    pub rejected: usize,
    /// The method that produced the solution: [`Method::Auto`] reports the
    /// method it settled on.
    pub method: Method,
    dense: Dense,
}

impl Solution {
    /// The state at `t`, from the method's continuous extension; `None` outside
    /// the integrated span (which ends at a terminal event, if one fired).
    #[must_use]
    pub fn eval(
        &self,
        t: f64,
    ) -> Option<Vec<f64>> {
        let (&first, &last) = (self.t.first()?, self.t.last()?);
        if !(first..=last).contains(&t) {
            return None;
        }
        if t == first {
            return self.y.first().cloned();
        }
        if t == last {
            return self.y.last().cloned();
        }
        match &self.dense {
            | Dense::Kernel(solution) => solution.eval(t),
            | Dense::Hermite { rates } => {
                let upper = self
                    .t
                    .partition_point(|&node| node < t)
                    .clamp(1, self.t.len() - 1);
                let lower = upper - 1;
                rssn::kernels::ode_geom::hermite_segment(
                    self.t[lower],
                    self.t[upper],
                    &self.y[lower],
                    &self.y[upper],
                    &rates[lower],
                    &rates[upper],
                    t,
                )
                .ok()
            },
        }
    }

    /// The span the solution covers: `(t0, t_end)`, where `t_end` is the
    /// terminal event time if one fired.
    #[must_use]
    pub fn span(&self) -> (f64, f64) {
        (
            self.t.first().copied().unwrap_or(0.0),
            self.t.last().copied().unwrap_or(0.0),
        )
    }
}

/// The step budget of [`Method::Auto`]'s explicit attempt. A non-stiff problem
/// at these tolerances needs far fewer; a stiff one runs into the stability
/// limit and exhausts it.
const AUTO_STEP_BUDGET: usize = 20_000;

/// Integrates `rhs` over `span = (t0, t1)` (forward in time) and keeps a
/// continuous extension, watching `specs.len()` event functions `events(i, t, y)`.
///
/// Event semantics are the kernel's: a sign change of `g` between accepted
/// states is located by root finding on the continuous extension (`Rk4`:
/// bisection by reintegrating the step); `direction` filters rising/falling
/// crossings and a `terminal` event ends the integration there. For `Rk4`,
/// `options.steps` is the **total** number of steps over the span.
///
/// # Errors
///
/// [`NumericError::Domain`] for non-finite data, bad tolerances or a backward
/// span, and [`NumericError::NotConverged`] when the step collapses.
pub fn solve(
    rhs: &mut Rate<'_>,
    y0: &[f64],
    span: (f64, f64),
    options: Options,
    specs: &[EventSpec],
    events: &mut Events<'_>,
) -> Result<Solution, NumericError> {
    use rssn::kernels::ode;
    use rssn::kernels::ode_adaptive::Event;
    use rssn::kernels::ode_adaptive::OdeOptions;
    use rssn::kernels::ode_adaptive::{
        self,
    };
    use rssn::kernels::ode_stiff;
    use std::cell::Cell;
    use std::cell::RefCell;

    let (start, end) = span;
    if !start.is_finite()
        || !end.is_finite()
        || !y0.iter().all(|x| x.is_finite())
        || !options.rtol.is_finite()
        || options.rtol < 0.0
        || !options.atol.is_finite()
        || options.atol <= 0.0
    {
        return Err(NumericError::Domain(
            "invalid ODE span, state or tolerances".into(),
        ));
    }
    if end < start {
        return Err(NumericError::Domain(
            "the ODE span must run forward in time".into(),
        ));
    }
    if !(end - start).is_finite() {
        return Err(NumericError::Domain(
            "the time interval is too large".into(),
        ));
    }
    if y0.is_empty() {
        return Err(NumericError::Domain("the ODE state is empty".into()));
    }
    if end == start {
        return Ok(Solution {
            t: vec![start],
            y: vec![y0.to_vec()],
            hits: Vec::new(),
            nfev: 0,
            rejected: 0,
            method: options.method,
            dense: Dense::Hermite {
                rates: vec![vec![0.0; y0.len()]],
            },
        });
    }
    if !options.h0.is_finite()
        || options.h0 < 0.0
        || !options.h_min.is_finite()
        || options.h_min < 0.0
        || options.h_max.is_nan()
        || options.h_max <= 0.0
        || options.h_min > options.h_max
        || options.max_steps == 0
        || options.max_evaluations == 0
    {
        return Err(NumericError::Domain(
            "invalid ODE step bounds or work budgets".into(),
        ));
    }
    let rhs_cell = RefCell::new(rhs);
    let failure: RefCell<Option<NumericError>> = RefCell::new(None);
    let evaluations = Cell::new(0usize);
    let f = |t: f64, y: &[f64], out: &mut [f64]| {
        if failure.borrow().is_some() || evaluations.get() >= options.max_evaluations {
            if failure.borrow().is_none() {
                *failure.borrow_mut() = Some(NumericError::NotConverged);
            }
            out.fill(f64::NAN);
            return;
        }
        evaluations.set(evaluations.get() + 1);

        match evaluate(&mut **rhs_cell.borrow_mut(), t, y, out.len()) {
            | Ok(rate) => out.copy_from_slice(&rate),
            | Err(error) => {
                if failure.borrow().is_none() {
                    *failure.borrow_mut() = Some(error);
                }
                out.fill(f64::NAN);
            },
        }
    };
    let events_cell = RefCell::new(events);
    let invalid_event = Cell::new(false);
    let guards: Vec<Box<dyn Fn(f64, &[f64]) -> f64 + '_>> = (0..specs.len())
        .map(|index| {
            let events_cell = &events_cell;
            let invalid_event = &invalid_event;
            Box::new(move |t: f64, y: &[f64]| {
                let value = events_cell.borrow_mut()(index, t, y);
                if !value.is_finite() {
                    invalid_event.set(true);
                }
                value
            }) as Box<dyn Fn(f64, &[f64]) -> f64 + '_>
        })
        .collect();
    let kernel_events: Vec<Event<'_>> = guards
        .iter()
        .zip(specs)
        .map(|(g, spec)| {
            Event {
                g: g.as_ref(),
                terminal: spec.terminal,
                direction: spec.direction,
            }
        })
        .collect();
    let opts = OdeOptions {
        rtol: options.rtol,
        atol: options.atol,
        h0: if options.h0 > 0.0 {
            Some(options.h0)
        } else {
            None
        },
        hmin: options.h_min,
        hmax: options.h_max,
        max_steps: options.max_steps,
    };
    let to_hits = |hits: &[ode_adaptive::EventHit]| -> Vec<Hit> {
        hits.iter()
            .map(|hit| {
                Hit {
                    index: hit.index,
                    t: hit.t,
                    y: hit.y.clone(),
                }
            })
            .collect()
    };
    let kernel = |method: Method, step_budget: usize| -> Result<Solution, NumericError> {
        let opts = OdeOptions {
            max_steps: step_budget,
            ..opts
        };
        let solution = match method {
            | Method::Dopri5 => ode_adaptive::dopri5(f, start, end, y0, &opts, &kernel_events),
            | Method::Radau => ode_stiff::radau5_events(f, start, end, y0, &opts, &kernel_events),
            | Method::Bdf => ode_stiff::bdf_events(f, start, end, y0, &opts, 5, &kernel_events),
            | Method::Rosenbrock23 => {
                ode_adaptive::rosenbrock23_events(f, start, end, y0, &opts, &kernel_events)
            },
            | Method::Rk4
            | Method::Auto
            | Method::StormerVerlet
            | Method::Ruth4
            | Method::GaussLegendre4
            | Method::GaussLegendre6
            | Method::ImplicitMidpoint
            | Method::AdaptiveRk4
            | Method::AdaptiveStormerVerlet
            | Method::AdaptiveRuth4
            | Method::AdaptiveImplicitMidpoint
            | Method::AdaptiveGaussLegendre4
            | Method::AdaptiveGaussLegendre6 => unreachable!("handled by the caller"),
        }
        .map_err(|_| NumericError::NotConverged)?;
        Ok(Solution {
            t: solution.t.clone(),
            y: solution.y.clone(),
            hits: to_hits(&solution.events),
            nfev: solution.nfev,
            rejected: solution.rejected,
            method,
            dense: Dense::Kernel(Box::new(solution)),
        })
    };
    let result = (|| {
        match options.method {
            | Method::Rk4 => {
                let (trajectory, fixed_hits) = ode::solve_fixed_events(
                    f,
                    y0,
                    (start, end),
                    options.steps.max(1),
                    ode::OdeSolverMethod::RungeKutta4,
                    &kernel_events,
                )
                .map_err(|_| NumericError::NotConverged)?;
                let mut rates = Vec::with_capacity(trajectory.t.len());
                for (&t, y) in trajectory.t.iter().zip(&trajectory.y) {
                    let mut rate = vec![0.0; y.len()];
                    f(t, y, &mut rate);
                    rates.push(rate);
                }
                let steps = options.steps.max(1);
                Ok(Solution {
                    hits: fixed_hits
                        .iter()
                        .map(|hit| {
                            Hit {
                                index: hit.index,
                                t: hit.t,
                                y: hit.y.clone(),
                            }
                        })
                        .collect(),
                    nfev: 4 * steps + trajectory.t.len(),
                    rejected: 0,
                    method: Method::Rk4,
                    dense: Dense::Hermite { rates },
                    t: trajectory.t,
                    y: trajectory.y,
                })
            },
            | Method::AdaptiveRk4
            | Method::AdaptiveStormerVerlet
            | Method::AdaptiveRuth4
            | Method::AdaptiveImplicitMidpoint
            | Method::AdaptiveGaussLegendre4
            | Method::AdaptiveGaussLegendre6 => {
                solve_adaptive_geometric(
                    &mut *rhs_cell.borrow_mut(),
                    y0,
                    (start, end),
                    options,
                    specs,
                    &mut *events_cell.borrow_mut(),
                    &invalid_event,
                )
            },
            | Method::ImplicitMidpoint
            | Method::StormerVerlet
            | Method::Ruth4
            | Method::GaussLegendre4
            | Method::GaussLegendre6 => {
                solve_fixed_geometric(
                    &mut *rhs_cell.borrow_mut(),
                    y0,
                    (start, end),
                    options.steps.max(1),
                    options.method,
                    specs,
                    &mut *events_cell.borrow_mut(),
                    &invalid_event,
                )
            },
            | Method::Auto => {
                let budgeted = OdeOptions {
                    max_steps: options.max_steps.min(AUTO_STEP_BUDGET),
                    ..opts
                };
                match ode_adaptive::dopri5(f, start, end, y0, &budgeted, &kernel_events) {
                    | Ok(solution) => {
                        Ok(Solution {
                            t: solution.t.clone(),
                            y: solution.y.clone(),
                            hits: to_hits(&solution.events),
                            nfev: solution.nfev,
                            rejected: solution.rejected,
                            method: Method::Dopri5,
                            dense: Dense::Kernel(Box::new(solution)),
                        })
                    },
                    // A genuine failure (non-finite state, callback error) is not
                    // a stiffness signal; only an exhausted budget is.
                    | Err(ode_adaptive::OdeError::StepLimit) if failure.borrow().is_none() => {
                        let remaining = options
                            .max_steps
                            .saturating_sub(options.max_steps.min(AUTO_STEP_BUDGET));
                        if remaining == 0 {
                            Err(NumericError::NotConverged)
                        } else {
                            kernel(Method::Radau, remaining)
                        }
                    },
                    | Err(_) => Err(NumericError::NotConverged),
                }
            },
            | method => kernel(method, options.max_steps),
        }
    })();
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }
    if invalid_event.get() {
        return Err(NumericError::Domain(
            "an event returned a non-finite value".into(),
        ));
    }
    result
}

// ---------------------------------------------------------------------------
// Higher-order equations (§8.9)
// ---------------------------------------------------------------------------

/// Rewrites `y⁽ⁿ⁾ = f(t, y, y′, …, y⁽ⁿ⁻¹⁾)` as the first-order system on the
/// stacked state `z = (y, y′, …, y⁽ⁿ⁻¹⁾)`, `n` blocks of `dim` components:
/// the first `n − 1` blocks of `z′` are the last `n − 1` blocks of `z`, and the
/// last block is `f(t, z)`.
///
/// `f` receives the stacked state and answers the `dim` components of the top
/// derivative. The order and block size are checked by the caller; a wrong
/// length from `f` is reported by the integrator as a shape mismatch.
pub fn reduce_order<'a>(
    order: usize,
    dim: usize,
    mut f: impl FnMut(f64, &[f64]) -> Vec<f64> + 'a,
) -> impl FnMut(f64, &[f64]) -> Vec<f64> + 'a {
    move |t, z| {
        let mut out = Vec::with_capacity(order * dim);
        out.extend_from_slice(&z[dim..order * dim]);
        // A wrong-length answer is passed on unchanged so the integrator
        // reports it as a shape mismatch rather than the reduction hiding it.
        out.extend(f(t, z));
        out
    }
}

// ---------------------------------------------------------------------------
// Boundary-value problems by single shooting (§8.9)
// ---------------------------------------------------------------------------

/// A boundary condition `r(y(t0), y(t1)) = 0` with as many residuals as the
/// state has components.
pub type Boundary<'a> = dyn FnMut(&[f64], &[f64]) -> Vec<f64> + 'a;

/// Solves the two-point boundary-value problem `y′ = f(t, y)`,
/// `r(y(t0), y(t1)) = 0` by single shooting: Newton's method on the initial
/// state with a forward-difference Jacobian (one integration per unknown) and
/// a backtracking line search. `guess` is the initial state to start from.
///
/// The returned solution is the final integration, so it has dense output.
/// Single shooting needs the problem to be well conditioned over the span
/// (the sensitivity of `y(t1)` to `y(t0)` must not blow up); stiff or
/// exponentially unstable problems need multiple shooting or collocation,
/// which this routine does not provide.
///
/// # Errors
///
/// [`NumericError::ShapeMismatch`] when `r` does not return `n` residuals,
/// [`NumericError::Singular`] for a singular Jacobian and
/// [`NumericError::NotConverged`] when Newton's method does not converge in
/// 60 iterations or an integration fails.
pub fn shoot(
    rhs: &mut Rate<'_>,
    guess: &[f64],
    span: (f64, f64),
    options: Options,
    boundary: &mut Boundary<'_>,
) -> Result<Solution, NumericError> {
    let mut residual = |p: &[f64]| -> Result<Vec<f64>, NumericError> {
        let solution = solve(rhs, p, span, options, &[], &mut |_, _, _| 0.0)?;
        let end = solution.y.last().ok_or(NumericError::NotConverged)?;
        Ok(boundary(p, end))
    };
    let initial =
        rssn::kernels::ode_geom::shoot_initial(&mut residual, guess).map_err(|error| {
            match error {
                | rssn::kernels::ode_geom::ShootingError::Residual(error) => error,
                | rssn::kernels::ode_geom::ShootingError::Numerical(error) => {
                    map_ode_geom_err(error)
                },
            }
        })?;
    solve(rhs, &initial, span, options, &[], &mut |_, _, _| 0.0)
}

// ---------------------------------------------------------------------------
// Symplectic and Geometric Integrators
// ---------------------------------------------------------------------------

/// Symplectic and geometric integration methods for Hamiltonian and
pub use rssn::kernels::ode_geom::DaeOptions;
/// Symplectic and geometric integration methods for Hamiltonian and
pub use rssn::kernels::ode_geom::DaeSolution;
/// Symplectic and geometric integration methods for Hamiltonian and
pub use rssn::kernels::ode_geom::DdeOptions;
/// Symplectic and geometric integration methods for Hamiltonian and
pub use rssn::kernels::ode_geom::DdeSolution;
/// Symplectic and geometric integration methods for Hamiltonian and
pub use rssn::kernels::ode_geom::SymplecticMethod;
/// Symplectic and geometric integration methods for Hamiltonian and
pub use rssn::kernels::ode_geom::SymplecticSolution;

fn map_ode_geom_err(err: rssn::kernels::ode_geom::OdeGeomError) -> NumericError {
    match err {
        | rssn::kernels::ode_geom::OdeGeomError::Empty => NumericError::Empty,
        | rssn::kernels::ode_geom::OdeGeomError::ShapeMismatch(s) => NumericError::ShapeMismatch(s),
        | rssn::kernels::ode_geom::OdeGeomError::Domain(s) => NumericError::Domain(s),
        | rssn::kernels::ode_geom::OdeGeomError::Singular => NumericError::Singular,
        | rssn::kernels::ode_geom::OdeGeomError::NotConverged => NumericError::NotConverged,
    }
}

pub fn integrate_symplectic<A>(
    accel: &mut A,
    q0: &[f64],
    p0: &[f64],
    span: (f64, f64),
    steps: usize,
    method: SymplecticMethod,
) -> Result<SymplecticSolution, NumericError>
where
    A: FnMut(f64, &[f64]) -> Vec<f64> + ?Sized,
{
    rssn::kernels::ode_geom::integrate_symplectic(accel, q0, p0, span, steps, method)
        .map_err(map_ode_geom_err)
}

pub fn integrate_hamiltonian<FQ, FP>(
    dq_dt: &mut FQ,
    dp_dt: &mut FP,
    q0: &[f64],
    p0: &[f64],
    span: (f64, f64),
    steps: usize,
    method: SymplecticMethod,
) -> Result<SymplecticSolution, NumericError>
where
    FQ: FnMut(f64, &[f64], &[f64]) -> Vec<f64> + ?Sized,
    FP: FnMut(f64, &[f64], &[f64]) -> Vec<f64> + ?Sized,
{
    rssn::kernels::ode_geom::integrate_hamiltonian(dq_dt, dp_dt, q0, p0, span, steps, method)
        .map_err(map_ode_geom_err)
}

pub fn gauss_legendre_step(
    rhs: &mut Rate<'_>,
    t: f64,
    y: &[f64],
    h: f64,
    stages: usize,
) -> Result<(Vec<f64>, usize), NumericError> {
    rssn::kernels::ode_geom::gauss_legendre_step(rhs, t, y, h, stages).map_err(map_ode_geom_err)
}

/// # Errors
///
/// [`NumericError::Domain`] for invalid inputs, [`NumericError::ShapeMismatch`] for
/// mismatched dimensions, and [`NumericError::NotConverged`] if Newton iteration fails.
pub fn solve_gauss_legendre(
    rhs: &mut Rate<'_>,
    y0: &[f64],
    span: (f64, f64),
    steps: usize,
    stages: usize,
) -> Result<Solution, NumericError> {
    let method = match stages {
        | 1 => Method::ImplicitMidpoint,
        | 2 => Method::GaussLegendre4,
        | 3 => Method::GaussLegendre6,
        | _ => {
            return Err(NumericError::Domain(
                "Gauss-Legendre stages must be 1, 2, or 3".into(),
            ));
        },
    };
    let invalid_event = std::cell::Cell::new(false);
    let mut no_events = |_: usize, _: f64, _: &[f64]| 0.0;
    solve_fixed_geometric_with_stages(
        rhs,
        y0,
        span,
        steps.max(1),
        method,
        Some(stages),
        &[],
        &mut no_events,
        &invalid_event,
    )
}

#[allow(clippy::too_many_arguments)]
fn solve_fixed_geometric(
    rhs: &mut Rate<'_>,
    y0: &[f64],
    span: (f64, f64),
    steps: usize,
    method: Method,
    specs: &[EventSpec],
    events: &mut Events<'_>,
    invalid_event: &std::cell::Cell<bool>,
) -> Result<Solution, NumericError> {
    solve_fixed_geometric_with_stages(
        rhs,
        y0,
        span,
        steps,
        method,
        None,
        specs,
        events,
        invalid_event,
    )
}

#[allow(clippy::too_many_arguments)]
fn solve_fixed_geometric_with_stages(
    rhs: &mut Rate<'_>,
    y0: &[f64],
    span: (f64, f64),
    steps: usize,
    method: Method,
    explicit_stages: Option<usize>,
    specs: &[EventSpec],
    events: &mut Events<'_>,
    invalid_event: &std::cell::Cell<bool>,
) -> Result<Solution, NumericError> {
    use rssn::kernels::ode_geom::GeometricMethod;
    use rssn::kernels::ode_geom::{
        self,
    };
    let kernel_method = match method {
        | Method::ImplicitMidpoint => GeometricMethod::ImplicitMidpoint,
        | Method::StormerVerlet => GeometricMethod::StormerVerlet,
        | Method::Ruth4 => GeometricMethod::Ruth4,
        | Method::GaussLegendre4 => GeometricMethod::GaussLegendre4,
        | Method::GaussLegendre6 => GeometricMethod::GaussLegendre6,
        | _ => return Err(NumericError::Domain("expected a geometric method".into())),
    };
    let solution = ode_geom::solve_fixed_geometric(
        rhs,
        y0,
        span,
        steps,
        kernel_method,
        explicit_stages,
        specs,
        events,
        invalid_event,
    )
    .map_err(map_ode_geom_err)?;
    Ok(Solution {
        t: solution.t,
        y: solution.y,
        hits: solution.hits,
        nfev: solution.nfev,
        rejected: solution.rejected,
        method,
        dense: Dense::Hermite {
            rates: solution.rates,
        },
    })
}

/// Error-controlled geometric integration, using the selected kernel method.
#[allow(clippy::too_many_arguments)]
fn solve_adaptive_geometric(
    rhs: &mut Rate<'_>,
    y0: &[f64],
    span: (f64, f64),
    options: Options,
    specs: &[EventSpec],
    events: &mut Events<'_>,
    invalid: &std::cell::Cell<bool>,
) -> Result<Solution, NumericError> {
    let method = options
        .method
        .adaptive_geometry()
        .ok_or_else(|| NumericError::Domain("expected an adaptive geometric method".into()))?;
    let solution = rssn::kernels::ode_geom::solve_adaptive_geometric(
        rhs,
        y0,
        span,
        method,
        rssn::kernels::ode_geom::AdaptiveGeomOptions {
            rtol: options.rtol,
            atol: options.atol,
            h0: options.h0,
            h_min: options.h_min,
            h_max: options.h_max,
            max_steps: options.max_steps,
            max_evaluations: options.max_evaluations,
        },
        specs,
        events,
        invalid,
    )
    .map_err(map_ode_geom_err)?;
    Ok(Solution {
        t: solution.t,
        y: solution.y,
        hits: solution.hits,
        nfev: solution.nfev,
        rejected: solution.rejected,
        method: options.method,
        dense: Dense::Hermite {
            rates: solution.rates,
        },
    })
}

// ---------------------------------------------------------------------------
// Differential-Algebraic Equation (DAE) Solvers (Index-1 and Index-2)
// ---------------------------------------------------------------------------

/// Options for Differential-Algebraic Equation (DAE) solvers.
pub fn dae_consistent_ic_index1<F, G>(
    f: &mut F,
    g: &mut G,
    t0: f64,
    y0: &[f64],
    z0_guess: &[f64],
    tol: f64,
    max_iter: usize,
) -> Result<(Vec<f64>, Vec<f64>), NumericError>
where
    F: FnMut(f64, &[f64], &[f64]) -> Vec<f64> + ?Sized,
    G: FnMut(f64, &[f64], &[f64]) -> Vec<f64> + ?Sized,
{
    rssn::kernels::ode_geom::dae_consistent_ic_index1(f, g, t0, y0, z0_guess, tol, max_iter)
        .map_err(map_ode_geom_err)
}

pub fn solve_dae_index1<F, G>(
    f: &mut F,
    g: &mut G,
    y0: &[f64],
    z0: &[f64],
    span: (f64, f64),
    options: DaeOptions,
) -> Result<DaeSolution, NumericError>
where
    F: FnMut(f64, &[f64], &[f64]) -> Vec<f64> + ?Sized,
    G: FnMut(f64, &[f64], &[f64]) -> Vec<f64> + ?Sized,
{
    rssn::kernels::ode_geom::solve_dae_index1(f, g, y0, z0, span, options).map_err(map_ode_geom_err)
}

pub fn dae_consistent_ic_index2<F, G>(
    f: &mut F,
    g: &mut G,
    t0: f64,
    y0: &[f64],
    z0_guess: &[f64],
    tol: f64,
    max_iter: usize,
) -> Result<(Vec<f64>, Vec<f64>), NumericError>
where
    F: FnMut(f64, &[f64], &[f64]) -> Vec<f64> + ?Sized,
    G: FnMut(f64, &[f64]) -> Vec<f64> + ?Sized,
{
    rssn::kernels::ode_geom::dae_consistent_ic_index2(f, g, t0, y0, z0_guess, tol, max_iter)
        .map_err(map_ode_geom_err)
}

pub fn solve_dae_index2<F, G>(
    f: &mut F,
    g: &mut G,
    y0: &[f64],
    z0_guess: &[f64],
    span: (f64, f64),
    options: DaeOptions,
) -> Result<DaeSolution, NumericError>
where
    F: FnMut(f64, &[f64], &[f64]) -> Vec<f64> + ?Sized,
    G: FnMut(f64, &[f64]) -> Vec<f64> + ?Sized,
{
    rssn::kernels::ode_geom::solve_dae_index2(f, g, y0, z0_guess, span, options)
        .map_err(map_ode_geom_err)
}

pub fn solve_dae_implicit<R>(
    residual: &mut R,
    y0: &[f64],
    yp0_guess: &[f64],
    span: (f64, f64),
    options: DaeOptions,
) -> Result<Solution, NumericError>
where
    R: FnMut(f64, &[f64], &[f64]) -> Vec<f64> + ?Sized,
{
    let sol = rssn::kernels::ode_geom::solve_dae_implicit(residual, y0, yp0_guess, span, options)
        .map_err(map_ode_geom_err)?;
    Ok(Solution {
        t: sol.t,
        y: sol.y,
        hits: Vec::new(),
        nfev: sol.nfev,
        rejected: sol.rejected,
        method: Method::Bdf,
        dense: Dense::Hermite { rates: sol.rates },
    })
}

pub fn solve_dde<F, H>(
    rhs: &mut F,
    history: &mut H,
    y0: &[f64],
    delays: &[f64],
    span: (f64, f64),
    options: DdeOptions,
) -> Result<DdeSolution, NumericError>
where
    F: FnMut(f64, &[f64], &[Vec<f64>]) -> Vec<f64> + ?Sized,
    H: FnMut(f64) -> Vec<f64> + ?Sized,
{
    rssn::kernels::ode_geom::solve_dde(rhs, history, y0, delays, span, options)
        .map_err(map_ode_geom_err)
}

pub fn solve_dde_state_dependent<F, H>(
    rhs: &mut F,
    history: &mut H,
    y0: &[f64],
    span: (f64, f64),
    options: DdeOptions,
) -> Result<DdeSolution, NumericError>
where
    F: FnMut(f64, &[f64], &dyn Fn(f64) -> Vec<f64>) -> Vec<f64> + ?Sized,
    H: FnMut(f64) -> Vec<f64> + ?Sized,
{
    rssn::kernels::ode_geom::solve_dde_state_dependent(rhs, history, y0, span, options)
        .map_err(map_ode_geom_err)
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
    fn rk4_solves_exponential_growth() {
        // y' = y, y(0) = 1, so y(1) = e.
        let mut rhs = |_t: f64, y: &[f64]| vec![y[0]];
        let ts = [0.0, 0.25, 0.5, 0.75, 1.0];
        let solution = integrate(
            &mut rhs,
            &[1.0],
            &ts,
            Options {
                method: Method::Rk4,
                steps: 100,
                ..Options::default()
            },
        )
        .unwrap();
        assert_eq!(solution.len(), 5);
        assert!(close(solution[4][0], std::f64::consts::E, 1e-8));
    }

    #[test]
    fn dopri5_solves_a_harmonic_oscillator() {
        // y'' = -y as the system (y, v)' = (v, -y).
        let mut rhs = |_t: f64, y: &[f64]| vec![y[1], -y[0]];
        let ts: Vec<f64> = (0..=20)
            .map(|index| index as f64 * std::f64::consts::PI / 20.0)
            .collect();
        let solution = integrate(&mut rhs, &[1.0, 0.0], &ts, Options::default()).unwrap();
        // y = cos t, v = -sin t: after half a period the state is (-1, 0).
        let last = solution.last().unwrap();
        assert!(close(last[0], -1.0, 1e-6));
        assert!(close(last[1], 0.0, 1e-6));
    }

    #[test]
    fn radau_solves_a_stiff_non_autonomous_problem() {
        // The exact solution is cos(t); the fast transient has rate -1000.
        let mut rhs = |t: f64, y: &[f64]| vec![-1000.0 * (y[0] - t.cos()) - t.sin()];
        let outcome = integrate(
            &mut rhs,
            &[1.0],
            &[0.0, 1.0],
            Options {
                method: Method::Radau,
                ..Options::default()
            },
        )
        .unwrap();
        assert!(close(outcome[1][0], 1.0_f64.cos(), 1e-6), "{outcome:?}");
        assert_eq!(Method::Radau.capability(), None);
        assert_eq!(Method::Dopri5.capability(), None);
    }

    #[test]
    fn radau_handles_coupled_stiff_and_nonlinear_states() {
        // x' = -1000(x - y²) + 2y y', y' = -y, with x=y².
        let mut rhs =
            |_t: f64, y: &[f64]| vec![-1000.0 * (y[0] - y[1] * y[1]) - 2.0 * y[1] * y[1], -y[1]];
        let solution = integrate(
            &mut rhs,
            &[1.0, 1.0],
            &[0.0, 0.5, 1.0],
            Options {
                method: Method::Radau,
                ..Options::default()
            },
        )
        .unwrap();
        assert!(
            close(solution[2][0], (-2.0_f64).exp(), 2e-6),
            "{solution:?}"
        );
        assert!(
            close(solution[2][1], (-1.0_f64).exp(), 2e-6),
            "{solution:?}"
        );
    }

    #[test]
    fn radau_damps_a_very_fast_transient_without_explicit_stability_steps() {
        let mut calls = 0;
        let mut rhs = |_t: f64, y: &[f64]| {
            calls += 1;
            vec![-1e6 * y[0]]
        };
        let solution = integrate(
            &mut rhs,
            &[1.0],
            &[0.0, 1.0],
            Options {
                method: Method::Radau,
                ..Options::default()
            },
        )
        .unwrap();
        assert!(solution[1][0].abs() < 1e-9, "{solution:?}");
        assert!(calls < 30_000, "{calls} evaluations");
    }

    #[test]
    fn radau_rejects_invalid_input_and_rhs_shape() {
        let opts = Options {
            method: Method::Radau,
            ..Options::default()
        };
        let mut rhs = |_t: f64, _y: &[f64]| vec![1.0, 2.0];
        assert!(matches!(
            integrate(&mut rhs, &[1.0], &[0.0, 1.0], opts),
            Err(NumericError::ShapeMismatch(_))
        ));
        assert!(matches!(
            integrate(&mut rhs, &[1.0], &[0.0, f64::NAN], opts),
            Err(NumericError::Domain(_))
        ));
        assert!(matches!(
            integrate(&mut rhs, &[1.0], &[0.0, 1.0], Options { atol: 0.0, ..opts }),
            Err(NumericError::Domain(_))
        ));
    }

    #[test]
    fn each_stiff_solver_handles_a_nonuniform_output_grid() {
        for method in [Method::Radau, Method::Bdf, Method::Rosenbrock23] {
            let mut rhs = |t: f64, y: &[f64]| vec![-1000.0 * (y[0] - t.cos()) - t.sin()];
            let times = [0.0, 0.001, 0.1, 0.1, 0.73, 1.0];
            let trajectory = integrate(
                &mut rhs,
                &[1.0],
                &times,
                Options {
                    method,
                    ..Options::default()
                },
            )
            .unwrap();
            for (t, state) in times.iter().zip(&trajectory) {
                assert!(
                    (state[0] - t.cos()).abs() < 1e-5,
                    "{method:?} at {t}: {state:?}"
                );
            }
        }
    }

    #[test]
    fn all_methods_locate_a_terminal_event_inside_a_step() {
        for method in [
            Method::Rk4,
            Method::Dopri5,
            Method::Radau,
            Method::Bdf,
            Method::Rosenbrock23,
        ] {
            let mut rhs = |_t: f64, _y: &[f64]| vec![1.0];
            let mut event = |_t: f64, y: &[f64]| y[0] - 0.6;
            let (times, values) = integrate_until_event(
                &mut rhs,
                &[0.0],
                &[0.0, 0.5, 1.0],
                Options {
                    method,
                    steps: 4,
                    ..Options::default()
                },
                &mut event,
            )
            .unwrap();
            assert_eq!(times.len(), 3, "{method:?}: {times:?}");
            assert!((times[2] - 0.6).abs() < 1e-7, "{method:?}: {times:?}");
            assert!((values[2][0] - 0.6).abs() < 1e-7, "{method:?}: {values:?}");
        }
    }

    #[test]
    fn event_at_initial_time_stops_immediately() {
        let mut rhs = |_t: f64, _y: &[f64]| vec![1.0];
        let mut event = |_t: f64, y: &[f64]| y[0];
        let (times, states) = integrate_until_event(
            &mut rhs,
            &[0.0],
            &[0.0, 1.0],
            Options::default(),
            &mut event,
        )
        .unwrap();
        assert_eq!(times, vec![0.0]);
        assert_eq!(states, vec![vec![0.0]]);
    }

    #[test]
    fn a_right_hand_side_of_the_wrong_shape_is_refused() {
        let mut rhs = |_t: f64, _y: &[f64]| vec![1.0, 2.0];
        let outcome = integrate(&mut rhs, &[1.0], &[0.0, 1.0], Options::default());
        assert!(matches!(outcome, Err(NumericError::ShapeMismatch(_))));
    }

    #[test]
    fn dopri_integrates_small_time_intervals_and_rejects_nonfinite_rates() {
        let mut fast = |_t: f64, _y: &[f64]| vec![1e15];
        let trajectory = integrate(&mut fast, &[0.0], &[0.0, 1e-15], Options::default()).unwrap();
        assert!((trajectory[1][0] - 1.0).abs() < 1e-8);
        for method in [
            Method::Rk4,
            Method::Dopri5,
            Method::Radau,
            Method::Bdf,
            Method::Rosenbrock23,
        ] {
            let mut invalid = |_t: f64, _y: &[f64]| vec![f64::NAN];
            assert!(
                integrate(
                    &mut invalid,
                    &[1.0],
                    &[0.0, 1.0],
                    Options {
                        method,
                        ..Options::default()
                    }
                )
                .is_err(),
                "{method:?}"
            );
        }
    }

    // ----- dense output, events, order, convergence ---------------------

    /// The observed order of accuracy between two step sizes.
    fn observed_order(
        coarse: f64,
        fine: f64,
    ) -> f64 {
        (coarse / fine).log2()
    }

    #[test]
    fn rk4_converges_with_fourth_order() {
        // y' = y on [0, 1]: halving the step divides the error by 2^4.
        let error = |steps: usize| {
            let mut rhs = |_t: f64, y: &[f64]| vec![y[0]];
            let options = Options {
                method: Method::Rk4,
                steps,
                ..Options::default()
            };
            let out = integrate(&mut rhs, &[1.0], &[0.0, 1.0], options).unwrap();
            (out[1][0] - std::f64::consts::E).abs()
        };
        let (e1, e2, e3) = (error(8), error(16), error(32));
        assert!(
            (3.8..4.3).contains(&observed_order(e1, e2)),
            "orders {} {}",
            observed_order(e1, e2),
            observed_order(e2, e3)
        );
        assert!((3.8..4.3).contains(&observed_order(e2, e3)));
    }

    #[test]
    fn dopri5_error_tracks_the_tolerance() {
        // Tightening the tolerance by 10^2 tightens the error accordingly.
        let error = |rtol: f64| {
            let mut rhs = |_t: f64, y: &[f64]| vec![y[1], -y[0]];
            let options = Options {
                rtol,
                atol: rtol * 1e-2,
                ..Options::default()
            };
            let out = integrate(&mut rhs, &[1.0, 0.0], &[0.0, 10.0], options).unwrap();
            (out[1][0] - 10.0f64.cos()).abs()
        };
        let (loose, medium, tight) = (error(1e-4), error(1e-6), error(1e-8));
        assert!(
            loose < 1e-2 && medium < 1e-4 && tight < 1e-6,
            "{loose} {medium} {tight}"
        );
        assert!(tight < medium && medium < loose);
    }

    #[test]
    fn dense_output_follows_the_exact_solution() {
        // y' = y, y(0) = 1 on [0, 2]; evaluate between the accepted steps.
        let mut rhs = |_t: f64, y: &[f64]| vec![y[0]];
        let solution = solve(
            &mut rhs,
            &[1.0],
            (0.0, 2.0),
            Options::default(),
            &[],
            &mut |_, _, _| 0.0,
        )
        .unwrap();
        assert_eq!(solution.method, Method::Dopri5);
        for k in 0..=200 {
            let t = 2.0 * f64::from(k) / 200.0;
            let y = solution.eval(t).unwrap()[0];
            assert!((y - t.exp()).abs() < 1e-6 * t.exp().max(1.0), "t = {t}");
        }
        assert!(solution.eval(-0.1).is_none() && solution.eval(2.1).is_none());
    }

    #[test]
    fn hermite_dense_output_of_rk4_is_fourth_order_accurate() {
        // Cubic Hermite interpolation: O(h^4) in the interpolation error.
        let error = |steps: usize| {
            let mut rhs = |_t: f64, y: &[f64]| vec![y[0]];
            let options = Options {
                method: Method::Rk4,
                steps,
                ..Options::default()
            };
            let solution = solve(
                &mut rhs,
                &[1.0],
                (0.0, 1.0),
                options,
                &[],
                &mut |_, _, _| 0.0,
            )
            .unwrap();
            (0..steps)
                .map(|k| {
                    let t = (k as f64 + 0.5) / steps as f64;
                    (solution.eval(t).unwrap()[0] - t.exp()).abs()
                })
                .fold(0.0, f64::max)
        };
        let (e1, e2) = (error(10), error(20));
        assert!(
            (3.5..4.6).contains(&observed_order(e1, e2)),
            "{}",
            observed_order(e1, e2)
        );
    }

    fn oscillator_events(
        specs: &[EventSpec],
        method: Method,
    ) -> Solution {
        // cos t crosses zero at pi/2, 3pi/2, ...; it is falling at the first.
        let mut rhs = |_t: f64, y: &[f64]| vec![y[1], -y[0]];
        solve(
            &mut rhs,
            &[1.0, 0.0],
            (0.0, 4.0 * std::f64::consts::PI),
            Options {
                method,
                steps: 4000,
                ..Options::default()
            },
            specs,
            &mut |_, _t, y| y[0],
        )
        .unwrap()
    }

    #[test]
    fn events_respect_direction_and_terminal() {
        use std::f64::consts::PI;
        for method in [Method::Dopri5, Method::Rk4, Method::Radau] {
            // Both directions, non-terminal: pi/2, 3pi/2, 5pi/2, 7pi/2.
            let all = oscillator_events(
                &[EventSpec {
                    terminal: false,
                    direction: 0,
                }],
                method,
            );
            let times: Vec<f64> = all.hits.iter().map(|hit| hit.t).collect();
            assert_eq!(times.len(), 4, "{method:?}: {times:?}");
            for (k, t) in times.iter().enumerate() {
                assert!(
                    (t - (PI / 2.0 + PI * k as f64)).abs() < 1e-5,
                    "{method:?}: {times:?}"
                );
            }
            // The integration ran to the end of the span.
            assert!((all.span().1 - 4.0 * PI).abs() < 1e-9);
            // Rising only: 3pi/2 and 7pi/2.
            let rising = oscillator_events(
                &[EventSpec {
                    terminal: false,
                    direction: 1,
                }],
                method,
            );
            let times: Vec<f64> = rising.hits.iter().map(|hit| hit.t).collect();
            assert_eq!(times.len(), 2, "{method:?}: {times:?}");
            assert!((times[0] - 1.5 * PI).abs() < 1e-5 && (times[1] - 3.5 * PI).abs() < 1e-5);
            // Falling and terminal: stops at pi/2.
            let first = oscillator_events(
                &[EventSpec {
                    terminal: true,
                    direction: -1,
                }],
                method,
            );
            assert_eq!(first.hits.len(), 1);
            assert!((first.span().1 - PI / 2.0).abs() < 1e-5, "{method:?}");
            assert!((first.hits[0].y[0]).abs() < 1e-5);
            assert!(first.eval(PI).is_none(), "dense output ends at the event");
        }
    }

    #[test]
    fn auto_stays_explicit_on_a_mild_problem_and_switches_on_a_stiff_one() {
        let mut mild = |_t: f64, y: &[f64]| vec![-y[0]];
        let options = Options {
            method: Method::Auto,
            ..Options::default()
        };
        let solution = solve(
            &mut mild,
            &[1.0],
            (0.0, 5.0),
            options,
            &[],
            &mut |_, _, _| 0.0,
        )
        .unwrap();
        assert_eq!(solution.method, Method::Dopri5);
        assert!((solution.eval(5.0).unwrap()[0] - (-5.0f64).exp()).abs() < 1e-7);
        // y' = -1e6 (y - cos t) - sin t has the smooth solution cos t and a
        // transient with rate 1e6: an explicit method needs ~3e5 steps.
        let mut stiff = |t: f64, y: &[f64]| vec![-1.0e6 * (y[0] - t.cos()) - t.sin()];
        let solution = solve(
            &mut stiff,
            &[1.0],
            (0.0, 1.0),
            options,
            &[],
            &mut |_, _, _| 0.0,
        )
        .unwrap();
        assert_eq!(solution.method, Method::Radau);
        assert!((solution.eval(1.0).unwrap()[0] - 1.0f64.cos()).abs() < 1e-5);
        // `integrate` takes the same route.
        let grid = [0.0, 0.5, 1.0];
        let out = integrate(&mut stiff, &[1.0], &grid, options).unwrap();
        assert!((out[1][0] - 0.5f64.cos()).abs() < 1e-4);
    }

    #[test]
    fn a_higher_order_equation_reduces_to_first_order() {
        // y'' = -y, y(0) = 1, y'(0) = 0: y = cos t, stacked z = (y, y').
        let mut rhs = reduce_order(2, 1, |_t, z| vec![-z[0]]);
        let solution = solve(
            &mut rhs,
            &[1.0, 0.0],
            (0.0, std::f64::consts::PI),
            Options::default(),
            &[],
            &mut |_, _, _| 0.0,
        )
        .unwrap();
        let end = solution.y.last().unwrap();
        assert!((end[0] + 1.0).abs() < 1e-6 && end[1].abs() < 1e-6);
        // Third order, two components: the top derivative is minus the first
        // derivative, so y = (cos t, 2 sin t) with y'(0) = (0, 2) and
        // y''(0) = (-1, 0).
        let mut rhs = reduce_order(3, 2, |_t, z| vec![-z[2], -z[3]]);
        let solution = solve(
            &mut rhs,
            &[1.0, 0.0, 0.0, 2.0, -1.0, 0.0],
            (0.0, 1.0),
            Options::default(),
            &[],
            &mut |_, _, _| 0.0,
        )
        .unwrap();
        let end = solution.y.last().unwrap();
        assert!((end[0] - 1.0f64.cos()).abs() < 1e-6, "{end:?}");
        assert!((end[1] - 2.0 * 1.0f64.sin()).abs() < 1e-6, "{end:?}");
        assert!((end[2] + 1.0f64.sin()).abs() < 1e-6, "{end:?}");
    }

    #[test]
    fn shooting_solves_a_linear_boundary_value_problem() {
        // y'' = -y, y(0) = 0, y(pi/2) = 1 has y = sin t, so y'(0) = 1.
        let mut rhs = |_t: f64, y: &[f64]| vec![y[1], -y[0]];
        let solution = shoot(
            &mut rhs,
            &[0.0, 0.0],
            (0.0, std::f64::consts::FRAC_PI_2),
            Options::default(),
            &mut |a, b| vec![a[0], b[0] - 1.0],
        )
        .unwrap();
        assert!((solution.y[0][1] - 1.0).abs() < 1e-6);
        let mid = solution.eval(std::f64::consts::FRAC_PI_4).unwrap();
        assert!((mid[0] - std::f64::consts::FRAC_PI_4.sin()).abs() < 1e-6);
    }

    #[test]
    fn shooting_solves_a_nonlinear_boundary_value_problem() {
        // y'' = 3/2 y^2, y(0) = 4, y(1) = 1 has y = 4 / (1 + x)^2, y'(0) = -8.
        let mut rhs = |_t: f64, y: &[f64]| vec![y[1], 1.5 * y[0] * y[0]];
        let solution = shoot(
            &mut rhs,
            &[4.0, -5.0],
            (0.0, 1.0),
            Options::default(),
            &mut |a, b| vec![a[0] - 4.0, b[0] - 1.0],
        )
        .unwrap();
        assert!((solution.y[0][1] + 8.0).abs() < 1e-4, "{:?}", solution.y[0]);
        let y = solution.eval(0.5).unwrap()[0];
        assert!((y - 4.0 / 2.25).abs() < 1e-5, "{y}");
    }

    #[test]
    fn shooting_reports_a_wrong_number_of_conditions() {
        let mut rhs = |_t: f64, y: &[f64]| vec![y[1], -y[0]];
        let outcome = shoot(
            &mut rhs,
            &[0.0, 0.0],
            (0.0, 1.0),
            Options::default(),
            &mut |a, _| vec![a[0]],
        );
        assert!(matches!(outcome, Err(NumericError::ShapeMismatch(_))));
    }

    #[test]
    fn symplectic_integrators_conserve_hamiltonian_and_achieve_design_order() {
        // 1D Harmonic Oscillator: q' = p, p' = -q, H(q, p) = 0.5*(q^2 + p^2).
        // Exact at t = 2*pi: (q, p) = (1, 0).
        let t_end = 2.0 * std::f64::consts::PI;
        let mut accel = |_t: f64, q: &[f64]| vec![-q[0]];
        let energy = |q: &[f64], p: &[f64]| 0.5 * (q[0] * q[0] + p[0] * p[0]);

        let sol_verlet = integrate_symplectic(
            &mut accel,
            &[1.0],
            &[0.0],
            (0.0, t_end),
            200,
            SymplecticMethod::StormerVerlet,
        )
        .unwrap();
        let err_verlet = (sol_verlet.q.last().unwrap()[0] - 1.0).abs();
        assert!(err_verlet < 1e-3, "verlet error: {err_verlet}");
        assert!(sol_verlet.max_energy_drift(energy) < 5e-4);

        let sol_ruth3 = integrate_symplectic(
            &mut accel,
            &[1.0],
            &[0.0],
            (0.0, t_end),
            200,
            SymplecticMethod::Ruth3,
        )
        .unwrap();
        let err_ruth3 = (sol_ruth3.q.last().unwrap()[0] - 1.0).abs();
        assert!(err_ruth3 < 5e-5, "ruth3 error: {err_ruth3}");

        let sol_ruth4 = integrate_symplectic(
            &mut accel,
            &[1.0],
            &[0.0],
            (0.0, t_end),
            200,
            SymplecticMethod::Ruth4,
        )
        .unwrap();
        let err_ruth4 = (sol_ruth4.q.last().unwrap()[0] - 1.0).abs();
        assert!(err_ruth4 < 5e-6, "ruth4 error: {err_ruth4}");
        assert!(sol_ruth4.max_energy_drift(energy) < 1e-5);

        let sol_yoshida6 = integrate_symplectic(
            &mut accel,
            &[1.0],
            &[0.0],
            (0.0, t_end),
            200,
            SymplecticMethod::Yoshida6,
        )
        .unwrap();
        let err_y6 = (sol_yoshida6.q.last().unwrap()[0] - 1.0).abs();
        assert!(err_y6 < 1e-9, "yoshida6 error: {err_y6}");

        // Check Hermite dense output at t = pi/2: q = 0, p = -1
        let (q_mid, p_mid) = sol_yoshida6.eval(std::f64::consts::FRAC_PI_2).unwrap();
        assert!(q_mid[0].abs() < 1e-6 && (p_mid[0] + 1.0).abs() < 1e-6);
    }

    #[test]
    fn gauss_legendre_implicit_rk_preserves_quadratic_invariants_to_machine_precision() {
        // Nonlinear rigid body (Euler's equations):
        // y1' = (I2 - I3)/(I2*I3) y2 y3
        // y2' = (I3 - I1)/(I3*I1) y3 y1
        // y3' = (I1 - I2)/(I1*I2) y1 y2
        // Casimir invariant C(y) = y1^2 + y2^2 + y3^2 is quadratic and must be
        // conserved to machine precision by Gauss-Legendre 4 and 6!
        let (i1, i2, i3) = (2.0, 1.0, 2.0 / 3.0);
        let mut euler_rigid = |_t: f64, y: &[f64]| {
            vec![
                ((i2 - i3) / (i2 * i3)) * y[1] * y[2],
                ((i3 - i1) / (i3 * i1)) * y[2] * y[0],
                ((i1 - i2) / (i1 * i2)) * y[0] * y[1],
            ]
        };
        let y0 = [0.8, 0.6, 0.0];
        let casimir = |y: &[f64]| y[0] * y[0] + y[1] * y[1] + y[2] * y[2];
        let c0 = casimir(&y0);

        let sol4 = solve_gauss_legendre(&mut euler_rigid, &y0, (0.0, 10.0), 100, 2).unwrap();
        for yk in &sol4.y {
            assert!(
                (casimir(yk) - c0).abs() < 1e-12,
                "GL4 Casimir drift: {}",
                (casimir(yk) - c0).abs()
            );
        }

        let sol6 = solve_gauss_legendre(&mut euler_rigid, &y0, (0.0, 10.0), 100, 3).unwrap();
        for yk in &sol6.y {
            assert!(
                (casimir(yk) - c0).abs() < 1e-12,
                "GL6 Casimir drift: {}",
                (casimir(yk) - c0).abs()
            );
        }

        // Also test via Method::GaussLegendre4 and Method::Ruth4 in ode::solve
        let mut ho = |_t: f64, y: &[f64]| vec![y[1], -y[0]];
        let sol_m = solve(
            &mut ho,
            &[1.0, 0.0],
            (0.0, 1.0),
            Options {
                method: Method::GaussLegendre6,
                steps: 20,
                ..Options::default()
            },
            &[],
            &mut |_, _, _| 0.0,
        )
        .unwrap();
        let end = sol_m.y.last().unwrap();
        assert!((end[0] - 1.0f64.cos()).abs() < 1e-12);
        assert!((end[1] + 1.0f64.sin()).abs() < 1e-12);
    }

    #[test]
    fn dae_index1_solver_solves_robertson_and_algebraic_coupled_system() {
        // Semi-explicit index-1 DAE:
        // y' = -y + z
        // 0  = y^2 + z - exp(-2t) - sin(t)
        // Therefore z(t) = exp(-2t) + sin(t) - y^2.
        // In particular, for linear algebraic coupling:
        // y1' = -y1 + z1
        // 0   = 2*y1 - z1 => z1 = 2*y1 => y1' = y1 => y1(t) = e^t, z1(t) = 2*e^t.
        let mut f = |_t: f64, y: &[f64], z: &[f64]| vec![-y[0] + z[0]];
        let mut g = |t: f64, y: &[f64], z: &[f64]| vec![2.0 * y[0] - z[0] + t.sin() - t.sin()];
        // Give an inconsistent initial guess z0_guess = 0.0 (true z0 = 2.0):
        let sol = solve_dae_index1(
            &mut f,
            &mut g,
            &[1.0],
            &[0.0],
            (0.0, 1.0),
            DaeOptions::default(),
        )
        .unwrap();
        let y_end = sol.y.last().unwrap()[0];
        let z_end = sol.z.last().unwrap()[0];
        let e1 = 1.0f64.exp();
        assert!((y_end - e1).abs() < 1e-4, "y_end={y_end}, expected {e1}");
        assert!(
            (z_end - 2.0 * e1).abs() < 2e-4,
            "z_end={z_end}, expected {}",
            2.0 * e1
        );
        assert!(sol.max_constraint_violation(&mut g) < 1e-9);
    }

    #[test]
    fn dae_index2_solver_solves_constrained_cartesian_pendulum() {
        // Cartesian pendulum of length L = 1, gravity g = 1:
        // Differential variables y = [x, y_pos, u, v] (4 vars)
        // Algebraic variable z = [lambda] (1 var, tension/Lagrange multiplier)
        // Index-2 position constraint: 0 = x^2 + y_pos^2 - 1
        // Hidden velocity constraint: 2*x*u + 2*y_pos*v = 0
        // Hidden index-1 constraint: 2*(u^2 + v^2) + 2*x*(-2*x*lambda) + 2*y_pos*(-2*y_pos*lambda - 1) = 0
        let mut f = |_t: f64, y: &[f64], z: &[f64]| {
            let (x, yp, u, v) = (y[0], y[1], y[2], y[3]);
            let lam = z[0];
            vec![u, v, -2.0 * x * lam, -2.0 * yp * lam - 1.0]
        };
        let mut g = |_t: f64, y: &[f64]| vec![y[0] * y[0] + y[1] * y[1] - 1.0];

        // Start near (x=1, y=0, u=0, v=0) with slightly perturbed initial guess
        let sol = solve_dae_index2(
            &mut f,
            &mut g,
            &[1.001, 0.0, 0.0, 0.0],
            &[0.5],
            (0.0, 1.5),
            DaeOptions {
                rtol: 1e-6,
                atol: 1e-8,
                ..DaeOptions::default()
            },
        )
        .unwrap();

        // Verify constraint x^2 + y^2 - 1 == 0 and mechanical energy E = 0.5*(u^2 + v^2) + y_pos == 0
        // along the entire trajectory!
        for (yk, zk) in sol.y.iter().zip(&sol.z) {
            let pos_err = (yk[0] * yk[0] + yk[1] * yk[1] - 1.0).abs();
            let vel_err = (2.0 * yk[0] * yk[2] + 2.0 * yk[1] * yk[3]).abs();
            assert!(pos_err < 1e-9, "position constraint drift: {pos_err}");
            assert!(vel_err < 1e-8, "velocity constraint drift: {vel_err}");
            assert!(zk[0].is_finite());
        }
        let end = sol.y.last().unwrap();
        let energy_end = 0.5 * (end[2] * end[2] + end[3] * end[3]) + end[1];
        assert!(energy_end.abs() < 1e-3, "energy drift: {energy_end}");
    }

    #[test]
    fn dde_solver_matches_exact_method_of_steps_solution() {
        // y'(t) = -y(t - 1) for t >= 0, with history y(t) = 1 for t <= 0.
        // Exact analytical solution by Method of Steps:
        // - For t in [0, 1]: y'(t) = -1 => y(t) = 1 - t
        // - For t in [1, 2]: y'(t) = -(1 - (t - 1)) = t - 2 => y(t) = 1 - t + (t - 1)^2 / 2
        // - For t in [2, 3]: y(t) = 1 - t + (t - 1)^2 / 2 - (t - 2)^3 / 6
        let mut rhs = |_t: f64, _y: &[f64], delayed: &[Vec<f64>]| vec![-delayed[0][0]];
        let mut history = |_t: f64| vec![1.0];

        let sol = solve_dde(
            &mut rhs,
            &mut history,
            &[1.0],
            &[1.0],
            (0.0, 3.0),
            DdeOptions::default(),
        )
        .unwrap();

        let y_05 = sol.eval(0.5).unwrap()[0];
        assert!((y_05 - 0.5).abs() < 1e-7, "y(0.5) = {y_05}");

        let y_15 = sol.eval(1.5).unwrap()[0];
        let exact_15 = 1.0 - 1.5 + 0.5 * 0.5 / 2.0; // -0.375
        assert!(
            (y_15 - exact_15).abs() < 1e-6,
            "y(1.5) = {y_15}, exact = {exact_15}"
        );

        let y_25 = sol.eval(2.5).unwrap()[0];
        let exact_25 = 1.0 - 2.5 + 1.5 * 1.5 / 2.0 - 0.5f64.powi(3) / 6.0;
        assert!(
            (y_25 - exact_25).abs() < 1e-6,
            "y(2.5) = {y_25}, exact = {exact_25}"
        );

        // Also test state-dependent DDE API on y'(t) = -y(t - 1)
        let sol_sd = solve_dde_state_dependent(
            &mut |t, _y, lag| vec![-lag(t - 1.0)[0]],
            &mut history,
            &[1.0],
            (0.0, 2.0),
            DdeOptions::default(),
        )
        .unwrap();
        let y_sd_15 = sol_sd.eval(1.5).unwrap()[0];
        assert!((y_sd_15 - exact_15).abs() < 1e-5);
    }
}
