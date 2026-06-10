//! Module resolution, visibility, and `use` import binding. Phase 4.

use crate::ast::{DimExpr, Item, SourceFile, Spanned, Visibility};
use crate::diagnostics::{AeviaError, AeviaResult};
use crate::parser::items::parse_source;
use crate::types::dim::{resolve, DimVector};
use crate::types::env::TypeEnv;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A multi-module program loaded from an entry `.ae` file.
#[derive(Debug)]
pub struct ModuleProgram {
    pub entry: PathBuf,
    pub crate_root: Option<PathBuf>,
    modules: IndexMap<PathBuf, LoadedModule>,
}

/// One loaded source file and its resolved import bindings.
#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub path: PathBuf,
    pub ast: SourceFile,
    pub imports: ImportBindings,
}

/// Names brought into scope via `use` for dimensional checking.
#[derive(Debug, Clone, Default)]
pub struct ImportBindings {
    /// Function return dimensions keyed by local name (after `as` rename).
    pub functions: HashMap<String, DimVector>,
    /// Type alias dimensions keyed by local name.
    pub aliases: HashMap<String, DimVector>,
}

#[derive(Debug, Clone)]
enum Export {
    Fn(DimVector),
    Alias(DimVector),
}

/// Load an entry file and all `mod`-referenced modules.
pub fn load_program(entry: &Path) -> AeviaResult<ModuleProgram> {
    let entry = entry
        .canonicalize()
        .map_err(|e| AeviaError::io(entry, e))?;
    let crate_root = crate::manifest::find_project_root(&entry).ok();
    let mut program = ModuleProgram {
        entry: entry.clone(),
        crate_root: crate_root.clone(),
        modules: IndexMap::new(),
    };
    program.load_recursive(&entry)?;
    program.resolve_imports()?;
    Ok(program)
}

/// Entry AST for build / run (the file passed on the CLI).
#[must_use]
pub fn entry_ast(program: &ModuleProgram) -> &SourceFile {
    &program.modules[&program.entry].ast
}

/// Import bindings for the entry module.
#[must_use]
pub fn entry_imports(program: &ModuleProgram) -> &ImportBindings {
    &program.modules[&program.entry].imports
}

/// Iterate loaded modules (entry file and dependencies).
pub fn modules<'a>(program: &'a ModuleProgram) -> impl Iterator<Item = &'a LoadedModule> {
    program.modules.values()
}

/// Register functions and custom ops from every loaded module for JIT lowering.
pub fn register_lowerer(program: &ModuleProgram, lowerer: &mut crate::lowering::Lowerer) {
    for module in modules(program) {
        lowerer.register_items(&module.ast);
    }
}

/// Apply `use` imports into a type environment before checking.
pub fn apply_imports(env: &mut TypeEnv, imports: &ImportBindings) {
    for (name, dim) in &imports.aliases {
        env.register_alias(name.clone(), *dim);
    }
    for (name, dim) in &imports.functions {
        env.register_fn(name.clone(), *dim);
    }
}

impl ModuleProgram {
    fn load_recursive(&mut self, path: &Path) -> AeviaResult<()> {
        let key = path
            .canonicalize()
            .map_err(|e| AeviaError::io(path, e))?;
        if self.modules.contains_key(&key) {
            return Ok(());
        }

        let text = std::fs::read_to_string(&key).map_err(|e| AeviaError::io(&key, e))?;
        let ast = parse_source(&text)
            .map_err(|e| AeviaError::message(format!("parse error in {}: {e}", key.display())))?;

        let mut deps = external_mod_names(&ast);
        deps.extend(use_module_roots(&ast));
        deps.sort();
        deps.dedup();

        self.modules.insert(
            key.clone(),
            LoadedModule {
                path: key.clone(),
                ast,
                imports: ImportBindings::default(),
            },
        );

        for name in deps {
            if let Ok(child) = resolve_mod_path(&key, &name) {
                self.load_recursive(&child)?;
            }
        }

        Ok(())
    }

    fn resolve_imports(&mut self) -> AeviaResult<()> {
        let paths: Vec<PathBuf> = self.modules.keys().cloned().collect();
        for path in paths {
            let bindings = self.resolve_imports_for(&path)?;
            if let Some(module) = self.modules.get_mut(&path) {
                module.imports = bindings;
            }
        }
        Ok(())
    }

    fn resolve_imports_for(&self, path: &Path) -> AeviaResult<ImportBindings> {
        let module = self
            .modules
            .get(path)
            .ok_or_else(|| AeviaError::message("internal module table inconsistency"))?;
        let mut bindings = ImportBindings::default();

        for item in &module.ast.items {
            if let Item::Use { path: use_path, alias, glob } = &item.node {
                if *glob {
                    self.bind_glob_use(path, use_path, &mut bindings)?;
                } else {
                    self.bind_use(path, use_path, alias.as_deref(), &mut bindings)?;
                }
            }
        }
        Ok(bindings)
    }

    fn bind_use(
        &self,
        importer: &Path,
        use_path: &[String],
        alias: Option<&str>,
        bindings: &mut ImportBindings,
    ) -> AeviaResult<()> {
        if use_path.is_empty() {
            return Err(AeviaError::message("empty use path"));
        }

        let (module_path, name) = use_path.split_at(use_path.len() - 1);
        let export_name = &name[0];

        if module_path.is_empty() {
            return Err(AeviaError::message(format!(
                "use must import from a module path, got `{export_name}` alone"
            )));
        }

        let export = self.resolve_export(importer, module_path, export_name)?;
        let local = alias.unwrap_or(export_name);

        match export {
            Export::Fn(dim) => {
                bindings.functions.insert(local.to_string(), dim);
            }
            Export::Alias(dim) => {
                bindings.aliases.insert(local.to_string(), dim);
            }
        }
        Ok(())
    }

    fn bind_glob_use(
        &self,
        importer: &Path,
        use_path: &[String],
        bindings: &mut ImportBindings,
    ) -> AeviaResult<()> {
        if use_path.is_empty() {
            return Err(AeviaError::message("empty use path"));
        }

        let (owner, items) = self.resolve_module_items(importer, use_path)?;
        let same_crate = self.same_crate(importer, &owner);
        let same_file = importer == owner;

        for item in &items {
            match &item.node {
                Item::Function {
                    name,
                    return_type,
                    visibility,
                    ..
                } if visible(*visibility, same_file, same_crate) => {
                    let dim = return_type
                        .as_ref()
                        .and_then(resolve_dim)
                        .unwrap_or(DimVector::DIMENSIONLESS);
                    bindings.functions.insert(name.clone(), dim);
                }
                Item::TypeAlias {
                    name,
                    dimension_expr,
                    ..
                } => {
                    // Type aliases have no visibility modifier — treat as public.
                    if let Some(dim) = resolve_dim(dimension_expr) {
                        bindings.aliases.insert(name.clone(), dim);
                    }
                }
                Item::Const {
                    name,
                    dim,
                    visibility,
                    ..
                } if visible(*visibility, same_file, same_crate) => {
                    if let Some(dim_span) = dim {
                        if let Some(d) = resolve_dim(dim_span) {
                            bindings.functions.insert(name.clone(), d);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn resolve_export(
        &self,
        importer: &Path,
        module_path: &[String],
        name: &str,
    ) -> AeviaResult<Export> {
        let (owner, items) = self.resolve_module_items(importer, module_path)?;
        let same_crate = self.same_crate(importer, &owner);
        let same_file = importer == owner;

        for item in &items {
            match &item.node {
                Item::Function {
                    name: item_name,
                    return_type,
                    visibility,
                    ..
                } if item_name == name => {
                    if !visible(*visibility, same_file, same_crate) {
                        return Err(AeviaError::message(format!(
                            "`{name}` is not accessible from this module"
                        )));
                    }
                    let dim = return_type
                        .as_ref()
                        .and_then(resolve_dim)
                        .unwrap_or(DimVector::DIMENSIONLESS);
                    return Ok(Export::Fn(dim));
                }
                Item::TypeAlias {
                    name: item_name,
                    dimension_expr,
                    ..
                } if item_name == name => {
                    let dim = resolve_dim(dimension_expr).ok_or_else(|| {
                        AeviaError::message(format!("unknown unit in alias `{name}`"))
                    })?;
                    return Ok(Export::Alias(dim));
                }
                _ => {}
            }
        }

        Err(AeviaError::message(format!(
            "no export `{name}` in module `{}`",
            module_path.join("::")
        )))
    }

    fn resolve_module_items(
        &self,
        importer: &Path,
        path: &[String],
    ) -> AeviaResult<(PathBuf, Vec<Spanned<Item>>)> {
        if path.is_empty() {
            return Err(AeviaError::message("empty module path"));
        }

        let mut current_path = importer.to_path_buf();
        let mut items = self
            .modules
            .get(&current_path)
            .map(|m| m.ast.items.clone())
            .ok_or_else(|| AeviaError::message("module not loaded"))?;

        for segment in path {
            let child = find_child_module(&current_path, &items, segment)
                .or_else(|_| resolve_mod_path(&current_path, segment).map(ModuleRef::File));
            match child? {
                ModuleRef::File(child_path) => {
                    current_path = child_path;
                    items = self
                        .modules
                        .get(&current_path)
                        .map(|m| m.ast.items.clone())
                        .ok_or_else(|| {
                            AeviaError::message(format!(
                                "module file not loaded: {}",
                                current_path.display()
                            ))
                        })?;
                }
                ModuleRef::Inline(body) => {
                    items = body;
                }
            }
        }

        Ok((current_path, items))
    }

    fn same_crate(&self, a: &Path, b: &Path) -> bool {
        match &self.crate_root {
            Some(root) => a.starts_with(root) && b.starts_with(root),
            None => a.parent() == b.parent(),
        }
    }
}

#[derive(Debug, Clone)]
enum ModuleRef {
    File(PathBuf),
    Inline(Vec<Spanned<Item>>),
}

fn visible(vis: Visibility, same_file: bool, same_crate: bool) -> bool {
    match vis {
        Visibility::Private => same_file,
        Visibility::Crate => same_crate,
        Visibility::Public => true,
    }
}

fn use_module_roots(ast: &SourceFile) -> Vec<String> {
    ast.items
        .iter()
        .filter_map(|item| {
            if let Item::Use { path, .. } = &item.node {
                path.first().cloned()
            } else {
                None
            }
        })
        .collect()
}

fn external_mod_names(ast: &SourceFile) -> Vec<String> {
    ast.items
        .iter()
        .filter_map(|item| {
            if let Item::Module {
                name,
                body: None,
                ..
            } = &item.node
            {
                Some(name.clone())
            } else {
                None
            }
        })
        .collect()
}

fn find_child_module(
    declaring_file: &Path,
    items: &[Spanned<Item>],
    name: &str,
) -> AeviaResult<ModuleRef> {
    for item in items {
        if let Item::Module {
            name: mod_name,
            body,
            ..
        } = &item.node
        {
            if mod_name == name {
                return Ok(match body {
                    None => ModuleRef::File(resolve_mod_path(declaring_file, name)?),
                    Some(inner) => ModuleRef::Inline(inner.clone()),
                });
            }
        }
    }

    Err(AeviaError::message(format!("module `{name}` not found")))
}

/// Resolve `mod foo;` to a filesystem path relative to the declaring file.
pub fn resolve_mod_path(declaring_file: &Path, name: &str) -> AeviaResult<PathBuf> {
    let dir = declaring_file.parent().unwrap_or(Path::new("."));
    let mut candidates = vec![
        dir.join(format!("{name}.ae")),
        dir.join(name).join("mod.ae"),
        dir.join("modules").join(format!("{name}.ae")),
        dir.join("modules").join(name).join("mod.ae"),
    ];

    if let Ok(root) = crate::manifest::find_project_root(declaring_file) {
        candidates.push(root.join("src").join(format!("{name}.ae")));
        candidates.push(root.join("src").join(name).join("mod.ae"));
        candidates.push(root.join("src/modules").join(format!("{name}.ae")));
        candidates.push(root.join("src/modules").join(name).join("mod.ae"));
    }

    for candidate in candidates {
        if candidate.is_file() {
            return candidate.canonicalize().map_err(|e| AeviaError::io(&candidate, e));
        }
    }

    Err(AeviaError::message(format!(
        "cannot find module `{name}` (searched near {})",
        dir.display()
    )))
}

fn resolve_dim(span: &Spanned<DimExpr>) -> Option<DimVector> {
    resolve(span)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn resolves_mod_under_modules_dir() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(src.join("modules")).unwrap();
        fs::write(src.join("lib.ae"), "pub mod physics;").unwrap();
        fs::write(
            src.join("modules/physics.ae"),
            "pub fn energy(m: kg, v: m/s) -> J := 0.5 * m * v^2;",
        )
        .unwrap();

        let program = load_program(&src.join("lib.ae")).unwrap();
        assert_eq!(program.modules.len(), 2);
    }

    #[test]
    fn use_imports_function_return_type() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(src.join("modules")).unwrap();
        fs::write(
            src.join("modules/physics.ae"),
            "pub fn energy(m: kg, v: m/s) -> J := 0.5 * m * v^2;",
        )
        .unwrap();
        fs::write(
            src.join("main.ae"),
            "use physics::energy;\nfn main() -> J := energy(1.0, 1.0);",
        )
        .unwrap();

        // main.ae: `use physics::…` loads src/modules/physics.ae via filesystem fallback
        let program = load_program(&src.join("main.ae")).unwrap();
        let imports = &program.modules[&program.entry].imports;
        assert!(imports.functions.contains_key("energy"));
    }
}
