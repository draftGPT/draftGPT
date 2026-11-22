//! Saves the client's own schema (`/help`), one file per client build
//! (`client-<version>.help.json` next to the recordings), so every recording
//! has the schema of the exact build that produced it: its header names the
//! version.
//!
//! `POST /Help` returns an index of every function, type and event. Each item
//! takes two more requests: `format=Full` (types and fields) and
//! `format=Console` (URL and HTTP method, which `Full` leaves out). That's
//! about 10000 for the whole schema, so by default only champ select and
//! gameflow items are fetched. `Scope::Full`
//! fetches everything. Only the first recording after a patch pays for the
//! requests; later ones find the file already there.

use std::path::{Path, PathBuf};

use lcu::LcuHttp;
use lcu::recording::{help_path_for, help_path_for_version};
use lcu::transport::TransportError;
use lcu::transport::http::HelpFormat;
use serde_json::{Map, Value, json};

use crate::fsutil;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Champ select and gameflow items only.
    Relevant,
    Full,
}

#[derive(Debug, thiserror::Error)]
pub enum HelpDumpError {
    #[error("POST /Help failed")]
    Index(#[source] TransportError),
    #[error("writing {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// What a dump produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DumpStats {
    pub items: usize,
    /// Requests that failed (stored with their error).
    pub failed: usize,
}

const RELEVANT_PARTS: &[&str] = &["champselect", "champ-select", "gameflow"];

fn is_relevant(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    RELEVANT_PARTS.iter().any(|p| lower.contains(p))
}

/// Item names in one section of the index. The client returns an object keyed
/// by name; a plain list of names is accepted too.
fn index_names(index: &Value, section: &str) -> Vec<String> {
    match index.get(section) {
        Some(Value::Object(m)) => m.keys().cloned().collect(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

/// Fetches the schema and writes it to `path`. Items that fail are stored with
/// their error instead of aborting the dump.
pub async fn dump(
    http: &LcuHttp,
    path: &Path,
    scope: Scope,
    client_version: Option<&str>,
) -> Result<DumpStats, HelpDumpError> {
    let index = http.help_index().await.map_err(HelpDumpError::Index)?;

    let mut details = Map::new();
    let mut stats = DumpStats {
        items: 0,
        failed: 0,
    };
    for section in ["functions", "types", "events"] {
        let mut items = Map::new();
        for name in index_names(&index, section) {
            if scope == Scope::Relevant && !is_relevant(&name) {
                continue;
            }
            let mut entry = Map::new();
            for (key, format) in [("full", HelpFormat::Full), ("console", HelpFormat::Console)] {
                let value = match http.help_target(&name, format).await {
                    Ok(v) => v,
                    Err(e) => {
                        stats.failed += 1;
                        json!({ "error": e.to_string() })
                    }
                };
                entry.insert(key.to_string(), value);
            }
            items.insert(name, Value::Object(entry));
        }
        stats.items += items.len();
        details.insert(section.to_string(), Value::Object(items));
    }

    let out = json!({
        "client_version": client_version,
        "scope": match scope { Scope::Relevant => "relevant", Scope::Full => "full" },
        "index": index,
        "details": details,
    });
    let text = serde_json::to_vec_pretty(&out).expect("JSON values always serialize");
    write_atomic(path, &text).await?;
    Ok(stats)
}

/// What [`dump_for_recording`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelpOutcome {
    /// This client version's schema was already saved.
    AlreadySaved(PathBuf),
    /// Fetched and saved as this client version's schema.
    Saved(PathBuf),
    /// Saved next to the recording only: the client version was unknown, or
    /// some items failed (so the next recording tries the version file again).
    SavedForRecording { path: PathBuf, failed: usize },
}

/// Makes sure the relevant-scope schema for the recording at `recording`
/// exists: `client-<version>.help.json` in the recording's directory, fetched
/// only if it isn't there yet.
pub async fn dump_for_recording(
    http: &LcuHttp,
    recording: &Path,
) -> Result<HelpOutcome, HelpDumpError> {
    let fallback = help_path_for(recording);
    let Ok(version) = http.client_version().await else {
        let stats = dump(http, &fallback, Scope::Relevant, None).await?;
        return Ok(HelpOutcome::SavedForRecording {
            path: fallback,
            failed: stats.failed,
        });
    };
    let dir = recording.parent().unwrap_or(Path::new(""));
    let path = help_path_for_version(dir, &version);
    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return Ok(HelpOutcome::AlreadySaved(path));
    }
    let stats = dump(http, &fallback, Scope::Relevant, Some(&version)).await?;
    if stats.failed > 0 {
        return Ok(HelpOutcome::SavedForRecording {
            path: fallback,
            failed: stats.failed,
        });
    }
    // Complete: promote it to the shared file (rename is atomic on one volume).
    tokio::fs::rename(&fallback, &path)
        .await
        .map_err(io_err(&path))?;
    Ok(HelpOutcome::Saved(path))
}

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> HelpDumpError + '_ {
    move |source| HelpDumpError::Io {
        path: path.to_path_buf(),
        source,
    }
}

async fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), HelpDumpError> {
    fsutil::write_atomic(path, bytes)
        .await
        .map_err(io_err(path))
}

#[cfg(test)]
#[path = "../tests/unit/help_dump.rs"]
mod tests;
