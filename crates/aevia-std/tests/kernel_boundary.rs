//! Numerical backend dependencies belong to RSSN; test oracles stay dev-only.
#[test]
fn numerical_backends_do_not_return_to_the_language_runtime_dependencies() {
    let manifest = include_str!("../Cargo.toml");
    let backends = ["nalgebra", "faer", "rustfft", "statrs", "argmin"];
    let mut runtime = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            runtime = line == "[dependencies]";
            for backend in backends {
                assert_ne!(line, format!("[dependencies.{backend}]"));
            }
        } else if runtime {
            let name = line.split('=').next().unwrap_or("").trim();
            assert!(
                !backends.contains(&name),
                "{name} belongs in RSSN or dev-dependencies"
            );
        }
    }
}
