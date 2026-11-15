//! Saves the client's own schema (`/help`) next to a recording, so every
//! recording comes with the schema of the exact client build that produced it.
//!
//! `POST /Help` returns an index of every function, type and event. Each item
//! takes two more requests: `format=Full` (types and fields) and
//! `format=Console` (URL and HTTP method, which `Full` leaves out). That's
//! about 10000 for the whole schema, so by default only champ select and
//! gameflow items are fetched. `Scope::Full`
//! fetches everything. Dumps are cached per client version, so only the first
//! recording after a patch pays for the requests.

use std::path::{Path, PathBuf};

use lcu::LcuHttp;
use lcu::transport::TransportError;
use lcu::transport::http::HelpFormat;
use serde_json::{Map, Value, json};

use crate::fsutil;
use crate::recorder::slug;

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

/// Where per-client-version dumps are cached inside a captures directory.
/// Hidden, so it doesn't show up among recordings.
fn cache_dir(captures: &Path) -> PathBuf {
    captures.join(".help-cache")
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

/// Writes the relevant-scope dump for a recording, from the per-version cache
/// in `captures` when possible. Dumps with failed items aren't cached, so the
/// next recording retries them.
pub async fn dump_for_recording(
    http: &LcuHttp,
    dest: &Path,
    captures: &Path,
) -> Result<(), HelpDumpError> {
    let Ok(version) = http.client_version().await else {
        return dump(http, dest, Scope::Relevant, None).await.map(drop);
    };
    let cached = cache_dir(captures).join(format!("{}.help.json", slug(&version)));
    if !tokio::fs::try_exists(&cached).await.unwrap_or(false) {
        let stats = dump(http, dest, Scope::Relevant, Some(&version)).await?;
        if stats.failed == 0 {
            copy_atomic(dest, &cached).await?;
        }
        return Ok(());
    }
    copy_atomic(&cached, dest).await
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

async fn copy_atomic(from: &Path, to: &Path) -> Result<(), HelpDumpError> {
    let bytes = tokio::fs::read(from).await.map_err(io_err(from))?;
    write_atomic(to, &bytes).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relevance_filter() {
        assert!(is_relevant("GetLolChampSelectV1Session"));
        assert!(is_relevant("OnJsonApiEvent_lol-champ-select_v1_session"));
        assert!(is_relevant("LolGameflowGameflowPhase"));
        assert!(!is_relevant("GetLolChatV1Me"));
    }

    #[test]
    fn index_names_accepts_object_or_list() {
        let idx = json!({"types": {"A": "", "B": ""}, "events": ["E1"]});
        let mut t = index_names(&idx, "types");
        t.sort();
        assert_eq!(t, vec!["A", "B"]);
        assert_eq!(index_names(&idx, "events"), vec!["E1"]);
        assert!(index_names(&idx, "functions").is_empty());
    }

    #[test]
    fn cache_is_hidden_inside_captures() {
        assert_eq!(cache_dir(Path::new("c")), Path::new("c/.help-cache"));
    }
}
