//! §5.11: unit systems. A module `with units natural` reduces the dimensions
//! written in it; values stay honest SI quantities, so they cross module
//! boundaries without reinterpretation.

fn run(source: &str) -> String {
    aevia_core::run(source, "systems.aev").unwrap_or_else(|error| {
        panic!("{}: {}", error.code, error.message);
    })
}

/// Runs a program through the module loader, so its imports are loaded.
fn run_loaded(source: &str) -> String {
    let directory = std::env::temp_dir().join(format!(
        "aevia-unit-systems-{}-{}",
        std::process::id(),
        source.len()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("main.aev");
    std::fs::write(&path, source).unwrap();
    let program = aevia_core::driver::load(&path);
    assert!(
        !program.diagnostics.has_errors(),
        "{:?}",
        program.diagnostics.items
    );
    let mut interp = aevia_core::Interp::new(&program.entry().unwrap().checked).with_buffer();
    let diagnostics = aevia_core::driver::run_modules(&mut interp, &program);
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.items);
    interp.run_main().expect("runs");
    let output = interp.captured();
    let _ = std::fs::remove_dir_all(directory);
    output
}

fn check_codes(source: &str) -> Vec<String> {
    let compilation = aevia_core::compile("systems.aev", source);
    compilation
        .diagnostics
        .items
        .iter()
        .map(|item| item.code.clone())
        .collect()
}

#[test]
fn lengths_are_inverse_energies_in_natural_units() {
    let output = run(r#"module Hep with units natural

fn compton(m: Real[M]): Real[L] = 1 / m

let m_e = 0.51099895 MeV
let fermi: Real[L] = 1 fm

fn main(): IO Unit = do
    println(m_e)
    println(approx(fermi))
    println(approx(from_system<L>("natural", compton(m_e))))
"#);
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0], "0.51099895 MeV");
    // 1 fm = 1/(197.3269804 MeV).
    assert!(lines[1].starts_with("Ok(5.06773"), "{}", lines[1]);
    assert!(lines[1].ends_with("eV^-1)"), "{}", lines[1]);
    // The reduced Compton wavelength of the electron, back in SI.
    assert!(lines[2].starts_with("Ok(3.86159"), "{}", lines[2]);
    assert!(lines[2].ends_with(" m)"), "{}", lines[2]);
}

#[test]
fn velocities_are_pure_numbers_and_unity_constants_are_one() {
    let output = run_loaded(
        r#"module Hep with units natural
import Units.Natural as Nat

fn main(): IO Unit = do
    println(approx(3 m / (2 s)))
    println("{Nat.c} {Nat.hbar} {Nat.k_B}")
    println(approx(Nat.m_e))
"#,
    );
    let lines: Vec<&str> = output.lines().collect();
    // 1.5 m/s = 5.0034614e-9 c.
    assert!(lines[0].starts_with("Ok(5.00346"), "{}", lines[0]);
    assert_eq!(lines[1], "1 1 1");
    assert!(lines[2].starts_with("Ok(510998.95"), "{}", lines[2]);
}

#[test]
fn annotations_reduce_but_imports_keep_their_si_types() {
    // `Real[L]` and `Real[T]` are the same type in natural units.
    assert!(
        check_codes("module M with units natural\nfn f(t: Real[T]): Real[L] = t\n")
            .iter()
            .all(|code| !code.starts_with('E'))
    );
    // In SI they are not.
    assert!(check_codes("fn f(t: Real[T]): Real[L] = t\n").contains(&"E0401".to_string()));
}

#[test]
fn conversions_between_si_and_a_system() {
    let output = run(r#"fn main(): IO Unit = do
    let e = to_system("natural", 1.0 kg)
    println(approx(e))
    let back = from_system<M>("natural", e)
    println(back)
    let a0 = from_system<L>("atomic", 1.0)
    println(a0)
"#);
    let lines: Vec<&str> = output.lines().collect();
    // m c^2 for 1 kg.
    assert!(lines[0].starts_with("Ok(8.98755178"), "{}", lines[0]);
    assert!(
        lines[1].ends_with("kg") && lines[1].starts_with('1') || lines[1].starts_with("0.9999"),
        "{}",
        lines[1]
    );
    // The bohr radius.
    assert!(lines[2].starts_with("5.29177"), "{}", lines[2]);
}

#[test]
fn irrational_scales_and_unknown_systems_are_errors() {
    assert!(
        check_codes("module M with units natural\nlet q = 1 C\n").contains(&"E0511".to_string())
    );
    // A composition whose exponents are integral is exact again.
    assert!(
        !check_codes("module M with units natural\nlet q = 1 C^2/J\n")
            .contains(&"E0511".to_string())
    );
    assert!(check_codes("module M with units vulcan\nlet x = 1\n").contains(&"E0512".to_string()));
}

#[test]
fn geometrized_and_planck_units() {
    let output = run(r#"fn main(): IO Unit = do
    println(approx(to_system("geometrized", 1.98847e30 kg)))
    println(approx(to_system("planck", 1.0 m)))
"#);
    let lines: Vec<&str> = output.lines().collect();
    // The Sun's mass is 1.4766 km.
    assert!(lines[0].starts_with("Ok(1476."), "{}", lines[0]);
    // One metre is 6.187e34 Planck lengths.
    assert!(lines[1].starts_with("Ok(6.187"), "{}", lines[1]);
}
