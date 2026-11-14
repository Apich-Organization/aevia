//! The Aevia standard library.
//!
//! Chapter 8 of the specification is the contract: it declares the names, and
//! this crate holds the numerics behind them. `aevia-core` owns the values and
//! the dispatch; nothing here knows what a `Value` is. RSSN owns numerical
//! algorithms; this crate validates language contracts, adapts callbacks and
//! shapes, translates errors, and preserves Aevia exact arithmetic.
//!
//! The layering is deliberate:
//!
//! * [`spec`] reads the list of names chapter 8 promises, so the coverage test
//!   is derived from the document rather than from a hand-written list;
//! * [`stdlib`] embeds the §8.20 module tree (`Math`, `Units.SI`, `Data.Csv`,
//!   `Physics.Ising`, …) so `import Math { sqrt }` resolves: the modules are
//!   sources this crate ships, not a second implementation;
//! * [`linalg`], [`stats`], [`dist`], [`calc`], [`fft`], [`interp`], [`ode`],
//!   [`sde`], [`pde`], [`complex`], [`number`], [`geom`] and [`table`] are the
//!   typed adapters over RSSN numerical kernels and exact/data operations;
//! * `aevia-core` converts between those slices and runtime values.
//!
//! Where a result can be exact it is kept exact: `median` of `[1, 2, 4]` is the
//! rational `2`, and `linalg::det` answers a rational for a rational matrix.
//! The float path is used only when an element is a float or when the algorithm
//! has no exact form (eigenvalues, the SVD).

#![forbid(unsafe_code)]
#![allow(clippy::needless_range_loop, clippy::many_single_char_names)]
// Tuples of factors (LU, QR, SVD, eigen pairs) are returned as-is to match the formulas.
#![allow(clippy::type_complexity)]

pub mod calc;
pub mod complex;
pub mod dist;
pub mod fft;
pub mod geom;
pub mod interp;
pub mod linalg;
pub mod npy;
pub mod number;
pub mod ode;
pub mod pde;
pub mod sde;
pub mod sim;
pub mod sparse;
pub mod spec;
pub mod special;
pub mod stats;
pub mod stdlib;
pub mod table;

/// Why a numeric kernel refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NumericError {
    /// A matrix was expected to be square and was not.
    NotSquare,
    /// The shapes of two operands do not agree.
    ShapeMismatch(String),
    /// The matrix has no inverse / no unique solution.
    Singular,
    /// A matrix that should be positive definite is not.
    NotPositiveDefinite,
    /// An iterative method ran out of iterations.
    NotConverged,
    /// The input is outside the function's domain.
    Domain(String),
    /// The input was empty where a value was required.
    Empty,
}

impl NumericError {
    /// The name of the `LinAlgError`/`MathError` constructor this maps to.
    #[must_use]
    pub fn constructor(&self) -> &'static str {
        match self {
            | Self::NotSquare | Self::ShapeMismatch(_) => "ShapeMismatch",
            | Self::Singular => "Singular",
            | Self::NotPositiveDefinite => "NotPositiveDefinite",
            | Self::NotConverged => "NotConverged",
            | Self::Domain(_) | Self::Empty => "NotConverged",
        }
    }
}

impl std::fmt::Display for NumericError {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            | Self::NotSquare => f.write_str("the matrix is not square"),
            | Self::ShapeMismatch(detail) => write!(f, "shape mismatch: {detail}"),
            | Self::Singular => f.write_str("the matrix is singular"),
            | Self::NotPositiveDefinite => f.write_str("the matrix is not positive definite"),
            | Self::NotConverged => f.write_str("the iteration did not converge"),
            | Self::Domain(detail) => write!(f, "outside the domain: {detail}"),
            | Self::Empty => f.write_str("the input is empty"),
        }
    }
}

/// A complex number, in the same layout `Complex<ρ,d>` has at run time.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Complex {
    /// The real part.
    pub re: f64,
    /// The imaginary part.
    pub im: f64,
}

impl Complex {
    /// A complex number.
    #[must_use]
    pub const fn new(
        re: f64,
        im: f64,
    ) -> Self {
        Self { re, im }
    }

    /// The modulus.
    #[must_use]
    pub fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }

    /// The argument, in `(-π, π]`.
    #[must_use]
    pub fn arg(self) -> f64 {
        self.im.atan2(self.re)
    }

    /// The conjugate.
    #[must_use]
    pub fn conj(self) -> Self {
        Self::new(self.re, -self.im)
    }

    /// `self * other`.
    #[must_use]
    #[allow(clippy::should_implement_trait)] // checked/fallible arithmetic, not `std::ops::Mul`
    pub fn mul(
        self,
        other: Self,
    ) -> Self {
        Self::new(
            self.re * other.re - self.im * other.im,
            self.re * other.im + self.im * other.re,
        )
    }

    /// `self / other`.
    #[must_use]
    #[allow(clippy::should_implement_trait)] // checked/fallible arithmetic, not `std::ops::Div`
    pub fn div(
        self,
        other: Self,
    ) -> Self {
        let denominator = other.re * other.re + other.im * other.im;
        Self::new(
            (self.re * other.re + self.im * other.im) / denominator,
            (self.im * other.re - self.re * other.im) / denominator,
        )
    }

    /// `exp(self)`.
    #[must_use]
    pub fn exp(self) -> Self {
        let scale = self.re.exp();
        Self::new(scale * self.im.cos(), scale * self.im.sin())
    }

    /// `ln(self)`, the principal branch.
    #[must_use]
    pub fn ln(self) -> Self {
        Self::new(self.abs().ln(), self.arg())
    }

    /// The principal square root.
    #[must_use]
    pub fn sqrt(self) -> Self {
        let modulus = self.abs();
        if modulus == 0.0 {
            return Self::default();
        }
        let re = ((modulus + self.re) / 2.0).sqrt();
        let im = ((modulus - self.re) / 2.0).sqrt().copysign(self.im);
        Self::new(re, im)
    }
}
