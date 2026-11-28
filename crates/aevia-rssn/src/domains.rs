//! Domain modules: the manifest, grouped by mathematical domain, with a small
//! declarative overlay for the curated parts.
//!
//! # Why this exists
//!
//! The flat `Rssn` facade and the per-rule-set `Rssn_<rule_set>` namespaces
//! (see [`crate::codegen`]) follow the *kernel's* organisation. Neither can say
//! "this operator takes a prime modulus and returns a residue": the kernel
//! signature of `invmod` is `Real -> Real` (derived), and a kernel rule set
//! such as `discrete` is a bag of finite fields, graphs, codes and cryptography.
//!
//! This module adds the *Aevia-facing* layer, as data:
//!
//! * a **domain** table (`domains.overlay`): which operators form
//!   `Rssn_NumberTheory`, `Rssn_FiniteField`, `Rssn_Group`, ...;
//! * per-operator **typed signatures** (`sig`) written in Aevia types, from
//!   which the wrapper (argument encoding, `Result` error channel, result
//!   decoding) is *derived*, never written by hand;
//! * **codecs** for the few rich Aevia types whose wire form differs from their
//!   source form (for example a 0-based permutation vs the kernel's 1-based
//!   list); one pair of functions per type, not per operator;
//! * explicit **exclusions** and accepted-dynamic declarations, each with a
//!   reason.
//!
//! Everything not mentioned in the overlay still gets a generated wrapper, so a
//! newly added kernel operator is callable from its domain module on the next
//! run without any edit. Which wrapper depends on what the kernel knows:
//!
//! * the **witnessed** tier: the kernel observed the operator answering real
//!   calls and recorded the sorts of arguments and answers
//!   (`rssn::kernel::sort`; "applied to an integer, `divisors` is equal to a
//!   list of integers"). When such an overload is expressible in Aevia types
//!   the wrapper is typed exactly like a curated one, with the recorded call
//!   quoted in its documentation. Nobody wrote the signature: it follows the
//!   kernel's own test suite, so a new, tested kernel operator arrives typed;
//! * the **dynamic** tier, `Result<Value, OpError>`, for the rest.
//!
//! A witnessed signature is evidence, not a totality proof, which is exactly
//! what the wrapper's `Result` channel is for: `from_value<T>` validates the
//! answer and an argument outside what was observed yields `Err`, never a
//! value of the wrong type.
//!
//! What the overlay adds is a ratchet: [`problems`] fails when an operator is in a
//! domain declared `typed` and has neither a signature nor an exclusion, or
//! when a kernel rule set has no domain at all.
//!
//! # Build time or run time
//!
//! The modules are rendered at *module resolution* time from the live manifest
//! (`import Rssn_NumberTheory`, resolved by the driver through
//! [`crate::codegen::namespace_source`]), exactly like the rule-set namespaces:
//! no generated Aevia source is committed, so the facade cannot drift from the
//! kernel it is linked against. Only the overlay and the coverage report
//! (`tools/rssn-api-audit/RSSN-COVERAGE.md`) are committed; a test regenerates
//! the report and fails on a difference.

use crate::registry::OpEntry;
use crate::registry::Registry;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::OnceLock;

/// The overlay source, embedded.
pub const OVERLAY_SOURCE: &str = include_str!("../domains.overlay");

/// Prefix of a generated domain module name: `Rssn_NumberTheory`.
pub const MODULE_PREFIX: &str = "Rssn_";

/// One domain.
#[derive(Clone, Debug)]
pub struct Domain {
    /// CamelCase name: `NumberTheory`.
    pub name: String,
    /// One-line description.
    pub doc: String,
    /// Whether every operator must be typed, excluded or accepted-dynamic.
    pub typed_required: bool,
    /// Raw Aevia source placed before the generated wrappers (types, codecs).
    pub prelude: String,
}

/// A rule-set / name pattern assigned to a domain.
#[derive(Clone, Debug)]
pub struct Family {
    /// Domain the matching operators belong to.
    pub domain: String,
    /// Kernel rule set, exactly.
    pub rule_set: String,
    /// Operator-name glob (`*` matches any run of characters).
    pub glob: String,
}

/// A curated typed signature.
#[derive(Clone, Debug)]
pub struct SigDef {
    /// Kernel operator name.
    pub op: String,
    /// Aevia function name (defaults to the sanitised operator name).
    pub name: String,
    /// Explicit generic binders, in source declaration order.
    pub generics: Vec<String>,
    /// Parameters: `(name, Aevia type)`.
    pub params: Vec<(String, String)>,
    /// The success type `T` of the declared `Result<T, OpError>`.
    pub ret: String,
    /// 1-based line in the overlay, for diagnostics.
    pub line: usize,
}

/// A rich type's wire conversion.
#[derive(Clone, Debug)]
pub struct Codec {
    /// Aevia type name (head identifier).
    pub ty: String,
    /// `T -> Value`; empty for a decode-only type (`codec T - from_fn`).
    pub to_value: String,
    /// `Value -> Result<T, OpError>`.
    pub from_value: String,
}

/// The parsed overlay.
#[derive(Clone, Debug, Default)]
pub struct Overlay {
    /// Domains in declaration order.
    pub domains: Vec<Domain>,
    /// Families in declaration order; the first match wins.
    pub families: Vec<Family>,
    /// Typed signatures by operator, in declaration order. One operator can have
    /// several curated signatures, each under its own Aevia name (`sig heat_fd
    /// as heat_fd_2d(..)`): the kernel operator is rank polymorphic, Aevia types
    /// carry a literal rank, so each rank is one signature. The first signature
    /// of an operator that does not rename it keeps the operator's name.
    pub sigs: BTreeMap<String, Vec<SigDef>>,
    /// Excluded operators and why.
    pub excluded: BTreeMap<String, String>,
    /// Operators accepted as dynamic and why.
    pub accepted_dynamic: BTreeMap<String, String>,
    /// Codecs by type head.
    pub codecs: BTreeMap<String, Codec>,
    /// Errors found while parsing.
    pub errors: Vec<String>,
}

impl Overlay {
    /// Parse overlay text. Errors are collected in [`Overlay::errors`] so that
    /// one run reports every mistake.
    #[must_use]
    #[allow(
        clippy::too_many_lines,
        reason = "Reports all overlay directives through one source line cursor"
    )]
    pub fn parse(source: &str) -> Self {
        let mut overlay = Self::default();
        let mut lines = (1usize..).zip(source.lines()).peekable();
        while let Some((number, line)) = lines.next() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (keyword, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            let rest = rest.trim();
            match keyword {
                | "domain" => {
                    let (name, doc) = split_bar(rest);
                    overlay.domains.push(Domain {
                        name: name.to_string(),
                        doc: doc.to_string(),
                        typed_required: false,
                        prelude: String::new(),
                    });
                },
                | "family" => {
                    let mut words = rest.split_whitespace();
                    let Some(domain) = words.next() else {
                        overlay
                            .errors
                            .push(format!("line {number}: `family` needs a domain"));
                        continue;
                    };
                    for pattern in words {
                        let Some((rule_set, glob)) = pattern.split_once(':') else {
                            overlay.errors.push(format!(
                                "line {number}: pattern `{pattern}` is not `rule_set:glob`"
                            ));
                            continue;
                        };
                        overlay.families.push(Family {
                            domain: domain.to_string(),
                            rule_set: rule_set.to_string(),
                            glob: glob.to_string(),
                        });
                    }
                },
                | "mode" => {
                    let mut words = rest.split_whitespace();
                    let (Some(domain), Some(mode)) = (words.next(), words.next()) else {
                        overlay.errors.push(format!(
                            "line {number}: `mode` needs `Domain typed|dynamic`"
                        ));
                        continue;
                    };
                    match overlay.domains.iter_mut().find(|d| d.name == domain) {
                        | Some(found) => found.typed_required = mode == "typed",
                        | None => {
                            overlay.errors.push(format!(
                                "line {number}: `mode` names unknown domain `{domain}`"
                            ));
                        },
                    }
                },
                | "sig" => {
                    match parse_sig(rest, number) {
                        | Ok(sig) => {
                            let all = overlay.sigs.entry(sig.op.clone()).or_default();
                            if all.iter().any(|other| other.name == sig.name) {
                                overlay.errors.push(format!(
                                    "line {number}: duplicate `sig` for `{}` named `{}`",
                                    sig.op, sig.name
                                ));
                            } else {
                                all.push(sig);
                            }
                        },
                        | Err(error) => overlay.errors.push(error),
                    }
                },
                | "exclude" | "dynamic" => {
                    let (ops, reason) = split_bar(rest);
                    if reason.is_empty() {
                        overlay
                            .errors
                            .push(format!("line {number}: `{keyword}` needs `| reason`"));
                        continue;
                    }
                    let table = if keyword == "exclude" {
                        &mut overlay.excluded
                    } else {
                        &mut overlay.accepted_dynamic
                    };
                    for op in ops.split_whitespace() {
                        table.insert(op.to_string(), reason.to_string());
                    }
                },
                | "codec" => {
                    let words: Vec<&str> = rest.split_whitespace().collect();
                    if let [ty, to, from] = words[..] {
                        overlay.codecs.insert(
                            ty.to_string(),
                            Codec {
                                ty: ty.to_string(),
                                // `-` declares a decode-only type: the kernel
                                // produces it (a decomposition record) but never
                                // takes it, so no encoder exists.
                                to_value: if to == "-" {
                                    String::new()
                                } else {
                                    to.to_string()
                                },
                                from_value: from.to_string(),
                            },
                        );
                    } else {
                        overlay
                            .errors
                            .push(format!("line {number}: `codec` needs `Type to_fn from_fn`"));
                    }
                },
                | "prelude" => {
                    let domain = rest.to_string();
                    let mut text = String::new();
                    while let Some(body) = lines
                        .peek()
                        .and_then(|(_, line)| line.trim_start().strip_prefix('|'))
                    {
                        text.push_str(body.strip_prefix(' ').unwrap_or(body));
                        text.push('\n');
                        lines.next();
                    }
                    match overlay.domains.iter_mut().find(|d| d.name == domain) {
                        | Some(found) => found.prelude.push_str(&text),
                        | None => {
                            overlay.errors.push(format!(
                                "line {number}: `prelude` names unknown domain `{domain}`"
                            ));
                        },
                    }
                },
                | other => {
                    overlay
                        .errors
                        .push(format!("line {number}: unknown directive `{other}`"));
                },
            }
        }
        overlay
    }
}

fn split_bar(text: &str) -> (&str, &str) {
    match text.split_once('|') {
        | Some((left, right)) => (left.trim(), right.trim()),
        | None => (text.trim(), ""),
    }
}

/// Split at top-level commas (angle, round and square brackets nest).
#[allow(
    clippy::single_call_fn,
    reason = "Isolates top-level comma recognition from signature field validation"
)]
fn split_top(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for ch in text.chars() {
        match ch {
            | '<' | '(' | '[' | '{' => depth = depth.saturating_add(1),
            | '>' | ')' | ']' | '}' => depth = depth.saturating_sub(1),
            | _ => {},
        }
        if ch == ',' && depth == 0 {
            parts.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

/// `op [as name]<d, ...>(p: T, ...) -> Result<R, OpError>`
#[allow(
    clippy::single_call_fn,
    reason = "Keeps nesting and diagnostic handling apart from the overlay line cursor"
)]
fn parse_sig(
    text: &str,
    line: usize,
) -> Result<SigDef, String> {
    let (head, tail) = text
        .split_once('(')
        .ok_or_else(|| format!("line {line}: `sig` needs a parameter list"))?;
    let (head, generics) = if let Some((head, parameters)) = head.split_once('<') {
        let invalid = || format!("line {line}: expected distinct generic parameters in `<d, ...>`");
        let body = parameters.trim().strip_suffix('>').ok_or_else(invalid)?;
        let mut names = Vec::new();
        for name in body.split(',').map(str::trim) {
            let mut chars = name.chars();
            if !chars.next().is_some_and(|c| c.is_ascii_alphabetic())
                || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
                || names.iter().any(|previous| previous == name)
            {
                return Err(invalid());
            }
            names.push(name.to_string());
        }
        (head, names)
    } else {
        (head, Vec::new())
    };
    let head: Vec<&str> = head.split_whitespace().collect();
    let (op, name) = match head[..] {
        | [op] => (op.to_string(), crate::codegen::sanitise(op)),
        | [op, "as", name] => (op.to_string(), name.to_string()),
        | _ => return Err(format!("line {line}: expected `sig op [as name](...)`")),
    };
    // The parameter list ends at the matching parenthesis.
    let mut depth = 0usize;
    let mut close = None;
    for (offset, ch) in tail.char_indices() {
        match ch {
            | '(' => depth = depth.saturating_add(1),
            | ')' => {
                if depth == 0 {
                    close = tail.split_at_checked(offset);
                    break;
                }
                depth = depth.saturating_sub(1);
            },
            | _ => {},
        }
    }
    let (body, after) = close.ok_or_else(|| format!("line {line}: unbalanced parameter list"))?;
    let mut params = Vec::new();
    for part in split_top(body) {
        let (pname, pty) = part
            .split_once(':')
            .ok_or_else(|| format!("line {line}: parameter `{part}` needs `name: Type`"))?;
        params.push((pname.trim().to_string(), pty.trim().to_string()));
    }
    let ret = after
        .strip_prefix(')')
        .and_then(|rest| rest.trim().strip_prefix("->"))
        .map(str::trim)
        .ok_or_else(|| format!("line {line}: `sig` needs `-> Result<T, OpError>`"))?;
    let inner = ret
        .strip_prefix("Result<")
        .and_then(|rest| rest.strip_suffix(", OpError>"))
        .ok_or_else(|| {
            format!(
                "line {line}: the result must be `Result<T, OpError>`; the kernel can always fail"
            )
        })?;
    Ok(SigDef {
        op,
        name,
        generics,
        params,
        ret: inner.trim().to_string(),
        line,
    })
}

/// The embedded overlay, parsed once.
#[must_use]
pub fn overlay() -> &'static Overlay {
    static OVERLAY: OnceLock<Overlay> = OnceLock::new();
    OVERLAY.get_or_init(|| Overlay::parse(OVERLAY_SOURCE))
}

/// The parameter names of the curated wrapper `function`, when it is one.
///
/// This is how a kernel finding about "argument 3" becomes "argument `axes_a`
/// (#3)": the kernel counts positions, the wrapper has names. Witnessed and
/// dynamic wrappers have no curated names and answer `None`.
#[must_use]
pub fn wrapper_params(
    function: &str,
    op: &str,
) -> Option<Vec<String>> {
    overlay()
        .sigs
        .get(op)?
        .iter()
        .find(|sig| sig.name == function)
        .map(|sig| sig.params.iter().map(|(name, _)| name.clone()).collect())
}

/// How an operator is exposed by its domain module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// A curated, typed wrapper with a `Result` error channel.
    Typed,
    /// A typed wrapper generated from an overload the kernel witnessed.
    Witnessed,
    /// A generated `Result<Value, OpError>` wrapper; `Some(reason)` when the
    /// overlay accepts that explicitly.
    Dynamic(Option<String>),
    /// Deliberately not in the domain module.
    Excluded(String),
    /// No family claims the operator.
    Unclassified,
}

/// One operator's placement.
#[derive(Clone, Debug)]
pub struct Placement<'a> {
    /// The operator.
    pub op: &'a OpEntry,
    /// Its domain, if a family claims it.
    pub domain: Option<&'a Domain>,
    /// How it is exposed.
    pub status: Status,
}

#[allow(
    clippy::single_call_fn,
    reason = "Keeps prefix, interior and suffix matching separate from rule precedence"
)]
fn glob_matches(
    glob: &str,
    name: &str,
) -> bool {
    let mut parts = glob.split('*');
    let (Some(prefix), Some(suffix)) = (parts.next(), parts.next_back()) else {
        return glob == name;
    };
    let Some(mut rest) = name.strip_prefix(prefix) else {
        return false;
    };
    for part in parts {
        let Some((_, remaining)) = rest.split_once(part) else {
            return false;
        };
        rest = remaining;
    }
    rest.ends_with(suffix)
}

/// Place every operator of the manifest.
#[must_use]
pub fn classify<'a>(
    registry: &'a Registry,
    overlay: &'a Overlay,
) -> Vec<Placement<'a>> {
    classify_placements(registry, overlay, None)
}

fn classify_placements<'a>(
    registry: &'a Registry,
    overlay: &'a Overlay,
    only_domain: Option<&str>,
) -> Vec<Placement<'a>> {
    let capacity = if only_domain.is_none() {
        registry.op_count()
    } else {
        0
    };
    let mut placements = Vec::with_capacity(capacity);
    for op in registry.ops() {
        let domain = overlay
            .families
            .iter()
            .find(|family| family.rule_set == op.rule_set && glob_matches(&family.glob, &op.name))
            .and_then(|family| overlay.domains.iter().find(|d| d.name == family.domain));
        // The first matching family still wins. Skip the type and witness
        // checks only after establishing that the operator is in another domain.
        if only_domain.is_some_and(|name| domain.is_none_or(|d| d.name != name)) {
            continue;
        }
        let status = if domain.is_none() {
            Status::Unclassified
        } else if let Some(reason) = overlay.excluded.get(&op.name) {
            Status::Excluded(reason.clone())
        } else if overlay.sigs.contains_key(&op.name) {
            Status::Typed
        } else if !overlay.accepted_dynamic.contains_key(&op.name) && !witnessed_sigs(op).is_empty()
        {
            Status::Witnessed
        } else {
            Status::Dynamic(overlay.accepted_dynamic.get(&op.name).cloned())
        };
        placements.push(Placement { op, domain, status });
    }
    placements
}

/// The coverage check. An empty result means the overlay and the manifest
/// agree; each entry is one thing to fix, written so that it names the fix.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "Lists independent overlay consistency checks in diagnostic order"
)]
pub fn problems(
    registry: &Registry,
    overlay: &Overlay,
) -> Vec<String> {
    let mut out: Vec<String> = overlay.errors.clone();
    let known: BTreeSet<&str> = registry.ops().map(|op| op.name.as_str()).collect();
    let rule_sets: BTreeSet<&str> = registry.ops().map(|op| op.rule_set.as_str()).collect();
    for (what, names) in [
        ("sig", overlay.sigs.keys().collect::<Vec<_>>()),
        ("exclude", overlay.excluded.keys().collect()),
        ("dynamic", overlay.accepted_dynamic.keys().collect()),
    ] {
        for name in names {
            if !known.contains(name.as_str()) {
                out.push(format!(
                    "overlay `{what} {name}`: the manifest has no such operator (renamed or removed in the kernel?)"
                ));
            }
        }
    }
    for name in overlay.sigs.keys() {
        if overlay.excluded.contains_key(name) {
            out.push(format!("`{name}` has both a `sig` and an `exclude`"));
        }
    }
    for family in &overlay.families {
        if !overlay.domains.iter().any(|d| d.name == family.domain) {
            out.push(format!("family names unknown domain `{}`", family.domain));
        }
        if !rule_sets.contains(family.rule_set.as_str()) {
            out.push(format!(
                "family `{}:{}` names a rule set the manifest does not have",
                family.rule_set, family.glob
            ));
        }
    }
    for placement in classify(registry, overlay) {
        let op = placement.op;
        match &placement.status {
            | Status::Unclassified => out.push(format!(
                "operator `{}:{}` is in no domain: add a `family` line to domains.overlay (or `exclude` it with a reason)",
                op.rule_set, op.name
            )),
            | Status::Dynamic(None)
                if placement.domain.is_some_and(|domain| domain.typed_required) =>
            {
                out.push(format!(
                    "operator `{}:{}` is in typed domain `{}` but has no `sig`, `dynamic` or `exclude` entry",
                    op.rule_set,
                    op.name,
                    placement.domain.map_or("?", |d| d.name.as_str())
                ));
            },
            | Status::Typed => {
                for sig in overlay.sigs.get(&op.name).into_iter().flatten() {
                    let (min, max) = op.arity;
                    let n = sig.params.len();
                    if n < min || max.is_some_and(|max| n > max) {
                        out.push(format!(
                            "overlay line {}: `sig {}` takes {n} parameters but the kernel accepts {min}..{}",
                            sig.line,
                            op.name,
                            max.map_or_else(|| "*".to_string(), |m| m.to_string())
                        ));
                    }
                    if op.higher_order {
                        out.push(format!(
                            "overlay line {}: `{}` takes callbacks; a `sig` cannot express that, use the generated callback binding",
                            sig.line, op.name
                        ));
                    }
                    for (_, ty) in &sig.params {
                        check_type_known(ty, &sig.generics, overlay, &mut out, sig.line);
                        if let Some(codec) = overlay.codecs.get(head(ty))
                            && codec.to_value.is_empty()
                        {
                            out.push(format!(
                                "overlay line {}: parameter type `{ty}` is decode-only (`codec {} - ..`) and cannot be passed to the kernel",
                                sig.line, codec.ty
                            ));
                        }
                    }
                    check_type_known(&sig.ret, &sig.generics, overlay, &mut out, sig.line);
                }
            },
            | _ => {},
        }
    }
    for codec in overlay.codecs.values() {
        for function in [&codec.to_value, &codec.from_value] {
            if function.is_empty() {
                continue;
            }
            let declared = overlay
                .domains
                .iter()
                .any(|d| d.prelude.contains(&format!("fn {function}(")));
            if !declared {
                out.push(format!(
                    "codec `{}` names `{function}`, which no prelude declares",
                    codec.ty
                ));
            }
        }
    }
    out
}

/// Types in a `sig` must be builtin, generic, or declared by a prelude.
fn check_type_known(
    ty: &str,
    generics: &[String],
    overlay: &Overlay,
    out: &mut Vec<String>,
    line: usize,
) {
    const BUILTIN: &[&str] = &[
        "Int", "Float", "Real", "Bool", "String", "Unit", "Tensor", "Value", "Option", "Complex",
    ];
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut Vec<String>| {
        if word.chars().next().is_some_and(char::is_uppercase)
            && !BUILTIN.contains(&word.as_str())
            && !generics.contains(word)
            && !overlay
                .domains
                .iter()
                .any(|d| d.prelude.contains(&format!("type {word}")))
        {
            out.push(format!(
                "overlay line {line}: type `{word}` is neither builtin nor declared in a prelude"
            ));
        }
        word.clear();
    };
    for ch in ty.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            word.push(ch);
        } else {
            flush(&mut word, out);
        }
    }
    flush(&mut word, out);
}

/// Module name of a domain.
#[must_use]
pub fn module_name(domain: &str) -> String {
    format!("{MODULE_PREFIX}{domain}")
}

/// All domain module names, in declaration order.
#[must_use]
pub fn module_names() -> Vec<String> {
    overlay()
        .domains
        .iter()
        .map(|d| module_name(&d.name))
        .collect()
}

/// Render one domain module, if `module` names one.
#[must_use]
pub fn domain_source(
    registry: &Registry,
    module: &str,
) -> Option<String> {
    let name = module.strip_prefix(MODULE_PREFIX)?;
    let overlay = overlay();
    let domain = overlay.domains.iter().find(|d| d.name == name)?;
    let placements = classify_placements(registry, overlay, Some(name));
    Some(render(registry, overlay, domain, &placements))
}

/// Every domain module as `(file name, source)`.
#[must_use]
pub fn domain_files(registry: &Registry) -> Vec<(String, String)> {
    let overlay = overlay();
    let placements = classify(registry, overlay);
    overlay
        .domains
        .iter()
        .map(|d| {
            (
                format!("{}.aev", module_name(&d.name)),
                render(registry, overlay, d, &placements),
            )
        })
        .collect()
}

/// How a value of Aevia type `ty` is put on the wire.
#[allow(
    clippy::single_call_fn,
    reason = "Keeps generated parameter encoding and result decoding independently readable"
)]
fn encode(
    overlay: &Overlay,
    ty: &str,
    expr: &str,
) -> String {
    match overlay.codecs.get(head(ty)) {
        | Some(codec) => format!("{}({expr})", codec.to_value),
        | None => format!("to_value({expr})"),
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "Keeps generated result decoding and parameter encoding independently readable"
)]
fn decode(
    overlay: &Overlay,
    ty: &str,
    expr: &str,
) -> String {
    match overlay.codecs.get(head(ty)) {
        | Some(codec) => format!("{}({expr})", codec.from_value),
        | None => format!("from_value<{ty}>({expr})"),
    }
}

fn head(ty: &str) -> &str {
    ty.split(['<', '[', ' ']).next().unwrap_or(ty)
}

fn render(
    registry: &Registry,
    overlay: &Overlay,
    domain: &Domain,
    placements: &[Placement<'_>],
) -> String {
    let mut out = String::new();
    let module = module_name(&domain.name);
    let _ = writeln!(
        out,
        "## {doc}\n\
         ##\n\
         ## Generated from the rssn Kernel Manifest and `domains.overlay`. Do not edit:\n\
         ## rendered on import; see `domains::domain_files`.\n\
         ## rssn {version}, api {api}, manifest {hash}\n\
         module {module}\n",
        doc = domain.doc,
        version = registry.version,
        api = registry.api_version,
        hash = registry.manifest_hash,
    );
    if !domain.prelude.is_empty() {
        let _ = writeln!(
            out,
            "# --- types and codecs (curated) ---\n{}",
            domain.prelude
        );
    }
    let _ = writeln!(out, "# --- operators ---\n");
    let mut used: BTreeSet<String> = BTreeSet::new();
    // Names the prelude declares are reserved: a generated wrapper must not
    // silently shadow them.
    for line in domain.prelude.lines() {
        if let Some(rest) = line.strip_prefix("fn ")
            && let Some(name) = rest.split(['(', '<']).next()
        {
            used.insert(name.trim().to_string());
        }
    }
    for placement in placements {
        if placement.domain.is_none_or(|d| d.name != domain.name) {
            continue;
        }
        let op = placement.op;
        match &placement.status {
            | Status::Excluded(reason) => {
                let _ = writeln!(out, "## `{}` is not bound here: {reason}\n", op.name);
            },
            | Status::Typed => {
                for sig in overlay.sigs.get(&op.name).into_iter().flatten() {
                    let name = unique(&mut used, &sig.name);
                    typed_wrapper(&mut out, overlay, op, sig, &name);
                }
            },
            | Status::Witnessed => {
                let base = crate::codegen::sanitise(&op.name);
                for (index, sig) in witnessed_sigs(op).iter().enumerate() {
                    // The best-supported overload takes the operator's name;
                    // further ones are named after what distinguishes them.
                    let wanted = if index == 0 {
                        base.clone()
                    } else {
                        format!("{base}_{}", sig.tag)
                    };
                    let name = unique(&mut used, &wanted);
                    witnessed_wrapper(&mut out, op, sig, &name);
                }
            },
            | Status::Dynamic(reason) => {
                let name = unique(&mut used, &crate::codegen::sanitise(&op.name));
                dynamic_wrapper(&mut out, op, &name, reason.as_deref());
            },
            | Status::Unclassified => {},
        }
    }
    out
}

fn unique(
    used: &mut BTreeSet<String>,
    base: &str,
) -> String {
    let mut name = base.to_string();
    for suffix in 2.. {
        if used.insert(name.clone()) {
            break;
        }
        name = format!("{base}_{suffix}");
    }
    name
}

fn docs(
    out: &mut String,
    op: &OpEntry,
    tier: &str,
) {
    let _ = writeln!(out, "## `{}.{}` ({tier})", op.rule_set, op.name);
    for line in op.doc.lines() {
        let _ = writeln!(out, "## {}", line.trim_end());
    }
    if !op.examples.is_empty() {
        let _ = writeln!(out, "## Examples:");
        for ex in &op.examples {
            if ex.note.is_empty() {
                let _ = writeln!(out, "##   `{}` => `{}`", ex.input, ex.output);
            } else {
                let _ = writeln!(out, "##   `{}` => `{}` ({})", ex.input, ex.output, ex.note);
            }
        }
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "Renders the overlay-typed facade at one auditable template boundary"
)]
fn typed_wrapper(
    out: &mut String,
    overlay: &Overlay,
    op: &OpEntry,
    sig: &SigDef,
    name: &str,
) {
    docs(out, op, "typed");
    let generics = if sig.generics.is_empty() {
        String::new()
    } else {
        format!("<{}>", sig.generics.join(", "))
    };
    let params: Vec<String> = sig
        .params
        .iter()
        .map(|(n, t)| format!("{n}: {t}"))
        .collect();
    let args: Vec<String> = sig
        .params
        .iter()
        .map(|(n, t)| encode(overlay, t, n))
        .collect();
    let _ = writeln!(
        out,
        "fn {name}{generics}({params}): Result<{ret}, OpError> =\n    match Rssn.dynamic(\"{op}\", [{args}]) with\n    | Ok(wire) -> {decoded}\n    | Err(failure) -> Err(failure)\n",
        params = params.join(", "),
        ret = sig.ret,
        op = crate::codegen::escape(&op.name),
        args = args.join(", "),
        decoded = decode(overlay, &sig.ret, "wire"),
    );
}

/// A witnessed overload in Aevia types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WitnessedSig {
    /// Parameter types.
    pub params: Vec<String>,
    /// The success type of the `Result`.
    pub ret: String,
    /// A short word for the overload (`int`, `real`, `float`), used to name
    /// the wrappers after the first.
    pub tag: &'static str,
}

/// The Aevia spelling of a witnessed type, when it has one.
///
/// A symbolic scalar is `Real` (an Aevia `Real` is an exact number *or* an
/// expression), a rectangular list is a tensor of its rank, and a structured
/// kernel object (a graph, a group, a sparse matrix: a term headed by its
/// constructor) is an opaque `Value` handle, which is what it is on the Aevia
/// side: it can be passed back to the kernel and nothing else. Tuples and
/// ragged lists have no general Aevia type; an overload that mentions one
/// stays in the dynamic tier.
fn witnessed_type(ty: &crate::registry::SigType) -> Option<String> {
    use crate::registry::SigType as S;
    Some(match ty {
        | S::Int => "Int".to_string(),
        | S::Real | S::Symbol | S::Dyn => "Real".to_string(),
        | S::Float => "Float".to_string(),
        | S::Bool => "Bool".to_string(),
        | S::Str => "String".to_string(),
        | S::Complex => "Complex".to_string(),
        | S::Opaque(_) => "Value".to_string(),
        | S::Tensor { rank, elem, .. } => {
            let rank = (*rank)?;
            let elem = match elem.as_ref() {
                | S::Int => "Int",
                | S::Real | S::Symbol | S::Dyn => "Real",
                | S::Float => "Float",
                | S::Bool => "Bool",
                | _ => return None,
            };
            format!("Tensor<{rank}, {elem}>")
        },
        | _ => return None,
    })
}

/// The witnessed overloads of `op` that Aevia can state, best supported
/// first (the kernel's order).
#[must_use]
pub fn witnessed_sigs(op: &OpEntry) -> Vec<WitnessedSig> {
    if op.higher_order {
        return Vec::new();
    }
    let mut out: Vec<WitnessedSig> = Vec::new();
    // Witnessed overloads first, then the refinements: a refinement has the
    // parameters of the symbolic contract (everything `Real`) and a sharper
    // result, which the wrapper's `Result` channel checks.
    let overloads = op
        .witnessed
        .iter()
        .filter_map(|&index| op.signatures.get(index));
    for sig in overloads.chain(&op.refinements) {
        if sig.variadic.is_some() {
            continue;
        }
        let params: Option<Vec<String>> = sig.params.iter().map(witnessed_type).collect();
        let (Some(params), Some(ret)) = (params, witnessed_type(&sig.ret)) else {
            continue;
        };
        // Only handles in and a handle out says nothing a caller can check:
        // that is the dynamic wrapper.
        if ret == "Value" && params.iter().all(|p| p == "Value") {
            continue;
        }
        let mentions = |word: &str| params.iter().chain([&ret]).any(|t| t.contains(word));
        let tag = if mentions("Float") {
            "float"
        } else if mentions("Real") {
            "real"
        } else if mentions("Int") {
            "int"
        } else {
            "bool"
        };
        let candidate = WitnessedSig { params, ret, tag };
        // Two kernel overloads can collapse to one Aevia signature (a symbol
        // and a term are both `Real`).
        if !out.iter().any(|kept| kept.params == candidate.params) {
            out.push(candidate);
        }
    }
    out
}

/// Parameter names for a witnessed wrapper.
///
/// The kernel records no names, so
/// they come from the Aevia type: `n` for an integer, `x` for a scalar, `xs`
/// for a vector, `m` for a matrix, `t3` for a rank-3 tensor, `flag` for a
/// boolean and `v` for a handle. A kind that occurs more than once is
/// numbered (`x1`, `x2`) so the names stay distinct.
#[must_use]
pub fn witnessed_param_names(types: &[String]) -> Vec<String> {
    let base = |ty: &str| -> String {
        match ty {
            | "Int" => "n".to_string(),
            | "Real" | "Float" => "x".to_string(),
            | "Bool" => "flag".to_string(),
            | "String" => "s".to_string(),
            | "Complex" => "z".to_string(),
            | "Value" => "v".to_string(),
            | _ => {
                match ty
                    .strip_prefix("Tensor<")
                    .and_then(|rest| rest.split(',').next())
                    .and_then(|rank| rank.trim().parse::<usize>().ok())
                {
                    | Some(1) => "xs".to_string(),
                    | Some(2) => "m".to_string(),
                    | Some(rank) => format!("t{rank}"),
                    | None => "a".to_string(),
                }
            },
        }
    };
    let bases: Vec<String> = types.iter().map(|t| base(t)).collect();
    let mut totals = BTreeMap::<&str, usize>::new();
    for base in &bases {
        let total = totals.entry(base).or_default();
        *total = total.saturating_add(1);
    }
    let mut seen = std::collections::BTreeMap::<&str, usize>::new();
    bases
        .iter()
        .map(|b| {
            if totals.get(b.as_str()) == Some(&1) {
                b.clone()
            } else {
                let k = seen.entry(b.as_str()).or_insert(0);
                *k = k.saturating_add(1);
                format!("{b}{k}")
            }
        })
        .collect()
}

#[allow(
    clippy::single_call_fn,
    reason = "Renders the witnessed facade separately from curated signatures"
)]
fn witnessed_wrapper(
    out: &mut String,
    op: &OpEntry,
    sig: &WitnessedSig,
    name: &str,
) {
    docs(out, op, "witnessed");
    let _ = writeln!(
        out,
        "## Typed from kernel evidence, not by hand: the engine was observed to satisfy this\n## signature and it replays from a recorded call."
    );
    if let Some((call, sort)) = op.witnesses.first() {
        let _ = writeln!(out, "## Witness: `{call}` has sort `{sort}`.");
    }
    let names = witnessed_param_names(&sig.params);
    let params: Vec<String> = sig
        .params
        .iter()
        .zip(&names)
        .map(|(t, n)| format!("{n}: {t}"))
        .collect();
    let args: Vec<String> = names.iter().map(|n| format!("to_value({n})")).collect();
    let _ = writeln!(
        out,
        "fn {name}({params}): Result<{ret}, OpError> =\n    match Rssn.dynamic(\"{op}\", [{args}]) with\n    | Ok(wire) -> {decoded}\n    | Err(failure) -> Err(failure)\n",
        params = params.join(", "),
        ret = sig.ret,
        op = crate::codegen::escape(&op.name),
        args = args.join(", "),
        // A handle is returned as it is; anything else is validated.
        decoded = if sig.ret == "Value" {
            "Ok(wire)".to_string()
        } else {
            format!("from_value<{}>(wire)", sig.ret)
        },
    );
}

#[allow(
    clippy::single_call_fn,
    reason = "Renders the value-list facade separately from typed signatures"
)]
fn dynamic_wrapper(
    out: &mut String,
    op: &OpEntry,
    name: &str,
    accepted: Option<&str>,
) {
    docs(out, op, "dynamic");
    match accepted {
        | Some(reason) => {
            let _ = writeln!(out, "## Dynamic by decision: {reason}");
        },
        | None => {
            let _ = writeln!(
                out,
                "## Dynamic: no curated signature yet; the result is a `Value` (`from_value<T>` it)."
            );
        },
    }
    let (min, max) = op.arity;
    let op_name = crate::codegen::escape(&op.name);
    if op.higher_order {
        let _ = writeln!(
            out,
            "fn {name}(args: Tensor<1, Value>, callbacks: Tensor<1, Callback>): Result<Value, OpError> = Rssn.call_ho(\"{op_name}\", args, callbacks)\n"
        );
    } else if max == Some(min) && min <= 8 {
        let generics: Vec<String> = (0..min).map(|i| format!("T{i}")).collect();
        let params: Vec<String> = (0..min).map(|i| format!("a{i}: T{i}")).collect();
        let values: Vec<String> = (0..min).map(|i| format!("to_value(a{i})")).collect();
        let generics = if generics.is_empty() {
            String::new()
        } else {
            format!("<{}>", generics.join(", "))
        };
        let _ = writeln!(
            out,
            "fn {name}{generics}({params}): Result<Value, OpError> = Rssn.dynamic(\"{op_name}\", [{values}])\n",
            params = params.join(", "),
            values = values.join(", "),
        );
    } else {
        let _ = writeln!(
            out,
            "fn {name}(args: Tensor<1, Value>): Result<Value, OpError> = Rssn.dynamic(\"{op_name}\", args)\n"
        );
    }
}

/// A markdown report: how every operator family is exposed.
///
/// `stdlib_names` are the names exported by chapter 8 / the embedded standard
/// library, so the report can say which kernel operators already have a
/// hand-written standard-library entry point.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "Keeps the generated report sections and their ordering together"
)]
pub fn coverage_report(
    registry: &Registry,
    overlay: &Overlay,
    stdlib_names: &BTreeSet<String>,
) -> String {
    let catalog = crate::bindings::catalog(registry);
    let flat_typed: BTreeSet<&str> = catalog
        .iter()
        .filter(|row| matches!(row.coverage, crate::bindings::Coverage::Typed))
        .map(|row| row.op.name.as_str())
        .collect();
    let placements = classify(registry, overlay);
    let mut out = String::new();
    // Optional JIT and GPU capabilities change the sealed manifest hash.
    // This report compares the operator inventory and Aevia placements.
    let _ = writeln!(
        out,
        "# rssn operator coverage in Aevia\n\n\
         Generated by `bazel run //:refresh_kernel`;\n\
         do not edit. Source of truth: the Kernel Manifest and `crates/aevia-rssn/domains.overlay`.\n\n\
         RSSN {}, kernel API {}.\n",
        registry.version, registry.api_version
    );
    let total = placements.len();
    let count = |f: &dyn Fn(&Placement<'_>) -> bool| placements.iter().filter(|p| f(p)).count();
    let typed = count(&|p| p.status == Status::Typed);
    let witnessed = count(&|p| p.status == Status::Witnessed);
    let dynamic = count(&|p| matches!(p.status, Status::Dynamic(_)));
    let excluded = count(&|p| matches!(p.status, Status::Excluded(_)));
    let unclassified = count(&|p| p.status == Status::Unclassified);
    let flat = flat_typed.len();
    let std_overlap = placements
        .iter()
        .filter(|p| stdlib_names.contains(&p.op.name))
        .count();
    let _ = writeln!(
        out,
        "## Summary\n\n\
         | | operators |\n|---|---:|\n\
         | in the manifest | {total} |\n\
         | typed in a domain module (curated `sig`, `Result` channel) | {typed} |\n\
         | typed in a domain module from kernel evidence (witnessed overload, `Result` channel) | {witnessed} |\n\
         | dynamic in a domain module (generated, `Result<Value, OpError>`) | {dynamic} |\n\
         | excluded from domain modules (with reason) | {excluded} |\n\
         | unclassified (must be 0) | {unclassified} |\n\
         | exact-typed in the flat `Rssn.<op>` facade (some overload pinned to a declared/overlay signature) | {flat} |\n\
         | with a same-named chapter-8 standard-library entry | {std_overlap} |\n"
    );
    let _ = writeln!(
        out,
        "Exposure tiers, from best to worst: **typed** (`Rssn_<Domain>.f(x: Int): Result<Int, OpError>`, curated), \
         **witnessed** (the same shape of wrapper, generated from an overload the kernel observed and can replay), \
         **dynamic** (`Rssn_<Domain>.f(a, b): Result<Value, OpError>`), and the always-available escape hatch \
         `Rssn.dynamic(\"f\", [..])`. Nothing in the manifest is unreachable.\n"
    );
    let _ = writeln!(out, "## By domain\n");
    let _ = writeln!(
        out,
        "| module | mode | ops | typed | witnessed | dynamic | excluded | kernel rule sets |\n|---|---|---:|---:|---:|---:|---:|---|"
    );
    for domain in &overlay.domains {
        let mine: Vec<&Placement<'_>> = placements
            .iter()
            .filter(|p| p.domain.is_some_and(|d| d.name == domain.name))
            .collect();
        let sets: BTreeSet<&str> = mine.iter().map(|p| p.op.rule_set.as_str()).collect();
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} | {} | {} |",
            module_name(&domain.name),
            if domain.typed_required {
                "typed-required"
            } else {
                "dynamic-ok"
            },
            mine.len(),
            mine.iter().filter(|p| p.status == Status::Typed).count(),
            mine.iter()
                .filter(|p| p.status == Status::Witnessed)
                .count(),
            mine.iter()
                .filter(|p| matches!(p.status, Status::Dynamic(_)))
                .count(),
            mine.iter()
                .filter(|p| matches!(p.status, Status::Excluded(_)))
                .count(),
            sets.into_iter().collect::<Vec<_>>().join(", "),
        );
    }
    let _ = writeln!(out, "\n## By kernel rule set\n");
    let _ = writeln!(
        out,
        "| rule set | ops | declared/overlay sig | derived sig | kernel-witnessed | flat-facade exact | domain typed | domain witnessed | domain dynamic | excluded | domains |\n|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|"
    );
    let mut by_set: BTreeMap<&str, Vec<&Placement<'_>>> = BTreeMap::new();
    for p in &placements {
        by_set.entry(p.op.rule_set.as_str()).or_default().push(p);
    }
    for (set, ps) in &by_set {
        let domains: BTreeSet<&str> = ps
            .iter()
            .filter_map(|p| p.domain.map(|d| d.name.as_str()))
            .collect();
        let _ = writeln!(
            out,
            "| `{set}` | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            ps.len(),
            ps.iter().filter(|p| p.op.source.is_exact()).count(),
            ps.iter().filter(|p| !p.op.source.is_exact()).count(),
            ps.iter().filter(|p| !p.op.witnessed.is_empty()).count(),
            ps.iter()
                .filter(|p| flat_typed.contains(p.op.name.as_str()))
                .count(),
            ps.iter().filter(|p| p.status == Status::Typed).count(),
            ps.iter().filter(|p| p.status == Status::Witnessed).count(),
            ps.iter()
                .filter(|p| matches!(p.status, Status::Dynamic(_)))
                .count(),
            ps.iter()
                .filter(|p| matches!(p.status, Status::Excluded(_)))
                .count(),
            domains.into_iter().collect::<Vec<_>>().join(", "),
        );
    }
    let _ = writeln!(out, "\n## Typed operators\n");
    for domain in &overlay.domains {
        // An operator with several curated signatures lists the Aevia names.
        let names: Vec<String> = placements
            .iter()
            .filter(|p| {
                p.status == Status::Typed && p.domain.is_some_and(|d| d.name == domain.name)
            })
            .map(|p| {
                if let Some(sigs) = overlay.sigs.get(&p.op.name)
                    && sigs.len() > 1
                {
                    let aliases: Vec<&str> = sigs.iter().map(|s| s.name.as_str()).collect();
                    format!("{} ({})", p.op.name, aliases.join(", "))
                } else {
                    p.op.name.clone()
                }
            })
            .collect();
        if !names.is_empty() {
            let _ = writeln!(
                out,
                "* `{}`: {}",
                module_name(&domain.name),
                names.join(", ")
            );
        }
    }
    let _ = writeln!(out, "\n## Exclusions and accepted-dynamic decisions\n");
    for (op, reason) in &overlay.excluded {
        let _ = writeln!(out, "* excluded `{op}`: {reason}");
    }
    for (op, reason) in &overlay.accepted_dynamic {
        let _ = writeln!(out, "* dynamic `{op}`: {reason}");
    }
    let _ = writeln!(out, "\n## Untyped operators per typed-required domain\n");
    for domain in overlay.domains.iter().filter(|d| d.typed_required) {
        let names: Vec<&str> = placements
            .iter()
            .filter(|p| {
                matches!(p.status, Status::Dynamic(_))
                    && p.domain.is_some_and(|d| d.name == domain.name)
            })
            .map(|p| p.op.name.as_str())
            .collect();
        let _ = writeln!(
            out,
            "* `{}`: {}",
            module_name(&domain.name),
            if names.is_empty() {
                "none".to_string()
            } else {
                names.join(", ")
            }
        );
    }
    out
}

#[cfg(test)]
mod witnessed_names {
    use super::witnessed_param_names;

    fn names(types: &[&str]) -> Vec<String> {
        let owned: Vec<String> = types.iter().map(|t| (*t).to_string()).collect();
        witnessed_param_names(&owned)
    }

    #[test]
    fn names_follow_the_type() {
        assert_eq!(names(&["Int"]), ["n"]);
        assert_eq!(names(&["Tensor<2, Real>", "Tensor<1, Real>"]), ["m", "xs"]);
        assert_eq!(
            names(&["Bool", "Value", "Tensor<3, Int>"]),
            ["flag", "v", "t3"]
        );
    }

    #[test]
    fn repeated_kinds_are_numbered_and_stay_distinct() {
        let got = names(&["Real", "Float", "Int", "Real"]);
        assert_eq!(got, ["x1", "x2", "n", "x3"]);
        let mut unique = got.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), got.len());
        assert!(names(&[]).is_empty());
    }
}

#[cfg(test)]
mod domain_rendering {
    use super::*;

    #[test]
    fn explicit_dimension_parameters_render_as_a_generic_declaration() {
        let overlay = Overlay::parse(
            "sig invmod as cut_root<d, theta>(z: Complex[d], angle: Float[theta]) -> Result<Complex[d^(1/2)], OpError>",
        );
        assert!(overlay.errors.is_empty(), "{:?}", overlay.errors);
        let [sig] = overlay.sigs.get("invmod").unwrap().as_slice() else {
            panic!("expected one signature");
        };
        assert_eq!(sig.name, "cut_root");
        assert_eq!(sig.generics, ["d", "theta"]);
        let op = crate::registry::registry()
            .ops()
            .find(|operator| operator.name == "invmod")
            .expect("can render the wrapper using an existing manifest entry");
        let mut text = String::new();
        typed_wrapper(&mut text, &overlay, op, sig, &sig.name);
        assert!(text.contains(
            "fn cut_root<d, theta>(z: Complex[d], angle: Float[theta]): Result<Complex[d^(1/2)], OpError>"
        ));
        assert!(text.contains("from_value<Complex[d^(1/2)]>(wire)"));
    }

    #[test]
    fn malformed_generic_parameter_lists_have_overlay_diagnostics() {
        for generics in ["<>", "<d,>", "<d, d>", "<1d>", "<d", "<d><e>"] {
            let source =
                format!("sig op as root{generics}(z: Complex[d]) -> Result<Complex[d], OpError>");
            let overlay = Overlay::parse(&source);
            assert_eq!(overlay.errors.len(), 1, "{generics}");
            assert!(overlay.errors[0].contains("distinct generic parameters"));
            assert!(overlay.sigs.is_empty());
        }
    }

    #[test]
    fn batch_and_individual_sources_match_in_declaration_order() {
        let registry = crate::registry::registry();
        let files = domain_files(registry);
        let modules = module_names();
        assert_eq!(files.len(), modules.len());
        for (module, (file, source)) in modules.iter().zip(&files) {
            assert_eq!(file, &format!("{module}.aev"));
            assert_eq!(domain_source(registry, module).as_ref(), Some(source));
        }
        for name in ["NumberTheory", "Rssn_", "Rssn_number_theory"] {
            assert!(domain_source(registry, name).is_none());
        }
    }

    #[test]
    fn first_matching_family_wins_even_if_its_domain_is_missing() {
        let registry = crate::registry::registry();
        let op = registry
            .ops()
            .find(|op| op.name == "invmod")
            .expect("number theory operator");
        let mut overlay = Overlay::parse(&format!(
            "domain Wanted | test domain\nfamily Missing {}:{}\nfamily Wanted {}:*",
            op.rule_set, op.name, op.rule_set,
        ));
        let is_invmod = |p: &Placement<'_>| std::ptr::eq(p.op, op);
        assert!(
            !classify_placements(registry, &overlay, Some("Wanted"))
                .iter()
                .any(is_invmod)
        );
        let placements = classify(registry, &overlay);
        assert_eq!(
            placements.iter().find(|p| is_invmod(p)).unwrap().status,
            Status::Unclassified,
        );
        overlay.families.swap(0, 1);
        assert!(
            classify_placements(registry, &overlay, Some("Wanted"))
                .iter()
                .any(is_invmod)
        );
    }

    #[test]
    fn generation_uses_the_supplied_registry_snapshot() {
        let registry = crate::registry::registry();
        let source = domain_source(registry, "Rssn_NumberTheory").expect("default domain");
        let mut manifest = crate::kernel::kernel().manifest().as_ref().clone();
        let marker = "an alternate registry fixture for invmod";
        let operator = manifest
            .ops
            .iter_mut()
            .find(|op| op.name == "invmod")
            .expect("number theory operator");
        operator.doc = marker.to_owned();
        let alternate = Registry::from_manifest(&manifest);
        let changed = domain_source(&alternate, "Rssn_NumberTheory").expect("alternate domain");
        assert!(!source.contains(marker));
        assert!(changed.contains(marker));
        assert!(
            domain_files(&alternate)
                .iter()
                .any(|(file, text)| file == "Rssn_NumberTheory.aev" && text == &changed)
        );
        assert_eq!(domain_source(registry, "Rssn_NumberTheory"), Some(source));
    }
}
