//! Ordinary differential equations (§8.9).
//!
//! The state is a flat `Vec<f64>`: the runtime flattens a record or a tensor in
//! field order and restores it afterwards (§8.9's flattening contract), so the
//! integrator only ever sees numbers.
//!
//! `Rk4` is the fixed-step method and needs a step count; `Dopri5` is the
//! adaptive Dormand–Prince pair. The three stiff methods of §8.9 (`Radau`,
//! `Bdf`, `Rosenbrock23`) are declared but not implemented here: they are
//! reported as a named capability gap rather than silently integrated with the
//! explicit method, which would answer a stiff problem with a wrong number.

use crate::NumericError;

/// The integration method of §8.9.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// Fixed-step fourth-order Runge–Kutta.
    Rk4,
    /// Adaptive Dormand–Prince 5(4).
    Dopri5,
    /// Implicit Radau IIA — not implemented in this build.
    Radau,
    /// Implicit BDF — not implemented in this build.
    Bdf,
    /// Rosenbrock 2/3 — not implemented in this build.
    Rosenbrock23,
}

impl Method {
    /// The capability this method needs, when it is not implemented.
    #[must_use]
    pub fn capability(&self) -> Option<&'static str> {
        match self {
            | Self::Rk4 | Self::Dopri5 => None,
            | Self::Radau => Some("ode.radau"),
            | Self::Bdf => Some("ode.bdf"),
            | Self::Rosenbrock23 => Some("ode.rosenbrock"),
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
            | other => {
                return Err(NumericError::Domain(format!(
                    "the `{other:?}` method is not available in this build"
                )));
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
    while (t1 - t).abs() > 1e-14 * t1.abs().max(1.0) {
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
        if error <= 1.0 || step.abs() <= 1e-14 * t.abs().max(1.0) {
            t += step;
            y = candidate;
        }
        // 0.9 * error^(-1/5) keeps the step inside the stability region.
        let factor = if error == 0.0 {
            5.0
        } else {
            (0.9 * error.powf(-0.2)).clamp(0.2, 5.0)
        };
        h = step * factor;
        if h.abs() < 1e-14 * t.abs().max(1.0) {
            if error > 1.0 {
                return Err(NumericError::NotConverged);
            }
            break;
        }
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
    fn a_stiff_method_is_a_named_gap() {
        let mut rhs = |_t: f64, y: &[f64]| vec![-1000.0 * y[0]];
        let outcome = integrate(
            &mut rhs,
            &[1.0],
            &[0.0, 1.0],
            Options {
                method: Method::Radau,
                ..Options::default()
            },
        );
        assert!(matches!(outcome, Err(NumericError::Domain(_))));
        assert_eq!(Method::Radau.capability(), Some("ode.radau"));
        assert_eq!(Method::Dopri5.capability(), None);
    }

    #[test]
    fn a_right_hand_side_of_the_wrong_shape_is_refused() {
        let mut rhs = |_t: f64, _y: &[f64]| vec![1.0, 2.0];
        let outcome = integrate(&mut rhs, &[1.0], &[0.0, 1.0], Options::default());
        assert!(matches!(outcome, Err(NumericError::ShapeMismatch(_))));
    }
}
