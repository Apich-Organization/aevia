//! Chapter-8 signature conformance: `spec/08-stdlib.md` §8.1–§8.16 versus the checker.
//!
//! The name-coverage test answers "does a name exist?"; this file answers the
//! next question — does the *declared signature* agree with the scheme a call
//! is checked against? It extracts every `name : signature` declaration from
//! the chapter's code blocks (including grouped spellings such as
//! `sin cos tan : …` and skipping record-field lines inside `type … = { … }`),
//! asks `aevia_types::builtins::schemes` for the implemented scheme, and
//! compares the two on the parts that are mechanically comparable:
//!
//! * **presence** — every declared name has a scheme;
//! * **arity** — the number of top-level parameters of the signature and of the
//!   scheme's function type;
//! * **return class** — `Result`/`Option`/`Tensor`/`IO`/record/function/value.
//!
//! Parameter *order and types* are not compared textually — spec and scheme
//! spell types differently (`Scalar<ρ,d>` vs `Real[d]`, `(a, b)` vs `a -> b`) —
//! so the generator writes the full table (`--ignored generate`) for manual
//! review, and [`EXPECTED_MISMATCHES`] records the differences that are real but
//! owned elsewhere. A new mismatch fails the test until it is either fixed or
//! recorded with its owner: the table is evidence, not decoration.

#![allow(clippy::result_large_err)]

use aevia_types::adt::{OPTION_ID, RESULT_ID};
use aevia_types::builtins;
use aevia_types::{Scheme, Type};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

struct Decl {
    section: String,
    name: String,
    /// The declared signature as written (line breaks folded).
    signature: String,
}

fn spec_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/08-stdlib.md")
}

fn starts_new_decl(text: &str) -> bool {
    // A bullet starts with "- " (a continuation line may start with "->").
    text.starts_with("type ")
        || text.starts_with("- ")
        || text == "-"
        || text.starts_with('|')
        || text.starts_with('#')
        || text.starts_with("//")
}

/// The names of a `name1 name2 : signature` line, or `None` when the line is
/// not a declaration at all (a continuation such as `-> { t: … }` contains a
/// colon but no leading names).
fn declaration_names(head: &str) -> Option<Vec<String>> {
    let mut names = Vec::new();
    for token in head.split_whitespace() {
        if token.is_empty() {
            return None;
        }
        // An operator spelling such as `(&&)`.
        if let Some(inner) = token
            .strip_prefix('(')
            .and_then(|rest| rest.strip_suffix(')'))
        {
            if inner.is_empty()
                || !inner
                    .chars()
                    .all(|ch| "&|<>=+-*/%!~^".contains(ch))
            {
                return None;
            }
            names.push(inner.to_string());
            continue;
        }
        // A value name, possibly with a type application (`zeros_of<shape>`):
        // the scheme is registered under the bare name. A bare operator
        // spelling (`+`, `-`) is a declaration too (§8.14's measurement
        // arithmetic).
        let bare = token.split('<').next().unwrap_or(token);
        let operator = !bare.is_empty()
            && bare
                .chars()
                .all(|ch| "&|<>=+-*/%!~^".contains(ch));
        if operator {
            names.push(bare.to_string());
            continue;
        }
        if bare.is_empty()
            || !bare
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            || !bare.chars().next().is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        {
            return None;
        }
        names.push(bare.to_string());
    }
    if names.is_empty() {
        None
    } else {
        Some(names)
    }
}

fn split_top_commas(text: &str) -> Vec<&str> {    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let bytes = text.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        // `->` and `=>` are not angle brackets: their `>` must not close a
        // `<…>` group, or every function arrow inside a parameter list would
        // corrupt the depth (`sdeint_diag`'s nested closures).
        let arrow = index > 0 && (bytes[index - 1] == b'-' || bytes[index - 1] == b'=');
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b'<' if !arrow => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'>' if !arrow => depth -= 1,
            b',' if depth == 0 => {
                parts.push(text[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(text[start..].trim());
    parts
}

/// The top-level `->` of a signature: `(params) -> ret`.
fn split_arrow(signature: &str) -> Option<(&str, &str)> {
    let bytes = signature.as_bytes();
    let mut depth = 0i32;
    for index in 0..bytes.len().saturating_sub(1) {
        let arrow = index > 0 && (bytes[index - 1] == b'-' || bytes[index - 1] == b'=');
        match bytes[index] {
            b'(' | b'[' | b'{' => depth += 1,
            b'<' if !arrow => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'>' if !arrow => depth -= 1,
            b'-' if bytes[index + 1] == b'>' && depth == 0 => {
                return Some((signature[..index].trim(), signature[index + 2..].trim()));
            }
            _ => {}
        }
    }
    None
}

/// The number of top-level parameters a declared signature takes.
fn declared_arity(signature: &str) -> Option<usize> {
    // No top-level arrow: the signature describes a value (`zero : Scalar<…>`,
    // `pi : Real[1]`), not a function.
    let Some((lhs, _)) = split_arrow(signature) else {
        return Some(0);
    };
    // A constraint (`TotalOrd a => (a, a) -> Ordering`) is not a parameter.
    let lhs = lhs.rsplit_once("=>").map_or(lhs, |(_, rest)| rest).trim();
    if lhs.is_empty() {
        return None;
    }
    if lhs.starts_with('(') && lhs.ends_with(')') {
        let inner = &lhs[1..lhs.len() - 1];
        if inner.trim().is_empty() {
            Some(0)
        } else {
            Some(split_top_commas(inner).len())
        }
    } else if lhs.contains(',') && split_top_commas(lhs).len() > 1 {
        Some(split_top_commas(lhs).len())
    } else {
        Some(1)
    }
}

/// The return class of a declared signature (`value`, `result`, …).
fn declared_return_class(signature: &str) -> &'static str {
    let ret = split_arrow(signature).map_or(signature, |(_, ret)| ret).trim();
    return_class(ret)
}

fn return_class(ret: &str) -> &'static str {
    // A parenthesised return is either a tuple or a parenthesised function
    // type: `-> (a -> c)` is a function, `-> (A, B)` is a tuple.
    if ret.starts_with('(') && ret.ends_with(')') {
        let inner = &ret[1..ret.len() - 1];
        if split_arrow(inner).is_some() {
            return "function";
        }
        if split_top_commas(inner).len() > 1 {
            return "tuple";
        }
        return return_class(inner.trim());
    }
    if ret.starts_with('{') {
        return "record";
    }
    let head = ret.split(['<', '[', '(', ' ']).next().unwrap_or("");
    match head {
        "Result" => "result",
        "Option" => "option",
        "Tensor" => "tensor",
        "Sparse" => "sparse",
        "IO" => "io",
        "Int" | "Float" | "Real" | "Complex" | "Bool" | "String" | "Prop" | "Ordering"
        | "Scalar" | "Unit" => "value",
        "Table" => "table",
        _ if ret.contains("->") => "function",
        _ if !head.is_empty()
            && head.chars().next().is_some_and(|ch| ch.is_ascii_uppercase()) =>
        {
            // A nominal type named `Measurement`, `Series`, …; the scheme may
            // lower it to a structural record (Aevia tuples are records).
            "nominal"
        }
        _ => "value",
    }
}

/// Resolves an `同上` ("same as above") declaration against the previous
/// signature: `同上` inherits it, `+ PlotOpts`/`（带 PlotOpts）` appends one
/// options parameter, `（默认参数）` strips one.
fn inherit_signature(previous: &str, marker: &str) -> String {
    let mut signature = previous.to_string();
    if marker.contains("默认参数") {
        if let Some((lhs, ret)) = split_arrow(&signature) {
            let inner = lhs.trim().trim_start_matches('(').trim_end_matches(')');
            let kept: Vec<&str> = split_top_commas(inner)
                .into_iter()
                .filter(|part| *part != "PlotOpts")
                .collect();
            signature = format!("({}) -> {ret}", kept.join(", "));
        }
        return signature;
    }
    let extras = marker.matches("PlotOpts").count();
    if extras == 0 {
        return signature;
    }
    if let Some((lhs, ret)) = split_arrow(&signature) {
        let inner = lhs.trim().trim_start_matches('(').trim_end_matches(')');
        let extra = vec!["PlotOpts"; extras].join(", ");
        let params = if inner.trim().is_empty() {
            extra
        } else {
            format!("{inner}, {extra}")
        };
        signature = format!("({params}) -> {ret}");
    }
    signature
}

///
/// Aevia has no tuple type of its own (`(a, b)` is a record with positional
/// labels §3.2.4), and a nominal value type may be lowered to a record by the
/// checker's representation; those are agreement, not drift.
fn classes_agree(declared: &str, scheme: &str) -> bool {
    declared == scheme
        || matches!(
            (declared, scheme),
            ("tuple", "record")
                | ("nominal", "record")
                | ("nominal", "value")
                | ("sparse", "tensor")
                | ("sparse", "value")
                | ("table", "record")
                | ("table", "tensor")
                | ("table", "value")
        )
}

/// Ends the pending declaration (if any): records it, and remembers its
/// completed signature for a following `同上`.
fn flush_declaration(
    out: &mut Vec<Decl>,
    current: &mut Option<(String, Vec<String>, String)>,
    last_signature: &mut Option<String>,
) {
    let Some((section, names, signature)) = current.take() else {
        return;
    };
    last_signature.replace(signature.clone());
    for name in names {
        out.push(Decl {
            section: section.clone(),
            name,
            signature: signature.clone(),
        });
    }
}

fn declarations() -> Vec<Decl> {
    let source = std::fs::read_to_string(spec_path()).expect("spec/08-stdlib.md");
    let mut out = Vec::new();
    let mut section = String::new();
    let mut in_fence = false;
    let mut braces = 0i32;
    // The previous declaration in this fence, for the `同上` ("same as above")
    // shorthand the specification uses for variants of the previous signature.
    let mut last_signature: Option<String> = None;
    // The declaration being accumulated: section, names, signature so far.
    let mut current: Option<(String, Vec<String>, String)> = None;
    for raw in source.lines() {
        if let Some(rest) = raw.strip_prefix("## 8.") {
            let number: String = rest
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches('.')
                .to_string();
            section = format!("8.{number}");
            in_fence = false;
            braces = 0;
            current = None;
            last_signature = None;
            continue;
        }
        let trimmed = raw.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            braces = 0;
            current = None;
            last_signature = None;
            continue;
        }
        if !in_fence {
            continue;
        }
        let before = braces;
        braces += trimmed.matches('{').count() as i32 - trimmed.matches('}').count() as i32;
        braces = braces.max(0);
        if before > 0 || braces > 0 {
            flush_declaration(&mut out, &mut current, &mut last_signature);
            continue;
        }
        if trimmed.is_empty() || starts_new_decl(trimmed) {
            flush_declaration(&mut out, &mut current, &mut last_signature);
            continue;
        }
        // `name1 name2 : signature`; a declaration may continue on the next
        // line (indented), which is folded into `signature`.
        if let Some((head, rest)) = trimmed.split_once(':') {
            if let Some(names) = declaration_names(head) {
                // A new declaration ends the previous one: without this flush
                // only the last declaration of each block would be kept.
                flush_declaration(&mut out, &mut current, &mut last_signature);
                let mut signature = rest.to_string();
                for marker in ["  #", " #"] {
                    if let Some((before, _)) = signature.split_once(marker) {
                        signature = before.to_string();
                    }
                }
                let signature = signature.trim().to_string();
                if signature.starts_with("同上") {
                    // `同上` (same as above) refers to the declaration just
                    // before it: `root : 同上` is `nsolve_f`'s signature,
                    // `errorbar_with : 同上 + PlotOpts` adds one parameter.
                    let Some(previous) = last_signature.as_deref() else {
                        continue;
                    };
                    let signature = inherit_signature(previous, &signature);
                    current = Some((section.clone(), names, signature));
                } else {
                    current = Some((section.clone(), names, signature));
                }
                continue;
            }
        }
        if let Some((_, _, signature)) = current.as_mut() {
            if let Some((before, _)) = trimmed.split_once(" #") {
                signature.push(' ');
                signature.push_str(before.trim());
            } else {
                signature.push(' ');
                signature.push_str(trimmed);
            }
        }
    }
    // Only §8.1–§8.16 (the audit range; §8.17+ is a different interface family).
    out.retain(|decl| {
        let mut parts = decl.section.split('.');
        parts.next();
        let minor: u32 = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        minor <= 16
    });
    out
}

// ---------------------------------------------------------------------------
// Scheme rendering and comparison
// ---------------------------------------------------------------------------

fn scheme_arity(scheme: &Scheme) -> usize {
    match &scheme.ty {
        Type::Fn(params, _) => params.len(),
        _ => 0,
    }
}

fn scheme_return_class(scheme: &Scheme) -> &'static str {
    fn class(ty: &Type) -> &'static str {
        match ty {
            Type::Adt(id, _) if *id == RESULT_ID => "result",
            Type::Adt(id, _) if *id == OPTION_ID => "option",
            Type::Tensor { .. } => "tensor",
            Type::Io(_) => "io",
            Type::Record(_) => "record",
            Type::Fn(_, _) => "function",
            Type::Adt(_, _) => "value",
            _ => "value",
        }
    }
    match &scheme.ty {
        Type::Fn(_, ret) => class(ret),
        Type::Io(inner) => match &**inner {
            Type::Fn(_, ret) => class(ret),
            other => class(other),
        },
        other => class(other),
    }
}

fn scheme_type_head(scheme: &Scheme) -> String {
    // The full scheme (parameters and result), so the table shows parameter
    // order and types for the manual review; `Type`'s Display renders the
    // function type as `(p1, p2) -> ret`.
    scheme.ty.to_string()
}

/// A signature that cannot be compared mechanically:
///
/// * operator spellings are parsed by the checker as syntax, not as callable
///   names (`&&`, `==`, `+`, …);
/// * `zeros`/`ones`/`fill` are call-form builtins (the checker special-cases
///   their call position and has no value scheme — the same limitation pinned
///   in `stdlib_modules.rs::call_form_names_are_the_only_missing_exports`);
/// * `Some`/`None`/`Ok`/`Err` are constructors, not functions.
const SPECIAL: &[&str] = &[
    "&&", "||", "==", "!=", "<", "<=", ">", ">=", "*", "/", "+", "-", "zeros", "ones", "fill",
    "Some", "None", "Ok", "Err",
];

/// Differences that are real and recorded with their owner. Keyed
/// `section|name`. Each entry is `(section, name, reason, owner)`.
const EXPECTED_MISMATCHES: &[(&str, &str, &str, &str)] = &[
    // The generic scheme is deliberately loose (four fresh types -> a fresh
    // type); the *call* is typed by `Checker::sde_call` from `RateOf`/
    // `DiffusionOf` and answers `Result<SdePath<s>, SdeError>`. The loose
    // scheme is what makes an unimported/ill-typed call still report arity and
    // argument errors instead of "unknown name".
    (
        "8.9.1",
        "sdeint_diag",
        "generic scheme is loose; `Checker::sde_call` types the call as \
         `Result<SdePath<s>, SdeError>` from RateOf/DiffusionOf",
        "frontend (aevia-types builtins/check)",
    ),
    (
        "8.9.1",
        "sdeint_general",
        "generic scheme is loose; `Checker::sde_call` types the call as \
         `Result<SdePath<s>, SdeError>` from RateOf/DiffusionOf",
        "frontend (aevia-types builtins/check)",
    ),
];

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Mismatch {
    section: String,
    name: String,
    reason: String,
}

/// Findings from the manual review of parameter *types and order* (the
/// mechanical test compares presence, arity and return class; types are read
/// against the rendered scheme in the generated table). Each entry is
/// `(section, name, finding, owner)`. The test asserts every name here is a
/// declaration the table actually contains, so a vanished name is caught.
const MANUAL_FINDINGS: &[(&str, &str, &str, &str)] = &[
    (
        "8.4",
        "inv",
        "scheme keeps `ρ`; §8.4 refines the result to `Scalar<div_rep(ρ),1/d>` \
         (Int input must become a real representation)",
        "frontend (aevia-types builtins/check)",
    ),
    (
        "8.4",
        "linsolve",
        "scheme keeps `ρ`; §8.4 refines the result to `Scalar<div_rep(ρ),e/d>`",
        "frontend (aevia-types builtins/check)",
    ),
    (
        "8.4",
        "lstsq",
        "scheme keeps `ρ`; §8.4 refines the result to `Scalar<div_rep(ρ),e/d>`",
        "frontend (aevia-types builtins/check)",
    ),
    (
        "8.1",
        "mean",
        "scheme keeps `ρ`; §8.1/§8.4/§8.7 refine the result to `Scalar<div_rep(ρ),d>`",
        "frontend (aevia-types builtins/check)",
    ),
    (
        "8.4",
        "var",
        "scheme keeps `ρ`; §8.4/§8.7 refine the result to `Scalar<div_rep(ρ),d^2>`",
        "frontend (aevia-types builtins/check)",
    ),
    (
        "8.7",
        "std",
        "scheme keeps `ρ`; §8.7 refines the result to `Scalar<div_rep(ρ),d>`",
        "frontend (aevia-types builtins/check)",
    ),
    (
        "8.7",
        "cov",
        "scheme keeps `ρ`; §8.7 refines the result to `Scalar<div_rep(ρ),d*e>`",
        "frontend (aevia-types builtins/check)",
    ),
    (
        "8.7",
        "corr",
        "scheme keeps `ρ`; §8.7 refines the result to `Scalar<div_rep(ρ),1>`",
        "frontend (aevia-types builtins/check)",
    ),
    (
        "8.6",
        "maximize",
        "`同上` follows `minimize_f`, so the declared argument is `Float`; the \
         scheme is the `Real` form. The spec should say which function it \
         inherits (runtime implements the `Real` form)",
        "docs-aligner (spec §8.6 wording)",
    ),
    (
        "8.4",
        "rank",
        "scheme parameter is a fresh type (`t0`), so `rank(3)` type-checks; \
         §8.4 declares `Tensor<2, a> -> Int`",
        "frontend (aevia-types builtins/check)",
    ),
    (
        "8.1",
        "shape",
        "scheme parameter is a fresh type (`t0`), so `shape(3)` type-checks; \
         §8.1 declares `Tensor<n, a> -> Tensor<1, Int>`",
        "frontend (aevia-types builtins/check)",
    ),
    (
        "8.15",
        "complex_sqrt",
        "scheme returns `Complex<d0^1>`; §8.15 declares `Complex[d^(1/2)]`",
        "frontend (aevia-types builtins/check)",
    ),
    (
        "8.4",
        "transpose",
        "the sparse overload of §8.4 (`Sparse<F,[m,n],τ> -> Sparse<F,[n,m],τ>`) \
         has no scheme and no runtime arm; there is no `Sparse` value \
         representation in aevia-core this round, so this is a documented \
         capability gap (`storage.sparse`), not a silent omission",
        "runtime (`storage.sparse`, out of this round's scope) + frontend scheme",
    ),
    (
        "8.11",
        "read_npy",
        "scheme fixes rank 2 (`Tensor<2, Float>`); §8.11 declares \
         `Tensor<n, Float>` and the codec reads rank 0/1/2",
        "frontend + runtime (widen the scheme)",
    ),
    (
        "8.11",
        "write_npy",
        "scheme fixes rank 2 (`Tensor<2, Float>`); §8.11 declares \
         `Tensor<n, Float>` and the codec writes rank 0/1/2",
        "frontend + runtime (widen the scheme)",
    ),
    (
        "8.14",
        "+",
        "§8.14 declares `Measurement` arithmetic (`+`, `-`, `*`); at audit time \
         `measure(1.0,0.1) + measure(2.0,0.2)` was `E0302` (no scheme, no \
         runtime arm). Runtime task-29 is landing the value-level propagation; \
         the checker schemes are task-28 item 7",
        "frontend (schemes, task-28) + runtime (dispatch, task-29)",
    ),
    (
        "8.14",
        "-",
        "§8.14 declares `Measurement` arithmetic (`+`, `-`, `*`); at audit time \
         there was no scheme and no runtime arm",
        "frontend (schemes, task-28) + runtime (dispatch, task-29)",
    ),
    (
        "8.14",
        "*",
        "§8.14 declares `Measurement` arithmetic (`+`, `-`, `*`); at audit time \
         there was no scheme and no runtime arm",
        "frontend (schemes, task-28) + runtime (dispatch, task-29)",
    ),
];

fn audit() -> (Vec<Mismatch>, Vec<String>) {
    let decls = declarations();
    let mut mismatches = Vec::new();
    let mut table = Vec::new();
    for decl in &decls {
        let spec_arity = declared_arity(&decl.signature);
        let spec_class = declared_return_class(&decl.signature);
        let schemes = builtins::schemes(&decl.name);
        match schemes {
            None => {
                if !SPECIAL.contains(&decl.name.as_str()) {
                    mismatches.push(Mismatch {
                        section: decl.section.clone(),
                        name: decl.name.clone(),
                        reason: "no checker scheme for a declared name".to_string(),
                    });
                }
                table.push(format!(
                    "| {} | `{}` | `{}` | MISSING | - | - | checker/frontend |",
                    decl.section, decl.name, decl.signature
                ));
            }
            Some(schemes) => {
                let mut arities: Vec<usize> = schemes.iter().map(scheme_arity).collect();
                arities.sort_unstable();
                arities.dedup();
                let classes: Vec<&str> = {
                    let mut values: Vec<&str> =
                        schemes.iter().map(scheme_return_class).collect();
                    values.sort_unstable();
                    values.dedup();
                    values
                };
                let rendered: Vec<String> = schemes.iter().map(scheme_type_head).collect();
                let arity_ok = spec_arity.map_or(true, |arity| arities.contains(&arity));
                let class_ok = classes
                    .iter()
                    .any(|scheme| classes_agree(spec_class, scheme));
                if !arity_ok {
                    mismatches.push(Mismatch {
                        section: decl.section.clone(),
                        name: decl.name.clone(),
                        reason: format!(
                            "arity: spec {:?} vs scheme {arities:?}",
                            spec_arity
                        ),
                    });
                }
                if !class_ok {
                    mismatches.push(Mismatch {
                        section: decl.section.clone(),
                        name: decl.name.clone(),
                        reason: format!("return class: spec {spec_class} vs scheme {classes:?}"),
                    });
                }
                table.push(format!(
                    "| {} | `{}` | `{}` | `{}` | {} / {} | {} | {} |",
                    decl.section,
                    decl.name,
                    decl.signature,
                    rendered.join(" ; "),
                    spec_arity.map_or("?".to_string(), |a| a.to_string()),
                    format!("{arities:?}"),
                    if arity_ok && class_ok { "MATCH" } else { "MISMATCH" },
                    if arity_ok && class_ok {
                        "-".to_string()
                    } else {
                        format!(
                            "{}: spec class {spec_class}, scheme {classes:?}",
                            if !arity_ok { "arity" } else { "return class" }
                        )
                    }
                ));
            }
        }
    }
    (mismatches, table)
}

/// The generated table, for manual review (parameter types and order are
/// compared by eye against the scheme rendering). Writes to the design folder,
/// never to `spec/`.
#[test]
#[ignore = "generator: writes the audit table for review"]
fn generate() {
    let decls = declarations();
    eprintln!("declarations: {}", decls.len());
    let (mismatches, table) = audit();
    eprintln!("table rows: {}", table.len());
    let mut out = String::from(
        "# Chapter-8 signature audit (§8.1–§8.16)\n\n\
         Generated by `cargo test -p aevia-std --test spec_signatures -- --ignored generate`.\n\
         Columns: spec signature; checker scheme head; spec/scheme arity; verdict; note.\n\n\
         | § | name | spec signature | checker scheme | arity spec/scheme | verdict | note |\n\
         |---|---|---|---|---|---|---|\n",
    );
    out.push_str(&table.join("\n"));
    out.push_str("\n\n## Recorded mismatches\n\n");
    if mismatches.is_empty() {
        out.push_str("None: arity and return class agree for every declared signature.\n");
    } else {
        for mismatch in &mismatches {
            let owner = EXPECTED_MISMATCHES
                .iter()
                .find(|(section, name, _, _)| {
                    *section == mismatch.section && *name == mismatch.name
                })
                .map_or("unowned (fix it)", |(_, _, _, owner)| *owner);
            out.push_str(&format!(
                "- {} `{}`: {} — owner: {owner}\n",
                mismatch.section, mismatch.name, mismatch.reason
            ));
        }
    }
    out.push_str("\n\n## Manual type/order review\n\n");
    if MANUAL_FINDINGS.is_empty() {
        out.push_str("No parameter-type or parameter-order deviations found.\n");
    } else {
        out.push_str(
            "The mechanical check above compares presence, arity and return class. \
             These are the differences found by reading the parameter types and \
             order against the rendered scheme:\n\n",
        );
        for (section, name, finding, owner) in MANUAL_FINDINGS {
            out.push_str(&format!(
                "- {section} `{name}`: {finding} — owner: {owner}\n"
            ));
        }
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.workbuddy-ai/design/chapter8-signature-audit.md");
    std::fs::write(&path, out).expect("write audit table");
    eprintln!("wrote {}", path.display());
    eprintln!("mismatches: {}", mismatches.len());
}

/// Every manually reviewed finding names a declaration the table contains, so
/// a renamed or removed spec entry invalidates the finding instead of silently
/// leaving stale text in the report.
#[test]
fn the_manual_findings_name_real_declarations() {
    let decls = declarations();
    for (section, name, _, _) in MANUAL_FINDINGS {
        assert!(
            decls
                .iter()
                .any(|decl| decl.section == *section && decl.name == *name),
            "the manual finding for {section} `{name}` has no declaration"
        );
    }
}

/// Every §8.1–§8.16 name has a scheme whose arity and return class agree with
/// the specification, except the differences recorded in
/// [`EXPECTED_MISMATCHES`].
#[test]
fn every_declared_signature_has_a_matching_scheme() {
    let (mismatches, _) = audit();
    let mut unexpected: Vec<&Mismatch> = mismatches
        .iter()
        .filter(|mismatch| {
            !EXPECTED_MISMATCHES.iter().any(|(section, name, _, _)| {
                *section == mismatch.section && *name == mismatch.name
            })
        })
        .collect();
    unexpected.sort();
    assert_eq!(
        expected_mismatch_keys(),
        mismatch_keys(&mismatches),
        "the recorded mismatch table is out of date"
    );
    assert!(
        unexpected.is_empty(),
        "the declared signature and the checker disagree (fix, or record it with an owner):\n{}",
        unexpected
            .iter()
            .map(|mismatch| format!(
                "  {} `{}`: {}",
                mismatch.section, mismatch.name, mismatch.reason
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn expected_mismatch_keys() -> Vec<(String, String)> {
    let mut keys: Vec<(String, String)> = EXPECTED_MISMATCHES
        .iter()
        .map(|(section, name, _, _)| ((*section).to_string(), (*name).to_string()))
        .collect();
    keys.sort();
    keys
}

fn mismatch_keys(mismatches: &[Mismatch]) -> Vec<(String, String)> {
    let mut keys: Vec<(String, String)> = mismatches
        .iter()
        .map(|mismatch| (mismatch.section.clone(), mismatch.name.clone()))
        .collect();
    keys.sort();
    keys
}
