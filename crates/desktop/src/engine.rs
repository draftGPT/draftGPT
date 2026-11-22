//! Source events → draft → recommendations → `draft-update` events for the
//! page. The same loop serves the live client and replays.

use std::sync::Mutex;
use std::time::Instant;

use draft_core::{DraftState, Recommendations, Recommender, StatsRecommender, Tracker};
use futures::StreamExt;
use lcu::{SourceEvent, SourceStream};
use serde::Serialize;

/// What the page shows (`ui/desktop/src/source.ts` mirrors it).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    /// Increases with every update; the page ignores one older than it has.
    pub seq: u64,
    /// The draft, its timer counted down to when this update was sent.
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
#[derive(Debug)]
pub struct Shared {
    inner: Mutex<Inner>,
}

#[derive(Debug)]
struct Inner {
    update: Update,
    /// When `update.draft` last changed: its timer is as of then.
    drafted_at: Instant,
}

impl Inner {
    /// The update as of now: the timer has kept running since the draft came.
    fn current(&self) -> Update {
        let mut u = self.update.clone();
        let since = i64::try_from(self.drafted_at.elapsed().as_millis()).unwrap_or(i64::MAX);
        if let Some(left) = u.draft.as_mut().and_then(|d| d.timer.remaining_ms.as_mut()) {
            *left = left.saturating_sub(since).max(0);
        }
        u
    }
}

impl Shared {
    pub fn new(floating: bool, status: &str) -> Self {
        Self {
            inner: Mutex::new(Inner {
                update: Update {
                    status: status.to_string(),
                    floating,
                    can_dock: cfg!(windows),
                    ..Update::default()
                },
                drafted_at: Instant::now(),
            }),
        }
    }

    /// The latest update, timer counted down to now.
    pub fn latest(&self) -> Update {
        lock(&self.inner).current()
    }

    /// The draft as the engine last set it (timer as received).
    pub fn draft(&self) -> Option<DraftState> {
        lock(&self.inner).update.draft.clone()
    }

    pub fn has_draft(&self) -> bool {
        lock(&self.inner).update.draft.is_some()
    }

    pub fn floating(&self) -> bool {
        lock(&self.inner).update.floating
    }

    /// Changes the latest update and returns it as of now, with the next
    /// sequence number.
    pub fn change(&self, f: impl FnOnce(&mut Update)) -> Update {
        let mut inner = lock(&self.inner);
        let before = inner.update.draft.clone();
        f(&mut inner.update);
        if inner.update.draft != before {
            inner.drafted_at = Instant::now();
        }
        inner.update.seq += 1;
        inner.current()
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
                if draft == shared.draft() {
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
#[path = "../tests/unit/engine.rs"]
mod tests;
