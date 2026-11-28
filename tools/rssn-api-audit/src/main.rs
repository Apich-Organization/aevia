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
use std::rc::Rc;
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

#[derive(Default)]
struct Sources {
    parsed: BTreeMap<PathBuf, Rc<syn::File>>,
}

impl Sources {
    fn parse(
        &mut self,
        path: &Path,
    ) -> Result<Rc<syn::File>, String> {
        if let Some(parsed) = self.parsed.get(path) {
            return Ok(Rc::clone(parsed));
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let parsed =
            Rc::new(syn::parse_file(&text).map_err(|e| format!("{}: {e}", path.display()))?);
        self.parsed.insert(path.to_path_buf(), Rc::clone(&parsed));
        Ok(parsed)
    }
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

// Collect named import paths and remember whether the statement has a glob.
fn named_uses(
    tree: &syn::UseTree,
    prefix: &mut Vec<String>,
    out: &mut Vec<(Vec<String>, String)>,
) -> bool {
    match tree {
        | syn::UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            let has_glob = named_uses(&path.tree, prefix, out);
            let _ = prefix.pop();
            has_glob
        },
        | syn::UseTree::Name(name) => {
            let mut path = prefix.clone();
            path.push(name.ident.to_string());
            out.push((path, name.ident.to_string()));
            false
        },
        | syn::UseTree::Rename(name) => {
            let mut path = prefix.clone();
            path.push(name.ident.to_string());
            out.push((path, name.rename.to_string()));
            false
        },
        | syn::UseTree::Group(group) => {
            let mut has_glob = false;
            for tree in &group.items {
                has_glob |= named_uses(tree, prefix, out);
            }
            has_glob
        },
        | syn::UseTree::Glob(_) => true,
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "keeps visibility, cfg and re-export traversal in one place"
)]
fn resolve_local_function(
    sources: &mut Sources,
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
                named_uses(&u.tree, &mut Vec::new(), &mut names);
                for (target, alias) in names {
                    if alias == *head {
                        let child_cfg = [cfg_so_far, &cfg(&u.attrs)].join(" ").trim().to_string();
                        if let Some(found) = resolve_local_function(
                            sources,
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
                sources,
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
        let parsed = sources.parse(&file)?;
        return resolve_local_function(
            sources,
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
    sources: &mut Sources,
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
                named_uses(&u.tree, &mut Vec::new(), &mut uses);
                for (target, alias) in uses {
                    if alias == *head {
                        let c = [cfg_so_far, &cfg(&u.attrs)].join(" ").trim().to_string();
                        let found = resolve_local_methods(
                            sources,
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
                sources,
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
        let parsed = sources.parse(&file)?;
        return resolve_local_methods(
            sources,
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
    sources: &mut Sources,
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
                let has_glob = named_uses(&u.tree, &mut Vec::new(), &mut names);
                let named_count = names.len();
                let mut resolved = 0usize;
                for (target, alias) in names {
                    let start = out.len();
                    if let Some((signature, resolved_cfg)) =
                        resolve_local_function(sources, items, dir, &target, &c, 0, 0)?
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
                        resolve_local_methods(sources, items, dir, &target, &c, 0, 0)?
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
                    resolved = resolved.saturating_add(usize::from(out.len() > start));
                }
                let reason = if resolved == 0 {
                    "unresolved-reexport"
                } else if has_glob || resolved != named_count {
                    "partial-reexport"
                } else {
                    "resolved-reexport"
                };
                out.push(Entry {
                    alias_of: None,
                    path: format!("{prefix}::<reexport:{hash:016x}>"),
                    signature: text,
                    cfg: c,
                    reason,
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
                    parsed = sources.parse(&file)?;
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
                        visit_items(
                            sources,
                            &[Item::Impl(implementation)],
                            prefix,
                            dir,
                            &child_cfg,
                            out,
                        )?;
                    }
                }
            },
            | Item::Mod(m) if public(&m.vis) => {
                let name = m.ident.to_string();
                let child_prefix = format!("{prefix}::{name}");
                let child_cfg = [inherited_cfg, &cfg(&m.attrs)].join(" ").trim().to_string();
                if let Some((_, items)) = &m.content {
                    visit_items(
                        sources,
                        items,
                        &child_prefix,
                        &dir.join(&name),
                        &child_cfg,
                        out,
                    )?;
                } else {
                    let file = dir.join(format!("{name}.rs"));
                    let nested = dir.join(&name).join("mod.rs");
                    let file = if file.exists() {
                        file
                    } else {
                        nested
                    };
                    let parsed = sources.parse(&file)?;
                    visit_items(
                        sources,
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
fn explicit_type(ty: &rssn::kernel::signature::TypeExpr) -> bool {
    use rssn::kernel::signature::TypeExpr as T;
    match ty {
        | T::Dyn(_) => false,
        | T::Dim(elem, _) | T::Tensor { elem, .. } | T::Sparse { elem, .. } => explicit_type(elem),
        | T::Record(fields) => fields.iter().all(|(_, ty)| explicit_type(ty)),
        | T::Variant(tags) => {
            tags.iter()
                .all(|(_, fields)| fields.iter().all(explicit_type))
        },
        | T::Function(params, ret) => params.iter().all(explicit_type) && explicit_type(ret),
        | T::Table(row) => row.columns.iter().all(|(_, ty)| explicit_type(ty)),
        | _ => true,
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "isolates manifest provenance validation"
)]
fn exported(
    manifest: &rssn::kernel::KernelManifest
) -> Result<BTreeMap<String, Vec<String>>, String> {
    use rssn::kernel::manifest::SignatureSource;
    let mut paths: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for info in &manifest.ops {
        let Some(path) = &info.rust_path else {
            continue;
        };
        let typed_source = match info.signature_source {
            | SignatureSource::Declared => true,
            | SignatureSource::Overlay => {
                info.overloads.iter().any(|signature| {
                    explicit_type(&signature.ret) && signature.params.iter().all(explicit_type)
                })
            },
            | _ => false,
        };
        if !path.starts_with("crate::")
            || !typed_source
            || info.overloads.is_empty()
            || info.overloads.len() != info.keys.len()
        {
            return Err(format!(
                "invalid source provenance or missing explicit typed signature for {}",
                info.name
            ));
        }
        for (signature, key) in info.overloads.iter().zip(&info.keys) {
            let expected =
                rssn::kernel::signature::OpKey::new(&info.rule_set, &info.name, Some(signature));
            if *key != expected {
                return Err(format!(
                    "manifest key disagrees with the signature for {}",
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
fn report(
    root: &Path,
    manifest: &rssn::kernel::KernelManifest,
) -> Result<Inventory, String> {
    let mut entries = Vec::new();
    let mut sources = Sources::default();
    for module in ["kernels", "backend", "sim"] {
        let dir = root.join("src").join(module);
        let path = dir.join("mod.rs");
        let parsed = sources.parse(&path)?;
        visit_items(
            &mut sources,
            &parsed.items,
            &format!("crate::{module}"),
            &dir,
            "",
            &mut entries,
        )?;
    }
    let exports = exported(manifest)?;
    let present: BTreeSet<_> = entries.iter().map(|entry| entry.path.as_str()).collect();
    let mut missing = Vec::new();
    let mut resolved = Vec::new();
    let src = root.join("src");
    for path in exports
        .keys()
        .filter(|path| !present.contains(path.as_str()))
    {
        // Resolve only the function named by the adapter. Keep sibling
        // modules outside the numerical source inventory.
        let Some(tail) = path.strip_prefix("crate::") else {
            missing.push(path);
            continue;
        };
        let segments: Vec<_> = tail.split("::").map(str::to_owned).collect();
        let lib = sources.parse(&src.join("lib.rs"))?;
        if let Some((signature, cfg)) =
            resolve_local_function(&mut sources, &lib.items, &src, &segments, "", 0, 1)?
        {
            resolved.push(Entry {
                alias_of: None,
                path: path.clone(),
                reason: type_reason(&signature, false),
                signature: compact(&signature),
                cfg,
            });
        } else {
            missing.push(path);
        }
    }
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
    entries.extend(resolved);
    entries.sort_by(|a, b| (&a.path, &a.signature, &a.cfg).cmp(&(&b.path, &b.signature, &b.cfg)));
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

#[derive(Default)]
struct Inventory {
    tsv: String,
    aliases: BTreeMap<String, String>,
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps the source identity check separate from rendering"
)]
fn proven_aliases(entries: &[Entry]) -> BTreeMap<String, String> {
    let mut aliases = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    for entry in entries {
        match &entry.alias_of {
            | Some(target) => {
                if let Some(previous) = aliases.insert(entry.path.clone(), target.clone())
                    && previous != *target
                {
                    ambiguous.insert(entry.path.as_str());
                }
            },
            | None => {
                ambiguous.insert(entry.path.as_str());
            },
        }
    }
    // Different conditional definitions of the same path can designate
    // different functions. Share review only for unambiguous source identities.
    aliases.retain(|path, _| !ambiguous.contains(path.as_str()));
    aliases
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps canonical TSV serialization separate from source analysis"
)]
fn render(
    entries: Vec<Entry>,
    exports: &BTreeMap<String, Vec<String>>,
) -> Inventory {
    let mut out = String::from(
        "# RSSN public kernel, backend and simulation source inventory v2\n# source syntax only; cfg is not evaluated; macros are not expanded; exported is not a runtime capability claim\n# path\tclassification\tabi names\tcfg\tsignature\n",
    );
    let aliases = proven_aliases(&entries);
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
    Inventory { tsv: out, aliases }
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
        Err("kernel API inventory changed; inspect --report and update with --write-checked")
    }
}

/// Require a declared adapter or an explicit Rust host boundary for each API.
#[allow(
    clippy::single_call_fn,
    reason = "keeps the source boundary policy directly testable"
)]
fn check_complete(
    inventory: &Inventory,
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
    let reviewed: BTreeSet<_> = host_boundaries
        .keys()
        .filter_map(|path| identity(path, &inventory.aliases))
        .collect();
    let mut matched_boundaries = BTreeSet::new();
    let mut adapted = 0usize;
    let mut host_only = 0usize;
    let mut missing = Vec::new();
    for row in inventory
        .tsv
        .lines()
        .filter(|row| !row.starts_with('#') && !row.is_empty())
    {
        let fields: Vec<_> = row.split('\t').collect();
        let [path, class, names, _, _] = fields.as_slice() else {
            return Err("malformed source inventory row".into());
        };
        if matches!(
            *class,
            "unexpanded-macro" | "unresolved-reexport" | "partial-reexport" | "resolved-reexport"
        ) {
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
            host_only = host_only.saturating_add(1);
        } else if identity(path, &inventory.aliases).is_some_and(|key| reviewed.contains(&key)) {
            host_only = host_only.saturating_add(1);
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
        Ok((adapted, host_only))
    } else {
        Err(format!(
            "{} callable source interfaces need a declared adapter or a reviewed Rust host boundary:\n{}",
            missing.len(),
            missing.join("\n")
        ))
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps snapshot writes and validation directly testable"
)]
fn apply_inventory(
    mode: &str,
    snapshot: &Path,
    inventory: &Inventory,
    policy: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if matches!(mode, "--check" | "--verify") {
        check_snapshot(&std::fs::read_to_string(snapshot)?, &inventory.tsv)?;
        println!("kernel API inventory verified");
    }
    if matches!(mode, "--coverage-check" | "--verify" | "--write-checked") {
        let (adapted, host_only) = check_complete(inventory, policy)?;
        println!(
            "{adapted} callable source interfaces have declared adapters; {host_only} reviewed Rust host interfaces"
        );
    }
    if matches!(mode, "--write" | "--write-checked") {
        std::fs::write(snapshot, &inventory.tsv)?;
        println!("wrote {}", snapshot.display());
    }
    if mode == "--report" {
        print!("{}", inventory.tsv);
    }
    Ok(())
}

#[allow(
    clippy::single_call_fn,
    reason = "keeps domain report rendering separate from CLI argument parsing"
)]
fn apply_domain_report(
    action: &str,
    manifest: &rssn::kernel::KernelManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    let registry = aevia_rssn::registry::Registry::from_manifest(manifest);
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"));
    let chapter = std::fs::read_to_string(directory.join("../../spec/08-stdlib.md"))?;
    let names = aevia_stdlib_model::spec::names(&chapter)
        .into_iter()
        .collect();
    let text =
        aevia_rssn::domains::coverage_report(&registry, aevia_rssn::domains::overlay(), &names);
    let path = directory.join("RSSN-COVERAGE.md");
    match action {
        | "report" => print!("{text}"),
        | "write" => std::fs::write(path, text)?,
        | _ => {
            if std::fs::read_to_string(path)?.replace("\r\n", "\n") != text {
                return Err("Aevia domain coverage changed; review --domains-report and run --domains-write".into());
            }
            println!("Aevia domain coverage verified");
        },
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    type Generator = fn(&Path, &str) -> Result<String, String>;
    const USAGE: &str = "usage: rssn-api-audit --verify|--write-checked|--check|--write|--report|--coverage-check|--domains-report|--domains-write|--domains-check|--bindings-check|--bindings-write|--bindings-report|--structured-check|--structured-write|--structured-report|--codecs-check|--codecs-write|--codecs-report [--manifest PATH]";
    let mut args = std::env::args_os().skip(1);
    let mode = args.next().ok_or(USAGE)?;
    let mode = mode.to_str().ok_or(USAGE)?;
    let manifest_path = match (args.next(), args.next(), args.next()) {
        | (None, None, None) => None,
        | (Some(flag), Some(path), None) if flag == "--manifest" => Some(PathBuf::from(path)),
        | _ => return Err(USAGE.into()),
    };
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
            if manifest_path.is_some() {
                return Err("--manifest applies to inventory commands".into());
            }
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
        "--verify"
            | "--write-checked"
            | "--check"
            | "--write"
            | "--report"
            | "--coverage-check"
            | "--domains-report"
            | "--domains-write"
            | "--domains-check"
    ) {
        return Err(USAGE.into());
    }
    let snapshot = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("inventory.tsv");
    let manifest = if let Some(path) = manifest_path {
        let supplied = rssn::kernel::KernelManifest::from_json(&std::fs::read_to_string(&path)?)?;
        if !supplied.verify() {
            return Err(format!("manifest digest disagrees with {}", path.display()).into());
        }
        std::sync::Arc::new(supplied)
    } else {
        rssn::api::Session::new().manifest()
    };
    if let Some(action) = mode.strip_prefix("--domains-") {
        return apply_domain_report(action, &manifest);
    }
    let result = report(&root, &manifest)?;
    apply_inventory(
        mode,
        &snapshot,
        &result,
        include_str!("../source-boundaries.tsv"),
    )
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
    fn declared_exports_can_point_outside_the_numerical_modules() {
        use rssn::kernel::manifest::SignatureSource;

        let dir =
            std::env::temp_dir().join(format!("rssn-audit-extra-export-{}", std::process::id()));
        let src = dir.join("src");
        for module in ["kernels", "backend", "sim"] {
            let module_dir = src.join(module);
            std::fs::create_dir_all(&module_dir).unwrap();
            std::fs::write(module_dir.join("mod.rs"), "").unwrap();
        }
        std::fs::write(
            src.join("lib.rs"),
            r#"#[cfg(feature = "pde")] pub mod rules;
               mod private { pub fn f() {} }"#,
        )
        .unwrap();
        std::fs::write(
            src.join("rules.rs"),
            "pub mod pde { pub fn sample(x: f64) -> f64 { x } fn hidden() {} }",
        )
        .unwrap();

        let mut manifest = rssn::api::Session::new().manifest().as_ref().clone();
        let mut operator = manifest
            .ops
            .iter()
            .find(|info| {
                matches!(info.signature_source, SignatureSource::Declared)
                    && info.rust_path.is_some()
                    && info.keys.len() == 1
            })
            .expect("a declared operator with one overload")
            .clone();
        let path = "crate::rules::pde::sample";
        operator.rust_path = Some(path.into());
        let expected = format!(
            "{path}\tdeclared\t{}\t# [cfg (feature = \"pde\")]\tfn sample (x : f64) -> f64",
            operator.name
        );
        manifest.ops = vec![operator];

        let inventory = report(&dir, &manifest).unwrap();
        let entries: Vec<_> = inventory
            .tsv
            .lines()
            .filter(|line| !line.starts_with('#'))
            .collect();
        assert_eq!(entries, [expected.as_str()]);
        for missing in [
            "crate::rules::pde::hidden",
            "crate::rules::pde::absent",
            "crate::private::f",
        ] {
            manifest.ops[0].rust_path = Some(missing.into());
            assert_eq!(
                report(&dir, &manifest).err(),
                Some(format!(
                    "ABI metadata points to missing public kernel functions:\n{missing}"
                ))
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn combined_snapshot_actions_use_the_same_checked_report() {
        let path =
            std::env::temp_dir().join(format!("rssn-checked-inventory-{}.tsv", std::process::id()));
        let accepted = Inventory {
            tsv: "crate::kernels::f\tdeclared\tnum_f\t\tfn f()\n".into(),
            ..Inventory::default()
        };
        let incomplete = Inventory {
            tsv: "crate::kernels::f\tno-declared-adapter\t\t\tfn f()\n".into(),
            ..Inventory::default()
        };
        std::fs::write(&path, "previous snapshot").unwrap();
        assert!(apply_inventory("--write-checked", &path, &incomplete, "").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "previous snapshot");
        assert!(apply_inventory("--verify", &path, &accepted, "").is_err());

        apply_inventory("--write-checked", &path, &accepted, "").unwrap();
        apply_inventory("--verify", &path, &accepted, "").unwrap();
        std::fs::write(&path, &incomplete.tsv).unwrap();
        assert!(apply_inventory("--verify", &path, &incomplete, "").is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn one_audit_resolves_each_file_from_the_same_snapshot() {
        let dir = std::env::temp_dir().join(format!("rssn-audit-syntax-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("module.rs");
        std::fs::write(&path, "pub fn first() {}").unwrap();
        let mut sources = Sources::default();
        let first = sources.parse(&path).unwrap();
        std::fs::write(&path, "pub fn second() {}").unwrap();
        let again = sources.parse(&path).unwrap();
        assert!(Rc::ptr_eq(&first, &again));
        let fresh = Sources::default().parse(&path).unwrap();
        assert!(compact(&fresh.items[0]).contains("second"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn alias_coverage_requires_resolved_item_identity() {
        let parsed=syn::parse_file(r"pub mod child{pub struct T;impl T{pub fn method(&self){}}}pub use child::T as Alias;pub fn unrelated(){}").unwrap();
        let mut rows = Vec::new();
        visit_items(
            &mut Sources::default(),
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
        assert!(
            report
                .tsv
                .contains("crate::kernels::Alias::method\tdeclared-alias\tnum_method")
        );
        assert!(
            report
                .tsv
                .contains("crate::kernels::unrelated\tno-declared-adapter\t")
        );
        assert!(check_complete(&report, "").is_err());
        let policy = "crate::kernels::unrelated\tRust-specific test interface\n";
        assert_eq!(check_complete(&report, policy), Ok((2, 1)));
        let stale = "crate::kernels::child::T::method\tAlready adapted\n";
        assert!(check_complete(&report, stale).is_err());
    }

    #[test]
    fn reexported_public_type_methods_and_parent_generic_receivers_keep_public_paths() {
        let source=syn::parse_file(r"mod hidden {pub struct Model<T>(T);impl<T> Model<T>{pub fn new(x:T)->Self{Self(x)} fn private(){}}}pub use hidden::Model as Public;pub struct Parent<T>(T);mod child{use super::Parent;impl<T> Parent<T>{pub fn value(&self)->&T{&self.0}}}pub struct Grid<'grid>(&'grid [f64]);impl<'grid> Grid<'grid>{pub fn new(axis:&'grid[f64])->Self{Self(axis)}}").unwrap();
        let mut rows = Vec::new();
        visit_items(
            &mut Sources::default(),
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
            rows.iter()
                .any(|r| r.path == "crate::kernels::Grid<'grid>::new")
        );
        assert!(
            !rows
                .iter()
                .any(|r| r.path.contains("hidden::") || r.path.ends_with("::private"))
        );
    }

    #[test]
    fn completeness_requires_real_declared_adapters_and_ignores_noncallable_fingerprints() {
        let complete = concat!(
            "# header\ncrate::kernels::f\tdeclared\tnum_f\t\tfn f()\n",
            "crate::kernels::<macro:1>\tunexpanded-macro\t\t\tthread_local\n",
            "crate::kernels::<reexport:1>\tresolved-reexport\t\t\tf\n",
            "crate::kernels::<reexport:2>\tpartial-reexport\t\t\t{ f, missing }\n",
            "crate::kernels::<reexport:3>\tunresolved-reexport\t\t\tother::*\n",
        );
        let inventory = |tsv: String| {
            Inventory {
                tsv,
                ..Inventory::default()
            }
        };
        assert_eq!(check_complete(&inventory(complete.into()), ""), Ok((1, 0)));
        assert!(
            check_complete(
                &inventory(complete.replace("declared\tnum_f", "opaque\t")),
                ""
            )
            .unwrap_err()
            .contains("crate::kernels::f")
        );
        assert!(
            check_complete(
                &inventory(complete.replace("declared\tnum_f", "declared\t")),
                ""
            )
            .is_err()
        );
    }

    #[test]
    fn reviewed_boundary_covers_only_proven_source_aliases() {
        let entry = |name: &str, alias_of: Option<&str>, cfg: &str| {
            Entry {
                alias_of: alias_of.map(|target| format!("crate::kernels::{target}")),
                path: format!("crate::kernels::{name}"),
                signature: "fn f()".into(),
                cfg: cfg.into(),
                reason: "opaque",
            }
        };
        let rows = vec![
            entry("source", None, ""),
            entry("alias", Some("source"), ""),
            entry("chained", Some("alias"), ""),
        ];
        let policy = "crate::kernels::source\tCaller-owned Rust work state\n";
        let exports = BTreeMap::new();
        assert_eq!(
            check_complete(&render(rows.clone(), &exports), policy),
            Ok((0, 3))
        );

        let mut ambiguous = rows;
        ambiguous.push(entry("alias", Some("alternative"), "feature = alt"));
        let error = check_complete(&render(ambiguous, &exports), policy).unwrap_err();
        assert!(error.contains("crate::kernels::alias"));
        assert!(error.contains("crate::kernels::chained"));
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
            &mut Sources::default(),
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
        let import_count = |class| entries.iter().filter(|entry| entry.reason == class).count();
        assert_eq!(import_count("resolved-reexport"), 1);
        assert_eq!(import_count("partial-reexport"), 1);
        assert_eq!(import_count("unresolved-reexport"), 3);
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
            &mut Sources::default(),
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
            &mut Sources::default(),
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
            &mut Sources::default(),
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
                &mut Sources::default(),
                &file.items,
                "crate::kernels::fixture",
                Path::new("."),
                "",
                &mut entries,
            )
            .unwrap();
            render(entries, &BTreeMap::new()).tsv
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
        let manifest = rssn::api::Session::new().manifest();
        let actual = report(&root, &manifest).expect("live inventory");
        assert!(
            actual.tsv == expected,
            "kernel inventory changed; review `--report` and regenerate with `--write`"
        );
    }
}
