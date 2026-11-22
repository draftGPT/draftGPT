//! Small file helpers shared by the background writers.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Writes to a uniquely named temporary file and renames it, so readers never
/// see a partial file and concurrent writers don't clobber each other.
pub(crate) async fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let tmp = temp_path(path);
    tokio::fs::write(&tmp, bytes).await?;
    tokio::fs::rename(&tmp, path).await
}

/// A hidden sibling of `path`, unique per process and call.
fn temp_path(path: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{name}.{}.{n}.tmp", std::process::id()))
}

#[cfg(test)]
#[path = "../tests/unit/fsutil.rs"]
mod tests;
