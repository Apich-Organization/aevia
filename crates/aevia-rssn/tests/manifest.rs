//! Conformance between the manifest, the unit table and the generated module.
//!
//! None of these tests pins a count from the design standard. The manifest is
//! the source of truth — the kernel gains operators, units and constants
//! independently of this repository — so every assertion is a *relationship*
//! with the live manifest (`manifest.ops.len()`, `manifest.units.len()`, the
//! live `manifest_hash`). A number typed here would be the stale-documentation
//! failure this file exists to catch.

use aevia_rssn::{codegen, registry, unit_table};

/// §5.3.3's common non-SI units that the kernel registry lacked until
/// `rssn 3.0.0-rc.1`. They are asserted directly — not excused — because a
/// unit the design standard declares is part of the language surface: if one
/// silently stopped resolving, that is a regression, not a manifest fact.
const SPEC_NON_SI: [(&str, &str, &str); 7] = [
    // (name, dimension in the fixture's spelling, exact scale)
    ("Da", "M", "1.66053906660e-27"),
    ("kn", "L/T", "1852/3600"),
    ("mph", "L/T", "1609.344/3600"),
    ("acre", "L^2", "4046.8564224"),
    ("bit", "1", "1"),
    ("B", "1", "8"),
    ("KiB", "1", "8192"),
];

#[test]
fn every_hand_written_unit_exists_in_the_manifest() {
    // `UnitTable::si` is the hand-written test fixture of §5.3; the compiler's
    // table is built from `manifest.units` ([`aevia_rssn::unit_table`]). Every
    // unit the fixture declares must therefore be in the manifest with the
    // same dimension and scale, or the compiler and the kernel disagree about
    // a spelling. The kernel may define *more* units than §5.3 lists (CGS,
    // astronomy, …), which is not a disagreement.
    let fixture = aevia_dim::UnitTable::si();
    let manifest = unit_table();
    let mut missing = Vec::new();
    for unit in fixture.list() {
        match manifest.lookup(&unit.name) {
            Some(other) => {
                assert_eq!(
                    other.dim, unit.dim,
                    "unit `{}` has a different dimension in the manifest",
                    unit.name
                );
                assert_eq!(
                    other.scale, unit.scale,
                    "unit `{}` has a different scale in the manifest",
                    unit.name
                );
            }
            None => missing.push(unit.name.clone()),
        }
    }
    assert!(
        missing.is_empty(),
        "the kernel manifest lacks unit(s) the design standard declares: {missing:?}"
    );
}

/// The seven non-SI units of §5.3.3 that the kernel registry used to lack now
/// resolve through the manifest, with their exact definitions.
#[test]
fn the_specs_non_si_units_resolve_from_the_manifest() {
    let table = unit_table();
    let env = aevia_dim::DimEnv::si();
    for (name, dimension, scale) in SPEC_NON_SI {
        let unit = table
            .lookup(name)
            .unwrap_or_else(|| panic!("`{name}` is declared by §5.3.3 but does not resolve"));
        assert_eq!(
            unit.dim,
            env.parse(dimension).expect("a fixture dimension"),
            "unit `{name}` has dimension `{dimension}`"
        );
        assert_eq!(
            unit.scale,
            aevia_dim::Scale::parse(scale).expect("a fixture scale"),
            "unit `{name}` has scale `{scale}`"
        );
    }
    // Two of them carry the aliases the standard names them by.
    assert_eq!(table.lookup("u").map(|unit| unit.name), Some("Da".to_string()));
    assert_eq!(
        table.lookup("byte").map(|unit| unit.name),
        Some("B".to_string())
    );
}

/// The table the compiler installs *is* `manifest.units`: same count, same
/// names. A filter or a skip in `unit_defs` would silently hide a kernel unit
/// from every program.
#[test]
fn the_installed_table_is_the_manifest_unit_table() {
    let registry = registry();
    let table = unit_table();
    assert_eq!(
        table.list().len(),
        registry.units.len(),
        "the installed unit table is not the manifest's"
    );
    for unit in &registry.units {
        assert!(
            table.lookup(&unit.name).is_some(),
            "`{}` is in the manifest but not in the installed table",
            unit.name
        );
    }
    // The fixture declares a suffix of the manifest (the kernel knows more),
    // so a manifest unit outside §5.3 is expected — but a fixture unit outside
    // the manifest is not, and is tested above.
    assert!(
        registry.units.len() >= aevia_dim::UnitTable::si().list().len(),
        "the kernel manifest has fewer units than the design standard declares"
    );
}

#[test]
fn the_unicode_spellings_resolve() {
    let table = unit_table();
    assert!(table.lookup("Ω").is_some(), "Ω must resolve to ohm");
    assert!(table.lookup("Å").is_some(), "Å must resolve to angstrom");
}

/// Every spelling pair §5.3 writes with a slash (`Ω`/`ohm`, `°`/`deg`, `l`/
/// `liter`, `Da`/`u`, …) must resolve in the table the compiler installs. The
/// kernel indexes the ASCII half; the bridge adds the symbol half, so this is
/// the test that keeps the two halves together.
#[test]
fn every_spelling_the_standard_writes_resolves() {
    let table = unit_table();
    for (spelling, canonical) in [
        ("Ω", "ohm"),
        ("Å", "angstrom"),
        ("°", "deg"),
        ("degree", "deg"),
        ("′", "arcmin"),
        ("″", "arcsec"),
        ("liter", "l"),
        ("litre", "l"),
        ("u", "Da"),
        ("byte", "B"),
        ("°C", "degC"),
        ("°F", "degF"),
    ] {
        let short = table
            .lookup(spelling)
            .unwrap_or_else(|| panic!("`{spelling}` does not resolve"));
        let long = table
            .lookup(canonical)
            .unwrap_or_else(|| panic!("`{canonical}` does not resolve"));
        assert_eq!(
            (short.dim.clone(), short.scale.clone()),
            (long.dim.clone(), long.scale.clone()),
            "`{spelling}` and `{canonical}` must denote the same unit"
        );
    }
    // A bare `°` is the angle, not a temperature: 90 degrees is pi/2 radians.
    assert_eq!(
        table.lookup("°").map(|unit| unit.dim),
        Some(aevia_dim::Dim::one())
    );
}

#[test]
fn the_manifest_supplies_the_si_base_units() {
    let table = unit_table();
    for name in ["m", "kg", "s", "A", "K", "mol", "cd"] {
        assert!(table.lookup(name).is_some(), "missing base unit `{name}`");
    }
}

#[test]
fn affine_temperatures_keep_their_offset() {
    let table = unit_table();
    let celsius = table.lookup("°C").expect("°C resolves");
    assert!(celsius.affine);
    assert!(celsius.offset.is_some());
}

/// The static mirror of a dimension is the one thing the checker and the
/// kernel must agree about. A unit whose dimension does not survive
/// `convert_dim(to_dim_expr(d))` would be typed as a different quantity than
/// the one the kernel measures.
#[test]
fn every_unit_dimension_survives_the_signature_mirror() {
    for unit in &registry().units {
        let mirrored = aevia_rssn::registry::convert_dim(&aevia_rssn::registry::to_dim_expr(&unit.dim));
        assert_eq!(
            mirrored, unit.dim,
            "unit `{}` does not survive the dimension mirror",
            unit.name
        );
    }
}

/// The same for the physical constants, which are the only other place a
/// concrete dimension crosses the bridge.
#[test]
fn every_constant_dimension_survives_the_signature_mirror() {
    for constant in &registry().constants {
        let mirrored =
            aevia_rssn::registry::convert_dim(&aevia_rssn::registry::to_dim_expr(&constant.dim));
        assert_eq!(
            mirrored, constant.dim,
            "constant `{}` does not survive the dimension mirror",
            constant.symbol
        );
    }
}

#[test]
fn every_operator_with_a_signature_renders_one() {
    let registry = registry();
    let mut without = Vec::new();
    for op in registry.ops() {
        if op.signatures.is_empty() && op.source.is_exact() {
            without.push(op.name.clone());
        }
    }
    assert!(
        without.is_empty(),
        "exact signatures that render nothing: {without:?}"
    );
}

#[test]
fn the_generated_module_pins_the_manifest_hash() {
    let registry = registry();
    let text = codegen::operators(registry);
    assert!(
        text.contains(&registry.manifest_hash),
        "the generated module must record the hash it came from"
    );
}

/// A conformance report, not an assertion on the numbers: a run of this suite
/// states what the build actually carries instead of what a document remembers.
/// The only facts asserted are structural — every operator has exactly one
/// signature source, and the pinned hash is the 64-hex-char `hash_hex` form.
#[test]
fn the_manifest_shape_is_reported() {
    let registry = registry();
    let mut declared = 0;
    let mut overlay = 0;
    let mut derived = 0;
    let mut none = 0;
    for op in registry.ops() {
        match op.source {
            aevia_rssn::Source::Declared => declared += 1,
            aevia_rssn::Source::Overlay => overlay += 1,
            aevia_rssn::Source::Derived => derived += 1,
            aevia_rssn::Source::None => none += 1,
        }
    }
    eprintln!(
        "manifest: rssn {} api {} hash {} — {} operators ({} declared, {} overlay, \
         {} derived, {} dynamic-only), {} units, {} constants",
        registry.version,
        registry.api_version,
        registry.manifest_hash,
        registry.op_count(),
        declared,
        overlay,
        derived,
        none,
        registry.units.len(),
        registry.constants.len(),
    );
    assert_eq!(
        registry.op_count(),
        declared + overlay + derived + none,
        "every operator has exactly one signature source"
    );
    assert_eq!(
        registry.manifest_hash.len(),
        64,
        "the manifest hash is the 64-hex-char `hash_hex`, not `hash_u64`"
    );
}

#[test]
fn a_constant_has_a_value_and_a_dimension() {
    let registry = registry();
    let light = registry
        .constants
        .iter()
        .find(|constant| constant.symbol == "c")
        .expect("the speed of light");
    assert!(light.exact);
    assert_eq!(light.value.rat.numer().to_string(), "299792458");
}
