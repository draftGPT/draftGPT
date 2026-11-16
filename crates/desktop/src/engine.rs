//! Source events → draft → recommendations → `draft-update` events for the
//! page. The same loop serves the live client and replays.

use std::sync::Mutex;

use draft_core::{DraftState, Recommendations, Recommender, StatsRecommender, Tracker};
use futures::StreamExt;
use lcu::{SourceEvent, SourceStream};
use serde::Serialize;

/// What the page shows (`ui/desktop/src/source.ts` mirrors it).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub draft: Option<DraftState>,
    pub recs: Option<Recommendations>,
    /// Connection state in words.
    pub status: String,
    /// The window floats instead of following the client window.
    pub floating: bool,
    /// Docking works here (Windows only); the page hides the toggle otherwise.
    pub can_dock: bool,
}

/// The latest update, shared by the engine, the commands and the dock.
#[derive(Debug, Default)]
pub struct Shared {
    latest: Mutex<Update>,
}

impl Shared {
    pub fn new(floating: bool, status: &str) -> Self {
        Self {
            latest: Mutex::new(Update {
                status: status.to_string(),
                floating,
                can_dock: cfg!(windows),
                ..Update::default()
            }),
        }
    }

    pub fn latest(&self) -> Update {
        lock(&self.latest).clone()
    }

    /// Changes the latest update and returns the new one.
    pub fn change(&self, f: impl FnOnce(&mut Update)) -> Update {
        let mut u = lock(&self.latest);
        f(&mut u);
        u.clone()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Turns source events into updates; `emit` is called with each changed one.
pub struct Engine {
    tracker: Tracker,
    recommender: StatsRecommender,
    replay: Option<String>,
}

impl Engine {
    /// `replay`: the recording's name, for the status line.
    pub fn new(recommender: StatsRecommender, replay: Option<String>) -> Self {
        Self {
            tracker: Tracker::new(),
            recommender,
            replay,
        }
    }

    /// Applies one source event to `shared`; returns the update if anything
    /// the page shows changed.
    pub fn handle(&mut self, ev: &SourceEvent, shared: &Shared) -> Option<Update> {
        match ev {
            SourceEvent::Connected(info) => {
                let status = match &self.replay {
                    Some(name) => format!("Replaying {name}"),
                    None => match &info.version {
                        Some(v) => format!("League client connected · {v}"),
                        None => "League client connected".to_string(),
                    },
                };
                Some(shared.change(|u| u.status = status))
            }
            SourceEvent::Disconnected { reason } => {
                // A new connection starts from fresh snapshots.
                self.tracker = Tracker::new();
                Some(shared.change(|u| {
                    u.status.clone_from(reason);
                    u.draft = None;
                    u.recs = None;
                }))
            }
            SourceEvent::Event(raw) => {
                if !self.tracker.apply(raw) {
                    return None;
                }
                let draft = self.tracker.state();
                if draft == shared.latest().draft {
                    return None;
                }
                let recs = draft.as_ref().map(|d| self.recommender.recommend(d));
                Some(shared.change(|u| {
                    u.draft = draft;
                    u.recs = recs;
                }))
            }
            SourceEvent::Wire(_) => None,
        }
    }

    /// Runs until the source ends (a finished replay; the live source never
    /// ends on its own).
    pub async fn run(mut self, mut stream: SourceStream, shared: &Shared, emit: impl Fn(&Update)) {
        while let Some(ev) = stream.next().await {
            if let Some(u) = self.handle(&ev, shared) {
                emit(&u);
            }
        }
        if let Some(name) = &self.replay {
            let u = shared.change(|u| u.status = format!("Replay of {name} finished"));
            emit(&u);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lcu::recording::read_file;
    use lcu::{ClientInfo, EventKind, RawEvent};

    fn recommender() -> StatsRecommender {
        StatsRecommender::from_json(include_str!("../../../data/stats/16.22.json")).unwrap()
    }

    #[test]
    fn a_replayed_fixture_produces_drafts_with_recommendations_then_ends() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/16_22_draft_ban_and_pick_timeout_autolock.jsonl"
        );
        let (_, events) = read_file(path.as_ref()).unwrap();
        let shared = Shared::new(true, "Starting");
        let mut engine = Engine::new(recommender(), Some("x".into()));
        let connected = SourceEvent::Connected(ClientInfo::default());
        assert_eq!(
            engine.handle(&connected, &shared).unwrap().status,
            "Replaying x"
        );
        let updates: Vec<Update> = events
            .into_iter()
            .filter_map(|e| engine.handle(&SourceEvent::Event(e), &shared))
            .collect();
        assert!(updates.len() > 3);
        assert!(
            updates
                .iter()
                .filter(|u| u.draft.is_some())
                .all(|u| u.recs.is_some())
        );
        assert!(
            updates.last().unwrap().draft.is_none(),
            "champ select ended"
        );
    }

    #[test]
    fn disconnect_clears_the_draft_and_unchanged_events_emit_nothing() {
        let shared = Shared::new(false, "Starting");
        let mut engine = Engine::new(recommender(), None);
        let session = SourceEvent::Event(RawEvent {
            t: 0,
            uri: lcu::SESSION_URI.into(),
            kind: EventKind::Update,
            data: serde_json::json!({"myTeam": [{"cellId": 0}]}),
        });
        assert!(engine.handle(&session, &shared).unwrap().draft.is_some());
        assert!(
            engine.handle(&session, &shared).is_none(),
            "same draft again"
        );
        let gone = SourceEvent::Disconnected {
            reason: "League client closed".into(),
        };
        let u = engine.handle(&gone, &shared).unwrap();
        assert_eq!((u.draft, u.status.as_str()), (None, "League client closed"));
        assert!(!u.floating);
    }
}
