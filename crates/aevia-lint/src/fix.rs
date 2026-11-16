//! Transactional fix application for `aevia lint --fix`.

use std::collections::HashMap;
use std::path::Path;

use aevia_syntax::SourceMap;

use crate::pass::Applicability;
use crate::pass::LintDiagnostic;
use crate::pass::TextEdit;

/// Outcome of applying machine-applicable fixes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FixSummary {
    /// Number of fixes applied.
    pub applied: usize,
    /// Paths modified on disk.
    pub files_modified: Vec<String>,
}

/// Applies `MachineApplicable` fixes cleanly, rejecting overlapping edits.
pub struct FixEngine;

impl FixEngine {
    /// Applies non-overlapping `MachineApplicable` edits to files in `sources`.
    ///
    /// # Errors
    /// Returns an error if a file cannot be written to disk.
    pub fn apply(
        diagnostics: &[LintDiagnostic],
        sources: &SourceMap,
    ) -> Result<FixSummary, String> {
        let mut per_file: HashMap<String, Vec<TextEdit>> = HashMap::new();
        let mut applied = 0usize;

        for diag in diagnostics {
            for fix in &diag.fixes {
                if fix.applicability != Applicability::MachineApplicable || fix.edits.is_empty() {
                    continue;
                }
                let mut valid = true;
                for edit in &fix.edits {
                    if sources.file(edit.span.file).is_none() {
                        valid = false;
                        break;
                    }
                }
                if !valid {
                    continue;
                }
                for edit in &fix.edits {
                    if let Some(file) = sources.file(edit.span.file) {
                        per_file
                            .entry(file.name.clone())
                            .or_default()
                            .push(edit.clone());
                    }
                }
                applied += 1;
            }
        }

        let mut files_modified = Vec::new();
        for (path, mut edits) in per_file {
            let Ok(original) = std::fs::read_to_string(&path) else {
                continue;
            };
            // Sort by start offset, drop overlaps.
            edits.sort_by_key(|e| (e.span.start, e.span.end));
            let mut non_overlapping: Vec<TextEdit> = Vec::with_capacity(edits.len());
            let mut last_end = 0u32;
            for edit in edits {
                if edit.span.start >= last_end
                    && edit.span.end <= u32::try_from(original.len()).unwrap_or(u32::MAX)
                {
                    last_end = edit.span.end;
                    non_overlapping.push(edit);
                }
            }
            if non_overlapping.is_empty() {
                continue;
            }
            let mut updated = original.clone();
            for edit in non_overlapping.into_iter().rev() {
                let start = usize::try_from(edit.span.start).unwrap_or(0);
                let end = usize::try_from(edit.span.end).unwrap_or(updated.len());
                if start <= end && end <= updated.len() {
                    updated.replace_range(start..end, &edit.replacement);
                }
            }
            if updated != original {
                std::fs::write(Path::new(&path), updated)
                    .map_err(|e| format!("cannot write `{path}`: {e}"))?;
                files_modified.push(path);
            }
        }
        files_modified.sort();
        Ok(FixSummary {
            applied,
            files_modified,
        })
    }
}
