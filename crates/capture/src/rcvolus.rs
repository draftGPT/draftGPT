//! Converts recordings made by RCVolus/lol-pick-ban-ui (MIT) into our format.
//!
//! Theirs is one JSON object: `dataPoints`, the champ select session polled
//! about once per second (`{isChampSelectActive, session, time}`, where
//! `session` is the client's 404 error body outside champ select), and
//! `summoners`, account details of the local player. We keep only the session.
//! The recording starts at the first poll inside champ select, which becomes
//! the snapshot (like a live recording, which never snapshots a missing
//! session); consecutive identical polls collapse into one event. `summoners`
//! is dropped, everything else is redacted like a live recording.

use serde::Deserialize;
use serde_json::Value;

use lcu::recording::{FORMAT_VERSION, Header};
use lcu::{EventKind, RawEvent, SESSION_URI};

use crate::redact::Redactor;

/// The header label of converted recordings. They have no client version.
pub const LABEL: &str = "rcvolus";

#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("not an RCVolus recording: {0}")]
    Format(#[from] serde_json::Error),
    #[error("recording has no data points")]
    Empty,
    #[error("recording never reaches champ select (no session in any data point)")]
    NoSession,
    #[error("data point {index}: bad time `{time}`: {source}")]
    Time {
        index: usize,
        time: String,
        source: jiff::Error,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Recording {
    data_points: Vec<DataPoint>,
}

#[derive(Deserialize)]
struct DataPoint {
    #[serde(default)]
    session: Value,
    time: String,
}

impl DataPoint {
    /// The session, or `None` for the error body (or nothing) polled outside
    /// champ select.
    fn session(&self) -> Option<&Value> {
        self.session.get("actions").map(|_| &self.session)
    }
}

/// Converts the text of an RCVolus recording into a header and redacted events.
pub fn convert(json: &str) -> Result<(Header, Vec<RawEvent>), ConvertError> {
    let rec: Recording = serde_json::from_str(json)?;
    if rec.data_points.is_empty() {
        return Err(ConvertError::Empty);
    }
    let first = rec
        .data_points
        .iter()
        .position(|dp| dp.session().is_some())
        .ok_or(ConvertError::NoSession)?;
    let start = parse_time(first, &rec.data_points[first].time)?;
    let mut redactor = Redactor::new();
    let mut events = Vec::new();
    let mut prev: Option<Option<&Value>> = None;
    let mut last_t = 0;
    for (index, dp) in rec.data_points.iter().enumerate().skip(first) {
        let session = dp.session();
        let kind = match (prev, session) {
            (None, _) => EventKind::Snapshot,
            (Some(p), s) if p == s => continue,
            (Some(None), Some(_)) => EventKind::Create,
            (Some(Some(_)), Some(_)) => EventKind::Update,
            (Some(Some(_)), None) => EventKind::Delete,
            (Some(None), None) => unreachable!("equal, skipped above"),
        };
        prev = Some(session);
        let at = parse_time(index, &dp.time)?;
        // Polls are in order; never let a clock that went backwards reorder them.
        let since_start = u64::try_from(at.duration_since(start).as_millis()).unwrap_or(0);
        last_t = last_t.max(since_start);
        let mut data = session.cloned().unwrap_or(Value::Null);
        redactor.redact(&mut data);
        events.push(RawEvent {
            t: last_t,
            uri: SESSION_URI.to_string(),
            kind,
            data,
        });
    }
    let header = Header {
        format_version: FORMAT_VERSION,
        client_version: None,
        started_at: start.to_string(),
        label: LABEL.to_string(),
    };
    Ok((header, events))
}

/// The recording as `.jsonl` text: the header line, then one event per line.
pub fn to_jsonl(header: &Header, events: &[RawEvent]) -> String {
    let mut out = serde_json::to_string(header).expect("header serializes");
    out.push('\n');
    for ev in events {
        out.push_str(&serde_json::to_string(ev).expect("event serializes"));
        out.push('\n');
    }
    out
}

fn parse_time(index: usize, time: &str) -> Result<jiff::Timestamp, ConvertError> {
    time.parse().map_err(|source| ConvertError::Time {
        index,
        time: time.to_string(),
        source,
    })
}

#[cfg(test)]
#[path = "../tests/unit/rcvolus.rs"]
mod tests;
