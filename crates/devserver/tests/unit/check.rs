use super::*;
use lcu::SESSION_URI;
use serde_json::json;

fn ev(uri: &str, data: Value) -> RawEvent {
    RawEvent {
        t: 0,
        uri: uri.into(),
        kind: EventKind::Update,
        data,
    }
}

#[test]
fn clean_session_has_no_problems() {
    let data = json!({"timer": {"phase": "BAN_PICK"}, "myTeam": [{"assignedPosition": "top"}],
                          "actions": [[{"type": "ban"}]]});
    assert_eq!(check(&ev(SESSION_URI, data)), None);
}

#[test]
fn reports_unknown_values_with_raw_text() {
    let data = json!({"timer": {"phase": "NEW_PHASE"}, "theirTeam": [{"assignedPosition": "support"}],
                          "actions": [[{"type": "ban"}, {"type": "brand_new_action"}]]});
    let msg = check(&ev(SESSION_URI, data)).unwrap();
    assert!(msg.contains("timer.phase: \"NEW_PHASE\""), "{msg}");
    assert!(
        msg.contains("actions[0][1].type: \"brand_new_action\""),
        "{msg}"
    );
    assert!(
        msg.contains("theirTeam[0].assignedPosition: \"support\""),
        "{msg}"
    );
}

#[test]
fn reports_type_errors() {
    let msg = check(&ev(SESSION_URI, json!({"myTeam": "oops"}))).unwrap();
    assert!(msg.contains("session parse failed"), "{msg}");
}

#[test]
fn clash_states() {
    let uri = lcu::CLASH_TOURNAMENT_SUMMARY.uri;
    assert_eq!(check(&ev(uri, json!([{"state": "SCOUTING"}]))), None);
    let msg = check(&ev(uri, json!([{"state": "DRAFTING"}]))).unwrap();
    assert!(msg.contains("DRAFTING"), "{msg}");
}

#[test]
fn gameflow_phase() {
    assert_eq!(check(&ev(GAMEFLOW_PHASE_URI, json!("Lobby"))), None);
    assert!(check(&ev(GAMEFLOW_PHASE_URI, json!("Brand New"))).is_some());
}

#[test]
fn subset_list_and_matchmaking() {
    assert_eq!(
        check(&ev(SUBSET_CHAMPION_LIST_URI, json!([57, 45, 53]))),
        None
    );
    assert!(check(&ev(SUBSET_CHAMPION_LIST_URI, json!({"x": 1}))).is_some());
    let dodged = json!({"dodgeData": {"dodgerId": 9, "state": "PartyDodged"}});
    assert_eq!(check(&ev(MATCHMAKING_URI, dodged)), None);
    assert_eq!(check(&ev(MATCHMAKING_URI, json!(null))), None);
    let stranger = json!({"dodgeData": {"dodgerId": 0, "state": "StrangerDodged"}});
    assert_eq!(check(&ev(MATCHMAKING_URI, stranger)), None);
    let msg = check(&ev(
        MATCHMAKING_URI,
        json!({"dodgeData": {"state": "SomethingNew"}}),
    ))
    .unwrap();
    assert!(msg.contains("dodgeData.state: \"SomethingNew\""), "{msg}");
}

#[test]
fn ignores_other_uris_and_deletes() {
    assert_eq!(check(&ev("/other", json!(1))), None);
    let mut del = ev(SESSION_URI, json!(null));
    del.kind = EventKind::Delete;
    assert_eq!(check(&del), None);
}
