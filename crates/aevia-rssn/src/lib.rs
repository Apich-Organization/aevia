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

pub mod bindings;
pub mod codegen;
pub mod device;
pub mod domains;
pub mod error;
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
pub use rssn::kernel::signature::OpKey;

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
    if value == value.trunc() && value.abs() < 1e16 {
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
}
