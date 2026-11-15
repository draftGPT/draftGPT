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
mod tests {
    use super::*;

    #[tokio::test]
    async fn atomic_write_replaces_whole_file_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/x.help.json");
        write_atomic(&path, b"first").await.unwrap();
        write_atomic(&path, b"second").await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        let leftovers = std::fs::read_dir(dir.path().join("sub")).unwrap().count();
        assert_eq!(leftovers, 1, "no temp files left");
    }

    #[test]
    fn temp_paths_are_unique_and_hidden() {
        let p = Path::new("/c/.help-cache/16.21.help.json");
        let (a, b) = (temp_path(p), temp_path(p));
        assert_ne!(a, b);
        assert!(a.file_name().unwrap().to_string_lossy().starts_with('.'));
        assert_eq!(a.parent(), p.parent());
    }
}
