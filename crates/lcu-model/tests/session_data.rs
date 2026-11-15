//! Parses the session JSON in `tests/data/` (old RCVolus snapshots, a real
//! 16.20 draft, a 16.21 sample) and checks the fields the drafter depends on.

use std::path::PathBuf;

use lcu_model::types::{ActionType, ChampSelectSession, Position, TimerPhase};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn load(name: &str) -> ChampSelectSession {
    let text = std::fs::read_to_string(data_dir().join(name)).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    lcu_model::types::parse(&value).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn every_file_parses_with_known_enum_values() {
    let mut count = 0;
    for entry in std::fs::read_dir(data_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        let s = load(&name);
        count += 1;
        assert_ne!(s.timer.phase, TimerPhase::Unknown, "{name}: timer phase");
        assert!(!s.actions.is_empty(), "{name}: no actions");
        for a in s.actions.iter().flatten() {
            assert_ne!(a.kind, ActionType::Unknown, "{name}: action {}", a.id);
        }
        for p in s.my_team.iter().chain(&s.their_team) {
            assert_ne!(
                p.assigned_position,
                Position::Unknown,
                "{name}: cell {}",
                p.cell_id
            );
        }
    }
    assert!(count >= 7, "expected at least 7 data files, found {count}");
}

#[test]
fn tournament_final_has_full_draft() {
    let s = load("rcvolus_2019_tournament_final.json");
    assert_eq!(s.timer.phase, TimerPhase::Finalization);
    let actions: Vec<_> = s.actions.iter().flatten().collect();
    assert_eq!(actions.len(), 20);
    assert!(actions.iter().all(|a| a.completed));
    assert_eq!(
        actions.iter().filter(|a| a.kind == ActionType::Ban).count(),
        10
    );
    assert_eq!(
        actions
            .iter()
            .filter(|a| a.kind == ActionType::Pick)
            .count(),
        10
    );
    // 2019 client: no isAllyAction field.
    assert!(actions.iter().all(|a| a.is_ally_action.is_none()));
    assert!(
        s.my_team
            .iter()
            .chain(&s.their_team)
            .all(|p| p.champion().is_some())
    );
}

#[test]
fn custom_2022_has_ban_in_progress() {
    let s = load("rcvolus_2022_custom_ban_in_progress.json");
    assert_eq!(s.timer.phase, TimerPhase::BanPick);
    let current: Vec<_> = s.in_progress().collect();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].kind, ActionType::Ban);
}

#[test]
fn synthetic_ranked_session() {
    let s = load("synthetic_16_21_ranked_pick_phase.json");
    assert_eq!(s.queue_id, Some(420));
    assert!(s.has_simultaneous_bans);
    let me = s.local_player().expect("local player");
    assert_eq!(me.assigned_position, Position::Middle);
    assert_eq!(me.champion(), None);
    assert_eq!(me.hover(), Some(103));
    assert!(s.my_team[3].is_autofilled);
    let current: Vec<_> = s.in_progress().collect();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].actor_cell_id, 6);
    assert_eq!(current[0].is_ally_action, Some(false));
    assert!(
        s.actions
            .iter()
            .flatten()
            .any(|a| a.kind == ActionType::TenBansReveal)
    );
    assert_eq!(s.bans.my_team_bans.len(), 4);
}

#[test]
fn real_16_20_planning_has_every_ban_in_progress() {
    let s = load("dosxnjos_16_20_draft_planning.json");
    assert_eq!(s.timer.phase, TimerPhase::Planning);
    assert_eq!(s.queue_id, Some(400));
    let bans: Vec<_> = s
        .actions
        .iter()
        .flatten()
        .filter(|a| a.kind == ActionType::Ban)
        .collect();
    assert!(!bans.is_empty());
    assert!(bans.iter().all(|a| a.is_in_progress && !a.completed));
}

#[test]
fn real_16_20_missed_ban_is_no_champion() {
    let s = load("dosxnjos_16_20_draft_missed_ban.json");
    let missed: Vec<_> = s
        .actions
        .iter()
        .flatten()
        .filter(|a| a.kind == ActionType::Ban && a.completed && a.champion_id == -1)
        .collect();
    assert!(!missed.is_empty());
    assert!(missed.iter().all(|a| a.champion().is_none()));
}

#[test]
fn real_16_20_game_starting_has_full_teams() {
    let s = load("dosxnjos_16_20_draft_game_starting.json");
    assert_eq!(s.timer.phase, TimerPhase::GameStarting);
    assert_eq!(s.my_team.len(), 5);
    assert!(s.my_team.iter().all(|p| p.champion().is_some()));
    assert!(s.local_player().is_some());
}
