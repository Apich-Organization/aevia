//! Track 3 numerics from Aevia source: ODE solutions with dense output and
//! events, higher-order equations with dimension tracking, boundary-value
//! problems, SDE continuous output / events / ensembles, GMRES / BiCGSTAB and
//! the parallel sparse product.

#![allow(clippy::result_large_err)]

use aevia_core::driver;
use std::path::Path;
use std::path::PathBuf;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("aevia-t3-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch");
        Self(path)
    }

    fn write(
        &self,
        source: &str,
    ) -> PathBuf {
        let path = self.0.join("main.aev");
        std::fs::write(&path, source).expect("write");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn errors(entry: &Path) -> Vec<(String, String)> {
    driver::load(entry)
        .diagnostics
        .items
        .iter()
        .filter(|item| item.code.starts_with('E'))
        .map(|item| (item.code.clone(), item.message.clone()))
        .collect()
}

fn run(
    source: &str,
    name: &str,
) -> String {
    let scratch = Scratch::new(name);
    let entry = scratch.write(source);
    let program = driver::load(&entry);
    assert!(
        !program.diagnostics.has_errors(),
        "does not load: {:?}",
        program
            .diagnostics
            .items
            .iter()
            .map(|item| (&item.code, &item.message))
            .collect::<Vec<_>>()
    );
    let module = program.entry().expect("entry");
    let mut interp = aevia_core::Interp::new(&module.checked).with_buffer();
    let diagnostics = driver::run_modules(&mut interp, &program);
    assert!(!diagnostics.has_errors(), "does not run");
    if let Err(error) = interp.run_main() {
        panic!("main failed: {} {}", error.code, error.message);
    }
    interp.captured()
}

fn check_errors(
    source: &str,
    name: &str,
) -> Vec<(String, String)> {
    let scratch = Scratch::new(name);
    let entry = scratch.write(source);
    errors(&entry)
}

#[test]
fn ode_solve_has_dense_output_and_multiple_events() {
    let output = run(
        r#"module P
import Ode { OdeOpts, OdeEvent, Dopri5, ode_solve }
fn rhs(_t: Float[T], s: { x: Float[1], v: Float[1/T] }): { x: Float[1/T], v: Float[1/T^2] } =
    { x = s.v, v = -s.x / (1.0 s^2) }
fn zero(_t: Float[T], s: { x: Float[1], v: Float[1/T] }): Float[1] = s.x
fn main(): IO Unit = do
    let opts: OdeOpts<{ x: Float[1], v: Float[1/T] }> = { rtol = 1e-10, atol = 1e-12, method = Dopri5, steps = None, events = None }
    let events = [{ g = zero, terminal = false, direction = 0 }]
    let sol = ode_solve(rhs, { x = 1.0, v = 0.0 / (1.0 s) }, (0.0 s, 10.0 s), opts, events)
    let mid = sol.at(2.5 s)
    println("dense {abs(mid.x - cos(2.5)) < 1e-7}")
    println("hits {length(sol.events)}")
    let first = sol.events[0]
    println("first {abs(first.t / (1.0 s) - 1.5707963267948966) < 1e-6}")
"#,
        "ode-solve",
    );
    assert_eq!(output, "dense true\nhits 3\nfirst true\n", "{output}");
}

#[test]
fn ode_solve_terminal_event_ends_the_solution() {
    let output = run(
        r#"module P
import Ode { OdeOpts, OdeEvent, Dopri5, ode_solve }
fn rhs(_t: Float[T], y: Float[1]): Float[1/T] = -y / (1.0 s)
fn half(_t: Float[T], y: Float[1]): Float[1] = y - 0.5
fn main(): IO Unit = do
    let opts: OdeOpts<Float[1]> = { rtol = 1e-10, atol = 1e-12, method = Dopri5, steps = None, events = None }
    let sol = ode_solve(rhs, 1.0, (0.0 s, 5.0 s), opts, [{ g = half, terminal = true, direction = -1 }])
    let last = sol.t[length(sol.t) - 1]
    println("stop {abs(last / (1.0 s) - 0.6931471805599453) < 1e-6}")
"#,
        "ode-terminal",
    );
    assert_eq!(output, "stop true\n", "{output}");
}

#[test]
fn a_second_order_equation_tracks_dimensions() {
    let output = run(
        r#"module P
import Ode { odeint_order }
fn accel(_t: Float[T], x: Float[L], v: Float[L/T]): Float[L/T^2] = -x / (1.0 s^2)
fn main(): IO Unit = do
    let sol = odeint_order(accel, (1.0 m, 0.0 m / (1.0 s)), [0.0 s, 1.0 s, 2.0 s])
    let last = sol.y[2]
    println("{abs(last / (1.0 m) - cos(2.0)) < 1e-6}")
    let (_x, v) = sol.d[2]
    println("{abs(v / (1.0 m / (1.0 s)) + sin(2.0)) < 1e-6}")
"#,
        "order2",
    );
    assert_eq!(output, "true\ntrue\n", "{output}");
}

#[test]
fn a_wrong_derivative_dimension_is_a_type_error() {
    // The acceleration must be L/T^2, not L/T.
    let found = check_errors(
        r#"module P
import Ode { odeint_order }
fn accel(_t: Float[T], x: Float[L], v: Float[L/T]): Float[L/T] = v
fn main(): IO Unit = do
    let sol = odeint_order(accel, (1.0 m, 0.0 m / (1.0 s)), [0.0 s, 1.0 s])
    println(sol.y[1])
"#,
        "order-bad",
    );
    assert!(
        found
            .iter()
            .any(|(code, _)| code == "E0401" || code == "E0302"),
        "{found:?}"
    );
}

#[test]
fn a_boundary_value_problem_is_solved_by_shooting() {
    let output = run(
        r#"module P
import Ode { OdeOpts, Dopri5, ode_bvp }
fn rhs(_t: Float[T], s: { y: Float[1], p: Float[1/T] }): { y: Float[1/T], p: Float[1/T^2] } =
    { y = s.p, p = -s.y / (1.0 s^2) }
fn bc(a: { y: Float[1], p: Float[1/T] }, b: { y: Float[1], p: Float[1/T] }): Tensor<1, Float[1]> =
    [a.y, b.y - 1.0]
fn main(): IO Unit = do
    let opts: OdeOpts<{ y: Float[1], p: Float[1/T] }> = { rtol = 1e-10, atol = 1e-12, method = Dopri5, steps = None, events = None }
    let sol = ode_bvp(rhs, { y = 0.0, p = 0.5 / (1.0 s) }, (0.0 s, 1.5707963267948966 s), bc, opts)
    let mid = sol.at(0.7853981633974483 s)
    println("{abs(mid.y - 0.7071067811865476) < 1e-6}")
"#,
        "bvp",
    );
    assert_eq!(output, "true\n", "{output}");
}

#[test]
fn a_stiff_problem_switches_solver() {
    let output = run(
        r#"module P
import Ode { OdeOpts, AutoSwitch, ode_solve }
fn rhs(t: Float[T], y: Float[1]): Float[1/T] = (-1.0e6 * (y - cos(t / (1.0 s))) - sin(t / (1.0 s))) / (1.0 s)
fn main(): IO Unit = do
    let opts: OdeOpts<Float[1]> = { rtol = 1e-8, atol = 1e-10, method = AutoSwitch, steps = None, events = None }
    let sol = ode_solve(rhs, 1.0, (0.0 s, 1.0 s), opts, [])
    println(sol.method)
"#,
        "auto",
    );
    assert_eq!(output, "Radau\n", "{output}");
}

const SDE_HEAD: &str = r#"module S
import Sde { SdeMethod, SdeStructure, SdeOpts, SdeEvent, Fixed, Milstein, Diagonal, sdeint_diag, sde_at, sde_resample, sde_interp, sde_events, sde_ensemble_diag, sde_ensemble_seed }
fn drift(_t: Float[T], x: Float[1]): Float[1/T] = 0.0 * x / (1.0 s)
fn diffusion(_t: Float[T], x: Float[1]): Float[1/T^(1/2)] = 0.4 * x / sqrt(1.0 s)
fn opts(seed: Int): SdeOpts<T> = Fixed({ structure = Diagonal, method = Milstein, seed = seed, t0 = 0.0 s, dt = 0.01 s, steps = 200, terms = 0 })
"#;

#[test]
fn an_sde_path_is_callable_at_any_time_and_resampled() {
    let source = format!(
        "{SDE_HEAD}{}",
        r#"fn main(): IO Unit = do
    match sdeint_diag(drift, diffusion, 1.0, opts(3)) with
    | Ok(path) -> do
        let f = sde_interp(path)
        println("node {f(0.5 s) == path.y[50]}")
        let mid = sde_at(path, 0.505 s)
        println("chord {abs(mid - 0.5 * (path.y[50] + path.y[51])) < 1e-12}")
        let saved = sde_resample(path, [0.0 s, 1.0 s, 2.0 s])
        println("saved {length(saved)} {saved[0]}")
    | Err(e) -> println("error")
"#
    );
    let output = run(&source, "sde-at");
    assert_eq!(output, "node true\nchord true\nsaved 3 1.0\n", "{output}");
}

#[test]
fn sde_events_cut_the_path_at_a_terminal_crossing() {
    let source = format!(
        "{SDE_HEAD}{}",
        r#"fn above(_t: Float[T], x: Float[1]): Float[1] = x - 1.05
fn main(): IO Unit = do
    match sdeint_diag(drift, diffusion, 1.0, opts(11)) with
    | Ok(path) -> do
        let r = sde_events(path, [{ g = above, terminal = true, direction = 1 }])
        let n = length(r.hits)
        let last = r.path.t[length(r.path.t) - 1]
        if n == 0 then println("none {length(r.path.t) == length(path.t)}")
        else println("cut {abs(r.hits[0].y - 1.05) < 1e-9} {last == r.hits[0].t} {length(r.path.t) <= length(path.t)}")
    | Err(e) -> println("error")
"#
    );
    let output = run(&source, "sde-events");
    assert!(
        output == "none true\n" || output == "cut true true true\n",
        "{output}"
    );
}

#[test]
fn an_sde_ensemble_member_equals_the_single_run_with_its_seed() {
    let source = format!(
        "{SDE_HEAD}{}",
        r#"fn main(): IO Unit = do
    let batch = sde_ensemble_diag(drift, diffusion, 1.0, opts(5), 64)
    println("{length(batch)}")
    match batch[7] with
    | Ok(p) -> do
        println("seed {p.seed == sde_ensemble_seed(5, 7)}")
        match sdeint_diag(drift, diffusion, 1.0, opts(sde_ensemble_seed(5, 7))) with
        | Ok(q) -> println("same {p.y[200] == q.y[200]}")
        | Err(e) -> println("error")
    | Err(e) -> println("error")
"#
    );
    let output = run(&source, "sde-ens");
    assert_eq!(output, "64\nseed true\nsame true\n", "{output}");
}

#[test]
fn gmres_and_bicgstab_solve_a_nonsymmetric_sparse_system() {
    let output = run(
        r#"module K
import LinearAlgebra.Sparse { Precond, Ilu0, Jacobi, sparse_coo, gmres_with, bicgstab_with, spsolve }
fn main(): IO Unit = do
    let a = sparse_coo([4, 4], [(0, 0), (0, 1), (1, 0), (1, 1), (1, 2), (2, 1), (2, 2), (2, 3), (3, 2), (3, 3)], [4.0, -0.5, -1.5, 4.0, -0.5, -1.5, 4.0, -0.5, -1.5, 4.0])
    let b = [1.0, 2.0, 3.0, 4.0]
    let ref = spsolve(a, b)
    let g = gmres_with(a, b, { tol = 1e-12, maxiter = 100, restart = 4, precond = Ilu0 })
    let s = bicgstab_with(a, b, { tol = 1e-12, maxiter = 100, precond = Jacobi })
    println("{g.converged} {s.converged} {g.precond}")
    println("{abs(g.x[3] - ref[3]) < 1e-9} {abs(s.x[0] - ref[0]) < 1e-9}")
"#,
        "krylov",
    );
    assert_eq!(output, "true true Ilu0\ntrue true\n", "{output}");
}

#[test]
fn a_large_sparse_matvec_matches_the_sequential_sum_bitwise() {
    // 140k rows: above the parallel threshold (2^17 = 131072 multiply-adds;
    // the diagonal has 50000, the 3-column right side raises it to 150000).
    let output = run(
        r#"module M
import LinearAlgebra.Sparse { sparse_coo }
fn main(): IO Unit = do
    let n = 140000
    let a = sparse_coo([n, n], par_tabulate(n, fn i => (i, i)), par_tabulate(n, fn i => 2.0 + 0.001 * i))
    let x = par_tabulate(n, fn i => 1.0 / (1.0 + i))
    let y = a @ x
    let bad = sum(par_tabulate(n, fn i => if y[i] == (2.0 + 0.001 * i) * (1.0 / (1.0 + i)) then 0 else 1))
    let ok = bad == 0
    println("{ok}")
"#,
        "matvec",
    );
    assert_eq!(output, "true\n", "{output}");
}
