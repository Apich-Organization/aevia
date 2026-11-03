//! Conformance between the manifest, the unit table and the generated module.

use aevia_rssn::{codegen, registry, unit_table};

/// Units the kernel does not define. They are listed rather than ignored: a
/// spelling that silently stopped resolving would be a regression, and a new
/// one here means rssn gained a unit this fixture should track.
const NOT_IN_THE_KERNEL: [&str; 7] = ["Da", "kn", "mph", "acre", "bit", "B", "KiB"];

#[test]
fn every_hand_written_unit_exists_in_the_manifest() {
    // `UnitTable::si` is a test fixture now; this keeps it honest. A unit that
    // exists there but not in the kernel would mean the compiler and the
    // kernel disagree about a spelling.
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
    for name in &missing {
        assert!(
            NOT_IN_THE_KERNEL.contains(&name.as_str()),
            "`{name}` is defined in the fixture but not in the kernel"
        );
    }
    // A list that names one unit twice would excuse two regressions at once.
    let mut unique = NOT_IN_THE_KERNEL.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        NOT_IN_THE_KERNEL.len(),
        "the exception list repeats a unit"
    );
}

#[test]
fn the_unicode_spellings_resolve() {
    let table = unit_table();
    assert!(table.lookup("Ω").is_some(), "Ω must resolve to ohm");
    assert!(table.lookup("Å").is_some(), "Å must resolve to angstrom");
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
