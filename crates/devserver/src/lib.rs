//! Dev server for testing capture: runs an event source and the capture
//! controller, and serves a page that shows both.
//!
//! Binds to 127.0.0.1 only, accepts requests only for its own host name and
//! origin (so other websites can't drive it or read from it), redacts event
//! data before it reaches the page, never proxies requests to the LCU and never
//! exposes its credentials.

pub mod api;
pub mod check;
pub mod page;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use capture::{CaptureError, Controller, Notice, RecordingExtras, RecordingInfo, Status};
use futures::StreamExt;
use lcu::{LiveHandle, SourceEvent, SourceStream};
use tokio::sync::broadcast;

pub use api::router;
pub use page::{Level, PageMsg};

use page::PageFeed;

pub struct AppState {
    controller: Mutex<Controller>,
    feed: Mutex<PageFeed>,
    live: Option<LiveHandle>,
    extras: Mutex<RecordingExtras>,
    tx: broadcast::Sender<PageMsg>,
}

impl AppState {
    /// `live` gives access to the client for the per-recording extras (state
    /// sweep, `/help` dump); `None` for sources without a real client.
    pub fn new(
        captures_dir: impl Into<PathBuf>,
        auto: bool,
        live: Option<LiveHandle>,
    ) -> Arc<Self> {
        let dir = captures_dir.into();
        let (tx, _) = broadcast::channel(1024);
        Arc::new(Self {
            extras: Mutex::new(RecordingExtras::new()),
            controller: Mutex::new(Controller::new(dir, auto)),
            feed: Mutex::new(PageFeed::default()),
            live,
            tx,
        })
    }

    pub fn status(&self) -> Status {
        self.controller().status()
    }

    pub fn start(&self, label: &str) -> Result<RecordingInfo, CaptureError> {
        let info = self.controller().start(label)?;
        self.after(vec![Notice::Started(info.clone())]);
        Ok(info)
    }

    pub fn stop(&self) -> Result<Option<RecordingInfo>, CaptureError> {
        let info = self.controller().stop()?;
        if let Some(i) = &info {
            self.after(vec![Notice::Stopped(i.clone())]);
        }
        Ok(info)
    }

    pub fn set_auto(&self, on: bool) -> Status {
        let notices = self.controller().set_auto(on);
        self.after(notices);
        self.status()
    }

    /// The page's view on connect (status, then the latest state of each
    /// resource) and a receiver for everything after. Subscribing first means
    /// nothing falls in between.
    pub fn subscribe(&self) -> (Vec<PageMsg>, broadcast::Receiver<PageMsg>) {
        let rx = self.tx.subscribe();
        let (status, mut latest) = {
            let c = self.controller();
            (c.status(), c.latest().cloned().collect::<Vec<_>>())
        };
        // `latest()` is in URI order; the page's log should read in time order.
        latest.sort_by_key(|ev| ev.t);
        let mut feed = lock(&self.feed);
        let initial = std::iter::once(PageMsg::Status(status))
            .chain(latest.iter().map(|ev| feed.event(ev)))
            .collect();
        (initial, rx)
    }

    pub fn captures_dir(&self) -> PathBuf {
        self.controller().dir().to_path_buf()
    }

    /// Finishes any recording in progress and waits briefly for the sweep and
    /// `/help` dump. Stop the source task first so nothing new starts meanwhile.
    pub async fn shutdown(&self) {
        if let Err(e) = self.controller().stop() {
            tracing::error!("finishing recording on exit failed: {e}");
        }
        let extras = std::mem::take(&mut *lock(&self.extras));
        extras.finish().await;
    }

    /// Shows a message on the page.
    pub fn notice(&self, level: Level, message: impl Into<String>) {
        self.broadcast(PageMsg::Notice {
            level,
            message: message.into(),
        });
    }

    fn controller(&self) -> MutexGuard<'_, Controller> {
        lock(&self.controller)
    }

    fn broadcast(&self, msg: PageMsg) {
        // No receivers just means no page is open.
        let _ = self.tx.send(msg);
    }

    /// Acts on notices (a new recording gets its sweep and `/help` dump), reports them to
    /// the page, and sends the updated status.
    fn after(&self, notices: Vec<Notice>) {
        for n in notices {
            let level = match &n {
                Notice::Started(info) => {
                    if let Some(http) = self.live.as_ref().and_then(LiveHandle::http) {
                        lock(&self.extras).spawn(http, info);
                    }
                    Level::Info
                }
                Notice::Stopped(_) => Level::Info,
                Notice::Error(e) => {
                    tracing::error!("{e}");
                    Level::Error
                }
            };
            self.broadcast(PageMsg::Notice {
                level,
                message: n.to_string(),
            });
        }
        self.broadcast(PageMsg::Status(self.status()));
    }
}

/// A panic while holding one of these locks leaves plain data behind; keep going.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Feeds a source into the controller and the page until the source ends.
/// Locks are only held for synchronous work, never across an await.
pub async fn run_source(state: Arc<AppState>, mut stream: SourceStream) {
    while let Some(ev) = stream.next().await {
        let notices = state.controller().handle(&ev);
        match &ev {
            SourceEvent::Event(raw) => {
                let msg = lock(&state.feed).event(raw);
                state.broadcast(msg);
            }
            SourceEvent::Disconnected { .. } => lock(&state.feed).reset(),
            SourceEvent::Connected(_) | SourceEvent::Wire(_) => {}
        }
        // Wire frames only go to disk (there are too many for the page) and
        // change nothing the page shows, unless they caused a notice. Every
        // other event updates the status (connection, phase, event count).
        if !notices.is_empty() || !matches!(ev, SourceEvent::Wire(_)) {
            state.after(notices);
        }
    }
}
