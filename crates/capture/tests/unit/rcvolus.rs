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
