//! Hot-reload repository-owned document snapshots.

use std::fs;
use std::io;
use std::io::Read as _;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use aevia_registry_core::Error;

use crate::state::AppState;

const MAX_DOCUMENT_FILES: usize = 4096;
const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum decoded size of one chapter or module.
pub const MARKDOWN_LIMIT: u64 = 1024 * 1024;
/// Maximum size of the generated bilingual catalog.
pub const CATALOG_LIMIT: u64 = 32 * 1024 * 1024;

#[derive(Clone, Copy)]
enum Source {
    Spec,
    Stdlib,
    Manuals,
}

impl Source {
    const fn name(self) -> &'static str {
        match self {
            | Self::Spec => "spec",
            | Self::Stdlib => "stdlib",
            | Self::Manuals => "manuals",
        }
    }

    fn directory(
        self,
        state: &AppState,
    ) -> &Path {
        match self {
            | Self::Spec => &state.config.spec_dir,
            | Self::Stdlib => &state.config.stdlib_docs_dir,
            | Self::Manuals => &state.config.manuals_dir,
        }
    }

    const fn max_depth(self) -> usize {
        match self {
            | Self::Spec => 2,
            | Self::Stdlib => 1,
            | Self::Manuals => 3,
        }
    }

    async fn load(
        self,
        state: &AppState,
    ) -> Result<(), aevia_registry_core::Error> {
        match self {
            | Self::Spec => state.reload_spec().await.map(|_| ()),
            | Self::Stdlib => state.reload_stdlib_docs().await.map(|_| ()),
            | Self::Manuals => state.reload_manuals().await,
        }
    }
}

/// Watch the mounts independently so an unavailable source cannot stall the others.
pub fn spawn_reloader(state: &AppState) {
    for source in [Source::Spec, Source::Stdlib, Source::Manuals] {
        tokio::spawn(watch(state.clone(), source));
    }
}

#[allow(
    clippy::single_call_fn,
    reason = "one source watches and retries independently of other documentation roots"
)]
async fn watch(
    state: AppState,
    source: Source,
) {
    let directory = source.directory(&state).to_path_buf();
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut published = None;
    let mut observed = None;
    let mut last_error = None;
    let mut retry_at = tokio::time::Instant::now();
    loop {
        interval.tick().await;
        let path = directory.clone();
        let next =
            tokio::task::spawn_blocking(move || fingerprint(&path, source.max_depth())).await;
        let next = match next {
            | Ok(Ok(next)) => next,
            | Ok(Err(error)) => {
                report_error(source, &mut last_error, error.to_string());
                continue;
            },
            | Err(error) => {
                report_error(source, &mut last_error, error.to_string());
                continue;
            },
        };
        if observed.as_ref() != Some(&next) {
            observed = Some(next);
            // Observe a stable candidate across two polls before opening it.
            continue;
        }
        if published.as_ref() == Some(&next) || tokio::time::Instant::now() < retry_at {
            continue;
        }
        match source.load(&state).await {
            | Ok(()) => {
                if published.is_some() || last_error.is_some() {
                    tracing::info!(source = source.name(), "reloaded documentation");
                }
                published = Some(next);
                last_error = None;
            },
            | Err(error) => {
                report_error(source, &mut last_error, error.render());
                let now = tokio::time::Instant::now();
                retry_at = now.checked_add(Duration::from_secs(4)).unwrap_or(now);
            },
        }
    }
}

fn report_error(
    source: Source,
    last_error: &mut Option<String>,
    error: String,
) {
    if last_error.as_ref() != Some(&error) {
        tracing::warn!(source = source.name(), %error, "documentation reload deferred");
        *last_error = Some(error);
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FileStamp {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

#[allow(
    clippy::single_call_fn,
    reason = "a recursive filesystem snapshot has its own stable ordering"
)]
fn fingerprint(
    path: &Path,
    max_depth: usize,
) -> io::Result<Vec<FileStamp>> {
    let mut files = Vec::new();
    let mut remaining = MAX_DOCUMENT_FILES;
    collect(path, max_depth, true, &mut files, &mut remaining)?;
    if files
        .iter()
        .fold(0u64, |sum, stamp| sum.saturating_add(stamp.len))
        > MAX_SOURCE_BYTES
    {
        return Err(io::Error::other(
            "documentation source exceeds the total byte limit",
        ));
    }
    files.sort_unstable();
    Ok(files)
}

fn collect(
    path: &Path,
    depth: usize,
    root: bool,
    files: &mut Vec<FileStamp>,
    remaining: &mut usize,
) -> io::Result<()> {
    *remaining = remaining
        .checked_sub(1)
        .ok_or_else(|| io::Error::other("too many entries in the documentation source"))?;
    let metadata = match fs::metadata(path) {
        | Ok(metadata) => metadata,
        | Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        | Err(error) => return Err(error),
    };
    if metadata.is_dir() && depth > 0 {
        for entry in fs::read_dir(path)? {
            collect(
                &entry?.path(),
                depth.saturating_sub(1),
                false,
                files,
                remaining,
            )?;
        }
    } else if metadata.is_file()
        && (root
            || matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("md" | "json")
            ))
    {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt as _;

        files.push(FileStamp {
            path: path.to_path_buf(),
            len: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            identity: (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            ),
        });
    }
    Ok(())
}

/// Samples the inputs on both sides of a render so a concurrent deployment
/// cannot publish a snapshot assembled from files that changed while loading.
///
/// # Errors
/// Returns the filesystem or validation error for an inconsistent candidate.
pub fn checked_load<T>(
    path: &Path,
    depth: usize,
    code: &'static str,
    load: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    let stamp = || {
        fingerprint(path, depth).map_err(|error| {
            Error::new(
                code,
                format!(
                    "cannot inspect documentation source `{}`: {error}",
                    path.display()
                ),
            )
        })
    };
    let before = stamp()?;
    let candidate = load()?;
    if stamp()? != before {
        return Err(Error::new(
            code,
            "documentation files changed while loading; retrying",
        ));
    }
    Ok(candidate)
}

/// Reads one regular UTF-8 file up to the limit even if it grows after opening.
///
/// # Errors
/// Returns an IO error for non-files, invalid UTF-8 or oversized inputs.
pub fn read_document(
    path: &Path,
    limit: u64,
) -> io::Result<String> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(io::Error::other(
            "expected a regular document within the size limit",
        ));
    }
    let file = fs::File::open(path)?;
    let mut text = String::new();
    let bytes = file
        .take(limit.saturating_add(1))
        .read_to_string(&mut text)?;
    if u64::try_from(bytes).map_or(true, |size| size > limit) {
        return Err(io::Error::other("document exceeds the size limit"));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_changes_reject_the_candidate() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("spec.md");
        fs::write(&path, "# stable").unwrap();
        assert!(
            checked_load(root.path(), 1, "E_TEST", || {
                fs::write(&path, "# edited while loading").unwrap();
                Ok(())
            })
            .is_err()
        );
        assert_eq!(
            read_document(&path, 3).unwrap_err().kind(),
            io::ErrorKind::Other
        );
        assert!(checked_load(root.path(), 1, "E_TEST", || Ok(())).is_ok());
    }

    #[test]
    fn only_document_files_affect_the_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("book/en");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("01-start.md"), "first").unwrap();
        let first = fingerprint(root.path(), 3).unwrap();
        assert_eq!(first.len(), 1);
        fs::write(nested.join("01-start.md.tmp"), "ignored").unwrap();
        assert_eq!(fingerprint(root.path(), 3).unwrap(), first);
        fs::write(nested.join("01-start.md"), "second").unwrap();
        assert_ne!(fingerprint(root.path(), 3).unwrap(), first);
        assert!(fingerprint(root.path(), 2).unwrap().is_empty());
    }
}
