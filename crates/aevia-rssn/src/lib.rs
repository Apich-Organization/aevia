//! Bridge between Aevia and the rssn scientific computing kernel.
//!
//! Aevia does not re-implement computer algebra. rssn owns the term
//! representation, the rewrite engine, the dimension layer and the numeric
//! kernels; this crate owns the dynamic value bridge and manifest bindings.
//! `aevia-std` also calls plain RSSN kernels for its typed numerical adapters.
//!
//! Three layers live here:
//!
//! * [`kernel`] — the live session, the calls, and the conversion between the
//!   two value models;
//! * [`registry`] — the Kernel Manifest, indexed: operators, units,
//!   constants, capabilities;
//! * [`codegen`] — the `Rssn` module, generated from that manifest.
//!
//! ```
//! use aevia_rssn::Num;
//! use aevia_rssn::Val;
//! use aevia_rssn::kernel;
//! use aevia_rssn::registry;
//!
//! let k = kernel::kernel();
//! let value = k.call("sqrt", &[Val::Num(Num::Float(16.0))]).expect("sqrt");
//! assert_eq!(value, Val::Num(Num::Float(4.0)));
//! assert!(registry::registry().op("sin").is_some());
//! ```

#![forbid(unsafe_code)]
#![allow(clippy::module_name_repetitions)]
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

pub mod bindings;
pub mod codegen;
pub mod device;
pub mod domains;
pub mod error;
pub use rssn::execution;
/// Runtime-dispatched SIMD microkernels from RSSN (`AVX2+FMA`, `NEON`, scalar).
pub use rssn::kernels::simd;
#[cfg(feature = "jit")]
pub mod jit;
pub mod kernel;
pub mod registry;
pub mod sim;
pub mod value;
pub mod witness;

pub use error::Failure;
pub use error::KernelError;
pub use kernel::Computed;
pub use kernel::Expr;
pub use kernel::Kernel;
pub use kernel::Opts;
pub use kernel::SourceOpSignature;
pub use kernel::kernel;
pub use kernel::reset;
pub use registry::Capabilities;
pub use registry::Capability;
pub use registry::ConstantInfo;
pub use registry::OpEntry;
pub use registry::Registry;
pub use registry::Sig;
pub use registry::SigType;
pub use registry::Source;
pub use registry::UnitInfo;
pub use registry::registry;
pub use registry::resolve_key;
/// Scoped cooperative cancellation shared with the scientific kernels.
pub use rssn::cancellation;
pub use rssn::kernel::signature::OpKey;

/// Host execution capabilities probed once from the runtime environment.
#[must_use]
pub fn host_execution_environment() -> &'static execution::HostEnvironment {
    execution::host_environment()
}

/// Resolves worker count from an optional `AEVIA_JOBS`-style override, the
/// kernel's [`execution::ExecutionMode`], and the host default thread budget.
///
/// Blank, non-numeric, or zero overrides fall back to the host execution policy
/// instead of collapsing to single-threaded execution.
#[must_use]
pub fn resolve_worker_threads(
    env_override: Option<&str>,
    mode: execution::ExecutionMode,
    default_threads: usize,
) -> usize {
    if let Some(explicit) = env_override
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .and_then(|text| text.parse::<std::num::NonZeroUsize>().ok())
    {
        return explicit.get();
    }
    match mode {
        | execution::ExecutionMode::Serial => 1,
        | execution::ExecutionMode::Parallel(limit) => limit.clamp(1, default_threads.max(1)),
        | execution::ExecutionMode::Auto => default_threads.max(1),
    }
}

/// Default worker thread count honoring `AEVIA_JOBS`, `RAYON_NUM_THREADS`,
/// `RSSN_EXECUTION_MODE`, and host CPU parallelism via `rssn::execution`.
#[must_use]
pub fn default_worker_threads() -> usize {
    let env_override = std::env::var("AEVIA_JOBS").ok();
    resolve_worker_threads(
        env_override.as_deref(),
        execution::default_execution_mode(),
        execution::default_cpu_threads(),
    )
}

/// Resolves an effective [`execution::ExecutionMode`] from an explicit job
/// budget, worker-thread nesting flag, and the ambient host mode.
#[must_use]
pub const fn resolve_execution_mode(
    jobs: usize,
    on_worker: bool,
    host_mode: execution::ExecutionMode,
) -> execution::ExecutionMode {
    if on_worker || jobs <= 1 {
        return execution::ExecutionMode::Serial;
    }
    match host_mode {
        | execution::ExecutionMode::Serial => execution::ExecutionMode::Serial,
        | execution::ExecutionMode::Parallel(limit) => {
            let cap = if jobs < limit { jobs } else { limit };
            if cap <= 1 {
                execution::ExecutionMode::Serial
            } else {
                execution::ExecutionMode::Parallel(cap)
            }
        },
        | execution::ExecutionMode::Auto => execution::ExecutionMode::Parallel(jobs),
    }
}

/// Resolves the effective [`execution::ExecutionMode`] for an Aevia worker
/// budget `jobs` under [`execution::default_execution_mode`].
#[must_use]
pub fn execution_mode_for_budget(
    jobs: usize,
    on_worker: bool,
) -> execution::ExecutionMode {
    resolve_execution_mode(jobs, on_worker, execution::default_execution_mode())
}

/// Whether `items` exceeds `minimum` and `mode` allows parallel execution.
#[must_use]
pub fn should_split_for_mode(
    items: usize,
    minimum: usize,
    mode: execution::ExecutionMode,
) -> bool {
    match mode {
        | execution::ExecutionMode::Serial => false,
        | execution::ExecutionMode::Parallel(limit) if limit <= 1 => false,
        | execution::ExecutionMode::Auto | execution::ExecutionMode::Parallel(_) => {
            execution::should_split(items, minimum)
        },
    }
}

/// Default chunk size for SIMD + Rayon elementwise and reduction passes.
const SIMD_PAR_CHUNK: usize = 8_192;
/// Minimum element count before splitting a SIMD-backed reduction or binary pass.
const SIMD_PAR_MIN: usize = 32_768;

/// Human-readable label for the host's runtime-detected SIMD instruction set.
#[must_use]
pub fn host_simd_label() -> &'static str {
    let simd = host_execution_environment().simd();
    if simd.avx512f {
        "AVX-512"
    } else if simd.avx2 && simd.fma {
        "AVX2+FMA"
    } else if simd.avx2 {
        "AVX2"
    } else if simd.sse42 {
        "SSE4.2"
    } else if simd.neon {
        "NEON"
    } else {
        "scalar"
    }
}

/// Runs `f` within the thread budget specified by `mode`.
pub fn run_in_mode<R, F>(
    mode: execution::ExecutionMode,
    f: F,
) -> R
where
    R: Send,
    F: FnOnce() -> R + Send,
{
    match mode {
        | execution::ExecutionMode::Serial => f(),
        | execution::ExecutionMode::Auto => execution::with_scoped_pool(None, f),
        | execution::ExecutionMode::Parallel(limit) => {
            if limit <= 1 {
                f()
            } else {
                execution::with_scoped_pool(Some(limit), f)
            }
        },
    }
}

/// Processes `items` in mutable chunks of `chunk_size`, in parallel when
/// `items.len() >= min_items` and `mode` allows parallel execution.
pub fn par_for_chunks_mut_in_mode<T, F>(
    items: &mut [T],
    chunk_size: usize,
    min_items: usize,
    mode: execution::ExecutionMode,
    f: F,
) where
    T: Send,
    F: Fn(usize, &mut [T]) + Sync + Send,
{
    let chunk = chunk_size.max(1);
    if !should_split_for_mode(items.len(), min_items, mode) {
        for (index, slice) in items.chunks_mut(chunk).enumerate() {
            f(index.saturating_mul(chunk), slice);
        }
        return;
    }
    let min_chunks = min_items.div_ceil(chunk).max(2);
    run_in_mode(mode, || {
        rssn::kernels::par::for_chunks_mut(items, chunk, min_chunks, |chunk_index, slice| {
            f(chunk_index.saturating_mul(chunk), slice);
        });
    });
}

/// Elementwise binary `f64` operation dispatched to runtime SIMD microkernels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimdBinaryOp {
    /// `a[i] + b[i]`.
    Add,
    /// `a[i] - b[i]`.
    Sub,
    /// `a[i] * b[i]`.
    Mul,
}

/// Computes `out[i] = op(a[i], b[i])` over `0..a.len().min(b.len())` using
/// runtime-dispatched SIMD (`AVX2+FMA`/`NEON`) and parallel chunks when
/// `mode` allows and the slice is large enough.
#[must_use]
pub fn simd_binary_f64_in_mode(
    op: SimdBinaryOp,
    a: &[f64],
    b: &[f64],
    mode: execution::ExecutionMode,
) -> Vec<f64> {
    let n = a.len().min(b.len());
    let mut out = vec![0.0; n];
    par_for_chunks_mut_in_mode(
        &mut out,
        SIMD_PAR_CHUNK,
        SIMD_PAR_MIN,
        mode,
        |offset, dst| {
            let end = offset.saturating_add(dst.len());
            if let (Some(sa), Some(sb)) = (a.get(offset..end), b.get(offset..end)) {
                match op {
                    | SimdBinaryOp::Add => simd::add_into_f64(sa, sb, dst),
                    | SimdBinaryOp::Sub => simd::sub_into_f64(sa, sb, dst),
                    | SimdBinaryOp::Mul => simd::hadamard_mul_into_f64(sa, sb, dst),
                }
            }
        },
    );
    out
}

/// Computes `out[i] = a[i] * scale` using runtime-dispatched SIMD and parallel
/// chunks when `mode` allows and `a` is large enough.
#[must_use]
pub fn simd_scale_f64_in_mode(
    a: &[f64],
    scale: f64,
    mode: execution::ExecutionMode,
) -> Vec<f64> {
    let mut out = a.to_vec();
    par_for_chunks_mut_in_mode(&mut out, SIMD_PAR_CHUNK, SIMD_PAR_MIN, mode, |_, dst| {
        simd::scale_f64(scale, dst);
    });
    out
}

/// Deterministic Neumaier-compensated sum of `items`, bit-identical across
/// serial and parallel modes.
#[must_use]
pub fn par_sum_kahan_f64_in_mode(
    items: &[f64],
    mode: execution::ExecutionMode,
) -> f64 {
    const BLOCK: usize = 4_096;
    if !should_split_for_mode(items.len(), SIMD_PAR_MIN, mode) {
        return rssn::kernels::par::sum_kahan_fixed(items.len(), BLOCK, usize::MAX, |i| {
            items.get(i).copied().unwrap_or(0.0)
        });
    }
    run_in_mode(mode, || {
        rssn::kernels::par::sum_kahan_fixed(items.len(), BLOCK, SIMD_PAR_MIN, |i| {
            items.get(i).copied().unwrap_or(0.0)
        })
    })
}

/// Deterministic SIMD + parallel dot product of `a` and `b`.
#[must_use]
pub fn par_dot_f64_in_mode(
    a: &[f64],
    b: &[f64],
    mode: execution::ExecutionMode,
) -> f64 {
    const BLOCK: usize = 4_096;
    let n = a.len().min(b.len());
    if !should_split_for_mode(n, SIMD_PAR_MIN, mode) {
        return rssn::kernels::par::dot_fixed(a, b, BLOCK, usize::MAX);
    }
    run_in_mode(mode, || {
        rssn::kernels::par::dot_fixed(a, b, BLOCK, SIMD_PAR_MIN)
    })
}

/// Deterministic overflow-safe Euclidean (`L2`) norm of `items`.
#[must_use]
pub fn par_norm2_f64_in_mode(
    items: &[f64],
    mode: execution::ExecutionMode,
) -> f64 {
    const BLOCK: usize = 4_096;
    if !should_split_for_mode(items.len(), SIMD_PAR_MIN, mode) {
        return rssn::kernels::par::norm2_fixed(items, BLOCK, usize::MAX);
    }
    run_in_mode(mode, || {
        rssn::kernels::par::norm2_fixed(items, BLOCK, SIMD_PAR_MIN)
    })
}

/// Computes `(min, max)` over `items` using runtime-dispatched SIMD (and
/// parallel chunks for large slices), propagating `NaN` if any element is `NaN`.
#[must_use]
pub fn simd_min_max_f64_in_mode(
    items: &[f64],
    mode: execution::ExecutionMode,
) -> Option<(f64, f64)> {
    if items.is_empty() {
        return None;
    }
    if !should_split_for_mode(items.len(), SIMD_PAR_MIN, mode) {
        return simd::min_max_f64(items);
    }
    let chunk_count = items.len().div_ceil(SIMD_PAR_CHUNK);
    let partials = run_in_mode(mode, || {
        rssn::kernels::par::map_indexed(chunk_count, 2, |c| {
            let start = c.saturating_mul(SIMD_PAR_CHUNK);
            let end = start.saturating_add(SIMD_PAR_CHUNK).min(items.len());
            items.get(start..end).and_then(simd::min_max_f64)
        })
    });
    let mut iter = partials.into_iter().flatten();
    let (mut lo, mut hi) = iter.next()?;
    for (c_lo, c_hi) in iter {
        lo = if lo.is_nan() || c_lo.is_nan() {
            f64::NAN
        } else {
            lo.min(c_lo)
        };
        hi = if hi.is_nan() || c_hi.is_nan() {
            f64::NAN
        } else {
            hi.max(c_hi)
        };
    }
    Some((lo, hi))
}

/// Maps `items` through `f` in index order while honoring `mode`,
/// `HostEnvironment`, and cooperative cancellation scopes.
pub fn par_map_f64_slice<F>(
    items: &[f64],
    grain: usize,
    mode: execution::ExecutionMode,
    f: F,
) -> Vec<f64>
where
    F: Fn(f64) -> f64 + Sync + Send,
{
    if !should_split_for_mode(items.len(), grain, mode) {
        return items.iter().copied().map(f).collect();
    }
    run_in_mode(mode, || {
        rssn::kernels::par::map_slice(items, grain, |&x| f(x))
    })
}

/// The normalized Fresnel integrals `S(x) = ∫₀ˣ sin(πt²/2) dt` and
/// `C(x) = ∫₀ˣ cos(πt²/2) dt` (DLMF 7.2.7), from the kernel's numerics.
#[must_use]
pub fn fresnel_s(x: f64) -> f64 {
    rssn::kernels::fresnel::fresnel_s(x)
}

/// See [`fresnel_s`].
#[must_use]
pub fn fresnel_c(x: f64) -> f64 {
    rssn::kernels::fresnel::fresnel_c(x)
}
pub use value::Num;
pub use value::SparseKind;
pub use value::Sym;
pub use value::UnitVal;
pub use value::Val;

/// The dynamic ABI version this build understands.
pub const SUPPORTED_API_VERSION: u32 = 2;

/// Checks that a manifest can be consumed by this compiler.
///
/// # Errors
///
/// `E1004` when the ABI version differs from the one the bridge was written
/// against.
pub fn check_compatibility(registry: &Registry) -> Result<(), KernelError> {
    if registry.api_version != SUPPORTED_API_VERSION {
        return Err(KernelError::new(
            "E1004",
            format!(
                "rssn speaks api {} but this build of Aevia understands api {}",
                registry.api_version, SUPPORTED_API_VERSION
            ),
        )
        .with_help("rebuild Aevia against the matching rssn release"));
    }
    Ok(())
}

/// Spellings the kernel does not index, mapped to the name it uses.
///
/// The kernel names units in ASCII (`ohm`, `angstrom`, `deg`, `arcmin`,
/// `arcsec`); the specification writes the symbols. The table stays the
/// kernel's — this only adds spellings §5.3 declares, never a unit.
const SPELLINGS: [(&str, &str); 7] = [
    ("Ω", "ohm"),
    ("Å", "angstrom"),
    ("µ", "u"),
    ("°", "deg"),
    ("degree", "deg"),
    ("′", "arcmin"),
    ("″", "arcsec"),
];

/// The kernel's unit table, as Aevia unit definitions.
///
/// This is the only place the unit table is built; the compiler installs it
/// into its `UnitTable` and the runtime looks units up there, so a unit added
/// to rssn appears without a change here.
#[must_use]
pub fn unit_defs() -> Vec<aevia_dim::UnitDef> {
    registry()
        .units
        .iter()
        .map(|unit| {
            // The display spelling is a spelling users write: `25.0 °C`, not
            // `25.0 degC`. Index it too when it differs from the name.
            let mut aliases = unit.aliases.clone();
            if unit.display != unit.name && !aliases.contains(&unit.display) {
                aliases.push(unit.display.clone());
            }
            for (spelling, name) in SPELLINGS {
                if unit.name == name && !aliases.iter().any(|alias| alias == spelling) {
                    aliases.push((*spelling).to_string());
                }
            }
            aevia_dim::UnitDef {
                name: unit.name.clone(),
                aliases,
                dim: unit.dim.clone(),
                scale: unit.scale.clone(),
                offset: unit.offset.clone(),
                prefixable: unit.prefixable,
                affine: unit.affine,
                display: unit.display.clone(),
                category: unit.category.clone(),
            }
        })
        .collect()
}

/// The kernel's unit table.
#[must_use]
pub fn unit_table() -> aevia_dim::UnitTable {
    aevia_dim::UnitTable::from_defs(unit_defs())
}

/// Formats a float the way Aevia's `show` does.
#[must_use]
pub fn format_float(value: f64) -> String {
    if !value.is_finite() {
        return if value.is_nan() {
            "NaN".to_string()
        } else if value > 0.0 {
            "∞".to_string()
        } else {
            "-∞".to_string()
        };
    }
    if value.fract().classify() == std::num::FpCategory::Zero && value.abs() < 1e16 {
        return format!("{value:.1}");
    }
    let mut text = format!("{value}");
    if !text.contains('.') && !text.contains('e') && !text.contains('E') {
        text.push_str(".0");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_is_reachable() {
        let registry = registry::registry();
        assert_eq!(registry.api_version, SUPPORTED_API_VERSION);
        assert!(
            registry.ops().count() > 1000,
            "expected the full operator set"
        );
    }

    #[test]
    fn a_capability_is_answered_from_the_manifest() {
        let registry = registry::registry();
        assert!(registry.capabilities.has("core.dynamic_call"));
        assert_eq!(
            registry.capabilities.has("core.jit"),
            registry.capabilities.jit
        );
    }

    #[test]
    fn the_operator_count_is_the_manifest_count() {
        let registry = registry::registry();
        assert_eq!(
            registry.op_count(),
            kernel::kernel().manifest().ops.len(),
            "aliases are spellings, not operators"
        );
        assert_eq!(registry.ops().count(), registry.op_count());
    }

    #[test]
    fn floats_print_with_a_decimal_point() {
        assert_eq!(format_float(4.0), "4.0");
        assert_eq!(format_float(0.5), "0.5");
    }

    #[test]
    fn worker_sizing_honors_execution_mode_and_rejects_zero_overrides() {
        use execution::ExecutionMode;
        assert_eq!(resolve_worker_threads(None, ExecutionMode::Auto, 6), 6);
        assert_eq!(resolve_worker_threads(Some("0"), ExecutionMode::Auto, 6), 6);
        assert_eq!(
            resolve_worker_threads(Some("bad"), ExecutionMode::Auto, 6),
            6
        );
        assert_eq!(resolve_worker_threads(Some(""), ExecutionMode::Auto, 6), 6);
        assert_eq!(resolve_worker_threads(Some("3"), ExecutionMode::Auto, 6), 3);
        assert_eq!(
            resolve_worker_threads(None, ExecutionMode::Parallel(4), 8),
            4
        );
        assert_eq!(resolve_worker_threads(None, ExecutionMode::Serial, 8), 1);
    }

    #[test]
    fn execution_mode_resolution_caps_workers_and_forces_serial_on_worker() {
        use execution::ExecutionMode;
        assert_eq!(
            resolve_execution_mode(1, false, ExecutionMode::Auto),
            ExecutionMode::Serial
        );
        assert_eq!(
            resolve_execution_mode(8, true, ExecutionMode::Auto),
            ExecutionMode::Serial
        );
        assert_eq!(
            resolve_execution_mode(8, false, ExecutionMode::Serial),
            ExecutionMode::Serial
        );
        assert_eq!(
            resolve_execution_mode(8, false, ExecutionMode::Parallel(4)),
            ExecutionMode::Parallel(4)
        );
        assert_eq!(
            resolve_execution_mode(2, false, ExecutionMode::Parallel(4)),
            ExecutionMode::Parallel(2)
        );
        assert_eq!(
            resolve_execution_mode(3, false, ExecutionMode::Auto),
            ExecutionMode::Parallel(3)
        );
    }

    #[test]
    fn par_map_f64_slice_matches_serial_and_parallel_modes() {
        use execution::ExecutionMode;
        let input: Vec<f64> = (0..5000).map(|i| f64::from(i) * 0.25).collect();
        let serial = par_map_f64_slice(&input, 64, ExecutionMode::Serial, |x| x * 2.0 + 1.0);
        let parallel = par_map_f64_slice(&input, 64, ExecutionMode::Parallel(2), |x| x * 2.0 + 1.0);
        assert_eq!(serial, parallel);
        assert!(!should_split_for_mode(10_000, 64, ExecutionMode::Serial));
        assert!(!should_split_for_mode(
            10_000,
            64,
            ExecutionMode::Parallel(1)
        ));
    }

    #[test]
    fn simd_and_parallel_reductions_are_bit_identical_across_modes() {
        use execution::ExecutionMode;
        let n = 40_000;
        let a: Vec<f64> = (0..n)
            .map(|i| f64::from(i as u32) * 0.125 - 100.0)
            .collect();
        let b: Vec<f64> = (0..n)
            .map(|i| f64::from((i % 97) as u32) * 0.5 + 1.0)
            .collect();

        for op in [SimdBinaryOp::Add, SimdBinaryOp::Sub, SimdBinaryOp::Mul] {
            let s = simd_binary_f64_in_mode(op, &a, &b, ExecutionMode::Serial);
            let p = simd_binary_f64_in_mode(op, &a, &b, ExecutionMode::Parallel(2));
            assert_eq!(s, p);
        }
        assert_eq!(
            simd_scale_f64_in_mode(&a, 3.5, ExecutionMode::Serial),
            simd_scale_f64_in_mode(&a, 3.5, ExecutionMode::Parallel(2)),
        );
        assert_eq!(
            par_sum_kahan_f64_in_mode(&a, ExecutionMode::Serial).to_bits(),
            par_sum_kahan_f64_in_mode(&a, ExecutionMode::Parallel(2)).to_bits(),
        );
        assert_eq!(
            par_dot_f64_in_mode(&a, &b, ExecutionMode::Serial).to_bits(),
            par_dot_f64_in_mode(&a, &b, ExecutionMode::Parallel(2)).to_bits(),
        );
        assert_eq!(
            par_norm2_f64_in_mode(&a, ExecutionMode::Serial).to_bits(),
            par_norm2_f64_in_mode(&a, ExecutionMode::Parallel(2)).to_bits(),
        );
        assert_eq!(
            simd_min_max_f64_in_mode(&a, ExecutionMode::Serial),
            simd_min_max_f64_in_mode(&a, ExecutionMode::Parallel(2)),
        );
        let mut with_nan = a.clone();
        with_nan[n / 2] = f64::NAN;
        let (lo, hi) = simd_min_max_f64_in_mode(&with_nan, ExecutionMode::Parallel(2)).unwrap();
        assert!(lo.is_nan() && hi.is_nan());
        assert!(!host_simd_label().is_empty());
    }
}
