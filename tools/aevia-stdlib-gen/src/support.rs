#[path = "docs.rs"]
#[allow(dead_code)]
pub mod docs;

use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

/// Adds the failing path and operation to an I/O diagnostic.
pub fn file_error(
    action: &str,
    path: &Path,
    error: &io::Error,
) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("{action} `{}`: {error}", path.display()),
    )
}

/// Finds regular files of the given extension in path order, skipping directory links.
///
/// # Errors
/// Returns the failing path if a directory cannot be listed or a matching entry is not a file.
pub fn regular_files(
    root: &Path,
    extension: &str,
) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(dir) = directories.pop() {
        let entries = fs::read_dir(&dir)
            .map_err(|error| file_error("cannot list directory", &dir, &error))?;
        for entry in entries {
            let entry = entry.map_err(|error| file_error("cannot list directory", &dir, &error))?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|error| file_error("cannot inspect file", &path, &error))?;
            if kind.is_dir() {
                directories.push(path);
            } else if path.extension().is_some_and(|ext| ext == extension) {
                if !kind.is_file() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "expected a regular .{extension} file at `{}`",
                            path.display()
                        ),
                    ));
                }
                files.push(path);
            }
        }
    }
    files.sort_unstable();
    Ok(files)
}

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
         # Regenerate with `bazel run //:refresh_stdlib`.\n\
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
// The support module is also included by the standard-library tests.
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
