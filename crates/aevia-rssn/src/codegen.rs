//! Generates the `Rssn` modules from the live kernel manifest.
//!
//! Exact, source-representable signatures use typed declarations pinned to
//! their manifest keys. Derived, dynamically represented or unavailable
//! signatures use a value-list dynamic route with an explicit `Result` and
//! a coverage explanation. `namespace_files` and the flat facade consume the
//! same binding catalog; no approximate signature is presented as exact.
//! Units and constants are generated from the same manifest.

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
        "The rssn kernel: exact typed bindings and explicit dynamic fallbacks.",
    );
    // The count is stated so that a reader (and the conformance test) can
    // check the module against `manifest.ops.len()` instead of against the
    // generator's own arithmetic.
    let _ = writeln!(out, "## operators: {}", registry.op_count());

    let _ = writeln!(
        out,
        "## Value, Symbol and OpError are the shared ABI types.\n"
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
        "## Exact overloads pin their keys; other bindings use the dynamic ABI.\n\
         ## Aliases resolve by name and are not declared a second time."
    );
    let catalog = crate::bindings::catalog(registry);
    let mut current = String::new();
    for op in registry.ops() {
        if op.rule_set != current {
            let _ = writeln!(out, "\n# {}.", op.rule_set);
            current.clone_from(&op.rule_set);
        }
        let rows: Vec<_> = catalog
            .iter()
            .filter(|row| std::ptr::eq(row.op, op))
            .collect();
        let _ = writeln!(out, "{}", declaration(op, &rows));
    }
    out
}

/// Optional modules grouped by the registering kernel rule set. They use the
/// same identifiers as the flat facade and can be regenerated as the kernel
/// adds operators. Exact, representable signatures pin their keys; everything
/// else uses the signature-free dynamic ABI with an explicit error result.
#[must_use]
pub fn namespace_files(registry: &Registry) -> Vec<(String, String)> {
    let catalog = crate::bindings::catalog(registry);
    namespace_names(registry)
        .into_iter()
        .map(|(module, group)| {
            let source = namespace_render(registry, &catalog, &module, &group);
            (format!("{module}.aev"), source)
        })
        .collect()
}

/// Generate a single virtual rule-set module by its module name, if present.
/// This avoids rendering unrelated namespaces while resolving an import.
#[must_use]
pub fn namespace_source(
    registry: &Registry,
    name: &str,
) -> Option<String> {
    let (_, group) = namespace_names(registry)
        .into_iter()
        .find(|(module, _)| module == name)?;
    Some(namespace_render(
        registry,
        &crate::bindings::catalog(registry),
        name,
        &group,
    ))
}

fn namespace_names(registry: &Registry) -> Vec<(String, String)> {
    namespace_names_for(registry.ops().map(|op| op.rule_set.as_str()))
}

/// Deterministically assign module names to rule-set names. When sanitisation
/// collides, every member gets a stable hash suffix instead of depending on
/// registration order. A hash collision is resolved by appending the original
/// bytes, so distinct names never share a module.
#[must_use]
pub fn namespace_names_for<'a>(groups: impl IntoIterator<Item = &'a str>) -> Vec<(String, String)> {
    let unique: std::collections::BTreeSet<&str> = groups.into_iter().collect();
    let mut buckets: std::collections::BTreeMap<String, Vec<&str>> =
        std::collections::BTreeMap::new();
    for group in unique {
        buckets
            .entry(format!("Rssn_{}", sanitise(group)))
            .or_default()
            .push(group);
    }
    let mut names = Vec::new();
    for (base, bucket) in buckets {
        if bucket.len() == 1 {
            names.push((base, bucket[0].to_string()));
            continue;
        }
        let mut hashes: std::collections::BTreeMap<u64, Vec<&str>> =
            std::collections::BTreeMap::new();
        for group in bucket {
            let hash = group.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });
            hashes.entry(hash).or_default().push(group);
        }
        for (hash, collisions) in hashes {
            let duplicate = collisions.len() > 1;
            for group in collisions {
                let suffix = if duplicate {
                    format!("_{hash:016x}_{}", hex(group.as_bytes()))
                } else {
                    format!("_{hash:016x}")
                };
                names.push((format!("{base}{suffix}"), group.to_string()));
            }
        }
    }
    // A hashed collision name can itself equal another group's ordinary
    // spelling. Resolve such cross-bucket collisions too, without making the
    // result depend on iteration order.
    loop {
        let mut counts = std::collections::BTreeMap::new();
        for (name, _) in &names {
            *counts.entry(name.clone()).or_insert(0usize) += 1;
        }
        if counts.values().all(|count| *count == 1) {
            break;
        }
        for (name, group) in &mut names {
            if counts[name] > 1 {
                name.push_str(&format!("_x{}", hex(group.as_bytes())));
            }
        }
    }
    names.sort_by(|a, b| a.1.cmp(&b.1));
    names
}

fn namespace_render(
    registry: &Registry,
    catalog: &[crate::bindings::Binding<'_>],
    module: &str,
    group: &str,
) -> String {
    let mut out = String::new();
    header(
        &mut out,
        registry,
        module,
        &format!("Kernel rule set: {group}."),
    );
    for op in registry.ops().filter(|op| op.rule_set == group) {
        let rows: Vec<_> = catalog
            .iter()
            .filter(|row| std::ptr::eq(row.op, op))
            .collect();
        let _ = writeln!(out, "{}", declaration(op, &rows));
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
    // `pi` is the exact symbolic prelude constant. Rounding it to a decimal
    // here would change every constant whose manifest scale includes pi.
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
fn declaration(
    op: &OpEntry,
    rows: &[&crate::bindings::Binding<'_>],
) -> String {
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
        let row = rows[0];
        let _ = writeln!(
            out,
            "## No exact signature; errors are returned by the dynamic ABI."
        );
        if op.higher_order {
            let _ = writeln!(
                out,
                "fn {}(args: Tensor<1, Value>, callbacks: Tensor<1, Callback>): Result<Value, OpError> = Rssn.call_ho(\"{}\", args, callbacks)",
                row.name,
                escape(&op.name)
            );
        } else {
            let _ = writeln!(
                out,
                "fn {}(args: Tensor<1, Value>): Result<Value, OpError> = Rssn.dynamic(\"{}\", args)",
                row.name,
                escape(&op.name)
            );
        }
        return out;
    }
    for (index, signature) in op.signatures.iter().enumerate() {
        let name = &rows[index].name;
        match &rows[index].coverage {
            | crate::bindings::Coverage::Typed => {},
            | crate::bindings::Coverage::Dynamic(reasons) => {
                let _ = writeln!(
                    out,
                    "## Dynamic/approximate signature: {}",
                    reasons.join("; ")
                );
            },
            | crate::bindings::Coverage::Unavailable(reasons) => {
                let _ = writeln!(out, "## Bridge limitation: {}", reasons.join("; "));
            },
        }
        if !matches!(rows[index].coverage, crate::bindings::Coverage::Typed) {
            if op.higher_order {
                let _ = writeln!(
                    out,
                    "fn {name}(args: Tensor<1, Value>, callbacks: Tensor<1, Callback>): Result<Value, OpError> = Rssn.call_ho(\"{}\", args, callbacks)",
                    escape(&op.name)
                );
            } else {
                let _ = writeln!(
                    out,
                    "fn {name}(args: Tensor<1, Value>): Result<Value, OpError> = Rssn.dynamic(\"{}\", args)",
                    escape(&op.name)
                );
            }
            continue;
        }
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
                name = name,
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
        let ret = render_with(&mut dims, &signature.ret);
        let generics = if dims.names.is_empty() {
            String::new()
        } else {
            let mut names: Vec<_> = dims.names.values().cloned().collect();
            names.sort();
            format!("<{}>", names.join(", "))
        };
        let _ = writeln!(
            out,
            "fn {name}{generics}({params}): {ret} = Rssn.invoke(\"{op_name}\", \"{hash}\", {args})",
            params = params.join(", "),
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

/// Names for the explicitly bound dimension variables of one declaration (`d`, `d2`, …).
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
            "d".to_string()
        } else {
            format!("d{}", self.next + 1)
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
        | SigType::Tensor {
            rank: Some(rank),
            shape: Some(shape),
            elem,
        } if *rank == shape.len() => {
            format!(
                "Tensor<[{}], {}>",
                shape
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
                render_with(dims, elem)
            )
        },
        | SigType::Tensor { shape: Some(_), .. } => "Dyn<term>".to_string(),
        | SigType::Tensor { rank, elem, .. } | SigType::Sparse { rank, elem } => {
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

/// Preferred source identifier for an overload before collision disambiguation.
#[must_use]
pub fn binding_base_name(
    name: &str,
    index: usize,
    nullary: bool,
) -> String {
    let base = if index == 0 {
        sanitise(name)
    } else {
        format!("{}_{}", sanitise(name), index + 1)
    };
    if nullary {
        value_name(&base)
    } else {
        base
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
