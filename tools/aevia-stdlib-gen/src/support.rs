/// The `Units.Constants` module, generated from the live Kernel Manifest.
///
/// Header + the constants module aevia-rssn already generates (§13.6.5): this
/// is the same code path `aevia rssn --emit` uses, so the standard library
/// cannot grow a second constant table.
pub fn constants_module_source() -> String {
    let registry = aevia_rssn::registry();
    let generated = aevia_rssn::codegen::constants(registry);
    let body = generated
        .split_once("module Constants\n")
        .unwrap_or_else(|| panic!("codegen::constants did not emit a `module Constants` header"))
        .1;
    format!(
        "## The `Units.Constants` module of §8.20.\n\
         ##\n\
         ## Generated from the rssn Kernel Manifest: one quantity per physical\n\
         ## constant of `manifest.constants` (§13.6.5). Do not edit; regenerate with\n\
         ## `cargo run -p aevia-stdlib-gen -- --write`.\n\
         ## rssn {version}, api {api}\n\
         module Units.Constants\n{body}",
        version = registry.version,
        api = registry.api_version,
    )
}

/// `Units.Natural`, `Units.Planck`, ...: the constants in one unit system
/// (§5.11.7), from the same manifest tables as `Units.Constants`.
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
