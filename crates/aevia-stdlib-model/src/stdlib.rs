//! Declarative module catalog and source generation for chapter 8.
use crate::spec;
use std::collections::BTreeMap;

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
    },
    ModuleDef {
        path: "Math",
        sections: &["8.2", "8.2.1", "8.3"],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Math/Special",
        sections: &["8.2.2"],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Math/Complex",
        sections: &["8.15"],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Math/NumberTheory",
        sections: &[],
        names: &["factor", "is_prime", "primes", "gcd", "lcm"],
        types: &[],
    },
    ModuleDef {
        path: "Math/Combinatorics",
        sections: &[],
        names: &["binomial", "permutations"],
        types: &[],
    },
    ModuleDef {
        path: "Math/Geometry",
        sections: &[],
        names: &["polygon_area", "distance"],
        types: &[],
    },
    ModuleDef {
        path: "Units/SI",
        sections: &[],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Units/Imperial",
        sections: &[],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Units/Astronomy",
        sections: &[],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Units/CGS",
        sections: &[],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Units/Natural",
        sections: &[],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Units/Planck",
        sections: &[],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Units/Atomic",
        sections: &[],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Units/Geometrized",
        sections: &[],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Units/Constants",
        sections: &[],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Tensor",
        sections: &[],
        names: &["zeros", "ones", "fill"],
        types: &[],
    },
    ModuleDef {
        path: "LinearAlgebra",
        sections: &["8.4"],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "LinearAlgebra/Sparse",
        sections: &["8.4.1"],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Calculus",
        sections: &["8.5"],
        names: &[],
        types: &[],
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
    },
    ModuleDef {
        path: "Optimize",
        sections: &[],
        names: &["minimize", "minimize_f", "maximize", "least_squares"],
        types: &[],
    },
    ModuleDef {
        path: "Stats",
        sections: &["8.7"],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Stats/Distributions",
        sections: &["8.7.1"],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "FFT",
        sections: &[],
        names: &["fft", "ifft", "fft_norm", "rfft", "irfft", "fftfreq"],
        types: &[],
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
    },
    ModuleDef {
        path: "Ode",
        sections: &["8.9"],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Sde",
        sections: &["8.9.1"],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Pde",
        sections: &["8.10"],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Data",
        sections: &["8.11"],
        names: &[],
        types: &[("Table", 1)],
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
    },
    ModuleDef {
        path: "Data/Npy",
        sections: &[],
        names: &["read_npy", "write_npy"],
        types: &[],
    },
    ModuleDef {
        path: "Data/Json",
        sections: &[],
        names: &["read_json", "write_json", "to_json", "from_json"],
        types: &[],
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
    },
    ModuleDef {
        path: "Plot",
        sections: &["8.12"],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Test",
        sections: &["8.13"],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Measurement",
        sections: &["8.14"],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Device",
        sections: &["8.18"],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Physics",
        sections: &[],
        names: &["simulate"],
        types: &[],
    },
    ModuleDef {
        path: "Physics/Ising",
        sections: &[],
        names: &[],
        types: &[],
    },
    ModuleDef {
        path: "Physics/Geodesic",
        sections: &[],
        names: &[],
        types: &[],
    },
];

/// Reject duplicate or unsafe catalog paths before generating any files.
fn validate_catalog() {
    let mut paths = std::collections::BTreeSet::new();
    for module in TREE {
        assert!(
            paths.insert(module.path),
            "duplicate module path `{}`",
            module.path
        );
        for component in module.path.split('/') {
            let mut chars = component.chars();
            assert!(
                chars
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                    && chars.all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "invalid module path `{}`",
                module.path
            );
        }
    }
}

/// The generated source for every §8.20 module whose contents come from the
/// specification. `path → source`, `/` separated.
///
/// `Units.Constants` is generated separately from the live RSSN manifest.
#[must_use]
pub fn spec_module_sources(spec_source: &str) -> Vec<(String, String)> {
    validate_catalog();
    let sections = spec::sections(spec_source);
    let declared = spec::names(spec_source);
    for module in TREE {
        for name in module.names {
            assert!(
                declared.binary_search(&name.to_string()).is_ok(),
                "module `{}` re-exports `{name}`, which is absent from chapter 8",
                module.path
            );
        }
    }
    let by_number: BTreeMap<&str, &spec::Section> = sections
        .iter()
        .map(|section| (section.number.as_str(), section))
        .collect();
    TREE.iter()
        .filter(|module| !is_manifest_generated(module.path))
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
            (module.path.to_string(), render_module(module, &names, spec_source))
        })
        .collect()
}

/// Constructible declarations are copied from the authoritative chapter, with
/// comments removed. The allowlist avoids emitting illustrative or result-only
/// types whose dimensions cannot yet be represented by source declarations.
fn type_declarations(
    path: &str,
    source: &str,
) -> Vec<String> {
    let (section, wanted): (&str, &[&str]) = match path {
        | "Ode" => ("8.9", &["OdeMethod", "OdeOpts"]),
        // §8.4.1: the preconditioner of `cg_with`.
        | "LinearAlgebra/Sparse" => ("8.4.1", &["Precond"]),
        // §8.7.1: the nominal distribution type and its parameter records,
        // so `Normal({ mu = 0.0 m, sigma = 0.1 m })` is a source value.
        | "Stats/Distributions" => {
            (
                "8.7.1",
                &[
                    "Normal",
                    "Uniform",
                    "Exponential",
                    "Gamma",
                    "Beta",
                    "ChiSquared",
                    "StudentT",
                    "Poisson",
                    "Binomial",
                    "Distribution",
                ],
            )
        },
        | "FFT" => ("8.8", &["Norm", "AxisScale"]),
        | "Sde" => {
            (
                "8.9.1",
                &[
                    "SdeMethod",
                    "SdeStructure",
                    "SdeFixedOpts",
                    "SdeAdaptiveOpts",
                    "SdeOpts",
                ],
            )
        },
        | _ => return Vec::new(),
    };
    let mut active = false;
    let mut fenced = false;
    let mut declarations = BTreeMap::new();
    let mut current: Option<(String, Vec<String>)> = None;
    for raw in source.lines() {
        if !fenced && (raw.starts_with("## ") || raw.starts_with("### ")) {
            active = raw.split_whitespace().nth(1) == Some(section);
        }
        if !active {
            continue;
        }
        if raw.trim_start().starts_with("```") {
            if let Some((name, lines)) = current.take() {
                declarations.insert(name, lines.join("\n"));
            }
            fenced = !fenced;
            continue;
        }
        if !fenced {
            continue;
        }
        let line = raw.split('#').next().unwrap_or("").trim_end();
        if let Some(rest) = line.strip_prefix("type ") {
            if let Some((name, lines)) = current.take() {
                declarations.insert(name, lines.join("\n"));
            }
            let name = rest
                .split(|c: char| c == '<' || c == '=' || c.is_whitespace())
                .next()
                .unwrap_or("");
            if wanted.contains(&name) {
                current = Some((name.to_string(), vec![line.to_string()]));
            }
        } else if line.trim().is_empty() {
            if let Some((name, lines)) = current.take() {
                declarations.insert(name, lines.join("\n"));
            }
        } else if let Some((_, lines)) = &mut current {
            // A declaration ends before another top-level signature.
            if !line.starts_with(char::is_whitespace) && line != "}" {
                let (name, lines) = current.take().unwrap();
                declarations.insert(name, lines.join("\n"));
            } else if line.trim_start().starts_with("| ") {
                // The specification packs multiple variants onto continuation
                // lines, whereas Aevia source requires one per line.
                for variant in line.trim().trim_start_matches("| ").split(" | ") {
                    lines.push(format!("    | {variant}"));
                }
            } else {
                lines.push(line.to_string());
            }
        }
    }
    if let Some((name, lines)) = current {
        declarations.insert(name, lines.join("\n"));
    }
    wanted
        .iter()
        .map(|name| {
            declarations
                .remove(*name)
                .unwrap_or_else(|| panic!("missing type `{name}` in section {section}"))
        })
        .collect()
}

/// Module-specific structured data that cannot be derived from section exports.
/// Units are ambient values; physics records and façades describe the typed
/// boundary around the kernel's scenario interface.
fn special_body(path: &str) -> Option<String> {
    let unit_note = match path {
        | "Units/SI" => Some("SI unit names are ambient Unit values from the kernel registry."),
        | "Units/Imperial" => {
            Some("Imperial unit names are ambient Unit values from the kernel registry.")
        },
        | "Units/Astronomy" => {
            Some("Astronomy unit names are ambient Unit values from the kernel registry.")
        },
        | "Units/CGS" => Some("CGS unit names are ambient Unit values from the kernel registry."),
        | _ => None,
    };
    if let Some(note) = unit_note {
        return Some(format!("## {note}\n## This module declares no bindings.\n"));
    }
    let (fields, result_name, result_fields, scenario): (
        &[(&str, &str)],
        &str,
        &[(&str, &str)],
        &str,
    ) = match path {
        | "Physics/Ising" => {
            (
                &[
                    ("width", "Int"),
                    ("height", "Int"),
                    ("temperature", "Float[1]"),
                    ("mc_steps", "Int"),
                    ("seed", "Option<Int>"),
                ],
                "IsingResult",
                &[("spins", "Tensor<2, Int>"), ("magnetization", "Float[1]")],
                "ising",
            )
        },
        | "Physics/Geodesic" => {
            (
                &[
                    ("black_hole_mass", "Float[M]"),
                    ("initial_state", "Tensor<1, Float[1]>"),
                    ("proper_time_end", "Float[T]"),
                    ("initial_dt", "Float[T]"),
                ],
                "GeodesicResult",
                &[("orbit", "Tensor<2, Float[1]>")],
                "geodesic",
            )
        },
        | _ => return None,
    };
    let mut out = String::new();
    for (name, fields) in [("Params", fields), (result_name, result_fields)] {
        out.push_str(&format!("type {name} = {{\n"));
        for (field, ty) in fields {
            out.push_str(&format!("    {field}: {ty},\n"));
        }
        out.push_str("}\n\n");
    }
    out.push_str(&format!("fn run(p: Params): Result<{result_name}, SimError> =\n    match simulate(\"{scenario}\", to_json(p)) with\n    | Ok(json) -> from_json(json)\n    | Err(error) -> Err(error)\n"));
    Some(out)
}

/// Renders one generated module: a header, the `module` declaration, one
/// self-alias per name and one alias per re-exported type.
fn render_module(
    module: &ModuleDef,
    names: &[&str],
    spec_source: &str,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "## The `{}` module of §8.20.\n\
         ##\n\
         ## Generated standard library; do not edit.\n\
         ## Regenerate with `cargo run -p aevia-stdlib-gen -- --write`.\n\
         module {}\n\n",
        module.path,
        module.path.replace('/', ".")
    ));
    for declaration in type_declarations(module.path, spec_source) {
        out.push_str(&declaration);
        out.push_str("\n\n");
    }
    if let Some(body) = special_body(module.path) {
        out.push_str(&body);
    }
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

/// Generate the Rust embedding table from the same catalog as the sources.
#[must_use]
pub fn embedding_table_source() -> String {
    validate_catalog();
    let mut out = String::from(
        "// Generated by aevia-stdlib-gen; do not edit.\nstatic MODULES: &[Module] = &[\n",
    );
    for module in TREE {
        out.push_str(&format!(
            "    Module {{ path: \"{}\", source: include_str!(\"../stdlib/{}.aev\") }},\n",
            module.path, module.path
        ));
    }
    out.push_str("];\n");
    out
}

/// The unit-system modules (§5.11.7): module path and kernel system name.
pub const SYSTEM_MODULES: &[(&str, &str)] = &[
    ("Units/Natural", "natural"),
    ("Units/Planck", "planck"),
    ("Units/Atomic", "atomic"),
    ("Units/Geometrized", "geometrized"),
];

/// Modules whose source comes from the Kernel Manifest rather than from
/// chapter 8: the constants and the constants of each unit system.
#[must_use]
pub fn is_manifest_generated(path: &str) -> bool {
    path == "Units/Constants" || SYSTEM_MODULES.iter().any(|(module, _)| *module == path)
}
