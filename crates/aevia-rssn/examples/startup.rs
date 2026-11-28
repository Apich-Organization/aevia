//! Measures what one process pays before checking any source: the kernel
//! session and the registry built from its manifest.
//!
//! `cargo run -p aevia-rssn --profile profiling --example startup`
fn main() {
    let t = std::time::Instant::now();
    let session = rssn::api::Session::new();
    let session_ms = t.elapsed().as_secs_f64() * 1e3;
    drop(session);
    let t = std::time::Instant::now();
    let kernel = aevia_rssn::kernel();
    let kernel_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = std::time::Instant::now();
    let manifest = kernel.manifest();
    let manifest_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = std::time::Instant::now();
    let registry = aevia_rssn::registry();
    let registry_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = std::time::Instant::now();
    let units = aevia_rssn::unit_table();
    let units_ms = t.elapsed().as_secs_f64() * 1e3;
    println!(
        "rssn Session::new {session_ms:.1} ms | aevia kernel() {kernel_ms:.1} ms | manifest {manifest_ms:.1} ms | registry {registry_ms:.1} ms ({} ops) | unit_table {units_ms:.1} ms",
        registry.ops().count()
    );
    let _ = (manifest, units);
}
