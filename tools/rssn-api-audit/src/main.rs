//! Audits public Rust numerical APIs against declared RSSN dynamic exports.
//! A source inventory is not a count of callable operators: unregistered,
//! generic, callback and Rust-only shapes remain separate.
use rssn_api_model::numerical_codegen;
use rssn_api_model::structured_codegen;

use quote::ToTokens;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use syn::Item;
use syn::Visibility;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    path: String,
    signature: String,
    cfg: String,
    reason: &'static str,
}
fn public(vis: &Visibility) -> bool {
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
    let path = if path.first().is_some_and(|part| part == "self") {
        &path[1..]
    } else {
        path
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
                            depth + 1,
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
                depth + 1,
                module_depth + 1,
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
        let file = if let Some(path) = path_attr {
            dir.parent().unwrap_or(dir).join(path)
        } else if direct.exists() {
            direct
        } else {
            nested
        };
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
            depth + 1,
            module_depth + 1,
        );
    }
    Ok(None)
}

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
                let hash = text.bytes().fold(0xcbf29ce484222325u64, |h, b| {
                    (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
                });
                let c = [inherited_cfg, &cfg(&m.attrs)].join(" ").trim().to_string();
                out.push(Entry {
                    path: format!("{prefix}::<macro:{hash:016x}>"),
                    signature: format!("{} (unexpanded)", compact(&m.mac.path)),
                    cfg: c,
                    reason: "unexpanded-macro",
                });
            },
            | Item::Use(u) if public(&u.vis) => {
                let text = compact(&u.tree);
                let hash = text.bytes().fold(0xcbf29ce484222325u64, |h, b| {
                    (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
                });
                let c = [inherited_cfg, &cfg(&u.attrs)].join(" ").trim().to_string();
                let mut names = Vec::new();
                named_uses(&u.tree, &[], &mut names);
                for (target, alias) in names {
                    if let Some((signature, resolved_cfg)) =
                        resolve_local_function(items, dir, &target, &c, 0, 0)?
                    {
                        out.push(Entry {
                            path: format!("{prefix}::{alias}"),
                            reason: type_reason(&signature, false),
                            signature: compact(&signature),
                            cfg: resolved_cfg,
                        });
                    }
                }
                out.push(Entry {
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
                    let file = if direct.exists() {
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
                        let Some(original) = aliases.get(&path.path.segments[0].ident.to_string())
                        else {
                            continue;
                        };
                        let mut implementation = implementation.clone();
                        implementation.self_ty = Box::new(
                            syn::parse_str(original)
                                .map_err(|e| format!("invalid parent type: {e}"))?,
                        );
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
                let hash: String = key
                    .signature_hash
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
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

fn report(root: &Path) -> Result<String, String> {
    let mut entries = Vec::new();
    for module in ["kernels", "backend"] {
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
    for path in exports.keys() {
        if !present.contains(path.as_str()) {
            return Err(format!(
                "ABI metadata points to missing public kernel function: {path}"
            ));
        }
    }
    Ok(render(entries, &exports))
}
fn render(
    entries: Vec<Entry>,
    exports: &BTreeMap<String, Vec<String>>,
) -> String {
    let mut out = String::from(
        "# RSSN public kernel and backend source inventory v1\n# source syntax only; cfg is not evaluated; macros are not expanded; exported is not a runtime capability claim\n# path\tclassification\tabi names\tcfg\tsignature\n",
    );
    for e in entries {
        let names = exports.get(&e.path);
        let class = if names.is_some() {
            "declared"
        } else {
            e.reason
        };
        let names = names.map_or_else(String::new, |x| x.join(","));
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            e.path, class, names, e.cfg, e.signature
        ));
    }
    out
}
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if matches!(args.as_slice(), [arg] if matches!(arg.as_str(), "--structured-write" | "--structured-check" | "--structured-report"))
    {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../rssn");
        let policy = root.join("tools/rssn-api-gen/structured-policy.tsv");
        let target = root.join("src/rules/numerical_abi/structured_generated.rs");
        let result = structured_codegen::generate(&root, &std::fs::read_to_string(policy)?)?;
        match args[0].as_str() {
            | "--structured-report" => print!("{result}"),
            | "--structured-write" => {
                std::fs::write(&target, result)?;
                println!("wrote {}", target.display());
            },
            | _ => {
                if std::fs::read_to_string(&target)? != result {
                    return Err("generated structured codecs changed; review --structured-report and run --structured-write".into());
                }
                println!("generated structured codecs verified");
            },
        }
        return Ok(());
    }
    if matches!(args.as_slice(), [arg] if matches!(arg.as_str(), "--bindings-write" | "--bindings-check" | "--bindings-report"))
    {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../rssn");
        let policy = root.join("tools/rssn-api-gen/numerical-policy.tsv");
        let target = root.join("src/rules/numerical_abi/generated.rs");
        let source = std::fs::read_to_string(policy)?;
        let result = numerical_codegen::generate(&root, &source)?;
        match args[0].as_str() {
            | "--bindings-report" => print!("{result}"),
            | "--bindings-write" => {
                std::fs::write(&target, result)?;
                println!("wrote {}", target.display());
            },
            | _ => {
                let saved = std::fs::read_to_string(&target)?;
                if saved != result {
                    return Err("generated numerical bindings changed; review --bindings-report and run --bindings-write".into());
                }
                println!("generated numerical bindings verified");
            },
        }
        return Ok(());
    }
    if args != ["--check"] && args != ["--write"] && args != ["--report"] {
        return Err("usage: rssn-api-audit --check|--write|--report|--bindings-check|--bindings-write|--bindings-report|--structured-check|--structured-write|--structured-report".into());
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../rssn");
    let snapshot = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("inventory.tsv");
    let result = report(&root)?;
    match args[0].as_str() {
        | "--report" => print!("{result}"),
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
mod tests {
    use super::*;

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
            r#"
            pub struct Tape;
            struct Hidden;
            mod derivative {
                use super::{Tape as Parent, Hidden};
                impl Parent { pub fn vjp(&self) {} fn secret(&self) {} }
                impl Hidden { pub fn hidden(&self) {} }
                pub fn not_exported() {}
            }
        "#,
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
