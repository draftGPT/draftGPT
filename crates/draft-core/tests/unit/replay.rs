use super::*;

#[test]
fn a_fixture_replays_into_distinct_drafts() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/16_22_draft_ban_and_pick_timeout_autolock.jsonl"
    );
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("skipped: fixtures submodule not checked out");
        return;
    };
    let r = replay(&text).unwrap();
    assert!(r.steps.len() > 5, "{}", r.steps.len());
    assert!(r.steps.windows(2).all(|w| w[0].t <= w[1].t));
    assert!(r.steps.windows(2).all(|w| {
        DraftState {
            timer: w[1].draft.timer.clone(),
            ..w[0].draft.clone()
        } != w[1].draft
    }));
    assert!(r.client_version.is_some());
}

#[test]
fn a_broken_recording_is_an_error() {
    assert!(replay("").is_err());
    assert!(
        replay("{\"format_version\":1,\"started_at\":\"x\",\"label\":\"x\"}\nnot json").is_err()
    );
}
