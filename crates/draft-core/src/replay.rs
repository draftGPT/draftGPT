//! A whole recording as a list of drafts, for stepping through it (the web
//! drafter's replay). Live input uses [`Tracker`] event by event instead.

use lcu_model::recording::{self, RecordingError};
use serde::Serialize;

use crate::{DraftState, Tracker};

/// One step of a replay: the draft after an event that changed it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ReplayStep {
    /// Milliseconds since the recording started.
    pub t: u64,
    pub draft: DraftState,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Replay {
    pub label: String,
    pub client_version: Option<String>,
    pub steps: Vec<ReplayStep>,
}

/// Reads a recording (`.jsonl` text) into the drafts it goes through. A step
/// that only differs from the previous one in the timer is left out; so are
/// the moments outside champ select.
pub fn replay(text: &str) -> Result<Replay, RecordingError> {
    let (header, events) = recording::read(text.as_bytes())?;
    let mut tracker = Tracker::new();
    let mut steps: Vec<ReplayStep> = Vec::new();
    for ev in events {
        let ev = ev?;
        if !tracker.apply(&ev) {
            continue;
        }
        let Some(draft) = tracker.state() else {
            continue;
        };
        let same = steps.last().is_some_and(|last| {
            DraftState {
                timer: draft.timer.clone(),
                ..last.draft.clone()
            } == draft
        });
        if !same {
            steps.push(ReplayStep { t: ev.t, draft });
        }
    }
    Ok(Replay {
        label: header.label,
        client_version: header.client_version,
        steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixture_replays_into_distinct_drafts() {
        let text =
            include_str!("../../../fixtures/16_22_draft_ban_and_pick_timeout_autolock.jsonl");
        let r = replay(text).unwrap();
        assert!(r.steps.len() > 5, "{}", r.steps.len());
        assert!(r.steps.windows(2).all(|w| w[0].t <= w[1].t));
        assert!(r.steps.windows(2).all(|w| {
            DraftState {
                timer: w[1].draft.timer.clone(),
                ..w[0].draft.clone()
            } != w[1].draft
        }));
        assert!(r.client_version.is_some());
    }

    #[test]
    fn a_broken_recording_is_an_error() {
        assert!(replay("").is_err());
        assert!(
            replay("{\"format_version\":1,\"started_at\":\"x\",\"label\":\"x\"}\nnot json")
                .is_err()
        );
    }
}
