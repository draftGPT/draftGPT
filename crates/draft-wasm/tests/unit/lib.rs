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
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/16_22_draft_missed_ban.jsonl"
    );
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("skipped: fixtures submodule not checked out");
        return;
    };
    let out: serde_json::Value = serde_json::from_str(&replay(&text).unwrap()).unwrap();
    assert!(out["steps"].as_array().unwrap().len() > 1);
    assert!(out["steps"][0]["draft"]["ally"].is_array());
}
