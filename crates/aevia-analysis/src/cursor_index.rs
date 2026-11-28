//! A point index for selecting the narrowest of overlapping source ranges.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Keeps the chosen item at every boundary where the answer changes.
#[derive(Clone, Debug, Default)]
pub struct CursorIndex<T> {
    changes: Vec<(u32, Option<T>)>,
}

impl<T: Copy + Eq> CursorIndex<T> {
    /// Indexes inclusive ranges. Equal widths prefer the original input order.
    pub fn new(ranges: impl IntoIterator<Item = (u32, u32, T)>) -> Self {
        let mut starts = ranges
            .into_iter()
            .filter(|(start, end, _)| start <= end)
            .enumerate()
            .collect::<Vec<_>>();
        starts.sort_unstable_by_key(|(_, (start, _, _))| *start);
        let mut starts = starts.into_iter().peekable();
        let mut active = BinaryHeap::<Reverse<(u32, usize, u32)>>::new();
        let mut changes = Vec::new();
        let mut previous = None;
        // The heap compares the width, then the original ordinal; payloads
        // are stored separately so a span need not implement ordering.
        let mut values = vec![None; starts.len()];
        loop {
            let next_start = starts.peek().map(|(_, (start, _, _))| *start);
            let next_end = active
                .peek()
                .and_then(|Reverse((_, _, end))| end.checked_add(1));
            let position = match (next_start, next_end) {
                | (Some(start), Some(end)) => start.min(end),
                | (Some(point), None) | (None, Some(point)) => point,
                | (None, None) => break,
            };
            while starts
                .peek()
                .is_some_and(|(_, (start, _, _))| *start == position)
            {
                if let Some((order, (start, end, value))) = starts.next() {
                    if let Some(slot) = values.get_mut(order) {
                        *slot = Some(value);
                    }
                    active.push(Reverse((end.saturating_sub(start), order, end)));
                }
            }
            while active
                .peek()
                .is_some_and(|Reverse((_, _, end))| *end < position)
            {
                active.pop();
            }
            let current = active
                .peek()
                .and_then(|Reverse((_, order, _))| values.get(*order).copied().flatten());
            if current != previous {
                changes.push((position, current));
                previous = current;
            }
        }
        Self { changes }
    }

    /// Returns the narrowest indexed range's payload at the given byte offset.
    pub fn at(
        &self,
        offset: u32,
    ) -> Option<T> {
        let end = self.changes.partition_point(|(start, _)| *start <= offset);
        self.changes
            .get(end.checked_sub(1)?)
            .and_then(|(_, value)| *value)
    }
}

#[cfg(test)]
mod tests {
    use super::CursorIndex;

    #[test]
    fn overlaps_ties_gaps_and_maximum_offset_match_a_linear_lookup() {
        let ranges = [
            (1, 5, 'a'),
            (3, 6, 'b'),
            (4, 4, 'c'),
            (2, 3, 'd'),
            (0, 9, 'e'),
            (6, 7, 'f'),
            (7, 8, 'g'),
            (12, 14, 'h'),
            (17, 16, 'i'),
            (u32::MAX - 1, u32::MAX, 'j'),
            (u32::MAX, u32::MAX, 'k'),
        ];
        let index = CursorIndex::new(ranges);
        for point in (0..20).chain((u32::MAX - 2)..=u32::MAX) {
            let expected = ranges
                .iter()
                .filter(|(start, end, _)| (*start..=*end).contains(&point))
                .min_by_key(|(start, end, _)| end - start)
                .map(|(_, _, value)| *value);
            assert_eq!(index.at(point), expected, "at {point}");
        }
    }
}
