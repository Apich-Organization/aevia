//! Generates the `Rssn` modules from the Kernel Manifest.
//!
//! Aevia has no operator table. This module turns `manifest.ops`,
//! `manifest.units` and `manifest.constants` into Aevia source, one typed
//! declaration per overload, each pinning the `OpKey` it was generated from so
//! that the static choice and the executed operator cannot disagree.
//!
//! Three files come out, because one file is one module: `Rssn.aev` for the
//! operators and capabilities, `Units.aev` and `Constants.aev` for the two
//! tables.
//!
//! # Dimension parameters in declarations
//!
//! A manifest signature can be dimension-parametric (`sde_wiener` takes
//! `Real[T]` and returns `Tensor<2, Real[T^(1/2)]>`); the emitted declaration
//! spells that with Aevia dimension variables (`T`, `T2`, …) rather than
//! `Dyn<term>`, and the names are shared across the parameters and the result
//! of one declaration. `Dyn<term>` is now only for genuinely unrepresentable
//! types: a kernel signature that itself says `Dyn(rep, dim = any)` (the
//! manifest deliberately leaves the representation and dimension open, e.g.
//! `quantity`/`convert` values), a **rank-variable** tensor (`Tensor<n, τ>` — a
//! declaration needs a concrete rank; the checker answers `E0304`, and the
//! refined-shape spelling `Tensor<[n], τ>` fixes the rank instead), a type
//! parameter, a table row, a device handle, a variant, a host opaque type, or
//! `Never`. The generated module is a typed façade: each `Rssn.invoke` result
//! is dynamically typed by the checker, so the module is checked with that one
//! documented gap (see `tests/generated.rs::is_the_invoke_typing_gap`), while
//! `Units.aev` and `Constants.aev` type-check cleanly.

use crate::registry::OpEntry;
use crate::registry::Registry;
use crate::registry::SigType;
use crate::registry::Source;
use crate::registry::render_type;
use num_traits::One;
use num_traits::Signed;
use std::fmt::Write as _;

/// Renders the generated modules as `(file name, source)` pairs.
#[must_use]
pub fn files(registry: &Registry) -> Vec<(String, String)> {
    vec![
        ("Rssn.aev".to_string(), operators(registry)),
        ("Units.aev".to_string(), units(registry)),
        ("Constants.aev".to_string(), constants(registry)),
    ]
}

fn header(
    out: &mut String,
    registry: &Registry,
    module: &str,
    what: &str,
) {
    let _ = writeln!(
        out,
        "## {what}\n\
         ##\n\
         ## Generated from the rssn Kernel Manifest. Do not edit:\n\
         ## regenerate with `aevia rssn --emit <dir>`.\n\
         ## rssn {version}, api {api}, manifest {hash}\n\
         module {module}\n",
        version = registry.version,
        api = registry.api_version,
        hash = registry.manifest_hash,
    );
}

/// The `Rssn` module: capabilities and one declaration per overload.
#[must_use]
pub fn operators(registry: &Registry) -> String {
    let mut out = String::new();
    header(
        &mut out,
        registry,
        "Rssn",
        "The rssn kernel: every operator, typed by its manifest signature.",
    );
    // The count is stated so that a reader (and the conformance test) can
    // check the module against `manifest.ops.len()` instead of against the
    // generator's own arithmetic.
    let _ = writeln!(out, "## operators: {}", registry.op_count());

    let _ = writeln!(
        out,
        "# --- opaque kernel types ---\n\
         ## The dynamic ABI's own types. They have no constructors here: a\n\
         ## value of one is created by `to_value` or returned by a call.\n\
         type Value\ntype Symbol\ntype OpError\n"
    );

    let _ = writeln!(out, "# --- version and capabilities ---");
    let _ = writeln!(
        out,
        "let version: String = \"{}\"",
        escape(&registry.version)
    );
    let _ = writeln!(out, "let api_version: Int = {}", registry.api_version);
    let _ = writeln!(
        out,
        "let manifest_hash: String = \"{}\"",
        escape(&registry.manifest_hash)
    );
    for (name, value) in capability_flags(registry) {
        let _ = writeln!(out, "let {name}: Bool = {value}");
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "# --- operators ---");
    let _ = writeln!(
        out,
        "## One declaration per overload, one operator per block. Aliases are not\n\
         ## declared: `Rssn.<alias>` resolves by name, and a second declaration\n\
         ## would be a second `fn` for one operator."
    );
    let mut current = String::new();
    for op in registry.ops() {
        if op.rule_set != current {
            let _ = writeln!(out, "\n# {}.", op.rule_set);
            current.clone_from(&op.rule_set);
        }
        let _ = writeln!(out, "{}", declaration(op));
    }
    out
}

/// The `Units` module.
#[must_use]
pub fn units(registry: &Registry) -> String {
    let mut out = String::new();
    header(&mut out, registry, "Units", "The kernel's unit table.");
    // `s` is both second and siemens, `t` both tonne and tesla, because a
    // binding cannot be capitalised. The unit whose own spelling is already
    // lowercase keeps the plain name — that is the one Aevia source resolves —
    // and the other keeps its category.
    let mut claimed: std::collections::HashSet<String> = std::collections::HashSet::new();
    for unit in &registry.units {
        if unit.name == value_name(&unit.name) {
            claimed.insert(unit.name.clone());
        }
    }
    let mut used: Vec<String> = Vec::new();
    for unit in &registry.units {
        let spelling = match spellable(&unit.name, &unit.aliases) {
            | Some(name) => name,
            | None => {
                let _ = writeln!(
                    out,
                    "## `{}` ({}) cannot be written in Aevia source: every spelling is a keyword",
                    unit.display, unit.category
                );
                continue;
            },
        };
        let dim = unit.dim.to_plain_string();
        let ty = if unit.affine {
            format!("Point[{dim}]")
        } else {
            format!("Real[{dim}]")
        };
        let mut binding = value_name(&unit.name);
        if unit.name != binding && claimed.contains(&binding) {
            binding = format!("{}_{}", binding, unit.category.replace('-', "_"));
        }
        if used.contains(&binding) {
            binding = format!("{}_{}", binding, used.len());
        }
        used.push(binding.clone());
        let _ = writeln!(
            out,
            "## `{}` — {}\nlet {}: {ty} = 1.0 {spelling}",
            unit.display, unit.category, binding,
        );
    }
    out
}

/// The `Constants` module.
#[must_use]
pub fn constants(registry: &Registry) -> String {
    let mut out = String::new();
    header(
        &mut out,
        registry,
        "Constants",
        "The CODATA constants the kernel carries.",
    );
    // Two symbols can lowercase to one binding: `M_u` (molar mass constant)
    // and `m_u` (atomic mass constant) both spell `m_u`, and a module cannot
    // define one name twice. The second keeps its symbol with a numeric
    // suffix, in symbol order, so the output stays deterministic.
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    let needs_pi = registry
        .constants
        .iter()
        .any(|constant| constant.value.pi != 0);
    if needs_pi {
        let _ = writeln!(
            out,
            "## π, for the constants the kernel states as a multiple of it. The\n\
             ## prelude will own this once `Math` exists; a module that needs it\n\
             ## cannot borrow it yet, and a wrong value here would be worse than\n\
             ## the nearest double.\n\
             let pi: Real = {:.17}\n",
            std::f64::consts::PI
        );
    }
    for constant in &registry.constants {
        let dim = constant.dim.to_plain_string();
        let ty = if dim.is_empty() || dim == "1" {
            "Real".to_string()
        } else {
            format!("Real[{dim}]")
        };
        let mut binding = value_name(&constant.symbol);
        if !used.insert(binding.clone()) {
            let mut suffix = 2;
            loop {
                let candidate = format!("{binding}_{suffix}");
                if used.insert(candidate.clone()) {
                    binding = candidate;
                    break;
                }
                suffix += 1;
            }
        }
        let _ = writeln!(
            out,
            "## {} ({}{})\nlet {}: {ty} = {}",
            constant.name,
            constant.source,
            if constant.exact {
                ", exact"
            } else {
                ", measured"
            },
            binding,
            constant_value(constant),
        );
    }
    out
}

/// A unit expression for a dimension, in source syntax.
///
/// `Dim::to_plain_string` writes `1/Amount`, which is not a unit literal; this
/// writes `Amount^-1`, which is.
#[must_use]
pub fn unit_source(dim: &aevia_dim::Dim) -> String {
    let mut factors = Vec::new();
    for (base, exponent) in dim.base_factors() {
        let symbol = match base {
            | "Length" => "m",
            | "Mass" => "kg",
            | "Time" => "s",
            | "Current" => "A",
            | "Temperature" => "K",
            | "Amount" => "mol",
            | "Luminosity" => "cd",
            | other => other,
        };
        if exponent.is_one() {
            factors.push(symbol.to_string());
            continue;
        }
        // The exponent keeps its sign: `mol^-1`, never `mol`.
        if exponent.denom().is_one() {
            factors.push(format!("{symbol}^{}", exponent.numer()));
        } else {
            factors.push(format!(
                "{symbol}^({})",
                aevia_dim::format_rational(exponent)
            ));
        }
    }
    factors.join("*")
}

/// A constant as a value expression.
///
/// The manifest stores a scale as `rat * pi^pi`, exactly. A scale without a
/// factor of `pi` is a rational, and a rational with a terminating expansion
/// can ride on a quantity literal (`6.6e-11 m^3*kg^-1*s^-2`).
///
/// A factor of `pi` cannot: `hbar` is `h/(2*pi)`, and no decimal is equal to
/// it. Multiplying out gave a 47-digit f64 with about 17 meaningful digits —
/// a precision the value does not have, and not the value either. The exact
/// rational and the power of `pi` are kept apart instead, so the only rounding
/// is the one that writes `pi`.
fn constant_value(constant: &crate::registry::ConstantInfo) -> String {
    let unit = unit_source(&constant.dim);
    let coefficient = exact_text(&constant.value.rat);
    let quantity = if unit.is_empty() {
        coefficient
    } else {
        format!("{coefficient} {unit}")
    };
    if constant.value.pi == 0 {
        return quantity;
    }
    let power = constant.value.pi.unsigned_abs();
    let factor = if power == 1 {
        "pi".to_string()
    } else {
        format!("pi^{power}")
    };
    // Parenthesised: `J*s / pi` would read `pi` as part of the unit.
    if constant.value.pi > 0 {
        format!("({quantity}) * {factor}")
    } else {
        format!("({quantity}) / {factor}")
    }
}

/// The capability flags `Rssn.aev` declares.
///
/// One table, in `registry::Capabilities::table`; this only spells the rows.
fn capability_flags(registry: &Registry) -> Vec<(String, bool)> {
    registry
        .capabilities
        .table()
        .into_iter()
        .map(|capability| (capability.binding, capability.enabled))
        .collect()
}

/// Renders one operator's typed declarations.
fn declaration(op: &OpEntry) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "## `{}.{}` ({} signature, {})",
        op.rule_set,
        op.name,
        op.source.as_str(),
        stability(op.stability)
    );
    if op.source == Source::Derived {
        let _ = writeln!(
            out,
            "## Conservative: the kernel derived this from the registration, so it\n## says `Real`/`Term` and not a tensor or quantity type."
        );
    }
    if !op.doc.is_empty() {
        for line in op.doc.lines().take(6) {
            let _ = writeln!(out, "## {}", line.trim_end());
        }
    }
    if op.signatures.is_empty() {
        // A registration with no signature has no type to declare. Emitting
        // `Rssn.invoke(name, "", [])` produced a value that could never be
        // produced (`E1005`: no key has an empty hash), so the module says how
        // to reach it instead of pretending to bind it.
        let _ = writeln!(
            out,
            "## No signature: reach it with Rssn.dynamic(\"{}\", [...])",
            escape(&op.name)
        );
        return out;
    }
    for (index, signature) in op.signatures.iter().enumerate() {
        let name = if index == 0 {
            sanitise(&op.name)
        } else {
            format!("{}_{}", sanitise(&op.name), index + 1)
        };
        // A declaration pins its `OpKey`; an overload the manifest gives no
        // key for cannot be pinned, and emitting `""` produced a hash no
        // `Rssn.invoke` can find (`E1005` on every call). Say so instead of
        // binding a name that cannot work.
        let Some(key) = op.key(index) else {
            let _ = writeln!(
                out,
                "## Overload {} of `{}` has no OpKey in the manifest: reach it with Rssn.dynamic(\"{}\", [...])",
                index + 1,
                escape(&op.name),
                escape(&op.name)
            );
            continue;
        };
        let hash = hex(&key.signature_hash);
        let mut dims = DimNames::default();
        if signature.params.is_empty() && signature.variadic.is_none() {
            // A nullary operator is a constant: `Rssn.E` reads better than
            // `Rssn.E()` and matches how the manifest names it.
            let _ = writeln!(
                out,
                "let {name}: {ret} = Rssn.invoke(\"{op_name}\", \"{hash}\", [])",
                name = value_name(&name),
                ret = render_with(&mut dims, &signature.ret),
                op_name = escape(&op.name),
            );
            continue;
        }
        let mut params: Vec<String> = signature
            .params
            .iter()
            .enumerate()
            .map(|(position, ty)| format!("a{position}: {}", render_with(&mut dims, ty)))
            .collect();
        // The dynamic ABI takes `Value`s, so every argument crosses the
        // boundary through `to_value`, exactly as the specification writes it.
        let fixed: Vec<String> = (0..signature.params.len())
            .map(|position| format!("to_value(a{position})"))
            .collect();
        // The third argument is a value list: `[]`, `rest`, or a literal.
        let arguments = match signature.variadic {
            | None => format!("[{}]", fixed.join(", ")),
            // A variadic operator takes the list as it stands, or after the
            // fixed arguments.
            | Some(_) if fixed.is_empty() => {
                params.push("rest: Tensor<1, Value>".to_string());
                "rest".to_string()
            },
            | Some(_) => {
                params.push("rest: Tensor<1, Value>".to_string());
                format!("concat([{}], rest)", fixed.join(", "))
            },
        };
        let _ = writeln!(
            out,
            "fn {name}({params}): {ret} = Rssn.invoke(\"{op_name}\", \"{hash}\", {args})",
            params = params.join(", "),
            ret = render_with(&mut dims, &signature.ret),
            op_name = escape(&op.name),
            args = arguments,
        );
    }
    out
}

fn stability(value: rssn::kernel::signature::Stability) -> &'static str {
    use rssn::kernel::signature::Stability;
    match value {
        | Stability::Stable => "stable",
        | Stability::Experimental => "experimental",
        | Stability::Deprecated => "deprecated",
    }
}

/// A type that can be written in a declaration.
///
/// Dimensions are the interesting part: a signature parameter can be a
/// dimension *variable* (`Real[T]`, `Tensor<2, Real[T^(1/2)]>`), and the
/// emitted source spells it with an Aevia dimension variable instead of
/// falling back to `Dyn<term>`. A declaration cannot bind a dimension variable
/// — it is implicitly quantified by the checker, exactly as `Float[T]` in a
/// user signature is — so the names are local to the declaration and assigned
/// in first-appearance order (`T`, `T2`, …), sharing one mapping across the
/// parameters and the result.
///
/// `Dyn<term>` remains only where the Aevia source genuinely cannot state the
/// type: an unknown tensor rank (`Tensor<_, τ>` has no declaration spelling), a
/// type-parameter reference (`TypeParamKind::Type`), a table with an unknown
/// row, a device handle, a variant, a host opaque type, or `Never`.
fn render(ty: &SigType) -> String {
    render_with(&mut DimNames::default(), ty)
}

/// The declaration spelling of one type, for conformance tests.
///
/// The same renderer `--emit` uses, so a test can pin the spelling of a
/// hand-built `SigType` (including the `Dyn<term>` cases) without going through
/// the live manifest.
#[must_use]
pub fn declaration_type(ty: &SigType) -> String {
    render(ty)
}

/// Names for the dimension variables of one declaration (`T`, `T2`, …).
#[derive(Default)]
struct DimNames {
    names: std::collections::BTreeMap<u32, String>,
    next: usize,
}

impl DimNames {
    fn name(
        &mut self,
        id: u32,
    ) -> String {
        if let Some(name) = self.names.get(&id) {
            return name.clone();
        }
        let name = if self.next == 0 {
            "T".to_string()
        } else {
            format!("T{}", self.next + 1)
        };
        self.next += 1;
        self.names.insert(id, name.clone());
        name
    }
}

fn render_with(
    dims: &mut DimNames,
    ty: &SigType,
) -> String {
    match ty {
        | SigType::Tensor { rank: None, .. } | SigType::Sparse { rank: None, .. } => {
            "Dyn<term>".to_string()
        },
        | SigType::Tensor { rank, elem } | SigType::Sparse { rank, elem } => {
            format!("Tensor<{}, {}>", rank.unwrap_or(1), render_with(dims, elem))
        },
        | SigType::Var(_) | SigType::Table | SigType::Device | SigType::Never => {
            "Dyn<term>".to_string()
        },
        // A host type has no Aevia spelling; the kernel decides.
        | SigType::Opaque(_) => "Dyn<term>".to_string(),
        | SigType::Unit(dim) => format!("Unit[{}]", render_dim(dims, dim)),
        | SigType::Point(dim) => format!("Point[{}]", render_dim(dims, dim)),
        | SigType::Dim(inner, dim) => {
            format!("{}[{}]", render_with(dims, inner), render_dim(dims, dim))
        },
        | SigType::Func(params, ret) => {
            let types: Vec<String> = params.iter().map(|ty| render_with(dims, ty)).collect();
            format!("({}) -> {}", types.join(", "), render_with(dims, ret))
        },
        | SigType::Record(fields) => {
            let items: Vec<String> = fields
                .iter()
                .map(|(name, ty)| format!("{name}: {}", render_with(dims, ty)))
                .collect();
            format!("{{ {} }}", items.join(", "))
        },
        | SigType::Variant(_) => "Dyn<term>".to_string(),
        | other => render_type(other),
    }
}

/// The Aevia spelling of a dimension: canonical base names, and a generated
/// variable name (`T`) for a dimension variable, so a declaration can state
/// `Real[T^(1/2)]`.
fn render_dim(
    dims: &mut DimNames,
    dim: &aevia_dim::Dim,
) -> String {
    if dim.is_one() {
        return "1".to_string();
    }
    if dim.is_concrete() {
        return dim.to_plain_string();
    }
    let mut numerator = Vec::new();
    let mut denominator = Vec::new();
    for (atom, exponent) in dim.factors() {
        let name = match atom {
            | aevia_dim::Atom::Base(name) => name.to_string(),
            | aevia_dim::Atom::Var(id) => dims.name(*id),
        };
        let (target, magnitude) = if exponent.is_negative() {
            (&mut denominator, -exponent.clone())
        } else {
            (&mut numerator, exponent.clone())
        };
        if magnitude.is_one() {
            target.push(name);
        } else if magnitude.is_integer() {
            target.push(format!("{name}^{}", magnitude.numer()));
        } else {
            target.push(format!(
                "{name}^({})",
                aevia_dim::format_rational(&magnitude)
            ));
        }
    }
    let numerator = if numerator.is_empty() {
        "1".to_string()
    } else {
        numerator.join("*")
    };
    match denominator.len() {
        | 0 => numerator,
        | 1 => format!("{numerator}/{}", denominator[0]),
        | _ => format!("{numerator}/({})", denominator.join("*")),
    }
}

/// A spelling of `name` that Aevia source can contain.
fn spellable(
    name: &str,
    aliases: &[String],
) -> Option<String> {
    if !is_keyword(name) {
        return Some(name.to_string());
    }
    aliases.iter().find(|alias| !is_keyword(alias)).cloned()
}

/// Words that cannot be a declaration name.
const KEYWORDS: [&str; 40] = [
    "module",
    "import",
    "open",
    "dimension",
    "unit",
    "type",
    "class",
    "instance",
    "derive",
    "fn",
    "let",
    "rec",
    "in",
    "test",
    "match",
    "with",
    "if",
    "then",
    "else",
    "do",
    "for",
    "while",
    "true",
    "false",
    "and",
    "or",
    "not",
    "return",
    "break",
    "continue",
    "where",
    "as",
    "effect",
    "handler",
    "extern",
    "jit",
    "public",
    "private",
    "prop",
    "proof",
];

fn is_keyword(name: &str) -> bool {
    KEYWORDS.contains(&name)
}

/// A name for a `let` binding.
///
/// A capitalised name is a constructor pattern, not a binding, so a constant
/// or a nullary operator is spelled with a lowercase first letter.
fn value_name(name: &str) -> String {
    let sanitised = sanitise(name);
    let mut chars = sanitised.chars();
    match chars.next() {
        | Some(first) => format!("{}{}", first.to_lowercase(), chars.as_str()),
        | None => sanitised,
    }
}

fn sanitise(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for (index, ch) in name.chars().enumerate() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            if index == 0 && ch.is_ascii_digit() {
                out.push('_');
            }
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        return "_".to_string();
    }
    if is_keyword(&out) {
        return format!("{out}_");
    }
    out
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            | '"' => out.push_str("\\\""),
            | '\\' => out.push_str("\\\\"),
            | '\n' => out.push_str("\\n"),
            | '\r' => {},
            | other => out.push(other),
        }
    }
    out
}

/// A rational as source text, exactly.
///
/// Formatting through `f64` turned `1.602176634e-19` into `0.0`; the constant
/// table has to survive the round trip, so the exact form is written.
fn exact_text(value: &num_rational::BigRational) -> String {
    if value.denom().is_one() {
        return value.numer().to_string();
    }
    // A quantity literal is a number followed by a unit, so the coefficient has
    // to be a literal. A rational with a terminating decimal expansion can be
    // written exactly; `parse_decimal` reads it back without rounding, so the
    // constant survives. Anything else falls back to a fraction, which the
    // reader still accepts as an expression but which cannot carry a unit.
    let decimal = aevia_dim::format_rational_decimal(value);
    if aevia_dim::parse_decimal(&decimal).as_ref() == Some(value) {
        return decimal;
    }
    format!("({})/({})", value.numer(), value.denom())
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
