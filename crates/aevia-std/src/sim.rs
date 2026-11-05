//! Simulation scenarios (§8.17).
//!
//! `simulate(name, json)` is the dynamic entry point and the typed wrappers
//! (`Physics.Ising`, `Physics.Geodesic`, …) are the same call with a record
//! converted to JSON. Both go through rssn's scenario registry, which is the
//! only place the scenario names exist (29 in rssn 3.0.0-rc.1, including
//! `ising_wolff`); reimplementing the list here would make Aevia's list drift
//! from the backend's.

/// Every scenario rssn provides, in registry order.
#[must_use]
pub fn scenarios() -> &'static [&'static str] {
    rssn::sim::scenario::SCENARIOS
}

/// True when `name` is a scenario rssn knows.
#[must_use]
pub fn is_scenario(name: &str) -> bool {
    scenarios().contains(&name)
}

/// Runs a scenario with JSON parameters and returns its JSON result.
///
/// # Errors
///
/// The backend's message: an unknown scenario, parameters that do not parse, or
/// a failed simulation.
pub fn simulate(name: &str, params: &str) -> Result<String, String> {
    rssn::sim::scenario::run(name, params)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scenario_list_is_the_backends() {
        // The count is the kernel's, not a constant here; the named checks pin
        // that the facade really reads the live registry rather than a copy
        // that has drifted.
        assert!(scenarios().len() >= 20);
        assert!(is_scenario("ising"));
        assert!(is_scenario("ising_wolff"));
        assert!(is_scenario("geodesic"));
        assert!(!is_scenario("not_a_scenario"));
    }

    #[test]
    fn an_unknown_scenario_is_an_error_not_a_panic() {
        assert!(simulate("not_a_scenario", "{}").is_err());
    }

    #[test]
    fn the_ising_scenario_runs() {
        // A tiny lattice, one sweep: the point is that the typed wrapper has a
        // backend, not that the physics is interesting at this size.
        let params = r#"{"width": 4, "height": 4, "temperature": 2.0, "mc_steps": 1}"#;
        let outcome = simulate("ising", params);
        assert!(outcome.is_ok(), "{outcome:?}");
        let json = outcome.unwrap();
        assert!(json.contains("spins") || json.contains("magnetization"));
    }
}
