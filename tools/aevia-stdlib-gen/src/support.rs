#[path = "docs.rs"]
#[allow(dead_code)]
pub mod docs;

/// The `Units.Constants` module, generated from the live Kernel Manifest.
///
/// Header + the constants module aevia-rssn already generates (§13.6.5): this
/// is the same code path `aevia rssn --emit` uses, so the standard library
/// cannot grow a second constant table.
#[allow(
    clippy::single_call_fn,
    reason = "shared by the generator and standard-library tests"
)]
pub fn constants_module_source() -> String {
    let registry = aevia_rssn::registry();
    let generated = aevia_rssn::codegen::constants(registry);
    let body = generated
        .split_once("module Constants\n")
        .unwrap_or_else(|| panic!("codegen::constants did not emit a `module Constants` header"))
        .1;
    format!(
        "# Generated from the RSSN kernel manifest.\n\
         # Regenerate with `cargo run -p aevia-stdlib-gen -- --write`.\n\
         # RSSN {version}, API {api}\n\
         ## Physical constants in SI units. Each entry names its reference and\n\
         ## says whether the value is exact or measured.\n\
         ##\n\
         ## Use `import Units.Constants as C` to refer to the speed of light as\n\
         ## `C.c` and the reduced Planck constant as `C.hbar`.\n\
         module Units.Constants\n{body}",
        version = registry.version,
        api = registry.api_version,
    )
}

/// `Units.Natural`, `Units.Planck`, ...: the constants in one unit system
/// (§5.11.7), from the same manifest tables as `Units.Constants`.
// Shared with the standard-library tests by path; the generator itself does not call it.
#[allow(dead_code)]
#[allow(
    clippy::single_call_fn,
    reason = "shared by the generator and standard-library tests"
)]
pub fn system_module_source(
    path: &str,
    system: &str,
) -> String {
    let registry = aevia_rssn::registry();
    let found = registry
        .unit_system(system)
        .unwrap_or_else(|| panic!("the kernel manifest has no unit system `{system}`"));
    aevia_rssn::codegen::system_constants(registry, &found, &path.replace('/', "."))
}
