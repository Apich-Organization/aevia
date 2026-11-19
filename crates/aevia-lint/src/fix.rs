//! Applies machine-applicable fixes to their original source snapshots.

use std::collections::BTreeMap;
use std::fs;

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
    source: &'a SourceFile,
    replacements: BTreeMap<ByteRange, &'a str>,
}

impl FileEdits<'_> {
    fn overlaps(
        &self,
        range: ByteRange,
    ) -> bool {
        let before = self.replacements.range(..range).next_back();
        let after = self.replacements.range(range..).next();
        before.is_some_and(|(previous, _)| ranges_conflict(*previous, range))
            || after.is_some_and(|(next, _)| ranges_conflict(range, *next))
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
    file: &'a str,
    range: ByteRange,
    replacement: &'a str,
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
        let mut per_file = BTreeMap::new();
        let mut applied = 0usize;
        for fix in diagnostics.iter().flat_map(|diagnostic| &diagnostic.fixes) {
            if fix.applicability != Applicability::MachineApplicable || fix.edits.is_empty() {
                continue;
            }
            let Some(mut planned): Option<Vec<_>> = fix
                .edits
                .iter()
                .map(|edit| plan_edit(sources, &mut per_file, edit))
                .collect()
            else {
                continue;
            };
            planned.sort_unstable_by_key(|edit| (edit.file, edit.range));
            if planned.windows(2).any(|pair| {
                matches!(pair, [a, b] if a.file == b.file && ranges_conflict(a.range, b.range))
            }) {
                continue;
            }
            for edit in planned {
                if let Some(Some(file)) = per_file.get_mut(edit.file) {
                    file.replacements.insert(edit.range, edit.replacement);
                }
            }
            applied = applied.saturating_add(1);
        }

        let mut modified_files = Vec::new();
        for (path, file) in per_file {
            let Some(file) = file else { continue };
            if file.replacements.is_empty() {
                continue;
            }
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
                fs::read_to_string(path).map_err(|e| format!("cannot read `{path}`: {e}"))?;
            if current != file.source.text {
                return Err(format!("`{path}` changed while preparing lint fixes"));
            }
            modified_files.push((path, updated));
        }
        let mut files_modified = Vec::with_capacity(modified_files.len());
        for (path, updated) in modified_files {
            fs::write(path, updated).map_err(|e| format!("cannot write `{path}`: {e}"))?;
            files_modified.push(path.to_owned());
        }
        Ok(FixSummary {
            applied,
            files_modified,
        })
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "validates each edit against its UTF-8 source snapshot before overlap checks"
)]
fn plan_edit<'a>(
    sources: &'a SourceMap,
    files: &mut BTreeMap<&'a str, Option<FileEdits<'a>>>,
    edit: &'a TextEdit,
) -> Option<PlannedEdit<'a>> {
    let source = sources.file(edit.span.file)?;
    let start = usize::try_from(edit.span.start).ok()?;
    let end = usize::try_from(edit.span.end).ok()?;
    let path = source.name.as_str();
    let file = files
        .entry(path)
        .or_insert_with(|| {
            let current = fs::read_to_string(path).ok()?;
            (current == source.text).then(|| {
                FileEdits {
                    source,
                    replacements: BTreeMap::new(),
                }
            })
        })
        .as_ref()?;
    if (!std::ptr::eq(file.source, source) && file.source.text != source.text)
        || file.overlaps((start, end))
    {
        return None;
    }
    file.source.text.get(start..end)?;
    Some(PlannedEdit {
        file: path,
        range: (start, end),
        replacement: &edit.replacement,
    })
}
