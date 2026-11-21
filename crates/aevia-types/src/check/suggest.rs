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
        | _ => MAX_EDITS,
    };
    let mut best: Option<(usize, &'a str)> = None;
    let mut other = Vec::new();
    for candidate in candidates {
        if candidate == name
            || candidate.is_empty()
            || candidate.contains([':', '@', '$', '#'])
            || candidate.starts_with("__")
        {
            continue;
        }
        let limit = best.map_or(limit, |(distance, _)| distance);
        let Some(limit) = candidate_chars(candidate, &target, limit, &mut other) else {
            continue;
        };
        let distance = osa_distance(&target, &other, limit);
        if distance > limit || distance >= other.len() {
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
        | _ => MAX_EDITS,
    };
    let mut best: Option<((usize, bool, bool), &'a str)> = None;
    let mut other = Vec::new();
    for candidate in candidates {
        if candidate == name || candidate.is_empty() || candidate.contains([':', '@', '$', '#']) {
            continue;
        }
        let limit = best.map_or(limit, |((distance, _, _), _)| distance);
        let Some(limit) = candidate_chars(candidate, &target, limit, &mut other) else {
            continue;
        };
        let distance = osa_distance(&target, &other, limit);
        if distance > limit {
            continue;
        }
        let contained = is_subsequence(&other, &target) || is_subsequence(&target, &other);
        if distance >= other.len() && !contained {
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

const MAX_EDITS: usize = 3;
const BAND_WIDTH: usize = MAX_EDITS.saturating_mul(2).saturating_add(1);

fn candidate_chars(
    candidate: &str,
    target: &[char],
    mut limit: usize,
    other: &mut Vec<char>,
) -> Option<usize> {
    // Beyond one edit the first letter must agree, ignoring case.
    if limit > 1
        && !target
            .first()?
            .to_lowercase()
            .eq(candidate.chars().next()?.to_lowercase())
    {
        limit = 1;
    }
    let min = target.len().saturating_sub(limit);
    let max = target.len().saturating_add(limit);
    // A UTF-8 character occupies one to four bytes.
    if !(min..=max.saturating_mul(4)).contains(&candidate.len()) {
        return None;
    }
    other.clear();
    other.extend(candidate.chars().take(max.saturating_add(1)));
    (min..=max).contains(&other.len()).then_some(limit)
}

// Compute only cells within `limit` edits of the diagonal. Every accepted
// path stays in that band, and transposition needs at most two previous rows.
// Suggestion cutoffs are bounded by MAX_EDITS, including when the best result
// so far or the first-letter rule tightens the band.
fn osa_distance(
    a: &[char],
    b: &[char],
    limit: usize,
) -> usize {
    let outside = limit.saturating_add(1);
    let lookup = |row: &(usize, [usize; BAND_WIDTH]), column: usize| {
        column
            .checked_sub(row.0)
            .and_then(|offset| row.1.get(offset).copied())
            .unwrap_or(outside)
    };
    let mut rows = [(0, [outside; BAND_WIDTH]); 3];
    let [older, last, next] = &mut rows;
    let (mut transposition, mut previous, mut current) = (older, last, next);
    for (column, cell) in (0..=limit.min(b.len())).zip(&mut previous.1) {
        *cell = column;
    }
    for (prefix, character) in a.iter().enumerate() {
        let length = prefix.saturating_add(1);
        current.0 = length.saturating_sub(limit);
        current.1.fill(outside);
        let mut left = outside;
        let end = length.saturating_add(limit).min(b.len());
        for (column, cell) in (current.0..=end).zip(&mut current.1) {
            let Some(other_index) = column.checked_sub(1) else {
                *cell = length;
                left = length;
                continue;
            };
            let Some(other) = b.get(other_index) else {
                break;
            };
            let deletion = lookup(previous, column).saturating_add(1);
            let insertion = left.saturating_add(1);
            let replacement =
                lookup(previous, other_index).saturating_add(usize::from(character != other));
            let mut value = deletion.min(insertion).min(replacement).min(outside);
            if let (Some(before), Some(other_before)) =
                (prefix.checked_sub(1), other_index.checked_sub(1))
                && a.get(before) == Some(other)
                && b.get(other_before) == Some(character)
            {
                value = value.min(lookup(transposition, other_before).saturating_add(1));
            }
            *cell = value;
            left = value;
        }
        (transposition, previous, current) = (previous, current, transposition);
    }
    lookup(previous, b.len())
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

    #[test]
    fn tightening_the_band_keeps_name_and_unit_ties() {
        assert_eq!(closest_name("pllint", ["pllintx", "pllin"]), Some("pllin"));
        assert_eq!(closest_name("pllint", ["pllin", "pllintx"]), Some("pllin"));
        assert_eq!(closest_unit("kmm", ["klm", "km"]), Some("km"));
        assert_eq!(closest_name("Δeltaxx", ["δeltayx"]), Some("δeltayx"));
        assert_eq!(closest_name("Ωmega", ["Σmega"]), Some("Σmega"));
        assert_eq!(closest_name("İstanbxx", ["istanbul"]), None);
    }

    fn full_distance(
        a: &[char],
        b: &[char],
    ) -> usize {
        let mut cells = vec![vec![0; b.len() + 1]; a.len() + 1];
        for (i, row) in cells.iter_mut().enumerate() {
            row[0] = i;
        }
        for (j, cell) in cells[0].iter_mut().enumerate() {
            *cell = j;
        }
        for i in 1..=a.len() {
            for j in 1..=b.len() {
                let mut best = (cells[i - 1][j] + 1)
                    .min(cells[i][j - 1] + 1)
                    .min(cells[i - 1][j - 1] + usize::from(a[i - 1] != b[j - 1]));
                if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                    best = best.min(cells[i - 2][j - 2] + 1);
                }
                cells[i][j] = best;
            }
        }
        cells[a.len()][b.len()]
    }

    #[test]
    fn every_cutoff_matches_an_independent_full_matrix() {
        let letters = ['a', 't', 'İ', 'δ', 'é', '時', '\u{307}'];
        let mut state = 0xd9da_736b_617a_bb91_u64;
        let mut word = |len| {
            (0..len)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    letters[state as usize % letters.len()]
                })
                .collect::<Vec<_>>()
        };
        for case in 0..2048 {
            let a = word(case % 13);
            let b = word((case / 13) % 13);
            let expected = full_distance(&a, &b);
            for limit in 0..=super::MAX_EDITS {
                assert_eq!(osa_distance(&a, &b, limit), expected.min(limit + 1));
            }
        }
    }
}
