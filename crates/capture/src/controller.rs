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
use lcu::{ClientInfo, EventKind, RawEvent, SourceEvent, WireFrame};
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
        }
    }

    /// Latest event per URI (current state of each core resource).
    pub fn latest(&self) -> impl Iterator<Item = &RawEvent> {
        self.latest.values()
    }

    pub fn set_auto(&mut self, enabled: bool) -> Vec<Notice> {
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

    fn apply_auto(&mut self) -> Vec<Notice> {
        let by_auto = self.active.as_ref().map(|a| a.by_auto);
        match auto::decide(self.auto_enabled, self.phase, by_auto) {
            Some(AutoAction::Start) => vec![match self.start_inner("auto", true) {
                Ok(info) => Notice::Started(info),
                Err(e) => Notice::Error(format!("couldn't start recording: {e}")),
            }],
            Some(AutoAction::Stop) => self.stop_notice().into_iter().collect(),
            None => Vec::new(),
        }
    }

    /// Starts a manual recording. Auto mode never stops manual recordings.
    pub fn start(&mut self, label: &str) -> Result<RecordingInfo, CaptureError> {
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
            if let Err(e) = recorder.write_wire(&frame) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redact::find_unredacted;
    use lcu::recording::{read_file, read_wire_file, wire_path_for};
    use lcu::{GAMEFLOW_PHASE_URI, SESSION_URI};
    use serde_json::json;

    /// Events carry increasing source times starting at 1_000_000.
    struct Feed {
        t: u64,
    }

    impl Feed {
        fn new() -> Self {
            Self { t: 1_000_000 }
        }

        fn next_t(&mut self) -> u64 {
            self.t += 100;
            self.t
        }

        fn event(&mut self, uri: &str, kind: EventKind, data: serde_json::Value) -> SourceEvent {
            SourceEvent::Event(RawEvent {
                t: self.next_t(),
                uri: uri.into(),
                kind,
                data,
            })
        }

        fn phase(&mut self, p: &str) -> SourceEvent {
            self.event(GAMEFLOW_PHASE_URI, EventKind::Update, json!(p))
        }

        fn session(&mut self, champ: i64) -> SourceEvent {
            self.event(
                SESSION_URI,
                EventKind::Update,
                json!({"myTeam": [{"cellId": 0, "championId": champ,
                       "summonerId": 123_456_789_012_u64, "puuid": "real-puuid"}]}),
            )
        }

        fn wire(&mut self, text: &str) -> SourceEvent {
            SourceEvent::Wire(WireFrame {
                t: self.next_t(),
                text: text.into(),
            })
        }
    }

    fn connected() -> SourceEvent {
        SourceEvent::Connected(ClientInfo {
            port: Some(1),
            pid: None,
            version: Some("16.21.1".into()),
        })
    }

    fn stopped_path(notices: &[Notice]) -> PathBuf {
        notices
            .iter()
            .find_map(|n| match n {
                Notice::Stopped(i) => Some(i.path.clone()),
                _ => None,
            })
            .expect("a Stopped notice")
    }

    #[test]
    fn auto_records_one_champ_select() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = Feed::new();
        let mut c = Controller::new(dir.path(), true);
        c.handle(&connected());
        assert!(c.handle(&f.phase("Lobby")).is_empty());

        let n = c.handle(&f.phase("ChampSelect"));
        assert!(matches!(n.as_slice(), [Notice::Started(_)]), "{n:?}");
        c.handle(&f.session(0));
        c.handle(&f.session(103));
        let path = stopped_path(&c.handle(&f.phase("GameStart")));
        assert!(c.status().recording.is_none());

        let (header, events) = read_file(&path).unwrap();
        assert_eq!(header.client_version.as_deref(), Some("16.21.1"));
        let kinds: Vec<_> = events.iter().map(|e| (e.kind, e.uri.as_str())).collect();
        assert_eq!(
            kinds,
            vec![
                (EventKind::Snapshot, GAMEFLOW_PHASE_URI),
                (EventKind::Update, SESSION_URI),
                (EventKind::Update, SESSION_URI),
                (EventKind::Update, GAMEFLOW_PHASE_URI),
            ]
        );
        // Snapshot at 0; the clock starts at the first live event.
        assert_eq!(
            events.iter().map(|e| e.t).collect::<Vec<_>>(),
            vec![0, 0, 100, 200]
        );
        for e in &events {
            assert_eq!(find_unredacted(&e.data), Vec::<String>::new(), "{e:?}");
        }
        assert_eq!(events[2].data["myTeam"][0]["championId"], json!(103));
    }

    #[test]
    fn stale_events_dont_shift_the_clock() {
        // An event long before the manual start mustn't put a gap at the start.
        let dir = tempfile::tempdir().unwrap();
        let mut f = Feed::new();
        let mut c = Controller::new(dir.path(), false);
        c.handle(&f.session(1));
        f.t += 600_000; // ten minutes later
        let info = c.start("late").unwrap();
        c.handle(&f.session(2));
        c.handle(&f.session(3));
        c.stop().unwrap();
        let (_, events) = read_file(&info.path).unwrap();
        assert_eq!(
            events.iter().map(|e| e.t).collect::<Vec<_>>(),
            vec![0, 0, 100]
        );
    }

    #[test]
    fn manual_recording_starts_with_current_state_and_survives_phase_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = Feed::new();
        let mut c = Controller::new(dir.path(), false);
        c.handle(&connected());
        c.handle(&f.phase("ChampSelect"));
        c.handle(&f.session(1));
        c.start("my test").unwrap();
        assert!(matches!(
            c.start("again"),
            Err(CaptureError::AlreadyRecording(_))
        ));
        c.handle(&f.phase("GameStart"));
        assert!(
            c.status().recording.is_some(),
            "auto must not stop manual recordings"
        );
        let info = c.stop().unwrap().unwrap();
        let (_, events) = read_file(&info.path).unwrap();
        assert_eq!(events.len(), 3);
        assert!(events[..2].iter().all(|e| e.kind == EventKind::Snapshot));
        assert!(c.stop().unwrap().is_none());
    }

    #[test]
    fn wire_file_gets_preroll_including_the_trigger() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = Feed::new();
        let mut c = Controller::new(dir.path(), true);
        // As with a live client: each event's frame arrives just before it.
        c.handle(
            &f.wire(r#"[8,"OnJsonApiEvent",{"uri":"/lol-lobby/v2/lobby","data":{"puuid":"x"}}]"#),
        );
        c.handle(&f.wire(r#"[8,"OnJsonApiEvent",{"uri":"/lol-gameflow/v1/gameflow-phase","data":"ChampSelect"}]"#));
        c.handle(&f.phase("ChampSelect"));
        c.handle(&f.wire("not json at all"));
        assert_eq!(c.status().recording.unwrap().wire_frames, 3);
        let info = c.stop().unwrap().unwrap();

        let wire = read_wire_file(&wire_path_for(&info.path)).unwrap();
        assert_eq!(wire.len(), 3);
        assert_eq!(wire[0].t, 0);
        let trigger = wire[1].frame.as_ref().unwrap();
        assert_eq!(trigger[2]["data"], json!("ChampSelect"));
        assert_eq!(
            find_unredacted(wire[0].frame.as_ref().unwrap()),
            Vec::<String>::new()
        );
        assert_eq!(wire[2].unparsable.as_ref().unwrap().bytes, 15);
    }

    #[test]
    fn preroll_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = Feed::new();
        let mut c = Controller::new(dir.path(), false);
        for _ in 0..(WIRE_PREROLL + 10) {
            c.handle(&f.wire("[0]"));
        }
        let info = c.start("x").unwrap();
        c.stop().unwrap();
        assert_eq!(
            read_wire_file(&wire_path_for(&info.path)).unwrap().len(),
            WIRE_PREROLL
        );
    }

    #[test]
    fn enabling_auto_mid_champ_select_starts_recording() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = Feed::new();
        let mut c = Controller::new(dir.path(), false);
        c.handle(&f.phase("ChampSelect"));
        assert!(c.status().recording.is_none());
        assert!(matches!(c.set_auto(true).as_slice(), [Notice::Started(_)]));
    }

    #[test]
    fn disconnect_stops_auto_recording_and_clears_state() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = Feed::new();
        let mut c = Controller::new(dir.path(), true);
        c.handle(&f.phase("ChampSelect"));
        c.handle(&f.session(1));
        let n = c.handle(&SourceEvent::Disconnected {
            reason: "client closed".into(),
        });
        stopped_path(&n);
        assert_eq!(c.latest().count(), 0);
        assert_eq!(c.status().phase, GameflowPhase::None);
    }

    #[test]
    fn delete_removes_from_latest() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = Feed::new();
        let mut c = Controller::new(dir.path(), false);
        c.handle(&f.session(1));
        assert_eq!(c.latest().count(), 1);
        c.handle(&f.event(SESSION_URI, EventKind::Delete, json!(null)));
        assert_eq!(c.latest().count(), 0);
    }
}
