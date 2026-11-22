//! Turns a stream of `SourceEvent`s into recordings. Shared by the `capture` CLI
//! and `devserver`, so recording behaves the same in both.
//!
//! The controller is synchronous and does no networking. It remembers the
//! latest event per URI, so a recording started mid-draft begins with the
//! current state (as `Snapshot` events at `t = 0`), and the last few wire
//! frames, so the wire file includes what happened just before the start
//! (e.g. the phase change that triggered auto mode). The state sweep and
//! `/help` dump for a new recording are the caller's job (see `extras::RecordingExtras`), since
//! only the caller has access to the live client.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use lcu::types::GameflowPhase;
use lcu::{
    CLASH_TOURNAMENT_STATE_INFO, CLASH_TOURNAMENT_SUMMARY, ClientInfo, EventKind, RawEvent,
    SourceEvent, WireFrame,
};
use serde::Serialize;

use crate::auto::{self, AutoAction};
use crate::recorder::{Recorder, RecordingInfo};

/// Wire frames kept from before a recording starts.
const WIRE_PREROLL: usize = 64;

pub struct Controller {
    dir: PathBuf,
    auto_enabled: bool,
    connection: ConnectionState,
    phase: GameflowPhase,
    latest: BTreeMap<String, RawEvent>,
    preroll: VecDeque<WireFrame>,
    active: Option<Active>,
    /// Why recording is off, if it is (e.g. replaying a recording).
    no_recording: Option<String>,
}

struct Active {
    recorder: Recorder,
    by_auto: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("already recording {0}")]
    AlreadyRecording(String),
    #[error("recording file: {0}")]
    Io(#[from] io::Error),
    #[error("recording is off: {0}")]
    Disabled(String),
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConnectionState {
    #[default]
    Waiting,
    Connected {
        client: ClientInfo,
    },
    Disconnected {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub connection: ConnectionState,
    pub phase: GameflowPhase,
    pub auto: bool,
    pub recording: Option<RecordingStatus>,
    /// Why recording is off, if it is; the page disables its controls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording_disabled: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordingStatus {
    #[serde(flatten)]
    pub info: RecordingInfo,
    pub by_auto: bool,
    pub wire_frames: usize,
}

/// Side effects of handling an event, for the caller to report or act on.
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    Started(RecordingInfo),
    Stopped(RecordingInfo),
    Error(String),
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Started(i) => write!(f, "recording started: {}", i.path.display()),
            Self::Stopped(i) => write!(
                f,
                "recording saved: {} ({} events)",
                i.path.display(),
                i.events
            ),
            Self::Error(e) => write!(f, "{e}"),
        }
    }
}

impl Controller {
    pub fn new(dir: impl Into<PathBuf>, auto_enabled: bool) -> Self {
        Self {
            dir: dir.into(),
            auto_enabled,
            connection: ConnectionState::Waiting,
            phase: GameflowPhase::None,
            latest: BTreeMap::new(),
            preroll: VecDeque::with_capacity(WIRE_PREROLL),
            active: None,
            no_recording: None,
        }
    }

    /// A controller that never records, neither by hand nor automatically
    /// (for sources that aren't a live client, such as a replay, whose
    /// recordings would only copy the source and pass for live captures).
    pub fn without_recording(dir: impl Into<PathBuf>, reason: impl Into<String>) -> Self {
        Self {
            no_recording: Some(reason.into()),
            ..Self::new(dir, false)
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn status(&self) -> Status {
        Status {
            connection: self.connection.clone(),
            phase: self.phase,
            auto: self.auto_enabled,
            recording: self.active.as_ref().map(|a| RecordingStatus {
                info: a.recorder.info().clone(),
                by_auto: a.by_auto,
                wire_frames: a.recorder.wire_frames(),
            }),
            recording_disabled: self.no_recording.clone(),
        }
    }

    /// Latest event per URI (current state of each core resource).
    pub fn latest(&self) -> impl Iterator<Item = &RawEvent> {
        self.latest.values()
    }

    pub fn set_auto(&mut self, enabled: bool) -> Vec<Notice> {
        // Auto stays off; the status says why (`recording_disabled`).
        if self.no_recording.is_some() {
            return Vec::new();
        }
        self.auto_enabled = enabled;
        // Turning auto on while already in champ select starts right away.
        self.apply_auto()
    }

    pub fn handle(&mut self, event: &SourceEvent) -> Vec<Notice> {
        match event {
            SourceEvent::Connected(info) => {
                self.connection = ConnectionState::Connected {
                    client: info.clone(),
                };
                Vec::new()
            }
            SourceEvent::Disconnected { reason } => {
                self.connection = ConnectionState::Disconnected {
                    reason: reason.clone(),
                };
                self.latest.clear();
                self.preroll.clear();
                self.phase = GameflowPhase::None;
                // The draft this auto recording was for is gone with the client.
                if self.active.as_ref().is_some_and(|a| a.by_auto) {
                    self.stop_notice().into_iter().collect()
                } else {
                    Vec::new()
                }
            }
            SourceEvent::Event(ev) => self.handle_event(ev),
            SourceEvent::Wire(frame) => self.handle_wire(frame),
        }
    }

    fn handle_event(&mut self, ev: &RawEvent) -> Vec<Notice> {
        let mut notices = Vec::new();
        if ev.kind == EventKind::Delete {
            self.latest.remove(&ev.uri);
        } else if crate::clash::is_scouting_snapshot(&ev.uri) {
            // Recorded below, but not state for the next recording.
        } else {
            self.latest.insert(ev.uri.clone(), ev.clone());
        }
        if let Some(active) = &mut self.active
            && let Err(e) = active.recorder.write(ev.clone())
        {
            notices.push(Notice::Error(format!("writing recording failed: {e}")));
        }
        if let Some(phase) = auto::phase_of(ev) {
            self.phase = phase;
            notices.extend(self.apply_auto());
        } else if is_clash_state_uri(&ev.uri) {
            notices.extend(self.apply_auto());
        }
        notices
    }

    fn handle_wire(&mut self, frame: &WireFrame) -> Vec<Notice> {
        let Some(active) = &mut self.active else {
            if self.preroll.len() == WIRE_PREROLL {
                self.preroll.pop_front();
            }
            self.preroll.push_back(frame.clone());
            return Vec::new();
        };
        match active.recorder.write_wire(frame) {
            Ok(()) => Vec::new(),
            Err(e) => vec![Notice::Error(format!(
                "writing wire file failed, continuing without it: {e}"
            ))],
        }
    }

    /// A Clash tournament the player is in is in lock-in, scouting or in game.
    pub fn clash_active(&self) -> bool {
        auto::clash_active(
            [
                CLASH_TOURNAMENT_SUMMARY.uri,
                CLASH_TOURNAMENT_STATE_INFO.uri,
            ]
            .iter()
            .filter_map(|uri| self.latest.get(*uri))
            .map(|ev| &ev.data),
        )
    }

    pub fn is_recording(&self) -> bool {
        self.active.is_some()
    }

    /// The latest data of one resource.
    pub fn latest_of(&self, uri: &str) -> Option<&RawEvent> {
        self.latest.get(uri)
    }

    fn apply_auto(&mut self) -> Vec<Notice> {
        let by_auto = self.active.as_ref().map(|a| a.by_auto);
        let clash = self.clash_active();
        let label = if clash { "clash" } else { "auto" };
        match auto::decide(self.auto_enabled, self.phase, clash, by_auto) {
            Some(AutoAction::Start) => vec![match self.start_inner(label, true) {
                Ok(info) => Notice::Started(info),
                Err(e) => Notice::Error(format!("couldn't start recording: {e}")),
            }],
            Some(AutoAction::Stop) => self.stop_notice().into_iter().collect(),
            None => Vec::new(),
        }
    }

    /// Starts a manual recording. Auto mode never stops manual recordings.
    pub fn start(&mut self, label: &str) -> Result<RecordingInfo, CaptureError> {
        if let Some(reason) = &self.no_recording {
            return Err(CaptureError::Disabled(reason.clone()));
        }
        if let Some(a) = &self.active {
            return Err(CaptureError::AlreadyRecording(
                a.recorder.info().name.clone(),
            ));
        }
        self.start_inner(label, false)
    }

    fn start_inner(&mut self, label: &str, by_auto: bool) -> Result<RecordingInfo, CaptureError> {
        let version = match &self.connection {
            ConnectionState::Connected { client } => client.version.clone(),
            _ => None,
        };
        let mut recorder = Recorder::create(&self.dir, label, version)?;
        for ev in self.latest.values() {
            recorder.write_snapshot(ev.clone())?;
        }
        let mut notice = None;
        for frame in self.preroll.drain(..) {
            if let Err(e) = recorder.write_preroll_wire(&frame) {
                notice = Some(e);
                break;
            }
        }
        if let Some(e) = notice {
            tracing::warn!("writing wire file failed, continuing without it: {e}");
        }
        let info = recorder.info().clone();
        tracing::info!("recording started: {}", info.path.display());
        self.active = Some(Active { recorder, by_auto });
        Ok(info)
    }

    /// Stops the current recording. `Ok(None)` if nothing was recording.
    pub fn stop(&mut self) -> Result<Option<RecordingInfo>, CaptureError> {
        let Some(active) = self.active.take() else {
            return Ok(None);
        };
        let info = active.recorder.finish()?;
        tracing::info!("recording stopped: {} ({} events)", info.name, info.events);
        Ok(Some(info))
    }

    fn stop_notice(&mut self) -> Option<Notice> {
        match self.stop() {
            Ok(info) => info.map(Notice::Stopped),
            Err(e) => Some(Notice::Error(format!("finishing recording failed: {e}"))),
        }
    }
}

fn is_clash_state_uri(uri: &str) -> bool {
    uri == CLASH_TOURNAMENT_SUMMARY.uri || uri == CLASH_TOURNAMENT_STATE_INFO.uri
}

#[cfg(test)]
#[path = "../tests/unit/controller.rs"]
mod tests;
