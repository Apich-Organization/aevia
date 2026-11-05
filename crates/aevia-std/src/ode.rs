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
}

impl Method {
    /// The capability this method needs, when it is not implemented.
    #[must_use]
    pub fn capability(&self) -> Option<&'static str> {
        match self {
            | Self::Rk4 | Self::Dopri5 | Self::Radau | Self::Bdf | Self::Rosenbrock23 => None,
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
    use rssn::kernels::ode_adaptive::{self, Event, OdeOptions};
    use rssn::kernels::{ode, ode_stiff};
    use std::cell::{Cell, RefCell};

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
    let rhs = RefCell::new(rhs);
    let failure = RefCell::new(None);
    let f = |t: f64, y: &[f64], out: &mut [f64]| match evaluate(
        &mut **rhs.borrow_mut(),
        t,
        y,
        out.len(),
    ) {
        | Ok(rate) => out.copy_from_slice(&rate),
        | Err(error) => {
            if failure.borrow().is_none() {
                *failure.borrow_mut() = Some(error);
            }
            out.fill(f64::NAN);
        },
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
                    &f,
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
                | Method::Dopri5 => ode_adaptive::dopri5(&f, start, end, y0, &opts, &events),
                | Method::Radau => ode_stiff::radau5_events(&f, start, end, y0, &opts, &events),
                | Method::Bdf => ode_stiff::bdf_events(&f, start, end, y0, &opts, 5, &events),
                | Method::Rosenbrock23 => {
                    ode_adaptive::rosenbrock23_events(&f, start, end, y0, &opts, &events)
                },
                | Method::Rk4 => unreachable!(),
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
        Method::Dopri5 | Method::Radau | Method::Bdf | Method::Rosenbrock23
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
            | Method::Radau | Method::Bdf | Method::Rosenbrock23 => unreachable!("handled above"),
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
        return Err(NumericError::Domain("the ODE right-hand side returned a non-finite value".into()));
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
    use rssn::kernels::ode_adaptive::{self, OdeOptions};
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
    let f = |t: f64, y: &[f64], out: &mut [f64]| match evaluate(
        &mut **rhs.borrow_mut(),
        t,
        y,
        out.len(),
    ) {
        | Ok(rate) => out.copy_from_slice(&rate),
        | Err(error) => {
            if failure.borrow().is_none() {
                *failure.borrow_mut() = Some(error);
            }
            out.fill(f64::NAN);
        },
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
            t = if step == remaining { t1 } else { t + step };
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
        for method in [Method::Rk4, Method::Dopri5, Method::Radau, Method::Bdf, Method::Rosenbrock23] {
            let mut invalid = |_t: f64, _y: &[f64]| vec![f64::NAN];
            assert!(integrate(&mut invalid, &[1.0], &[0.0, 1.0], Options { method, ..Options::default() }).is_err(), "{method:?}");
        }
    }
}
