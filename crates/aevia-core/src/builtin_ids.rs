//! Dense identifiers for the builtins `builtins::call` implements.
//!
//! The interpreter dispatches builtins by name, and a `match` on a string is
//! a chain of length-and-memcmp tests, one per arm. Resolving the name to a
//! [`BuiltinId`] once turns the dispatch itself into a jump table.

#![cfg_attr(
    not(test),
    deny(
        missing_docs,
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::nursery,
        clippy::panic,
        clippy::pedantic,
        clippy::single_call_fn,
        clippy::unwrap_used
    )
)]

use crate::fxhash::FxHashMap;
use std::cell::Cell;
use std::sync::OnceLock;

macro_rules! builtin_ids {
    ($($variant:ident => $name:literal,)*) => {
        /// A builtin implemented by `builtins::call`.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        #[repr(u16)]
        pub enum BuiltinId {
            $($variant,)*
        }

        impl BuiltinId {
            /// Every id with its source-level name, in declaration order.
            pub(crate) const ALL: &'static [(&'static str, BuiltinId)] =
                &[$(($name, BuiltinId::$variant),)*];
        }
    };
}

builtin_ids! {
    Print => "print",
    Println => "println",
    Eprintln => "eprintln",
    Show => "show",
    Repr => "repr",
    ToPretty => "to_pretty",
    ToLatex => "to_latex",
    ToTypst => "to_typst",
    Sqrt => "sqrt",
    Cbrt => "cbrt",
    Exp => "exp",
    Ln => "ln",
    Log => "log",
    Log2 => "log2",
    Log10 => "log10",
    Sin => "sin",
    Cos => "cos",
    Tan => "tan",
    Asin => "asin",
    Acos => "acos",
    Atan => "atan",
    Sinh => "sinh",
    Cosh => "cosh",
    Tanh => "tanh",
    Asinh => "asinh",
    Acosh => "acosh",
    Atanh => "atanh",
    Gamma => "gamma",
    Erf => "erf",
    Erfc => "erfc",
    FresnelS => "fresnel_s",
    FresnelC => "fresnel_c",
    Atan2 => "atan2",
    Hypot => "hypot",
    Abs => "abs",
    Sign => "sign",
    Min => "min",
    Max => "max",
    Clamp => "clamp",
    IsNan => "is_nan",
    IsFinite => "is_finite",
    IsInfinite => "is_infinite",
    Round => "round",
    Floor => "floor",
    Ceil => "ceil",
    Trunc => "trunc",
    Mod => "mod",
    Rem => "rem",
    Div => "div",
    Quot => "quot",
    Gcd => "gcd",
    Lcm => "lcm",
    Factorial => "factorial",
    DoubleFactorial => "double_factorial",
    ToFloat => "to_float",
    ToReal => "to_real",
    ToInt => "to_int",
    To => "to",
    DeltaTo => "delta_to",
    ValueIn => "value_in",
    Base => "base",
    Point => "point",
    Zeros => "zeros",
    Ones => "ones",
    Fill => "fill",
    Linspace => "linspace",
    Range => "range",
    Length => "length",
    Shape => "shape",
    Rank => "rank",
    Sum => "sum",
    Mean => "mean",
    Var => "var",
    Std => "std",
    MinAll => "min_all",
    MaxAll => "max_all",
    Norm => "norm",
    Normalize => "normalize",
    Dot => "dot",
    Matmul => "matmul",
    Matvec => "matvec",
    Vecmat => "vecmat",
    Outer => "outer",
    Transpose => "transpose",
    Trace => "trace",
    Det => "det",
    Inv => "inv",
    Linsolve => "linsolve",
    Eye => "eye",
    Diag => "diag",
    Flatten => "flatten",
    At => "at",
    ParMap => "par_map",
    ParTabulate => "par_tabulate",
    ParReduce => "par_reduce",
    Pure => "pure",
    Spawn => "spawn",
    Await => "await",
    Map => "map",
    Map2 => "map2",
    ZipWith => "zip_with",
    Filter => "filter",
    Fold => "fold",
    Take => "take",
    Drop => "drop",
    Reverse => "reverse",
    ReverseAxis => "reverse_axis",
    Sort => "sort",
    Argsort => "argsort",
    SortAxis => "sort_axis",
    SortByKey => "sort_by_key",
    Concat => "concat",
    Any => "any",
    All => "all",
    Find => "find",
    Prod => "prod",
    Centroid => "centroid",
    Join => "join",
    Split => "split",
    Replace => "replace",
    Trim => "trim",
    Upper => "upper",
    Lower => "lower",
    Contains => "contains",
    StartsWith => "starts_with",
    EndsWith => "ends_with",
    ParseInt => "parse_int",
    ParseFloat => "parse_float",
    Chars => "chars",
    Format => "format",
    AndThen => "and_then",
    UnwrapOr => "unwrap_or",
    IsSome => "is_some",
    IsNone => "is_none",
    IsOk => "is_ok",
    IsErr => "is_err",
    Ok => "ok",
    OkOr => "ok_or",
    First => "first",
    Last => "last",
    Cumsum => "cumsum",
    Cumprod => "cumprod",
    Argmax => "argmax",
    Argmin => "argmin",
    MapErr => "map_err",
    Symbol => "symbol",
    Symfun => "symfun",
    Dsolve => "dsolve",
    Diff => "diff",
    DiffAt => "diff_at",
    Antiderivative => "antiderivative",
    Defint => "defint",
    Limit => "limit",
    Series => "series",
    Solve => "solve",
    Nsolve => "nsolve",
    Eval => "eval",
    Simplify => "simplify",
    Approx => "approx",
    Subst => "subst",
    ToValue => "to_value",
    ToCallback => "to_callback",
    FromValue => "from_value",
    Decide => "decide",
    Assuming => "assuming",
    ByCases => "by_cases",
    ReadFile => "read_file",
    WriteFile => "write_file",
    ReadLine => "read_line",
    Input => "input",
    LoadPlugin => "load_plugin",
    Args => "args",
    TimeNow => "time_now",
    WithCap => "with_cap",
    Random => "random",
    Rng => "rng",
    Assert => "assert",
    AssertEq => "assert_eq",
    ApproxEq => "approx_eq",
    Forall => "forall",
    Line => "line",
    Scatter => "scatter",
    Label => "label",
    Combine => "combine",
    Plot => "plot",
    Save => "save",
    PlotSaveGif => "plot_save_gif",
    Pi => "pi",
    E => "e",
    Oo => "oo",
    Nan => "nan",
    I => "i",
    Lgamma => "lgamma",
    Enumerate => "enumerate",
    Zip => "zip",
    FoldLeft => "fold_left",
    SortBy => "sort_by",
    SortFloat => "sort_float",
    Id => "id",
    Const => "const",
    Apply => "apply",
    Compose => "compose",
    Flip => "flip",
    Curry => "curry",
    Uncurry => "uncurry",
    Partial => "partial",
    Not => "not",
    Xor => "xor",
    Compare => "compare",
    PartialCompare => "partial_compare",
    TotalCompare => "total_compare",
    Hash => "hash",
    SymEq => "sym_eq",
    SymNe => "sym_ne",
    SymLt => "sym_lt",
    Zero => "zero",
    Beta => "beta",
    Zeta => "zeta",
    BesselJ0 => "bessel_j0",
    BesselJ1 => "bessel_j1",
    J1 => "j1",
    BesselY0 => "bessel_y0",
    BesselY1 => "bessel_y1",
    Y1 => "y1",
    BesselJn => "bessel_jn",
    BesselYn => "bessel_yn",
    LegendreP => "legendre_p",
    HermiteH => "hermite_h",
    LaguerreL => "laguerre_l",
    ChebyshevT => "chebyshev_t",
    ChebyshevU => "chebyshev_u",
    Complex => "complex",
    Re => "re",
    Im => "im",
    Conj => "conj",
    Magnitude => "magnitude",
    Arg => "arg",
    ComplexSqrt => "complex_sqrt",
    ComplexExp => "complex_exp",
    ComplexLn => "complex_ln",
    ComplexSin => "complex_sin",
    ComplexCos => "complex_cos",
    Factor => "factor",
    Expand => "expand",
    Cancel => "cancel",
    Together => "together",
    Apart => "apart",
    Collect => "collect",
    SetOf => "set_of",
    Elements => "elements",
    Member => "member",
    SetInsert => "set_insert",
    SetRemove => "set_remove",
    Union => "union",
    Intersection => "intersection",
    Difference => "difference",
    IsSubset => "is_subset",
    Degree => "degree",
    IsPrime => "is_prime",
    Primes => "primes",
    Binomial => "binomial",
    Permutations => "permutations",
    PolygonArea => "polygon_area",
    Distance => "distance",
    Measure => "measure",
    Value => "value",
    Uncertainty => "uncertainty",
    CompileFail => "compile_fail",
    Lstsq => "lstsq",
    Eig => "eig",
    Eigvals => "eigvals",
    Svd => "svd",
    Qr => "qr",
    Lu => "lu",
    Cholesky => "cholesky",
    Tril => "tril",
    Triu => "triu",
    Reshape => "reshape",
    ZerosOf => "zeros_of",
    OnesOf => "ones_of",
    RefineShape => "refine_shape",
    ReshapeDyn => "reshape_dyn",
    Set => "set",
    MapIndexed => "map_indexed",
    ApproxMap => "approx_map",
    DetUnit => "det_unit",
    Dyn => "dyn",
    DynWith => "dyn_with",
    Check => "check",
    ToSystem => "to_system",
    FromSystem => "from_system",
    CheckWith => "check_with",
    DimOf => "dim_of",
    ValueOf => "value_of",
    PowDyn => "pow_dyn",
    Median => "median",
    Mode => "mode",
    Quantile => "quantile",
    Cov => "cov",
    Corr => "corr",
    Skewness => "skewness",
    Kurtosis => "kurtosis",
    Histogram => "histogram",
    NumericDeriv => "numeric_deriv",
    Quadrature => "quadrature",
    Product => "product",
    ProductDyn => "product_dyn",
    SumF => "sum_f",
    Grad => "grad",
    GradF => "grad_f",
    Jacobian => "jacobian",
    Hessian => "hessian",
    Laplacian => "laplacian",
    Divergence => "divergence",
    Curl => "curl",
    SolveComplex => "solve_complex",
    NsolveF => "nsolve_f",
    Root => "root",
    SolveSystem => "solve_system",
    LeastSquares => "least_squares",
    Minimize => "minimize",
    Maximize => "maximize",
    MinimizeF => "minimize_f",
    Pdf => "pdf",
    Cdf => "cdf",
    Sample => "sample",
    SampleN => "sample_n",
    Fft => "fft",
    Ifft => "ifft",
    FftNorm => "fft_norm",
    IfftNorm => "ifft_norm",
    FftAxis => "fft_axis",
    IfftAxis => "ifft_axis",
    Fft2 => "fft2",
    Ifft2 => "ifft2",
    Fftn => "fftn",
    Ifftn => "ifftn",
    FftnAxes => "fftn_axes",
    IfftnAxes => "ifftn_axes",
    FftnNorm => "fftn_norm",
    IfftnNorm => "ifftn_norm",
    Rfft => "rfft",
    Rfft2 => "rfft2",
    Rfftn => "rfftn",
    RfftnAxes => "rfftn_axes",
    Irfft => "irfft",
    Irfft2 => "irfft2",
    Irfftn => "irfftn",
    IrfftnAxes => "irfftn_axes",
    Fftshift => "fftshift",
    Ifftshift => "ifftshift",
    FftshiftAxis => "fftshift_axis",
    IfftshiftAxis => "ifftshift_axis",
    Fftfreq => "fftfreq",
    Rfftfreq => "rfftfreq",
    Interp1 => "interp1",
    Spline => "spline",
    Interp2 => "interp2",
    Polyfit => "polyfit",
    ChebyshevFit => "chebyshev_fit",
    Polyval => "polyval",
    Odeint => "odeint",
    OdeintWith => "odeint_with",
    SolveIvp => "solve_ivp",
    OdeSolve => "ode_solve",
    OdeBvp => "ode_bvp",
    OdeintOrder => "odeint_order",
    OdeintOrderWith => "odeint_order_with",
    Wiener => "wiener",
    SdeOrder => "sde_order",
    SdeOrderWith => "sde_order_with",
    SdeEnsembleSeed => "sde_ensemble_seed",
    SdeintDiag => "sdeint_diag",
    SdeintGeneral => "sdeint_general",
    SdeAt => "sde_at",
    SdeResample => "sde_resample",
    SdeInterp => "sde_interp",
    SdeEvents => "sde_events",
    SdeEnsembleDiag => "sde_ensemble_diag",
    SdeEnsembleGeneral => "sde_ensemble_general",
    SdeMember => "sde_member",
    SolvePoisson => "solve_poisson",
    SolveHeat => "solve_heat",
    SolveWave => "solve_wave",
    SolveAdvection => "solve_advection",
    ReadNpy => "read_npy",
    WriteNpy => "write_npy",
    ReadSvg => "read_svg",
    WriteSvg => "write_svg",
    ReadGif => "read_gif",
    WriteGif => "write_gif",
    ReadCsv => "read_csv",
    ReadCsvDyn => "read_csv_dyn",
    WriteCsv => "write_csv",
    ToJson => "to_json",
    FromJson => "from_json",
    ReadJson => "read_json",
    WriteJson => "write_json",
    Column => "column",
    ColumnDyn => "column_dyn",
    Row => "row",
    Select => "select",
    Schema => "schema",
    ToTable => "to_table",
    ToDynamic => "to_dynamic",
    LineWith => "line_with",
    ScatterWith => "scatter_with",
    Hist => "hist",
    HistWith => "hist_with",
    Heatmap => "heatmap",
    HeatmapWith => "heatmap_with",
    Errorbar => "errorbar",
    ErrorbarWith => "errorbar_with",
    DefaultPlotOpts => "default_plot_opts",
    Simulate => "simulate",
    DeviceOf => "device_of",
    ToDevice => "to_device",
    On => "on",
    TypeRep => "type_rep",
    FromDense => "from_dense",
    ToDense => "to_dense",
    Nnz => "nnz",
    Density => "density",
    FormatOf => "format_of",
    Convert => "convert",
    ToBcsr => "to_bcsr",
    BlockSize => "block_size",
    SparseCoo => "sparse_coo",
    SparseCsr => "sparse_csr",
    Speye => "speye",
    Spdiag => "spdiag",
    Spsolve => "spsolve",
    Cg => "cg",
    CgWith => "cg_with",
    GmresWith => "gmres_with",
    BicgstabWith => "bicgstab_with",
    Kron => "kron",
    Contract => "contract",
    MapNonzero => "map_nonzero",
    Nonzeros => "nonzeros",
    Get => "get",
}

/// Slots in the per-thread resolution cache. A power of two.
const CACHE_SLOTS: usize = 512;

thread_local! {
    /// Direct-mapped `name address → id + 1` cache (0 is empty).
    ///
    /// Call sites name builtins with the same string every time, so the
    /// address of the name identifies the call site after the first lookup.
    /// A hit is only trusted after the cached id's own name compares equal,
    /// so a freed-and-reused address can cost a miss but never a wrong id.
    static RESOLVED: [Cell<u16>; CACHE_SLOTS] = const { [const { Cell::new(0) }; CACHE_SLOTS] };
}

impl BuiltinId {
    /// The builtin called `name`, if the runtime implements one.
    #[allow(
        clippy::single_call_fn,
        reason = "resolve source names separately from builtin execution"
    )]
    pub fn from_name(name: &str) -> Option<Self> {
        static TABLE: OnceLock<FxHashMap<&'static str, BuiltinId>> = OnceLock::new();
        let slot = ((name.as_ptr() as usize >> 3) ^ name.len()) & CACHE_SLOTS.saturating_sub(1);
        let cached = RESOLVED.with(|cache| cache.get(slot).map_or(0, Cell::get));
        if let Some(&(known, id)) = usize::from(cached)
            .checked_sub(1)
            .and_then(|index| Self::ALL.get(index))
            && known == name
        {
            return Some(id);
        }
        let id = *TABLE
            .get_or_init(|| Self::ALL.iter().copied().collect())
            .get(name)?;
        RESOLVED.with(|cache| {
            if let Some(cell) = cache.get(slot) {
                cell.set((id as u16).saturating_add(1));
            }
        });
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_resolves_to_its_own_id() {
        for (name, id) in BuiltinId::ALL {
            assert_eq!(BuiltinId::from_name(name), Some(*id), "{name}");
            // A second lookup is answered by the cache and must agree.
            assert_eq!(BuiltinId::from_name(name), Some(*id), "{name} (cached)");
        }
    }

    #[test]
    fn names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for (name, _) in BuiltinId::ALL {
            assert!(seen.insert(*name), "`{name}` is declared twice");
        }
    }

    #[test]
    fn a_reused_address_cannot_return_the_wrong_id() {
        // Two different names whose cache slots collide must each resolve to
        // their own id, however often they alternate.
        let first = String::from("sin");
        let second = String::from("cos");
        for _ in 0..4 {
            assert_eq!(
                BuiltinId::from_name(&first).map(|id| id as usize),
                BuiltinId::from_name("sin").map(|id| id as usize)
            );
            assert_ne!(BuiltinId::from_name(&first), BuiltinId::from_name(&second));
        }
    }

    #[test]
    fn unknown_names_have_no_id() {
        assert_eq!(BuiltinId::from_name("not_a_builtin"), None);
        assert_eq!(BuiltinId::from_name(""), None);
    }
}
