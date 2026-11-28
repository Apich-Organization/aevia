//! Descriptions of the standard-library names, written for the person calling
//! them from Aevia.
//!
//! Each [`Guide`] says what a name does *in an Aevia program*: what it takes,
//! what comes back, how dimensions and `Result`s behave, and what to use
//! instead in a neighbouring case. It never describes the Rust code behind the
//! name. The REPL's `:doc` prints these, and the generated standard-library
//! modules (`tools/aevia-stdlib-gen`) put them first in each name's `##`
//! documentation.
//!
//! Every example is checked by the CLI tests, which compile it without running
//! it, so a guide cannot drift from the checker's signatures. The table is
//! sorted by name; the unit tests keep it that way.

/// One standard-library name's description and example.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Guide {
    /// The name as written in Aevia source.
    pub name: &'static str,
    /// What the name does, for the Aevia caller.
    pub description: &'static str,
    /// Example lines: one expression or declaration per line. Lines starting
    /// with `:` are REPL commands and are skipped when the example is compiled.
    pub example: &'static str,
}

/// The guide for `name`, if there is one.
#[must_use]
pub fn guide(name: &str) -> Option<&'static Guide> {
    GUIDES
        .binary_search_by_key(&name, |guide| guide.name)
        .ok()
        .and_then(|index| GUIDES.get(index))
}

/// Words that only make sense to someone reading the Rust sources. A guide is
/// for the person calling the name from Aevia, so none of them may appear in a
/// description; the generated standard-library documentation is held to the
/// same list.
const RUST_ISMS: &[&str] = &[
    "&[",
    "&mut",
    "Vec<",
    "usize",
    "u32",
    "i64",
    "f64",
    "f32",
    "::",
    "crate",
    "rustdoc",
    "unwrap()",
    "assert_eq!",
    "impl",
    "struct",
    "pub fn",
    "Box<",
    "Rc<",
];

/// The first Rust-only word or symbol in `text`, if any.
///
/// Words are matched whole, so `construct` does not trip `struct`.
#[must_use]
pub fn find_rust_ism(text: &str) -> Option<&'static str> {
    RUST_ISMS.iter().copied().find(|ism| {
        let word = ism.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        text.match_indices(ism).any(|(at, hit)| {
            !word
                || (!text[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_')
                    && !text
                        .get(at.saturating_add(hit.len())..)
                        .is_some_and(|tail| {
                            tail.starts_with(|c: char| c.is_alphanumeric() || c == '_')
                        }))
        })
    })
}

/// Every guide, sorted by name.
pub const GUIDES: &[Guide] = &[
    Guide {
        name: "AxisScale",
        description: "The scale of a plot axis: Linear or Log. It is a field of PlotOpts.",
        example: "",
    },
    Guide {
        name: "Beta",
        description: "The beta distribution on the interval from 0 to 1 with dimensionless shape parameters alpha and beta.",
        example: "",
    },
    Guide {
        name: "Binomial",
        description: "The binomial distribution: the number of successes in n independent trials that each succeed with probability p.",
        example: "",
    },
    Guide {
        name: "Cauchy",
        description: "The Cauchy distribution with location loc and scale, both in the dimension of the variable. It has no mean or variance.",
        example: "",
    },
    Guide {
        name: "ChiSquared",
        description: "The chi-squared distribution with df degrees of freedom.",
        example: "",
    },
    Guide {
        name: "Distribution",
        description: "A probability distribution, built with one of its constructors, for example Normal({ mu = 0.0, sigma = 1.0 }). Pass it to pdf, cdf, cdf_batch, dist_quantile, sample or sample_n. Parameters carry dimensions, so a Normal of lengths has mu and sigma in metres and its density is per metre.",
        example: "",
    },
    Guide {
        name: "Exponential",
        description: "The exponential distribution with the given rate, whose dimension is the reciprocal of the variable's. Its mean is 1/rate.",
        example: "",
    },
    Guide {
        name: "F",
        description: "The F distribution with df1 and df2 degrees of freedom, used for ratios of variances.",
        example: "",
    },
    Guide {
        name: "Gamma",
        description: "The gamma distribution with a dimensionless shape and a rate whose dimension is the reciprocal of the variable's.",
        example: "",
    },
    Guide {
        name: "Geometric",
        description: "The geometric distribution: the number of failures before the first success, each trial succeeding with probability p. Counts start at 0.",
        example: "",
    },
    Guide {
        name: "LogNormal",
        description: "The log-normal distribution: the variable's logarithm is normal with mean mu and standard deviation sigma, both dimensionless.",
        example: "",
    },
    Guide {
        name: "NegativeBinomial",
        description: "The negative binomial distribution: the number of failures before r successes, each trial succeeding with probability p. Counts start at 0.",
        example: "",
    },
    Guide {
        name: "Norm",
        description: "Chooses how a Fourier transform is scaled: Backward leaves the forward transform unscaled and divides the inverse by the number of points (the default of fft and ifft), Forward divides the forward transform instead, and Unitary divides both by the square root of the number of points so that the transform preserves energy.",
        example: "",
    },
    Guide {
        name: "Normal",
        description: "The normal (Gaussian) distribution with mean mu and standard deviation sigma, both in the dimension of the variable.",
        example: "",
    },
    Guide {
        name: "OdeAdaptiveOpts",
        description: "Options for adaptive stepping: the same fields as OdeOpts plus the starting step h0 (zero lets the solver choose), the smallest and largest steps h_min and h_max, and limits on the number of steps (max_steps, counting rejected attempts) and right-hand-side evaluations (max_evaluations). Running out of either limit is an error.",
        example: "",
    },
    Guide {
        name: "OdeEvent",
        description: "An event for ode_solve: g(t, y) is a dimensionless function whose sign change marks the event, terminal says whether the integration stops there, and direction is -1 to react only to falling crossings, 1 for rising crossings and 0 for both.",
        example: "",
    },
    Guide {
        name: "OdeHit",
        description: "One event that fired during ode_solve: the index of the event in the list you supplied, the time t it happened and the state y at that time.",
        example: "",
    },
    Guide {
        name: "OdeMethod",
        description: "The integration scheme for an ODE solver. Dopri5 is the adaptive default for smooth problems; Rk4 is a fixed-step classical method. Radau, Bdf and Rosenbrock23 are for stiff problems, and AutoSwitch starts with Dopri5 and falls back to Radau. StormerVerlet, Ruth4 and the GaussLegendre and ImplicitMidpoint methods are geometric integrators that preserve the structure of Hamiltonian systems, and the Adaptive* forms vary the step while keeping that method.",
        example: "",
    },
    Guide {
        name: "OdeOpts",
        description: "Options for odeint_with, solve_ivp and ode_solve with a fixed or automatically chosen step: rtol and atol are the relative and absolute error tolerances, method is an OdeMethod, steps is the number of sub-steps for fixed-step methods (or None), and events is an optional function whose sign change stops the integration.",
        example: "",
    },
    Guide {
        name: "OdePhysicalOpts",
        description: "Like OdeAdaptiveOpts, but atol has the same structure and units as the state, so each component gets a tolerance in its own unit instead of a single dimensionless number.",
        example: "",
    },
    Guide {
        name: "OdeSolution",
        description: "The result of ode_solve. It holds the accepted times t and states y, a function at(t) that gives the state at any time in the span, the events that fired (events), the number of right-hand-side evaluations (nfev), the number of rejected steps (rejected) and the name of the method that finished the run.",
        example: "",
    },
    Guide {
        name: "Poisson",
        description: "The Poisson distribution of counts with mean lambda.",
        example: "",
    },
    Guide {
        name: "Precond",
        description: "The preconditioner an iterative sparse solver applies before iterating: Identity (none), Jacobi (diagonal scaling), BlockJacobi (inverted diagonal blocks), Ilu0 (incomplete LU with no fill), Ic0 (incomplete Cholesky for symmetric positive-definite matrices) or Auto (let the solver choose). A good preconditioner can cut the iteration count a great deal.",
        example: "",
    },
    Guide {
        name: "SdeAdaptiveOpts",
        description: "Options for an adaptive-step SDE run: the noise structure, method and seed, the time span t0 to t1, tolerances rtol and atol, a limit max_steps, and cells, the finest noise resolution (0 picks 4096; other values round up to a power of two).",
        example: "",
    },
    Guide {
        name: "SdeEvent",
        description: "An event for sde_events: g(t, y) is a dimensionless function whose sign change marks the event, terminal says whether the path stops there, and direction is -1 for falling crossings, 1 for rising and 0 for both.",
        example: "",
    },
    Guide {
        name: "SdeFixedOpts",
        description: "Options for a fixed-step SDE run: the noise structure, the method, the seed (the same seed always gives the same path), the start time t0, the step dt and the number of steps, and terms, the number of Fourier terms for Levy areas (0 picks a default).",
        example: "",
    },
    Guide {
        name: "SdeHit",
        description: "One event found on an SDE path: the index of the event, the time t it happened and the interpolated state y.",
        example: "",
    },
    Guide {
        name: "SdeMethod",
        description: "The scheme for a stochastic differential equation. The name fixes the stochastic calculus: names containing Stratonovich converge to the Stratonovich solution and the rest to the Ito solution. EulerMaruyama is the simplest (strong order 1/2); Milstein, SrkPlaten and their variants reach strong order 1; DriftImplicitEuler, ThetaMaruyama and DriftImplicitMilstein treat the drift implicitly for stiff drifts.",
        example: "",
    },
    Guide {
        name: "SdeOpts",
        description: "Chooses the stepping of an SDE run: Fixed(options) for a uniform grid or Adaptive(options) for error-controlled steps.",
        example: "",
    },
    Guide {
        name: "SdeStructure",
        description: "How the noise enters the state: Diagonal gives each component its own noise channel, General(m) uses m channels that commute, and GeneralNoncommutative(m) uses m channels that do not commute, which needs extra Levy-area terms in the higher-order schemes.",
        example: "",
    },
    Guide {
        name: "StudentT",
        description: "Student's t distribution with df degrees of freedom.",
        example: "",
    },
    Guide {
        name: "Table",
        description: "A table with named columns and a row type parameter. Table<{ time: Float }> provides a static row type for operations such as row. For a table read from a file, the header supplies the actual column names and optional unit metadata. Use schema to inspect that metadata and column_dyn to read cells whose types are determined at run time.",
        example: "",
    },
    Guide {
        name: "Uniform",
        description: "The continuous uniform distribution on the interval from lo to hi, both in the dimension of the variable.",
        example: "",
    },
    Guide {
        name: "Weibull",
        description: "The Weibull distribution with a dimensionless shape and a scale in the dimension of the variable.",
        example: "",
    },
    Guide {
        name: "abs",
        description: "Absolute value; the dimension is unchanged, so abs(-3 m) is 3 m. For a Complex number use magnitude.",
        example: "abs(-3)\nabs(-2.5 m)",
    },
    Guide {
        name: "acos",
        description: "Inverse cosine in radians. The argument must be dimensionless and in [-1, 1]; outside that range it is a runtime error (E0806).",
        example: "acos(0.5)",
    },
    Guide {
        name: "acosh",
        description: "Inverse hyperbolic cosine. The argument must be at least 1; smaller arguments are a runtime error (E0806).",
        example: "acosh(2.0)",
    },
    Guide {
        name: "all",
        description: "True if the predicate holds for every element; true for an empty tensor.",
        example: "all(fn x => x > 0, [1, 2, 3])",
    },
    Guide {
        name: "and_then",
        description: "Chains a step that may itself fail: on Some(x) or Ok(x) it applies f to x and returns that result, otherwise it passes None or the Err through unchanged. Unlike map, f returns an Option or Result of its own.",
        example: "and_then(fn x => if x > 0 then Some(x) else None, Some(3))",
    },
    Guide {
        name: "antiderivative",
        description: "antiderivative(f) returns a symbolic antiderivative as a function, with the constant of integration taken as zero. The result dimension is output times input. When no closed form exists the call fails; use defint or quadrature for a definite integral.",
        example: "antiderivative(fn x => 2 * x)(3)",
    },
    Guide {
        name: "any",
        description: "True if the predicate holds for at least one element; false for an empty tensor.",
        example: "any(fn x => x > 2, [1, 2, 3])",
    },
    Guide {
        name: "apart",
        description: "apart(expr, x) splits a rational expression in x into partial fractions, which is useful before integrating term by term.",
        example: "let x = symbol(\"x\")\napart(1 / (x * (x + 1)), x)",
    },
    Guide {
        name: "apply",
        description: "apply(f, x) calls f on x. It is mainly useful for passing function application itself as a value.",
        example: "apply(fn x => x + 1, 4)",
    },
    Guide {
        name: "approx",
        description: "Converts an exact expression to a Float, keeping its dimension. It returns a Result because an expression that still contains free symbols has no numeric value; unwrap it with unwrap_or or handle the Err.",
        example: "approx(pi)",
    },
    Guide {
        name: "approx_eq",
        description: "approx_eq(a, b, tol) is true when a and b differ by at most the absolute tolerance tol. All three must have the same dimension, so approx_eq(1.0 m, 1.001 m, 0.01 m) is the right way to compare lengths.",
        example: "approx_eq(0.1 + 0.2, 0.3, 0.000001)",
    },
    Guide {
        name: "approx_map",
        description: "Applies approx to every element of a vector of exact values, returning a Result of a Float vector. It fails if any element still contains a free symbol.",
        example: "approx_map(fn x => x, [1 / 3, 2 / 3])",
    },
    Guide {
        name: "arg",
        description: "The phase angle of a complex number in radians, in (-pi, pi].",
        example: "arg(complex(0, 1))",
    },
    Guide {
        name: "argmax",
        description: "The index of the first largest element of a vector. NaN never wins; an empty vector is a runtime error.",
        example: "argmax([1, 5, 2])",
    },
    Guide {
        name: "argmin",
        description: "The index of the first smallest element of a vector. NaN never wins; an empty vector is a runtime error.",
        example: "argmin([4, 1, 7])",
    },
    Guide {
        name: "args",
        description: "Return the program arguments supplied after -- on the command line. In an interactive session the list is normally empty.",
        example: "args()",
    },
    Guide {
        name: "argsort",
        description: "The positions that would sort the data: indexing the data with them gives it in ascending order. The sort is stable.",
        example: "argsort([30, 10, 20])",
    },
    Guide {
        name: "asin",
        description: "Inverse sine in radians. The argument must be dimensionless and in [-1, 1]; outside that range it is a runtime error (E0806).",
        example: "asin(1.0)",
    },
    Guide {
        name: "asinh",
        description: "Inverse hyperbolic sine of a dimensionless argument. Defined for every real input.",
        example: "asinh(1.0)",
    },
    Guide {
        name: "assert",
        description: "Fails the program with a diagnostic when the Bool is false, and does nothing when it is true. It is an IO action, so call it inside a do block or at the top level of a test.",
        example: "assert(1 + 1 == 2)",
    },
    Guide {
        name: "assert_eq",
        description: "Fails with both values shown when the two arguments are not equal. It is an IO action. For Floats compare with approx_eq instead, since rounding errors make == unreliable.",
        example: "assert_eq(2 + 2, 4)",
    },
    Guide {
        name: "assuming",
        description: "assuming(p, thunk) runs the zero-argument function thunk with the proposition p taken as true, so symbolic simplifications inside it may use it. Only simple predicates such as x > 0 are supported.",
        example: "assuming(sym_lt(0, 1), fn () => 1)",
    },
    Guide {
        name: "at",
        description: "at(t, i) is the i-th row of a tensor (an element for a vector), counting from 0. A negative index counts from the end, and an out-of-range index is a runtime error; t[i] is the same operation.",
        example: "at([10, 20, 30], 1)",
    },
    Guide {
        name: "atan",
        description: "Inverse tangent in radians, in (-pi/2, pi/2). For the angle of a point (x, y) use atan2, which also knows the quadrant.",
        example: "atan(1.0)",
    },
    Guide {
        name: "atan2",
        description: "The angle, in radians in (-pi, pi], of the point (x, y) measured from the positive x axis; note the order atan2(y, x). Both arguments must have the same dimension, so lengths work directly and the result is dimensionless.",
        example: "atan2(1.0, -1.0)\natan2(2.0 m, 2.0 m)",
    },
    Guide {
        name: "atanh",
        description: "Inverse hyperbolic tangent. The argument must lie between -1 and 1; beyond that range it is a runtime error (E0806), and at exactly 1 or -1 a Float gives an infinity.",
        example: "atanh(0.5)",
    },
    Guide {
        name: "await",
        description: "Wait for a Task and obtain its result once. Ctrl-C cancels the current wait; background tasks cannot read the REPL terminal.",
        example: "let example = do\n    task <- spawn(fn () => pure(42))\n    await(task)\nexample",
    },
    Guide {
        name: "base",
        description: "Converts a quantity to its coherent base-unit representation: base(150 cm) is 1.5 m. Useful for comparing or printing mixed units.",
        example: "base(150 cm)",
    },
    Guide {
        name: "bessel_j0",
        description: "Bessel function of the first kind of order 0, J0(x).",
        example: "bessel_j0(1.0)",
    },
    Guide {
        name: "bessel_j1",
        description: "Bessel function of the first kind of order 1, J1(x). j1 is a shorter name for the same function.",
        example: "bessel_j1(1.0)",
    },
    Guide {
        name: "bessel_jn",
        description: "bessel_jn(n, x) is the Bessel function of the first kind of integer order n.",
        example: "bessel_jn(2, 1.0)",
    },
    Guide {
        name: "bessel_y0",
        description: "Bessel function of the second kind of order 0, Y0(x), for x > 0.",
        example: "bessel_y0(1.0)",
    },
    Guide {
        name: "bessel_y1",
        description: "Bessel function of the second kind of order 1, Y1(x), for x > 0. y1 is a shorter name for the same function.",
        example: "bessel_y1(1.0)",
    },
    Guide {
        name: "bessel_yn",
        description: "bessel_yn(n, x) is the Bessel function of the second kind of integer order n, for x > 0.",
        example: "bessel_yn(2, 1.0)",
    },
    Guide {
        name: "beta",
        description: "The beta function B(a, b) = gamma(a) gamma(b) / gamma(a + b) of two dimensionless numbers.",
        example: "beta(2.0, 3.0)",
    },
    Guide {
        name: "bicgstab_with",
        description: "BiCGSTAB iterative solver for general sparse systems: bicgstab_with(a, b, { tol, maxiter, precond }). It needs little memory per iteration. Check the converged field of the result.",
        example: "import LinearAlgebra.Sparse { Jacobi }\nbicgstab_with(speye(3), [1.0, 2.0, 3.0], { tol = 0.000001, maxiter = 100, precond = Jacobi })",
    },
    Guide {
        name: "binomial",
        description: "binomial(n, k) is the number of ways to choose k items from n, as an exact Int. It is 0 when k > n.",
        example: "binomial(5, 2)",
    },
    Guide {
        name: "block_size",
        description: "The (rows, columns) of the dense blocks of a block-compressed sparse tensor.",
        example: "block_size(to_bcsr(speye(4), 2, 2))",
    },
    Guide {
        name: "by_cases",
        description: "by_cases(p, yes, no) picks yes() or no() when p can be decided, and otherwise builds a symbolic if-then-else expression. Use it for piecewise definitions in symbolic code.",
        example: "by_cases(sym_lt(1, 2), fn () => 10, fn () => 20)",
    },
    Guide {
        name: "cancel",
        description: "Cancels common factors between numerator and denominator of a rational expression so the fraction is in lowest terms.",
        example: "let x = symbol(\"x\")\ncancel((x^2 - 1) / (x - 1))",
    },
    Guide {
        name: "cbrt",
        description: "Cube root of an Int, Real or Float. The result dimension is the argument's dimension to the power 1/3, so cbrt(8 m^3) is 2 m. Negative arguments give the real cube root.",
        example: "cbrt(27.0)\ncbrt(8 m^3)",
    },
    Guide {
        name: "cdf",
        description: "cdf(dist, x) is the probability that a draw from the distribution is at most x: a dimensionless number between 0 and 1.",
        example: "import Stats.Distributions { Normal }\ncdf(Normal({ mu = 0.0, sigma = 1.0 }), 0.0)",
    },
    Guide {
        name: "cdf_batch",
        description: "Evaluates cumulative probabilities for a vector of points in the distribution's dimension. The results are dimensionless and retain point order and repetitions. A valid distribution and an empty input give an empty result. Invalid parameters or a NaN point raise a domain error.",
        example: "import Stats.Distributions { cdf_batch, Normal }\ncdf_batch(Normal({ mu = 0.0, sigma = 1.0 }), [-1.0, 0.0, 1.0])",
    },
    Guide {
        name: "ceil",
        description: "Rounds up to the nearest whole number, keeping the type and dimension: ceil(-2.2) is -2.",
        example: "ceil(2.1)",
    },
    Guide {
        name: "centroid",
        description: "The mean point of a vector of Points.",
        example: "centroid([point(0.0, degC), point(10.0, degC)])",
    },
    Guide {
        name: "cg",
        description: "Conjugate-gradient solver for a symmetric positive-definite sparse system: cg(a, b, tol, max_iterations). Returns a record with x, the iteration count, the final residual and whether it converged, so check converged before trusting x. For non-symmetric systems use gmres_with.",
        example: "cg(speye(3), [1.0, 2.0, 3.0], 0.000001, 100)",
    },
    Guide {
        name: "cg_with",
        description: "Preconditioned conjugate gradients: cg_with(a, b, { tol, maxiter, precond }), where precond is one of Identity, Jacobi, BlockJacobi, Ilu0, Ic0 or Auto. The result record also names the preconditioner it used.",
        example: "import LinearAlgebra.Sparse { Jacobi }\ncg_with(speye(3), [1.0, 2.0, 3.0], { tol = 0.000001, maxiter = 100, precond = Jacobi })",
    },
    Guide {
        name: "chars",
        description: "Splits a string into a vector of its characters (grapheme clusters), one String each.",
        example: "chars(\"abc\")",
    },
    Guide {
        name: "chebyshev_fit",
        description: "Least-squares fit of a Chebyshev series of the given degree to the points (xs, ys), which is better conditioned than polyfit for high degrees. Returns the coefficients.",
        example: "chebyshev_fit([0.0, 0.5, 1.0], [1.0, 2.0, 5.0], 2)",
    },
    Guide {
        name: "chebyshev_t",
        description: "chebyshev_t(n, x) is the Chebyshev polynomial of the first kind T_n(x); on [-1, 1] it equals cos(n acos x).",
        example: "chebyshev_t(3, 0.5)",
    },
    Guide {
        name: "chebyshev_u",
        description: "chebyshev_u(n, x) is the Chebyshev polynomial of the second kind U_n(x).",
        example: "chebyshev_u(3, 0.5)",
    },
    Guide {
        name: "check",
        description: "Check a Dyn quantity against a requested static dimension and return Ok(value) or Err(error).",
        example: "check<L>(dyn(3 m))",
    },
    Guide {
        name: "cholesky",
        description: "Cholesky factor L of a symmetric positive-definite matrix, A = L L^T. Returns Err(LinAlgError) when the matrix is not positive definite.",
        example: "cholesky([[4.0, 2.0], [2.0, 3.0]])",
    },
    Guide {
        name: "clamp",
        description: "clamp(x, lo, hi) limits x to the interval [lo, hi]. All three arguments must have the same dimension.",
        example: "clamp(15, 0, 10)\nclamp(-1.5 m, 0 m, 2 m)",
    },
    Guide {
        name: "collect",
        description: "collect(expr, x) groups the terms of expr by powers of x.",
        example: "let x = symbol(\"x\")\nlet a = symbol(\"a\")\nlet b = symbol(\"b\")\ncollect(a * x + b * x + x^2, x)",
    },
    Guide {
        name: "column",
        description: "Looks up a column by name at run time and returns its cells in a rank-one tensor. The surrounding expression supplies the expected element type independently of the table’s row type. Numeric cells are dimensionless Float values and other cells are String values. A missing column returns Err(DataError). Use column_dyn when the cells’ types are determined at run time.",
        example: "fn readings(t: Table<{ time: Float }>, name: String): Result<Tensor<1, Float>, DataError> = column(t, name)",
    },
    Guide {
        name: "column_dyn",
        description: "Looks up a column by name at run time and returns a rank-one tensor of dynamic Value cells. Numeric cells are dimensionless Float values and other cells are String values. This works with any table row type. A missing column returns Err(DataError).",
        example: "fn pick(t: Table<Dynamic>, name: String): Result<Tensor<1, Value>, DataError> = column_dyn(t, name)",
    },
    Guide {
        name: "combine",
        description: "Collects a vector of series in one plot specification. The renderer uses shared axes and checks dimensions and explicit axis settings.",
        example: "combine([line([0.0, 1.0], [0.0, 1.0]), scatter([0.0, 1.0], [1.0, 0.0])])",
    },
    Guide {
        name: "compare",
        description: "Compares two values, returning Some(Less), Some(Equal) or Some(Greater), or None when they cannot be ordered (for example a NaN). For a comparison that never fails use total_compare.",
        example: "compare(1, 2)",
    },
    Guide {
        name: "compile_fail",
        description: "Takes Aevia source as a string and returns true if it does NOT compile. Use it in tests to check that the type checker rejects a unit mismatch or other mistake; the source is compiled on its own, with only the Prelude in scope.",
        example: "compile_fail(\"let x = 1 m + 1 s\")",
    },
    Guide {
        name: "complex",
        description: "Builds a complex number from real and imaginary parts with a common dimension.",
        example: "complex(3 m, 400 cm)",
    },
    Guide {
        name: "complex_cos",
        description: "The cosine of a dimensionless Complex number.",
        example: "complex_cos(complex(0, 1))",
    },
    Guide {
        name: "complex_exp",
        description: "The complex exponential e^z for a dimensionless Complex z.",
        example: "complex_exp(complex(0, 1))",
    },
    Guide {
        name: "complex_ln",
        description: "The principal natural logarithm of a dimensionless Complex number; the imaginary part lies in (-pi, pi].",
        example: "complex_ln(complex(1, 1))",
    },
    Guide {
        name: "complex_sin",
        description: "The sine of a dimensionless Complex number.",
        example: "complex_sin(complex(0, 1))",
    },
    Guide {
        name: "complex_sqrt",
        description: "Principal square root of a complex number. The dimension is raised to the power 1/2.",
        example: "complex_sqrt(complex(-4, 0))",
    },
    Guide {
        name: "compose",
        description: "compose(f, g) is the function x => f(g(x)): g runs first. For left-to-right pipelines write x |> g |> f instead.",
        example: "compose(fn x => x + 1, fn x => x * 2)(5)",
    },
    Guide {
        name: "concat",
        description: "Concatenate two strings or two compatible rank-1 collections. String interpolation is useful when combining labels and values.",
        example: "concat(\"hello \", \"world\")",
    },
    Guide {
        name: "conj",
        description: "The complex conjugate: the imaginary part changes sign.",
        example: "conj(complex(1, 2))",
    },
    Guide {
        name: "const",
        description: "const(a, b) returns a and ignores b. const(a) cannot be partially applied this way; use it as a two-argument function, for instance when you need a function that ignores its input.",
        example: "const(1, 2)",
    },
    Guide {
        name: "contains",
        description: "contains(s, part) is true when part occurs anywhere inside s.",
        example: "contains(\"hello\", \"ell\")",
    },
    Guide {
        name: "contract",
        description: "Contracts two tensors over chosen axes, like tensordot: contract(a, b, axes_a, axes_b) sums products over the paired axes. The axis lists must be written as literal lists. Two sparse arguments give a sparse result; one sparse gives a dense one.",
        example: "contract(speye(2), speye(2), [1], [0])",
    },
    Guide {
        name: "convert",
        description: "Changes the storage format of a sparse tensor, named as a type argument: convert<Coo>(a). The contents do not change; only how they are laid out.",
        example: "convert<Coo>(speye(3))",
    },
    Guide {
        name: "corr",
        description: "Pearson correlation coefficient of two vectors of equal length, a dimensionless number between -1 and 1.",
        example: "corr([1.0, 2.0, 3.0], [2.0, 4.0, 7.0])",
    },
    Guide {
        name: "cos",
        description: "Trigonometric function. Its argument must be dimensionless; angle units such as deg carry an exact conversion scale.",
        example: "sin(30 deg)\ncos(0.0)",
    },
    Guide {
        name: "cosh",
        description: "Hyperbolic cosine of a dimensionless argument.",
        example: "cosh(0.0)",
    },
    Guide {
        name: "cov",
        description: "Sample covariance of two vectors of equal length. The result's dimension is the product of the two element dimensions.",
        example: "cov([1.0, 2.0, 3.0], [2.0, 4.0, 7.0])",
    },
    Guide {
        name: "cumprod",
        description: "Running products of a dimensionless vector: [1, 2, 3] becomes [1, 2, 6].",
        example: "cumprod([1, 2, 3])",
    },
    Guide {
        name: "cumsum",
        description: "Running totals of a vector: [1, 2, 3] becomes [1, 3, 6]. The dimension is kept.",
        example: "cumsum([1, 2, 3])",
    },
    Guide {
        name: "curl",
        description: "The curl of a three-component vector field at a point. The coordinates must have a length dimension, and the result has the field's dimension per length.",
        example: "curl(fn v => [v[1], -v[0], 0], [1, 2, 3])",
    },
    Guide {
        name: "curry",
        description: "curry(f) turns a two-argument function into a function that takes one argument and returns a function waiting for the other.",
        example: "curry(fn (a, b) => a + b)(1)(2)",
    },
    Guide {
        name: "decide",
        description: "Decides whether a symbolic proposition is true. It returns Some(true) or Some(false) when the kernel can settle it and None when it cannot. Plain Bool values are always settled.",
        example: "decide(sym_lt(1, 2))",
    },
    Guide {
        name: "default_plot_opts",
        description: "A plot options record with linear axes and automatic limits. Use a record update to set labels, color, display units, scales or limits for a *_with constructor.",
        example: "{ default_plot_opts with color = Some(\"blue\"), xlim = Some((0.0, 10.0)) }",
    },
    Guide {
        name: "defint",
        description: "defint(f, a, b) is the exact definite integral of f from a to b, as a Result: Err(MathError) when the integrand has no closed form on the interval. Bounds may be oo. For a numeric integral of a Float function use quadrature.",
        example: "defint(fn x => x^2, 0, 3)",
    },
    Guide {
        name: "degree",
        description: "degree(poly, x) is the degree of a polynomial in the variable x, as an Int.",
        example: "let x = symbol(\"x\")\ndegree(x^3 + 2 * x, x)",
    },
    Guide {
        name: "delta_to",
        description: "delta_to(unit, d) shows a temperature difference d in an affine unit such as degC using the unit's scale only and ignoring its offset, so a 10 K rise is a 10 degC rise rather than -263.15 degC. Affine units otherwise accept only points built with point.",
        example: "delta_to(degC, 10.0 K)",
    },
    Guide {
        name: "density",
        description: "The fraction of entries of a sparse tensor that are stored, between 0 and 1.",
        example: "density(speye(4))",
    },
    Guide {
        name: "det",
        description: "The determinant of a square, dimensionless matrix. For a matrix with units use det_unit, because the determinant's dimension depends on the matrix size.",
        example: "det([[1.0, 2.0], [3.0, 4.0]])",
    },
    Guide {
        name: "det_unit",
        description: "Determinant of a square matrix whose entries carry a unit. Because the determinant's dimension is the entry dimension to the power n, you supply the unit of the entries and the result is a dynamically typed quantity.",
        example: "det_unit([[1.0 m, 2.0 m], [3.0 m, 4.0 m]], m)",
    },
    Guide {
        name: "device_of",
        description: "The device a matrix currently lives on.",
        example: "device_of([[1.0, 2.0], [3.0, 4.0]])",
    },
    Guide {
        name: "diag",
        description: "On a vector, builds the square matrix with that vector on its diagonal; on a matrix, extracts the diagonal as a vector.",
        example: "diag([1.0, 2.0, 3.0])\ndiag([[1, 2], [3, 4]])",
    },
    Guide {
        name: "diff",
        description: "diff(f) returns the symbolic derivative of f as a new function; differentiate at a point by calling the result. The derivative's dimension is that of the output divided by the input, so differentiating a position in m with respect to a time in s gives m/s. Use numeric_deriv for functions you can only evaluate numerically.",
        example: "diff(fn t => t^3)(2)",
    },
    Guide {
        name: "diff_at",
        description: "diff_at(f, x) is the derivative of f evaluated at x; it is shorthand for diff(f)(x).",
        example: "diff_at(fn x => x^3, 2)",
    },
    Guide {
        name: "difference",
        description: "The elements of the first set that are not in the second.",
        example: "difference(set_of([1, 2, 3]), set_of([2]))",
    },
    Guide {
        name: "dist_quantile",
        description: "Returns the quantile of a distribution at a dimensionless probability in [0, 1]. The result has the distribution's dimension. At probabilities 0 and 1 it returns the lower and upper support endpoints, which can be infinite. Invalid parameters and probabilities raise a domain error.",
        example: "import Stats.Distributions { Normal, dist_quantile }\ndist_quantile(Normal({ mu = 2.0 m, sigma = 0.5 m }), 0.975)",
    },
    Guide {
        name: "distance",
        description: "Euclidean distance between two points given as vectors of equal length, in the coordinates' dimension.",
        example: "distance([0.0 m, 0.0 m], [3.0 m, 4.0 m])",
    },
    Guide {
        name: "div",
        description: "Floored integer division: div(-7, 2) is -4. It pairs with mod, so a == div(a, b) * b + mod(a, b). Compare quot, which rounds toward zero. The / operator on Ints gives an exact Real instead.",
        example: "div(-7, 2)",
    },
    Guide {
        name: "divergence",
        description: "The divergence of a vector field at a point: the sum of the partial derivative of each component with respect to its own coordinate.",
        example: "divergence(fn v => [v[0]^2, v[1]^2], [1, 2])",
    },
    Guide {
        name: "dot",
        description: "The inner product of two vectors of equal length. The result dimension is the product of the element dimensions, so the dot product of a force and a displacement is an energy.",
        example: "dot([1.0, 2.0, 3.0], [4.0, 5.0, 6.0])",
    },
    Guide {
        name: "double_factorial",
        description: "The double factorial n!! = n (n-2) (n-4) …, for a non-negative Int: double_factorial(7) is 105.",
        example: "double_factorial(7)",
    },
    Guide {
        name: "drop",
        description: "drop(n, xs) removes the first n elements, giving an empty tensor if there are fewer. On a matrix it removes rows.",
        example: "drop(1, [1, 2, 3])",
    },
    Guide {
        name: "dyn",
        description: "Erase the static dimension of a quantity into a runtime-checked Dyn value. Use check<dimension> to recover a static dimension safely.",
        example: "dyn(3 m)\ncheck<L>(dyn(3 m))",
    },
    Guide {
        name: "e",
        description: "Euler's number 2.718…, as an exact symbolic Real constant. It stays exact until you ask for a Float with approx, so exp(1) and e are the same value.",
        example: "approx(e)",
    },
    Guide {
        name: "eig",
        description: "Eigen-decomposition of a square matrix. Returns a pair: the vector of eigenvalues and a matrix whose columns are the matching eigenvectors. Both are Complex, since a real matrix can have complex eigenvalues. Use eigvals when you only need the values.",
        example: "eig([[2.0, 0.0], [0.0, 3.0]])",
    },
    Guide {
        name: "eigvals",
        description: "The eigenvalues of a square matrix, as a vector of Complex numbers that keep the matrix's dimension.",
        example: "eigvals([[2.0, 0.0], [0.0, 3.0]])",
    },
    Guide {
        name: "elements",
        description: "The members of a set as a vector, in ascending order.",
        example: "elements(set_of([3, 1, 2]))",
    },
    Guide {
        name: "ends_with",
        description: "ends_with(s, suffix) is true when s ends with suffix.",
        example: "ends_with(\"hello\", \"lo\")",
    },
    Guide {
        name: "enumerate",
        description: "Pairs each element with its position as a record { index, value }, starting at 0.",
        example: "enumerate([5, 6, 7])",
    },
    Guide {
        name: "eprintln",
        description: "Print a value followed by a newline to standard error. Useful for progress and diagnostics while standard output carries results.",
        example: "eprintln(\"checking inputs\")",
    },
    Guide {
        name: "erf",
        description: "The error function, 2/sqrt(pi) times the integral of exp(-t^2) from 0 to x. It maps the reals onto (-1, 1) and is the building block of the normal CDF.",
        example: "erf(1.0)",
    },
    Guide {
        name: "erfc",
        description: "The complementary error function, 1 - erf(x). Use it for large x, where 1 - erf(x) loses all its digits to cancellation.",
        example: "erfc(3.0)",
    },
    Guide {
        name: "errorbar",
        description: "Draws central points and vertical whiskers using nonnegative error magnitudes. The three vectors have equal lengths, and the error has the same dimension as y. Non-finite errors omit the corresponding whisker.",
        example: "errorbar([0.0, 1.0, 2.0], [1.0, 2.0, 3.0], [0.1, 0.2, 0.1])",
    },
    Guide {
        name: "errorbar_with",
        description: "Like errorbar, with a PlotOpts record for label, colour, units, scales and limits.",
        example: "errorbar_with([0.0, 1.0], [1.0, 2.0], [0.1, 0.2], default_plot_opts)",
    },
    Guide {
        name: "eval",
        description: "Evaluates an expression: it substitutes known values and folds constants, giving the exact result where one exists. Use approx to turn the result into a Float.",
        example: "eval(2 + 3)",
    },
    Guide {
        name: "exp",
        description: "Exponential or logarithmic function on a dimensionless input. Domain failures and unresolved symbolic values are distinct from a numerical approximation.",
        example: "exp(1)\nln(2.0)",
    },
    Guide {
        name: "expand",
        description: "Multiplies out products and powers into a sum of terms: (x + 1)^2 becomes x^2 + 2x + 1. Use it before reading off polynomial coefficients.",
        example: "let x = symbol(\"x\")\nexpand((x + 1)^2)",
    },
    Guide {
        name: "eye",
        description: "The n x n identity matrix as Float. Use it as a starting matrix or to check inv(A) * A.",
        example: "eye(3)",
    },
    Guide {
        name: "factor",
        description: "On an Int, returns its prime factorisation as a vector of (prime, exponent) pairs: factor(12) is [(2, 2), (3, 1)]. On an exact expression, factors it over the rationals.",
        example: "factor(12)\nlet x = symbol(\"x\")\nfactor(x^2 - 1)",
    },
    Guide {
        name: "factorial",
        description: "n! for a non-negative Int, computed exactly with no overflow: factorial(25) has all its digits. For non-integers use gamma.",
        example: "factorial(5)",
    },
    Guide {
        name: "fft",
        description: "Discrete Fourier transform of a vector. Real or integer input is treated as complex. The result is not normalised; ifft divides by the length, so ifft(fft(x)) recovers x. The output keeps the input's dimension.",
        example: "fft([1.0, 0.0, -1.0, 0.0])",
    },
    Guide {
        name: "fft2",
        description: "Two-dimensional Fourier transform over the last two axes of a tensor.",
        example: "fft2([[1.0, 2.0], [3.0, 4.0]])",
    },
    Guide {
        name: "fft_axis",
        description: "Fourier transform along a single axis of a tensor; axes count from 0 and negative values count from the end.",
        example: "fft_axis([[1.0, 2.0], [3.0, 4.0]], 1)",
    },
    Guide {
        name: "fft_norm",
        description: "Like fft, with the scaling convention chosen by the second argument: Backward (none going forward, 1/n back), Forward (1/n going forward) or Unitary (1/sqrt(n) both ways).",
        example: "import FFT { Unitary }\nfft_norm([1.0, 2.0, 3.0, 4.0], Unitary)",
    },
    Guide {
        name: "fftfreq",
        description: "fftfreq(n, dt) lists the frequencies matching an n-point fft of samples taken dt apart, in the order fft returns them. The dimension is the reciprocal of dt, so a spacing in seconds gives hertz.",
        example: "fftfreq(4, 0.5 s)",
    },
    Guide {
        name: "fftn",
        description: "Fourier transform over every axis of a tensor.",
        example: "fftn([[1.0, 2.0], [3.0, 4.0]])",
    },
    Guide {
        name: "fftn_axes",
        description: "Fourier transform over a chosen list of axes, such as [0, 2]. An out-of-range or repeated axis is a runtime error.",
        example: "fftn_axes([[1.0, 2.0], [3.0, 4.0]], [0, 1])",
    },
    Guide {
        name: "fftn_norm",
        description: "Fourier transform over every axis with a chosen normalisation (Backward, Forward or Unitary).",
        example: "import FFT { Unitary }\nfftn_norm([[1.0, 2.0], [3.0, 4.0]], Unitary)",
    },
    Guide {
        name: "fftshift",
        description: "Rotates every axis of a spectrum so the zero frequency sits in the middle, which is how spectra are usually plotted. ifftshift undoes it.",
        example: "fftshift([0, 1, 2, 3])",
    },
    Guide {
        name: "fftshift_axis",
        description: "fftshift applied to one axis only.",
        example: "fftshift_axis([[0, 1, 2, 3]], 1)",
    },
    Guide {
        name: "fill",
        description: "fill(value, n1, n2, ...) builds a tensor of the given shape with every element equal to value, keeping its type and dimension: fill(2.0 m, 3) is three lengths of 2 m.",
        example: "fill(1.5, 2, 3)\nfill(2.0 m, 3)",
    },
    Guide {
        name: "filter",
        description: "Keeps the elements for which the predicate is true, in order. On a matrix the predicate sees one row at a time; on a Table it sees one row record.",
        example: "filter(fn x => x > 1, [1, 2, 3])",
    },
    Guide {
        name: "find",
        description: "Returns Some of the first element satisfying the predicate, or None when there is none.",
        example: "find(fn x => x > 1, [1, 2, 3])",
    },
    Guide {
        name: "first",
        description: "The first element as an Option, None for an empty tensor. On a matrix it is the first row.",
        example: "first([1, 2, 3])",
    },
    Guide {
        name: "flatten",
        description: "Collapses a tensor of rank 2 or more into a vector, reading the elements row by row. To change the shape instead of dropping it use reshape.",
        example: "flatten([[1, 2], [3, 4]])",
    },
    Guide {
        name: "flip",
        description: "flip(f) returns f with its two arguments swapped.",
        example: "flip(fn (a, b) => a - b)(1, 10)",
    },
    Guide {
        name: "floor",
        description: "Rounds down to the nearest whole number, keeping the type and dimension: floor(-2.7) is -3.",
        example: "floor(2.7)",
    },
    Guide {
        name: "fold",
        description: "Combine a collection with an initial accumulator, from left to right. The combining function receives the accumulator and the next element.",
        example: "fold(fn (a, x) => a + x, 0, [1, 2, 3])",
    },
    Guide {
        name: "fold_left",
        description: "Combine a collection with an initial accumulator, from left to right. The combining function receives the accumulator and the next element.",
        example: "fold(fn (a, x) => a + x, 0, [1, 2, 3])",
    },
    Guide {
        name: "forall",
        description: "Checks a proposition over fixed numeric samples. Use a unary predicate taking a dimensionless Real. The samples are 0, 1, -1, 2, -3, and 1/2. A false verdict returns false. If no verdict is false but a sample is undecidable, the call raises E0804.",
        example: "forall(fn (x: Real) => x + 0 == x)",
    },
    Guide {
        name: "format",
        description: "Build formatted text from a template and arguments. The template uses positional or sequential placeholders.",
        example: "format(r\"answer: {}\", [\"42\"])",
    },
    Guide {
        name: "format_of",
        description: "The storage format of a sparse tensor as a string: \"Csr\", \"Csc\", \"Coo\" or \"Bcsr\".",
        example: "format_of(speye(3))",
    },
    Guide {
        name: "fresnel_c",
        description: "Fresnel cosine integral C(x), the integral of cos(pi t^2 / 2) from 0 to x. Exact arguments stay symbolic; a Float argument gives a Float.",
        example: "fresnel_c(1.0)",
    },
    Guide {
        name: "fresnel_s",
        description: "Fresnel sine integral S(x), the integral of sin(pi t^2 / 2) from 0 to x. Exact arguments stay symbolic so you can differentiate or integrate them; a Float argument gives a Float.",
        example: "fresnel_s(1.0)",
    },
    Guide {
        name: "from_dense",
        description: "Converts a dense tensor to a sparse one, storing only the non-zero entries. The storage format is a type argument: from_dense<Coo>(a), from_dense<Csc>(a) or from_dense<Bcsr>(a); with none it uses Csr.",
        example: "from_dense([[1.0, 0.0], [0.0, 2.0]])",
    },
    Guide {
        name: "from_json",
        description: "Decodes a Json value into the type you ask for, returning Err(SimError) when the JSON does not have that shape. State the target type with an annotation.",
        example: "let back: Result<Int, SimError> = from_json(to_json(3))",
    },
    Guide {
        name: "from_system",
        description: "from_system<D>(name, y) is the inverse of to_system: it reads y as a value in the named unit system and returns an ordinary quantity of dimension D. You give D as a type argument because the reduction forgets which dimension the value came from.",
        example: "from_system<L>(\"natural\", to_system(\"natural\", 1.0 m))",
    },
    Guide {
        name: "from_value",
        description: "Converts a dynamic Value back to a concrete Aevia type, returning Err(OpError) if the value does not have the shape you asked for. The target type comes from the surrounding annotation.",
        example: "let n: Result<Int, OpError> = from_value(to_value(3))",
    },
    Guide {
        name: "gamma",
        description: "The gamma function, which extends factorial: gamma(n) == factorial(n - 1) for positive integers. The argument must be dimensionless. At zero and the negative integers it has poles, where a Float result is an infinity or an enormous number.",
        example: "gamma(5.0)",
    },
    Guide {
        name: "gcd",
        description: "Greatest common divisor of two Ints; the result is never negative, and gcd(0, 0) is 0.",
        example: "gcd(12, 18)",
    },
    Guide {
        name: "get",
        description: "Reads one entry of a sparse tensor by index: get(a, i, j) for a matrix. Positions that are not stored read as zero.",
        example: "get(speye(3), 1, 1)",
    },
    Guide {
        name: "gmres_with",
        description: "Restarted GMRES for general (non-symmetric) sparse systems: gmres_with(a, b, { tol, maxiter, restart, precond }). Floating-point elements only. Check the converged field of the result.",
        example: "import LinearAlgebra.Sparse { Jacobi }\ngmres_with(speye(3), [1.0, 2.0, 3.0], { tol = 0.000001, maxiter = 100, restart = 20, precond = Jacobi })",
    },
    Guide {
        name: "grad",
        description: "grad(f, x) is the gradient of a scalar function of a vector, evaluated at x: the vector of partial derivatives. Each component has the dimension of the output over the input. For Float functions use grad_f.",
        example: "grad(fn v => v[0]^2 + v[1]^2, [1, 2])",
    },
    Guide {
        name: "grad_f",
        description: "Gradient of a Float scalar function of a vector at a point, computed by automatic differentiation rather than symbolically. Same shape and dimension rules as grad.",
        example: "grad_f(fn v => v[0]^2 + v[1]^2, [1.0, 2.0])",
    },
    Guide {
        name: "heatmap",
        description: "Draws a color-scaled matrix with row zero at the top and columns increasing to the right. Non-finite cells are left blank.",
        example: "heatmap([[1.0, 2.0], [3.0, 4.0]])",
    },
    Guide {
        name: "heatmap_with",
        description: "Like heatmap, with a PlotOpts record for label, units, scales and limits.",
        example: "heatmap_with([[1.0, 2.0], [3.0, 4.0]], default_plot_opts)",
    },
    Guide {
        name: "hermite_h",
        description: "hermite_h(n, x) is the physicists' Hermite polynomial H_n(x), which appears in the quantum harmonic oscillator.",
        example: "hermite_h(3, 0.5)",
    },
    Guide {
        name: "hessian",
        description: "hessian(f, x) is the matrix of second partial derivatives of a scalar function at x. Its entries have the dimension of the output over the input squared.",
        example: "hessian(fn v => v[0]^2 * v[1], [1, 2])",
    },
    Guide {
        name: "hist",
        description: "Draws counts in a positive number of equal-width bins. Pass at least one finite observation. Non-finite observations are skipped.",
        example: "hist([1.0, 2.0, 2.5, 3.0, 9.0], 4)",
    },
    Guide {
        name: "hist_with",
        description: "Like hist, with a PlotOpts record for label, colour, units, scales and limits.",
        example: "hist_with([1.0, 2.0, 2.5, 3.0, 9.0], 4, default_plot_opts)",
    },
    Guide {
        name: "histogram",
        description: "`histogram(xs, bins)` counts finite sample values in a positive number of equal-width bins. Returns integer counts and `bins + 1` Float edges in the sample’s dimension.",
        example: "histogram([1.0, 2.0, 2.5, 3.0, 9.0], 4)",
    },
    Guide {
        name: "hypot",
        description: "The length of the vector (a, b), sqrt(a^2 + b^2), computed without intermediate overflow. Both arguments must share a dimension, which the result keeps.",
        example: "hypot(3.0 m, 4.0 m)",
    },
    Guide {
        name: "i",
        description: "The imaginary unit as a Complex constant, so 3 + 4*i builds a complex number. Complex arithmetic follows the usual rules, i*i == -1.",
        example: "i * i",
    },
    Guide {
        name: "id",
        description: "The identity function: returns its argument unchanged. Handy as a default function argument.",
        example: "id(5)",
    },
    Guide {
        name: "ifft",
        description: "Inverse discrete Fourier transform, divided by the vector length so that ifft(fft(x)) recovers x.",
        example: "ifft(fft([1.0, 2.0, 3.0, 4.0]))",
    },
    Guide {
        name: "ifft2",
        description: "Inverse two-dimensional Fourier transform over the last two axes.",
        example: "ifft2([[1.0, 2.0], [3.0, 4.0]])",
    },
    Guide {
        name: "ifft_axis",
        description: "Inverse Fourier transform along a single axis of a tensor.",
        example: "ifft_axis([[1.0, 2.0], [3.0, 4.0]], 1)",
    },
    Guide {
        name: "ifft_norm",
        description: "Inverse transform with the same Norm convention you used for fft_norm; pass the same value for a round trip.",
        example: "import FFT { Unitary }\nifft_norm([1.0, 2.0, 3.0, 4.0], Unitary)",
    },
    Guide {
        name: "ifftn",
        description: "Inverse Fourier transform over every axis of a tensor.",
        example: "ifftn([[1.0, 2.0], [3.0, 4.0]])",
    },
    Guide {
        name: "ifftn_axes",
        description: "Inverse Fourier transform over a chosen list of axes.",
        example: "ifftn_axes([[1.0, 2.0], [3.0, 4.0]], [0, 1])",
    },
    Guide {
        name: "ifftn_norm",
        description: "Inverse Fourier transform over every axis with a chosen normalisation; use the same Norm as the forward transform.",
        example: "import FFT { Unitary }\nifftn_norm([[1.0, 2.0], [3.0, 4.0]], Unitary)",
    },
    Guide {
        name: "ifftshift",
        description: "Undoes fftshift: moves the zero frequency from the middle back to the first position.",
        example: "ifftshift([2, 3, 0, 1])",
    },
    Guide {
        name: "ifftshift_axis",
        description: "ifftshift applied to one axis only.",
        example: "ifftshift_axis([[2, 3, 0, 1]], 1)",
    },
    Guide {
        name: "im",
        description: "The imaginary part of a complex number.",
        example: "im(complex(1, 2))",
    },
    Guide {
        name: "input",
        description: "Prints a prompt and reads one line from standard input, without the trailing newline. It is an IO action, so use it inside a do block.",
        example: "input(\"name? \")",
    },
    Guide {
        name: "interp1",
        description: "Linear interpolation: interp1(xs, ys, x) looks up x in the increasing sample points xs and interpolates the matching ys. Outside the range of xs the first or last segment is extended as a straight line. The result has ys's dimension.",
        example: "interp1([0.0, 1.0, 2.0], [0.0, 10.0, 40.0], 1.5)",
    },
    Guide {
        name: "interp2",
        description: "Bilinear interpolation at (x, y) on a grid. Store the sample at (xs[i], ys[j]) in table[j][i]. Finite queries outside the grid are clamped to its edges.",
        example: "interp2([0.0, 1.0], [0.0, 1.0], [[0.0, 1.0], [2.0, 3.0]], 0.5, 0.5)",
    },
    Guide {
        name: "interp2Batch",
        description: "Bilinear interpolation for paired samples. Use increasing axes and place the sample at (xs[i], ys[j]) in table[j][i]. Supply two equally long query tensors with the dimensions of their respective axes. Finite queries clamp to the edges. Float results keep the input order and the table's dimension.",
        example: "interp2Batch([0.0, 1.0], [0.0, 1.0], [[0.0, 1.0], [2.0, 3.0]], [0.5, 0.0], [0.5, 1.0])",
    },
    Guide {
        name: "intersection",
        description: "The set of elements that are in both sets.",
        example: "intersection(set_of([1, 2]), set_of([2, 3]))",
    },
    Guide {
        name: "inv",
        description: "The inverse of a square matrix, returned as a Result: a singular matrix gives Err(LinAlgError). The elements of the inverse carry the reciprocal dimension. To solve a system prefer linsolve, which is more accurate than multiplying by an inverse.",
        example: "inv([[1.0, 2.0], [3.0, 4.0]])",
    },
    Guide {
        name: "irfft",
        description: "Inverse of rfft: rebuilds the real signal from the half spectrum. The length defaults to an even number, 2(m - 1); pass the original length as a second argument for odd-length signals.",
        example: "irfft(rfft([1.0, 2.0, 3.0, 4.0]))\nirfft(rfft([1.0, 2.0, 3.0]), 3)",
    },
    Guide {
        name: "irfft2",
        description: "Inverse of rfft2. An optional second argument gives the original length of the last axis when it was odd.",
        example: "irfft2(rfft2([[1.0, 2.0], [3.0, 4.0]]))",
    },
    Guide {
        name: "irfftn",
        description: "Inverse of rfftn. An optional second argument gives the original length of the last axis when it was odd.",
        example: "irfftn(rfftn([[1.0, 2.0], [3.0, 4.0]]))",
    },
    Guide {
        name: "irfftn_axes",
        description: "Inverse of rfftn_axes. A third argument gives the original length of the last listed axis when it was odd.",
        example: "irfftn_axes(rfftn_axes([[1.0, 2.0], [3.0, 4.0]], [0, 1]), [0, 1])",
    },
    Guide {
        name: "is_err",
        description: "True when a Result is Err.",
        example: "is_err(parse_int(\"x\"))",
    },
    Guide {
        name: "is_finite",
        description: "True when a number is neither infinite nor NaN. Exact integers and rational values remain finite at any magnitude.",
        example: "is_finite(1.5)",
    },
    Guide {
        name: "is_infinite",
        description: "True for positive or negative infinity. NaN and ordinary numbers give false.",
        example: "is_infinite(1.0 / 0.0)",
    },
    Guide {
        name: "is_nan",
        description: "True when a Float is NaN. It is the only reliable test, because NaN is never equal to anything.",
        example: "is_nan(0.0 / 0.0)",
    },
    Guide {
        name: "is_none",
        description: "True when an Option is None.",
        example: "is_none(None)",
    },
    Guide {
        name: "is_ok",
        description: "True when a Result is Ok.",
        example: "is_ok(parse_int(\"7\"))",
    },
    Guide {
        name: "is_prime",
        description: "True when an Int is prime. Negative numbers, 0 and 1 are not.",
        example: "is_prime(97)",
    },
    Guide {
        name: "is_some",
        description: "True when an Option holds a value.",
        example: "is_some(Some(3))",
    },
    Guide {
        name: "is_subset",
        description: "is_subset(a, b) is true when every element of a is also in b.",
        example: "is_subset(set_of([1]), set_of([1, 2]))",
    },
    Guide {
        name: "j1",
        description: "Bessel function of the first kind of order 1; the same as bessel_j1.",
        example: "j1(1.0)",
    },
    Guide {
        name: "jacobian",
        description: "jacobian(f, x) is the matrix of partial derivatives of a vector function at x: row i holds the gradient of output i, so a function from n inputs to m outputs gives an m x n matrix.",
        example: "jacobian(fn v => [v[0] * v[1], v[0] + v[1]], [2, 3])",
    },
    Guide {
        name: "join",
        description: "join(sep, parts) joins a vector of strings with sep between them. It is the inverse of split.\n\njoin(left, right, key) joins two tables on a named column in each table. Each pair of matching rows contributes one output row. The left rows keep their order and each left row's matches follow the order of the right table. The output keeps the left columns in order and appends the right non-key columns. A missing key or an ambiguous column name raises E0903.",
        example: "join(\", \", [\"a\", \"b\", \"c\"])\nfn joined(left: Table<{ id: Float, x: Float }>, right: Table<{ id: Float, y: Float }>): Table<{ id: Float, x: Float, y: Float }> = join(left, right, \"id\")",
    },
    Guide {
        name: "kron",
        description: "The Kronecker product of two sparse matrices, a block matrix with a[i][j] * b in block (i, j). It is how a 2-D Laplacian is built from 1-D ones.",
        example: "kron(speye(2), spdiag([1.0, 2.0]))",
    },
    Guide {
        name: "kurtosis",
        description: "Sample kurtosis of a vector, a dimensionless measure of how heavy the tails are.",
        example: "kurtosis([1.0, 2.0, 2.0, 3.0, 9.0])",
    },
    Guide {
        name: "label",
        description: "Sets the series tooltip shown by SVG and HTML output.",
        example: "label(line([0.0, 1.0], [0.0, 1.0]), \"y = x\")",
    },
    Guide {
        name: "laguerre_l",
        description: "laguerre_l(n, x) is the Laguerre polynomial L_n(x), which appears in the radial hydrogen wavefunctions.",
        example: "laguerre_l(2, 0.5)",
    },
    Guide {
        name: "laplacian",
        description: "The Laplacian of a scalar field at a point: the sum of its second partial derivatives. The result dimension is the output over the input squared.",
        example: "laplacian(fn v => v[0]^2 + v[1]^2, [1, 2])",
    },
    Guide {
        name: "last",
        description: "The last element as an Option, None for an empty tensor. On a matrix it is the last row.",
        example: "last([1, 2, 3])",
    },
    Guide {
        name: "lcm",
        description: "Least common multiple of two Ints; zero if either argument is zero.",
        example: "lcm(4, 6)",
    },
    Guide {
        name: "least_squares",
        description: "least_squares(residuals, x0) finds the parameters x that minimise the sum of squares of the vector-valued residual function, starting from x0. Use it to fit a model to data; for a linear model lstsq is simpler and exact.",
        example: "least_squares(fn p => [p[0] - 1, p[1] - 2], [0, 0])",
    },
    Guide {
        name: "legendre_p",
        description: "legendre_p(n, x) is the Legendre polynomial of degree n, orthogonal on [-1, 1].",
        example: "legendre_p(2, 0.5)",
    },
    Guide {
        name: "length",
        description: "Return the collection length. For strings, count Unicode grapheme clusters rather than UTF-8 bytes.",
        example: "length([1, 2, 3])\nlength(\"hello\")",
    },
    Guide {
        name: "lgamma",
        description: "Natural logarithm of the absolute value of gamma. Prefer it to log(gamma(x)) for large x, where gamma itself overflows.",
        example: "lgamma(100.0)",
    },
    Guide {
        name: "limit",
        description: "limit(f, x) is the limit of f as its argument approaches x, as a Result; use oo or -oo for limits at infinity. Err(MathError) means the kernel could not decide the limit.",
        example: "limit(fn x => sin(x) / x, 0)",
    },
    Guide {
        name: "line",
        description: "Builds a line series from equal-length vectors of concrete real coordinates. Rendering skips pairs with a non-finite coordinate and connects the surviving pairs. SVG axis captions include the coordinate units.",
        example: "line([0.0, 1.0, 2.0], [0.0, 1.0, 4.0])",
    },
    Guide {
        name: "line_with",
        description: "Builds a line series with a PlotOpts record. Copy default_plot_opts to set a label, color, display units, scales or axis limits.",
        example: "line_with([0.0, 1.0], [1.0, 10.0], { default_plot_opts with color = Some(\"blue\") })",
    },
    Guide {
        name: "linsolve",
        description: "Solves A x = b for x, returning a Result; a singular or non-square system gives Err(LinAlgError). The solution has the dimension of b divided by that of A. For over-determined systems use lstsq.",
        example: "linsolve([[2.0, 0.0], [0.0, 4.0]], [2.0, 4.0])",
    },
    Guide {
        name: "linspace",
        description: "Returns evenly spaced floating-point samples with the endpoints’ dimension. For finite endpoints, two or more samples include both limits. One sample returns the start, and a nonpositive count returns an empty tensor.",
        example: "linspace(0.0, 1.0, 5)\nlinspace(0.0 m, 10.0 m, 3)",
    },
    Guide {
        name: "ln",
        description: "Natural logarithm of a dimensionless argument. It is the inverse of `exp` on the positive real line.",
        example: "ln(2.0)",
    },
    Guide {
        name: "log",
        description: "Natural logarithm of one dimensionless argument. This is the same function as `ln`.",
        example: "log(2.0)",
    },
    Guide {
        name: "log10",
        description: "Base-10 logarithm of a dimensionless argument. A tenfold increase adds one to the logarithm.",
        example: "log10(100.0)",
    },
    Guide {
        name: "log2",
        description: "Base-2 logarithm of a dimensionless argument. Doubling the input adds one to the logarithm.",
        example: "log2(8.0)",
    },
    Guide {
        name: "lower",
        description: "Converts a string to lower case.",
        example: "lower(\"HeLLo\")",
    },
    Guide {
        name: "lstsq",
        description: "Least-squares solution of A x = b for a tall matrix A: the x that minimises the length of A x - b. Returns a Result, so a rank-deficient system gives Err(LinAlgError). Use it to fit a linear model.",
        example: "lstsq([[1.0, 0.0], [1.0, 1.0], [1.0, 2.0]], [1.0, 2.0, 3.0])",
    },
    Guide {
        name: "lu",
        description: "LU decomposition with pivoting. Returns the triple (P, L, U) satisfying P A = L U, where P is the permutation matrix that records the row swaps, L is unit lower-triangular and U is upper-triangular.",
        example: "lu([[1.0, 2.0], [3.0, 4.0]])",
    },
    Guide {
        name: "magnitude",
        description: "The modulus (absolute value) of a complex number, keeping its dimension. Use this instead of abs for Complex.",
        example: "magnitude(complex(3, 4))",
    },
    Guide {
        name: "map",
        description: "Apply a function to collection elements, preserving their order. The function receives a row for supported higher-rank tensor overloads.",
        example: "map(fn (x) => x * x, [1, 2, 3])",
    },
    Guide {
        name: "map2",
        description: "map2(f, a, b) applies a two-argument function to corresponding elements of two tensors of the same shape and returns a tensor of that shape.",
        example: "map2(fn (x, y) => x + y, [1, 2, 3], [10, 20, 30])",
    },
    Guide {
        name: "map_err",
        description: "Transforms the error of a Result and leaves an Ok untouched. Useful for converting one error type into another before using ?.",
        example: "map_err(fn e => \"failed: {e}\", parse_int(\"x\"))",
    },
    Guide {
        name: "map_indexed",
        description: "map_indexed(f, xs) calls f(i, x) for every element, passing its position as well as its value.",
        example: "map_indexed(fn (i, x) => i * x, [5, 6, 7])",
    },
    Guide {
        name: "map_nonzero",
        description: "Applies a function to every stored entry of a sparse tensor, leaving the zeros alone, so the function should map zero to zero.",
        example: "map_nonzero(fn x => 2.0 * x, speye(3))",
    },
    Guide {
        name: "matmul",
        description: "Matrix product of an n x k and a k x m matrix. The result dimension is the product of the two dimensions. A sparse matrix may be multiplied by a vector. A shape mismatch is rejected by the checker when the shapes are known.",
        example: "matmul([[1.0, 2.0], [3.0, 4.0]], [[0.0, 1.0], [1.0, 0.0]])",
    },
    Guide {
        name: "matvec",
        description: "Matrix times column vector: for an n x k matrix and a vector of length k, the vector of n row dot-products.",
        example: "matvec([[1.0, 2.0], [3.0, 4.0]], [1.0, 1.0])",
    },
    Guide {
        name: "max",
        description: "The larger of two numbers of the same dimension. To reduce a whole tensor use max_all.",
        example: "max(3, 5)",
    },
    Guide {
        name: "max_all",
        description: "The largest element of a tensor of any rank. Use max for two numbers. An empty tensor is a runtime error.",
        example: "max_all([3, 1, 2])",
    },
    Guide {
        name: "maximize",
        description: "maximize(f, x0) searches for a local maximum of a scalar function of a vector, starting at x0, and returns the maximiser as a Result.",
        example: "maximize(fn v => -(v[0] - 1)^2, [0])",
    },
    Guide {
        name: "mean",
        description: "The arithmetic mean of every element of a tensor of any rank; the result has the elements' dimension. An Int tensor gives an exact Real quotient.",
        example: "mean([1.0, 2.0, 3.0, 6.0])\nmean([1.0 m, 2.0 m, 3.0 m])",
    },
    Guide {
        name: "measure",
        description: "Construct a measurement record containing a value and an uncertainty with the same physical dimension.",
        example: "measure(3 m, 0.1 m)",
    },
    Guide {
        name: "median",
        description: "The middle value of a vector; for an even length, the mean of the two middle values. The dimension is kept.",
        example: "median([3.0, 1.0, 2.0, 10.0])",
    },
    Guide {
        name: "member",
        description: "member(x, set) is true when x is in the set.",
        example: "member(2, set_of([1, 2, 3]))",
    },
    Guide {
        name: "min",
        description: "The smaller of two numbers of the same dimension. To reduce a whole tensor use min_all.",
        example: "min(3, 5)\nmin(2 m, 150 cm)",
    },
    Guide {
        name: "min_all",
        description: "The smallest element of a tensor of any rank. Use min for two numbers. An empty tensor is a runtime error.",
        example: "min_all([3, 1, 2])",
    },
    Guide {
        name: "minimize",
        description: "minimize(f, x0) searches for a local minimum of a scalar function of a vector, starting at x0, and returns the minimiser as a Result. The result is only as global as your starting point. Use maximize for a maximum or minimize_f for Float functions.",
        example: "minimize(fn v => (v[0] - 1)^2 + (v[1] - 2)^2, [0, 0])",
    },
    Guide {
        name: "minimize_f",
        description: "Float version of minimize: finds a local minimum of a Float function of a vector from the given starting vector, returning a Result.",
        example: "minimize_f(fn v => (v[0] - 1.0)^2 + (v[1] - 2.0)^2, [0.0, 0.0])",
    },
    Guide {
        name: "mod",
        description: "Remainder that takes the sign of the divisor (floored): mod(-7, 3) is 2. For a positive divisor the result is always in [0, b), which suits wrapping an index around a ring. Compare rem, which follows the dividend.",
        example: "mod(-7, 3)\nmod(7.5, 2.0)",
    },
    Guide {
        name: "mod_pow",
        description: "Computes the exact residue of base raised to exponent modulo modulus. The result lies in [0, modulus). All arguments support arbitrary-size integers. The exponent must be nonnegative and the modulus positive.",
        example: "mod_pow(-3, 3, 7)\nmod_pow(2, 1000000, 97)",
    },
    Guide {
        name: "mode",
        description: "The most frequent value of a vector; ties go to the smallest value.",
        example: "mode([1, 2, 2, 3])",
    },
    Guide {
        name: "nan",
        description: "The IEEE not-a-number Float. It compares unequal to everything, itself included, so test it with is_nan rather than ==.",
        example: "is_nan(nan)",
    },
    Guide {
        name: "nnz",
        description: "The number of stored (non-zero) entries of a sparse tensor.",
        example: "nnz(speye(3))",
    },
    Guide {
        name: "nonzeros",
        description: "Lists the stored entries of a sparse tensor as (index vector, value) pairs, in order.",
        example: "nonzeros(speye(2))",
    },
    Guide {
        name: "norm",
        description: "The Euclidean (2-) norm of a vector, or the Frobenius norm of a matrix, in the elements' dimension. A second Int argument selects the p-norm of a vector.",
        example: "norm([3.0, 4.0])\nnorm([1.0, 2.0, 3.0], 1)",
    },
    Guide {
        name: "normalize",
        description: "Divides a tensor by its norm, so the result has norm 1 and is dimensionless. Normalising a zero tensor gives NaN entries, since it divides zero by zero.",
        example: "normalize([3.0, 4.0])",
    },
    Guide {
        name: "not",
        description: "Logical negation of a Bool.",
        example: "not(true)",
    },
    Guide {
        name: "nsolve",
        description: "nsolve(f, x0) finds a root of f numerically, starting the search at x0. It returns Ok(root) or Err(MathError) when the iteration does not converge, so check the Result and try a different start. root is a synonym.",
        example: "nsolve(fn x => x^2 - 2, 1)",
    },
    Guide {
        name: "nsolve_f",
        description: "Float version of nsolve: finds a root of a Float function from a Float starting point. Returns Err(MathError) when it does not converge.",
        example: "nsolve_f(fn x => x * x - 2.0, 1.0)",
    },
    Guide {
        name: "numeric_deriv",
        description: "Returns the derivative of a Float function as a new Float function, computed numerically rather than symbolically. Use it when the function calls numeric-only code; use diff when you want an exact result.",
        example: "numeric_deriv(fn x => x * x)(3.0)",
    },
    Guide {
        name: "ode_bvp",
        description: "Solves a boundary-value problem by single shooting: ode_bvp(f, y0_guess, (t0, t1), residual, options). It adjusts the initial state until residual(start, end) is zero, and returns the solution. It does not converge for strongly unstable problems.",
        example: "let solver = ode_bvp",
    },
    Guide {
        name: "ode_solve",
        description: "The full-featured ODE solver: ode_solve(f, y0, (t0, t1), options, events). It returns a solution record with the accepted times and states, a function at(t) that gives the state at any time in the span, the events that fired, and counts of work done. Each event is a record { g, terminal, direction } where g changes sign when the event happens; a terminal event stops the integration. Use odeint when you only need values at chosen times.",
        example: "let solver = ode_solve",
    },
    Guide {
        name: "odeint",
        description: "Integrates dy/dt = f(t, y) and returns the state at each requested time: odeint(f, y0, times). The state can be a number, a vector or a record; the function must return a matching record of rates, whose dimension is the state's per unit time. It uses an adaptive Dormand-Prince method with tight tolerances. Use odeint_with to choose the method and tolerances.",
        example: "fn decay(t: Float[T], y: Float[1]): Float[1/T] = -y / 1.0 s\nodeint(decay, 1.0, linspace(0.0 s, 2.0 s, 5))",
    },
    Guide {
        name: "odeint_order",
        description: "Integrates a higher-order equation such as y'' = f(t, y, y'). Give the right-hand side and the initial values of y and its derivatives as a tuple; the result holds the times, y at each time, and all derivatives. Each derivative's dimension is that of y per time to the power of its order.",
        example: "let solver = odeint_order",
    },
    Guide {
        name: "odeint_order_with",
        description: "Like odeint_order, with an options record choosing the method and tolerances.",
        example: "let solver = odeint_order_with",
    },
    Guide {
        name: "odeint_with",
        description: "Like odeint, with options (method, tolerances, fixed step count, events) in a record. It returns the times and states actually reached as { t, y }.",
        example: "import Ode { Dopri5 }\nfn decay(t: Float[T], y: Float[1]): Float[1/T] = -y / 1.0 s\nodeint_with(decay, 1.0, linspace(0.0 s, 2.0 s, 5), { rtol = 0.000001, atol = 0.000000001, method = Dopri5, steps = None, events = None })",
    },
    Guide {
        name: "ok",
        description: "Turns a Result into an Option, keeping the Ok value and discarding the error.",
        example: "ok(parse_int(\"7\"))",
    },
    Guide {
        name: "ok_or",
        description: "Turns an Option into a Result, using the second argument as the error for None.",
        example: "ok_or(None, \"missing\")",
    },
    Guide {
        name: "on",
        description: "Runs the zero-argument function thunk on the requested CPU device. This IO action currently supports Cpu. Requests for Gpu or Distributed produce E1001.",
        example: "fn run_here(m: Tensor<2, Float>): IO Int = on(device_of(m), fn () => 1 + 1)",
    },
    Guide {
        name: "ones",
        description: "ones(n1, n2, ...) builds a tensor of ones with one extent per argument. The elements are Float unless you give a type argument: ones<Int>(3).",
        example: "ones(2, 3)\nones<Int>(3)",
    },
    Guide {
        name: "ones_of",
        description: "A tensor of ones whose shape is a type argument: ones_of<[2, 3]>(). The shape is checked at compile time. When the shape is only known at run time use ones.",
        example: "ones_of<[2, 3]>()",
    },
    Guide {
        name: "oo",
        description: "Positive infinity as an exact Real. Write -oo for negative infinity. Use it as a limit or integration bound; combinations that would need NaN stay symbolic instead of producing one.",
        example: "limit(fn x => 1 / x, oo)",
    },
    Guide {
        name: "outer",
        description: "The outer product of two vectors: entry (i, j) is a[i] * b[j], giving a matrix of shape length(a) x length(b).",
        example: "outer([1.0, 2.0], [3.0, 4.0])",
    },
    Guide {
        name: "par_map",
        description: "Like map, but the work may be spread over threads. The result order always matches the input, so the answer never depends on the thread count. It applies to Int and Float data, not to symbolic Reals.",
        example: "par_map(fn x => x * 2.0, [1.0, 2.0, 3.0])",
    },
    Guide {
        name: "parse_float",
        description: "Parses a decimal or scientific-notation number such as \"2.5\" or \"1e-3\", returning Ok or Err(ParseError).",
        example: "parse_float(\"2.5\")",
    },
    Guide {
        name: "parse_int",
        description: "Parses a decimal integer, returning Ok(n) or Err(ParseError) when the text is not a valid Int. Surrounding whitespace is ignored.",
        example: "parse_int(\"42\")\nparse_int(\"4x\")",
    },
    Guide {
        name: "partial",
        description: "partial(f, a) fixes the first argument of a two-argument function and returns a function of the second.",
        example: "partial(fn (a, b) => a * b, 3)(4)",
    },
    Guide {
        name: "partial_compare",
        description: "Compares two Floats, returning None when either is NaN. Prefer it to < and > when NaNs may occur and you need to notice.",
        example: "partial_compare(1.0, nan)",
    },
    Guide {
        name: "pdf",
        description: "pdf(dist, x) is the probability density (or mass, for discrete distributions) of a distribution at x. Its dimension is the reciprocal of x's dimension. Distributions are built with constructors such as Normal and Uniform from Stats.Distributions.",
        example: "import Stats.Distributions { Normal }\npdf(Normal({ mu = 0.0, sigma = 1.0 }), 0.0)",
    },
    Guide {
        name: "permutations",
        description: "permutations(n) is the number of orderings of n distinct items, n!.",
        example: "permutations(4)",
    },
    Guide {
        name: "pi",
        description: "The exact symbolic constant pi. It is not a pre-rounded Float; use approx for a numerical approximation.",
        example: "pi\napprox(pi)",
    },
    Guide {
        name: "plot",
        description: "Renders a plot specification to plot.html with inline SVG when the IO action runs. Rendering or writing failures report E0905.",
        example: "plot(combine([line([0.0, 1.0], [0.0, 1.0])]))",
    },
    Guide {
        name: "plot_save_gif",
        description: "Renders a nonempty vector of plot specifications as looping 800 by 400 GIF frames. Pass an Int delay from 0 through 65535 in hundredths of a second. The unqualified two-argument call uses a delay of 10. The allocation limit permits up to 312 frames. The IO action returns Ok(()) or Err(IOError).",
        example: "import Plot as Plots\nPlots.plot_save_gif(\"anim.gif\", [Plots.combine([Plots.line([0.0, 1.0], [0.0, 1.0])])], 25)",
    },
    Guide {
        name: "point",
        description: "point(value, unit) builds an absolute value in an affine unit such as degC or degF, whose zero is not the physical zero. Subtract two points to get a difference; you cannot add two points.",
        example: "point(20.0, degC)",
    },
    Guide {
        name: "polyfit",
        description: "polyfit(xs, ys, degree) fits a least-squares polynomial. The x values must be dimensionless; the returned coefficients start with the constant term and rise in power, and carry y's dimension. Evaluate them with polyval.",
        example: "polyfit([0.0, 1.0, 2.0, 3.0], [1.0, 3.0, 7.0, 13.0], 2)",
    },
    Guide {
        name: "polygon_area",
        description: "Area of a simple polygon from a matrix of (x, y) vertices in order, via the shoelace formula; the result has the square of the coordinates' dimension.",
        example: "polygon_area([[0.0 m, 0.0 m], [2.0 m, 0.0 m], [2.0 m, 3.0 m]])",
    },
    Guide {
        name: "polyval",
        description: "polyval(coefficients, x) evaluates a polynomial given its coefficients starting with the constant term, as returned by polyfit.",
        example: "polyval([1.0, 2.0, 3.0], 2.0)",
    },
    Guide {
        name: "primes",
        description: "All primes up to and including n, as a vector.",
        example: "primes(30)",
    },
    Guide {
        name: "print",
        description: "Print a value to standard output without a trailing newline. This returns IO Unit and executes when the REPL runs the IO expression.",
        example: "print(\"answer: \")\nprintln(42)",
    },
    Guide {
        name: "println",
        description: "Print a value to standard output followed by a newline. This returns IO Unit; the REPL executes the action once. String interpolation is useful for labels and mixed values.",
        example: "println(\"answer: {6 * 7}\")\nprintln([1, 2, 3])",
    },
    Guide {
        name: "prod",
        description: "The product of every element of a tensor of any rank. The elements must be dimensionless, since the dimension of a product would depend on the length.",
        example: "prod([1, 2, 3, 4])",
    },
    Guide {
        name: "product",
        description: "product(f, a, b) is the exact product of f(k) for k from a to b, both integers; bounds may be symbolic or oo when a closed form exists. The terms must be dimensionless; use product_dyn for dimensioned terms.",
        example: "product(fn k => k, 1, 5)",
    },
    Guide {
        name: "product_dyn",
        description: "Like product, but for terms that carry a dimension. The result is a dynamically typed value (Dyn) because the dimension of a product depends on the number of terms.",
        example: "product_dyn(fn k => k, 1, 5)",
    },
    Guide {
        name: "pure",
        description: "Wrap a value in an IO action without performing an effect. Useful as the final expression in a do block.",
        example: "pure(42)",
    },
    Guide {
        name: "qr",
        description: "QR decomposition A = Q R of a matrix into an orthogonal Q and an upper-triangular R. Returns the pair (Q, R).",
        example: "qr([[1.0, 2.0], [3.0, 4.0]])",
    },
    Guide {
        name: "quadrature",
        description: "quadrature(f, a, b) integrates a Float function numerically over [a, b] and returns Ok(value), or Err(MathError) if the method does not converge. The result dimension is output times input.",
        example: "quadrature(fn x => x * x, 0.0, 3.0)",
    },
    Guide {
        name: "quantile",
        description: "quantile(xs, q) is the value below which a fraction q (between 0 and 1) of the data falls, with linear interpolation; quantile(xs, 0.5) is the median.",
        example: "quantile([1.0, 2.0, 3.0, 4.0], 0.25)",
    },
    Guide {
        name: "quantiles",
        description: "Compute Type-7 quantiles for a vector of dimensionless probabilities in [0, 1]. The batch shares one ordered sample and keeps the sample dimension and query order, including duplicates. Exact inputs use rational interpolation. The sample must be finite and nonempty. An empty probability vector returns an empty result.",
        example: "quantiles([4.0 m, 1.0 m, 3.0 m, 2.0 m], [0.75, 0.25, 0.5])",
    },
    Guide {
        name: "quot",
        description: "Integer division that rounds toward zero: quot(-7, 2) is -3. It pairs with rem. Compare div, which rounds down.",
        example: "quot(-7, 2)",
    },
    Guide {
        name: "random",
        description: "A random Float in [0, 1). It is an IO action because each call gives a different answer; for a reproducible stream use rng(seed).",
        example: "random()",
    },
    Guide {
        name: "range",
        description: "Construct an integer sequence with an exclusive end. An optional step may be positive or negative and cannot be zero.",
        example: "range(0, 5)\nrange(5, 0, -1)",
    },
    Guide {
        name: "rank",
        description: "The number of axes of a tensor: 1 for a vector, 2 for a matrix. A scalar has rank 0. Use shape for the length of each axis.",
        example: "rank([[1, 2], [3, 4]])",
    },
    Guide {
        name: "re",
        description: "The real part of a complex number.",
        example: "re(complex(1, 2))",
    },
    Guide {
        name: "read_csv",
        description: "Reads comma-separated .csv or tab-separated .tsv files. An explicit type argument or the surrounding expression supplies the row type, which is used only for static checking. The reader checks quoting and row widths using the file’s header. Header units such as s in time [s] are column metadata reported by schema, and numeric cells are dimensionless Float values. The IO action returns Ok(table) or Err(DataError).",
        example: "read_csv<{ time: Float, speed: Float }>(\"run.csv\")",
    },
    Guide {
        name: "read_csv_dyn",
        description: "Reads comma-separated .csv or tab-separated .tsv files as Table<Dynamic>. The reader checks quoting and row widths. Header units such as s in time [s] are column metadata reported by schema, and numeric cells are dimensionless Float values. The IO action returns Ok(table) or Err(DataError). Use column_dyn to get values by column name.",
        example: "read_csv_dyn(\"run.tsv\")",
    },
    Guide {
        name: "read_file",
        description: "Create an IO action to read a UTF-8 file. The action returns a Result and requires the fs capability.",
        example: "read_file(\"data.txt\")",
    },
    Guide {
        name: "read_gif",
        description: "Reads an animated GIF into a record with width, height, loop_count and a vector of frames, each frame having its delay in hundredths of a second and its RGBA bytes. It is an IO action returning Ok or Err(IOError).",
        example: "read_gif(\"anim.gif\")",
    },
    Guide {
        name: "read_json",
        description: "Reads a JSON file as a dynamic Json value, returning Ok(json) or Err(DataError). Convert parts of it to typed values with from_json. It is an IO action.",
        example: "read_json(\"config.json\")",
    },
    Guide {
        name: "read_line",
        description: "Create an IO action that reads one line of terminal input. In the REPL, Ctrl-C cancels the read and returns to the prompt.",
        example: "let ask = do\n    line <- read_line()\n    println(\"you typed: {line}\")\nask",
    },
    Guide {
        name: "read_npy",
        description: "Reads a NumPy .npy file of floats as a tensor. You must state the rank as a type argument, read_npy<2>(path), so the result type is known; a file of a different rank gives Err(DataError). It is an IO action.",
        example: "read_npy<2>(\"matrix.npy\")",
    },
    Guide {
        name: "read_svg",
        description: "Reads an SVG file as text. It is an IO action returning Ok(text) or Err(IOError).",
        example: "read_svg(\"figure.svg\")",
    },
    Guide {
        name: "refine_shape",
        description: "Checks at run time that a tensor has a given shape, written as a type argument where ? matches any size: refine_shape<[3, ?]>(m). Returns Some(tensor) with the sharper type when it matches and None otherwise, so you can branch on it with match.",
        example: "refine_shape<[2, ?]>([[1, 2], [3, 4]])",
    },
    Guide {
        name: "rem",
        description: "Remainder that takes the sign of the dividend (truncated): rem(-7, 3) is -1. Compare mod, which follows the divisor.",
        example: "rem(-7, 3)",
    },
    Guide {
        name: "replace",
        description: "replace(s, from, to) replaces every occurrence of from in s with to and returns the new string.",
        example: "replace(\"hello\", \"l\", \"L\")",
    },
    Guide {
        name: "repr",
        description: "Return a source representation that preserves numeric phase, unlike display-oriented show. It does not print by itself.",
        example: "repr(1.0)\nprintln(repr(1.0))",
    },
    Guide {
        name: "reshape",
        description: "Gives a tensor a new shape with the same elements, written as a type argument: reshape<[3, 2]>(t). Returns Err(ShapeError) when the element counts differ (a mismatch between two shapes both known at compile time is reported as an error before the program runs). For a shape known only at run time use reshape_dyn.",
        example: "reshape<[2, 2]>([1, 2, 3, 4])",
    },
    Guide {
        name: "reshape_dyn",
        description: "`reshape_dyn(t, shape)` takes one nonnegative `Int` extent per input axis. It returns `Ok(t)` with the reshaped tensor, or a shape error for invalid extents, rank, or element count.",
        example: "reshape_dyn([[1, 2], [3, 4]], [4, 1])",
    },
    Guide {
        name: "reverse",
        description: "Reverses the first axis of a tensor. For a matrix that reverses the rows; use reverse_axis to choose the axis.",
        example: "reverse([1, 2, 3])",
    },
    Guide {
        name: "reverse_axis",
        description: "Reverses a tensor along a chosen axis, e.g. reverse_axis(m, 1) flips every row.",
        example: "reverse_axis([[1, 2], [3, 4]], 1)",
    },
    Guide {
        name: "rfft",
        description: "Fourier transform of real data that keeps only the n/2 + 1 non-redundant frequencies, roughly halving the work and the output size compared with fft.",
        example: "rfft([1.0, 2.0, 3.0, 4.0])",
    },
    Guide {
        name: "rfft2",
        description: "Real two-dimensional Fourier transform over the last two axes, keeping the non-redundant half along the last one.",
        example: "rfft2([[1.0, 2.0], [3.0, 4.0]])",
    },
    Guide {
        name: "rfftfreq",
        description: "rfftfreq(n, dt) lists the n/2 + 1 non-negative frequencies matching rfft of n samples taken dt apart.",
        example: "rfftfreq(4, 0.5 s)",
    },
    Guide {
        name: "rfftn",
        description: "Real Fourier transform over every axis, keeping the non-redundant half along the last axis.",
        example: "rfftn([[1.0, 2.0], [3.0, 4.0]])",
    },
    Guide {
        name: "rfftn_axes",
        description: "Real Fourier transform over a chosen list of axes; the last axis listed keeps only the half spectrum and the others get full complex transforms.",
        example: "rfftn_axes([[1.0, 2.0], [3.0, 4.0]], [0, 1])",
    },
    Guide {
        name: "rng",
        description: "rng(seed) returns a pseudo-random number generator: a function with no arguments that gives the next number in [0, 1) each time you call it. The sequence depends only on the seed, so runs are reproducible. For unseeded randomness use random().",
        example: "let next = rng(42)\nnext()",
    },
    Guide {
        name: "root",
        description: "A synonym for nsolve: finds a root of f near the starting point x0, returning a Result.",
        example: "root(fn x => x * x - 2.0, 1.0)",
    },
    Guide {
        name: "round",
        description: "Rounds to the nearest whole number, with halves going away from zero (round(2.5) is 3, round(-2.5) is -3). A second Int argument keeps that many decimals; a negative one rounds to tens, hundreds and so on.",
        example: "round(2.5)\nround(3.14159, 2)\nround(1234, -2)",
    },
    Guide {
        name: "row",
        description: "Builds a record from the selected row of a table. Indices start at zero, and -1 selects the last row. An out-of-range index returns Err(DataError).",
        example: "fn last_row(t: Table<{ x: Float }>) = row(t, -1)",
    },
    Guide {
        name: "sample",
        description: "Draws one random value from a distribution. It is an IO action because each call gives a different result; for a reproducible sequence use sample_n with a seed.",
        example: "import Stats.Distributions { Normal }\nsample(Normal({ mu = 0.0, sigma = 1.0 }))",
    },
    Guide {
        name: "sample_n",
        description: "sample_n(dist, n, seed) draws n values. With Some(seed) the result is reproducible; with None it is random.",
        example: "import Stats.Distributions { Normal }\nsample_n(Normal({ mu = 0.0, sigma = 1.0 }), 5, Some(42))",
    },
    Guide {
        name: "save",
        description: "Writes standalone SVG to a path with a case-insensitive .svg extension. Other paths receive HTML with inline SVG. Rendering or writing failures report E0905.",
        example: "save(\"plot.svg\", combine([line([0.0, 1.0], [0.0, 1.0])]))",
    },
    Guide {
        name: "scatter",
        description: "Builds an individual marker from each finite coordinate pair in two equal-length vectors.",
        example: "scatter([0.0, 1.0, 2.0], [0.0, 1.0, 4.0])",
    },
    Guide {
        name: "scatter_with",
        description: "Like scatter, with a PlotOpts record for label, colour, units, scales and limits.",
        example: "scatter_with([0.0, 1.0], [1.0, 10.0], default_plot_opts)",
    },
    Guide {
        name: "schema",
        description: "Reports each run-time column’s name, detected cell type and optional unit. A column whose cells all parse as numbers has dtype Float. Other columns have dtype String. The unit is the text stored in the column header.",
        example: "fn describe(t: Table<{ x: Float }>) = schema(t)",
    },
    Guide {
        name: "sde_at",
        description: "The state of an SDE path at an arbitrary time, found by linear interpolation between the stored steps.",
        example: "let lookup = sde_at",
    },
    Guide {
        name: "sde_ensemble_diag",
        description: "Integrates n independent paths of a diagonal-noise SDE with seeds derived from the base seed, and returns a vector with one Result per path. The result does not depend on how the work is scheduled.",
        example: "let solver = sde_ensemble_diag",
    },
    Guide {
        name: "sde_ensemble_general",
        description: "Integrates n independent paths of a general-noise SDE with seeds derived from the base seed, and returns a vector with one Result per path.",
        example: "let solver = sde_ensemble_general",
    },
    Guide {
        name: "sde_ensemble_seed",
        description: "The seed used for member number index of an ensemble started from seed. It is a pure function, so you can reproduce one member of an ensemble on its own.",
        example: "sde_ensemble_seed(42, 3)",
    },
    Guide {
        name: "sde_events",
        description: "Finds where event functions change sign along an SDE path. Returns the path, cut at the first terminal event, and the list of hits with their times and states.",
        example: "let detect = sde_events",
    },
    Guide {
        name: "sde_interp",
        description: "Turns an SDE path into a function of time that interpolates linearly between the stored steps.",
        example: "let lookup = sde_interp",
    },
    Guide {
        name: "sde_order",
        description: "The strong and weak convergence orders of an SDE scheme for commutative noise, as a record { strong, weak }.",
        example: "import Sde { Milstein }\nsde_order(Milstein)",
    },
    Guide {
        name: "sde_order_with",
        description: "Like sde_order, but for a given noise structure, since some schemes lose accuracy when the noise channels do not commute.",
        example: "import Sde { Milstein, Diagonal }\nsde_order_with(Milstein, Diagonal)",
    },
    Guide {
        name: "sde_resample",
        description: "The states of an SDE path at each time in a vector of times, by linear interpolation between stored steps.",
        example: "let lookup = sde_resample",
    },
    Guide {
        name: "sdeint_diag",
        description: "Integrates a stochastic differential equation dy = drift dt + diffusion dW in which every state component has its own independent noise: sdeint_diag(drift, diffusion, y0, options). The options say whether to use a fixed or adaptive step, which scheme (EulerMaruyama, Milstein, ...), the seed and the time grid. The same seed always gives the same path. Returns Ok(path) or Err(SdeError).",
        example: "let solver = sdeint_diag",
    },
    Guide {
        name: "sdeint_general",
        description: "Like sdeint_diag, for noise that couples the components: the diffusion returns a matrix with one column per noise channel. The structure option states how many channels there are and whether they commute.",
        example: "let solver = sdeint_general",
    },
    Guide {
        name: "select",
        description: "Keeps only the named columns of a Table: select(table, [\"time\", \"speed\"]).",
        example: "fn narrow(t: Table<{ time: Float, speed: Float, note: String }>) = select(t, [\"time\", \"speed\"])",
    },
    Guide {
        name: "series",
        description: "series(f, x0, n) is the Taylor expansion of f about x0 up to order n, as an exact expression.",
        example: "series(fn x => exp(x), 0, 3)",
    },
    Guide {
        name: "set",
        description: "`set(t, index, value)` returns a copy of the tensor with one element replaced. Supply one `Int` index per axis. Indices start at zero, and negative indices count back from the end of the axis.",
        example: "set([1, 2, 3], [1], 9)",
    },
    Guide {
        name: "set_insert",
        description: "Returns a new set with the element added. The original set is unchanged.",
        example: "set_insert(set_of([1, 2]), 3)",
    },
    Guide {
        name: "set_of",
        description: "Builds a set from a vector, dropping duplicates. Elements must have a total order, so Int and String work but Float does not.",
        example: "set_of([3, 1, 2, 3])",
    },
    Guide {
        name: "set_remove",
        description: "Returns a new set without the element. Removing something that is absent returns the same set.",
        example: "set_remove(set_of([1, 2]), 2)",
    },
    Guide {
        name: "shape",
        description: "Inspect the extents of a tensor. Shapes are checked separately from the dimensions carried by its elements.",
        example: "shape([[1, 2], [3, 4]])",
    },
    Guide {
        name: "show",
        description: "Convert a value to display text without printing it. Use println to emit that text, or interpolate the value directly. For phase-preserving source notation use repr.",
        example: "show(3 m)\nprintln(\"distance: {3 m}\")",
    },
    Guide {
        name: "sign",
        description: "The sign of a number as an Int: -1, 0 or 1. The dimension is ignored.",
        example: "sign(-2.5)",
    },
    Guide {
        name: "simplify",
        description: "Rewrites an exact expression into a simpler equivalent form. The kernel picks the form; use expand, factor, cancel or together when you need a specific one.",
        example: "let x = symbol(\"x\")\nsimplify((x^2 - 1) / (x - 1))",
    },
    Guide {
        name: "simulate",
        description: "Runs a named simulation scenario of the scientific kernel (for example \"ising\") from JSON parameters and returns its JSON result, or Err(SimError) for an unknown scenario or bad parameters. Most scenarios also have a typed wrapper module such as Physics.Ising, which is easier to use.",
        example: "simulate(\"ising\", to_json({ width = 8, height = 8, temperature = 2.0, mc_steps = 100, seed = Some(1) }))",
    },
    Guide {
        name: "sin",
        description: "Trigonometric function. Its argument must be dimensionless; angle units such as deg carry an exact conversion scale.",
        example: "sin(30 deg)\ncos(0.0)",
    },
    Guide {
        name: "sinh",
        description: "Hyperbolic sine of a dimensionless argument.",
        example: "sinh(1.0)",
    },
    Guide {
        name: "skewness",
        description: "Sample skewness of a vector: positive when the long tail is on the right. The result is dimensionless.",
        example: "skewness([1.0, 2.0, 2.0, 3.0, 9.0])",
    },
    Guide {
        name: "solve",
        description: "solve(f) finds the real roots of f(x) = 0 symbolically. It returns a Result holding a record with the roots, their multiplicities, any conditions they depend on (such as a != 0) and a complete flag that is true when the list is exhaustive. For numerical root finding use nsolve.",
        example: "solve(fn x => x^2 - 4)",
    },
    Guide {
        name: "solve_advection",
        description: "Solves the advection (transport) equation on a regular grid, returning the field at each time step as Ok(fields), or Err(PdeError) for an unstable time step or mismatched grid. See the Pde module for the record fields.",
        example: "let solver = solve_advection",
    },
    Guide {
        name: "solve_complex",
        description: "Like solve, but for a function that returns a Complex value: it finds all roots of f(x) = 0, including non-real ones, as a Result holding the same record as solve.",
        example: "solve_complex(fn x => complex(x^2 + 1, 0))",
    },
    Guide {
        name: "solve_heat",
        description: "Solves heat diffusion on a regular 2D grid and returns the initial field and every time step. The edges keep their initial values. Set method to \"adi\" for implicit stepping beyond the explicit stability limit. The default \"explicit\" requires alpha * dt / h^2 <= 1/4.",
        example: "import Pde { solve_heat }\nsolve_heat({ grid = { nx = 3, ny = 3, h = 1.0 }, alpha = 1.0, dt = 2.0, steps = 1, method = \"adi\", initial = [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]] })",
    },
    Guide {
        name: "solve_ivp",
        description: "Solves an initial value problem over a time span: solve_ivp(f, y0, (t0, t1), options), returning { t, y } at the steps taken. Options are as for odeint_with and choose the integration method.",
        example: "import Ode { Dopri5 }\nfn decay(t: Float[T], y: Float[1]): Float[1/T] = -y / 1.0 s\nsolve_ivp(decay, 1.0, (0.0 s, 2.0 s), { rtol = 0.000001, atol = 0.000000001, method = Dopri5, steps = None, events = None })",
    },
    Guide {
        name: "solve_poisson",
        description: "Solves the Poisson equation on a regular rectangular grid by finite differences. The argument is a record holding the grid and the source field; boundary values are zero. Returns Ok(field) with the grid and the values, or Err(PdeError) when the grid and field disagree. See the Pde module for the record fields.",
        example: "let solver = solve_poisson",
    },
    Guide {
        name: "solve_system",
        description: "solve_system(f, x0) solves a system of equations f(x) = 0, where f maps a vector to a vector of the same length, starting from x0. Returns Ok(solution) or Err(MathError).",
        example: "solve_system(fn v => [v[0] + v[1] - 3, v[0] - v[1] - 1], [1, 1])",
    },
    Guide {
        name: "solve_wave",
        description: "Solves the wave equation on a regular grid by finite differences, returning the field at each time step as Ok(fields), or Err(PdeError) for an unstable time step or mismatched grid. See the Pde module for the record fields.",
        example: "let solver = solve_wave",
    },
    Guide {
        name: "sort",
        description: "Sorts ascending, stably. Elements need a total order, so Int, String and Bool qualify but Float does not (use sort_float). On a matrix it sorts the rows lexicographically.",
        example: "sort([3, 1, 2])",
    },
    Guide {
        name: "sort_axis",
        description: "Sorts every one-dimensional slice of a tensor along the given axis, keeping the shape.",
        example: "sort_axis([[3, 1], [4, 2]], 1)",
    },
    Guide {
        name: "sort_by",
        description: "sort_by(cmp, xs) sorts with your own comparison, which returns an Ordering. The sort is stable, so ties keep their input order.",
        example: "sort_by(fn (a, b) => total_compare(b, a), [3, 1, 2])",
    },
    Guide {
        name: "sort_by_key",
        description: "sort_by_key(f, xs) sorts by the key f(x) of each element (stable), which is shorter than writing a full comparison with sort_by.",
        example: "sort_by_key(fn x => -x, [1, 3, 2])",
    },
    Guide {
        name: "sort_float",
        description: "sort_float(xs, nan_last) sorts Floats in ascending order, which plain sort will not do because Float has no total order. The Bool decides whether NaNs go last (true) or first.",
        example: "sort_float([3.0, nan, 1.0], true)",
    },
    Guide {
        name: "sparse_coo",
        description: "Assembles a sparse tensor from its non-zeros: sparse_coo(shape, indices, values) with one index tuple per value. Duplicate indices are summed, which is how finite-element assembly adds contributions; entries that sum to zero are dropped.",
        example: "sparse_coo([3, 3], [(0, 0), (1, 1), (2, 2)], [1.0, 2.0, 3.0])",
    },
    Guide {
        name: "sparse_csr",
        description: "Builds a compressed-sparse-row matrix from its raw arrays: sparse_csr(rows, cols, row_pointers, column_indices, values). Prefer sparse_coo unless you already hold CSR arrays.",
        example: "sparse_csr(2, 2, [0, 1, 2], [0, 1], [1.0, 2.0])",
    },
    Guide {
        name: "spawn",
        description: "Start an IO thunk as a task. Its buffered output is emitted when awaited. Work that cannot cross threads runs locally.",
        example: "let job = spawn(fn () => pure(42))\njob",
    },
    Guide {
        name: "spdiag",
        description: "A sparse square matrix with the given vector on its diagonal; elements keep their dimension.",
        example: "spdiag([1.0, 2.0, 3.0])",
    },
    Guide {
        name: "speye",
        description: "The n x n sparse identity matrix of Floats.",
        example: "speye(4)",
    },
    Guide {
        name: "spline",
        description: "Cubic-spline interpolation: spline(xs, ys, x) fits a smooth curve through the sample points (xs increasing) and evaluates it at x. Outside the range of xs the end polynomial is extended, which can drift quickly, so avoid extrapolating far.",
        example: "spline([0.0, 1.0, 2.0, 3.0], [0.0, 1.0, 4.0, 9.0], 1.5)",
    },
    Guide {
        name: "split",
        description: "split(s, sep) cuts a string at every occurrence of sep and returns the pieces as a vector of strings.",
        example: "split(\"a,b,c\", \",\")",
    },
    Guide {
        name: "spsolve",
        description: "Solves a sparse square system A x = b directly (sparse LU) and returns x, whose dimension is that of b over that of A. Use cg or gmres_with for large systems where an iterative method is cheaper.",
        example: "spsolve(speye(3), [1.0, 2.0, 3.0])",
    },
    Guide {
        name: "sqrt",
        description: "Compute a square root. Exact inputs may remain exact or symbolic; Float inputs produce numerical results. A quantity's dimension is raised to one half.",
        example: "sqrt(16.0)\nsqrt(9 m^2)",
    },
    Guide {
        name: "starts_with",
        description: "starts_with(s, prefix) is true when s begins with prefix.",
        example: "starts_with(\"hello\", \"he\")",
    },
    Guide {
        name: "std",
        description: "Computes sample standard deviation in the elements’ dimension. A singleton has zero standard deviation.",
        example: "std([1.0, 2.0, 3.0, 4.0])",
    },
    Guide {
        name: "subst",
        description: "subst(expr, var, value) replaces every occurrence of the symbol var in expr with value. The value must have the same dimension as the symbol.",
        example: "let x = symbol(\"x\")\nsubst(x^2 + 1, x, 3)",
    },
    Guide {
        name: "sum",
        description: "Sum tensor elements, or use the function-and-bounds overload for a finite summation. Empty tensor reductions return a phase-appropriate zero.",
        example: "sum([1, 2, 3])",
    },
    Guide {
        name: "sum_f",
        description: "sum_f(f, a, b) adds f(k) for the integers k from a to b using Float arithmetic. Use sum when you want an exact or symbolic result.",
        example: "sum_f(fn k => 1.0 / (k * k), 1.0, 100.0)",
    },
    Guide {
        name: "svd",
        description: "Singular value decomposition A = U S V^T. Returns the triple (U, singular values, V^T); the singular values are in descending order and keep the matrix's dimension.",
        example: "svd([[3.0, 0.0], [0.0, 4.0]])",
    },
    Guide {
        name: "sym_eq",
        description: "Builds the symbolic proposition that two exact numbers are equal. Real has no Bool equality, because equality of symbolic expressions is not always decidable; use decide to ask for a verdict.",
        example: "sym_eq(1 + 1, 2)",
    },
    Guide {
        name: "sym_lt",
        description: "Builds the symbolic proposition that the first exact number is smaller than the second. Use decide to ask for a verdict.",
        example: "sym_lt(1, 2)",
    },
    Guide {
        name: "sym_ne",
        description: "Builds the symbolic proposition that two exact numbers are different. Use decide to ask for a verdict.",
        example: "sym_ne(1, 2)",
    },
    Guide {
        name: "take",
        description: "take(n, xs) keeps the first n elements, or all of them if there are fewer. On a matrix it keeps the first n rows.",
        example: "take(2, [1, 2, 3])",
    },
    Guide {
        name: "tan",
        description: "Trigonometric function. Its argument must be dimensionless; angle units such as deg carry an exact conversion scale.",
        example: "sin(30 deg)\ncos(0.0)",
    },
    Guide {
        name: "tanh",
        description: "Hyperbolic tangent of a dimensionless argument. The result is always in (-1, 1).",
        example: "tanh(0.5)",
    },
    Guide {
        name: "tetrahedron_volume",
        description: "Computes the nonnegative volume from four rows of three finite coordinates. It cubes the coordinate dimension and is independent of vertex order. Coplanar or repeated vertices give zero. Non-finite coordinates or a non-finite computed volume raise a domain error.",
        example: "import Math.Geometry { tetrahedron_volume }\ntetrahedron_volume([[0.0 m, 0.0 m, 0.0 m], [1.0 m, 0.0 m, 0.0 m], [0.0 m, 1.0 m, 0.0 m], [0.0 m, 0.0 m, 1.0 m]])",
    },
    Guide {
        name: "time_now",
        description: "The current wall-clock time as a Float in seconds. It is an IO action because each call gives a different answer. Subtract two readings to time a computation.",
        example: "time_now()",
    },
    Guide {
        name: "to",
        description: "Choose the display unit for a quantity, preserving its coherent value. The target unit must measure the same dimension.",
        example: "to(km, 3000 m)",
    },
    Guide {
        name: "to_bcsr",
        description: "to_bcsr(a, br, bc) re-stores a sparse matrix as dense br x bc blocks. Block sizes that do not divide the matrix shape are a runtime error. Block storage pays off when non-zeros cluster.",
        example: "to_bcsr(speye(4), 2, 2)",
    },
    Guide {
        name: "to_callback",
        description: "Turns a source function into an opaque callback so it can be passed to a kernel operator that expects one (Rssn.call_ho). The function must have a concrete, fully known type. The callback is valid only for the duration of the call it is passed to.",
        example: "to_callback(fn (x: Float) => x * 2.0)",
    },
    Guide {
        name: "to_dense",
        description: "Converts a sparse tensor back to an ordinary dense tensor with explicit zeros.",
        example: "to_dense(speye(3))",
    },
    Guide {
        name: "to_device",
        description: "Places a tensor on the requested CPU device and returns it. This IO action currently supports Cpu. Use device_of to query the tensor location. Gpu and Distributed placement produce E1001 even if the RSSN host can discover a GPU.",
        example: "fn stay(m: Tensor<2, Float>): IO Tensor<2, Float> = to_device(m, device_of(m))",
    },
    Guide {
        name: "to_dynamic",
        description: "Changes the static row type of a table to Dynamic while retaining all rows, column names and unit metadata. Use column_dyn to read cells by name.",
        example: "fn loosen(t: Table<{ time: Float }>): Table<Dynamic> = to_dynamic(t)",
    },
    Guide {
        name: "to_float",
        description: "Converts a number to a Float, keeping its dimension. It returns a Result, because a value that still contains free symbols has no Float to convert to; handle the Err or unwrap it with unwrap_or.",
        example: "to_float(1 / 3)",
    },
    Guide {
        name: "to_int",
        description: "Converts a dimensionless number to an Int by truncating toward zero. Use round, floor or ceil first if you want a different rounding.",
        example: "to_int(3.7)",
    },
    Guide {
        name: "to_json",
        description: "Encodes a value as a dynamic Json value, ready for write_json or simulate. It does not check that the value can be represented; a value that cannot be encoded fails when it is used.",
        example: "to_json({ name = \"run1\", steps = 100 })",
    },
    Guide {
        name: "to_latex",
        description: "Render a value as LaTeX text. Use println to print the markup, or :latex EXPR to inspect it in the REPL.",
        example: "to_latex(1/2)\n:latex 1/2",
    },
    Guide {
        name: "to_pretty",
        description: "Return formatted, multiline text for structured values without printing it. Use println to emit the resulting String, or :pretty EXPR in the REPL.",
        example: "println(to_pretty([[1, 2], [3, 4]]))",
    },
    Guide {
        name: "to_real",
        description: "Converts an Int or Float to an exact Real, keeping its dimension. A Float converts to the exact rational it holds, so to_real(0.5) is 1/2.",
        example: "to_real(0.5)",
    },
    Guide {
        name: "to_system",
        description: "to_system(name, x) converts a quantity to its value in a unit system in which some physical constants are 1. The name must be a string literal: \"natural\", \"planck\", \"atomic\" or \"geometrized\". The dimension of x must be known at compile time.",
        example: "to_system(\"natural\", 1.0 J)",
    },
    Guide {
        name: "to_table",
        description: "Builds a table from a vector of records. Field names supply the columns and values become cell text. An element that is not a record returns Err(DataError). On success the result is Ok(table).",
        example: "to_table([{ x = 1.0 }, { x = 2.0 }])",
    },
    Guide {
        name: "to_typst",
        description: "Render a value as Typst markup text; this does not print or render a document by itself.",
        example: "println(to_typst(1/2))",
    },
    Guide {
        name: "to_value",
        description: "Wraps an Aevia value in the dynamic Value type, which is the interchange form used by Rssn.dynamic and the kernel bridge. Use from_value to convert back.",
        example: "to_value(3)",
    },
    Guide {
        name: "together",
        description: "Combines a sum of fractions over one common denominator.",
        example: "let x = symbol(\"x\")\ntogether(1 / x + 1 / (x + 1))",
    },
    Guide {
        name: "total_compare",
        description: "Compares two values with a total order, always returning an Ordering. Use it with sort_by and for deterministic ordering of sets.",
        example: "total_compare(2, 1)",
    },
    Guide {
        name: "trace",
        description: "The sum of the diagonal of a square matrix, in the elements' dimension.",
        example: "trace([[1.0, 2.0], [3.0, 4.0]])",
    },
    Guide {
        name: "transpose",
        description: "Swaps the rows and columns of a matrix. Sparse matrices stay sparse.",
        example: "transpose([[1, 2], [3, 4]])",
    },
    Guide {
        name: "tril",
        description: "The lower-triangular part of a matrix: entries above the diagonal become zero.",
        example: "tril([[1, 2], [3, 4]])",
    },
    Guide {
        name: "trim",
        description: "Removes leading and trailing whitespace from a string.",
        example: "trim(\"  hi  \")",
    },
    Guide {
        name: "triu",
        description: "The upper-triangular part of a matrix: entries below the diagonal become zero.",
        example: "triu([[1, 2], [3, 4]])",
    },
    Guide {
        name: "trunc",
        description: "Drops the fractional part, rounding toward zero: trunc(-2.7) is -2.",
        example: "trunc(-2.7)",
    },
    Guide {
        name: "type_rep",
        description: "type_rep<T>() produces an unforgeable witness of the concrete type T, used with Rssn.typed so the result of a dynamic kernel call can be checked against the type you expect. T must be fully known at that point; a generic type parameter cannot be reflected.",
        example: "type_rep<Float>()",
    },
    Guide {
        name: "uncertainty",
        description: "The uncertainty field of a measurement: its one-sigma error, in the measurement's dimension.",
        example: "uncertainty(measure(1.5 m, 0.1 m))",
    },
    Guide {
        name: "uncurry",
        description: "uncurry(f) turns a function of the form a => b => c into one that takes both arguments together.",
        example: "uncurry(fn a => fn b => a + b)(1, 2)",
    },
    Guide {
        name: "union",
        description: "The set of elements that are in either set.",
        example: "union(set_of([1, 2]), set_of([2, 3]))",
    },
    Guide {
        name: "unwrap_or",
        description: "Returns the value inside Some or Ok, or the supplied default for None or Err.",
        example: "unwrap_or(None, 0)",
    },
    Guide {
        name: "upper",
        description: "Converts a string to upper case.",
        example: "upper(\"hello\")",
    },
    Guide {
        name: "value",
        description: "The value field of a measurement, the number without its uncertainty.",
        example: "value(measure(1.5 m, 0.1 m))",
    },
    Guide {
        name: "value_in",
        description: "Inspect a quantity numerically in a chosen unit without changing its physical value. Unit and quantity dimensions must agree.",
        example: "value_in(km, 3000 m)",
    },
    Guide {
        name: "var",
        description: "Computes sample variance with divisor `n - 1` for two or more observations. A singleton has zero variance. The result dimension is the element dimension squared. A second Bool argument chooses the population variance when true.",
        example: "var([1.0, 2.0, 3.0, 4.0])\nvar([1.0, 2.0, 3.0, 4.0], true)",
    },
    Guide {
        name: "vecmat",
        description: "Row vector times matrix: for a vector of length n and an n x m matrix, the vector of m column dot-products.",
        example: "vecmat([1.0, 1.0], [[1.0, 2.0], [3.0, 4.0]])",
    },
    Guide {
        name: "wiener",
        description: "wiener(seed, channels, steps, dt) generates the increments of independent Brownian motions as a steps x channels matrix. Each entry has the dimension of the square root of dt, and the same seed reproduces the same values.",
        example: "wiener(42, 2, 5, 0.01 s)",
    },
    Guide {
        name: "with_cap",
        description: "with_cap(capabilities, thunk) runs the IO action thunk() while permitting only the listed capabilities, given as a comma-separated string such as \"fs\" or \"fs,rand\". An action that needs a capability not listed fails; nested uses intersect, and the outer set is restored afterwards. Use it to sandbox code you do not fully trust.",
        example: "with_cap(\"rand\", fn () => random())",
    },
    Guide {
        name: "write_csv",
        description: "Writes a table as comma-separated CSV and includes any column units in the header. The IO action returns Ok(()) or Err(DataError).",
        example: "fn dump(t: Table<{ x: Float }>) = write_csv(\"out.csv\", t)",
    },
    Guide {
        name: "write_file",
        description: "Writes a string to a file, replacing any existing content. It is an IO action that returns Ok(()) or Err(IOError), for example when the directory does not exist or the program lacks the file-system capability.",
        example: "write_file(\"out.txt\", \"hello\")",
    },
    Guide {
        name: "write_gif",
        description: "Writes a record of the shape read_gif returns to a GIF file. It is an IO action returning Ok(()) or Err(IOError).",
        example: "fn save_anim(movie: { frames: Tensor<1, { delay_cs: Int, rgba: Tensor<1, Int> }>, height: Int, loop_count: Int, width: Int }) = write_gif(\"anim.gif\", movie)",
    },
    Guide {
        name: "write_json",
        description: "Writes a Json value to a file. Build the value with to_json. It is an IO action returning Ok(()) or Err(DataError).",
        example: "write_json(\"out.json\", to_json([1, 2, 3]))",
    },
    Guide {
        name: "write_npy",
        description: "Writes a dimensionless Float tensor of any rank to a NumPy .npy file (little-endian float64). It is an IO action returning Ok(()) or Err(DataError).",
        example: "write_npy(\"matrix.npy\", [[1.0, 2.0], [3.0, 4.0]])",
    },
    Guide {
        name: "write_svg",
        description: "Writes SVG text to a file. It is an IO action returning Ok(()) or Err(IOError).",
        example: "write_svg(\"figure.svg\", \"<svg></svg>\")",
    },
    Guide {
        name: "xor",
        description: "True when exactly one of the two Bools is true.",
        example: "xor(true, false)",
    },
    Guide {
        name: "y1",
        description: "Bessel function of the second kind of order 1; the same as bessel_y1.",
        example: "y1(1.0)",
    },
    Guide {
        name: "zero",
        description: "The additive identity in whatever numeric type and dimension the context needs. It is the natural starting value for a fold that sums numbers.",
        example: "fold(fn (acc, x) => acc + x, zero, [1, 2, 3])",
    },
    Guide {
        name: "zeros",
        description: "zeros(n1, n2, ...) builds a tensor of zeros with one extent per argument, so zeros(3) is a vector and zeros(2, 3) a 2 x 3 matrix. The elements are Float unless you give a type argument: zeros<Int>(3) or zeros<Real>(3). When the shape is a compile-time constant zeros_of<[2, 3]>() carries it in the type.",
        example: "zeros(2, 3)\nzeros<Int>(3)",
    },
    Guide {
        name: "zeros_of",
        description: "A tensor of zeros whose shape is a type argument: zeros_of<[2, 3]>(). The shape is checked at compile time. When the shape is only known at run time use zeros.",
        example: "zeros_of<[2, 3]>()",
    },
    Guide {
        name: "zeta",
        description: "The Riemann zeta function for a dimensionless argument. zeta(2) is pi^2/6. It has a pole at 1.",
        example: "zeta(2.0)",
    },
    Guide {
        name: "zip",
        description: "Pairs the elements of two tensors by position, stopping at the shorter one: zip([1, 2], [3, 4]) is [(1, 3), (2, 4)]. Combine with map to process pairs, or use zip_with to apply a function directly.",
        example: "zip([1, 2], [3, 4])",
    },
    Guide {
        name: "zip_with",
        description: "zip_with(f, a, b) applies f to corresponding elements of two tensors of the same shape; it behaves like map2.",
        example: "zip_with(fn (x, y) => x * y, [1, 2, 3], [4, 5, 6])",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_sorted_and_has_no_duplicates() {
        for pair in GUIDES.windows(2) {
            assert!(
                pair[0].name < pair[1].name,
                "`{}` must sort before `{}`",
                pair[0].name,
                pair[1].name
            );
        }
    }

    #[test]
    fn every_guide_has_prose_and_an_example() {
        for guide in GUIDES {
            assert!(
                guide.description.trim().len() > 20,
                "{}: the description is missing or a stub",
                guide.name
            );
            assert!(
                guide.description.trim_end().ends_with(['.', ')', '!']),
                "{}: the description should be whole sentences",
                guide.name
            );
            // Types are described without an example; values and functions
            // always have one.
            let is_type = guide.name.starts_with(|c: char| c.is_ascii_uppercase());
            assert!(
                is_type || !guide.example.trim().is_empty(),
                "{}: no example",
                guide.name
            );
        }
    }

    #[test]
    fn descriptions_speak_to_the_aevia_caller() {
        for guide in GUIDES {
            if let Some(word) = find_rust_ism(guide.description) {
                panic!(
                    "{}: the description mentions `{word}`, which is Rust, not Aevia",
                    guide.name
                );
            }
        }
    }

    #[test]
    fn lookup_finds_names_and_refuses_unknown_ones() {
        assert_eq!(guide("gcd").map(|guide| guide.name), Some("gcd"));
        assert!(guide("not_a_standard_name").is_none());
    }
}
