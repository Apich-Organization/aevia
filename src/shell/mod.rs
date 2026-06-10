//! Interactive REPL for expressions and quick checks. Phase 5.

use crate::diagnostics::{AeviaError, AeviaResult};
use crate::parser::items::parse_source;
use std::io::{self, Write};
use std::path::PathBuf;

/// Run the interactive Aevia shell on stdin/stdout.
pub fn run() -> AeviaResult<()> {
    println!("Aevia shell — enter expressions or commands (:help, :quit)");
    let stdin = io::stdin();
    loop {
        print!("aevia> ");
        io::stdout().flush().map_err(|e| AeviaError::message(e.to_string()))?;
        let mut line = String::new();
        let n = stdin.read_line(&mut line).map_err(|e| AeviaError::message(e.to_string()))?;
        if n == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if matches!(line, ":quit" | ":exit" | ":q") {
            break;
        }
        let _ = handle_line(line);
    }
    Ok(())
}

fn handle_line(line: &str) -> AeviaResult<()> {
    match line {
        ":help" | ":h" => {
            print_help();
            Ok(())
        }
        cmd if cmd.starts_with(":check ") => {
            let path = PathBuf::from(cmd.trim_start_matches(":check ").trim());
            shell_check_file(&path)
        }
        cmd if cmd.starts_with(":load ") => {
            let path = PathBuf::from(cmd.trim_start_matches(":load ").trim());
            shell_load_file(&path)
        }
        _ => eval_expression(line),
    }
}

fn print_help() {
    println!(
        r#"Commands:
  <expr>           Evaluate an expression (e.g. 0.5 * 2.0 * 3.0^2)
  :check <file.ae> Type-check a source file
  :load <file.ae>  Load, type-check, compile, and JIT-run the main function
  :help            Show this message
  :quit            Exit"#
    );
}

fn shell_check_file(path: &PathBuf) -> AeviaResult<()> {
    let program = crate::modules::load_program(path)?;
    let ast = crate::modules::entry_ast(&program);
    let imports = crate::modules::entry_imports(&program);
    let result = crate::types::checker::check_with_imports(ast, imports);
    if result.ok() {
        println!("ok: {} passed dimensional check", path.display());
        Ok(())
    } else {
        for err in &result.errors {
            eprintln!("error: {err}");
        }
        Err(AeviaError::message(format!("check failed for {}", path.display())))
    }
}

fn shell_load_file(path: &PathBuf) -> AeviaResult<()> {
    let program = crate::modules::load_program(path)?;
    let mut ast_owned = crate::modules::entry_ast(&program).clone();
    crate::macro_expand::expand(&mut ast_owned)?;
    let ast = &ast_owned;
    let imports = crate::modules::entry_imports(&program);
    let result = crate::types::checker::check_with_imports(ast, imports);
    if !result.ok() {
        for err in &result.errors {
            eprintln!("error: {err}");
        }
        return Err(AeviaError::message(format!("check failed for {}", path.display())));
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
    for item in &ast.items {
        if let crate::ast::Item::Function { name, .. } = &item.node {
            if name == "main" || main_func.is_none() {
                main_func = Some(item.node.clone());
            }
        }
    }

    if let Some(crate::ast::Item::Function {
        name,
        params,
        body,
        attributes,
        ..
    }) = main_func
    {
        let mut env = std::collections::HashMap::new();
        for param in &params {
            let id = lowerer.builder.variable(&param.name);
            env.insert(param.name.clone(), id);
        }

        let root = match &body {
            crate::ast::FunctionBody::Expression(expr) => lowerer.lower_expr(expr, &mut env)?,
            crate::ast::FunctionBody::Block(stmts) => lowerer.lower_block(stmts, &mut env)?,
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

        let ast_proj = rssn_advanced::ast::convert::dag_to_ast(lowerer.builder.arena(), root);
        op_reg.apply_to_jit(&mut compiler);

        let compiled_fn = compiler.compile(&ast_proj).map_err(|e| {
            AeviaError::message(format!("JIT compilation failed: {:?}", e))
        })?;

        // Call with 1.0 arguments as defaults
        let args = vec![1.0; params.len()];
        let res = compiled_fn(args.as_ptr());

        println!("Result of calling `{}`: {}", name, res);
        Ok(())
    } else {
        println!("ok: {} passed check (no main function found to run)", path.display());
        Ok(())
    }
}

fn eval_expression(line: &str) -> AeviaResult<()> {
    let src = if line.contains("fn ") {
        if line.ends_with(';') {
            line.to_string()
        } else {
            format!("{line};")
        }
    } else {
        format!("fn __repl() := {line};")
    };

    let file = parse_source(&src)
        .map_err(|e| AeviaError::message(format!("parse error: {e}")))?;
    let result = crate::types::checker::check(&file);
    if !result.ok() {
        for err in &result.errors {
            eprintln!("error: {err}");
        }
        return Err(AeviaError::message("expression failed type check"));
    }

    let mut lowerer = crate::lowering::Lowerer::new();
    lowerer.register_items(&file);

    let item = file
        .items
        .iter()
        .find(|i| matches!(i.node, crate::ast::Item::Function { .. }))
        .ok_or_else(|| AeviaError::message("no function to evaluate"))?;

    let (params, body) = match &item.node {
        crate::ast::Item::Function { params, body, .. } => (params, body),
        _ => return Err(AeviaError::message("expected function")),
    };

    let mut env = std::collections::HashMap::new();
    for param in params {
        let id = lowerer.builder.variable(&param.name);
        env.insert(param.name.clone(), id);
    }

    let root = match body {
        crate::ast::FunctionBody::Expression(expr) => lowerer.lower_expr(expr, &mut env)?,
        crate::ast::FunctionBody::Block(stmts) => lowerer.lower_block(stmts, &mut env)?,
    };

    let mut compiler = rssn_advanced::jit::compiler::JitCompiler::try_new().map_err(|e| {
        AeviaError::message(format!("JIT init failed: {:?}", e))
    })?;

    let ast_proj = rssn_advanced::ast::convert::dag_to_ast(lowerer.builder.arena(), root);
    let compiled = compiler
        .compile(&ast_proj)
        .map_err(|e| AeviaError::message(format!("JIT compile failed: {:?}", e)))?;

    let args = vec![1.0; params.len()];
    let value = compiled(args.as_ptr());
    println!("{value}");
    Ok(())
}
