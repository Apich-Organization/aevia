//! Stochastic differential equations (§8.9.1, new in this round).
//!
//! The differential-equation chapter now has a second surface next to
//! [`crate::ode`]: `dX = a(t, X) dt + G(t, X) dW` with a typed drift and a
//! diffusion, integrated by the schemes of `rssn::kernels::sde`. This crate owns
//! the *interface*: method/calculus/structure selection, the state as a flat
//! `Vec<f64>` (the runtime flattens and rebuilds records and tensors exactly as
//! for the ODE path, §8.9), and one deterministic RNG contract.
//!
//! # What is implemented
//!
//! | method | calculus | noise it may be used with | order (strong/weak) |
//! |---|---|---|---|
//! | `EulerMaruyama` | Itô | diagonal, commutative general, non-commutative general | 0.5 / 1 |
//! | `Milstein` | Itô | diagonal, commutative general | 1 / 1 |
//! | `MilsteinStratonovich` | Stratonovich | diagonal, commutative general | 1 / 1 |
//! | `HeunStratonovich` | Stratonovich | diagonal, commutative general | 1 / 1 |
//! | `SrkPlaten` | Itô | diagonal, commutative general | 1 / 1 |
//! | `MilsteinNoncommutative` | Itô | general only (Lévy areas) | 1 / 1 |
//!
//! The calculus is part of the method name, not an orthogonal option: nearly
//! every Itô/Stratonovich mix-up in the wild is a scheme whose name does not say
//! which integral it converges to.
//!
//! Step policy is fixed (`dt`/`steps`, or an explicit **uniform** grid) or
//! adaptive (`rtol`/`atol`, Brownian-bridge rejection in the kernel). Adaptive
//! integration exists for the Itô commutative (`EulerMaruyama`/`Milstein`) and
//! the non-commutative Milstein schemes; a Stratonovich adaptive combination is
//! [`SdeError::Unsupported`] with a capability name, not a silent fallback. The
//! kernel's adaptive controller returns the *Milstein* solution (its estimate is
//! EM-vs-Milstein), which is stated in [`integrate`]'s documentation rather than
//! hidden: asking for adaptive `EulerMaruyama` gets that same controlled path.
//!
//! # Determinism and the two RNG streams
//!
//! * Fixed-step paths on the **language stream** ([`wiener`], [`Stream`]): the
//!   stream is the interpreter's `rng(seed)` LCG (state → `seed * 6364136223846793005 + 1`,
//!   `(state >> 11) / 2^53`), with standard normals by Box–Muller (two uniforms
//!   per normal, cosine branch). [`wiener`] and a fixed-step diagonal/general path
//!   therefore consume *the same numbers* — a test asserts the path equals a
//!   hand-driven kernel EM run on `wiener`'s increments. Same build ⇒
//!   bit-identical; cross-platform bit identity is not promised.
//! * Fixed-step non-commutative Milstein and every adaptive path need the
//!   kernel's internal xoshiro stream (`rssn::kernels::random::Rng`) because the
//!   kernel samples Brownian bridges and Lévy areas as it goes. They are
//!   seeded from [`Options::seed`] and deterministic for a given build; they do
//!   not share the language LCG stream. This is a documented two-stream
//!   reality: the language stream is the one `wiener`/`rng` expose, the kernel
//!   stream is what bridge splitting consumes.
//!
//! # Limitations (deliberate, not silent)
//!
//! * Fixed-step kernels assume a uniform step; an explicit non-uniform grid is
//!   rejected with `E0802` (use the adaptive schedule for variable steps).
//! * A `General { commutative: true }` structure is a **claim by the caller**:
//!   the Milstein/SRK/Heun corrections for general noise are only valid for
//!   commutative noise. Passing `commutative: false` to them is `E1001`
//!   (`sde.noncommutative`); use `MilsteinNoncommutative` there.
//! * Lévy areas are simulated with a finite Fourier expansion
//!   ([`Options::terms`]); accuracy improves with the mode count, and the
//!   non-commutative scheme is strong order one at best.
//! * No events/root-finding and no dense output in this round.
//! * Ensembles run paths sequentially: an Aevia interpreter closure is
//!   single-threaded (`!Send`), so the kernel's rayon ensemble cannot be used
//!   without an `unsafe` callback. The per-path seed is derived from the pair
//!   `(seed, index)` only, so the result is independent of execution order.
//! * Record/tensor states are handled by the caller: this module sees the flat
//!   slice produced by the same flatten/rebuild contract as [`crate::ode`].
//!
//! The statistical order/convergence tests live in the kernel's own suite; the
//! tests here are deterministic (pinned seeds, path equality, error codes).

use crate::NumericError;
use rssn::kernels::random::Rng;
use rssn::kernels::sde as kernel;
use std::cell::{Cell, RefCell};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why an SDE integration refused or failed.
///
/// The code is the stable Aevia diagnostic (appendix B): `E0802` for a shape or
/// parameter error, `E0806` for a value outside the domain (a non-finite state),
/// `E0807` for a step collapse, and `E1001` for a method/structure/step-policy
/// combination this build does not implement — naming the capability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SdeError {
    /// Inconsistent dimensions or parameters (`E0802`).
    Invalid(String),
    /// A shape (`E0802`); kept separate from [`Self::Invalid`] for messages.
    Shape(String),
    /// The state became non-finite (`E0806`), with the kernel's reason.
    NonFinite(String),
    /// The adaptive step collapsed or exhausted its budget (`E0807`).
    StepLimit(String),
    /// An unsupported combination, naming the capability (`E1001`).
    Unsupported {
        /// The capability a build would need (`core.sde`, `sde.noncommutative`,
        /// `sde.interpretation`, `sde.events`, …).
        capability: &'static str,
        /// What exactly is not implemented, for the note.
        detail: String,
    },
}

impl SdeError {
    /// The stable Aevia diagnostic code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) | Self::Shape(_) => "E0802",
            Self::NonFinite(_) => "E0806",
            Self::StepLimit(_) => "E0807",
            Self::Unsupported { .. } => "E1001",
        }
    }

    /// The capability an `E1001` names, when it is one.
    #[must_use]
    pub fn capability(&self) -> Option<&'static str> {
        match self {
            Self::Unsupported { capability, .. } => Some(capability),
            Self::Invalid(_) | Self::Shape(_) | Self::NonFinite(_) | Self::StepLimit(_) => None,
        }
    }

    fn shape(message: impl Into<String>) -> Self {
        Self::Shape(message.into())
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    fn unsupported(capability: &'static str, detail: impl Into<String>) -> Self {
        Self::Unsupported {
            capability,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for SdeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) | Self::Shape(message) => f.write_str(message),
            Self::NonFinite(reason) => write!(f, "the state became non-finite: {reason}"),
            Self::StepLimit(reason) => f.write_str(reason),
            Self::Unsupported { capability, detail } => {
                write!(f, "this build does not provide `{capability}`: {detail}")
            }
        }
    }
}

impl std::error::Error for SdeError {}

impl From<kernel::SdeError> for SdeError {
    fn from(error: kernel::SdeError) -> Self {
        match error {
            kernel::SdeError::Invalid { what } => {
                Self::invalid(format!("the kernel rejected the SDE parameters: {what}"))
            }
            kernel::SdeError::NonFinite { step } => {
                Self::NonFinite(format!("the state left the reals at step {step}"))
            }
            kernel::SdeError::StepLimit { limit } => {
                Self::StepLimit(format!("the step budget of {limit} was exhausted"))
            }
            kernel::SdeError::StepUnderflow { t, h } => Self::StepLimit(format!(
                "the step size underflowed at t = {t} (h = {h:e})"
            )),
            kernel::SdeError::StepBelowCell { cells } => Self::StepLimit(format!(
                "an adaptive step cannot be shorter than one of the {cells} Brownian cells; \
                 use more cells"
            )),
        }
    }
}

impl From<NumericError> for SdeError {
    fn from(error: NumericError) -> Self {
        match error {
            NumericError::NotConverged => Self::StepLimit("the iteration did not converge".into()),
            NumericError::Domain(message) => Self::invalid(message),
            other => Self::invalid(other.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Methods, structures, options
// ---------------------------------------------------------------------------

/// A stochastic integration scheme. The calculus is part of the name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// Euler–Maruyama (Itô), strong order 1/2.
    EulerMaruyama,
    /// Milstein (Itô), strong order 1 for commutative noise.
    Milstein,
    /// Milstein (Stratonovich), strong order 1 for commutative noise.
    MilsteinStratonovich,
    /// Heun predictor–corrector (Stratonovich), strong order 1 for commutative
    /// noise.
    HeunStratonovich,
    /// SRK–Platen (Itô), strong order 1 for commutative noise.
    SrkPlaten,
    /// Milstein (Itô) for non-commutative noise, using simulated Lévy areas.
    MilsteinNoncommutative,
}

/// Which stochastic integral a scheme converges to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Calculus {
    /// The Itô integral.
    Ito,
    /// The Stratonovich integral.
    Stratonovich,
}

impl Method {
    /// The spelling used by the language surface and diagnostics.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::EulerMaruyama => "euler_maruyama",
            Self::Milstein => "milstein",
            Self::MilsteinStratonovich => "milstein_stratonovich",
            Self::HeunStratonovich => "heun_stratonovich",
            Self::SrkPlaten => "srk_platen",
            Self::MilsteinNoncommutative => "milstein_noncommutative",
        }
    }

    /// The method named `name`, if it is one this build knows.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "euler_maruyama" | "em" => Self::EulerMaruyama,
            "milstein" => Self::Milstein,
            "milstein_stratonovich" => Self::MilsteinStratonovich,
            "heun_stratonovich" | "heun" => Self::HeunStratonovich,
            "srk_platen" | "srk" => Self::SrkPlaten,
            "milstein_noncommutative" => Self::MilsteinNoncommutative,
            _ => return None,
        })
    }

    /// The integral the scheme converges to.
    #[must_use]
    pub fn calculus(self) -> Calculus {
        match self {
            Self::EulerMaruyama
            | Self::Milstein
            | Self::SrkPlaten
            | Self::MilsteinNoncommutative => Calculus::Ito,
            Self::MilsteinStratonovich | Self::HeunStratonovich => Calculus::Stratonovich,
        }
    }
}

/// The order of a scheme, as documented by the kernel.
///
/// The kernel states that *no* scheme here is better than strong order one and
/// that its Milstein variants are weak order one (not two), so weak order two
/// is never claimed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Order {
    /// Strong (pathwise) order.
    pub strong: f64,
    /// Weak (moment) order.
    pub weak: f64,
}

/// The strong/weak order of `method`, as the kernel's documentation states it.
#[must_use]
pub fn sde_order(method: Method) -> Order {
    match method {
        // Euler–Maruyama is strong 1/2, weak 1.
        Method::EulerMaruyama => Order {
            strong: 0.5,
            weak: 1.0,
        },
        // Every other implemented scheme is strong order one. The kernel is
        // explicit that they are weak order one too (not two).
        Method::Milstein
        | Method::MilsteinStratonovich
        | Method::HeunStratonovich
        | Method::SrkPlaten
        | Method::MilsteinNoncommutative => Order {
            strong: 1.0,
            weak: 1.0,
        },
    }
}

/// How the diffusion enters the equation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Structure {
    /// One noise channel per state component: `g` is `n × n` and diagonal, and
    /// the diffusion callback returns its `n` diagonal entries.
    Diagonal,
    /// `m` noise channels; the diffusion callback returns the `n × m` matrix in
    /// row-major order. `commutative` is the caller's claim: the derivative
    /// corrections used by Milstein/SRK/Heun are only valid for commutative
    /// noise.
    General {
        /// Number of independent Wiener processes (`m`).
        noise: usize,
        /// Whether the caller guarantees the noise is commutative.
        commutative: bool,
    },
}

impl Structure {
    /// The number of noise channels (`m`) for a state of `n` components.
    #[must_use]
    pub fn noise(self, state: usize) -> usize {
        match self {
            Self::Diagonal => state,
            Self::General { noise, .. } => noise,
        }
    }

    /// Whether the structure is (declared) commutative.
    #[must_use]
    pub fn is_commutative(self) -> bool {
        match self {
            Self::Diagonal => true,
            Self::General { commutative, .. } => commutative,
        }
    }

    /// How many values the diffusion callback returns.
    #[must_use]
    pub fn coefficients(self, state: usize) -> usize {
        match self {
            Self::Diagonal => state,
            Self::General { noise, .. } => state * noise,
        }
    }
}

/// The step policy.
#[derive(Clone, Debug, PartialEq)]
pub enum Schedule {
    /// Fixed steps of `dt`, `steps` of them: `t_k = t0 + k dt`.
    Fixed {
        /// Step size.
        dt: f64,
        /// Number of steps.
        steps: usize,
    },
    /// An explicit, **uniform** time grid (the first entry is `t0`).
    Grid {
        /// The times, strictly increasing with equal spacing.
        times: Vec<f64>,
    },
    /// Adaptive steps on `[t0, t1]`.
    Adaptive {
        /// End time.
        t1: f64,
        /// Relative tolerance of the local error.
        rtol: f64,
        /// Absolute tolerance of the local error.
        atol: f64,
        /// Step budget; exhaustion is `E0807`.
        max_steps: usize,
        /// Brownian cells for the non-commutative adaptive solver on `[t0,t1]`.
        /// Ignored by the commutative solver; must be positive when the method
        /// is `MilsteinNoncommutative`.
        cells: usize,
    },
}

/// Options of one integration.
///
/// There is no `Default`: a seed and a step are not guessable, so they are
/// constructor arguments (the spec requires an explicit seed for reproducibility
/// and an explicit step policy). [`Options::with_terms`] is required for
/// [`Method::MilsteinNoncommutative`].
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The scheme.
    pub method: Method,
    /// The noise structure.
    pub structure: Structure,
    /// The seed of the Brownian path (and of the language stream for fixed
    /// steps).
    pub seed: u64,
    /// Initial time.
    pub t0: f64,
    /// The step policy.
    pub schedule: Schedule,
    /// Fourier modes of the simulated Lévy areas; `0` until
    /// [`Options::with_terms`] sets it, and required (> 0) for the
    /// non-commutative method.
    pub terms: usize,
}

impl Options {
    /// `steps` fixed steps of `dt` from `t0`.
    #[must_use]
    pub fn fixed(
        method: Method,
        structure: Structure,
        seed: u64,
        t0: f64,
        dt: f64,
        steps: usize,
    ) -> Self {
        Self {
            method,
            structure,
            seed,
            t0,
            schedule: Schedule::Fixed { dt, steps },
            terms: 0,
        }
    }

    /// Fixed steps covering `[t0, t1]`: the step count is rounded, and `dt` is
    /// adjusted to `(t1 - t0) / steps` so the end time is reached exactly.
    #[must_use]
    pub fn fixed_span(
        method: Method,
        structure: Structure,
        seed: u64,
        t0: f64,
        t1: f64,
        dt: f64,
    ) -> Self {
        let steps = if dt > 0.0 && t1 > t0 {
            ((t1 - t0) / dt).round().max(1.0) as usize
        } else {
            1
        };
        let adjusted = (t1 - t0) / steps as f64;
        Self::fixed(method, structure, seed, t0, adjusted, steps)
    }

    /// An explicit uniform grid.
    #[must_use]
    pub fn grid(method: Method, structure: Structure, seed: u64, times: Vec<f64>) -> Self {
        let t0 = times.first().copied().unwrap_or(0.0);
        Self {
            method,
            structure,
            seed,
            t0,
            schedule: Schedule::Grid { times },
            terms: 0,
        }
    }

    /// Adaptive integration on `[t0, t1]` (`cells` is used only by the
    /// non-commutative method; pass `0` otherwise).
    #[must_use]
    #[allow(clippy::too_many_arguments)] // the option record is the language's, not a builder
    pub fn adaptive(
        method: Method,
        structure: Structure,
        seed: u64,
        t0: f64,
        t1: f64,
        rtol: f64,
        atol: f64,
        max_steps: usize,
        cells: usize,
    ) -> Self {
        Self {
            method,
            structure,
            seed,
            t0,
            schedule: Schedule::Adaptive {
                t1,
                rtol,
                atol,
                max_steps,
                cells,
            },
            terms: 0,
        }
    }

    /// Sets the number of Fourier modes for the simulated Lévy areas.
    #[must_use]
    pub fn with_terms(mut self, terms: usize) -> Self {
        self.terms = terms;
        self
    }
}

// ---------------------------------------------------------------------------
// The language RNG stream
// ---------------------------------------------------------------------------

/// The interpreter's `rng(seed)` stream, re-implemented here so the standard
/// library can produce the same numbers the language does.
///
/// The formula is the interpreter's ([`aevia_core::Interp::next_random`]): a
/// 64-bit LCG with multiplier `6364136223846793005` and increment `1`, taking the
/// top 53 bits. [`Stream::new`] mirrors the interpreter's default seed only if
/// given the same value; a language program's `rng(seed)` and
/// `sdeint(..., seed)` share the sequence when they share the seed.
///
/// Standard normals are Box–Muller, cosine branch, two uniforms per normal. A
/// zero uniform is replaced by the smallest positive double so `ln` stays
/// finite; the probability of that is below `2^-53`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stream {
    state: u64,
}

impl Stream {
    /// A stream seeded with `seed` (the same state `rng(seed)` starts from).
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The next uniform in `[0, 1)`.
    pub fn next_unit(&mut self) -> f64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        ((self.state >> 11) as f64) / 9_007_199_254_740_992.0
    }

    /// The next standard normal, Box–Muller (cosine branch).
    pub fn next_normal(&mut self) -> f64 {
        let u1 = self.next_unit().max(f64::MIN_POSITIVE);
        let u2 = self.next_unit();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

/// Brownian increments for `steps` steps of `dt`, `m` components each, on the
/// language stream seeded with `seed`.
///
/// The argument order is the kernel's `sde_wiener(seed, m, steps, h)` so the
/// language surface, the manifest operator and this helper cannot drift:
/// `wiener(seed, m, steps, dt)[k][j]` is the increment of the `j`-th Wiener
/// process over `[t0 + k dt, t0 + (k+1) dt]`, drawn step-major (component order
/// inside a step), which is exactly the layout the kernel's fixed-step solvers
/// consume. A fixed-step integration with the same seed and structure therefore
/// consumes these numbers.
#[must_use]
pub fn wiener(seed: u64, m: usize, steps: usize, dt: f64) -> Vec<Vec<f64>> {
    let mut stream = Stream::new(seed);
    wiener_from(&mut stream, m, dt, steps)
}

fn wiener_from(stream: &mut Stream, m: usize, dt: f64, steps: usize) -> Vec<Vec<f64>> {
    let scale = dt.abs().sqrt();
    (0..steps)
        .map(|_| {
            (0..m)
                .map(|_| scale * stream.next_normal())
                .collect::<Vec<f64>>()
        })
        .collect()
}

/// Seed of path `index` of an ensemble with base `seed`.
///
/// A function of the pair only (the kernel's derivation), so path `i` is the
/// same whichever order or concurrency runs the ensemble; this build runs the
/// paths sequentially because an interpreter closure is `!Send`.
#[must_use]
pub fn ensemble_seed(seed: u64, index: usize) -> u64 {
    kernel::ensemble_seed(seed, index)
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

/// One integrated path.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    /// Accepted time points (`t` and `y` are parallel).
    pub t: Vec<f64>,
    /// States at the accepted times, in time order.
    pub y: Vec<Vec<f64>>,
    /// Accepted steps.
    pub steps: usize,
    /// Steps rejected by the adaptive controller (fixed-step paths: 0).
    pub rejected: usize,
    /// Number of drift + diffusion callbacks made.
    pub nfev: usize,
    /// The seed actually used.
    pub seed: u64,
    /// The number of noise channels (`m`).
    pub noise: usize,
    /// The method actually used (adaptive `EulerMaruyama` runs the kernel's
    /// controlled Milstein solution, but keeps the requested name).
    pub method: Method,
}

impl Path {
    /// The final state, when the path has one.
    #[must_use]
    pub fn last(&self) -> Option<&Vec<f64>> {
        self.y.last()
    }
}

// ---------------------------------------------------------------------------
// Callback adapters
// ---------------------------------------------------------------------------

/// Adapts the Aevia-side `FnMut` callbacks to the kernel's `Fn` callbacks.
///
/// The kernel takes `Fn` (it may call a closure inside a rayon ensemble, where
/// the closure must be `Sync`); an Aevia interpreter closure is `!Send` and
/// cannot cross. This adapter owns the closures in a `RefCell`, writes into the
/// kernel's output slice, and records the first failure — because the kernel's
/// callbacks return `()`, an error has to travel through the adapter, not the
/// return value. No `unsafe`: the interior mutability is the whole mechanism.
struct Adapters<A, G> {
    drift: RefCell<A>,
    diffusion: RefCell<G>,
    drift_error: RefCell<Option<SdeError>>,
    diffusion_error: RefCell<Option<SdeError>>,
    state: usize,
    coefficients: usize,
    diagonal: bool,
    calls: Cell<usize>,
}

impl<A, G> Adapters<A, G>
where
    A: FnMut(f64, &[f64]) -> Result<Vec<f64>, SdeError>,
    G: FnMut(f64, &[f64]) -> Result<Vec<f64>, SdeError>,
{
    fn new(drift: A, diffusion: G, state: usize, structure: Structure) -> Self {
        Self {
            drift: RefCell::new(drift),
            diffusion: RefCell::new(diffusion),
            drift_error: RefCell::new(None),
            diffusion_error: RefCell::new(None),
            state,
            coefficients: structure.coefficients(state),
            diagonal: structure == Structure::Diagonal,
            calls: Cell::new(0),
        }
    }

    /// Writes the drift into the kernel's output slice.
    fn drift_at(&self, t: f64, y: &[f64], out: &mut [f64]) {
        if self.drift_error.borrow().is_some() {
            out.fill(0.0);
            return;
        }
        self.calls.set(self.calls.get() + 1);
        let result = {
            let mut drift = self.drift.borrow_mut();
            (*drift)(t, y)
        };
        match result {
            Ok(values) if values.len() == self.state => out.copy_from_slice(&values),
            Ok(values) => {
                *self.drift_error.borrow_mut() = Some(SdeError::shape(format!(
                    "the drift returned {} value(s) for a state of {}",
                    values.len(),
                    self.state
                )));
                out.fill(0.0);
            }
            Err(error) => {
                *self.drift_error.borrow_mut() = Some(error);
                out.fill(0.0);
            }
        }
    }

    /// Writes the diffusion into the kernel's `n × m` output slice.
    fn diffusion_at(&self, t: f64, y: &[f64], out: &mut [f64]) {
        if self.diffusion_error.borrow().is_some() {
            out.fill(0.0);
            return;
        }
        self.calls.set(self.calls.get() + 1);
        let result = {
            let mut diffusion = self.diffusion.borrow_mut();
            (*diffusion)(t, y)
        };
        match result {
            Ok(values) if values.len() == self.coefficients => {
                if self.diagonal {
                    // Expand the diagonal to the kernel's dense n×n layout.
                    out.fill(0.0);
                    for index in 0..self.state {
                        out[index * self.state + index] = values[index];
                    }
                } else {
                    out.copy_from_slice(&values);
                }
            }
            Ok(values) => {
                *self.diffusion_error.borrow_mut() = Some(SdeError::shape(format!(
                    "the diffusion returned {} coefficient(s); this structure needs {}",
                    values.len(),
                    self.coefficients
                )));
                out.fill(0.0);
            }
            Err(error) => {
                *self.diffusion_error.borrow_mut() = Some(error);
                out.fill(0.0);
            }
        }
    }

    /// The first callback failure, if any.
    fn failure(&self) -> Option<SdeError> {
        self.drift_error
            .borrow_mut()
            .take()
            .or_else(|| self.diffusion_error.borrow_mut().take())
    }

    fn nfev(&self) -> usize {
        self.calls.get()
    }
}

// ---------------------------------------------------------------------------
// Integration
// ---------------------------------------------------------------------------

/// Integrates `dX = a(t, X) dt + G(t, X) dW` from `y0`.
///
/// `drift` returns `n` values; `diffusion` returns `n` values for
/// [`Structure::Diagonal`] (the diagonal entries) or `n × m` values in
/// row-major order for [`Structure::General`].
///
/// The step policy comes from [`Options::schedule`]. Fixed steps consume
/// [`wiener`]'s language-stream increments; the non-commutative method and every
/// adaptive path consume the kernel's internal stream (seeded from
/// [`Options::seed`]); see the module docs.
///
/// The adaptive controller integrates the Itô commutative pair with the kernel's
/// Milstein-controlled step size: the returned states are the kernel's Milstein
/// solution for both `EulerMaruyama` and `Milstein` requests. This is the
/// kernel's documented contract, not a hidden substitution of the requested
/// scheme's control law.
///
/// # Errors
///
/// `E0802` for shapes/parameters, `E0806` for a non-finite state, `E0807` for a
/// step collapse, `E1001` (with a capability) for an unsupported combination.
pub fn integrate<A, G>(
    drift: A,
    diffusion: G,
    y0: &[f64],
    options: &Options,
) -> Result<Path, SdeError>
where
    A: FnMut(f64, &[f64]) -> Result<Vec<f64>, SdeError>,
    G: FnMut(f64, &[f64]) -> Result<Vec<f64>, SdeError>,
{
    validate_options(options, y0)?;
    let state = y0.len();
    let noise = options.structure.noise(state);
    let adapters = Adapters::new(drift, diffusion, state, options.structure);
    let run = match &options.schedule {
        Schedule::Fixed { dt, steps } => {
            fixed(&adapters, y0, options, *dt, *steps, noise)
        }
        Schedule::Grid { times } => {
            let (dt, steps) = uniform_grid(times)?;
            if (times[0] - options.t0).abs() > 1e-12 * options.t0.abs().max(1.0) {
                return Err(SdeError::invalid(
                    "the explicit grid does not start at the initial time",
                ));
            }
            fixed(&adapters, y0, options, dt, steps, noise)
        }
        Schedule::Adaptive {
            t1,
            rtol,
            atol,
            max_steps,
            cells,
        } => adaptive(&adapters, y0, options, *t1, *rtol, *atol, *max_steps, *cells),
    };
    // A callback failure is recorded inside the adapter (the kernel callbacks
    // return `()`); it takes precedence over the kernel's own error code because
    // it is the cause, not the symptom.
    if let Some(failure) = adapters.failure() {
        return Err(failure);
    }
    let mut path = run?;
    path.nfev = adapters.nfev();
    path.seed = options.seed;
    path.noise = noise;
    path.method = options.method;
    Ok(path)
}

/// Integrates one path per ensemble member, sequentially and order-preserving.
///
/// Member `i` uses [`ensemble_seed`]`(seed, i)` as its own seed, so the result
/// does not depend on execution order or concurrency. The paths run one after
/// another because the Aevia interpreter closures are `!Send`; the kernel's
/// rayon ensemble would need a `Send + Sync` callback, which this build cannot
/// produce. The returned paths are in index order.
///
/// # Errors
///
/// The first member error, in index order.
pub fn ensemble<A, G>(
    mut drift: A,
    mut diffusion: G,
    y0: &[f64],
    options: &Options,
    paths: usize,
) -> Result<Vec<Path>, SdeError>
where
    A: FnMut(f64, &[f64]) -> Result<Vec<f64>, SdeError>,
    G: FnMut(f64, &[f64]) -> Result<Vec<f64>, SdeError>,
{
    if paths == 0 {
        return Err(SdeError::invalid("an ensemble needs at least one path"));
    }
    let mut out = Vec::with_capacity(paths);
    for index in 0..paths {
        let mut member = options.clone();
        member.seed = ensemble_seed(options.seed, index);
        out.push(integrate(&mut drift, &mut diffusion, y0, &member)?);
    }
    Ok(out)
}

/// Checks the options against a state length before any work is done.
fn validate_options(options: &Options, y0: &[f64]) -> Result<(), SdeError> {
    if y0.is_empty() {
        return Err(SdeError::shape("the initial state is empty"));
    }
    if !y0.iter().all(|value| value.is_finite()) {
        return Err(SdeError::NonFinite(
            "the initial state is not finite".to_string(),
        ));
    }
    if !options.t0.is_finite() {
        return Err(SdeError::invalid("the initial time is not finite"));
    }
    if options.structure.noise(y0.len()) == 0 {
        return Err(SdeError::invalid("the noise dimension is zero"));
    }
    if options.method == Method::MilsteinNoncommutative && options.terms == 0 {
        return Err(SdeError::invalid(
            "`milstein_noncommutative` needs `terms > 0` for the simulated Lévy areas",
        ));
    }
    match &options.schedule {
        Schedule::Fixed { dt, steps } => {
            if !dt.is_finite() || *dt <= 0.0 {
                return Err(SdeError::invalid("the step size must be finite and positive"));
            }
            if *steps == 0 {
                return Err(SdeError::invalid("the step count must be positive"));
            }
        }
        Schedule::Grid { times } => {
            uniform_grid(times)?;
        }
        Schedule::Adaptive {
            t1,
            rtol,
            atol,
            max_steps,
            cells,
        } => {
            if !t1.is_finite() || *t1 <= options.t0 {
                return Err(SdeError::invalid("the end time must be after the initial time"));
            }
            if !rtol.is_finite() || *rtol <= 0.0 {
                return Err(SdeError::invalid("`rtol` must be finite and positive"));
            }
            if !atol.is_finite() || *atol < 0.0 {
                return Err(SdeError::invalid("`atol` must be finite and non-negative"));
            }
            if *max_steps == 0 {
                return Err(SdeError::invalid("the step budget must be positive"));
            }
            if options.method == Method::MilsteinNoncommutative && *cells == 0 {
                return Err(SdeError::invalid(
                    "the non-commutative adaptive method needs `cells > 0`",
                ));
            }
        }
    }
    check_combination(options)
}

/// Rejects method/structure/step-policy combinations this build cannot honour.
fn check_combination(options: &Options) -> Result<(), SdeError> {
    let adaptive = matches!(options.schedule, Schedule::Adaptive { .. });
    let commutative = options.structure.is_commutative();
    if options.method == Method::MilsteinNoncommutative && commutative {
        return Err(SdeError::unsupported(
            "sde.noncommutative",
            "`milstein_noncommutative` is for non-commutative noise; declare \
             `General { commutative = false }`",
        ));
    }
    if !commutative
        && matches!(
            options.method,
            Method::Milstein
                | Method::MilsteinStratonovich
                | Method::HeunStratonovich
                | Method::SrkPlaten
        )
    {
        return Err(SdeError::unsupported(
            "sde.noncommutative",
            format!(
                "`{}` uses derivative corrections valid only for commutative noise",
                options.method.name()
            ),
        ));
    }
    if !adaptive {
        return Ok(());
    }
    match options.method {
        Method::EulerMaruyama | Method::Milstein => Ok(()),
        Method::MilsteinNoncommutative => Ok(()),
        Method::MilsteinStratonovich | Method::HeunStratonovich | Method::SrkPlaten => {
            Err(SdeError::unsupported(
                "sde.interpretation",
                format!(
                    "this build has no adaptive controller for `{}`; use a fixed step",
                    options.method.name()
                ),
            ))
        }
    }
}

/// The step size and count of a uniform explicit grid.
fn uniform_grid(times: &[f64]) -> Result<(f64, usize), SdeError> {
    if times.len() < 2 {
        return Err(SdeError::invalid("the time grid needs at least two points"));
    }
    if !times.iter().all(|time| time.is_finite()) {
        return Err(SdeError::invalid("the time grid contains a non-finite time"));
    }
    let dt = times[1] - times[0];
    if dt <= 0.0 {
        return Err(SdeError::invalid("the time grid must be strictly increasing"));
    }
    let tolerance = 1e-9 * dt.abs().max(1.0);
    for window in times.windows(2) {
        if (window[1] - window[0] - dt).abs() > tolerance {
            return Err(SdeError::invalid(
                "a fixed-step path needs a uniform grid; use the adaptive \
                 schedule for variable steps",
            ));
        }
    }
    Ok((dt, times.len() - 1))
}

/// A fixed-step integration.
fn fixed<A, G>(
    adapters: &Adapters<A, G>,
    y0: &[f64],
    options: &Options,
    dt: f64,
    steps: usize,
    noise: usize,
) -> Result<Path, SdeError>
where
    A: FnMut(f64, &[f64]) -> Result<Vec<f64>, SdeError>,
    G: FnMut(f64, &[f64]) -> Result<Vec<f64>, SdeError>,
{
    let t0 = options.t0;
    let drift = |t: f64, y: &[f64], out: &mut [f64]| adapters.drift_at(t, y, out);
    let diffusion = |t: f64, y: &[f64], out: &mut [f64]| adapters.diffusion_at(t, y, out);
    let states = if options.method == Method::MilsteinNoncommutative {
        // Lévy areas are sampled by the kernel's stream (it needs the same Rng
        // that draws the bridge/fourier coefficients).
        let mut rng = Rng::new(options.seed);
        let sample = kernel::sample_noise(&mut rng, noise, steps, dt, options.terms.max(1));
        kernel::milstein_noncommutative(drift, diffusion, noise, t0, dt, y0, &sample)
            .map_err(SdeError::from)?
    } else {
        let increments = wiener(options.seed, noise, steps, dt);
        match options.method {
            Method::EulerMaruyama => {
                kernel::euler_maruyama(drift, diffusion, noise, t0, dt, y0, &increments)
            }
            Method::Milstein => {
                kernel::milstein(drift, diffusion, noise, t0, dt, y0, &increments)
            }
            Method::MilsteinStratonovich => kernel::milstein_stratonovich(
                drift,
                diffusion,
                noise,
                t0,
                dt,
                y0,
                &increments,
            ),
            Method::HeunStratonovich => {
                kernel::heun_stratonovich(drift, diffusion, noise, t0, dt, y0, &increments)
            }
            Method::SrkPlaten => {
                kernel::srk_platen(drift, diffusion, noise, t0, dt, y0, &increments)
            }
            // Handled above; kept total for the match.
            Method::MilsteinNoncommutative => unreachable!("non-commutative path"),
        }
        .map_err(SdeError::from)?
    };
    if let Some(failure) = adapters.failure() {
        return Err(failure);
    }
    let t: Vec<f64> = (0..states.len()).map(|step| t0 + step as f64 * dt).collect();
    Ok(Path {
        t,
        steps: states.len().saturating_sub(1),
        y: states,
        rejected: 0,
        nfev: adapters.nfev(),
        seed: options.seed,
        noise,
        method: options.method,
    })
}

/// An adaptive-step integration.
#[allow(clippy::too_many_arguments)] // mirrors the kernel's adaptive entry points
fn adaptive<A, G>(
    adapters: &Adapters<A, G>,
    y0: &[f64],
    options: &Options,
    t1: f64,
    rtol: f64,
    atol: f64,
    max_steps: usize,
    cells: usize,
) -> Result<Path, SdeError>
where
    A: FnMut(f64, &[f64]) -> Result<Vec<f64>, SdeError>,
    G: FnMut(f64, &[f64]) -> Result<Vec<f64>, SdeError>,
{
    let noise = options.structure.noise(y0.len());
    let drift = |t: f64, y: &[f64], out: &mut [f64]| adapters.drift_at(t, y, out);
    let diffusion = |t: f64, y: &[f64], out: &mut [f64]| adapters.diffusion_at(t, y, out);
    let path = if options.method == Method::MilsteinNoncommutative {
        let cell_count = cells.max(1);
        kernel::sde_adaptive_noncommutative_with(
            drift,
            diffusion,
            noise,
            options.t0,
            t1,
            y0,
            &kernel::SdeNoncommutativeOptions {
                base: kernel::SdeAdaptiveOptions {
                    rtol,
                    atol,
                    seed: options.seed,
                    max_steps,
                },
                cells: cell_count,
                terms: options.terms.max(1),
            },
        )
    } else {
        kernel::sde_adaptive_with(
            drift,
            diffusion,
            noise,
            options.t0,
            t1,
            y0,
            &kernel::SdeAdaptiveOptions {
                rtol,
                atol,
                seed: options.seed,
                max_steps,
            },
        )
    }
    .map_err(SdeError::from)?;
    if let Some(failure) = adapters.failure() {
        return Err(failure);
    }
    Ok(Path {
        steps: path.y.len().saturating_sub(1),
        t: path.t,
        y: path.y,
        rejected: path.rejected,
        nfev: adapters.nfev(),
        seed: options.seed,
        noise,
        method: options.method,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default seed of `Interp`'s language RNG.
    const INTERP_SEED: u64 = 0x2545_f491_4f6c_dd1d;

    /// A boxed drift or diffusion callback, so a test can name the pair.
    type Coeff = Box<dyn FnMut(f64, &[f64]) -> Result<Vec<f64>, SdeError>>;

    /// Geometric Brownian motion: `dX = mu X dt + sigma X dW` (scalar).
    fn gbm(mu: f64, sigma: f64) -> (Coeff, Coeff) {
        (
            Box::new(move |_t: f64, y: &[f64]| Ok(vec![mu * y[0]])),
            Box::new(move |_t: f64, y: &[f64]| Ok(vec![sigma * y[0]])),
        )
    }

    /// Ornstein–Uhlenbeck: `dX = theta (mu - X) dt + sigma dW`.
    fn ou(theta: f64, mu: f64, sigma: f64) -> (Coeff, Coeff) {
        (
            Box::new(move |_t: f64, y: &[f64]| Ok(vec![theta * (mu - y[0])])),
            Box::new(move |_t: f64, _y: &[f64]| Ok(vec![sigma])),
        )
    }

    #[test]
    fn the_stream_is_the_interpreters_rng() {
        // The interpreter's default seed is not public; construct a fresh
        // interpreter and compare its first outputs with the documented mirror.
        let compilation = aevia_core::compile("seed.aev", "let x = 1\n");
        let mut interp = aevia_core::Interp::new(&compilation.checked);
        let mut stream = Stream::new(INTERP_SEED);
        for _ in 0..8 {
            assert_eq!(
                interp.next_random().to_bits(),
                stream.next_unit().to_bits(),
                "the language stream must be the interpreter's LCG"
            );
        }
    }

    #[test]
    fn wiener_is_reproducible_and_has_the_documented_shape() {
        let first = wiener(7, 3, 5, 0.01);
        let second = wiener(7, 3, 5, 0.01);
        assert_eq!(first, second);
        assert_eq!(first.len(), 5);
        assert!(first.iter().all(|increment| increment.len() == 3));
        // A different seed gives a different path.
        assert_ne!(first, wiener(8, 3, 5, 0.01));
        // The scale is sqrt(dt): the mean square of a scalar increment is dt.
        let increments = wiener(9, 1, 10_000, 0.01);
        let mean_square: f64 = increments
            .iter()
            .map(|increment| increment[0] * increment[0])
            .sum::<f64>()
            / 10_000.0;
        assert!(
            (mean_square - 0.01).abs() < 0.004,
            "mean square {mean_square} should be close to dt = 0.01"
        );
    }

    /// A fixed-step path consumes exactly the increments `wiener` publishes: the
    /// EM path equals a hand-driven kernel EM run on those increments.
    #[test]
    fn a_fixed_path_consumes_the_published_increments() {
        let (drift, diffusion) = gbm(0.1, 0.4);
        let options = Options::fixed(Method::EulerMaruyama, Structure::Diagonal, 11, 0.0, 0.01, 20);
        let path = integrate(drift, diffusion, &[1.0], &options).expect("a path");
        let increments = wiener(11, 1, 20, 0.01);
        let expected = kernel::euler_maruyama(
            |_t: f64, y: &[f64], out: &mut [f64]| out[0] = 0.1 * y[0],
            |_t: f64, y: &[f64], out: &mut [f64]| out[0] = 0.4 * y[0],
            1,
            0.0,
            0.01,
            &[1.0],
            &increments,
        )
        .expect("the hand-driven reference");
        assert_eq!(path.y, expected, "the path is not the published wiener");
        assert_eq!(path.steps, 20);
        assert_eq!(path.t.len(), 21);
        assert_eq!(path.rejected, 0);
        assert_eq!(path.seed, 11);
        assert_eq!(path.noise, 1);
        assert!(path.nfev > 0);
    }

    #[test]
    fn a_fixed_path_is_bit_reproducible() {
        let run = || {
            let (drift, diffusion) = gbm(0.05, 0.2);
            let options =
                Options::fixed(Method::Milstein, Structure::Diagonal, 424, 0.0, 0.005, 40);
            integrate(drift, diffusion, &[2.0], &options).expect("a path")
        };
        let first = run();
        let second = run();
        assert_eq!(first, second);
    }

    #[test]
    fn every_fixed_method_runs_and_orders_are_reported() {
        for method in [
            Method::EulerMaruyama,
            Method::Milstein,
            Method::MilsteinStratonovich,
            Method::HeunStratonovich,
            Method::SrkPlaten,
        ] {
            let (drift, diffusion) = gbm(0.02, 0.3);
            let options = Options::fixed(method, Structure::Diagonal, 3, 0.0, 0.01, 10);
            let path = integrate(drift, diffusion, &[1.0], &options)
                .unwrap_or_else(|error| panic!("{}: {error}", method.name()));
            assert_eq!(path.method, method);
            assert!(path.y.iter().flatten().all(|value| value.is_finite()));
            let order = sde_order(method);
            assert!(order.strong > 0.0 && order.strong <= 1.0);
            assert_eq!(order.weak, 1.0);
        }
        assert_eq!(sde_order(Method::EulerMaruyama).strong, 0.5);
        assert_eq!(sde_order(Method::Milstein).strong, 1.0);
        assert_eq!(Method::Milstein.calculus(), Calculus::Ito);
        assert_eq!(
            Method::HeunStratonovich.calculus(),
            Calculus::Stratonovich
        );
        assert_eq!(
            Method::from_name("srk_platen"),
            Some(Method::SrkPlaten)
        );
        assert_eq!(Method::from_name("nope"), None);
    }

    /// For additive noise the Milstein/Heun corrections vanish, so the schemes
    /// coincide on the same Brownian path (they see the same increments).
    #[test]
    fn additive_noise_schemes_agree() {
        let (drift, diffusion) = ou(2.0, 0.0, 0.5);
        let em = integrate(
            drift,
            diffusion,
            &[1.0],
            &Options::fixed(Method::EulerMaruyama, Structure::Diagonal, 5, 0.0, 0.01, 50),
        )
        .expect("EM");
        let (drift, diffusion) = ou(2.0, 0.0, 0.5);
        let milstein = integrate(
            drift,
            diffusion,
            &[1.0],
            &Options::fixed(Method::Milstein, Structure::Diagonal, 5, 0.0, 0.01, 50),
        )
        .expect("Milstein");
        let em_end = em.last().expect("end")[0];
        let milstein_end = milstein.last().expect("end")[0];
        assert!(
            (em_end - milstein_end).abs() < 1e-12,
            "additive noise: EM {em_end} vs Milstein {milstein_end}"
        );
    }

    /// Diagonal and general noise with the same diagonal matrix are the same
    /// process and must produce the same path.
    #[test]
    fn diagonal_and_general_agree_on_a_diagonal_matrix() {
        let sigma = [[0.3, 0.0], [0.0, 0.2]];
        let (drift, diffusion) = (
            |_t: f64, y: &[f64]| Ok(vec![-0.1 * y[0], -0.2 * y[1]]),
            |_t: f64, _y: &[f64]| Ok(vec![sigma[0][0], sigma[1][1]]),
        );
        let diagonal = integrate(
            drift,
            diffusion,
            &[1.0, 2.0],
            &Options::fixed(Method::EulerMaruyama, Structure::Diagonal, 21, 0.0, 0.01, 20),
        )
        .expect("diagonal");
        let (drift, diffusion) = (
            |_t: f64, y: &[f64]| Ok(vec![-0.1 * y[0], -0.2 * y[1]]),
            move |_t: f64, _y: &[f64]| {
                Ok(vec![sigma[0][0], sigma[0][1], sigma[1][0], sigma[1][1]])
            },
        );
        let general = integrate(
            drift,
            diffusion,
            &[1.0, 2.0],
            &Options::fixed(
                Method::EulerMaruyama,
                Structure::General {
                    noise: 2,
                    commutative: true,
                },
                21,
                0.0,
                0.01,
                20,
            ),
        )
        .expect("general");
        assert_eq!(diagonal.y, general.y);
    }

    #[test]
    fn the_noncommutative_method_runs_and_needs_terms() {
        // Two correlated, non-commutative noise channels: G depends on y.
        let drift = |_t: f64, y: &[f64]| Ok(vec![-y[0], -y[1]]);
        let diffusion = |_t: f64, y: &[f64]| {
            Ok(vec![
                0.3 * y[0],
                0.1 * y[1],
                0.1 * y[0],
                0.3 * y[1],
            ])
        };
        let options = Options::fixed(
            Method::MilsteinNoncommutative,
            Structure::General {
                noise: 2,
                commutative: false,
            },
            7,
            0.0,
            0.01,
            10,
        )
        .with_terms(4);
        let first = integrate(drift, diffusion, &[1.0, 1.0], &options).expect("nc path");
        let second = integrate(drift, diffusion, &[1.0, 1.0], &options).expect("nc path");
        assert_eq!(first.y, second.y, "same seed, same path");
        assert!(first.y.iter().flatten().all(|value| value.is_finite()));

        // No `terms` is a parameter error, not a silent default.
        let missing = Options::fixed(
            Method::MilsteinNoncommutative,
            Structure::General {
                noise: 2,
                commutative: false,
            },
            7,
            0.0,
            0.01,
            10,
        );
        let error = integrate(
            |_t: f64, _y: &[f64]| Ok(vec![0.0, 0.0]),
            |_t: f64, _y: &[f64]| Ok(vec![0.0; 4]),
            &[1.0, 1.0],
            &missing,
        )
        .expect_err("terms are required");
        assert_eq!(error.code(), "E0802");
    }

    #[test]
    fn adaptive_paths_are_reproducible_and_report_their_work() {
        let run = || {
            let (drift, diffusion) = ou(1.5, 0.0, 0.4);
            let options = Options::adaptive(
                Method::Milstein,
                Structure::Diagonal,
                99,
                0.0,
                1.0,
                1e-3,
                1e-6,
                100_000,
                0,
            );
            integrate(drift, diffusion, &[2.0], &options).expect("adaptive path")
        };
        let first = run();
        let second = run();
        assert_eq!(first, second);
        assert!(first.t.windows(2).all(|window| window[1] > window[0]));
        assert!((first.t.first().copied().unwrap() - 0.0).abs() < 1e-12);
        assert!((first.t.last().copied().unwrap() - 1.0).abs() < 1e-12);
        assert!(first.steps > 0);
        assert!(first.nfev >= first.steps);
    }

    #[test]
    fn the_noncommutative_adaptive_method_runs() {
        let drift = |_t: f64, y: &[f64]| Ok(vec![-y[0], -2.0 * y[1]]);
        let diffusion = |_t: f64, y: &[f64]| {
            Ok(vec![
                0.2 * y[0],
                0.05 * y[1],
                0.05 * y[0],
                0.2 * y[1],
            ])
        };
        let options = Options::adaptive(
            Method::MilsteinNoncommutative,
            Structure::General {
                noise: 2,
                commutative: false,
            },
            13,
            0.0,
            0.2,
            1e-2,
            1e-5,
            10_000,
            64,
        )
        .with_terms(3);
        let path = integrate(drift, diffusion, &[1.0, 1.0], &options).expect("adaptive nc");
        assert!(path.steps > 0);
        assert!(path.t.windows(2).all(|window| window[1] > window[0]));
    }

    #[test]
    fn ensemble_members_equal_the_individual_runs() {
        let mut options = Options::fixed(Method::Milstein, Structure::Diagonal, 31, 0.0, 0.01, 25);
        let (drift, diffusion) = gbm(0.03, 0.25);
        let members = ensemble(drift, diffusion, &[1.0], &options, 3).expect("ensemble");
        assert_eq!(members.len(), 3);
        for (index, member) in members.iter().enumerate() {
            options.seed = ensemble_seed(31, index);
            let (drift, diffusion) = gbm(0.03, 0.25);
            let individual = integrate(drift, diffusion, &[1.0], &options)
                .expect("the individual run");
            assert_eq!(
                member.y, individual.y,
                "ensemble member {index} is not the individual run"
            );
        }
        // Order independence: re-running the ensemble gives the same members.
        let (drift, diffusion) = gbm(0.03, 0.25);
        let again = ensemble(drift, diffusion, &[1.0], &Options::fixed(
            Method::Milstein,
            Structure::Diagonal,
            31,
            0.0,
            0.01,
            25,
        ), 3)
        .expect("ensemble");
        assert_eq!(members, again);
    }

    #[test]
    fn errors_are_typed() {
        // A drift of the wrong length is E0802.
        let options = Options::fixed(Method::EulerMaruyama, Structure::Diagonal, 1, 0.0, 0.01, 2);
        let error = integrate(
            |_t: f64, _y: &[f64]| Ok(vec![0.0, 0.0, 0.0]),
            |_t: f64, _y: &[f64]| Ok(vec![0.0, 0.0]),
            &[1.0, 1.0],
            &options,
        )
        .expect_err("wrong drift length");
        assert_eq!(error.code(), "E0802");

        // A diffusion of the wrong length is E0802.
        let error = integrate(
            |_t: f64, _y: &[f64]| Ok(vec![0.0]),
            |_t: f64, _y: &[f64]| Ok(vec![0.0, 0.0]),
            &[1.0],
            &options,
        )
        .expect_err("wrong diffusion length");
        assert_eq!(error.code(), "E0802");

        // A zero step is a parameter error.
        let error = integrate(
            |_t: f64, _y: &[f64]| Ok(vec![0.0]),
            |_t: f64, _y: &[f64]| Ok(vec![0.0]),
            &[1.0],
            &Options::fixed(Method::EulerMaruyama, Structure::Diagonal, 1, 0.0, 0.0, 2),
        )
        .expect_err("zero step");
        assert_eq!(error.code(), "E0802");

        // An identical diffusion that becomes non-finite: E0806.
        let error = integrate(
            |_t: f64, _y: &[f64]| Ok(vec![f64::INFINITY]),
            |_t: f64, _y: &[f64]| Ok(vec![0.0]),
            &[1.0],
            &options,
        )
        .expect_err("non-finite");
        assert_eq!(error.code(), "E0806");

        // An adaptive step budget of one step cannot reach t1: E0807.
        let error = integrate(
            |_t: f64, y: &[f64]| Ok(vec![-100.0 * y[0]]),
            |_t: f64, _y: &[f64]| Ok(vec![0.1]),
            &[1.0],
            &Options::adaptive(
                Method::Milstein,
                Structure::Diagonal,
                2,
                0.0,
                1.0,
                1e-12,
                0.0,
                1,
                0,
            ),
        )
        .expect_err("step limit");
        assert_eq!(error.code(), "E0807");

        // A Stratonovich method under an adaptive controller names the gap.
        let error = integrate(
            |_t: f64, _y: &[f64]| Ok(vec![0.0]),
            |_t: f64, _y: &[f64]| Ok(vec![0.1]),
            &[1.0],
            &Options::adaptive(
                Method::HeunStratonovich,
                Structure::Diagonal,
                2,
                0.0,
                1.0,
                1e-3,
                1e-6,
                100,
                0,
            ),
        )
        .expect_err("adaptive Stratonovich");
        assert_eq!(error.code(), "E1001");
        assert_eq!(error.capability(), Some("sde.interpretation"));

        // A commutative method on declared non-commutative noise names the gap.
        let error = integrate(
            |_t: f64, _y: &[f64]| Ok(vec![0.0, 0.0]),
            |_t: f64, _y: &[f64]| Ok(vec![0.0; 4]),
            &[1.0, 1.0],
            &Options::fixed(
                Method::Milstein,
                Structure::General {
                    noise: 2,
                    commutative: false,
                },
                3,
                0.0,
                0.01,
                2,
            ),
        )
        .expect_err("non-commutative Milstein");
        assert_eq!(error.code(), "E1001");
        assert_eq!(error.capability(), Some("sde.noncommutative"));

        // A non-uniform explicit grid is rejected: the kernel's fixed-step
        // solvers assume one step size.
        let error = integrate(
            |_t: f64, _y: &[f64]| Ok(vec![0.0]),
            |_t: f64, _y: &[f64]| Ok(vec![0.1]),
            &[1.0],
            &Options::grid(
                Method::EulerMaruyama,
                Structure::Diagonal,
                1,
                vec![0.0, 0.01, 0.03],
            ),
        )
        .expect_err("non-uniform grid");
        assert_eq!(error.code(), "E0802");
    }

    #[test]
    fn a_uniform_explicit_grid_reaches_the_same_end_as_fixed_steps() {
        let times = vec![0.0, 0.01, 0.02, 0.03];
        let (drift, diffusion) = gbm(0.1, 0.2);
        let grid = integrate(
            drift,
            diffusion,
            &[1.0],
            &Options::grid(Method::EulerMaruyama, Structure::Diagonal, 17, times),
        )
        .expect("grid path");
        let (drift, diffusion) = gbm(0.1, 0.2);
        let fixed = integrate(
            drift,
            diffusion,
            &[1.0],
            &Options::fixed(Method::EulerMaruyama, Structure::Diagonal, 17, 0.0, 0.01, 3),
        )
        .expect("fixed path");
        assert_eq!(grid.y, fixed.y);
    }

    #[test]
    fn fixed_span_reaches_the_end_exactly() {
        let span = Options::fixed_span(
            Method::EulerMaruyama,
            Structure::Diagonal,
            5,
            0.0,
            1.0,
            0.3,
        );
        let (drift, diffusion) = gbm(0.0, 0.1);
        let path = integrate(drift, diffusion, &[1.0], &span).expect("a span path");
        assert_eq!(path.steps, 3, "the span is divided into whole steps");
        assert!((path.t.last().copied().unwrap() - 1.0).abs() < 1e-12);
    }

    /// The calculus is part of the scheme name: on the same Brownian path the
    /// Itô and Stratonovich Milstein solutions differ by the Itô correction,
    /// which for `dX = sigma X dW` makes the Stratonovich path larger by
    /// `exp(sigma^2 T / 2)` in the limit (here with the O(h) discretisation
    /// error much smaller than the tolerance).
    #[test]
    fn ito_and_stratonovich_differ_by_the_expected_correction() {
        let sigma = 0.2;
        let dt = 0.001;
        let steps = 100;
        let horizon = dt * steps as f64;
        let (drift, diffusion) = gbm(0.0, sigma);
        let ito = integrate(
            drift,
            diffusion,
            &[1.0],
            &Options::fixed(Method::Milstein, Structure::Diagonal, 77, 0.0, dt, steps),
        )
        .expect("Ito path");
        let (drift, diffusion) = gbm(0.0, sigma);
        let stratonovich = integrate(
            drift,
            diffusion,
            &[1.0],
            &Options::fixed(
                Method::MilsteinStratonovich,
                Structure::Diagonal,
                77,
                0.0,
                dt,
                steps,
            ),
        )
        .expect("Stratonovich path");
        let ratio = stratonovich.last().unwrap()[0] / ito.last().unwrap()[0];
        let expected = (0.5 * sigma * sigma * horizon).exp();
        assert!(
            (ratio - expected).abs() < 1e-3,
            "Ito vs Stratonovich ratio {ratio} should be exp(sigma^2 T/2) = {expected}"
        );
    }

    /// The kernel's adaptive controller integrates the Milstein solution for
    /// both Itô requests; the code keeps the requested name but the path is the
    /// same. This is the documented contract, asserted so it cannot drift into a
    /// silent scheme substitution either way.
    #[test]
    fn adaptive_em_and_milstein_are_the_same_controlled_path() {
        let options = |method| {
            Options::adaptive(
                method,
                Structure::Diagonal,
                123,
                0.0,
                0.5,
                1e-3,
                1e-6,
                100_000,
                0,
            )
        };
        let (drift, diffusion) = ou(2.0, 0.0, 0.3);
        let em = integrate(drift, diffusion, &[1.0], &options(Method::EulerMaruyama))
            .expect("adaptive EM");
        let (drift, diffusion) = ou(2.0, 0.0, 0.3);
        let milstein = integrate(drift, diffusion, &[1.0], &options(Method::Milstein))
            .expect("adaptive Milstein");
        assert_eq!(em.y, milstein.y);
    }

    #[test]
    fn the_noncommutative_method_needs_noncommutative_noise() {
        let error = integrate(
            |_t: f64, _y: &[f64]| Ok(vec![0.0]),
            |_t: f64, _y: &[f64]| Ok(vec![0.1]),
            &[1.0],
            &Options::fixed(
                Method::MilsteinNoncommutative,
                Structure::Diagonal,
                1,
                0.0,
                0.01,
                2,
            )
            .with_terms(4),
        )
        .expect_err("non-commutative method on diagonal noise");
        assert_eq!(error.code(), "E1001");
        assert_eq!(error.capability(), Some("sde.noncommutative"));
    }
}
