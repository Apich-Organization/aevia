//! CLI command implementations.

use crate::diagnostics::{AeviaError, AeviaResult};
use crate::logging;
use crate::project;
use crate::ast::{Item, FunctionBody};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct RunTarget {
    pub file: PathBuf,
    pub args: Vec<f64>,
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
        let manifest = crate::project::load_manifest_for(&file).ok();
        if let Some(m) = &manifest {
            for (name, spec) in &m.kernels {
                logging::pass_detail("build", &format!("loaded precompiled kernel `{}` from {}", name, spec.path));
            }
        }

        let program = crate::modules::load_program(&file)?;
        let mut ast = crate::modules::entry_ast(&program).clone();
        crate::macro_expand::expand(&mut ast)?;
        let parsed = &ast;
        let imports = crate::modules::entry_imports(&program);

        let check_res = crate::types::checker::check_with_imports(parsed, imports);
        if !check_res.ok() {
            let err_msg = check_res.errors.iter()
                .map(|e| format!("{}", e))
                .collect::<Vec<_>>()
                .join("\n");
            return Err(AeviaError::message(format!("type check failed in {}:\n{}", file.display(), err_msg)));
        }

        let mut lowerer = crate::lowering::Lowerer::new();
        let mut op_reg = rssn_advanced::custom::descriptor::CustomOpRegistry::new();
        for module in crate::modules::modules(&program) {
            let reg = crate::ops::registry_from_items(&module.ast.items, &mut lowerer.builder)?;
            for desc in reg.ops_iter() {
                let _ = op_reg.register(desc.clone());
            }
        }
        op_reg.register_with_builder(&mut lowerer.builder);
        crate::modules::register_lowerer(&program, &mut lowerer);
        let mut main_func = None;
        for item in &parsed.items {
            if let Item::Function { name, .. } = &item.node {
                if name == "main" || main_func.is_none() {
                    main_func = Some(item.node.clone());
                }
            }
        }

        if let Some(Item::Function {
            name,
            params,
            body,
            attributes,
            ..
        }) = main_func
        {
            let mut env = HashMap::new();
            for param in &params {
                let id = lowerer.builder.variable(&param.name);
                env.insert(param.name.clone(), id);
            }

            let root = match &body {
                FunctionBody::Expression(expr) => lowerer.lower_expr(expr, &mut env)?,
                FunctionBody::Block(stmts) => lowerer.lower_block(stmts, &mut env)?,
            };
            let fusion_cfg =
                crate::fusion::FusionConfig::from_function(&attributes, &body);
            let pipeline = crate::pipeline::optimize_with_fusion(
                &mut lowerer.builder,
                root,
                &op_reg,
                crate::pipeline::OptimizeConfig::default(),
                fusion_cfg,
            );
            if let Some(stats) = pipeline.egraph {
                logging::pass_detail(
                    "build",
                    &format!(
                        "e-graph: {} merges in {} rounds (converged={})",
                        stats.merges_performed, stats.rounds_completed, stats.converged
                    ),
                );
            }
            logging::pass_detail(
                "build",
                &format!(
                    "fusion: {} kernels ({} arithmetic, {} DAG nodes)",
                    pipeline.fusion.kernels.len(),
                    pipeline.fusion.arithmetic_kernel_count(),
                    pipeline.fusion.total_nodes()
                ),
            );
            let _root = pipeline.root;

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
    let RunTarget { file, args: user_args } = target;
    let file = file.canonicalize().map_err(|e| AeviaError::io(&file, e))?;
    if !is_ae_file(&file) {
        return Err(AeviaError::message(format!(
            "expected a .ae file, got {}",
            file.display()
        )));
    }
    let manifest = project::load_manifest_for(&file).ok();
    if let Some(ref m) = manifest {
        for (name, spec) in &m.kernels {
            logging::pass_detail("run", &format!("loaded precompiled kernel `{}` from {}", name, spec.path));
        }
    }
    
    let program = crate::modules::load_program(&file)?;
    let mut ast = crate::modules::entry_ast(&program).clone();
    crate::macro_expand::expand(&mut ast)?;
    let parsed = &ast;
    let imports = crate::modules::entry_imports(&program);

    let check_res = crate::types::checker::check_with_imports(parsed, imports);
    if !check_res.ok() {
        let err_msg = check_res.errors.iter()
            .map(|e| format!("{}", e))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(AeviaError::message(format!("type check failed in {}:\n{}", file.display(), err_msg)));
    }

    let mut lowerer = crate::lowering::Lowerer::new();
    let mut op_reg = rssn_advanced::custom::descriptor::CustomOpRegistry::new();
    for module in crate::modules::modules(&program) {
        let reg = crate::ops::registry_from_items(&module.ast.items, &mut lowerer.builder)?;
        for desc in reg.ops_iter() {
            let _ = op_reg.register(desc.clone());
        }
    }
    op_reg.register_with_builder(&mut lowerer.builder);
    crate::modules::register_lowerer(&program, &mut lowerer);
    let op_reg = std::sync::Arc::new(op_reg);

    let mut main_func = None;
    for item in &parsed.items {
        if let Item::Function { name, .. } = &item.node {
            if name == "main" || main_func.is_none() {
                main_func = Some(item.node.clone());
            }
        }
    }

    if let Some(Item::Function {
        name,
        params,
        body,
        attributes,
        ..
    }) = main_func
    {
        let mut env = HashMap::new();
        for param in &params {
            let id = lowerer.builder.variable(&param.name);
            env.insert(param.name.clone(), id);
        }

        let root = match &body {
            FunctionBody::Expression(expr) => lowerer.lower_expr(expr, &mut env)?,
            FunctionBody::Block(stmts) => lowerer.lower_block(stmts, &mut env)?,
        };
        let fusion_cfg = crate::fusion::FusionConfig::from_function(&attributes, &body);
        let pipeline = crate::pipeline::optimize_with_fusion(
            &mut lowerer.builder,
            root,
            &op_reg,
            crate::pipeline::OptimizeConfig::default(),
            fusion_cfg,
        );
        let root = pipeline.root;

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

        if let Some(ref m) = manifest {
            for (name, _) in &m.kernels {
                if let Some(func_id) = lowerer.builder.lookup_function(name) {
                    extern "C" fn kernel_stub(x: f64) -> f64 { x }
                    compiler.register_custom_function(func_id, kernel_stub);
                    logging::pass_detail("run", &format!("registered JIT function stub for kernel `{}`", name));
                }
            }
        }

        let ast_proj = rssn_advanced::ast::convert::dag_to_ast(lowerer.builder.arena(), root);
        op_reg.apply_to_jit(&mut compiler);

        if fusion_cfg.jit_kernel && pipeline.fusion.arithmetic_kernel_count() == 1 {
            if let Ok(Some(_batch)) = compiler.compile_batch_f64x2(&ast_proj) {
                logging::pass_detail(
                    "run",
                    &format!("`{}`: vector batch kernel available (f64x2)", name),
                );
            }
        }

        let compiled_fn = compiler.compile(&ast_proj).map_err(|e| {
            AeviaError::message(format!("JIT compilation failed: {:?}", e))
        })?;

        let args = if user_args.is_empty() {
            vec![1.0; params.len()]
        } else {
            if user_args.len() != params.len() {
                return Err(AeviaError::message(format!(
                    "arity mismatch: expected {} arguments for function `{}`, but got {}",
                    params.len(),
                    name,
                    user_args.len()
                )));
            }
            user_args
        };
        let res = compiled_fn(args.as_ptr());

        logging::pass_detail(
            "run",
            &format!(
                "JIT `{}` ok ({} fusion kernels)",
                name,
                pipeline.fusion.kernels.len()
            ),
        );
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
        let program = crate::modules::load_program(&file)?;
        let mut ast = crate::modules::entry_ast(&program).clone();
        crate::macro_expand::expand(&mut ast)?;
        let parsed = &ast;
        let imports = crate::modules::entry_imports(&program);

        let check_res = crate::types::checker::check_with_imports(parsed, imports);
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
    let files = resolve_inputs(&paths)?;
    let mut changed = 0usize;
    for file in &files {
        if crate::fmt::format_file_in_place(file)? {
            changed += 1;
            logging::pass_detail("fmt", &format!("reformatted {}", file.display()));
        }
    }
    logging::pass_detail(
        "fmt",
        &format!("{}/{} files updated", changed, files.len()),
    );
    Ok(())
}

/// `aevia lint`
pub fn lint(paths: Vec<PathBuf>) -> AeviaResult<()> {
    logging::pass("lint");
    let files = resolve_inputs(&paths)?;
    let mut report = crate::lint::LintReport::default();
    for file in &files {
        let file_report = crate::lint::lint_file(file)?;
        for lint in file_report.lints {
            report.lints.push(lint);
        }
    }
    emit_lint_report(&report);
    if report.ok() {
        logging::pass_detail("lint", &format!("{} files, no errors", files.len()));
        Ok(())
    } else {
        Err(AeviaError::message(format!(
            "lint failed with {} error(s)",
            report.lints.iter().filter(|l| l.level == crate::lint::LintLevel::Error).count()
        )))
    }
}

/// `aevia doc`
pub fn doc(paths: Vec<PathBuf>, output: Option<PathBuf>) -> AeviaResult<()> {
    logging::pass("doc");
    let files = resolve_inputs(&paths)?;
    for file in files {
        if let Some(ref out_dir) = output {
            crate::doc::generate_for_entry(&file, out_dir)?;
            logging::pass_detail("doc", &format!("wrote docs to {}", out_dir.display()));
        } else {
            let program = crate::modules::load_program(&file)?;
            for module in crate::modules::modules(&program) {
                let md = crate::doc::render_markdown(&module.path, &module.ast);
                print!("{md}");
            }
        }
    }
    Ok(())
}

/// `aevia shell`
pub fn shell() -> AeviaResult<()> {
    logging::pass("shell");
    crate::shell::run()
}

/// `aevia test`
pub fn test(path: &Path) -> AeviaResult<()> {
    logging::pass("test");
    let root = path
        .canonicalize()
        .map_err(|e| AeviaError::io(path, e))?;
    let results = crate::test_runner::run_project(&root)?;
    let passed = results.iter().filter(|r| r.passed).count();
    for result in &results {
        let status = if result.passed { "ok" } else { "FAIL" };
        println!(
            "  [{status}] {} — {}",
            result.path.display(),
            result.message
        );
    }
    logging::pass_detail("test", &format!("{passed}/{} passed", results.len()));
    if passed == results.len() {
        Ok(())
    } else {
        Err(AeviaError::message(format!(
            "{} test(s) failed",
            results.len() - passed
        )))
    }
}

fn emit_lint_report(report: &crate::lint::LintReport) {
    use crate::lint::LintLevel;
    for lint in &report.lints {
        let level = match lint.level {
            LintLevel::Error => "error",
            LintLevel::Warning => "warning",
        };
        eprintln!("{level}: {}: {}", lint.file.display(), lint.message);
    }
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
