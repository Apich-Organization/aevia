//! Audits public Rust numerical APIs against declared RSSN dynamic exports.
//! Source paths and callable operators are separate inventories. Proven aliases
//! share coverage; unresolved imports and macros remain review fingerprints.

#![deny(
    missing_docs,
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::nursery,
    clippy::pedantic,
    clippy::single_call_fn,
    clippy::unwrap_used
)]
use rssn_api_model::numerical_codegen;
use rssn_api_model::structured_codegen;

use quote::ToTokens;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;
use std::path::PathBuf;
use syn::Item;
use syn::Visibility;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    alias_of: Option<String>,
    path: String,
    signature: String,
    cfg: String,
    reason: &'static str,
}
const fn public(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}
fn compact(tokens: impl ToTokens) -> String {
    tokens
        .to_token_stream()
        .to_string()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn cfg(attrs: &[syn::Attribute]) -> String {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("cfg") || a.path().is_ident("cfg_attr"))
        .map(compact)
        .collect::<Vec<_>>()
        .join(" ")
}
#[derive(Default)]
struct TypeAudit {
    callback: bool,
    opaque: bool,
}
impl<'ast> syn::visit::Visit<'ast> for TypeAudit {
    fn visit_type_bare_fn(
        &mut self,
        node: &'ast syn::TypeBareFn,
    ) {
        self.callback = true;
        syn::visit::visit_type_bare_fn(self, node);
    }

    fn visit_trait_bound(
        &mut self,
        node: &'ast syn::TraitBound,
    ) {
        if node
            .path
            .segments
            .last()
            .is_some_and(|s| matches!(s.ident.to_string().as_str(), "Fn" | "FnMut" | "FnOnce"))
        {
            self.callback = true;
        }
        syn::visit::visit_trait_bound(self, node);
    }

    fn visit_type_path(
        &mut self,
        node: &'ast syn::TypePath,
    ) {
        if let Some(last) = node.path.segments.last() {
            let name = last.ident.to_string();
            if !matches!(
                name.as_str(),
                "f32"
                    | "f64"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "u128"
                    | "usize"
                    | "i8"
                    | "i16"
                    | "i32"
                    | "i64"
                    | "i128"
                    | "isize"
                    | "bool"
                    | "str"
                    | "String"
                    | "Vec"
                    | "Option"
                    | "Result"
                    | "Box"
                    | "Self"
            ) {
                self.opaque = true;
            }
        }
        syn::visit::visit_type_path(self, node);
    }
}
fn type_reason(
    sig: &syn::Signature,
    method: bool,
) -> &'static str {
    let mut audit = TypeAudit::default();
    syn::visit::Visit::visit_signature(&mut audit, sig);
    if audit.callback {
        "callback"
    } else if !sig.generics.params.is_empty() || sig.generics.where_clause.is_some() {
        "generic"
    } else if method {
        "associated-method"
    } else if audit.opaque {
        "opaque"
    } else {
        "no-declared-adapter"
    }
}

fn collect_parent_imports(
    tree: &syn::UseTree,
    parent_types: &BTreeSet<String>,
    aliases: &mut BTreeMap<String, String>,
) {
    match tree {
        | syn::UseTree::Name(name) if parent_types.contains(&name.ident.to_string()) => {
            aliases.insert(name.ident.to_string(), name.ident.to_string());
        },
        | syn::UseTree::Rename(name) if parent_types.contains(&name.ident.to_string()) => {
            aliases.insert(name.rename.to_string(), name.ident.to_string());
        },
        | syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_parent_imports(item, parent_types, aliases);
            }
        },
        | _ => {},
    }
}

// Expand only named, local imports. Globs and external paths remain explicit
// unresolved entries; guessing their exports would give false ABI evidence.
fn named_uses(
    tree: &syn::UseTree,
    prefix: &[String],
    out: &mut Vec<(Vec<String>, String)>,
) {
    match tree {
        | syn::UseTree::Path(path) => {
            let mut prefix = prefix.to_vec();
            prefix.push(path.ident.to_string());
            named_uses(&path.tree, &prefix, out);
        },
        | syn::UseTree::Name(name) => {
            let mut path = prefix.to_vec();
            path.push(name.ident.to_string());
            out.push((path, name.ident.to_string()));
        },
        | syn::UseTree::Rename(name) => {
            let mut path = prefix.to_vec();
            path.push(name.ident.to_string());
            out.push((path, name.rename.to_string()));
        },
        | syn::UseTree::Group(group) => {
            for tree in &group.items {
                named_uses(tree, prefix, out);
            }
        },
        | syn::UseTree::Glob(_) => {},
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "keeps visibility, cfg and re-export traversal in one place"
)]
fn resolve_local_function(
    items: &[Item],
    dir: &Path,
    path: &[String],
    cfg_so_far: &str,
    depth: usize,
    module_depth: usize,
) -> Result<Option<(syn::Signature, String)>, String> {
    if depth > 32 {
        return Ok(None);
    }
    let path = match path {
        | [first, rest @ ..] if first == "self" => rest,
        | _ => path,
    };
    let Some((head, tail)) = path.split_first() else {
        return Ok(None);
    };
    if tail.is_empty() {
        for item in items {
            if let Item::Fn(f) = item
                && public(&f.vis)
                && f.sig.ident == head.as_str()
            {
                return Ok(Some((
                    f.sig.clone(),
                    [cfg_so_far, &cfg(&f.attrs)].join(" ").trim().to_string(),
                )));
            }
            if let Item::Use(u) = item {
                if !public(&u.vis) {
                    continue;
                }
                let mut names = Vec::new();
                named_uses(&u.tree, &[], &mut names);
                for (target, alias) in names {
                    if alias == *head {
                        let child_cfg = [cfg_so_far, &cfg(&u.attrs)].join(" ").trim().to_string();
                        if let Some(found) = resolve_local_function(
                            items,
                            dir,
                            &target,
                            &child_cfg,
                            depth.saturating_add(1),
                            module_depth,
                        )? {
                            return Ok(Some(found));
                        }
                    }
                }
            }
        }
        return Ok(None);
    }
    for item in items {
        let Item::Mod(module) = item else {
            continue;
        };
        if module.ident != head.as_str() {
            continue;
        }
        // The first child may be private: this module can re-export its public
        // contents. Further traversal must use public module boundaries.
        if module_depth > 0 && !public(&module.vis) {
            return Ok(None);
        }
        let child_cfg = [cfg_so_far, &cfg(&module.attrs)]
            .join(" ")
            .trim()
            .to_string();
        if let Some((_, children)) = &module.content {
            return resolve_local_function(
                children,
                &dir.join(head),
                tail,
                &child_cfg,
                depth.saturating_add(1),
                module_depth.saturating_add(1),
            );
        }
        let path_attr = module.attrs.iter().find_map(|attr| {
            if !attr.path().is_ident("path") {
                return None;
            }
            let syn::Meta::NameValue(value) = &attr.meta else {
                return None;
            };
            let syn::Expr::Lit(value) = &value.value else {
                return None;
            };
            let syn::Lit::Str(value) = &value.lit else {
                return None;
            };
            Some(value.value())
        });
        let direct = dir.join(format!("{head}.rs"));
        let nested = dir.join(head).join("mod.rs");
        let file = path_attr.map_or_else(
            || {
                if direct.exists() {
                    direct
                } else {
                    nested
                }
            },
            |path| dir.parent().unwrap_or(dir).join(path),
        );
        if !file.exists() {
            return Ok(None);
        }
        let text =
            std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let parsed = syn::parse_file(&text).map_err(|e| format!("{}: {e}", file.display()))?;
        return resolve_local_function(
            &parsed.items,
            &dir.join(head),
            tail,
            &child_cfg,
            depth.saturating_add(1),
            module_depth.saturating_add(1),
        );
    }
    Ok(None)
}

/// Resolve public inherent methods of a named local type re-export. External
/// crates and private declarations remain unresolved; no signature guessing.
#[allow(
    clippy::too_many_lines,
    reason = "keeps visibility, cfg and inherent-method traversal in one place"
)]
fn resolve_local_methods(
    items: &[Item],
    dir: &Path,
    path: &[String],
    cfg_so_far: &str,
    depth: usize,
    module_depth: usize,
) -> Result<Vec<(syn::ImplItemFn, syn::Type, String)>, String> {
    if depth > 32 {
        return Ok(Vec::new());
    }
    let path = match path {
        | [first, rest @ ..] if first == "self" => rest,
        | _ => path,
    };
    let Some((head, tail)) = path.split_first() else {
        return Ok(Vec::new());
    };
    if tail.is_empty() {
        let visible = items.iter().any(|i| {
            match i {
                | Item::Struct(t) => public(&t.vis) && t.ident == head.as_str(),
                | Item::Enum(t) => public(&t.vis) && t.ident == head.as_str(),
                | _ => false,
            }
        });
        if visible {
            let mut result = Vec::new();
            for item in items {
                let Item::Impl(i) = item else { continue };
                if i.trait_.is_some() {
                    continue;
                }
                let syn::Type::Path(p) = i.self_ty.as_ref() else {
                    continue;
                };
                if p.qself.is_some()
                    || p.path.segments.len() != 1
                    || p.path
                        .segments
                        .first()
                        .is_none_or(|segment| segment.ident != head.as_str())
                {
                    continue;
                }
                for f in &i.items {
                    if let syn::ImplItem::Fn(f) = f
                        && public(&f.vis)
                    {
                        let c = [cfg_so_far, &cfg(&i.attrs), &cfg(&f.attrs)]
                            .join(" ")
                            .trim()
                            .to_string();
                        result.push((f.clone(), *i.self_ty.clone(), c));
                    }
                }
            }
            return Ok(result);
        }
        for item in items {
            if let Item::Use(u) = item
                && public(&u.vis)
            {
                let mut uses = Vec::new();
                named_uses(&u.tree, &[], &mut uses);
                for (target, alias) in uses {
                    if alias == *head {
                        let c = [cfg_so_far, &cfg(&u.attrs)].join(" ").trim().to_string();
                        let found = resolve_local_methods(
                            items,
                            dir,
                            &target,
                            &c,
                            depth.saturating_add(1),
                            module_depth,
                        )?;
                        if !found.is_empty() {
                            return Ok(found);
                        }
                    }
                }
            }
        }
        return Ok(Vec::new());
    }
    for item in items {
        let Item::Mod(module) = item else { continue };
        if module.ident != head.as_str() {
            continue;
        }
        if module_depth > 0 && !public(&module.vis) {
            return Ok(Vec::new());
        }
        let child_cfg = [cfg_so_far, &cfg(&module.attrs)]
            .join(" ")
            .trim()
            .to_string();
        if let Some((_, children)) = &module.content {
            return resolve_local_methods(
                children,
                &dir.join(head),
                tail,
                &child_cfg,
                depth.saturating_add(1),
                module_depth.saturating_add(1),
            );
        }
        let explicit = module.attrs.iter().find_map(|a| {
            if a.path().is_ident("path") {
                a.parse_args::<syn::LitStr>()
                    .ok()
                    .map(|x| x.value())
                    .or_else(|| {
                        let syn::Meta::NameValue(n) = &a.meta else {
                            return None;
                        };
                        let syn::Expr::Lit(l) = &n.value else {
                            return None;
                        };
                        let syn::Lit::Str(v) = &l.lit else {
                            return None;
                        };
                        Some(v.value())
                    })
            } else {
                None
            }
        });
        let file = explicit.map_or_else(
            || {
                let direct = dir.join(format!("{head}.rs"));
                if direct.exists() {
                    direct
                } else {
                    dir.join(head).join("mod.rs")
                }
            },
            |path| dir.parent().unwrap_or(dir).join(path),
        );
        if !file.exists() {
            return Ok(Vec::new());
        }
        let source =
            std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let parsed = syn::parse_file(&source).map_err(|e| format!("{}: {e}", file.display()))?;
        return resolve_local_methods(
            &parsed.items,
            &dir.join(head),
            tail,
            &child_cfg,
            depth.saturating_add(1),
            module_depth.saturating_add(1),
        );
    }
    Ok(Vec::new())
}

fn token_digest(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "all Rust item forms participate in one ordered source inventory pass"
)]
fn visit_items(
    items: &[Item],
    prefix: &str,
    dir: &Path,
    inherited_cfg: &str,
    out: &mut Vec<Entry>,
) -> Result<(), String> {
    let parent_types: BTreeSet<String> = items
        .iter()
        .filter_map(|item| {
            match item {
                | Item::Struct(x) if public(&x.vis) => Some(x.ident.to_string()),
                | Item::Enum(x) if public(&x.vis) => Some(x.ident.to_string()),
                | _ => None,
            }
        })
        .collect();
    for item in items {
        match item {
            | Item::Fn(f) if public(&f.vis) => {
                let c = [inherited_cfg, &cfg(&f.attrs)].join(" ").trim().to_string();
                out.push(Entry {
                    alias_of: None,
                    path: format!("{prefix}::{}", f.sig.ident),
                    signature: compact(&f.sig),
                    cfg: c,
                    reason: type_reason(&f.sig, false),
                });
            },
            | Item::Impl(i) if i.trait_.is_none() => {
                let ty = compact(&i.self_ty).replace(' ', "");
                for item in &i.items {
                    if let syn::ImplItem::Fn(f) = item {
                        if !public(&f.vis) {
                            continue;
                        }
                        let c = [inherited_cfg, &cfg(&i.attrs), &cfg(&f.attrs)]
                            .join(" ")
                            .trim()
                            .to_string();
                        let reason = if !i.generics.params.is_empty()
                            && type_reason(&f.sig, true) != "callback"
                        {
                            "generic"
                        } else {
                            type_reason(&f.sig, true)
                        };
                        out.push(Entry {
                            alias_of: None,
                            path: format!("{prefix}::{ty}::{}", f.sig.ident),
                            signature: compact(&f.sig),
                            cfg: c,
                            reason,
                        });
                    }
                }
            },
            | Item::Trait(t) if public(&t.vis) => {
                for item in &t.items {
                    if let syn::TraitItem::Fn(f) = item {
                        let c = [inherited_cfg, &cfg(&t.attrs), &cfg(&f.attrs)]
                            .join(" ")
                            .trim()
                            .to_string();
                        out.push(Entry {
                            alias_of: None,
                            path: format!("{prefix}::{}::{}", t.ident, f.sig.ident),
                            signature: compact(&f.sig),
                            cfg: c,
                            reason: "trait-method",
                        });
                    }
                }
            },
            // Macro expansion is intentionally not guessed. Recording every
            // module-level invocation and its token digest makes edits visible
            // to --check while making the inventory's limit explicit.
            | Item::Macro(m) => {
                let text = compact(&m.mac);
                let hash = token_digest(&text);
                let c = [inherited_cfg, &cfg(&m.attrs)].join(" ").trim().to_string();
                out.push(Entry {
                    alias_of: None,
                    path: format!("{prefix}::<macro:{hash:016x}>"),
                    signature: format!("{} (unexpanded)", compact(&m.mac.path)),
                    cfg: c,
                    reason: "unexpanded-macro",
                });
            },
            | Item::Use(u) if public(&u.vis) => {
                let text = compact(&u.tree);
                let hash = token_digest(&text);
                let c = [inherited_cfg, &cfg(&u.attrs)].join(" ").trim().to_string();
                let mut names = Vec::new();
                named_uses(&u.tree, &[], &mut names);
                for (target, alias) in names {
                    if let Some((signature, resolved_cfg)) =
                        resolve_local_function(items, dir, &target, &c, 0, 0)?
                    {
                        out.push(Entry {
                            alias_of: Some(format!(
                                "{prefix}::{}",
                                target
                                    .iter()
                                    .filter(|p| p.as_str() != "self")
                                    .cloned()
                                    .collect::<Vec<_>>()
                                    .join("::")
                            )),
                            path: format!("{prefix}::{alias}"),
                            reason: type_reason(&signature, false),
                            signature: compact(&signature),
                            cfg: resolved_cfg,
                        });
                    }
                    for (method, mut self_ty, resolved_cfg) in
                        resolve_local_methods(items, dir, &target, &c, 0, 0)?
                    {
                        let original = compact(&self_ty).replace(' ', "");
                        let module = target
                            .split_last()
                            .map_or(&[][..], |(_, modules)| modules)
                            .iter()
                            .filter(|p| p.as_str() != "self")
                            .cloned()
                            .collect::<Vec<_>>();
                        let alias_of = Some(format!(
                            "{prefix}::{}{}::{}",
                            if module.is_empty() {
                                String::new()
                            } else {
                                format!("{}::", module.join("::"))
                            },
                            original,
                            method.sig.ident
                        ));
                        if let syn::Type::Path(path) = &mut self_ty
                            && let Some(segment) = path.path.segments.first_mut()
                        {
                            segment.ident = syn::parse_str(&alias)
                                .map_err(|e| format!("invalid type alias: {e}"))?;
                        }
                        let name = compact(&self_ty).replace(' ', "");
                        out.push(Entry {
                            alias_of,
                            path: format!("{prefix}::{name}::{}", method.sig.ident),
                            signature: compact(&method.sig),
                            cfg: resolved_cfg,
                            reason: type_reason(&method.sig, true),
                        });
                    }
                }
                out.push(Entry {
                    alias_of: None,
                    path: format!("{prefix}::<reexport:{hash:016x}>"),
                    signature: text,
                    cfg: c,
                    reason: "unresolved-reexport",
                });
            },
            // Inherent implementations may live in a private child module while
            // their methods remain public through a public parent type. Resolve
            // direct `use super::Type` imports; do not expose the child's own API.
            | Item::Mod(m) if !public(&m.vis) && !parent_types.is_empty() => {
                let name = m.ident.to_string();
                let child_cfg = [inherited_cfg, &cfg(&m.attrs)].join(" ").trim().to_string();
                let parsed;
                let child_items = if let Some((_, items)) = &m.content {
                    items.as_slice()
                } else {
                    let direct = dir.join(format!("{name}.rs"));
                    let nested = dir.join(&name).join("mod.rs");
                    let explicit = m.attrs.iter().find_map(|attr| {
                        if !attr.path().is_ident("path") {
                            return None;
                        }
                        let syn::Meta::NameValue(n) = &attr.meta else {
                            return None;
                        };
                        let syn::Expr::Lit(l) = &n.value else {
                            return None;
                        };
                        let syn::Lit::Str(v) = &l.lit else {
                            return None;
                        };
                        Some(v.value())
                    });
                    let file = if let Some(p) = explicit {
                        dir.parent().unwrap_or(dir).join(p)
                    } else if direct.exists() {
                        direct
                    } else if nested.exists() {
                        nested
                    } else {
                        continue;
                    };
                    let text = std::fs::read_to_string(&file)
                        .map_err(|e| format!("{}: {e}", file.display()))?;
                    parsed =
                        syn::parse_file(&text).map_err(|e| format!("{}: {e}", file.display()))?;
                    parsed.items.as_slice()
                };
                let mut aliases = BTreeMap::new();
                for child in child_items {
                    if let Item::Use(u) = child
                        && let syn::UseTree::Path(path) = &u.tree
                        && path.ident == "super"
                    {
                        collect_parent_imports(&path.tree, &parent_types, &mut aliases);
                    }
                }
                for child in child_items {
                    if let Item::Impl(implementation) = child {
                        if implementation.trait_.is_some() {
                            continue;
                        }
                        let syn::Type::Path(path) = implementation.self_ty.as_ref() else {
                            continue;
                        };
                        if path.qself.is_some() || path.path.segments.len() != 1 {
                            continue;
                        }
                        let Some(segment) = path.path.segments.first() else {
                            continue;
                        };
                        let Some(original) = aliases.get(&segment.ident.to_string()) else {
                            continue;
                        };
                        let mut implementation = implementation.clone();
                        let mut receiver = path.clone();
                        let Some(segment) = receiver.path.segments.first_mut() else {
                            continue;
                        };
                        segment.ident = syn::parse_str(original)
                            .map_err(|e| format!("invalid parent type: {e}"))?;
                        implementation.self_ty = Box::new(syn::Type::Path(receiver));
                        visit_items(&[Item::Impl(implementation)], prefix, dir, &child_cfg, out)?;
                    }
                }
            },
            | Item::Mod(m) if public(&m.vis) => {
                let name = m.ident.to_string();
                let child_prefix = format!("{prefix}::{name}");
                let child_cfg = [inherited_cfg, &cfg(&m.attrs)].join(" ").trim().to_string();
                if let Some((_, items)) = &m.content {
                    visit_items(items, &child_prefix, &dir.join(&name), &child_cfg, out)?;
                } else {
                    let file = dir.join(format!("{name}.rs"));
                    let nested = dir.join(&name).join("mod.rs");
                    let file = if file.exists() {
                        file
                    } else {
                        nested
                    };
                    let text = std::fs::read_to_string(&file)
                        .map_err(|e| format!("{}: {e}", file.display()))?;
                    let parsed =
                        syn::parse_file(&text).map_err(|e| format!("{}: {e}", file.display()))?;
                    visit_items(
                        &parsed.items,
                        &child_prefix,
                        &dir.join(&name),
                        &child_cfg,
                        out,
                    )?;
                }
            },
            | _ => {},
        }
    }
    Ok(())
}
#[allow(
    clippy::single_call_fn,
    reason = "isolates manifest provenance validation"
)]
fn exported() -> Result<BTreeMap<String, Vec<String>>, String> {
    use rssn::kernel::manifest::SignatureSource;
    let session = rssn::api::Session::new();
    let manifest = session.manifest();
    let mut paths: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for info in &manifest.ops {
        let Some(path) = &info.rust_path else {
            continue;
        };
        if !path.starts_with("crate::")
            || info.signature_source != SignatureSource::Declared
            || info.overloads.is_empty()
            || info.overloads.len() != info.keys.len()
        {
            return Err(format!(
                "invalid source provenance or non-declared signature for {}",
                info.name
            ));
        }
        for (signature, key) in info.overloads.iter().zip(&info.keys) {
            let expected =
                rssn::kernel::signature::OpKey::new(&info.rule_set, &info.name, Some(signature));
            if *key != expected {
                return Err(format!(
                    "manifest key disagrees with declared signature for {}",
                    info.name
                ));
            }
            let label = if info.keys.len() == 1 {
                info.name.clone()
            } else {
                let mut hash = String::new();
                for byte in key.signature_hash {
                    let _ = write!(hash, "{byte:02x}");
                }
                format!("{}@{hash}", info.name)
            };
            paths.entry(path.clone()).or_default().push(label);
        }
    }
    for names in paths.values_mut() {
        names.sort();
        names.dedup();
    }
    Ok(paths)
}

#[allow(
    clippy::single_call_fn,
    reason = "isolates source collection from snapshot rendering"
)]
fn report(root: &Path) -> Result<String, String> {
    let mut entries = Vec::new();
    for module in ["kernels", "backend", "sim"] {
        let dir = root.join("src").join(module);
        let path = dir.join("mod.rs");
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let parsed = syn::parse_file(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        visit_items(
            &parsed.items,
            &format!("crate::{module}"),
            &dir,
            "",
            &mut entries,
        )?;
    }
    entries.sort_by(|a, b| (&a.path, &a.signature, &a.cfg).cmp(&(&b.path, &b.signature, &b.cfg)));
    let exports = exported()?;
    let present: BTreeSet<_> = entries.iter().map(|entry| entry.path.as_str()).collect();
    let missing: Vec<_> = exports
        .keys()
        .filter(|path| !present.contains(path.as_str()))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "ABI metadata points to missing public kernel functions:\n{}",
            missing
                .iter()
                .map(|path| path.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    Ok(render(entries, &exports))
}
fn identity(
    path: &str,
    aliases: &BTreeMap<String, String>,
) -> Option<String> {
    let mut path = path.to_string();
    let mut seen = BTreeSet::new();
    while let Some(next) = aliases.get(&path) {
        if !seen.insert(path.clone()) || seen.len() > 32 {
            return None;
        }
        path.clone_from(next);
    }
    Some(path)
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps canonical TSV serialization separate from source analysis"
)]
fn render(
    entries: Vec<Entry>,
    exports: &BTreeMap<String, Vec<String>>,
) -> String {
    let mut out = String::from(
        "# RSSN public kernel, backend and simulation source inventory v2\n# source syntax only; cfg is not evaluated; macros are not expanded; exported is not a runtime capability claim\n# path\tclassification\tabi names\tcfg\tsignature\n",
    );
    let aliases: BTreeMap<_, _> = entries
        .iter()
        .filter_map(|e| e.alias_of.as_ref().map(|a| (e.path.clone(), a.clone())))
        .collect();
    let mut inherited: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (path, names) in exports {
        if let Some(key) = identity(path, &aliases) {
            inherited
                .entry(key)
                .or_default()
                .extend(names.iter().cloned());
        }
    }
    for names in inherited.values_mut() {
        names.sort();
        names.dedup();
    }
    for e in entries {
        let direct = exports.get(&e.path);
        let key = identity(&e.path, &aliases);
        let names = direct.or_else(|| key.as_ref().and_then(|key| inherited.get(key)));
        let class = if direct.is_some() {
            "declared"
        } else if names.is_some() {
            "declared-alias"
        } else {
            e.reason
        };
        let names = names.map_or_else(String::new, |x| x.join(","));
        let _ = writeln!(
            out,
            "{}\t{}\t{}\t{}\t{}",
            e.path, class, names, e.cfg, e.signature
        );
    }
    out
}
#[allow(
    clippy::single_call_fn,
    reason = "keeps the unchanged snapshot contract directly testable"
)]
fn check_snapshot(
    saved: &str,
    actual: &str,
) -> Result<(), &'static str> {
    if saved == actual {
        Ok(())
    } else {
        Err("kernel API inventory changed; inspect --report and update with --write")
    }
}

/// Require a declared adapter or an explicit Rust host boundary for each API.
#[allow(
    clippy::single_call_fn,
    reason = "keeps the source boundary policy directly testable"
)]
fn check_complete(
    report: &str,
    policy: &str,
) -> Result<(usize, usize), String> {
    let mut host_boundaries = BTreeMap::new();
    for line in policy
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let (path, reason) = line
            .split_once('\t')
            .ok_or_else(|| format!("invalid source boundary: {line}"))?;
        if !path.starts_with("crate::") || reason.trim().is_empty() || reason.contains('\t') {
            return Err(format!("invalid source boundary: {line}"));
        }
        if host_boundaries.insert(path, reason).is_some() {
            return Err(format!("duplicate source boundary: {path}"));
        }
    }
    let mut matched_boundaries = BTreeSet::new();
    let mut adapted = 0usize;
    let mut missing = Vec::new();
    for row in report
        .lines()
        .filter(|row| !row.starts_with('#') && !row.is_empty())
    {
        let fields: Vec<_> = row.split('\t').collect();
        let [path, class, names, _, _] = fields.as_slice() else {
            return Err("malformed source inventory row".into());
        };
        if matches!(*class, "unexpanded-macro" | "unresolved-reexport") {
            continue;
        }
        if matches!(*class, "declared" | "declared-alias") && !names.is_empty() {
            if host_boundaries.contains_key(path) {
                return Err(format!(
                    "remove the Rust-only source boundary for adapted API {path}"
                ));
            }
            adapted = adapted.saturating_add(1);
        } else if host_boundaries.contains_key(path) {
            matched_boundaries.insert(*path);
        } else {
            missing.push(*path);
        }
    }
    if let Some(path) = host_boundaries
        .keys()
        .find(|path| !matched_boundaries.contains(**path))
    {
        return Err(format!(
            "source boundary references missing Rust API {path}"
        ));
    }
    if missing.is_empty() {
        Ok((adapted, matched_boundaries.len()))
    } else {
        Err(format!(
            "{} callable source interfaces need a declared adapter or a reviewed Rust host boundary:\n{}",
            missing.len(),
            missing.join("\n")
        ))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    type Generator = fn(&Path, &str) -> Result<String, String>;
    const USAGE: &str = "usage: rssn-api-audit --check|--write|--report|--coverage-check|--bindings-check|--bindings-write|--bindings-report|--structured-check|--structured-write|--structured-report|--codecs-check|--codecs-write|--codecs-report";
    let mut args = std::env::args().skip(1);
    let (Some(mode), None) = (args.next(), args.next()) else {
        return Err(USAGE.into());
    };
    let mode = mode.as_str();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../rssn");
    if let Some((family, action @ ("check" | "write" | "report"))) = mode
        .strip_prefix("--")
        .and_then(|name| name.split_once('-'))
    {
        let generation: Option<(&str, &str, &str, Generator)> = match family {
            | "codecs" => {
                Some((
                    "codec-policy.tsv",
                    "codec_generated.rs",
                    "composable codecs",
                    rssn_api_model::codec_codegen::generate,
                ))
            },
            | "structured" => {
                Some((
                    "structured-policy.tsv",
                    "structured_generated.rs",
                    "structured codecs",
                    structured_codegen::generate,
                ))
            },
            | "bindings" => {
                Some((
                    "numerical-policy.tsv",
                    "generated.rs",
                    "numerical bindings",
                    numerical_codegen::generate,
                ))
            },
            | _ => None,
        };
        if let Some((policy, filename, label, generate)) = generation {
            let policy = std::fs::read_to_string(root.join("tools/rssn-api-gen").join(policy))?;
            let source = generate(&root, &policy)?;
            let target = root.join("src/rules/numerical_abi").join(filename);
            match action {
                | "report" => print!("{source}"),
                | "write" => {
                    std::fs::write(&target, source)?;
                    println!("wrote {}", target.display());
                },
                | _ => {
                    if std::fs::read_to_string(&target)? != source {
                        return Err(format!("generated {label} changed; review --{family}-report and run --{family}-write").into());
                    }
                    println!("generated {label} verified");
                },
            }
            return Ok(());
        }
    }
    if !matches!(
        mode,
        "--check" | "--write" | "--report" | "--coverage-check"
    ) {
        return Err(USAGE.into());
    }
    let snapshot = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("inventory.tsv");
    let result = report(&root)?;
    match mode {
        | "--report" => print!("{result}"),
        | "--coverage-check" => {
            let policy = include_str!("../source-boundaries.tsv");
            let (adapted, host_only) = check_complete(&result, policy)?;
            println!(
                "{adapted} callable source interfaces have declared adapters; {host_only} reviewed Rust host interfaces"
            );
        },
        | "--write" => {
            std::fs::write(&snapshot, &result)?;
            println!("wrote {}", snapshot.display());
        },
        | _ => {
            let saved = std::fs::read_to_string(&snapshot)?;
            check_snapshot(&saved, &result)?;
            println!("kernel API inventory verified");
        },
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "tests assert the shape of fixed source fixtures"
)]
mod tests {
    use super::*;

    #[test]
    fn alias_coverage_requires_resolved_item_identity() {
        let parsed=syn::parse_file(r"pub mod child{pub struct T;impl T{pub fn method(&self){}}}pub use child::T as Alias;pub fn unrelated(){}").unwrap();
        let mut rows = Vec::new();
        visit_items(
            &parsed.items,
            "crate::kernels",
            Path::new("."),
            "",
            &mut rows,
        )
        .unwrap();
        let exports = BTreeMap::from([(
            "crate::kernels::child::T::method".into(),
            vec!["num_method".into()],
        )]);
        let report = render(rows, &exports);
        assert!(report.contains("crate::kernels::Alias::method\tdeclared-alias\tnum_method"));
        assert!(report.contains("crate::kernels::unrelated\tno-declared-adapter\t"));
        assert!(check_complete(&report, "").is_err());
        let policy = "crate::kernels::unrelated\tRust-specific test interface\n";
        assert_eq!(check_complete(&report, policy), Ok((2, 1)));
        let stale = "crate::kernels::child::T::method\tAlready adapted\n";
        assert!(check_complete(&report, stale).is_err());
    }

    #[test]
    fn reexported_public_type_methods_and_parent_generic_receivers_keep_public_paths() {
        let source=syn::parse_file(r"mod hidden {pub struct Model<T>(T);impl<T> Model<T>{pub fn new(x:T)->Self{Self(x)} fn private(){}}}pub use hidden::Model as Public;pub struct Parent<T>(T);mod child{use super::Parent;impl<T> Parent<T>{pub fn value(&self)->&T{&self.0}}}").unwrap();
        let mut rows = Vec::new();
        visit_items(
            &source.items,
            "crate::kernels",
            Path::new("."),
            "",
            &mut rows,
        )
        .unwrap();
        assert!(
            rows.iter()
                .any(|r| r.path == "crate::kernels::Public<T>::new")
        );
        assert!(
            rows.iter()
                .any(|r| r.path == "crate::kernels::Parent<T>::value")
        );
        assert!(
            !rows
                .iter()
                .any(|r| r.path.contains("hidden::") || r.path.ends_with("::private"))
        );
    }

    #[test]
    fn completeness_requires_real_declared_adapters_and_ignores_noncallable_fingerprints() {
        let complete = "# header\ncrate::kernels::f\tdeclared\tnum_f\t\tfn f()\ncrate::kernels::<macro:1>\tunexpanded-macro\t\t\tthread_local\n";
        assert_eq!(check_complete(complete, ""), Ok((1, 0)));
        assert!(
            check_complete(&complete.replace("declared\tnum_f", "opaque\t"), "")
                .unwrap_err()
                .contains("crate::kernels::f")
        );
        assert!(check_complete(&complete.replace("declared\tnum_f", "declared\t"), "").is_err());
    }

    #[test]
    fn named_reexports_resolve_only_public_functions_and_detect_cycles() {
        let parsed = syn::parse_file(
            r#"
            #[cfg(feature = "child")]
            mod inner { pub fn actual(x: f64) -> f64 { x } fn secret() {} }
            pub use inner::{actual as renamed, secret, missing};
            pub use renamed as chained;
            pub use cycle2 as cycle1;
            pub use cycle1 as cycle2;
            pub use inner::*;
        "#,
        )
        .unwrap();
        let mut entries = Vec::new();
        visit_items(
            &parsed.items,
            "crate::kernels::example",
            Path::new("."),
            "",
            &mut entries,
        )
        .unwrap();
        let paths: BTreeSet<_> = entries.iter().map(|entry| entry.path.as_str()).collect();
        assert!(paths.contains("crate::kernels::example::renamed"));
        assert!(paths.contains("crate::kernels::example::chained"));
        for name in ["secret", "missing", "cycle1", "cycle2", "actual"] {
            assert!(!paths.contains(format!("crate::kernels::example::{name}").as_str()));
        }
        assert!(
            entries
                .iter()
                .find(|entry| entry.path.ends_with("::renamed"))
                .unwrap()
                .cfg
                .contains("child")
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry.reason == "unresolved-reexport")
        );
    }

    #[test]
    fn named_reexport_reads_explicit_path_private_module() {
        let root = std::env::temp_dir().join(format!("rssn-audit-path-{}", std::process::id()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(
            root.join("sub/actual.rs"),
            "pub fn exposed() {} fn hidden() {}",
        )
        .unwrap();
        let parsed = syn::parse_file("#[path = \"sub/actual.rs\"] mod implementation; pub use implementation::exposed as result;").unwrap();
        let mut entries = Vec::new();
        visit_items(
            &parsed.items,
            "crate::kernels::example",
            &root.join("example"),
            "",
            &mut entries,
        )
        .unwrap();
        assert!(
            entries
                .iter()
                .any(|entry| entry.path == "crate::kernels::example::result")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn classifies_public_functions_methods_and_cfg_without_private_items() {
        let parsed = syn::parse_file(
            r#"
            #[cfg(feature = "math")]
            pub mod math {
                pub fn scalar(x: f64) -> f64 { x }
                pub fn callback<F: Fn(f64) -> f64>(f: F) -> f64 { f(1.0) }
                pub fn generic<T>(x: T) -> T { x }
                fn private() {}
                pub struct Point;
                impl Point { pub fn norm(&self) -> f64 { 0.0 } }
            }
        "#,
        )
        .unwrap();
        let mut rows = Vec::new();
        visit_items(
            &parsed.items,
            "crate::kernels",
            Path::new("."),
            "",
            &mut rows,
        )
        .unwrap();
        assert_eq!(rows.len(), 4);
        let reasons: BTreeMap<_, _> = rows.iter().map(|r| (r.path.as_str(), r.reason)).collect();
        assert_eq!(
            reasons["crate::kernels::math::scalar"],
            "no-declared-adapter"
        );
        assert_eq!(reasons["crate::kernels::math::callback"], "callback");
        assert_eq!(reasons["crate::kernels::math::generic"], "generic");
        assert_eq!(
            reasons["crate::kernels::math::Point::norm"],
            "associated-method"
        );
        assert!(rows.iter().all(|row| row.cfg.contains("feature")));
    }
    #[test]
    fn private_child_inherent_methods_are_visible_on_public_parent() {
        let file = syn::parse_file(
            r"
            pub struct Tape;
            struct Hidden;
            mod derivative {
                use super::{Tape as Parent, Hidden};
                impl Parent { pub fn vjp(&self) {} fn secret(&self) {} }
                impl Hidden { pub fn hidden(&self) {} }
                pub fn not_exported() {}
            }
        ",
        )
        .unwrap();
        let mut entries = Vec::new();
        visit_items(
            &file.items,
            "crate::backend::tape",
            Path::new("."),
            "",
            &mut entries,
        )
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "crate::backend::tape::Tape::vjp");
    }
    #[test]
    fn adding_a_public_kernel_function_invalidates_the_snapshot() {
        fn scan(source: &str) -> String {
            let file = syn::parse_file(source).expect("fixture");
            let mut entries = Vec::new();
            visit_items(
                &file.items,
                "crate::kernels::fixture",
                Path::new("."),
                "",
                &mut entries,
            )
            .unwrap();
            render(entries, &BTreeMap::new())
        }
        let saved = scan("pub fn existing(x: f64) -> f64 { x }");
        let changed =
            scan("pub fn existing(x: f64) -> f64 { x } pub fn added(x: f64) -> f64 { x }");
        assert!(check_snapshot(&saved, &saved).is_ok());
        assert!(check_snapshot(&saved, &changed).is_err());
        assert!(changed.contains("crate::kernels::fixture::added"));
    }
    #[test]
    fn checked_in_inventory_matches_live_kernel_sources() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../rssn");
        let snapshot = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("inventory.tsv");
        let expected = std::fs::read_to_string(&snapshot).expect("inventory snapshot");
        let actual = report(&root).expect("live inventory");
        assert!(
            actual == expected,
            "kernel inventory changed; review `--report` and regenerate with `--write`"
        );
    }
}
