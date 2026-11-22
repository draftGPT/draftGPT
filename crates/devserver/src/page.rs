//! What the dev page receives: messages, and the redaction and checks applied
//! to events before they leave the process.

use std::collections::HashSet;

use capture::Status;
use capture::redact::Redactor;
use lcu::RawEvent;
use serde::Serialize;

use crate::check;

/// Messages pushed to the page over `/api/events`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PageMsg {
    Status(Status),
    Event {
        /// Redacted copy of the event.
        event: RawEvent,
        /// Set when typed parsing failed or met an unknown enum value.
        problem: Option<String>,
    },
    Notice {
        level: Level,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Info,
    Error,
}

/// Turns events into page messages. Has its own redactor, separate from each
/// recording's, because the page lives longer than any one recording.
#[derive(Debug, Default)]
pub struct PageFeed {
    redactor: Redactor,
    /// Problems already logged, so a value that persists for a whole draft
    /// (the session updates every second) is logged once.
    logged_problems: HashSet<String>,
}

impl PageFeed {
    /// Redacts the event, then checks it. Checking the redacted copy keeps raw
    /// values out of problem messages (which also go to the log).
    pub fn event(&mut self, ev: &RawEvent) -> PageMsg {
        let mut event = ev.clone();
        self.redactor.redact_at(&event.uri, &mut event.data);
        event.uri = self.redactor.redact_uri(&event.uri);
        let problem = check::check(&event);
        if let Some(p) = &problem
            && self.logged_problems.insert(p.clone())
        {
            tracing::warn!("{}: {p}", event.uri);
        }
        PageMsg::Event { event, problem }
    }

    /// Starts afresh for a new client session, so the fake map doesn't grow
    /// for as long as devserver runs.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lcu::{EventKind, SESSION_URI};
    use serde_json::json;

    #[test]
    fn events_are_redacted_and_checked() {
        let mut feed = PageFeed::default();
        let ev = RawEvent {
            t: 1,
            uri: SESSION_URI.into(),
            kind: EventKind::Update,
            data: json!({"timer": {"phase": "NEW"}, "myTeam": [{"puuid": "real", "cellId": 0}]}),
        };
        let PageMsg::Event { event, problem } = feed.event(&ev) else {
            panic!()
        };
        assert!(
            event.data["myTeam"][0]["puuid"]
                .as_str()
                .unwrap()
                .starts_with("redacted-")
        );
        assert!(problem.unwrap().contains("timer.phase"));
    }
}
