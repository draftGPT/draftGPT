use super::*;
use crate::redact::find_unredacted;
use lcu::recording::{read_file, read_wire_file};
use serde_json::json;

fn ev(t: u64, uri: &str) -> RawEvent {
    RawEvent {
        t,
        uri: uri.into(),
        kind: EventKind::Update,
        data: json!({"x": t, "puuid": "real"}),
    }
}

fn frame(t: u64, text: &str) -> WireFrame {
    WireFrame {
        t,
        text: text.into(),
    }
}

#[test]
fn writes_redacted_events_and_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut rec = Recorder::create(dir.path(), "Ranked Dodge!", Some("16.21.1".into())).unwrap();
    rec.write_snapshot(ev(999, "/s")).unwrap();
    rec.write(ev(1000, "/a")).unwrap();
    rec.write(ev(1250, "/b")).unwrap();
    let info = rec.finish().unwrap();
    assert_eq!(info.events, 3);
    assert!(info.name.ends_with("_ranked-dodge"), "{}", info.name);

    let (header, events) = read_file(&info.path).unwrap();
    assert_eq!(header.format_version, FORMAT_VERSION);
    assert_eq!(header.client_version.as_deref(), Some("16.21.1"));
    assert_eq!(header.label, "ranked-dodge");
    assert_eq!(events[0].kind, EventKind::Snapshot);
    assert_eq!(
        events.iter().map(|e| e.t).collect::<Vec<_>>(),
        vec![0, 0, 250]
    );
    for e in &events {
        assert!(e.data["puuid"].as_str().unwrap().starts_with("redacted-"));
    }
}

/// Found live in Clash (16.23): a fetch round delivered its snapshots ~2 s
/// after stamping them, behind a champ select event.
#[test]
fn late_stamped_events_never_go_back_in_time() {
    let dir = tempfile::tempdir().unwrap();
    let mut rec = Recorder::create(dir.path(), "x", None).unwrap();
    rec.write(ev(1000, "/a")).unwrap();
    rec.write(ev(3000, "/session")).unwrap();
    rec.write(ev(1500, "/lol-clash/v1/bracket/1")).unwrap();
    rec.write(ev(3200, "/b")).unwrap();
    let info = rec.finish().unwrap();
    let (_, events) = read_file(&info.path).unwrap();
    assert_eq!(
        events.iter().map(|e| e.t).collect::<Vec<_>>(),
        vec![0, 2000, 2000, 2200]
    );
}

#[test]
fn wire_and_main_share_clock_and_fakes() {
    let dir = tempfile::tempdir().unwrap();
    let mut rec = Recorder::create(dir.path(), "x", None).unwrap();
    // A wire frame first: it starts the clock for both files.
    rec.write_wire(&frame(
        5_000,
        r#"[8,"e",{"uri":"/a","data":{"puuid":"real"}}]"#,
    ))
    .unwrap();
    rec.write(ev(5_040, "/a")).unwrap();
    rec.write_wire(&frame(5_060, "not json")).unwrap();
    assert_eq!(rec.wire_frames(), 2);
    let info = rec.finish().unwrap();

    let (_, events) = read_file(&info.path).unwrap();
    let wire = read_wire_file(&wire_path_for(&info.path)).unwrap();
    assert_eq!(wire[0].t, 0);
    assert_eq!(events[0].t, 40);
    assert_eq!(wire[1].t, 60);
    assert_eq!(wire[1].unparsable.as_ref().unwrap().bytes, 8);
    let frame = wire[0].frame.as_ref().unwrap();
    assert_eq!(find_unredacted(frame), Vec::<String>::new());
    assert_eq!(frame[2]["data"]["puuid"], events[0].data["puuid"]);
}

#[test]
fn no_wire_file_without_frames() {
    let dir = tempfile::tempdir().unwrap();
    let info = Recorder::create(dir.path(), "x", None)
        .unwrap()
        .finish()
        .unwrap();
    assert!(!wire_path_for(&info.path).exists());
}

#[test]
fn never_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let a = Recorder::create(dir.path(), "x", None)
        .unwrap()
        .finish()
        .unwrap();
    let b = Recorder::create(dir.path(), "x", None)
        .unwrap()
        .finish()
        .unwrap();
    assert_ne!(a.path, b.path);
}

#[test]
fn slugs() {
    assert_eq!(slug("  Blind Pick #2 "), "blind-pick--2");
    assert_eq!(slug("../../etc"), "etc");
    assert_eq!(slug(""), "capture");
    assert_eq!(slug("ARAM_test"), "aram_test");
}
