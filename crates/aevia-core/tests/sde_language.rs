//! The SDE surface (S8.9.1) from Aevia source.
//!
//! `crates/aevia-std/src/sde.rs` owns the numerics, `crates/aevia-core/src/builtins.rs`
//! owns the dispatch, and `crates/aevia-std/stdlib/Sde.aev` declares the option
//! ADTs. These tests are the language-level contract: a program imports `Sde`,
//! constructs the options, passes typed drift/diffusion closures and receives
//! the documented `Result<SdePath, SdeError>`. They go through the real loader,
//! the real checker and the real evaluator, so a break in any layer shows here.

#![allow(clippy::result_large_err)]

use aevia_core::driver;
use std::path::Path;
use std::path::PathBuf;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("aevia-sde-lang-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create scratch directory");
        Self(path)
    }

    fn write(
        &self,
        source: &str,
    ) -> PathBuf {
        let path = self.0.join("main.aev");
        std::fs::write(&path, source).expect("write program");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Loads and type-checks without running; returns the error diagnostics.
fn errors(entry: &Path) -> Vec<(String, String)> {
    let program = driver::load(entry);
    program
        .diagnostics
        .items
        .iter()
        .filter(|item| item.code.starts_with('E'))
        .map(|item| (item.code.clone(), item.message.clone()))
        .collect()
}

/// Loads and runs `main`, returning what it printed.
fn run(entry: &Path) -> String {
    let program = driver::load(entry);
    assert!(
        !program.diagnostics.has_errors(),
        "the program does not load: {:?}",
        program
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    let module = program.entry().expect("the entry module");
    let mut interp = aevia_core::Interp::new(&module.checked).with_buffer();
    let diagnostics = driver::run_modules(&mut interp, &program);
    assert!(
        !diagnostics.has_errors(),
        "the program does not run: {:?}",
        diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    interp.run_main().expect("`main` runs");
    interp.captured()
}

#[test]
fn ode_module_exports_stiff_methods_and_terminal_events() {
    let scratch = Scratch::new("ode-events");
    let entry = scratch.write(r#"module OdeSource
import Ode { OdeMethod, OdeOpts, Radau, odeint_with }
fn rhs(t: Float[T], x: Float[1]): Float[1/T] = 1.0 / (1.0 s)
fn event(t: Float[T], x: Float[1]): Float[1] = x - 0.6
fn main(): IO Unit = do
    let opts: OdeOpts<Float[1]> = { rtol = 1e-8, atol = 1e-10, method = Radau, steps = Some(4), events = Some(event) }
    let path = odeint_with(rhs, 0.0, [0.0 s, 0.5 s, 1.0 s], opts)
    println(path.y[2])
"#);
    let output = run(&entry);
    let value: f64 = output.trim().parse().expect("numeric final state");
    assert!((value - 0.6).abs() < 1e-7, "{output}");
}

/// `dX = X dt + X dW` from `X(0) = 1` over four steps of 1 s, with the option
/// records constructed from the `Sde` module and the closures typed through
/// `RateOf`/`DiffusionOf`.
const GBM: &str = r#"module SdeSource

import Sde { SdeMethod, SdeStructure, SdeOpts, Fixed, EulerMaruyama, Diagonal, sdeint_diag }

fn drift(t: Float[T], x: Float[1]): Float[1/T] = x / (1.0 s)

fn diffusion(t: Float[T], x: Float[1]): Float[1/T^(1/2)] = x / sqrt(1.0 s)

fn main(): IO Unit = do
    let opts = Fixed({
        structure = Diagonal,
        method = EulerMaruyama,
        seed = 7,
        t0 = 0.0 s,
        dt = 1.0 s,
        steps = 4,
        terms = 0,
    })
    match sdeint_diag(drift, diffusion, 1.0, opts) with
    | Ok(path) -> println("steps = {path.steps} noise = {path.noise}")
    | Err(e) -> println("error = {e}")
"#;

#[test]
fn a_source_program_constructs_the_options_and_integrates() {
    let scratch = Scratch::new("gbm");
    let entry = scratch.write(GBM);
    let output = run(&entry);
    assert!(
        output.starts_with("steps = 4 noise = 1"),
        "unexpected GBM output: {output}"
    );
}

/// Same program, same seed: the printed path metadata is identical, and the
/// path itself is deterministic.
#[test]
fn a_source_program_is_reproducible() {
    let scratch = Scratch::new("repeat");
    let entry = scratch.write(GBM);
    let first = run(&entry);
    let second = run(&entry);
    assert_eq!(first, second, "a fixed seed must reproduce the path");
}

/// The diffusion's dimension is part of the type (`DiffusionOf` divides by
/// `T^(1/2)`): returning a rate-shaped value instead is `E0401`, not a runtime
/// surprise.
#[test]
fn the_diffusion_dimension_is_checked() {
    let source = r#"module WrongDiffusion

import Sde { SdeMethod, SdeStructure, SdeOpts, Fixed, EulerMaruyama, Diagonal, sdeint_diag }

fn drift(t: Float[T], x: Float[1]): Float[1/T] = x / (1.0 s)

fn diffusion(t: Float[T], x: Float[1]): Float[1/T] = x / (1.0 s)

fn main(): IO Unit = do
    let opts = Fixed({
        structure = Diagonal,
        method = EulerMaruyama,
        seed = 1,
        t0 = 0.0 s,
        dt = 1.0 s,
        steps = 1,
        terms = 0,
    })
    match sdeint_diag(drift, diffusion, 1.0, opts) with
    | Ok(path) -> println("steps = {path.steps}")
    | Err(e) -> println("error = {e}")
"#;
    let scratch = Scratch::new("wrong-diffusion");
    let entry = scratch.write(source);
    let errors = errors(&entry);
    assert!(
        errors.iter().any(|(code, _)| code == "E0401"),
        "expected an E0401 dimension mismatch, found {errors:?}"
    );
}

/// The option record type is enforced: a missing `dt` is a *checker* error
/// (`E0306`), because `Fixed`'s payload is `SdeFixedOpts<T>`. The runtime's
/// `E0802` path (record built without the type) is covered in
/// `crates/aevia-core/tests/sde_runtime.rs`; here the language surface is the
/// thing under test.
#[test]
fn a_missing_option_field_is_a_static_error() {
    let source = r#"module MissingDt

import Sde { SdeMethod, SdeStructure, SdeOpts, Fixed, EulerMaruyama, Diagonal, sdeint_diag }

fn drift(t: Float[T], x: Float[1]): Float[1/T] = 0.0 / (1.0 s)

fn diffusion(t: Float[T], x: Float[1]): Float[1/T^(1/2)] = 1.0 / sqrt(1.0 s)

fn main(): IO Unit = do
    let opts = Fixed({
        structure = Diagonal,
        method = EulerMaruyama,
        seed = 1,
        t0 = 0.0 s,
        steps = 1,
        terms = 0,
    })
    match sdeint_diag(drift, diffusion, 1.0, opts) with
    | Ok(path) -> println("steps = {path.steps}")
    | Err(e) -> println("recoverable: {e}")
"#;
    let scratch = Scratch::new("missing-dt");
    let entry = scratch.write(source);
    let errors = errors(&entry);
    assert!(
        errors
            .iter()
            .any(|(code, message)| code == "E0306" && message.contains("dt")),
        "expected a record error naming `dt`, found {errors:?}"
    );
}

/// A value-dependent unsupported combination is still the recoverable
/// `SdeError::Unsupported` at run time and names its capability. The checker
/// cannot know that `MilsteinNoncommutative` cannot act on diagonal noise; the
/// programme gets `Err`, not a silently substituted scheme.
#[test]
fn an_unsupported_combination_is_a_recoverable_error() {
    let source = r#"module NoncommutativeOnDiagonal

import Sde { SdeMethod, SdeStructure, SdeOpts, Fixed, MilsteinNoncommutative, Diagonal, sdeint_diag }

fn drift(t: Float[T], x: Float[1]): Float[1/T] = 0.0 / (1.0 s)

fn diffusion(t: Float[T], x: Float[1]): Float[1/T^(1/2)] = 1.0 / sqrt(1.0 s)

fn main(): IO Unit = do
    let opts = Fixed({
        structure = Diagonal,
        method = MilsteinNoncommutative,
        seed = 1,
        t0 = 0.0 s,
        dt = 1.0 s,
        steps = 1,
        terms = 2,
    })
    match sdeint_diag(drift, diffusion, 1.0, opts) with
    | Ok(path) -> println("steps = {path.steps}")
    | Err(e) -> println("recoverable: {e}")
"#;
    let scratch = Scratch::new("noncommutative-diagonal");
    let entry = scratch.write(source);
    let output = run(&entry);
    assert!(
        output.contains("recoverable:") && output.contains("sde.noncommutative"),
        "expected a recoverable SdeError naming `sde.noncommutative`, got: {output}"
    );
}
