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
//! The **method name selects the scheme family and the calculus; the structure
//! selects the commutative or the non-commutative kernel**:
//!
//! | method | calculus | commutative noise (`Diagonal`, `General{commutative: true}`) | non-commutative noise (`General{commutative: false}`) | order strong/weak |
//! |---|---|---|---|---|
//! | `EulerMaruyama` | Itô | Euler–Maruyama | Euler–Maruyama | 0.5 / 1 |
//! | `Milstein` | Itô | Milstein | Milstein with Lévy areas | 1 / 1 |
//! | `MilsteinStratonovich` | Stratonovich | Milstein (Strat.) | Milstein (Strat.) with Lévy areas | 1 / 1 |
//! | `HeunStratonovich` | Stratonovich | Heun | Heun | 1 / 1 (commutative); **0.5 / 1** (non-commutative) |
//! | `SrkPlaten` | Itô | Platen's derivative-free SRK | Kloeden–Platen SRK (11.1.7) with Lévy areas | 1 / 1 |
//! | `MilsteinNoncommutative` | Itô | Milstein with Lévy areas (explicit alias) | same | 1 / 1 |
//! | `MilsteinNoncommutativeStratonovich` | Stratonovich | Milstein (Strat.) with Lévy areas (alias) | same | 1 / 1 |
//!
//! The calculus is part of the method name, not an orthogonal option: nearly
//! every Itô/Stratonovich mix-up in the wild is a scheme whose name does not say
//! which integral it converges to. [`sde_order_for`] reports the order for a
//! `(method, structure)` pair; [`sde_order`] is the commutative-noise order.
//!
//! Step policy is fixed (`dt`/`steps`, or an explicit **uniform** grid) or
//! adaptive (`rtol`/`atol`, Brownian-bridge rejection in the kernel). Adaptive
//! integration works for every method:
//!
//! * Itô, commutative noise (`EulerMaruyama`, `Milstein`, `SrkPlaten`): the
//!   kernel's embedded EM/Milstein controller, which returns the *Milstein*
//!   solution, so these requests give the same controlled path (the requested
//!   name is kept in [`Path::method`]);
//! * Stratonovich, commutative noise (`MilsteinStratonovich`,
//!   `HeunStratonovich`): **exact conversion to Itô form** — the drift becomes
//!   `a~ = a + 1/2 sum_j (D_j g)_{.j}` and the Itô controller runs on `(a~, g)`.
//!   The returned path is the controlled Itô–Milstein solution of the equivalent
//!   Itô SDE, which converges to the Stratonovich solution;
//! * non-commutative noise (and the `*Noncommutative*` methods): the fine-cell
//!   controller with Chen's relation (`cells`, default [`DEFAULT_CELLS`]), the
//!   Itô one, or the Itô-converted one for Stratonovich methods.
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
//! * Fixed-step schemes that use Lévy areas (every method on non-commutative
//!   noise except Euler–Maruyama and Heun, and the `*Noncommutative*` methods)
//!   and every adaptive path need the
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
//!   the Milstein/SRK corrections without Lévy areas are only valid for
//!   commutative noise. Declare `commutative: false` and the same method names
//!   dispatch to the Lévy-area kernels instead (no error).
//! * Lévy areas are simulated with a finite Fourier expansion
//!   ([`Options::terms`]) plus the Gaussian tail correction; accuracy improves
//!   with the mode count. `terms == 0` means "default": [`default_terms`]
//!   `= clamp(ceil(1/sqrt(h)), 16, 4096)` for the step (or the Brownian cell) `h`.
//!   Non-commutative schemes are strong order one at best.
//! * `HeunStratonovich` on non-commutative noise is accepted but is only strong
//!   order 1/2 there (it uses no Lévy areas); use `MilsteinStratonovich` or a
//!   `*Noncommutative*` method for order one.
//! * No events/root-finding and no dense output in this round (`SdeError::Unsupported`,
//!   capability `sde.events`, is reserved for them; no current combination produces it).
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
use std::cell::Cell;
use std::cell::RefCell;

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
        /// The capability a build would need (`core.sde`, `sde.events`, …). The
        /// former `sde.noncommutative` and `sde.interpretation` gaps are closed.
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
            | Self::Invalid(_) | Self::Shape(_) => "E0802",
            | Self::NonFinite(_) => "E0806",
            | Self::StepLimit(_) => "E0807",
            | Self::Unsupported { .. } => "E1001",
        }
    }

    /// The capability an `E1001` names, when it is one.
    #[must_use]
    pub fn capability(&self) -> Option<&'static str> {
        match self {
            | Self::Unsupported { capability, .. } => Some(capability),
            | Self::Invalid(_) | Self::Shape(_) | Self::NonFinite(_) | Self::StepLimit(_) => None,
        }
    }

    fn shape(message: impl Into<String>) -> Self {
        Self::Shape(message.into())
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    // Reserved for capabilities that are genuinely missing (`sde.events`); no current
    // method/structure/step-policy combination produces it.
    #[allow(dead_code)]
    fn unsupported(
        capability: &'static str,
        detail: impl Into<String>,
    ) -> Self {
        Self::Unsupported {
            capability,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for SdeError {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            | Self::Invalid(message) | Self::Shape(message) => f.write_str(message),
            | Self::NonFinite(reason) => write!(f, "the state became non-finite: {reason}"),
            | Self::StepLimit(reason) => f.write_str(reason),
            | Self::Unsupported { capability, detail } => {
                write!(f, "this build does not provide `{capability}`: {detail}")
            },
        }
    }
}

impl std::error::Error for SdeError {}

impl From<kernel::SdeError> for SdeError {
    fn from(error: kernel::SdeError) -> Self {
        match error {
            | kernel::SdeError::Invalid { what } => {
                Self::invalid(format!("the kernel rejected the SDE parameters: {what}"))
            },
            | kernel::SdeError::NonFinite { step } => {
                Self::NonFinite(format!("the state left the reals at step {step}"))
            },
            | kernel::SdeError::StepLimit { limit } => {
                Self::StepLimit(format!("the step budget of {limit} was exhausted"))
            },
            | kernel::SdeError::StepUnderflow { t, h } => {
                Self::StepLimit(format!("the step size underflowed at t = {t} (h = {h:e})"))
            },
            | kernel::SdeError::StepBelowCell { cells } => {
                Self::StepLimit(format!(
                    "an adaptive step cannot be shorter than one of the {cells} Brownian cells; \
                 use more cells"
                ))
            },
        }
    }
}

impl From<NumericError> for SdeError {
    fn from(error: NumericError) -> Self {
        match error {
            | NumericError::NotConverged => {
                Self::StepLimit("the iteration did not converge".into())
            },
            | NumericError::Domain(message) => Self::invalid(message),
            | other => Self::invalid(other.to_string()),
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
    /// Milstein (Itô) with simulated Lévy areas, whatever the structure (an
    /// explicit alias; `Milstein` already dispatches here on non-commutative
    /// noise).
    MilsteinNoncommutative,
    /// Milstein (Stratonovich) with simulated Lévy areas, whatever the structure
    /// (an explicit alias of `MilsteinStratonovich` on non-commutative noise).
    MilsteinNoncommutativeStratonovich,
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
            | Self::EulerMaruyama => "euler_maruyama",
            | Self::Milstein => "milstein",
            | Self::MilsteinStratonovich => "milstein_stratonovich",
            | Self::HeunStratonovich => "heun_stratonovich",
            | Self::SrkPlaten => "srk_platen",
            | Self::MilsteinNoncommutative => "milstein_noncommutative",
            | Self::MilsteinNoncommutativeStratonovich => "milstein_noncommutative_stratonovich",
        }
    }

    /// The method named `name`, if it is one this build knows.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            | "euler_maruyama" | "em" => Self::EulerMaruyama,
            | "milstein" => Self::Milstein,
            | "milstein_stratonovich" => Self::MilsteinStratonovich,
            | "heun_stratonovich" | "heun" => Self::HeunStratonovich,
            | "srk_platen" | "srk" => Self::SrkPlaten,
            | "milstein_noncommutative" => Self::MilsteinNoncommutative,
            | "milstein_noncommutative_stratonovich" => Self::MilsteinNoncommutativeStratonovich,
            | _ => return None,
        })
    }

    /// The integral the scheme converges to.
    #[must_use]
    pub fn calculus(self) -> Calculus {
        match self {
            | Self::EulerMaruyama
            | Self::Milstein
            | Self::SrkPlaten
            | Self::MilsteinNoncommutative => Calculus::Ito,
            | Self::MilsteinStratonovich
            | Self::HeunStratonovich
            | Self::MilsteinNoncommutativeStratonovich => Calculus::Stratonovich,
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

/// The strong/weak order of `method` on **commutative** noise (see
/// [`sde_order_for`] for a given structure), as the kernel's documentation states it.
#[must_use]
pub fn sde_order(method: Method) -> Order {
    match method {
        // Euler–Maruyama is strong 1/2, weak 1.
        | Method::EulerMaruyama => {
            Order {
                strong: 0.5,
                weak: 1.0,
            }
        },
        // Every other implemented scheme is strong order one. The kernel is
        // explicit that they are weak order one too (not two).
        | Method::Milstein
        | Method::MilsteinStratonovich
        | Method::HeunStratonovich
        | Method::SrkPlaten
        | Method::MilsteinNoncommutative
        | Method::MilsteinNoncommutativeStratonovich => {
            Order {
                strong: 1.0,
                weak: 1.0,
            }
        },
    }
}

/// The strong/weak order of `method` on noise of the given `structure`.
///
/// As [`sde_order`], except that `HeunStratonovich` on non-commutative noise is
/// only strong order 1/2 (it has no Lévy-area term), and that the schemes which
/// dispatch to the Lévy-area kernels on non-commutative noise keep strong order
/// one.
#[must_use]
pub fn sde_order_for(
    method: Method,
    structure: Structure,
) -> Order {
    if method == Method::HeunStratonovich && !structure.is_commutative() {
        return Order {
            strong: 0.5,
            weak: 1.0,
        };
    }
    sde_order(method)
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
    pub fn noise(
        self,
        state: usize,
    ) -> usize {
        match self {
            | Self::Diagonal => state,
            | Self::General { noise, .. } => noise,
        }
    }

    /// Whether the structure is (declared) commutative.
    #[must_use]
    pub fn is_commutative(self) -> bool {
        match self {
            | Self::Diagonal => true,
            | Self::General { commutative, .. } => commutative,
        }
    }

    /// How many values the diffusion callback returns.
    #[must_use]
    pub fn coefficients(
        self,
        state: usize,
    ) -> usize {
        match self {
            | Self::Diagonal => state,
            | Self::General { noise, .. } => state * noise,
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
        /// Ignored by the commutative solvers; `0` selects [`DEFAULT_CELLS`] when
        /// the non-commutative solver is used.
        cells: usize,
    },
}

/// Options of one integration.
///
/// There is no `Default`: a seed and a step are not guessable, so they are
/// constructor arguments (the spec requires an explicit seed for reproducibility
/// and an explicit step policy). [`Options::with_terms`] is optional: `0` selects
/// [`default_terms`].
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
    /// Fourier modes of the simulated Lévy areas; `0` (the default) selects
    /// [`default_terms`] for the step (or Brownian cell) size.
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
    pub fn grid(
        method: Method,
        structure: Structure,
        seed: u64,
        times: Vec<f64>,
    ) -> Self {
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
    pub fn with_terms(
        mut self,
        terms: usize,
    ) -> Self {
        self.terms = terms;
        self
    }
}

/// Brownian cells of the non-commutative adaptive solver when `cells == 0`.
pub const DEFAULT_CELLS: usize = 4096;

/// The default number of Fourier modes of the simulated Lévy areas for a step
/// (or Brownian cell) of size `h`: `clamp(ceil(1 / sqrt(h)), 16, 4096)`.
///
/// The truncation error of the area decays like `h p^(-3/2)` with the kernel's
/// Gaussian tail correction, so this rule keeps it far below the `O(h)` strong
/// error of the scheme at an `O(m^2 / sqrt(h))` sampling cost per step.
#[must_use]
pub fn default_terms(h: f64) -> usize {
    let h = h.abs();
    if !(h.is_finite() && h > 0.0) {
        return 16;
    }
    let p = (1.0 / h.sqrt()).ceil();
    if p >= 4096.0 {
        4096
    } else {
        (p as usize).max(16)
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
pub fn wiener(
    seed: u64,
    m: usize,
    steps: usize,
    dt: f64,
) -> Vec<Vec<f64>> {
    let mut stream = Stream::new(seed);
    wiener_from(&mut stream, m, dt, steps)
}

fn wiener_from(
    stream: &mut Stream,
    m: usize,
    dt: f64,
    steps: usize,
) -> Vec<Vec<f64>> {
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
pub fn ensemble_seed(
    seed: u64,
    index: usize,
) -> u64 {
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
    fn new(
        drift: A,
        diffusion: G,
        state: usize,
        structure: Structure,
    ) -> Self {
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
    fn drift_at(
        &self,
        t: f64,
        y: &[f64],
        out: &mut [f64],
    ) {
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
            | Ok(values) if values.len() == self.state => out.copy_from_slice(&values),
            | Ok(values) => {
                *self.drift_error.borrow_mut() = Some(SdeError::shape(format!(
                    "the drift returned {} value(s) for a state of {}",
                    values.len(),
                    self.state
                )));
                out.fill(0.0);
            },
            | Err(error) => {
                *self.drift_error.borrow_mut() = Some(error);
                out.fill(0.0);
            },
        }
    }

    /// Writes the diffusion into the kernel's `n × m` output slice.
    fn diffusion_at(
        &self,
        t: f64,
        y: &[f64],
        out: &mut [f64],
    ) {
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
            | Ok(values) if values.len() == self.coefficients => {
                if self.diagonal {
                    // Expand the diagonal to the kernel's dense n×n layout.
                    out.fill(0.0);
                    for index in 0..self.state {
                        out[index * self.state + index] = values[index];
                    }
                } else {
                    out.copy_from_slice(&values);
                }
            },
            | Ok(values) => {
                *self.diffusion_error.borrow_mut() = Some(SdeError::shape(format!(
                    "the diffusion returned {} coefficient(s); this structure needs {}",
                    values.len(),
                    self.coefficients
                )));
                out.fill(0.0);
            },
            | Err(error) => {
                *self.diffusion_error.borrow_mut() = Some(error);
                out.fill(0.0);
            },
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
/// The adaptive controller integrates with the kernel's Milstein-controlled
/// step size: the returned states are the kernel's Milstein solution for the
/// `EulerMaruyama`, `Milstein` and `SrkPlaten` requests (Itô). For the
/// Stratonovich methods the drift is converted exactly to its Itô equivalent
/// (`a~ = a + 1/2 sum_j (D_j g)_{.j}`) and the same controller runs on the
/// converted SDE: the returned path is the controlled Itô–Milstein solution of
/// the equivalent Itô SDE, which converges to the Stratonovich solution.
/// Non-commutative noise uses the fine-cell controller (`cells`, default
/// [`DEFAULT_CELLS`]). This is the kernel's documented contract, not a hidden
/// substitution of the requested scheme's control law.
///
/// # Errors
///
/// `E0802` for shapes/parameters, `E0806` for a non-finite state, `E0807` for a
/// step collapse. No method/structure/step-policy combination is refused with
/// `E1001` any more.
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
        | Schedule::Fixed { dt, steps } => fixed(&adapters, y0, options, *dt, *steps, noise),
        | Schedule::Grid { times } => {
            let (dt, steps) = uniform_grid(times)?;
            if (times[0] - options.t0).abs() > 1e-12 * options.t0.abs().max(1.0) {
                return Err(SdeError::invalid(
                    "the explicit grid does not start at the initial time",
                ));
            }
            fixed(&adapters, y0, options, dt, steps, noise)
        },
        | Schedule::Adaptive {
            t1,
            rtol,
            atol,
            max_steps,
            cells,
        } => {
            adaptive(
                &adapters, y0, options, *t1, *rtol, *atol, *max_steps, *cells,
            )
        },
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
fn validate_options(
    options: &Options,
    y0: &[f64],
) -> Result<(), SdeError> {
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
    match &options.schedule {
        | Schedule::Fixed { dt, steps } => {
            if !dt.is_finite() || *dt <= 0.0 {
                return Err(SdeError::invalid(
                    "the step size must be finite and positive",
                ));
            }
            if *steps == 0 {
                return Err(SdeError::invalid("the step count must be positive"));
            }
        },
        | Schedule::Grid { times } => {
            uniform_grid(times)?;
        },
        | Schedule::Adaptive {
            t1,
            rtol,
            atol,
            max_steps,
            cells,
        } => {
            if !t1.is_finite() || *t1 <= options.t0 {
                return Err(SdeError::invalid(
                    "the end time must be after the initial time",
                ));
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
            let _ = cells;
        },
    }
    Ok(())
}

/// The step size and count of a uniform explicit grid.
fn uniform_grid(times: &[f64]) -> Result<(f64, usize), SdeError> {
    if times.len() < 2 {
        return Err(SdeError::invalid("the time grid needs at least two points"));
    }
    if !times.iter().all(|time| time.is_finite()) {
        return Err(SdeError::invalid(
            "the time grid contains a non-finite time",
        ));
    }
    let dt = times[1] - times[0];
    if dt <= 0.0 {
        return Err(SdeError::invalid(
            "the time grid must be strictly increasing",
        ));
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

/// Whether a fixed-step run needs the kernel's Lévy-area noise: the explicit
/// `*Noncommutative*` methods always, and `Milstein`, `MilsteinStratonovich` and
/// `SrkPlaten` on non-commutative noise. Euler–Maruyama and Heun use plain
/// increments (and Heun is then only strong order 1/2).
fn uses_levy_areas(options: &Options) -> bool {
    match options.method {
        | Method::MilsteinNoncommutative | Method::MilsteinNoncommutativeStratonovich => true,
        | Method::Milstein | Method::MilsteinStratonovich | Method::SrkPlaten => {
            !options.structure.is_commutative()
        },
        | Method::EulerMaruyama | Method::HeunStratonovich => false,
    }
}

/// Whether an adaptive run uses the non-commutative (fine-cell) controller.
fn adaptive_noncommutative(options: &Options) -> bool {
    !options.structure.is_commutative()
        || matches!(
            options.method,
            Method::MilsteinNoncommutative | Method::MilsteinNoncommutativeStratonovich
        )
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
    let states = if uses_levy_areas(options) {
        // Lévy areas are sampled by the kernel's stream (it needs the same Rng
        // that draws the bridge/fourier coefficients).
        let mut rng = Rng::new(options.seed);
        let terms = if options.terms == 0 {
            default_terms(dt)
        } else {
            options.terms
        };
        let sample = kernel::sample_noise(&mut rng, noise, steps, dt, terms);
        match options.method {
            | Method::MilsteinStratonovich | Method::MilsteinNoncommutativeStratonovich => {
                kernel::milstein_noncommutative_stratonovich(
                    drift, diffusion, noise, t0, dt, y0, &sample,
                )
            },
            | Method::SrkPlaten => {
                kernel::srk_noncommutative(drift, diffusion, noise, t0, dt, y0, &sample)
            },
            | _ => kernel::milstein_noncommutative(drift, diffusion, noise, t0, dt, y0, &sample),
        }
        .map_err(SdeError::from)?
    } else {
        let increments = wiener(options.seed, noise, steps, dt);
        match options.method {
            | Method::EulerMaruyama => {
                kernel::euler_maruyama(drift, diffusion, noise, t0, dt, y0, &increments)
            },
            | Method::Milstein => {
                kernel::milstein(drift, diffusion, noise, t0, dt, y0, &increments)
            },
            | Method::MilsteinStratonovich => {
                kernel::milstein_stratonovich(drift, diffusion, noise, t0, dt, y0, &increments)
            },
            | Method::HeunStratonovich => {
                kernel::heun_stratonovich(drift, diffusion, noise, t0, dt, y0, &increments)
            },
            | Method::SrkPlaten => {
                kernel::srk_platen(drift, diffusion, noise, t0, dt, y0, &increments)
            },
            // The explicit Lévy-area methods are routed by `uses_levy_areas`.
            | Method::MilsteinNoncommutative | Method::MilsteinNoncommutativeStratonovich => {
                return Err(SdeError::invalid(
                    "internal: a Lévy-area method took the plain path",
                ));
            },
        }
        .map_err(SdeError::from)?
    };
    if let Some(failure) = adapters.failure() {
        return Err(failure);
    }
    let t: Vec<f64> = (0..states.len())
        .map(|step| t0 + step as f64 * dt)
        .collect();
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
    let stratonovich = options.method.calculus() == Calculus::Stratonovich;
    let base = kernel::SdeAdaptiveOptions {
        rtol,
        atol,
        seed: options.seed,
        max_steps,
    };
    let path = if adaptive_noncommutative(options) {
        let cell_count = if cells == 0 {
            DEFAULT_CELLS
        } else {
            cells
        };
        let cell_width = (t1 - options.t0) / cell_count as f64;
        let terms = if options.terms == 0 {
            default_terms(cell_width)
        } else {
            options.terms
        };
        let nc = kernel::SdeNoncommutativeOptions {
            base,
            cells: cell_count,
            terms,
        };
        if stratonovich {
            kernel::sde_adaptive_noncommutative_stratonovich_with(
                drift, diffusion, noise, options.t0, t1, y0, &nc,
            )
        } else {
            kernel::sde_adaptive_noncommutative_with(
                drift, diffusion, noise, options.t0, t1, y0, &nc,
            )
        }
    } else if stratonovich {
        kernel::sde_adaptive_stratonovich_with(drift, diffusion, noise, options.t0, t1, y0, &base)
    } else {
        kernel::sde_adaptive_with(drift, diffusion, noise, options.t0, t1, y0, &base)
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

// ---------------------------------------------------------------------------
// Continuous output and events on paths (§8.9.1)
// ---------------------------------------------------------------------------

impl Path {
    /// The state at time `t`, by linear interpolation between the accepted
    /// nodes; `None` outside `[t_first, t_last]`.
    ///
    /// A numerical SDE path is only known at its nodes. Between them the
    /// interpolant is the chord, which is continuous and deterministic. It is
    /// as accurate as the scheme itself (strong order at most 1, and a Brownian
    /// path is only Hölder-1/2 between nodes anyway); it does not add the
    /// within-step Brownian-bridge variance, so statistics of `eval` between
    /// nodes understate the within-step variation by `O(dt)`. Use the node
    /// values (`y`) when exact distributional statistics at the nodes are
    /// wanted.
    #[must_use]
    pub fn eval(
        &self,
        t: f64,
    ) -> Option<Vec<f64>> {
        let (&first, &last) = (self.t.first()?, self.t.last()?);
        if !(first..=last).contains(&t) {
            return None;
        }
        let upper = self.t.partition_point(|&node| node < t);
        if upper == 0 {
            return self.y.first().cloned();
        }
        if self.t[upper] == t {
            return Some(self.y[upper].clone());
        }
        let lower = upper - 1;
        let weight = (t - self.t[lower]) / (self.t[upper] - self.t[lower]);
        Some(
            self.y[lower]
                .iter()
                .zip(&self.y[upper])
                .map(|(a, b)| a + weight * (b - a))
                .collect(),
        )
    }

    /// The states at the requested times (`saveat`): [`Path::eval`] at each.
    ///
    /// # Errors
    ///
    /// `E0802` when a time lies outside the path's span.
    pub fn resample(
        &self,
        times: &[f64],
    ) -> Result<Vec<Vec<f64>>, SdeError> {
        times
            .iter()
            .map(|&t| {
                self.eval(t).ok_or_else(|| {
                    SdeError::invalid("a requested time lies outside the path's span")
                })
            })
            .collect()
    }
}

/// How one event function of a path is watched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventSpec {
    /// The path is cut at the first such event.
    pub terminal: bool,
    /// `1` rising only, `-1` falling only, `0` both.
    pub direction: i8,
}

/// An event on a path.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    /// Index of the event function.
    pub index: usize,
    /// The crossing time.
    pub t: f64,
    /// The interpolated state at the crossing.
    pub y: Vec<f64>,
}

/// Detects sign changes of `g_i(t, y)` along a path.
///
/// Between two consecutive nodes the path is the chord of [`Path::eval`]; a
/// crossing is a sign change of `g` at the two nodes in an allowed direction
/// (falling `g0 > 0 >= g1`, rising `g0 < 0 <= g1`, the kernel's convention
/// for ODE events), and its time is the root of `g` along the chord, found by
/// bisection. A node where `g` is exactly zero counts once (as the end of the
/// step that reaches it). Only the discretised path is watched: the
/// continuous process may cross and return between two nodes, which no
/// discrete scheme can see.
///
/// Hits come in time order. If an event is `terminal`, the returned path ends
/// at the first terminal hit (the hit is appended as its last node) and later
/// hits are dropped. The returned path is the input when nothing terminates.
pub fn events(
    path: &Path,
    specs: &[EventSpec],
    g: &mut dyn FnMut(usize, f64, &[f64]) -> f64,
) -> (Path, Vec<Hit>) {
    let mut hits: Vec<Hit> = Vec::new();
    let mut previous: Vec<f64> = match (path.t.first(), path.y.first()) {
        | (Some(&t), Some(y)) => (0..specs.len()).map(|i| g(i, t, y)).collect(),
        | _ => return (path.clone(), hits),
    };
    for k in 1..path.t.len() {
        let (t0, t1) = (path.t[k - 1], path.t[k]);
        let mut step_hits: Vec<Hit> = Vec::new();
        for (index, spec) in specs.iter().enumerate() {
            let g1 = g(index, t1, &path.y[k]);
            let g0 = previous[index];
            let crossing = (g0 < 0.0 && g1 >= 0.0 && spec.direction >= 0)
                || (g0 > 0.0 && g1 <= 0.0 && spec.direction <= 0);
            if crossing {
                let at = |s: f64| -> Vec<f64> {
                    path.y[k - 1]
                        .iter()
                        .zip(&path.y[k])
                        .map(|(a, b)| a + s * (b - a))
                        .collect()
                };
                let (mut lo, mut hi) = (0.0f64, 1.0f64);
                for _ in 0..64 {
                    let mid = f64::midpoint(lo, hi);
                    let value = g(index, t0 + mid * (t1 - t0), &at(mid));
                    if value == 0.0 {
                        hi = mid;
                        break;
                    }
                    if (value < 0.0) == (g0 < 0.0) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                let s = hi;
                step_hits.push(Hit {
                    index,
                    t: t0 + s * (t1 - t0),
                    y: at(s),
                });
            }
            previous[index] = g1;
        }
        step_hits.sort_by(|a, b| a.t.total_cmp(&b.t));
        if let Some(position) = step_hits.iter().position(|hit| specs[hit.index].terminal) {
            step_hits.truncate(position + 1);
            let last = step_hits[position].clone();
            hits.extend(step_hits);
            let mut cut = path.clone();
            cut.t.truncate(k);
            cut.y.truncate(k);
            cut.t.push(last.t);
            cut.y.push(last.y);
            cut.steps = cut.t.len() - 1;
            return (cut, hits);
        }
        hits.extend(step_hits);
    }
    (path.clone(), hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default seed of `Interp`'s language RNG.
    const INTERP_SEED: u64 = 0x2545_f491_4f6c_dd1d;

    /// A boxed drift or diffusion callback, so a test can name the pair.
    type Coeff = Box<dyn FnMut(f64, &[f64]) -> Result<Vec<f64>, SdeError>>;

    /// Geometric Brownian motion: `dX = mu X dt + sigma X dW` (scalar).
    fn gbm(
        mu: f64,
        sigma: f64,
    ) -> (Coeff, Coeff) {
        (
            Box::new(move |_t: f64, y: &[f64]| Ok(vec![mu * y[0]])),
            Box::new(move |_t: f64, y: &[f64]| Ok(vec![sigma * y[0]])),
        )
    }

    /// Ornstein–Uhlenbeck: `dX = theta (mu - X) dt + sigma dW`.
    fn ou(
        theta: f64,
        mu: f64,
        sigma: f64,
    ) -> (Coeff, Coeff) {
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
        let options = Options::fixed(
            Method::EulerMaruyama,
            Structure::Diagonal,
            11,
            0.0,
            0.01,
            20,
        );
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
            Method::MilsteinNoncommutative,
            Method::MilsteinNoncommutativeStratonovich,
        ] {
            let (drift, diffusion) = gbm(0.02, 0.3);
            let options =
                Options::fixed(method, Structure::Diagonal, 3, 0.0, 0.01, 10).with_terms(4);
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
        assert_eq!(Method::HeunStratonovich.calculus(), Calculus::Stratonovich);
        assert_eq!(Method::from_name("srk_platen"), Some(Method::SrkPlaten));
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
            &Options::fixed(
                Method::EulerMaruyama,
                Structure::Diagonal,
                21,
                0.0,
                0.01,
                20,
            ),
        )
        .expect("diagonal");
        let (drift, diffusion) = (
            |_t: f64, y: &[f64]| Ok(vec![-0.1 * y[0], -0.2 * y[1]]),
            move |_t: f64, _y: &[f64]| Ok(vec![sigma[0][0], sigma[0][1], sigma[1][0], sigma[1][1]]),
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
        let diffusion =
            |_t: f64, y: &[f64]| Ok(vec![0.3 * y[0], 0.1 * y[1], 0.1 * y[0], 0.3 * y[1]]);
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

        // `terms == 0` selects the documented default rule, not an error.
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
        let defaulted = integrate(drift, diffusion, &[1.0, 1.0], &missing).expect("default terms");
        let explicit = integrate(
            drift,
            diffusion,
            &[1.0, 1.0],
            &missing.clone().with_terms(default_terms(0.01)),
        )
        .expect("explicit terms");
        assert_eq!(defaulted.y, explicit.y, "terms = 0 means default_terms(dt)");
        assert_eq!(default_terms(0.01), 16);
        assert_eq!(default_terms(1e-4), 100);
        assert_eq!(default_terms(1e-12), 4096);
        assert_eq!(default_terms(0.0), 16);
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
        let diffusion =
            |_t: f64, y: &[f64]| Ok(vec![0.2 * y[0], 0.05 * y[1], 0.05 * y[0], 0.2 * y[1]]);
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
            let individual =
                integrate(drift, diffusion, &[1.0], &options).expect("the individual run");
            assert_eq!(
                member.y, individual.y,
                "ensemble member {index} is not the individual run"
            );
        }
        // Order independence: re-running the ensemble gives the same members.
        let (drift, diffusion) = gbm(0.03, 0.25);
        let again = ensemble(
            drift,
            diffusion,
            &[1.0],
            &Options::fixed(Method::Milstein, Structure::Diagonal, 31, 0.0, 0.01, 25),
            3,
        )
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

        // A Stratonovich method under an adaptive controller now runs (Ito conversion).
        let path = integrate(
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
                10_000,
                0,
            ),
        )
        .expect("adaptive Stratonovich");
        assert!(path.steps > 0);

        // A commutative-only method on declared non-commutative noise dispatches
        // to the Levy-area kernel instead of failing.
        let path = integrate(
            |_t: f64, _y: &[f64]| Ok(vec![0.0, 0.0]),
            |_t: f64, y: &[f64]| Ok(vec![0.1 * y[1], 0.1, 0.2, 0.1 * y[0]]),
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
        .expect("non-commutative Milstein");
        assert_eq!(path.steps, 2);

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
        let span =
            Options::fixed_span(Method::EulerMaruyama, Structure::Diagonal, 5, 0.0, 1.0, 0.3);
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
    fn the_noncommutative_methods_also_run_on_commutative_noise() {
        // The Levy-area methods are valid for any structure (the areas just
        // drop out of the commutative correction).
        for method in [
            Method::MilsteinNoncommutative,
            Method::MilsteinNoncommutativeStratonovich,
        ] {
            let path = integrate(
                |_t: f64, _y: &[f64]| Ok(vec![0.0]),
                |_t: f64, y: &[f64]| Ok(vec![0.1 * y[0]]),
                &[1.0],
                &Options::fixed(method, Structure::Diagonal, 1, 0.0, 0.01, 4).with_terms(4),
            )
            .expect("Levy-area method on diagonal noise");
            assert_eq!(path.steps, 4);
            assert_eq!(path.method, method);
        }
    }

    const NC: Structure = Structure::General {
        noise: 2,
        commutative: false,
    };

    /// Two non-commuting noise fields on a 2-state system.
    fn nc_system() -> (Coeff, Coeff) {
        (
            Box::new(|_t: f64, y: &[f64]| Ok(vec![-0.5 * y[0], -0.3 * y[1]])),
            Box::new(|_t: f64, y: &[f64]| Ok(vec![0.3 * y[1], 0.1, 0.2, 0.3 * y[0]])),
        )
    }

    /// The method name selects the family, the structure the kernel: `Milstein` /
    /// `MilsteinStratonovich` on non-commutative noise are the explicit
    /// non-commutative methods, bit for bit.
    #[test]
    fn method_names_dispatch_on_the_structure() {
        let run = |method| {
            let (drift, diffusion) = nc_system();
            let options = Options::fixed(method, NC, 17, 0.0, 0.01, 12).with_terms(5);
            integrate(drift, diffusion, &[1.0, 0.5], &options).expect("path")
        };
        assert_eq!(
            run(Method::Milstein).y,
            run(Method::MilsteinNoncommutative).y
        );
        assert_eq!(
            run(Method::MilsteinStratonovich).y,
            run(Method::MilsteinNoncommutativeStratonovich).y
        );
        assert_ne!(
            run(Method::Milstein).y,
            run(Method::MilsteinStratonovich).y,
            "the calculus is part of the method"
        );
        for method in [
            Method::SrkPlaten,
            Method::HeunStratonovich,
            Method::EulerMaruyama,
        ] {
            let path = run(method);
            assert!(path.y.iter().flatten().all(|value| value.is_finite()));
            assert_eq!(path.method, method);
        }
        // SRK on non-commutative noise is not the commutative SRK.
        assert_ne!(run(Method::SrkPlaten).y, run(Method::EulerMaruyama).y);
    }

    #[test]
    fn orders_depend_on_the_structure() {
        assert_eq!(sde_order_for(Method::HeunStratonovich, NC).strong, 0.5);
        assert_eq!(
            sde_order_for(Method::HeunStratonovich, Structure::Diagonal).strong,
            1.0
        );
        assert_eq!(sde_order_for(Method::Milstein, NC).strong, 1.0);
        assert_eq!(sde_order_for(Method::SrkPlaten, NC).strong, 1.0);
        assert_eq!(sde_order_for(Method::EulerMaruyama, NC).strong, 0.5);
        assert_eq!(
            sde_order(Method::MilsteinNoncommutativeStratonovich).strong,
            1.0
        );
        assert_eq!(
            Method::from_name("milstein_noncommutative_stratonovich"),
            Some(Method::MilsteinNoncommutativeStratonovich)
        );
        assert_eq!(
            Method::MilsteinNoncommutativeStratonovich.calculus(),
            Calculus::Stratonovich
        );
    }

    /// Adaptive Stratonovich on `dX = sigma X o dW`: the mean over paths is
    /// `exp(sigma^2 T / 2)` (the Ito mean would be 1), and adaptive `SrkPlaten`
    /// runs the Ito controller.
    #[test]
    fn adaptive_stratonovich_has_the_stratonovich_mean() {
        let sigma = 0.5;
        let paths = 300_u64;
        let (mut sum, mut square) = (0.0, 0.0);
        for seed in 0..paths {
            let (drift, diffusion) = gbm(0.0, sigma);
            let options = Options::adaptive(
                Method::MilsteinStratonovich,
                Structure::Diagonal,
                1000 + seed,
                0.0,
                1.0,
                1e-3,
                1e-4,
                100_000,
                0,
            );
            let path = integrate(drift, diffusion, &[1.0], &options).expect("adaptive Strat");
            let x = path.last().expect("end")[0];
            sum += x;
            square += x * x;
        }
        let n = paths as f64;
        let mean = sum / n;
        let se = ((square / n - mean * mean) / n).sqrt();
        let strat = (0.5 * sigma * sigma).exp();
        assert!(
            (mean - strat).abs() < 4.5 * se,
            "mean {mean} vs {strat} (se {se})"
        );

        let (drift, diffusion) = gbm(0.0, sigma);
        let srk = integrate(
            drift,
            diffusion,
            &[1.0],
            &Options::adaptive(
                Method::SrkPlaten,
                Structure::Diagonal,
                5,
                0.0,
                1.0,
                1e-3,
                1e-4,
                100_000,
                0,
            ),
        )
        .expect("adaptive SRK");
        assert!(srk.steps > 0);
    }

    #[test]
    fn adaptive_noncommutative_runs_for_every_method_with_default_cells() {
        for method in [
            Method::EulerMaruyama,
            Method::Milstein,
            Method::MilsteinStratonovich,
            Method::HeunStratonovich,
            Method::SrkPlaten,
            Method::MilsteinNoncommutativeStratonovich,
        ] {
            let (drift, diffusion) = nc_system();
            let options = Options::adaptive(method, NC, 21, 0.0, 0.2, 1e-2, 1e-5, 100_000, 0);
            let path = integrate(drift, diffusion, &[1.0, 0.5], &options)
                .unwrap_or_else(|error| panic!("{}: {error}", method.name()));
            assert!(path.steps > 0, "{}", method.name());
            assert!((path.t.last().copied().unwrap() - 0.2).abs() < 1e-12);
        }
    }

    // ----- continuous output and events on paths --------------------------

    fn decay_path() -> Path {
        // dX = -X dt with no noise: the Euler path is y_k = (1 - dt)^k.
        let (drift, diffusion) = ou(1.0, 0.0, 0.0);
        let options = Options::fixed(
            Method::EulerMaruyama,
            Structure::Diagonal,
            1,
            0.0,
            0.01,
            200,
        );
        integrate(drift, diffusion, &[1.0], &options).unwrap()
    }

    #[test]
    fn a_path_interpolates_linearly_between_nodes_and_resamples() {
        let path = decay_path();
        for (t, y) in path.t.iter().zip(&path.y) {
            assert_eq!(path.eval(*t).as_ref(), Some(y), "nodes are exact");
        }
        let mid = path.eval(0.005).unwrap()[0];
        assert!((mid - 0.5 * (path.y[0][0] + path.y[1][0])).abs() < 1e-15);
        let last = *path.t.last().unwrap();
        assert!(path.eval(-1e-9).is_none() && path.eval(last + 1e-9).is_none());
        let saved = path.resample(&[0.0, 0.5, last]).unwrap();
        assert_eq!(saved.len(), 3);
        // Chord error against the Euler solution is O(dt^2).
        assert!((saved[1][0] - 0.99f64.powf(50.0)).abs() < 1e-4);
        assert!(path.resample(&[last + 1.0]).is_err());
    }

    #[test]
    fn path_events_locate_crossings_with_direction_and_terminal() {
        let path = decay_path();
        let mut g = |_i: usize, _t: f64, y: &[f64]| y[0] - 0.5;
        let crossing = 0.01 * (0.5f64.ln() / 0.99f64.ln());
        let falling = EventSpec {
            terminal: false,
            direction: -1,
        };
        let (whole, hits) = events(&path, &[falling], &mut g);
        assert_eq!(hits.len(), 1);
        assert!(
            (hits[0].t - crossing).abs() < 1e-4,
            "{} vs {crossing}",
            hits[0].t
        );
        assert!((hits[0].y[0] - 0.5).abs() < 1e-9);
        assert_eq!(whole, path, "no terminal event leaves the path alone");
        // The wrong direction sees nothing.
        let rising = EventSpec {
            terminal: false,
            direction: 1,
        };
        assert!(events(&path, &[rising], &mut g).1.is_empty());
        // A terminal event cuts the path and the hit is its last node.
        let stop = EventSpec {
            terminal: true,
            direction: 0,
        };
        let (cut, hits) = events(&path, &[stop], &mut g);
        assert_eq!(hits.len(), 1);
        assert_eq!(*cut.t.last().unwrap(), hits[0].t);
        assert!((cut.y.last().unwrap()[0] - 0.5).abs() < 1e-9);
        assert!(cut.t.len() < path.t.len());
        assert_eq!(cut.steps, cut.t.len() - 1);
        // Two events: a non-terminal at 0.5 and a terminal at 0.25. Both fire,
        // in time order, and the path stops at the second.
        let mut two = |i: usize, _t: f64, y: &[f64]| y[0] - if i == 0 { 0.5 } else { 0.25 };
        let (cut, hits) = events(
            &path,
            &[
                falling,
                EventSpec {
                    terminal: true,
                    direction: -1,
                },
            ],
            &mut two,
        );
        assert_eq!(
            hits.iter().map(|hit| hit.index).collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert!(hits[0].t < hits[1].t);
        assert!((cut.t.last().unwrap() - 4.0f64.ln()).abs() < 0.02);
    }

    #[test]
    fn a_noisy_path_event_is_reproducible_for_a_seed() {
        let run = |seed: u64| {
            let (drift, diffusion) = gbm(0.0, 0.5);
            let options = Options::fixed(
                Method::Milstein,
                Structure::Diagonal,
                seed,
                0.0,
                0.001,
                2000,
            );
            let path = integrate(drift, diffusion, &[1.0], &options).unwrap();
            let mut g = |_i: usize, _t: f64, y: &[f64]| y[0] - 1.2;
            events(
                &path,
                &[EventSpec {
                    terminal: true,
                    direction: 1,
                }],
                &mut g,
            )
        };
        let (a, hits_a) = run(5);
        let (b, hits_b) = run(5);
        assert_eq!(a, b);
        assert_eq!(hits_a, hits_b);
        assert_ne!(run(6).0, a);
    }
}
