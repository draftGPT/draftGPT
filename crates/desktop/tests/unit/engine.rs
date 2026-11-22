use super::*;
use lcu::recording::read_file;
use lcu::{ClientInfo, EventKind, RawEvent};

fn recommender() -> StatsRecommender {
    StatsRecommender::from_json(include_str!("../../../../data/stats/16.22.json")).unwrap()
}

#[test]
fn a_replayed_fixture_produces_drafts_with_recommendations_then_ends() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/16_22_draft_ban_and_pick_timeout_autolock.jsonl"
    );
    let (_, events) = read_file(path.as_ref()).unwrap();
    let shared = Shared::new(true, "Starting");
    let mut engine = Engine::new(recommender(), Some("x".into()));
    let connected = SourceEvent::Connected(ClientInfo::default());
    assert_eq!(
        engine.handle(&connected, &shared).unwrap().status,
        "Replaying x"
    );
    let updates: Vec<Update> = events
        .into_iter()
        .filter_map(|e| engine.handle(&SourceEvent::Event(e), &shared))
        .collect();
    assert!(updates.len() > 3);
    assert!(
        updates
            .iter()
            .filter(|u| u.draft.is_some())
            .all(|u| u.recs.is_some())
    );
    assert!(
        updates.last().unwrap().draft.is_none(),
        "champ select ended"
    );
}

#[test]
fn disconnect_clears_the_draft_and_unchanged_events_emit_nothing() {
    let shared = Shared::new(false, "Starting");
    let mut engine = Engine::new(recommender(), None);
    let session = SourceEvent::Event(RawEvent {
        t: 0,
        uri: lcu::SESSION_URI.into(),
        kind: EventKind::Update,
        data: serde_json::json!({"myTeam": [{"cellId": 0}]}),
    });
    assert!(engine.handle(&session, &shared).unwrap().draft.is_some());
    assert!(
        engine.handle(&session, &shared).is_none(),
        "same draft again"
    );
    let gone = SourceEvent::Disconnected {
        reason: "League client closed".into(),
    };
    let u = engine.handle(&gone, &shared).unwrap();
    assert_eq!((u.draft, u.status.as_str()), (None, "League client closed"));
    assert!(!u.floating);
}

#[test]
fn the_timer_keeps_running_for_resent_updates_and_seq_increases() {
    let shared = Shared::new(true, "x");
    let mut d = DraftState::default();
    d.timer.remaining_ms = Some(30_000);
    let first = shared.change(|u| u.draft = Some(d.clone()));
    std::thread::sleep(std::time::Duration::from_millis(60));
    // Re-sent without a new draft (e.g. the float toggle): time has passed.
    let again = shared.change(|u| u.floating = false);
    let left = again.draft.unwrap().timer.remaining_ms.unwrap();
    assert!(left <= 29_940, "{left}");
    assert!(again.seq > first.seq);
    // The same draft set again doesn't restart the clock.
    let same = shared.change(|u| u.draft = Some(d.clone()));
    assert!(same.draft.unwrap().timer.remaining_ms.unwrap() <= 29_940);
}
