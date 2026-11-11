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
            | Self::Auto => None,
        }
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
}

impl Default for Options {
    fn default() -> Self {
        Self {
            method: Method::Dopri5,
            rtol: 1e-8,
            atol: 1e-10,
            steps: 20,
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
    if options.method == Method::Auto {
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
        ..OdeOptions::default()
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
                | Method::Rk4 | Method::Auto => unreachable!(),
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
    if matches!(
        options.method,
        Method::Dopri5 | Method::Radau | Method::Bdf | Method::Rosenbrock23 | Method::Auto
    ) && (!options.rtol.is_finite()
        || !options.atol.is_finite()
        || options.rtol < 0.0
        || options.atol <= 0.0)
    {
        return Err(NumericError::Domain(
            "adaptive tolerances must be finite, with rtol >= 0 and atol > 0".into(),
        ));
    }
    if ts
        .windows(2)
        .any(|pair| pair[1] < pair[0] || !(pair[1] - pair[0]).is_finite())
    {
        return Err(NumericError::Domain(
            "the time grid must be increasing with finite intervals".into(),
        ));
    }
    if matches!(
        options.method,
        Method::Radau | Method::Bdf | Method::Rosenbrock23
    ) {
        return stiff_integrate(rhs, y0, ts, options);
    }
    if options.method == Method::Auto {
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
    let mut out = Vec::with_capacity(ts.len());
    let mut y = y0.to_vec();
    let mut t = ts[0];
    out.push(y.clone());
    for target in &ts[1..] {
        if *target < t {
            return Err(NumericError::Domain(
                "the time grid must be increasing".to_string(),
            ));
        }
        if !(*target - t).is_finite() {
            return Err(NumericError::Domain(
                "the time interval is too large".into(),
            ));
        }
        match options.method {
            | Method::Rk4 => {
                let span = target - t;
                let steps = options.steps.max(1);
                let h = span / steps as f64;
                let mut time = t;
                for _ in 0..steps {
                    y = rk4_step(rhs, time, &y, h)?;
                    time += h;
                }
                t = *target;
            },
            | Method::Dopri5 => {
                let (state, time) = dopri5(rhs, &y, t, *target, options)?;
                y = state;
                t = time;
            },
            | Method::Radau | Method::Bdf | Method::Rosenbrock23 | Method::Auto => {
                unreachable!("handled above")
            },
        }
        out.push(y.clone());
    }
    Ok(out)
}

fn rk4_step(
    rhs: &mut Rate<'_>,
    t: f64,
    y: &[f64],
    h: f64,
) -> Result<Vec<f64>, NumericError> {
    let k1 = evaluate(rhs, t, y, y.len())?;
    let k2 = evaluate(rhs, t + h / 2.0, &add(y, &k1, h / 2.0), y.len())?;
    let k3 = evaluate(rhs, t + h / 2.0, &add(y, &k2, h / 2.0), y.len())?;
    let k4 = evaluate(rhs, t + h, &add(y, &k3, h), y.len())?;
    let mut out = y.to_vec();
    for index in 0..y.len() {
        out[index] += h / 6.0 * (k1[index] + 2.0 * k2[index] + 2.0 * k3[index] + k4[index]);
    }
    if !out.iter().all(|value| value.is_finite()) {
        return Err(NumericError::NotConverged);
    }
    Ok(out)
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

fn add(
    y: &[f64],
    rate: &[f64],
    scale: f64,
) -> Vec<f64> {
    y.iter()
        .zip(rate)
        .map(|(value, slope)| value + scale * slope)
        .collect()
}

/// Run one RSSN solve over the whole grid, preserving BDF history and using
/// the method's continuous extension for the requested output times.
fn stiff_integrate(
    rhs: &mut Rate<'_>,
    y0: &[f64],
    ts: &[f64],
    options: Options,
) -> Result<Vec<Vec<f64>>, NumericError> {
    use rssn::kernels::ode_adaptive::OdeOptions;
    use rssn::kernels::ode_adaptive::{
        self,
    };
    use rssn::kernels::ode_stiff;
    use std::cell::RefCell;

    let start = ts[0];
    let end = ts[ts.len() - 1];
    if start == end || y0.is_empty() {
        return Ok(vec![y0.to_vec(); ts.len()]);
    }
    if !(end - start).is_finite() {
        return Err(NumericError::Domain(
            "the time interval is too large".into(),
        ));
    }
    let failure = RefCell::new(None);
    let rhs = RefCell::new(rhs);
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
    let opts = OdeOptions {
        rtol: options.rtol,
        atol: options.atol,
        ..OdeOptions::default()
    };
    let solution = match options.method {
        | Method::Radau => ode_stiff::radau5(f, start, end, y0, &opts),
        | Method::Bdf => ode_stiff::bdf(f, start, end, y0, &opts, 5),
        | Method::Rosenbrock23 => ode_adaptive::rosenbrock23(f, start, end, y0, &opts),
        | _ => unreachable!("only stiff methods are routed here"),
    };
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }
    let solution = solution.map_err(|_| NumericError::NotConverged)?;
    ts.iter()
        .map(|&t| {
            if t == start {
                Ok(y0.to_vec())
            } else if t == end {
                solution.y.last().cloned().ok_or(NumericError::NotConverged)
            } else {
                solution.eval(t).ok_or(NumericError::NotConverged)
            }
        })
        .collect()
}

/// Dormand–Prince 5(4) with step-size control.
fn dopri5(
    rhs: &mut Rate<'_>,
    y0: &[f64],
    t0: f64,
    t1: f64,
    options: Options,
) -> Result<(Vec<f64>, f64), NumericError> {
    const A21: f64 = 1.0 / 5.0;
    const A31: f64 = 3.0 / 40.0;
    const A32: f64 = 9.0 / 40.0;
    const A41: f64 = 44.0 / 45.0;
    const A42: f64 = -56.0 / 15.0;
    const A43: f64 = 32.0 / 9.0;
    const A51: f64 = 19372.0 / 6561.0;
    const A52: f64 = -25360.0 / 2187.0;
    const A53: f64 = 64448.0 / 6561.0;
    const A54: f64 = -212.0 / 729.0;
    const A61: f64 = 9017.0 / 3168.0;
    const A62: f64 = -355.0 / 33.0;
    const A63: f64 = 46732.0 / 5247.0;
    const A64: f64 = 49.0 / 176.0;
    const A65: f64 = -5103.0 / 18656.0;
    const B1: f64 = 35.0 / 384.0;
    const B3: f64 = 500.0 / 1113.0;
    const B4: f64 = 125.0 / 192.0;
    const B5: f64 = -2187.0 / 6784.0;
    const B6: f64 = 11.0 / 84.0;
    // The embedded fourth-order weights, for the error estimate.
    const E1: f64 = 5179.0 / 57600.0 - B1;
    const E3: f64 = 7571.0 / 16695.0 - B3;
    const E4: f64 = 393.0 / 640.0 - B4;
    const E5: f64 = -92097.0 / 339200.0 - B5;
    const E6: f64 = 187.0 / 2100.0 - B6;
    const E7: f64 = 1.0 / 40.0;

    let n = y0.len();
    let mut y = y0.to_vec();
    let mut t = t0;
    let span = t1 - t0;
    if span == 0.0 {
        return Ok((y, t));
    }
    let mut h = (span / 20.0).abs().min(span.abs());
    if h == 0.0 {
        return Ok((y, t));
    }
    let mut guard = 0;
    while t < t1 {
        guard += 1;
        if guard > 1_000_000 {
            return Err(NumericError::NotConverged);
        }
        let remaining = t1 - t;
        let step = if (h.abs() > remaining.abs()) && (h * remaining > 0.0) {
            remaining
        } else {
            h
        };
        if !step.is_finite() || step <= 0.0 || t + step == t {
            return Err(NumericError::NotConverged);
        }
        let k1 = evaluate(rhs, t, &y, n)?;
        let k2 = evaluate(rhs, t + step / 5.0, &add(&y, &k1, step * A21), n)?;
        let mut stage: Vec<f64> = (0..n)
            .map(|index| step * (A31 * k1[index] + A32 * k2[index]))
            .collect();
        let k3 = evaluate(rhs, t + 3.0 * step / 10.0, &add(&y, &stage, 1.0), n)?;
        for index in 0..n {
            stage[index] = step * (A41 * k1[index] + A42 * k2[index] + A43 * k3[index]);
        }
        let k4 = evaluate(rhs, t + 4.0 * step / 5.0, &add(&y, &stage, 1.0), n)?;
        for index in 0..n {
            stage[index] =
                step * (A51 * k1[index] + A52 * k2[index] + A53 * k3[index] + A54 * k4[index]);
        }
        let k5 = evaluate(rhs, t + 8.0 * step / 9.0, &add(&y, &stage, 1.0), n)?;
        for index in 0..n {
            stage[index] = step
                * (A61 * k1[index]
                    + A62 * k2[index]
                    + A63 * k3[index]
                    + A64 * k4[index]
                    + A65 * k5[index]);
        }
        let k6 = evaluate(rhs, t + step, &add(&y, &stage, 1.0), n)?;
        let mut candidate = y.clone();
        for index in 0..n {
            candidate[index] += step
                * (B1 * k1[index]
                    + B3 * k3[index]
                    + B4 * k4[index]
                    + B5 * k5[index]
                    + B6 * k6[index]);
        }
        if !candidate.iter().all(|value| value.is_finite()) {
            return Err(NumericError::NotConverged);
        }
        let k7 = evaluate(rhs, t + step, &candidate, n)?;
        // Error estimate, scaled to the tolerances.
        let mut error = 0.0f64;
        for index in 0..n {
            let estimate = step
                * (E1 * k1[index]
                    + E3 * k3[index]
                    + E4 * k4[index]
                    + E5 * k5[index]
                    + E6 * k6[index]
                    + E7 * k7[index]);
            let scale = options.atol + options.rtol * y[index].abs().max(candidate[index].abs());
            error = error.max((estimate / scale).abs());
        }
        if !error.is_finite() {
            return Err(NumericError::NotConverged);
        }
        if error <= 1.0 {
            t = if step == remaining {
                t1
            } else {
                t + step
            };
            y = candidate;
        }
        // 0.9 * error^(-1/5) keeps the step inside the stability region.
        let factor = if error == 0.0 {
            5.0
        } else {
            (0.9 * error.powf(-0.2)).clamp(0.2, 5.0)
        };
        h = step * factor;
    }
    Ok((y, t))
}

// ---------------------------------------------------------------------------
// Solutions with dense output and events (§8.9)
// ---------------------------------------------------------------------------

/// How one event function is watched: a sign change of `g(t, y)` is an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventSpec {
    /// Integration stops at the event.
    pub terminal: bool,
    /// `1` rising only, `-1` falling only, `0` both.
    pub direction: i8,
}

/// The event functions of [`solve`]: `(index, t, y) -> g`.
pub type Events<'a> = dyn FnMut(usize, f64, &[f64]) -> f64 + 'a;

/// A located event.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    /// Index of the event function.
    pub index: usize,
    /// The event time.
    pub t: f64,
    /// The state at the event time.
    pub y: Vec<f64>,
}

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
                let h = self.t[upper] - self.t[lower];
                let s = (t - self.t[lower]) / h;
                let (h00, h10, h01, h11) = (
                    2.0 * s * s * s - 3.0 * s * s + 1.0,
                    s * s * s - 2.0 * s * s + s,
                    -2.0 * s * s * s + 3.0 * s * s,
                    s * s * s - s * s,
                );
                Some(
                    (0..self.y[lower].len())
                        .map(|index| {
                            h00 * self.y[lower][index]
                                + h10 * h * rates[lower][index]
                                + h01 * self.y[upper][index]
                                + h11 * h * rates[upper][index]
                        })
                        .collect(),
                )
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
    let rhs_cell = RefCell::new(rhs);
    let failure: RefCell<Option<NumericError>> = RefCell::new(None);
    let f = |t: f64, y: &[f64], out: &mut [f64]| {
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
        ..OdeOptions::default()
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
    let kernel = |method: Method| -> Result<Solution, NumericError> {
        let solution = match method {
            | Method::Dopri5 => ode_adaptive::dopri5(&f, start, end, y0, &opts, &kernel_events),
            | Method::Radau => ode_stiff::radau5_events(&f, start, end, y0, &opts, &kernel_events),
            | Method::Bdf => ode_stiff::bdf_events(&f, start, end, y0, &opts, 5, &kernel_events),
            | Method::Rosenbrock23 => {
                ode_adaptive::rosenbrock23_events(&f, start, end, y0, &opts, &kernel_events)
            },
            | Method::Rk4 | Method::Auto => unreachable!("handled by the caller"),
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
                    &f,
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
            | Method::Auto => {
                let budgeted = OdeOptions {
                    max_steps: AUTO_STEP_BUDGET,
                    ..opts
                };
                match ode_adaptive::dopri5(&f, start, end, y0, &budgeted, &kernel_events) {
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
                        kernel(Method::Radau)
                    },
                    | Err(_) => Err(NumericError::NotConverged),
                }
            },
            | method => kernel(method),
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
    let n = guess.len();
    let integrate_from = |rhs: &mut Rate<'_>, y0: &[f64]| -> Result<Solution, NumericError> {
        solve(rhs, y0, span, options, &[], &mut |_, _, _| 0.0)
    };
    let mut residual = |rhs: &mut Rate<'_>, p: &[f64]| -> Result<Vec<f64>, NumericError> {
        let solution = integrate_from(rhs, p)?;
        let end = solution.y.last().ok_or(NumericError::NotConverged)?;
        let r = boundary(p, end);
        if r.len() != n {
            return Err(NumericError::ShapeMismatch(format!(
                "the boundary condition returned {} residual(s) for a state of {n} component(s)",
                r.len()
            )));
        }
        if !r.iter().all(|value| value.is_finite()) {
            return Err(NumericError::Domain(
                "the boundary condition returned a non-finite value".into(),
            ));
        }
        Ok(r)
    };
    let norm = |v: &[f64]| v.iter().fold(0.0f64, |acc, x| acc.max(x.abs()));
    let mut p = guess.to_vec();
    let mut r = residual(rhs, &p)?;
    for _ in 0..60 {
        // The scale of the residual is the scale of the data, not 1.
        if norm(&r) <= 1e-9 * (1.0 + norm(&p)) {
            return integrate_from(rhs, &p);
        }
        // Forward-difference Jacobian, one column per initial component.
        let mut jacobian = vec![vec![0.0; n]; n];
        for column in 0..n {
            let step = f64::EPSILON.sqrt() * p[column].abs().max(1.0);
            let mut shifted = p.clone();
            shifted[column] += step;
            let shifted_r = residual(rhs, &shifted)?;
            for row in 0..n {
                jacobian[row][column] = (shifted_r[row] - r[row]) / step;
            }
        }
        let delta = solve_dense(jacobian, r.iter().map(|x| -x).collect())?;
        // Backtracking: accept the first step that reduces the residual.
        let mut scale = 1.0;
        let mut accepted = false;
        for _ in 0..30 {
            let trial: Vec<f64> = p.iter().zip(&delta).map(|(x, d)| x + scale * d).collect();
            if let Ok(trial_r) = residual(rhs, &trial)
                && norm(&trial_r) < norm(&r)
            {
                p = trial;
                r = trial_r;
                accepted = true;
                break;
            }
            scale *= 0.5;
        }
        if !accepted {
            return Err(NumericError::NotConverged);
        }
    }
    if norm(&r) <= 1e-9 * (1.0 + norm(&p)) {
        return integrate_from(rhs, &p);
    }
    Err(NumericError::NotConverged)
}

/// Gaussian elimination with partial pivoting for the small Newton systems of
/// [`shoot`].
fn solve_dense(
    mut a: Vec<Vec<f64>>,
    mut b: Vec<f64>,
) -> Result<Vec<f64>, NumericError> {
    let n = b.len();
    for column in 0..n {
        let pivot = (column..n)
            .max_by(|&i, &j| a[i][column].abs().total_cmp(&a[j][column].abs()))
            .unwrap_or(column);
        if a[pivot][column].abs() < 1e-300 || !a[pivot][column].is_finite() {
            return Err(NumericError::Singular);
        }
        a.swap(column, pivot);
        b.swap(column, pivot);
        for row in column + 1..n {
            let factor = a[row][column] / a[column][column];
            for k in column..n {
                a[row][k] -= factor * a[column][k];
            }
            b[row] -= factor * b[column];
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let tail: f64 = (row + 1..n).map(|k| a[row][k] * x[k]).sum();
        x[row] = (b[row] - tail) / a[row][row];
    }
    if x.iter().any(|value| !value.is_finite()) {
        return Err(NumericError::Singular);
    }
    Ok(x)
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
}
