use super::*;
use GameflowPhase as P;
use serde_json::json;

#[test]
fn starts_only_when_enabled_and_idle() {
    assert_eq!(
        decide(true, P::ChampSelect, false, None),
        Some(AutoAction::Start)
    );
    assert_eq!(decide(false, P::ChampSelect, false, None), None);
    assert_eq!(decide(true, P::ChampSelect, false, Some(false)), None);
    assert_eq!(decide(true, P::ChampSelect, false, Some(true)), None);
    assert_eq!(decide(true, P::Lobby, false, None), None);
}

#[test]
fn stops_only_its_own_recordings() {
    assert_eq!(
        decide(true, P::GameStart, false, Some(true)),
        Some(AutoAction::Stop)
    );
    assert_eq!(
        decide(true, P::Lobby, false, Some(true)),
        Some(AutoAction::Stop)
    );
    // Auto turned off mid-recording: still finishes its own recording.
    assert_eq!(
        decide(false, P::Lobby, false, Some(true)),
        Some(AutoAction::Stop)
    );
    assert_eq!(decide(true, P::Lobby, false, Some(false)), None);
}

#[test]
fn clash_records_around_the_game_but_not_during_it() {
    // Lock-in / scouting in the lobby or out of it: record.
    assert_eq!(decide(true, P::Lobby, true, None), Some(AutoAction::Start));
    assert_eq!(decide(true, P::None, true, None), Some(AutoAction::Start));
    assert_eq!(
        decide(true, P::CheckedIntoTournament, true, None),
        Some(AutoAction::Start)
    );
    // Queue and champ select keep the same recording going.
    assert_eq!(decide(true, P::ReadyCheck, true, Some(true)), None);
    assert_eq!(decide(true, P::ChampSelect, true, Some(true)), None);
    // The game: stop, and don't start again until it's over.
    assert_eq!(
        decide(true, P::GameStart, true, Some(true)),
        Some(AutoAction::Stop)
    );
    assert_eq!(decide(true, P::InProgress, true, None), None);
    assert_eq!(decide(true, P::EndOfGame, true, None), None);
    // Back in the lobby, tournament still on (next round): a new recording.
    assert_eq!(decide(true, P::Lobby, true, None), Some(AutoAction::Start));
    // Tournament over: stop.
    assert_eq!(
        decide(true, P::Lobby, false, Some(true)),
        Some(AutoAction::Stop)
    );
}

#[test]
fn clash_state_extraction() {
    let summary = json!([
        {"state": "RESULTS", "tournamentId": 1},
        {"state": "SCOUTING", "tournamentId": 2}
    ]);
    assert_eq!(clash_states(&summary), vec!["RESULTS", "SCOUTING"]);
    assert!(clash_active([&summary]));
    assert!(!clash_active([
        &json!([{"state": "UPCOMING"}]),
        &json!(null)
    ]));
    assert!(clash_active([&json!({"state": "IN_GAME"})]));
    assert!(!clash_active([&json!([])]));
}

#[test]
fn phase_extraction() {
    let ev = |kind, data| RawEvent {
        t: 0,
        uri: GAMEFLOW_PHASE_URI.into(),
        kind,
        data,
    };
    assert_eq!(
        phase_of(&ev(EventKind::Update, json!("ChampSelect"))),
        Some(P::ChampSelect)
    );
    assert_eq!(phase_of(&ev(EventKind::Delete, json!(null))), Some(P::None));
    assert_eq!(
        phase_of(&ev(EventKind::Update, json!(42))),
        Some(P::Unknown)
    );
    let other = RawEvent {
        uri: lcu::SESSION_URI.into(),
        ..ev(EventKind::Update, json!({}))
    };
    assert_eq!(phase_of(&other), None);
}
