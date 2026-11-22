//! Recording file format, shared by the recorder (`capture`) and replay.
//!
//! A recording `<name>.jsonl` is a [`Header`] line followed by one
//! [`RawEvent`] per line, with `t` in milliseconds since the start of the
//! recording. In the same directory:
//! - `client-<version>.help.json`: the client's `/help` schema, one per client
//!   build, shared by every recording whose header has that `client_version`
//!   (see [`help_path_for_version`]). Only when the version is unknown or the
//!   dump came back incomplete is it written per recording, `<name>.help.json`.
//! - `<name>.wire.jsonl`: every raw WebSocket frame received while recording,
//!   one [`WireLine`] per line. Local only, never committed.
//! - `<name>.sweep.json`: a snapshot of client state (GETs of parameterless
//!   endpoints), for the first recording of each client launch. Local only,
//!   never committed.

use std::io::BufRead;
use std::marker::PhantomData;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::raw::RawEvent;

pub const FORMAT_VERSION: u32 = 1;
pub const EXTENSION: &str = "jsonl";
pub const HELP_SUFFIX: &str = ".help.json";
pub const WIRE_SUFFIX: &str = ".wire.jsonl";
pub const SWEEP_SUFFIX: &str = ".sweep.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub format_version: u32,
    pub client_version: Option<String>,
    /// RFC 3339 timestamp.
    pub started_at: String,
    pub label: String,
}

/// One line of a wire file: the frame as received (redacted), or, for a frame
/// that wasn't JSON and so couldn't be redacted, only its size and the error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireLine {
    /// Milliseconds since the start of the recording.
    pub t: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unparsable: Option<Unparsable>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unparsable {
    pub bytes: usize,
    pub error: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RecordingError {
    #[error("reading recording: {0}")]
    Io(#[from] std::io::Error),
    #[error("recording is empty (no header line)")]
    Empty,
    #[error("recording format version {0} is newer than this build reads ({FORMAT_VERSION})")]
    UnsupportedVersion(u32),
    #[error("line {line}: {source}")]
    Json {
        line: usize,
        source: serde_json::Error,
    },
}

/// JSON-lines iterator: one `T` per non-empty line, with errors carrying the
/// line number. A named type so sources (e.g. replay) can store it.
pub struct JsonLines<R, T> {
    lines: std::io::Lines<R>,
    line: usize,
    _item: PhantomData<fn() -> T>,
}

impl<R: BufRead, T> JsonLines<R, T> {
    pub fn new(reader: R) -> Self {
        Self {
            lines: reader.lines(),
            line: 0,
            _item: PhantomData,
        }
    }

    /// Line number of the item returned last (1-based).
    pub fn line(&self) -> usize {
        self.line
    }
}

impl<R: BufRead, T: DeserializeOwned> Iterator for JsonLines<R, T> {
    type Item = Result<T, RecordingError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.line += 1;
            let text = match self.lines.next()? {
                Ok(t) => t,
                Err(e) => return Some(Err(e.into())),
            };
            // A byte order mark (some Windows editors add one) isn't JSON.
            let text = if self.line == 1 {
                text.strip_prefix('\u{feff}')
                    .map(str::to_owned)
                    .unwrap_or(text)
            } else {
                text
            };
            if text.trim().is_empty() {
                continue;
            }
            let line = self.line;
            return Some(
                serde_json::from_str(&text).map_err(|source| RecordingError::Json { line, source }),
            );
        }
    }
}

/// Events of a recording, after its header.
pub type Events<R> = JsonLines<R, RawEvent>;

/// Reads a recording from any buffered reader: the header, then the events.
pub fn read<R: BufRead>(reader: R) -> Result<(Header, Events<R>), RecordingError> {
    let mut lines = JsonLines::<R, Header>::new(reader);
    let header: Header = lines.next().ok_or(RecordingError::Empty)??;
    if header.format_version > FORMAT_VERSION {
        return Err(RecordingError::UnsupportedVersion(header.format_version));
    }
    let events = JsonLines {
        lines: lines.lines,
        line: lines.line,
        _item: PhantomData,
    };
    Ok((header, events))
}

/// Reads a wire file (no header; one [`WireLine`] per line).
pub fn read_wire<R: BufRead>(reader: R) -> JsonLines<R, WireLine> {
    JsonLines::new(reader)
}

/// Reads a whole recording file into memory.
pub fn read_file(path: &Path) -> Result<(Header, Vec<RawEvent>), RecordingError> {
    let file = std::io::BufReader::new(std::fs::File::open(path)?);
    let (header, events) = read(file)?;
    Ok((header, events.collect::<Result<_, _>>()?))
}

/// Reads a whole wire file into memory.
pub fn read_wire_file(path: &Path) -> Result<Vec<WireLine>, RecordingError> {
    let file = std::io::BufReader::new(std::fs::File::open(path)?);
    read_wire(file).collect()
}

/// `<dir>/<name>.help.json` for a recording at `<dir>/<name>.jsonl`: the
/// fallback when there's no per-version schema (see [`help_path_for_version`]).
pub fn help_path_for(recording: &Path) -> std::path::PathBuf {
    sibling(recording, HELP_SUFFIX)
}

/// `<dir>/client-<version>.help.json`, the `/help` schema shared by every
/// recording in `dir` from client build `version` (`16.22.834.8910` →
/// `client-16-22-834-8910.help.json`).
pub fn help_path_for_version(dir: &Path, version: &str) -> std::path::PathBuf {
    let slug: String = version
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    dir.join(format!("client-{slug}{HELP_SUFFIX}"))
}

/// `<dir>/<name>.wire.jsonl` for a recording at `<dir>/<name>.jsonl`.
pub fn wire_path_for(recording: &Path) -> std::path::PathBuf {
    sibling(recording, WIRE_SUFFIX)
}

/// `<dir>/<name>.sweep.json` for a recording at `<dir>/<name>.jsonl`.
pub fn sweep_path_for(recording: &Path) -> std::path::PathBuf {
    sibling(recording, SWEEP_SUFFIX)
}

fn sibling(recording: &Path, suffix: &str) -> std::path::PathBuf {
    let stem = recording.file_stem().unwrap_or_default().to_string_lossy();
    recording.with_file_name(format!("{stem}{suffix}"))
}

#[cfg(test)]
#[path = "../tests/unit/recording.rs"]
mod tests;
