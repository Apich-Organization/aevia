//! Prints a generated domain module: `cargo run -p aevia-rssn --example dump_domain -- Rssn_Calculus`.
fn main() {
    let name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Rssn_NumberTheory".to_string());
    match aevia_rssn::domains::domain_source(aevia_rssn::registry(), &name) {
        | Some(source) => println!("{source}"),
        | None => {
            eprintln!(
                "no domain module `{name}`; one of {:?}",
                aevia_rssn::domains::module_names()
            )
        },
    }
}
