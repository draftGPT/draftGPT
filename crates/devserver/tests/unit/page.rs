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
