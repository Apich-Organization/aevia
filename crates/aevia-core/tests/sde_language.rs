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

/// The former `sde.noncommutative` gap is closed: the Levy-area method takes any
/// structure, with the Levy-area mode count defaulted when `terms = 0`.
#[test]
fn the_noncommutative_method_runs_on_diagonal_noise() {
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
        terms = 0,
    })
    match sdeint_diag(drift, diffusion, 1.0, opts) with
    | Ok(path) -> println("steps = {path.steps}")
    | Err(e) -> println("recoverable: {e}")
"#;
    let scratch = Scratch::new("noncommutative-diagonal");
    let entry = scratch.write(source);
    let output = run(&entry);
    assert!(
        output.starts_with("steps = 1"),
        "the Levy-area method integrates diagonal noise, got: {output}"
    );
}

// ---------------------------------------------------------------------------
// Non-commutative noise and Stratonovich calculus from source
// ---------------------------------------------------------------------------

/// `dX = A X dt + B1 X dW1 + B2 X dW2` with `A = [[-0.5, 0.2], [0, -0.3]]`, the
/// non-commuting `B1 = [[0.1, 0.15], [0, 0]]`, `B2 = [[0, 0], [0.15, 0.1]]` and
/// `X(0) = (1, 0.5)`. `METHOD` and `SCHEDULE` are substituted.
const NC_PROGRAM: &str = r#"module NcSde

import Sde { SdeMethod, SdeStructure, SdeOpts, Fixed, Adaptive, GeneralNoncommutative, EulerMaruyama, Milstein, MilsteinStratonovich, HeunStratonovich, SrkPlaten, MilsteinNoncommutative, MilsteinNoncommutativeStratonovich, sdeint_general }

fn drift(t: Float[T], x: Tensor<1, Float[1]>): Tensor<1, Float[1/T]> =
    [(-0.5 * x[0] + 0.2 * x[1]) / (1.0 s), (-0.3 * x[1]) / (1.0 s)]

fn diffusion(t: Float[T], x: Tensor<1, Float[1]>): Tensor<2, Float[1/T^(1/2)]> =
    [[(0.1 * x[0] + 0.15 * x[1]) / sqrt(1.0 s), 0.0 / sqrt(1.0 s)],
     [0.0 / sqrt(1.0 s), (0.15 * x[0] + 0.1 * x[1]) / sqrt(1.0 s)]]

fn main(): IO Unit = do
    let opts = SCHEDULE
    match sdeint_general(drift, diffusion, [1.0, 0.5], opts) with
    | Ok(path) -> do
        let last = path.y[path.steps]
        println("{path.steps} {path.noise} {last[0]} {last[1]}")
    | Err(e) -> println("error = {e}")
"#;

fn fixed_schedule(
    method: &str,
    seed: u64,
) -> String {
    format!(
        "Fixed({{ structure = GeneralNoncommutative(2), method = {method}, seed = {seed}, t0 = 0.0 s, dt = 0.05 s, steps = 20, terms = 0 }})"
    )
}

fn adaptive_schedule(
    method: &str,
    seed: u64,
) -> String {
    format!(
        "Adaptive({{ structure = GeneralNoncommutative(2), method = {method}, seed = {seed}, t0 = 0.0 s, t1 = 1.0 s, rtol = 1e-3, atol = 1e-6, max_steps = 100000, cells = 0, terms = 0 }})"
    )
}

/// Runs the non-commutative program with a schedule and returns
/// `(steps, noise, x0, x1)` of its output line.
fn run_nc(
    scratch: &Scratch,
    schedule: &str,
) -> (usize, usize, f64, f64) {
    let entry = scratch.write(&NC_PROGRAM.replace("SCHEDULE", schedule));
    let output = run(&entry);
    let fields: Vec<&str> = output.split_whitespace().collect();
    assert!(
        fields.len() == 4,
        "unexpected output for `{schedule}`: {output}"
    );
    (
        fields[0].parse().expect("steps"),
        fields[1].parse().expect("noise"),
        fields[2].parse().expect("x0"),
        fields[3].parse().expect("x1"),
    )
}

const ALL_METHODS: [&str; 7] = [
    "EulerMaruyama",
    "Milstein",
    "MilsteinStratonovich",
    "HeunStratonovich",
    "SrkPlaten",
    "MilsteinNoncommutative",
    "MilsteinNoncommutativeStratonovich",
];

/// Every method integrates non-commutative noise with fixed steps (no `E1001`), is
/// reproducible with its seed and depends on the seed.
#[test]
fn every_method_integrates_noncommutative_noise_with_fixed_steps() {
    let scratch = Scratch::new("nc-fixed");
    for method in ALL_METHODS {
        let first = run_nc(&scratch, &fixed_schedule(method, 11));
        let again = run_nc(&scratch, &fixed_schedule(method, 11));
        let other = run_nc(&scratch, &fixed_schedule(method, 12));
        assert_eq!((first.0, first.1), (20, 2), "{method}");
        assert!(first.2.is_finite() && first.3.is_finite(), "{method}");
        assert_eq!(first, again, "{method}: a fixed seed reproduces the path");
        assert_ne!(first, other, "{method}: the seed changes the path");
    }
}

/// The adaptive controllers run for the Ito and the Stratonovich methods on
/// non-commutative noise (Stratonovich by exact Ito conversion), reproducibly.
#[test]
fn adaptive_integration_handles_stratonovich_and_noncommutative_noise() {
    let scratch = Scratch::new("nc-adaptive");
    for method in [
        "Milstein",
        "MilsteinStratonovich",
        "HeunStratonovich",
        "SrkPlaten",
        "MilsteinNoncommutativeStratonovich",
    ] {
        let first = run_nc(&scratch, &adaptive_schedule(method, 5));
        let again = run_nc(&scratch, &adaptive_schedule(method, 5));
        assert!(first.0 > 0 && first.1 == 2, "{method}: {first:?}");
        assert!(first.2.is_finite() && first.3.is_finite(), "{method}");
        assert_eq!(first, again, "{method}: a fixed seed reproduces the path");
    }
}

/// The mean of a linear SDE solves `m' = (A + c sum_j B_j^2) m` with `c = 0` for Ito and
/// `c = 1/2` for Stratonovich: the ensemble mean of each calculus matches its own mean ODE
/// within a few standard errors.
fn linear_mean(stratonovich: bool) -> [f64; 2] {
    let a = [[-0.5, 0.2], [0.0, -0.3]];
    let b1 = [[0.1, 0.15], [0.0, 0.0]];
    let b2 = [[0.0, 0.0], [0.15, 0.1]];
    let mul = |x: [[f64; 2]; 2], y: [[f64; 2]; 2]| {
        let mut out = [[0.0; 2]; 2];
        for i in 0..2 {
            for j in 0..2 {
                out[i][j] = x[i][0] * y[0][j] + x[i][1] * y[1][j];
            }
        }
        out
    };
    let (q1, q2) = (mul(b1, b1), mul(b2, b2));
    let c = if stratonovich { 0.5 } else { 0.0 };
    let mut m = [[0.0; 2]; 2];
    for i in 0..2 {
        for j in 0..2 {
            m[i][j] = a[i][j] + c * (q1[i][j] + q2[i][j]);
        }
    }
    let f = |x: [f64; 2]| {
        [
            m[0][0] * x[0] + m[0][1] * x[1],
            m[1][0] * x[0] + m[1][1] * x[1],
        ]
    };
    let mut x = [1.0, 0.5];
    let h = 1e-3;
    for _ in 0..1000 {
        let k1 = f(x);
        let k2 = f([x[0] + 0.5 * h * k1[0], x[1] + 0.5 * h * k1[1]]);
        let k3 = f([x[0] + 0.5 * h * k2[0], x[1] + 0.5 * h * k2[1]]);
        let k4 = f([x[0] + h * k3[0], x[1] + h * k3[1]]);
        for i in 0..2 {
            x[i] += h / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]);
        }
    }
    x
}

#[test]
fn ensemble_means_match_the_ito_and_stratonovich_mean_equations() {
    let scratch = Scratch::new("nc-mean");
    let paths = 40_u64;
    for (method, stratonovich, adaptive) in [
        ("Milstein", false, false),
        ("SrkPlaten", false, false),
        ("MilsteinStratonovich", true, false),
        ("MilsteinStratonovich", true, true),
    ] {
        let mut sums = [0.0_f64; 2];
        let mut squares = [0.0_f64; 2];
        for seed in 0..paths {
            let schedule = if adaptive {
                adaptive_schedule(method, 100 + seed)
            } else {
                fixed_schedule(method, 100 + seed)
            };
            let (_, _, x0, x1) = run_nc(&scratch, &schedule);
            for (k, x) in [x0, x1].into_iter().enumerate() {
                sums[k] += x;
                squares[k] += x * x;
            }
        }
        let n = paths as f64;
        let expected = linear_mean(stratonovich);
        for k in 0..2 {
            let mean = sums[k] / n;
            let se = ((squares[k] / n - mean * mean).max(1e-12) / n).sqrt();
            // the O(dt) bias of the scheme is far below the standard error here
            assert!(
                (mean - expected[k]).abs() < 4.5 * se + 5e-3,
                "{method} (adaptive: {adaptive}) component {k}: mean {mean} vs {} (se {se})",
                expected[k]
            );
        }
    }
}

/// A record state with general noise: the diffusion is a record of the same shape with a
/// noise axis on every field (`GeneralDiffusionOf<s>`); the former `sde.general.record`
/// gap is closed.
#[test]
fn general_noise_with_a_record_state_runs_from_source() {
    let source = r#"module RecordNoise

import Sde { SdeMethod, SdeStructure, SdeOpts, Fixed, Adaptive, GeneralNoncommutative, Milstein, MilsteinStratonovich, sdeint_general }

fn drift(t: Float[T], s: { x: Float[1], v: Float[1] }): RateOf<{ x: Float[1], v: Float[1] }> =
    { x = s.v / (1.0 s), v = (-s.x) / (1.0 s) }

fn diffusion(t: Float[T], s: { x: Float[1], v: Float[1] }): { x: Tensor<1, Float[1/T^(1/2)]>, v: Tensor<1, Float[1/T^(1/2)]> } =
    { x = [0.1 * s.v / sqrt(1.0 s), 0.0 / sqrt(1.0 s)], v = [0.0 / sqrt(1.0 s), 0.1 * s.x / sqrt(1.0 s)] }

fn main(): IO Unit = do
    let init = { x = 1.0, v = 0.0 }
    let fixed = Fixed({ structure = GeneralNoncommutative(2), method = Milstein, seed = 4, t0 = 0.0 s, dt = 0.05 s, steps = 20, terms = 0 })
    let adaptive = Adaptive({ structure = GeneralNoncommutative(2), method = MilsteinStratonovich, seed = 4, t0 = 0.0 s, t1 = 1.0 s, rtol = 1e-3, atol = 1e-6, max_steps = 100000, cells = 0, terms = 0 })
    match sdeint_general(drift, diffusion, init, fixed) with
    | Ok(path) -> println("fixed {path.steps} {path.noise} {path.y[path.steps].x}")
    | Err(e) -> println("error = {e}")
    match sdeint_general(drift, diffusion, init, adaptive) with
    | Ok(path) -> println("adaptive {path.noise} {path.y[path.steps].x}")
    | Err(e) -> println("error = {e}")
"#;
    let scratch = Scratch::new("record-general");
    let entry = scratch.write(source);
    let output = run(&entry);
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 2, "{output}");
    assert!(lines[0].starts_with("fixed 20 2 "), "{output}");
    assert!(lines[1].starts_with("adaptive 2 "), "{output}");
    assert_eq!(output, run(&entry), "reproducible");
    // oscillator x'' = -x over t = 1 with small noise: x(1) is near cos(1)
    let x: f64 = lines[0].rsplit(' ').next().unwrap().parse().unwrap();
    assert!((x - 1.0_f64.cos()).abs() < 0.15, "{output}");
}
