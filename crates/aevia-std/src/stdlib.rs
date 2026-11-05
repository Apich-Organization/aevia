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
//! # What is generated and what is written by hand
//!
//! * Modules whose contents are "the names §8.x declares" are **generated from
//!   `spec/08-stdlib.md`** by [`spec_module_sources`]. Each declaration is a
//!   self-alias (`let sqrt = sqrt`, and `type Table<a> = Table<a>` for the
//!   parser-special `Table` type), so a module re-exports exactly the names of
//!   its section and nothing else.
//! * `Units/*` and `Physics/Ising`, `Physics/Geodesic` are **written by hand**:
//!   units are ambient `Unit[d]` values from the kernel's unit table (§5), and
//!   the physics wrappers are real typed façades over the `simulate` entry
//!   point (§8.17).
//!
//! [`spec_module_sources`] is the generator; the test
//! `tests/stdlib_modules.rs` re-runs it against the checked-in specification
//! and compares, so the embedded sources cannot silently drift from the
//! document (and cannot be hand-edited into passing).

use crate::spec;
use std::collections::BTreeMap;

/// One embedded module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Module {
    /// The module path with `/` between components (`Math/Special`), matching
    /// the file path under the standard-library root.
    pub path: &'static str,
    /// The source, as shipped.
    pub source: &'static str,
}

/// One row of the module tree of §8.20.
///
/// A generated row names the section(s) it re-exports (`sections`), or — for a
/// section the specification splits across modules (§8.6, §8.7.1, §8.8, §8.11,
/// §8.16) — the explicit names. `types` lists parser-special type names that
/// can be re-exported with an alias (only `Table` today); other chapter-8 types
/// are either not nameable in the checker or would be a self-recursive alias
/// (see the report in `tests/stdlib_modules.rs`).
pub struct ModuleDef {
    /// Module path, `/` separated.
    pub path: &'static str,
    /// Sections of chapter 8 whose declarations this module re-exports.
    pub sections: &'static [&'static str],
    /// Explicit names, for sections split across several modules.
    pub names: &'static [&'static str],
    /// Parser-special types re-exported by alias: `(name, arity)`.
    pub types: &'static [(&'static str, usize)],
    /// True when the source is written by hand rather than generated.
    pub hand_written: bool,
}

/// The §8.20 module tree, in the order the design standard lists it.
///
/// `Prelude` is listed even though it is imported automatically: the module
/// exists so `import Prelude` resolves, and it is the one place the tree says
/// what "automatically imported" means.
pub const TREE: &[ModuleDef] = &[
    ModuleDef {
        path: "Prelude",
        sections: &[
            "8.1", "8.1.1", "8.1.2", "8.1.3", "8.1.4", "8.1.5", "8.1.6", "8.1.7",
        ],
        names: &[],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Math",
        sections: &["8.2", "8.2.1", "8.3"],
        names: &[],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Math/Special",
        sections: &["8.2.2"],
        names: &[],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Math/Complex",
        sections: &["8.15"],
        names: &[],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Math/NumberTheory",
        sections: &[],
        names: &["factor", "is_prime", "primes", "gcd", "lcm"],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Math/Combinatorics",
        sections: &[],
        names: &["binomial", "permutations"],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Math/Geometry",
        sections: &[],
        names: &["polygon_area", "distance"],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Units/SI",
        sections: &[],
        names: &[],
        types: &[],
        hand_written: true,
    },
    ModuleDef {
        path: "Units/Imperial",
        sections: &[],
        names: &[],
        types: &[],
        hand_written: true,
    },
    ModuleDef {
        path: "Units/Astronomy",
        sections: &[],
        names: &[],
        types: &[],
        hand_written: true,
    },
    ModuleDef {
        path: "Units/CGS",
        sections: &[],
        names: &[],
        types: &[],
        hand_written: true,
    },
    ModuleDef {
        path: "Units/Natural",
        sections: &[],
        names: &[],
        types: &[],
        hand_written: true,
    },
    ModuleDef {
        path: "Units/Constants",
        sections: &[],
        names: &[],
        types: &[],
        hand_written: true,
    },
    ModuleDef {
        path: "LinearAlgebra",
        sections: &["8.4"],
        names: &[],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "LinearAlgebra/Sparse",
        sections: &["8.4.1"],
        names: &[],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Calculus",
        sections: &["8.5"],
        names: &[],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Solve",
        sections: &[],
        names: &[
            "solve",
            "solve_complex",
            "nsolve",
            "nsolve_f",
            "root",
            "solve_system",
            "linsolve",
        ],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Optimize",
        sections: &[],
        names: &["minimize", "minimize_f", "maximize", "least_squares"],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Stats",
        sections: &["8.7"],
        names: &[],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Stats/Distributions",
        sections: &["8.7.1"],
        names: &[],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "FFT",
        sections: &[],
        names: &["fft", "ifft", "fft_norm", "rfft", "irfft", "fftfreq"],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Interpolate",
        sections: &[],
        names: &[
            "interp1",
            "interp2",
            "spline",
            "polyfit",
            "polyval",
            "chebyshev_fit",
        ],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Ode",
        sections: &["8.9"],
        names: &[],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Sde",
        sections: &["8.9.1"],
        names: &[],
        types: &[],
        // Hand-written: the §8.9.1 option ADTs are declarations a program must
        // be able to construct, and the alias-only generator cannot emit an ADT.
        hand_written: true,
    },
    ModuleDef {
        path: "Pde",
        sections: &["8.10"],
        names: &[],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Data",
        sections: &["8.11"],
        names: &[],
        types: &[("Table", 1)],
        hand_written: false,
    },
    ModuleDef {
        path: "Data/Csv",
        sections: &[],
        names: &[
            "read_csv",
            "read_csv_dyn",
            "write_csv",
            "column",
            "column_dyn",
        ],
        types: &[("Table", 1)],
        hand_written: false,
    },
    ModuleDef {
        path: "Data/Npy",
        sections: &[],
        names: &["read_npy", "write_npy"],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Data/Json",
        sections: &[],
        names: &["read_json", "write_json", "to_json", "from_json"],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Data/Table",
        sections: &[],
        names: &[
            "column",
            "column_dyn",
            "row",
            "select",
            "filter",
            "join",
            "schema",
            "to_table",
            "to_dynamic",
        ],
        types: &[("Table", 1)],
        hand_written: false,
    },
    ModuleDef {
        path: "Plot",
        sections: &["8.12"],
        names: &[],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Test",
        sections: &["8.13"],
        names: &[],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Measurement",
        sections: &["8.14"],
        names: &[],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Device",
        sections: &["8.18"],
        names: &[],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Physics",
        sections: &[],
        names: &["simulate"],
        types: &[],
        hand_written: false,
    },
    ModuleDef {
        path: "Physics/Ising",
        sections: &[],
        names: &[],
        types: &[],
        hand_written: true,
    },
    ModuleDef {
        path: "Physics/Geodesic",
        sections: &[],
        names: &[],
        types: &[],
        hand_written: true,
    },
];

macro_rules! module_table {
    ($(($path:literal, $file:literal)),+ $(,)?) => {
        &[$(Module { path: $path, source: include_str!($file) }),+]
    };
}

/// Every embedded module, in §8.20 order.
#[must_use]
pub fn modules() -> &'static [Module] {
    static MODULES: &[Module] = module_table!(
        ("Prelude", "../stdlib/Prelude.aev"),
        ("Math", "../stdlib/Math.aev"),
        ("Math/Special", "../stdlib/Math/Special.aev"),
        ("Math/Complex", "../stdlib/Math/Complex.aev"),
        ("Math/NumberTheory", "../stdlib/Math/NumberTheory.aev"),
        ("Math/Combinatorics", "../stdlib/Math/Combinatorics.aev"),
        ("Math/Geometry", "../stdlib/Math/Geometry.aev"),
        ("Units/SI", "../stdlib/Units/SI.aev"),
        ("Units/Imperial", "../stdlib/Units/Imperial.aev"),
        ("Units/Astronomy", "../stdlib/Units/Astronomy.aev"),
        ("Units/CGS", "../stdlib/Units/CGS.aev"),
        ("Units/Natural", "../stdlib/Units/Natural.aev"),
        ("Units/Constants", "../stdlib/Units/Constants.aev"),
        ("LinearAlgebra", "../stdlib/LinearAlgebra.aev"),
        ("LinearAlgebra/Sparse", "../stdlib/LinearAlgebra/Sparse.aev"),
        ("Calculus", "../stdlib/Calculus.aev"),
        ("Solve", "../stdlib/Solve.aev"),
        ("Optimize", "../stdlib/Optimize.aev"),
        ("Stats", "../stdlib/Stats.aev"),
        ("Stats/Distributions", "../stdlib/Stats/Distributions.aev"),
        ("FFT", "../stdlib/FFT.aev"),
        ("Interpolate", "../stdlib/Interpolate.aev"),
        ("Ode", "../stdlib/Ode.aev"),
        ("Sde", "../stdlib/Sde.aev"),
        ("Pde", "../stdlib/Pde.aev"),
        ("Data", "../stdlib/Data.aev"),
        ("Data/Csv", "../stdlib/Data/Csv.aev"),
        ("Data/Npy", "../stdlib/Data/Npy.aev"),
        ("Data/Json", "../stdlib/Data/Json.aev"),
        ("Data/Table", "../stdlib/Data/Table.aev"),
        ("Plot", "../stdlib/Plot.aev"),
        ("Test", "../stdlib/Test.aev"),
        ("Measurement", "../stdlib/Measurement.aev"),
        ("Device", "../stdlib/Device.aev"),
        ("Physics", "../stdlib/Physics.aev"),
        ("Physics/Ising", "../stdlib/Physics/Ising.aev"),
        ("Physics/Geodesic", "../stdlib/Physics/Geodesic.aev"),
    );
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
    for line in source.lines() {
        let line = line.trim_start();
        if line.starts_with('#') {
            continue;
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
        if end == 0 {
            continue;
        }
        let name = &rest[..end];
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// The generated source for every §8.20 module whose contents come from the
/// specification. `path → source`, `/` separated.
///
/// Hand-written modules (`Units/*`, `Physics/Ising`, `Physics/Geodesic`) are not
/// included: their contract is a real program, not a list of aliases.
#[must_use]
pub fn spec_module_sources(spec_source: &str) -> Vec<(String, String)> {
    let sections = spec::sections(spec_source);
    let by_number: BTreeMap<&str, &spec::Section> = sections
        .iter()
        .map(|section| (section.number.as_str(), section))
        .collect();
    TREE.iter()
        .filter(|module| !module.hand_written)
        .map(|module| {
            let mut names: Vec<&str> = Vec::new();
            for number in module.sections {
                let Some(section) = by_number.get(number) else {
                    panic!(
                        "spec/08-stdlib.md has no section `{number}`, but module `{}` re-exports it",
                        module.path
                    );
                };
                names.extend(section.names.iter().map(String::as_str));
            }
            names.extend(module.names.iter().copied());
            names.sort_unstable();
            names.dedup();
            (module.path.to_string(), render_module(module, &names))
        })
        .collect()
}

/// Chapter-8 names that are *call forms*, not first-class values.
///
/// The checker special-cases their call position (variadic shapes and
/// type arguments: `zeros(3, 4)`, `zeros<Int>(3)`, `fill(x, 3)`), and has no
/// value scheme for the bare name, so `let zeros = zeros` is `E0301`. They stay
/// globally callable without an import; a module simply cannot re-export them
/// until the checker gives them a value form. The gap is asserted by
/// `tests/stdlib_modules.rs` so it cannot be forgotten.
pub const CALL_FORM_ONLY: &[&str] = &["zeros", "ones", "fill"];

/// Renders one generated module: a header, the `module` declaration, one
/// self-alias per name and one alias per re-exported type.
fn render_module(
    module: &ModuleDef,
    names: &[&str],
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "## The `{}` module of §8.20.\n\
         ##\n\
         ## Generated from `spec/08-stdlib.md`; do not edit.\n\
         ## Regenerate with `cargo test -p aevia-std --test stdlib_modules -- --ignored regenerate`.\n\
         module {}\n\n",
        module.path,
        module.path.replace('/', ".")
    ));
    // Uppercase names are constructors (`Err`, `None`, `Ok`, `Some`): a `let`
    // binding cannot take a capitalised name (it parses as a constructor
    // pattern), and the constructors are already ambient from the Prelude's
    // type declarations.
    let mut skipped: Vec<(&str, &'static str)> = Vec::new();
    for name in names {
        if name.starts_with(|ch: char| ch.is_ascii_uppercase()) || name.contains('.') {
            skipped.push((
                name,
                "constructor or qualified name: ambient from the Prelude",
            ));
            continue;
        }
        if CALL_FORM_ONLY.contains(name) {
            skipped.push((
                name,
                "call form: the checker special-cases its call position and has no value scheme",
            ));
            continue;
        }
        out.push_str(&format!("let {name} = {name}\n"));
    }
    for (name, arity) in module.types {
        let params: Vec<String> = (0..*arity)
            .map(|index| format!("{}", (b'a' + u8::try_from(index).unwrap_or(0)) as char))
            .collect();
        if params.is_empty() {
            out.push_str(&format!("type {name} = {name}\n"));
        } else {
            let params = params.join(", ");
            out.push_str(&format!("type {name}<{params}> = {name}<{params}>\n"));
        }
    }
    if !skipped.is_empty() {
        out.push_str(
            "\n## Not re-exported (see the module header of `tests/stdlib_modules.rs`):\n",
        );
        for (name, reason) in &skipped {
            out.push_str(&format!("##   {name} — {reason}\n"));
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
                if let Some(rest) = line.strip_prefix("type ") {
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
