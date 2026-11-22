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
mod tests {
    use super::*;
    use crate::redact::find_unredacted;
    use serde_json::json;

    fn session(champion: i64) -> Value {
        json!({
            "actions": [[{"actorCellId": 0, "championId": champion, "id": 1, "type": "pick"}]],
            "gameId": 4_123_456_789_u64,
            "myTeam": [{"cellId": 0, "summonerId": 76_838_801}],
            "chatDetails": {"chatRoomName": "abc@sec.pvp.net", "chatRoomPassword": "pw"},
            "timer": {"phase": "BAN_PICK"}
        })
    }

    fn recording(points: &[(Value, &str)]) -> String {
        let not_found =
            json!({"errorCode": "RPC_ERROR", "httpStatus": 404, "message": "No active delegate"});
        let dps: Vec<Value> = points
            .iter()
            .map(|(s, time)| {
                let active = !s.is_null();
                let s = if active { s.clone() } else { not_found.clone() };
                json!({"isChampSelectActive": active, "session": s, "time": time})
            })
            .collect();
        json!({
            "dataPoints": dps,
            "summoners": [{"displayName": "Someone", "puuid": "p-1", "summonerId": 76_838_801}]
        })
        .to_string()
    }

    #[test]
    fn starts_at_first_session_and_collapses_duplicate_polls() {
        let json = recording(&[
            (Value::Null, "2019-10-30T19:20:14.000Z"),
            (Value::Null, "2019-10-30T19:20:15.000Z"),
            (session(0), "2019-10-30T19:20:16.000Z"),
            (session(0), "2019-10-30T19:20:17.000Z"),
            (session(157), "2019-10-30T19:20:18.500Z"),
            (Value::Null, "2019-10-30T19:20:20.000Z"),
            (Value::Null, "2019-10-30T19:20:21.000Z"),
            (session(1), "2019-10-30T19:20:22.000Z"),
        ]);
        let (header, events) = convert(&json).unwrap();
        assert_eq!(header.label, LABEL);
        assert_eq!(header.client_version, None);
        assert_eq!(header.started_at, "2019-10-30T19:20:16Z");
        let got: Vec<(u64, EventKind)> = events.iter().map(|e| (e.t, e.kind)).collect();
        assert_eq!(
            got,
            [
                (0, EventKind::Snapshot),
                (2_500, EventKind::Update),
                (4_000, EventKind::Delete),
                (6_000, EventKind::Create),
            ]
        );
        assert!(!events[0].data.is_null());
        assert!(events[2].data.is_null());
        assert!(events.iter().all(|e| e.uri == SESSION_URI));
        assert_eq!(events[1].data["actions"][0][0]["championId"], json!(157));
    }

    #[test]
    fn times_never_go_backwards() {
        let json = recording(&[
            (session(0), "2019-10-30T19:20:10.000Z"),
            (session(1), "2019-10-30T19:20:15.000Z"),
            (session(2), "2019-10-30T19:20:12.000Z"),
            (session(3), "2019-10-30T19:20:09.000Z"),
        ]);
        let (_, events) = convert(&json).unwrap();
        let ts: Vec<u64> = events.iter().map(|e| e.t).collect();
        assert_eq!(ts, [0, 5_000, 5_000, 5_000]);
    }

    #[test]
    fn output_is_redacted_and_drops_summoners() {
        let json = recording(&[(session(1), "2022-02-04T15:11:36.595Z")]);
        let (header, events) = convert(&json).unwrap();
        for ev in &events {
            assert_eq!(find_unredacted(&ev.data), Vec::<String>::new());
        }
        let text = to_jsonl(&header, &events);
        for leak in [
            "Someone",
            "p-1",
            "76838801",
            "4123456789",
            "pvp.net",
            "\"pw\"",
        ] {
            assert!(!text.contains(leak), "{leak} in {text}");
        }
        let (h, evs) = lcu::recording::read(text.as_bytes()).unwrap();
        assert_eq!(h, header);
        assert_eq!(evs.count(), 1);
    }

    #[test]
    fn rejects_other_files() {
        assert!(matches!(convert("{}"), Err(ConvertError::Format(_))));
        assert!(matches!(
            convert(r#"{"dataPoints": []}"#),
            Err(ConvertError::Empty)
        ));
        let never = recording(&[(Value::Null, "2019-10-30T19:20:14.000Z")]);
        assert!(matches!(convert(&never), Err(ConvertError::NoSession)));
        let bad_time = r#"{"dataPoints": [{"session": {"actions": []}, "time": "yesterday"}]}"#;
        assert!(matches!(
            convert(bad_time),
            Err(ConvertError::Time { index: 0, .. })
        ));
    }
}
