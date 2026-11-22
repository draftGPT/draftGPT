//! `RawEvent`: the unit of capture and replay. Untyped on purpose so a schema
//! change after a patch can never lose recorded data.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawEvent {
    /// Milliseconds on the source's clock. Only differences between events of
    /// the same source are meaningful: live sources use Unix time, recordings
    /// use time since the start of the recording.
    pub t: u64,
    pub uri: String,
    pub kind: EventKind,
    pub data: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    Create,
    Update,
    Delete,
    /// Current state fetched with a GET (on connect, or at the start of a
    /// recording) rather than pushed by the client.
    Snapshot,
}

#[cfg(test)]
#[path = "../tests/unit/raw.rs"]
mod tests;
