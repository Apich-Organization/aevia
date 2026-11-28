//! Applies machine-applicable fixes to their original source snapshots.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs;
use std::ops::Bound;
use std::path::PathBuf;

use aevia_syntax::FileId;
use aevia_syntax::SourceFile;
use aevia_syntax::SourceMap;

use crate::pass::Applicability;
use crate::pass::LintDiagnostic;
use crate::pass::TextEdit;

/// Outcome of applying machine-applicable fixes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FixSummary {
    /// Number of fixes accepted and applied.
    pub applied: usize,
    /// Paths modified on disk.
    pub files_modified: Vec<String>,
}

type ByteRange = (usize, usize);

struct FileEdits<'a> {
    path: PathBuf,
    source: &'a SourceFile,
    replacements: BTreeMap<ByteRange, &'a str>,
}

impl FileEdits<'_> {
    fn overlaps(
        &self,
        range: ByteRange,
    ) -> bool {
        let (start, end) = range;
        // Accepted ranges are disjoint. Only the last range starting before
        // the proposed end can intersect it. For an insertion, also include
        // an earlier insertion at the exact same byte.
        let upper = if start == end {
            Bound::Included(range)
        } else {
            Bound::Excluded((end, 0))
        };
        self.replacements
            .range((Bound::Unbounded, upper))
            .next_back()
            .is_some_and(|(&previous, _)| ranges_conflict(previous.min(range), previous.max(range)))
    }
}

fn ranges_conflict(
    left: ByteRange,
    right: ByteRange,
) -> bool {
    let (_, end) = left;
    let (start, _) = right;
    start < end || left == right
}

struct PlannedEdit<'a> {
    file: usize,
    range: ByteRange,
    replacement: &'a str,
}

#[derive(Default)]
struct EditPlan<'a> {
    by_source: HashMap<FileId, Option<usize>>,
    by_path: HashMap<PathBuf, usize>,
    files: Vec<FileEdits<'a>>,
}

impl<'a> EditPlan<'a> {
    fn source_index(
        &mut self,
        source: &'a SourceFile,
    ) -> Option<usize> {
        let path = fs::canonicalize(&source.name).ok()?;
        if let Some(&index) = self.by_path.get(&path) {
            return (self.files.get(index)?.source.text == source.text).then_some(index);
        }
        if fs::read_to_string(&path).ok()? != source.text {
            return None;
        }
        let index = self.files.len();
        self.by_path.insert(path.clone(), index);
        self.files.push(FileEdits {
            path,
            source,
            replacements: BTreeMap::new(),
        });
        Some(index)
    }

    fn edit(
        &mut self,
        sources: &'a SourceMap,
        edit: &'a TextEdit,
    ) -> Option<PlannedEdit<'a>> {
        let id = edit.span.file;
        let file = match self.by_source.get(&id) {
            | Some(index) => *index,
            | None => {
                let index = self.source_index(sources.file(id)?);
                self.by_source.insert(id, index);
                index
            },
        }?;
        let start = usize::try_from(edit.span.start).ok()?;
        let end = usize::try_from(edit.span.end).ok()?;
        let plan = self.files.get(file)?;
        plan.source.text.get(start..end)?;
        (!plan.overlaps((start, end))).then_some(PlannedEdit {
            file,
            range: (start, end),
            replacement: &edit.replacement,
        })
    }
}

/// Applies `MachineApplicable` fixes cleanly, rejecting overlapping edits.
pub struct FixEngine;

impl FixEngine {
    /// Applies machine-applicable fixes against the original source snapshots.
    ///
    /// A fix with any invalid, stale, or overlapping edit is skipped as a unit.
    /// For conflicts between fixes, the first eligible fix takes precedence.
    ///
    /// # Errors
    /// Returns an error if a selected file changes during preparation or cannot be written.
    pub fn apply(
        diagnostics: &[LintDiagnostic],
        sources: &SourceMap,
    ) -> Result<FixSummary, String> {
        let mut plan = EditPlan::default();
        let mut planned = Vec::new();
        let mut applied = 0usize;
        for fix in diagnostics.iter().flat_map(|diagnostic| &diagnostic.fixes) {
            if fix.applicability != Applicability::MachineApplicable || fix.edits.is_empty() {
                continue;
            }
            planned.clear();
            if fix
                .edits
                .iter()
                .try_for_each(|edit| {
                    planned.push(plan.edit(sources, edit)?);
                    Some(())
                })
                .is_none()
            {
                continue;
            }
            planned.sort_unstable_by_key(|edit| (edit.file, edit.range));
            if planned.windows(2).any(|pair| {
                matches!(pair, [a, b] if a.file == b.file && ranges_conflict(a.range, b.range))
            }) {
                continue;
            }
            for edit in &planned {
                if let Some(file) = plan.files.get_mut(edit.file) {
                    file.replacements.insert(edit.range, edit.replacement);
                }
            }
            applied = applied.saturating_add(1);
        }

        let mut modified_files = Vec::new();
        plan.files
            .sort_unstable_by_key(|file| file.source.name.as_str());
        for file in plan.files {
            if file.replacements.is_empty() {
                continue;
            }
            let path = file.source.name.as_str();
            let mut updated = String::with_capacity(file.source.text.len());
            let mut cursor = 0;
            for ((start, end), replacement) in file.replacements {
                let unchanged = file.source.text.get(cursor..start).ok_or_else(|| {
                    format!("invalid byte range when applying lint fixes to `{path}`")
                })?;
                updated.push_str(unchanged);
                updated.push_str(replacement);
                cursor = end;
            }
            let trailing = file.source.text.get(cursor..).ok_or_else(|| {
                format!("invalid trailing range when applying lint fixes to `{path}`")
            })?;
            updated.push_str(trailing);
            if updated == file.source.text {
                continue;
            }
            let current =
                fs::read_to_string(&file.path).map_err(|e| format!("cannot read `{path}`: {e}"))?;
            if current != file.source.text {
                return Err(format!("`{path}` changed while preparing lint fixes"));
            }
            modified_files.push((file.path, path, updated));
        }
        let mut files_modified = Vec::with_capacity(modified_files.len());
        for (target, path, updated) in modified_files {
            fs::write(target, updated).map_err(|e| format!("cannot write `{path}`: {e}"))?;
            files_modified.push(path.to_owned());
        }
        Ok(FixSummary {
            applied,
            files_modified,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_lookup_matches_pairwise_checks_at_all_small_boundaries() {
        let source = SourceFile::new("test", "");
        let ranges: Vec<_> = (0..5)
            .flat_map(|start| (start..5).map(move |end| (start, end)))
            .collect();
        let mut file = FileEdits {
            path: PathBuf::new(),
            source: &source,
            replacements: BTreeMap::new(),
        };
        for &left in &ranges {
            for &right in &ranges {
                if ranges_conflict(left.min(right), left.max(right)) {
                    continue;
                }
                file.replacements.clear();
                file.replacements.insert(left, "");
                file.replacements.insert(right, "");
                for &candidate in &ranges {
                    let expected = [left, right]
                        .into_iter()
                        .any(|old| ranges_conflict(old.min(candidate), old.max(candidate)));
                    assert_eq!(
                        file.overlaps(candidate),
                        expected,
                        "{left:?} {right:?} {candidate:?}"
                    );
                }
            }
        }
    }
}
