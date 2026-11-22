use super::*;
use serde_json::json;

#[test]
fn unknown_enum_values_dont_fail() {
    let s: ChampSelectSession = serde_json::from_value(json!({
        "timer": {"phase": "SOMETHING_NEW"},
        "myTeam": [{"cellId": 0, "assignedPosition": "support"}],
        "actions": [[{"id": 1, "type": "brand_new_action"}]],
        "trades": [{"id": 1, "cellId": 2, "state": "PENDING"}],
        "brandNewField": {"x": 1}
    }))
    .unwrap();
    assert_eq!(s.timer.phase, TimerPhase::Unknown);
    assert_eq!(s.my_team[0].assigned_position, Position::Unknown);
    assert_eq!(s.actions[0][0].kind, ActionType::Unknown);
    assert_eq!(s.trades[0].state, SwapState::Unknown);
}

#[test]
fn nulls_fall_back_to_defaults() {
    let s: ChampSelectSession = parse(&json!({
        "localPlayerCellId": null,
        "myTeam": [{"cellId": 1, "championPickIntent": null, "assignedPosition": null}],
        "actions": [[{"id": 1, "type": "pick", "isAllyAction": null}]]
    }))
    .unwrap();
    assert_eq!(s.my_team[0].champion_pick_intent, 0);
    assert_eq!(s.my_team[0].assigned_position, Position::Unassigned);
    assert_eq!(s.actions[0][0].is_ally_action, None);
    // Plain serde would reject the same input.
    assert!(
        serde_json::from_value::<ChampSelectSession>(json!({"localPlayerCellId": null})).is_err()
    );
}

#[test]
fn empty_object_is_default_session() {
    let s: ChampSelectSession = serde_json::from_value(json!({})).unwrap();
    assert_eq!(s, ChampSelectSession::default());
}

#[test]
fn missed_ban_is_no_champion() {
    let a: Action = parse(&json!({"type": "ban", "championId": -1, "completed": true})).unwrap();
    assert_eq!(a.champion(), None);
}

#[test]
fn voting_action_types_parse() {
    for (s, t) in [
        ("vote", ActionType::Vote),
        ("phase_transition", ActionType::PhaseTransition),
        ("vote_transition", ActionType::VoteTransition),
        ("team_vote_reveal", ActionType::TeamVoteReveal),
    ] {
        assert_eq!(serde_json::from_value::<ActionType>(json!(s)).unwrap(), t);
    }
}

#[test]
fn zero_champion_means_none() {
    let p: PlayerSelection =
        serde_json::from_value(json!({"championId": 0, "championPickIntent": 103})).unwrap();
    assert_eq!(p.champion(), None);
    assert_eq!(p.hover(), Some(103));
}

#[test]
fn positions_parse() {
    for (s, p) in [
        ("top", Position::Top),
        ("jungle", Position::Jungle),
        ("middle", Position::Middle),
        ("bottom", Position::Bottom),
        ("utility", Position::Utility),
        ("", Position::Unassigned),
    ] {
        assert_eq!(serde_json::from_value::<Position>(json!(s)).unwrap(), p);
    }
}

#[test]
fn subset_list_and_dodge_parse() {
    // Shapes from a 16.22 capture (custom, queue 3270, left champ select).
    let offer: SubsetChampionList = parse(&json!([57, 45, 53])).unwrap();
    assert_eq!(offer, vec![57, 45, 53]);
    let m: Matchmaking = parse(&json!({
        "dodgeData": {"dodgerId": 9, "state": "PartyDodged"},
        "errors": [], "isCurrentlyInQueue": false, "lobbyId": "", "queueId": 0,
        "readyCheck": {"state": "Invalid"}, "searchState": "Invalid"
    }))
    .unwrap();
    assert_eq!(m.dodge_data.state, DodgeState::PartyDodged);
    let m: Matchmaking = parse(&json!({"dodgeData": {"state": "SomethingNew"}})).unwrap();
    assert_eq!(m.dodge_data.state, DodgeState::Unknown);
}

#[test]
fn gameflow_phase_parses_bare_string() {
    let p: GameflowPhase = serde_json::from_value(json!("ChampSelect")).unwrap();
    assert_eq!(p, GameflowPhase::ChampSelect);
    let p: GameflowPhase = serde_json::from_value(json!("Brand New Phase")).unwrap();
    assert_eq!(p, GameflowPhase::Unknown);
}
