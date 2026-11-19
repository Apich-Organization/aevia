//! Name and unit suggestions for type diagnostics.

#![cfg_attr(
    not(test),
    deny(
        missing_docs,
        clippy::arithmetic_side_effects,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::nursery,
        clippy::panic,
        clippy::pedantic,
        clippy::single_call_fn,
        clippy::unwrap_used
    )
)]

/// The candidate closest to a misspelt `name`, for a "did you mean" help.
///
/// Distance is optimal string alignment over characters (a transposition
/// costs one), and the accepted distance grows with the name: one edit for
/// names of up to three characters, two up to six, three beyond. Internal
/// names (mangled instance globals, generated helpers) are never offered.
/// Ties go to the lexicographically smallest candidate, so the help text is
/// deterministic.
#[must_use]
pub fn closest_name<'a>(
    name: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let target: Vec<char> = name.chars().collect();
    let limit = match target.len() {
        | 0 => return None,
        | 1..=3 => 1,
        | 4..=6 => 2,
        | _ => 3,
    };
    let mut best: Option<(usize, &'a str)> = None;
    for candidate in candidates {
        if candidate == name
            || candidate.is_empty()
            || candidate.contains([':', '@', '$', '#'])
            || candidate.starts_with("__")
        {
            continue;
        }
        let other: Vec<char> = candidate.chars().collect();
        if other.len().abs_diff(target.len()) > limit {
            continue;
        }
        let distance = osa_distance(&target, &other, limit);
        if distance > limit || distance >= other.len() {
            continue;
        }
        // Beyond one edit the first letter must agree (ignoring case): a
        // reader misspells the middle of a name, and `Normal` -> `format`
        // is two edits away but no help at all.
        if distance > 1
            && !name
                .chars()
                .next()
                .into_iter()
                .flat_map(char::to_lowercase)
                .eq(candidate
                    .chars()
                    .next()
                    .into_iter()
                    .flat_map(char::to_lowercase))
        {
            continue;
        }
        let better = match best {
            | None => true,
            | Some((d, current)) => distance < d || (distance == d && candidate < current),
        };
        if better {
            best = Some((distance, candidate));
        }
    }
    best.map(|(_, candidate)| candidate)
}

/// Whether `short` can be obtained from `long` by deleting characters.
fn is_subsequence(
    short: &[char],
    long: &[char],
) -> bool {
    let mut rest = long.iter();
    short
        .iter()
        .all(|wanted| rest.by_ref().any(|have| have == wanted))
}

/// The unit spellings a reader is most likely to have meant; they win ties
/// between equally distant candidates.
const COMMON_UNITS: &[&str] = &[
    "m", "s", "g", "kg", "A", "K", "mol", "cd", "Hz", "N", "Pa", "J", "W", "C", "V", "F", "Ω", "S",
    "T", "L", "min", "h", "d", "rad", "deg", "eV", "bar", "atm", "ft", "in", "mi", "lb", "°C",
];

/// The unit spelling closest to a misspelt `name`.
///
/// Like [`closest_name`] (same distance and first-letter rules), but
/// equally distant candidates are ranked by what a reader most plausibly
/// meant instead of alphabetically: first the *containment-minimal* edit (the
/// candidate is `name` with characters deleted, or `name` with characters
/// added — `kmm` is `km` plus a stray letter, not `klm` with a wrong one),
/// then a common unit, then alphabetical order. A pure deletion is accepted
/// even when it leaves a single character (`mm` -> `m`).
#[must_use]
pub fn closest_unit<'a>(
    name: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let target: Vec<char> = name.chars().collect();
    let limit = match target.len() {
        | 0 => return None,
        | 1..=3 => 1,
        | 4..=6 => 2,
        | _ => 3,
    };
    let mut best: Option<((usize, bool, bool), &'a str)> = None;
    for candidate in candidates {
        if candidate == name || candidate.is_empty() || candidate.contains([':', '@', '$', '#']) {
            continue;
        }
        let other: Vec<char> = candidate.chars().collect();
        if other.len().abs_diff(target.len()) > limit {
            continue;
        }
        let contained = is_subsequence(&other, &target) || is_subsequence(&target, &other);
        let distance = osa_distance(&target, &other, limit);
        if distance > limit || (distance >= other.len() && !contained) {
            continue;
        }
        if distance > 1
            && !name
                .chars()
                .next()
                .into_iter()
                .flat_map(char::to_lowercase)
                .eq(candidate
                    .chars()
                    .next()
                    .into_iter()
                    .flat_map(char::to_lowercase))
        {
            continue;
        }
        let key = (distance, !contained, !COMMON_UNITS.contains(&candidate));
        let better = match best {
            | None => true,
            | Some((current_key, current)) => {
                key < current_key || (key == current_key && candidate < current)
            },
        };
        if better {
            best = Some((key, candidate));
        }
    }
    best.map(|(_, candidate)| candidate)
}

// Compute only cells within `limit` edits of the diagonal. Every accepted
// path stays in that band, and transposition needs at most two previous rows.
fn osa_distance(
    a: &[char],
    b: &[char],
    limit: usize,
) -> usize {
    let outside = limit.saturating_add(1);
    let lookup = |row: &(usize, Vec<usize>), column: usize| {
        column
            .checked_sub(row.0)
            .and_then(|offset| row.1.get(offset).copied())
            .unwrap_or(outside)
    };
    let mut transposition = (0, Vec::new());
    let mut previous = (0, (0..=limit.min(b.len())).collect());
    let mut current = (0, Vec::new());
    for (prefix, character) in a.iter().enumerate() {
        let length = prefix.saturating_add(1);
        current.0 = length.saturating_sub(limit);
        current.1.clear();
        for column in current.0..=length.saturating_add(limit).min(b.len()) {
            let Some(other_index) = column.checked_sub(1) else {
                current.1.push(length);
                continue;
            };
            let Some(other) = b.get(other_index) else {
                break;
            };
            let deletion = lookup(&previous, column).saturating_add(1);
            let insertion = current
                .1
                .last()
                .copied()
                .unwrap_or(outside)
                .saturating_add(1);
            let replacement =
                lookup(&previous, other_index).saturating_add(usize::from(character != other));
            let mut value = deletion.min(insertion).min(replacement).min(outside);
            if let (Some(before), Some(other_before)) =
                (prefix.checked_sub(1), other_index.checked_sub(1))
                && a.get(before) == Some(other)
                && b.get(other_before) == Some(character)
            {
                value = value.min(lookup(&transposition, other_before).saturating_add(1));
            }
            current.1.push(value);
        }
        std::mem::swap(&mut transposition, &mut previous);
        std::mem::swap(&mut current, &mut previous);
    }
    lookup(&previous, b.len())
}

#[cfg(test)]
mod suggestion_tests {
    use super::closest_name;
    use super::closest_unit;
    use super::osa_distance;

    #[test]
    fn a_stray_letter_is_deleted_rather_than_substituted() {
        // `kmm` is `km` with an extra `m`; `klm` (kilolumen) is a wrong
        // letter, and used to win alphabetically.
        let bases = ["m", "lm", "mi", "s"];
        assert_eq!(closest_unit("mm", bases), Some("m"));
        assert_eq!(closest_unit("mm", ["lm", "m"]), Some("m"));
        assert_eq!(closest_unit("kmm", ["klm", "km", "kg"]), Some("km"));
        // Equally good and equally contained: a common unit beats an
        // alphabetically earlier odd one.
        assert_eq!(closest_unit("hh", ["ah", "h"]), Some("h"));
        assert_eq!(closest_unit("kgg", ["kg", "kgg2"]), Some("kg"));
        assert_eq!(closest_unit("secs", ["sec", "s", "secs2"]), Some("sec"));
        // Nothing near stays nothing.
        assert_eq!(closest_unit("qqqq", ["m", "s"]), None);
    }

    #[test]
    fn long_unicode_names_stay_within_the_edit_band() {
        let original = format!("{}βγ", "α".repeat(20_000));
        let misspelling = format!("{}γβ", "α".repeat(20_000));
        assert_eq!(
            closest_name(&misspelling, [original.as_str()]),
            Some(original.as_str())
        );
        assert_eq!(osa_distance(&['a', 'b'], &['b', 'a'], 1), 1);
        assert_eq!(osa_distance(&['a', 'b'], &['b', 'a'], 0), 1);
    }

    #[test]
    fn close_names_are_suggested_and_far_ones_are_not() {
        let names = ["println", "print", "length", "linspace", "sqrt"];
        assert_eq!(closest_name("pritnln", names), Some("println"));
        assert_eq!(closest_name("lenght", names), Some("length"));
        assert_eq!(closest_name("sqtr", names), Some("sqrt"));
        assert_eq!(closest_name("zzz", names), None);
        assert_eq!(closest_name("x", ["y"]), None);
        assert_eq!(
            closest_name("value", ["valeu", "Show@Int::show"]),
            Some("valeu")
        );
    }
}
