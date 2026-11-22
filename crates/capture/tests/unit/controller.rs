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
fn auto_records_a_clash_round_from_scouting_to_game_start() {
    use lcu::{CLASH_TOURNAMENT_SUMMARY, Live};
    let dir = tempfile::tempdir().unwrap();
    let mut f = Feed::new();
    let mut c = Controller::new(dir.path(), true);
    c.handle(&connected());
    c.handle(&f.phase("Lobby"));
    let summary = |state: &str| json!([{"state": state, "tournamentId": 4321, "rosterId": "778899", "bracketId": 5}]);
    assert!(
        c.handle(&f.event(
            CLASH_TOURNAMENT_SUMMARY.uri,
            EventKind::Update,
            summary("UPCOMING")
        ))
        .is_empty()
    );

    let n = c.handle(&f.event(
        CLASH_TOURNAMENT_SUMMARY.uri,
        EventKind::Update,
        summary("SCOUTING"),
    ));
    let [Notice::Started(info)] = n.as_slice() else {
        panic!("{n:?}")
    };
    assert!(info.name.ends_with("_clash"), "{}", info.name);
    let roster = json!({"id": "778899", "name": "Team Real Name", "members": [{"summonerId": 12_345_678_901_u64, "position": "TOP"}]});
    c.handle(&f.event("/lol-clash/v1/roster/778899", EventKind::Snapshot, roster));
    c.handle(&f.phase("ChampSelect"));
    c.handle(&f.session(1));
    let path = stopped_path(&c.handle(&f.phase("GameStart")));
    // During the game: nothing, even with the tournament still on.
    assert!(c.handle(&f.phase("InProgress")).is_empty());
    // Next round after the game: a new recording.
    let n = c.handle(&f.phase("Lobby"));
    assert!(matches!(n.as_slice(), [Notice::Started(_)]), "{n:?}");
    let n = c.handle(&f.event(
        CLASH_TOURNAMENT_SUMMARY.uri,
        EventKind::Update,
        summary("RESULTS"),
    ));
    assert!(matches!(n.as_slice(), [Notice::Stopped(_)]), "{n:?}");

    let (_, events) = read_file(&path).unwrap();
    let uris: Vec<_> = events.iter().map(|e| e.uri.as_str()).collect();
    assert!(uris.contains(&SESSION_URI), "{uris:?}");
    let roster_ev = events
        .iter()
        .find(|e| e.uri.starts_with("/lol-clash/v1/roster/"))
        .unwrap();
    assert!(
        roster_ev.uri.starts_with("/lol-clash/v1/roster/redacted-"),
        "{}",
        roster_ev.uri
    );
    assert_eq!(roster_ev.data["members"][0]["position"], "TOP");
    for ev in &events {
        assert_eq!(
            crate::redact::find_unredacted_at(&ev.uri, &ev.data),
            Vec::<String>::new(),
            "{}",
            ev.uri
        );
    }
    // The default live source doesn't forward Clash; capture turns it on.
    assert!(!(Live::default().also_forward)("/lol-clash/v1/roster/1"));
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
    c.handle(&f.wire(r#"[8,"OnJsonApiEvent",{"uri":"/lol-lobby/v2/lobby","data":{"puuid":"x"}}]"#));
    c.handle(&f.wire(
        r#"[8,"OnJsonApiEvent",{"uri":"/lol-gameflow/v1/gameflow-phase","data":"ChampSelect"}]"#,
    ));
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
fn old_preroll_doesnt_delay_the_main_timeline() {
    // Live 16.22 capture: the oldest preroll frame was 12.5 s before the
    // start, so the first live event landed at t = 12537.
    let dir = tempfile::tempdir().unwrap();
    let mut f = Feed::new();
    let mut c = Controller::new(dir.path(), true);
    c.handle(
        &f.wire(r#"[8,"OnJsonApiEvent",{"uri":"/lol-matchmaking/v1/ready-check","data":{}}]"#),
    );
    f.t += 12_500;
    c.handle(&f.phase("ChampSelect"));
    c.handle(&f.session(1));
    c.handle(&f.wire("[0]"));
    c.handle(&f.session(2));
    let info = c.stop().unwrap().unwrap();

    // Phase snapshot, then the first live event at 0 (was 12_600).
    let (_, events) = read_file(&info.path).unwrap();
    assert_eq!(
        events.iter().map(|e| e.t).collect::<Vec<_>>(),
        vec![0, 0, 200]
    );
    let wire = read_wire_file(&wire_path_for(&info.path)).unwrap();
    assert_eq!(wire.iter().map(|w| w.t).collect::<Vec<_>>(), vec![0, 100]);
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

/// Found live in Clash (16.23): round 2's ranked stats were snapshotted
/// into every later recording, a normal game's included.
#[test]
fn scouting_data_is_recorded_but_not_carried_into_the_next_recording() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = Feed::new();
    let mut c = Controller::new(dir.path(), false);
    c.start("clash").unwrap();
    for uri in [
        "/lol-clash/v1/scouting/champions",
        "/lol-ranked/v1/ranked-stats/x",
        "/lol-summoner/v1/summoners/7",
    ] {
        c.handle(&f.event(uri, EventKind::Snapshot, json!({"x": 1})));
    }
    c.handle(&f.session(1));
    let first = c.stop().unwrap().unwrap();
    assert_eq!(read_file(&first.path).unwrap().1.len(), 4);
    assert_eq!(c.latest().count(), 1);
    let second = c.start("next").unwrap();
    c.stop().unwrap();
    let (_, events) = read_file(&second.path).unwrap();
    assert_eq!(
        events.iter().map(|e| e.uri.as_str()).collect::<Vec<_>>(),
        [SESSION_URI]
    );
}

#[test]
fn without_recording_never_records() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = Controller::without_recording(dir.path(), "replaying a recording");
    let mut f = Feed::new();
    c.handle(&connected());
    assert!(matches!(c.start("x"), Err(CaptureError::Disabled(_))));
    assert!(c.set_auto(true).is_empty());
    assert!(c.set_auto(false).is_empty());
    // Champ select starts, which would trigger auto mode.
    assert!(c.handle(&f.phase("ChampSelect")).is_empty());
    c.handle(&f.session(1));
    let st = c.status();
    assert!(!st.auto && st.recording.is_none());
    assert_eq!(
        st.recording_disabled.as_deref(),
        Some("replaying a recording")
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
