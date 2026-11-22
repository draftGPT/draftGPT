use super::*;

#[test]
fn recommends_from_json() {
    let stats = include_str!("../../../../data/stats/16.22.json");
    let engine = Engine::new(stats).unwrap();
    let draft = serde_json::to_string(&DraftState::default()).unwrap();
    let out: serde_json::Value = serde_json::from_str(&engine.recommend(&draft).unwrap()).unwrap();
    assert_eq!(out["patch"], "16.22");
    assert!(out["picks"].as_array().unwrap().len() > 1);
}

#[test]
fn replays_a_recording_to_json() {
    let text = include_str!("../../../../fixtures/16_22_draft_missed_ban.jsonl");
    let out: serde_json::Value = serde_json::from_str(&replay(text).unwrap()).unwrap();
    assert!(out["steps"].as_array().unwrap().len() > 1);
    assert!(out["steps"][0]["draft"]["ally"].is_array());
}
