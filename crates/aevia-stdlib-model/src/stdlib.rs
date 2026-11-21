//! Declarative module catalog and source generation for chapter 8.
use crate::spec;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Write as _;

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
        names: &["factor", "is_prime", "primes", "gcd", "lcm", "mod_pow"],
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
        names: &["polygon_area", "distance", "tetrahedron_volume"],
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
        names: &[
            "fft",
            "ifft",
            "fft_norm",
            "ifft_norm",
            "fft_axis",
            "ifft_axis",
            "fft2",
            "ifft2",
            "fftn",
            "ifftn",
            "fftn_axes",
            "ifftn_axes",
            "rfftn_axes",
            "irfftn_axes",
            "fftn_norm",
            "ifftn_norm",
            "rfft",
            "irfft",
            "rfft2",
            "irfft2",
            "rfftn",
            "irfftn",
            "fftshift",
            "ifftshift",
            "fftshift_axis",
            "ifftshift_axis",
            "fftfreq",
            "rfftfreq",
        ],
        types: &[],
    },
    ModuleDef {
        path: "Interpolate",
        sections: &[],
        names: &[
            "interp1",
            "interp2",
            "interp2Batch",
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
fn validate_catalog() -> Result<(), String> {
    let mut paths = BTreeSet::new();
    for module in TREE {
        if !paths.insert(module.path) {
            return Err(format!("duplicate module path `{}`", module.path));
        }
        for component in module.path.split('/') {
            let mut chars = component.chars();
            if !(chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_'))
            {
                return Err(format!("invalid module path `{}`", module.path));
            }
        }
    }
    Ok(())
}

/// The generated source for every §8.20 module whose contents come from the
/// specification. `path → source`, `/` separated.
///
/// `Units.Constants` is generated separately from the live RSSN manifest.
///
/// # Panics
///
/// Panics when the specification or the module catalog is incomplete. Use
/// [`try_spec_module_sources_with_docs`] to report the error to the caller.
#[must_use]
pub fn spec_module_sources(spec_source: &str) -> Vec<(String, String)> {
    spec_module_sources_with_docs(spec_source, &|_, _, _| None)
}

/// Supplies the `##` documentation of one re-exported name.
///
/// Called with the module path (`Math/NumberTheory`), the name, and the
/// signatures chapter 8 states for that name; returns markdown (no `##`
/// prefixes), or `None` to leave the name undocumented. The model has no
/// dependency on the kernel, so whoever has the Kernel Manifest and the Rust
/// sources of the library supplies this.
pub type DocFn<'a> = &'a dyn Fn(&str, &str, &[String]) -> Option<String>;

/// [`spec_module_sources`] with a documentation source for every name.
///
/// # Panics
///
/// Panics when the specification or the module catalog is incomplete.
#[must_use]
#[allow(
    clippy::panic,
    reason = "preserves the established fail-fast generation API"
)]
pub fn spec_module_sources_with_docs(
    spec_source: &str,
    docs: DocFn<'_>,
) -> Vec<(String, String)> {
    try_spec_module_sources_with_docs(spec_source, docs).unwrap_or_else(|error| panic!("{error}"))
}

/// Generates the specification's modules and attaches documentation to their exports.
///
/// # Errors
///
/// Returns an error for an invalid catalog, a missing section, export, or type,
/// or a duplicate numbered section or constructible type in that section.
pub fn try_spec_module_sources_with_docs(
    spec_source: &str,
    docs: DocFn<'_>,
) -> Result<Vec<(String, String)>, String> {
    validate_catalog()?;
    let chapter = spec::read_chapter(spec_source, true);
    let mut by_number = BTreeMap::new();
    for section in &chapter.sections {
        if !section.number.is_empty()
            && by_number.insert(section.number.as_str(), section).is_some()
        {
            return Err(format!("duplicate chapter 8 section {}", section.number));
        }
    }
    let declared: BTreeSet<&str> = chapter
        .sections
        .iter()
        .flat_map(|section| section.names.iter().map(String::as_str))
        .collect();
    for module in TREE {
        for name in module.names {
            if !declared.contains(name) {
                return Err(format!(
                    "module `{}` re-exports `{name}`, which is absent from chapter 8",
                    module.path
                ));
            }
        }
    }
    TREE.iter()
        .filter(|module| !is_manifest_generated(module.path))
        .map(|module| {
            let mut names: Vec<&str> = Vec::new();
            for number in module.sections {
                let section = by_number.get(number).ok_or_else(|| format!(
                    "spec/08-stdlib.md has no section `{number}`, but module `{}` re-exports it",
                    module.path
                ))?;
                names.extend(section.names.iter().map(String::as_str));
            }
            names.extend(module.names.iter().copied());
            names.sort_unstable();
            names.dedup();
            Ok((
                module.path.to_string(),
                render_module(module, &names, &chapter, docs)?,
            ))
        })
        .collect()
}

/// The module, authoritative section, and ordered constructible types to copy.
const TYPE_SOURCES: &[(&str, &str, &[&str])] = &[
    (
        "Ode",
        "8.9",
        &[
            "OdeMethod",
            "OdeOpts",
            "OdeAdaptiveOpts",
            "OdePhysicalOpts",
            "OdeEvent",
            "OdeHit",
            "OdeSolution",
        ],
    ),
    // §8.4.1: the preconditioner of `cg_with`.
    ("LinearAlgebra/Sparse", "8.4.1", &["Precond"]),
    // §8.7.1: the nominal distribution type and its parameter records,
    // so `Normal({ mu = 0.0 m, sigma = 0.1 m })` is a source value.
    (
        "Stats/Distributions",
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
            "LogNormal",
            "Weibull",
            "Cauchy",
            "F",
            "NegativeBinomial",
            "Geometric",
            "Distribution",
        ],
    ),
    ("FFT", "8.8", &["Norm", "AxisScale"]),
    (
        "Sde",
        "8.9.1",
        &[
            "SdeMethod",
            "SdeStructure",
            "SdeFixedOpts",
            "SdeAdaptiveOpts",
            "SdeOpts",
            "SdeEvent",
            "SdeHit",
        ],
    ),
];

/// Copies constructible types in the order expected by the generated module.
#[allow(
    clippy::single_call_fn,
    reason = "isolates chapter fence and declaration handling from module rendering"
)]
fn type_declarations(
    path: &str,
    chapter: &spec::Chapter<'_>,
) -> Result<Vec<String>, String> {
    let Some(&(_, section, wanted)) = TYPE_SOURCES.iter().find(|(module, _, _)| *module == path)
    else {
        return Ok(Vec::new());
    };
    wanted
        .iter()
        .map(|name| chapter.type_source(section, name))
        .collect()
}

/// Module-specific structured data that cannot be derived from section exports.
/// Units are ambient values; physics records and façades describe the typed
/// boundary around the kernel's scenario interface.
#[allow(
    clippy::single_call_fn,
    reason = "keeps non-specification module declarations separate"
)]
fn special_body(path: &str) -> Option<String> {
    type Fields = &'static [(&'static str, &'static str)];
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
    let (fields, result_name, result_fields, scenario): (Fields, &str, Fields, &str) = match path {
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
        let what = if name == "Params" {
            format!("The parameters of the `{scenario}` simulation, one field per setting.")
        } else {
            format!("What the `{scenario}` simulation returns.")
        };
        push_doc_text(&mut out, &what);
        let _ = writeln!(out, "type {name} = {{");
        for (field, ty) in fields {
            let _ = writeln!(out, "    {field}: {ty},");
        }
        out.push_str("}\n\n");
    }
    push_doc_text(
        &mut out,
        &format!(
            "Runs the `{scenario}` simulation with the given parameters and returns its result, \
             or `Err(SimError)` if the parameters are rejected."
        ),
    );
    let _ = write!(
        out,
        "fn run(p: Params): Result<{result_name}, SimError> =\n    match simulate(\"{scenario}\", to_json(p)) with\n    | Ok(json) -> from_json(json)\n    | Err(error) -> Err(error)\n"
    );
    Some(out)
}

/// Writes `text` as a `##` documentation block.
fn push_doc_text(
    out: &mut String,
    text: &str,
) {
    for line in text.lines() {
        if line.is_empty() {
            out.push_str("##\n");
        } else {
            out.push_str("## ");
            out.push_str(line);
            out.push('\n');
        }
    }
}

/// Documents the type `name` from its guide, when it has one.
fn push_doc(
    out: &mut String,
    name: &str,
) {
    if let Some(guide) = crate::guides::guide(name) {
        push_doc_text(out, guide.description);
    }
}

/// Documents a `type Name ...` declaration copied from the specification.
#[allow(
    clippy::single_call_fn,
    reason = "isolates source declaration name extraction"
)]
fn push_type_doc(
    out: &mut String,
    declaration: &str,
) {
    let name = declaration
        .strip_prefix("type ")
        .and_then(|rest| {
            rest.split(|c: char| c == '<' || c == '=' || c.is_whitespace())
                .next()
        })
        .unwrap_or("");
    push_doc(out, name);
}

/// Renders one generated module: a header, the `module` declaration, one
/// self-alias per name and one alias per re-exported type.
#[allow(
    clippy::single_call_fn,
    reason = "keeps textual formatting separate from catalog validation"
)]
fn render_module(
    module: &ModuleDef,
    names: &[&str],
    chapter: &spec::Chapter<'_>,
    docs: DocFn<'_>,
) -> Result<String, String> {
    let mut out = String::new();
    // The generated notice is a plain `#` comment: a `##` line would be
    // documentation, and would be repeated on the page of every module.
    let _ = write!(
        out,
        "## The `{}` module of §8.20.\n\
         # Generated standard library; do not edit.\n\
         # Regenerate with `cargo run -p aevia-stdlib-gen -- --write`.\n\
         module {}\n\n",
        module.path,
        module.path.replace('/', ".")
    );
    for declaration in type_declarations(module.path, chapter)? {
        push_type_doc(&mut out, &declaration);
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
        let found = chapter.signatures(name);
        if let Some(text) = docs(module.path, name, found) {
            push_doc_text(&mut out, &text);
        }
        let _ = writeln!(out, "let {name} = {name}");
    }
    for (name, arity) in module.types {
        let params: Vec<String> = ('a'..='z')
            .map(|letter| letter.to_string())
            .chain((26..).map(|index| format!("p{index}")))
            .take(*arity)
            .collect();
        push_doc(&mut out, name);
        if params.is_empty() {
            let _ = writeln!(out, "type {name} = {name}");
        } else {
            let params = params.join(", ");
            let _ = writeln!(out, "type {name}<{params}> = {name}<{params}>");
        }
    }
    if !skipped.is_empty() {
        out.push_str(
            "\n## Not re-exported (see the module header of `tests/stdlib_modules.rs`):\n",
        );
        for (name, reason) in &skipped {
            let _ = writeln!(out, "##   {name} — {reason}");
        }
    }
    Ok(out)
}

/// Generates the Rust embedding table from the same catalog as the sources.
///
/// # Panics
///
/// Panics if the built-in catalog includes duplicate or invalid paths.
#[must_use]
#[allow(
    clippy::panic,
    reason = "invalid static catalog paths are generator defects"
)]
pub fn embedding_table_source() -> String {
    if let Err(error) = validate_catalog() {
        panic!("{error}");
    }
    let mut out = String::from(
        "// Generated by aevia-stdlib-gen; do not edit.\nstatic MODULES: &[Module] = &[\n",
    );
    for module in TREE {
        let path = module.path;
        let _ = writeln!(
            out,
            "    Module {{ path: \"{path}\", source: include_str!(\"../stdlib/{path}.aev\") }},"
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_extraction_ignores_section_headings_inside_other_fences() {
        let source = "```markdown\n## 8.8 Quoted heading\ntype Norm = Fake\n```\n\
                      ## 8.8 FFT\n```aevia\ntype Norm = | Unscaled\ntype AxisScale = | Linear\n```\n";
        assert_eq!(
            type_declarations("FFT", &spec::read_chapter(source, true)).expect("extract types"),
            ["type Norm = | Unscaled", "type AxisScale = | Linear"]
        );
    }

    #[test]
    fn incomplete_specification_is_reported_as_an_error() {
        let error =
            try_spec_module_sources_with_docs("", &|_, _, _| None).expect_err("missing source");
        assert!(error.contains("which is absent from chapter 8"), "{error}");
    }

    #[test]
    fn duplicate_numbered_sections_fail_instead_of_overwriting_exports() {
        let source = "## 8.8 FFT\n```\nfft : Int\n```\n### 8.8 Repeated\n";
        let result = try_spec_module_sources_with_docs(source, &|_, _, _| None);
        assert_eq!(result.unwrap_err(), "duplicate chapter 8 section 8.8");
    }

    #[test]
    fn duplicate_constructible_types_fail_instead_of_overwriting_a_definition() {
        let source = "## 8.8 FFT\n```aevia\ntype Norm = | First\n```\n\
                      ```text\ntype Norm = | Second\ntype AxisScale = | Linear\n```\n";
        let result = type_declarations("FFT", &spec::read_chapter(source, true));
        assert_eq!(result.unwrap_err(), "duplicate type `Norm` in section 8.8");
    }

    #[test]
    fn a_blank_line_inside_a_constructible_record_preserves_its_remaining_fields() {
        let source = "## 8.9 Ode\n```aevia\ntype OdeOpts = {\n    a: Int,\n\n    b: Int,\n}\n```\n";
        let parsed = spec::read_chapter(source, true);
        assert_eq!(
            parsed.type_source("8.9", "OdeOpts").unwrap(),
            "type OdeOpts = {\n    a: Int,\n\n    b: Int,\n}"
        );
    }
}
