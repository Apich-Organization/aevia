//! The standard-library module tree of §8.20, embedded as source.
//!
//! Chapter 8's names are builtins, so a program can use them unqualified
//! without importing anything. §8.20 still defines a *module hierarchy*
//! (`Math`, `Units.SI`, `Data.Csv`, …), and the specification's own programs
//! write `import Math { sqrt }` and `open Units.SI`. The module sources are
//! therefore shipped with this crate and embedded here: the loader can fall
//! back to them when a path does not resolve on disk (see
//! `aevia_core::driver`), so the module system stays the existing one-file-per-
//! module system — there is no second module mechanism for the standard
//! library.
//!
//! All checked-in `.aev` sources are generated. Chapter-8 exports and selected
//! constructible types come from the specification; the catalog and generator
//! live in `aevia-stdlib-model`. `Units.Constants` comes from the RSSN manifest. Run
//! `cargo run -p aevia-stdlib-gen -- --write` to regenerate,
//! or pass `--check` to verify without modifying files.


/// One embedded module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Module {
    /// The module path with `/` between components (`Math/Special`), matching
    /// the file path under the standard-library root.
    pub path: &'static str,
    /// The source, as shipped.
    pub source: &'static str,
}

pub use aevia_stdlib_model::stdlib::ModuleDef;
pub use aevia_stdlib_model::stdlib::TREE;
pub use aevia_stdlib_model::stdlib::spec_module_sources;

include!("stdlib_table.rs");

/// Every embedded module, in catalog order.
#[must_use]
pub fn modules() -> &'static [Module] {
    MODULES
}

/// The module paths, `/` separated.
pub fn module_paths() -> impl Iterator<Item = &'static str> {
    modules().iter().map(|module| module.path)
}

/// The embedded source of module `path` (`["Units", "SI"]` → `Units/SI.aev`).
#[must_use]
pub fn source(path: &[&str]) -> Option<&'static str> {
    modules()
        .iter()
        .find(|module| module.path == path.join("/"))
        .map(|module| module.source)
}

/// Whether `path` names one of the standard-library modules.
#[must_use]
pub fn is_module_name(path: &[&str]) -> bool {
    source(path).is_some()
}

/// The top-level names a module exports, as written in its source.
///
/// This parses the small, declarative form the tree uses (`module …`, `let x =
/// …`, `fn f(…)`, `type T…`); it is not a general declaration parser, and does
/// not need to be: the source is generated in exactly that shape and the
/// conformance test checks the two agree.
#[must_use]
pub fn exports(path: &[&str]) -> Vec<&'static str> {
    let Some(source) = source(path) else {
        return Vec::new();
    };
    let mut out: Vec<&'static str> = Vec::new();
    let mut in_sum = false;
    for line in source.lines() {
        let line = line.trim_start();
        if line.starts_with('#') {
            continue;
        }
        let variants = if let Some(rest) = line.strip_prefix("type ") {
            let rhs = rest
                .split_once('=')
                .map(|(_, rhs)| rhs.trim_start())
                .unwrap_or("");
            in_sum = rhs.is_empty() || rhs.starts_with('|');
            if in_sum { rhs } else { "" }
        } else if in_sum && line.starts_with('|') {
            line
        } else {
            in_sum = false;
            ""
        };
        for variant in variants.split('|').skip(1) {
            let variant = variant.trim_start();
            let end = variant
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                .unwrap_or(variant.len());
            if end > 0 {
                let name = &variant[..end];
                if !out.contains(&name) {
                    out.push(name);
                }
            }
        }
        let Some(rest) = line
            .strip_prefix("let ")
            .or_else(|| line.strip_prefix("fn "))
            .or_else(|| line.strip_prefix("type "))
        else {
            continue;
        };
        let end = rest
            .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
            .unwrap_or(rest.len());
        if end > 0 {
            let name = &rest[..end];
            if !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

/// A lookup from module path to source, for the loader fallback.
#[must_use]
pub fn source_by_path(path: &str) -> Option<&'static str> {
    modules()
        .iter()
        .find(|module| module.path == path)
        .map(|module| module.source)
}

/// The module path a file path is expected to carry: `A/B.aev` → `A/B`.
///
/// The loader uses this to keep a synthetic path for a module that came from
/// the embedded tree.
#[must_use]
pub fn path_from_file(file: &str) -> Option<&'static str> {
    module_paths().find(|path| {
        let mut expected = String::from(*path);
        expected.push_str(".aev");
        expected == file
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec;

    #[test]
    fn every_tree_row_is_embedded() {
        for def in TREE {
            assert!(
                modules().iter().any(|module| module.path == def.path),
                "`{}` is in the tree but not embedded",
                def.path
            );
        }
        assert_eq!(
            TREE.len(),
            modules().len(),
            "the tree and the table disagree"
        );
    }

    #[test]
    fn constructible_types_follow_the_specification() {
        let spec = std::fs::read_to_string(spec_path()).expect("spec/08-stdlib.md");
        let changed = spec.replace("| Rk4 | Dopri5", "| Rk4 | NewMethod | Dopri5");
        let generated = spec_module_sources(&changed);
        let ode = &generated.iter().find(|(path, _)| path == "Ode").unwrap().1;
        assert!(ode.contains("| NewMethod |"));
        assert!(ode.contains("type OdeOpts<s>"));
        let sde = &generated.iter().find(|(path, _)| path == "Sde").unwrap().1;
        assert!(sde.contains("    | EulerMaruyama\n    | Milstein\n"));
        assert!(sde.contains("| GeneralNoncommutative(Int)"));
        assert!(sde.contains("type SdeOpts<T>"));
        assert!(!sde.contains("type SdePath"));
        assert_eq!(generated.len() + 1, TREE.len());
    }

    #[test]
    #[should_panic(expected = "which is absent from chapter 8")]
    fn split_exports_must_exist_in_specification() {
        let spec = std::fs::read_to_string(spec_path()).expect("spec/08-stdlib.md");
        let changed = spec.replace("factor", "missing_factor");
        assert!(!spec::names(&changed).contains(&"factor".to_string()));
        let _ = spec_module_sources(&changed);
    }

    #[test]
    fn exports_include_variant_constructors() {
        let ode = exports(&["Ode"]);
        assert!(ode.contains(&"OdeMethod") && ode.contains(&"Rk4"));
        let sde = exports(&["Sde"]);
        assert!(sde.contains(&"General") && sde.contains(&"Adaptive"));
        assert!(!exports(&["Physics", "Ising"]).contains(&"Ok"));
    }

    #[test]
    fn generated_modules_are_self_aliases() {
        let spec = std::fs::read_to_string(spec_path()).expect("spec/08-stdlib.md");
        let generated = spec_module_sources(&spec);
        assert!(!generated.is_empty());
        for (path, source) in generated {
            for line in source.lines() {
                if let Some(rest) = line.strip_prefix("let ") {
                    let (name, value) = rest
                        .split_once(" = ")
                        .unwrap_or_else(|| panic!("malformed alias in `{path}`: {line}"));
                    assert_eq!(name, value, "`{path}` has a non-self alias: {line}");
                }
                if let Some(rest) = line
                    .strip_prefix("type ")
                    .filter(|_| path != "Ode" && path != "Sde" && !path.starts_with("Physics/"))
                {
                    let (head, alias) = rest
                        .split_once(" = ")
                        .unwrap_or_else(|| panic!("malformed type alias in `{path}`: {line}"));
                    assert_eq!(head, alias, "`{path}` has a non-self type alias: {line}");
                }
            }
        }
    }

    fn spec_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/08-stdlib.md")
    }
}
