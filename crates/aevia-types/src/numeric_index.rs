//! Stable, variable-indexed active numeric facts. No inference substitution or
//! arithmetic policy is hidden here; the owner resolves facts after bindings.

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

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::collections::btree_map::Entry;

/// Retains active facts and indexes their dependents by variable.
#[derive(Clone, Debug)]
pub struct NumericIndex<C> {
    facts: BTreeMap<usize, C>,
    dependencies: HashMap<usize, Vec<u32>>,
    variables: HashMap<u32, BTreeSet<usize>>,
    variable_free: BTreeSet<usize>,
    dirty: BTreeSet<usize>,
    next: usize,
    live: usize,
}
impl<C> Default for NumericIndex<C> {
    fn default() -> Self {
        Self {
            facts: BTreeMap::new(),
            dependencies: HashMap::new(),
            variables: HashMap::new(),
            variable_free: BTreeSet::new(),
            dirty: BTreeSet::new(),
            next: 0,
            live: 0,
        }
    }
}
impl<C> NumericIndex<C> {
    pub(crate) const fn len(&self) -> usize {
        // `BTreeMap::len` cannot be called from the public const accessor.
        self.live
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &C> {
        self.facts.values()
    }

    pub(crate) fn get(
        &self,
        id: usize,
    ) -> Option<&C> {
        self.facts.get(&id)
    }

    pub(crate) fn affected(
        &self,
        variable: u32,
    ) -> Vec<usize> {
        self.variables
            .get(&variable)
            .map_or_default(|facts| facts.iter().copied().collect())
    }

    pub(crate) fn equivalent(
        &self,
        dependencies: &[u32],
        mut equal: impl FnMut(&C) -> bool,
    ) -> bool {
        let candidates = dependencies
            .first()
            .and_then(|variable| self.variables.get(variable));
        let candidates = if dependencies.is_empty() {
            Some(&self.variable_free)
        } else {
            candidates
        };
        candidates.is_some_and(|facts| {
            facts
                .iter()
                .any(|id| self.facts.get(id).is_some_and(&mut equal))
        })
    }

    pub(crate) fn insert(
        &mut self,
        fact: C,
        dependencies: Vec<u32>,
    ) -> usize {
        let id = self.insert_waiting(fact, dependencies);
        self.dirty.insert(id);
        id
    }

    /// Tracks an already checked fact until a dependency changes.
    pub(crate) fn insert_waiting(
        &mut self,
        fact: C,
        dependencies: Vec<u32>,
    ) -> usize {
        let id = loop {
            let id = self.next;
            self.next = id.wrapping_add(1);
            if let Entry::Vacant(slot) = self.facts.entry(id) {
                slot.insert(fact);
                break id;
            }
        };
        self.live = self.live.saturating_add(1);
        self.link(id, dependencies);
        id
    }

    pub(crate) fn replace(
        &mut self,
        id: usize,
        fact: C,
        dependencies: Vec<u32>,
    ) {
        if self.replace_waiting(id, fact, dependencies) {
            self.dirty.insert(id);
        }
    }

    /// Updates a checked fact and its dependency links without assigning a new ID.
    pub(crate) fn replace_waiting(
        &mut self,
        id: usize,
        fact: C,
        dependencies: Vec<u32>,
    ) -> bool {
        if !self.facts.contains_key(&id) {
            return false;
        }
        self.unlink(id);
        self.facts.insert(id, fact);
        self.link(id, dependencies);
        true
    }

    pub(crate) fn retire(
        &mut self,
        id: usize,
    ) {
        self.unlink(id);
        if self.facts.remove(&id).is_some() {
            self.live = self.live.saturating_sub(1);
        }
        self.dirty.remove(&id);
    }

    fn link(
        &mut self,
        id: usize,
        mut dependencies: Vec<u32>,
    ) {
        dependencies.sort_unstable();
        dependencies.dedup();
        if dependencies.is_empty() {
            self.variable_free.insert(id);
        }
        for variable in &dependencies {
            self.variables.entry(*variable).or_default().insert(id);
        }
        self.dependencies.insert(id, dependencies);
    }

    fn unlink(
        &mut self,
        id: usize,
    ) {
        self.variable_free.remove(&id);
        if let Some(dependencies) = self.dependencies.remove(&id) {
            for variable in dependencies {
                if let Some(facts) = self.variables.get_mut(&variable) {
                    facts.remove(&id);
                    if facts.is_empty() {
                        self.variables.remove(&variable);
                    }
                }
            }
        }
    }

    fn connected(
        &self,
        seeds: impl IntoIterator<Item = usize>,
    ) -> Vec<usize> {
        let mut reached = BTreeSet::new();
        let mut visited_variables = HashSet::new();
        let mut pending = VecDeque::new();
        for id in seeds {
            if self.facts.contains_key(&id) && reached.insert(id) {
                pending.push_back(id);
            }
        }
        while let Some(id) = pending.pop_front() {
            if let Some(dependencies) = self.dependencies.get(&id) {
                for variable in dependencies {
                    if !visited_variables.insert(*variable) {
                        continue;
                    }
                    if let Some(facts) = self.variables.get(variable) {
                        for next in facts {
                            if reached.insert(*next) {
                                pending.push_back(*next);
                            }
                        }
                    }
                }
            }
        }
        reached.into_iter().collect()
    }

    pub(crate) fn project(
        &self,
        roots: impl IntoIterator<Item = u32>,
    ) -> Vec<usize> {
        self.connected(roots.into_iter().flat_map(|variable| {
            self.variables
                .get(&variable)
                .into_iter()
                .flat_map(|facts| facts.iter().copied())
        }))
    }

    pub(crate) fn take_dirty_component(&mut self) -> Option<Vec<usize>> {
        let first = *self.dirty.first()?;
        let component = self.connected([first]);
        for id in &component {
            self.dirty.remove(id);
        }
        Some(component)
    }

    /// Schedules only the facts that still depend on this variable.
    pub(crate) fn wake_dependents(
        &mut self,
        variable: u32,
    ) {
        if let Some(facts) = self.variables.get(&variable) {
            self.dirty.extend(facts.iter().copied());
        }
    }

    /// Takes the scheduled fact with the lowest ID without gathering its component.
    pub(crate) fn take_dirty_fact(&mut self) -> Option<usize> {
        self.dirty.pop_first()
    }

    /// Removes all facts in ID order, clearing their dependency links.
    pub(crate) fn drain(&mut self) -> impl Iterator<Item = C> + use<C> {
        std::mem::take(self).facts.into_values()
    }

    pub(crate) fn retry(
        &mut self,
        component: &[usize],
    ) {
        self.dirty.extend(
            component
                .iter()
                .copied()
                .filter(|id| self.facts.contains_key(id)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projection_visits_only_the_requested_function_component() {
        let mut index = NumericIndex::default();
        for function in 0..4096 {
            let base = function * 3;
            index.insert((function, "literal"), vec![base]);
            index.insert((function, "join"), vec![base, base + 1, base + 2]);
            index.insert((function, "default"), vec![base, base + 1]);
        }
        let component = index.project([777 * 3 + 2]);
        assert_eq!(component.len(), 3);
        assert!(component.iter().all(|id| index.get(*id).unwrap().0 == 777));
        let mut visited = 0;
        while let Some(component) = index.take_dirty_component() {
            assert_eq!(component.len(), 3);
            visited += component.len();
        }
        assert_eq!(visited, 3 * 4096);
        assert!(index.take_dirty_component().is_none());
    }
    #[test]
    fn alias_refresh_preserves_links_and_split_components_are_revisited() {
        let mut index = NumericIndex::default();
        let a = index.insert("left", vec![1, 2]);
        let b = index.insert("right", vec![2, 3]);
        assert_eq!(index.take_dirty_component().unwrap(), vec![a, b]);
        let affected = index.affected(2);
        for id in affected {
            let deps = if id == a {
                vec![1, 4]
            } else {
                vec![4, 3]
            };
            index.replace(id, *index.get(id).unwrap(), deps);
        }
        assert!(index.affected(2).is_empty());
        assert_eq!(index.project([4]), vec![a, b]);
        assert_eq!(index.take_dirty_component().unwrap(), vec![a, b]);
        index.replace(a, "left", vec![1]);
        index.replace(b, "right", vec![3]);
        assert_eq!(index.take_dirty_component().unwrap(), vec![a]);
        assert_eq!(index.take_dirty_component().unwrap(), vec![b]);
        index.retire(a);
        assert!(index.project([1]).is_empty());
        assert_eq!(index.len(), 1);
    }
    #[test]
    fn concrete_facts_stay_dirty_until_validated_and_errors_can_retry() {
        let mut index = NumericIndex::default();
        let id = index.insert("concrete", vec![]);
        assert!(index.equivalent(&[], |fact| *fact == "concrete"));
        let component = index.take_dirty_component().unwrap();
        assert_eq!(component, vec![id]);
        index.retry(&component);
        assert_eq!(index.take_dirty_component().unwrap(), component);
        index.retire(id);
        assert!(!index.equivalent(&[], |_| true));
        assert_eq!(index.iter().count(), 0);
    }
    #[test]
    fn a_high_fanout_variable_is_expanded_once() {
        let mut index = NumericIndex::default();
        for variable in 1..4097 {
            index.insert(variable, vec![0, variable]);
        }
        assert_eq!(index.project([0]).len(), 4096);
        assert_eq!(index.take_dirty_component().unwrap().len(), 4096);
        assert!(index.take_dirty_component().is_none());
    }

    #[test]
    fn counter_wrap_does_not_replace_a_live_fact() {
        let mut index = NumericIndex::default();
        let first = index.insert("first", vec![0]);
        index.next = usize::MAX;
        let last = index.insert("last", vec![1]);
        let wrapped = index.insert("wrapped", vec![2]);
        assert_eq!([first, last, wrapped], [0, usize::MAX, 1]);
        assert_eq!(index.get(first), Some(&"first"));
        assert_eq!(index.project([0]), vec![first]);
        assert_eq!(index.project([2]), vec![wrapped]);
    }
}
