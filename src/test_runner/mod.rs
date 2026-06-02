//! Project test harness for `.ae` files under `tests/`. Phase 5.

use crate::diagnostics::{AeviaError, AeviaResult};
use crate::types::checker;
use std::path::{Path, PathBuf};

/// Test directive parsed from `// @aevia-test: <mode>` at the top of a file.
#[derive(Debug, Clone, PartialEq)]
pub enum TestMode {
    /// File must parse and pass dimensional checking (default).
    Check,
    /// File must fail dimensional checking.
    ExpectError,
    /// File must compile and run via JIT; no result assertion.
    Run,
    /// File must run and return the given value (approximate).
    RunEquals(f64),
}

/// Outcome of running one test file.
#[derive(Debug)]
pub struct TestResult {
    pub path: PathBuf,
    pub passed: bool,
    pub message: String,
}

/// Run all `.ae` tests under `project_root/tests/`.
pub fn run_project(project_root: &Path) -> AeviaResult<Vec<TestResult>> {
    let tests_dir = project_root.join("tests");
    if !tests_dir.is_dir() {
        return Err(AeviaError::message(format!(
            "no tests/ directory in {}",
            project_root.display()
        )));
    }

    let mut files = Vec::new();
    collect_tests(&tests_dir, &mut files)?;
    if files.is_empty() {
        return Err(AeviaError::message(format!(
            "no .ae test files in {}",
            tests_dir.display()
        )));
    }

    let mut results = Vec::new();
    for path in files {
        results.push(run_test_file(&path)?);
    }
    Ok(results)
}

fn collect_tests(dir: &Path, out: &mut Vec<PathBuf>) -> AeviaResult<()> {
    for entry in std::fs::read_dir(dir).map_err(|e| AeviaError::io(dir, e))? {
        let entry = entry.map_err(|e| AeviaError::io(dir, e))?;
        let path = entry.path();
        if path.is_dir() {
            collect_tests(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "ae") {
            out.push(path);
        }
    }
    Ok(())
}

fn run_test_file(path: &Path) -> AeviaResult<TestResult> {
    let text = std::fs::read_to_string(path).map_err(|e| AeviaError::io(path, e))?;
    let mode = parse_test_mode(&text);

    match mode {
        TestMode::Check => run_check_test(path, false),
        TestMode::ExpectError => run_check_test(path, true),
        TestMode::Run => run_jit_test(path, None),
        TestMode::RunEquals(expected) => run_jit_test(path, Some(expected)),
    }
}

fn parse_test_mode(source: &str) -> TestMode {
    for line in source.lines().take(20) {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("// @aevia-test:") else {
            continue;
        };
        let mode = rest.trim();
        if let Some(val) = mode.strip_prefix("run-equals ") {
            return TestMode::RunEquals(val.trim().parse().unwrap_or(0.0));
        }
        return match mode {
            "check" => TestMode::Check,
            "error" | "expect-error" => TestMode::ExpectError,
            "run" => TestMode::Run,
            _ => TestMode::Check,
        };
    }
    TestMode::Check
}

fn run_check_test(path: &Path, expect_fail: bool) -> AeviaResult<TestResult> {
    let program = crate::modules::load_program(path)?;
    let ast = crate::modules::entry_ast(&program);
    let imports = crate::modules::entry_imports(&program);
    let check = checker::check_with_imports(ast, imports);
    let failed = !check.ok();
    let passed = failed == expect_fail;
    let message = if passed {
        if expect_fail {
            "failed as expected".to_string()
        } else {
            "check passed".to_string()
        }
    } else if expect_fail {
        "expected type error but check passed".to_string()
    } else {
        check
            .errors
            .first()
            .map(|e| e.message.clone())
            .unwrap_or_else(|| "check failed".to_string())
    };
    Ok(TestResult {
        path: path.to_path_buf(),
        passed,
        message,
    })
}

fn run_jit_test(path: &Path, expected: Option<f64>) -> AeviaResult<TestResult> {
    let value = eval_main(path)?;

    if let Some(exp) = expected {
        let passed = (value - exp).abs() < 1e-9;
        let message = if passed {
            format!("run-equals {exp}")
        } else {
            format!("expected {exp}, got {value}")
        };
        return Ok(TestResult {
            path: path.to_path_buf(),
            passed,
            message,
        });
    }

    Ok(TestResult {
        path: path.to_path_buf(),
        passed: true,
        message: format!("run succeeded (= {value})"),
    })
}

fn eval_main(path: &Path) -> AeviaResult<f64> {
    use crate::ast::{FunctionBody, Item};
    use std::collections::HashMap;

    let program = crate::modules::load_program(path)?;
    let parsed = crate::modules::entry_ast(&program);
    let imports = crate::modules::entry_imports(&program);
    let check = checker::check_with_imports(parsed, imports);
    if !check.ok() {
        return Err(AeviaError::message("type check failed"));
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

    let main = parsed
        .items
        .iter()
        .find(|i| matches!(&i.node, Item::Function { name, .. } if name == "main"))
        .ok_or_else(|| AeviaError::message("no main function"))?;

    let Item::Function {
        params,
        body,
        attributes,
        ..
    } = &main.node
    else {
        return Err(AeviaError::message("expected main"));
    };

    let mut env = HashMap::new();
    for param in params {
        let id = lowerer.builder.variable(&param.name);
        env.insert(param.name.clone(), id);
    }

    let root = match body {
        FunctionBody::Expression(expr) => lowerer.lower_expr(expr, &mut env)?,
        FunctionBody::Block(stmts) => lowerer.lower_block(stmts, &mut env)?,
    };
    let fusion_cfg = crate::fusion::FusionConfig::from_function(attributes, body);
    let pipeline = crate::pipeline::optimize_with_fusion(
        &mut lowerer.builder,
        root,
        &op_reg,
        crate::pipeline::OptimizeConfig::default(),
        fusion_cfg,
    );
    let root = pipeline.root;

    let mut compiler = rssn_advanced::jit::compiler::JitCompiler::try_new().map_err(|e| {
        AeviaError::message(format!("JIT init: {:?}", e))
    })?;
    let op_reg = std::sync::Arc::new(op_reg);
    op_reg.apply_to_jit(&mut compiler);

    let ast_proj = rssn_advanced::ast::convert::dag_to_ast(lowerer.builder.arena(), root);
    let compiled = compiler
        .compile(&ast_proj)
        .map_err(|e| AeviaError::message(format!("JIT: {:?}", e)))?;
    let args = vec![1.0; params.len()];
    Ok(compiled(args.as_ptr()))
}
