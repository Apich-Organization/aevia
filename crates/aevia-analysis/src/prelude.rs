//! The names the prelude binds.
//!
//! `aevia_types::builtins` decides what a builtin *means*, but it answers one
//! question at a time (`schemes("sqrt")`), so it cannot be enumerated. The
//! list below is the set of names that function accepts, transcribed from it;
//! [`NAMES`] is filtered through `builtins::is_builtin` before use, and
//! `tests::every_prelude_name_is_a_builtin` fails if a name here stops being
//! one. The list is the only thing this crate knows that the checker does not
//! expose, and it is checked rather than trusted.
//!
//! It is not a second copy of the *types*: the scheme always comes from the
//! checker, so a completion can never disagree with what `check_module` would
//! infer.

use aevia_types::builtins;

/// Every name `aevia_types::builtins::schemes` answers for (the list lives
/// next to the schemes so the checker can offer spelling suggestions too).
pub use aevia_types::builtins::PRELUDE_NAMES as NAMES;

/// The prelude names that are still builtins.
///
/// A name that the checker no longer knows is dropped rather than offered: an
/// editor that completes `foo` into an error is worse than one that stays
/// quiet, and the test below makes the drift visible either way.
pub fn names() -> impl Iterator<Item = &'static str> {
    NAMES
        .iter()
        .copied()
        .filter(|name| builtins::is_builtin(name) || *name == "to_callback")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_prelude_name_is_a_builtin() {
        let stale: Vec<&str> = NAMES
            .iter()
            .copied()
            .filter(|name| !builtins::is_builtin(name) && *name != "to_callback")
            .collect();
        assert!(
            stale.is_empty(),
            "these names are in the prelude list but not in the checker: {stale:?}"
        );
    }

    #[test]
    fn the_list_has_no_duplicates() {
        let mut sorted = NAMES.to_vec();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(before, sorted.len(), "the prelude list repeats a name");
    }

    #[test]
    fn a_scheme_is_available_for_every_name() {
        for name in names() {
            assert!(
                builtins::schemes(name).is_some() || name == "to_callback",
                "`{name}` is listed but has no scheme"
            );
        }
    }
}
