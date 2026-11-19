//! Hot-reload repository-owned document snapshots.

use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use crate::state::AppState;

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
    inode: u64,
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
    collect(path, max_depth, true, &mut files)?;
    files.sort_unstable();
    Ok(files)
}

fn collect(
    path: &Path,
    depth: usize,
    root: bool,
    files: &mut Vec<FileStamp>,
) -> io::Result<()> {
    let metadata = match fs::metadata(path) {
        | Ok(metadata) => metadata,
        | Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        | Err(error) => return Err(error),
    };
    if metadata.is_dir() && depth > 0 {
        for entry in fs::read_dir(path)? {
            collect(&entry?.path(), depth.saturating_sub(1), false, files)?;
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
            inode: metadata.ino(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
