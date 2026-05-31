//! CLI command implementations.

mod stub;

use crate::diagnostics::{AeviaError, AeviaResult};
use crate::logging;
use crate::project;
use crate::ast::{Item, FunctionBody};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use stub::not_implemented;

#[derive(Debug)]
pub struct RunTarget {
    pub file: PathBuf,
}

/// `aevia new`
pub fn new(name: &str, parent: &Path) -> AeviaResult<PathBuf> {
    logging::pass("new project");
    project::create_new(name, parent)
}

/// `aevia build`
pub fn build(paths: Vec<PathBuf>) -> AeviaResult<()> {
    logging::pass("build");
    let files = resolve_inputs(&paths)?;
    for file in files {
        let content = std::fs::read_to_string(&file)
            .map_err(|e| AeviaError::io(&file, e))?;
        let parsed = crate::parser::items::parse_source(&content)
            .map_err(|e| AeviaError::message(format!("parse error in {}: {}", file.display(), e)))?;
        
        let check_res = crate::types::checker::check(&parsed);
        if !check_res.ok() {
            let err_msg = check_res.errors.iter()
                .map(|e| format!("{}", e))
                .collect::<Vec<_>>()
                .join("\n");
            return Err(AeviaError::message(format!("type check failed in {}:\n{}", file.display(), err_msg)));
        }

        let mut lowerer = crate::lowering::Lowerer::new();
        lowerer.register_items(&parsed);
        
        let mut main_func = None;
        for item in &parsed.items {
            if let Item::Function { name, .. } = &item.node {
                if name == "main" || main_func.is_none() {
                    main_func = Some(item.node.clone());
                }
            }
        }

        if let Some(Item::Function { name, params, body, .. }) = main_func {
            let mut env = HashMap::new();
            for param in &params {
                let id = lowerer.builder.variable(&param.name);
                env.insert(param.name.clone(), id);
            }
            
            let _root = match &body {
                FunctionBody::Expression(expr) => lowerer.lower_expr(expr, &mut env)?,
                FunctionBody::Block(stmts) => lowerer.lower_block(stmts, &mut env)?,
            };

            let packed = lowerer.builder.packed_snapshot();
            let mut out_path = file.clone();
            out_path.set_extension("rssn");
            
            let bytes = packed.encode().map_err(|e| {
                AeviaError::message(format!("serialization failed: {:?}", e))
            })?;
            std::fs::write(&out_path, bytes.as_bytes()).map_err(|e| {
                AeviaError::io(&out_path, e)
            })?;
            
            logging::pass_detail("build", &format!("compiled function `{}` to {}", name, out_path.display()));
        } else {
            return Err(AeviaError::message(format!("no function found to compile in {}", file.display())));
        }
    }
    Ok(())
}

/// `aevia run`
pub fn run(target: RunTarget) -> AeviaResult<()> {
    logging::pass("run");
    let RunTarget { file } = target;
    let file = file.canonicalize().map_err(|e| AeviaError::io(&file, e))?;
    if !is_ae_file(&file) {
        return Err(AeviaError::message(format!(
            "expected a .ae file, got {}",
            file.display()
        )));
    }
    let _ = project::load_manifest_for(&file).ok();
    
    let content = std::fs::read_to_string(&file)
        .map_err(|e| AeviaError::io(&file, e))?;
    let parsed = crate::parser::items::parse_source(&content)
        .map_err(|e| AeviaError::message(format!("parse error in {}: {}", file.display(), e)))?;
    
    let check_res = crate::types::checker::check(&parsed);
    if !check_res.ok() {
        let err_msg = check_res.errors.iter()
            .map(|e| format!("{}", e))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(AeviaError::message(format!("type check failed in {}:\n{}", file.display(), err_msg)));
    }

    let mut lowerer = crate::lowering::Lowerer::new();
    lowerer.register_items(&parsed);

    let mut main_func = None;
    for item in &parsed.items {
        if let Item::Function { name, .. } = &item.node {
            if name == "main" || main_func.is_none() {
                main_func = Some(item.node.clone());
            }
        }
    }

    if let Some(Item::Function { name, params, body, .. }) = main_func {
        let mut env = HashMap::new();
        for param in &params {
            let id = lowerer.builder.variable(&param.name);
            env.insert(param.name.clone(), id);
        }
        
        let root = match &body {
            FunctionBody::Expression(expr) => lowerer.lower_expr(expr, &mut env)?,
            FunctionBody::Block(stmts) => lowerer.lower_block(stmts, &mut env)?,
        };

        // Compile and execute JIT
        let mut compiler = rssn_advanced::jit::compiler::JitCompiler::try_new().map_err(|e| {
            AeviaError::message(format!("JIT initialization failed: {:?}", e))
        })?;

        // Register custom lt/gt/eq helpers
        extern "C" fn lt(a: f64, b: f64) -> f64 { if a < b { 1.0 } else { 0.0 } }
        extern "C" fn gt(a: f64, b: f64) -> f64 { if a > b { 1.0 } else { 0.0 } }
        extern "C" fn eq(a: f64, b: f64) -> f64 { if a == b { 1.0 } else { 0.0 } }

        if let Some(lt_id) = lowerer.builder.lookup_function("lt") {
            compiler.register_custom_function_2(lt_id, lt);
        }
        if let Some(gt_id) = lowerer.builder.lookup_function("gt") {
            compiler.register_custom_function_2(gt_id, gt);
        }
        if let Some(eq_id) = lowerer.builder.lookup_function("eq") {
            compiler.register_custom_function_2(eq_id, eq);
        }

        let ast_proj = rssn_advanced::ast::convert::dag_to_ast(lowerer.builder.arena(), root);
        let compiled_fn = compiler.compile(&ast_proj).map_err(|e| {
            AeviaError::message(format!("JIT compilation failed: {:?}", e))
        })?;

        // Call with 1.0 arguments as defaults
        let args = vec![1.0; params.len()];
        let res = compiled_fn(args.as_ptr());
        
        logging::pass_detail("run", &format!("JIT compilation of `{}` succeeded", name));
        println!("Result of calling `{}`: {}", name, res);
    } else {
        return Err(AeviaError::message(format!("no function found to run in {}", file.display())));
    }
    Ok(())
}

/// `aevia check`
pub fn check(paths: Vec<PathBuf>) -> AeviaResult<()> {
    logging::pass("check");
    let files = resolve_inputs(&paths)?;
    for file in files {
        let content = std::fs::read_to_string(&file)
            .map_err(|e| AeviaError::io(&file, e))?;
        let parsed = crate::parser::items::parse_source(&content)
            .map_err(|e| AeviaError::message(format!("parse error in {}: {}", file.display(), e)))?;
        
        let check_res = crate::types::checker::check(&parsed);
        if !check_res.ok() {
            let err_msg = check_res.errors.iter()
                .map(|e| format!("{}", e))
                .collect::<Vec<_>>()
                .join("\n");
            return Err(AeviaError::message(format!("type check failed in {}:\n{}", file.display(), err_msg)));
        }
        logging::pass_detail("check", &format!("{} passed", file.display()));
    }
    Ok(())
}

/// `aevia fmt`
pub fn fmt(paths: Vec<PathBuf>) -> AeviaResult<()> {
    logging::pass("fmt");
    let _ = resolve_inputs(&paths)?;
    not_implemented("formatter", "Phase 1")
}

/// `aevia lint`
pub fn lint(paths: Vec<PathBuf>) -> AeviaResult<()> {
    logging::pass("lint");
    let _ = resolve_inputs(&paths)?;
    not_implemented("linter", "Phase 5")
}

/// `aevia doc`
pub fn doc(paths: Vec<PathBuf>) -> AeviaResult<()> {
    logging::pass("doc");
    let _ = resolve_inputs(&paths)?;
    not_implemented("documentation generator", "Phase 5")
}

/// `aevia shell`
pub fn shell() -> AeviaResult<()> {
    logging::pass("shell");
    not_implemented("interactive REPL", "Phase 5")
}

/// `aevia test`
pub fn test(path: &Path) -> AeviaResult<()> {
    logging::pass("test");
    let tests_dir = path.join("tests");
    if !tests_dir.is_dir() {
        return Err(AeviaError::message(format!(
            "no tests/ directory in {}",
            path.display()
        )));
    }
    not_implemented("test harness", "Phase 5")
}

fn is_ae_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "ae")
}

/// Expand paths to `.ae` files (files or directories).
fn resolve_inputs(paths: &[PathBuf]) -> AeviaResult<Vec<PathBuf>> {
    if paths.is_empty() {
        return Err(AeviaError::message(
            "provide at least one path (file or directory)",
        ));
    }

    let mut out = Vec::new();
    for path in paths {
        let path = path.canonicalize().map_err(|e| AeviaError::io(path, e))?;
        if path.is_file() {
            if is_ae_file(&path) {
                out.push(path);
            } else {
                return Err(AeviaError::message(format!(
                    "not an .ae file: {}",
                    path.display()
                )));
            }
        } else if path.is_dir() {
            collect_ae_files(&path, &mut out)?;
        } else {
            return Err(AeviaError::message(format!(
                "path not found: {}",
                path.display()
            )));
        }
    }
    if out.is_empty() {
        return Err(AeviaError::message("no .ae files found in given paths"));
    }
    Ok(out)
}

fn collect_ae_files(dir: &Path, out: &mut Vec<PathBuf>) -> AeviaResult<()> {
    for entry in std::fs::read_dir(dir).map_err(|e| AeviaError::io(dir, e))? {
        let entry = entry.map_err(|e| AeviaError::io(dir, e))?;
        let path = entry.path();
        if path.is_dir() {
            collect_ae_files(&path, out)?;
        } else if is_ae_file(&path) {
            out.push(path);
        }
    }
    Ok(())
}
