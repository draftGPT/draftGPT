//! Checks every recording in `fixtures/`: it parses, it's redacted, the typed
//! structs understand it, and each one still shows the situation it's named
//! for (see `fixtures/README.md`).

use std::path::{Path, PathBuf};

use capture::redact::find_unredacted;
use lcu::recording::{Header, read_file};
use lcu::types::{
    self, ActionType, ChampSelectSession, DodgeState, GameflowPhase, Matchmaking,
    SubsetChampionList, TimerPhase,
};
use lcu::{
    EventKind, GAMEFLOW_PHASE_URI, MATCHMAKING_URI, RawEvent, SESSION_URI,
    SUBSET_CHAMPION_LIST_URI, is_session_uri,
};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn all_fixtures() -> Vec<PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(fixtures_dir())
        .expect("fixtures/ exists")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .collect();
    files.sort();
    files
}

fn load(name: &str) -> (Header, Vec<RawEvent>) {
    let path = fixtures_dir().join(format!("{name}.jsonl"));
    read_file(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Sessions under `/lol-champ-select/v1/session`, typed, in order (skipping
/// the null snapshot of a recording that starts outside champ select).
fn sessions(events: &[RawEvent]) -> Vec<ChampSelectSession> {
    events
        .iter()
        .filter(|e| e.uri == SESSION_URI && e.kind != EventKind::Delete && !e.data.is_null())
        .map(|e| types::parse(&e.data).unwrap())
        .collect()
}

fn dodge_states(events: &[RawEvent]) -> Vec<DodgeState> {
    events
        .iter()
        .filter(|e| e.uri == MATCHMAKING_URI && !e.data.is_null())
        .map(|e| {
            types::parse::<Matchmaking>(&e.data)
                .unwrap()
                .dodge_data
                .state
        })
        .collect()
}

fn phases(events: &[RawEvent]) -> Vec<GameflowPhase> {
    events
        .iter()
        .filter(|e| e.uri == GAMEFLOW_PHASE_URI)
        .map(|e| types::parse(&e.data).unwrap())
        .collect()
}

fn my_pick(s: &ChampSelectSession) -> &types::Action {
    s.actions
        .iter()
        .flatten()
        .find(|a| a.kind == ActionType::Pick && a.actor_cell_id == s.local_player_cell_id)
        .expect("a pick action for the local player")
}

#[test]
fn every_fixture_parses_is_redacted_and_typed() {
    let files = all_fixtures();
    assert!(files.len() >= 10, "{files:?}");
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy();
        let (header, events) = read_file(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        // Converted RCVolus recordings don't know their client version.
        assert!(
            header.client_version.is_some() || header.label == capture::rcvolus::LABEL,
            "{name}: no client_version"
        );
        assert!(!events.is_empty(), "{name}: no events");
        assert_eq!(
            events[0].kind,
            EventKind::Snapshot,
            "{name}: starts with state"
        );
        assert!(
            events.windows(2).all(|w| w[0].t <= w[1].t),
            "{name}: times go backwards"
        );
        for (i, ev) in events.iter().enumerate() {
            let at = format!("{name} event {i} ({} {:?})", ev.uri, ev.kind);
            assert_eq!(find_unredacted(&ev.data), Vec::<String>::new(), "{at}");
            if ev.kind == EventKind::Delete || ev.data.is_null() {
                continue;
            }
            match ev.uri.as_str() {
                uri if is_session_uri(uri) => {
                    let s: ChampSelectSession = types::parse(&ev.data).expect(&at);
                    assert_ne!(s.timer.phase, TimerPhase::Unknown, "{at}");
                    for a in s.actions.iter().flatten() {
                        assert_ne!(a.kind, ActionType::Unknown, "{at}: action type");
                    }
                }
                GAMEFLOW_PHASE_URI => {
                    let p: GameflowPhase = types::parse(&ev.data).expect(&at);
                    assert_ne!(p, GameflowPhase::Unknown, "{at}");
                }
                SUBSET_CHAMPION_LIST_URI => {
                    types::parse::<SubsetChampionList>(&ev.data).expect(&at);
                }
                MATCHMAKING_URI => {
                    let m: Matchmaking = types::parse(&ev.data).expect(&at);
                    assert_ne!(m.dodge_data.state, DodgeState::Unknown, "{at}");
                }
                other => panic!("{at}: not a core resource: {other}"),
            }
        }
    }
}

#[test]
fn aram_bench_swaps() {
    let (_, events) = load("16_22_aram_bench_swaps");
    let s = sessions(&events);
    assert!(s.iter().all(|s| s.bench_enabled));
    let mut benches: Vec<Vec<i64>> = s
        .iter()
        .map(|s| s.bench_champions.iter().map(|b| b.champion_id).collect())
        .collect();
    benches.dedup();
    assert!(
        benches.len() >= 3,
        "bench should change with swaps: {benches:?}"
    );
}

#[test]
fn draft_planning_hovers() {
    let (_, events) = load("16_22_draft_planning_hovers");
    let s = sessions(&events);
    assert_eq!(s[0].timer.phase, TimerPhase::Planning);
    let mut hovers: Vec<i64> = s
        .iter()
        .filter_map(|s| s.local_player().and_then(|p| p.hover()))
        .collect();
    hovers.dedup();
    assert!(hovers.len() >= 3, "several planning hovers: {hovers:?}");
    assert!(
        s.iter()
            .flat_map(|s| s.actions.iter().flatten())
            .any(|a| a.kind == ActionType::TenBansReveal && a.actor_cell_id == -1)
    );
}

#[test]
fn draft_ban_and_pick_timeout_autolock() {
    let (_, events) = load("16_22_draft_ban_and_pick_timeout_autolock");
    let s = sessions(&events);
    assert!(
        s.iter()
            .flat_map(|s| s.actions.iter().flatten())
            .any(|a| a.kind == ActionType::PhaseTransition)
    );
    let last = s.last().unwrap();
    assert_eq!(last.timer.phase, TimerPhase::Finalization);
    // The hover was locked in when the pick timer ran out.
    let hovered = s
        .iter()
        .rev()
        .find_map(|s| s.local_player().and_then(|p| p.hover()))
        .expect("a hover before the timeout");
    let pick = my_pick(last);
    assert!(pick.completed);
    assert_eq!(pick.champion(), Some(hovered));
}

#[test]
fn draft_missed_ban() {
    let (_, events) = load("16_22_draft_missed_ban");
    let last = sessions(&events).pop().unwrap();
    let ban = last
        .actions
        .iter()
        .flatten()
        .find(|a| a.kind == ActionType::Ban)
        .unwrap();
    assert!(ban.completed);
    assert_eq!(ban.champion_id, -1);
    assert_eq!(ban.champion(), None);
    // With simultaneous bans, `bans` stays empty; bans live in actions.
    assert!(last.bans.my_team_bans.is_empty());
}

#[test]
fn draft_pick_timeout_kicked() {
    let (_, events) = load("16_22_draft_pick_timeout_kicked");
    let last = sessions(&events).pop().unwrap();
    let pick = my_pick(&last);
    assert!(pick.is_in_progress && !pick.completed && pick.champion().is_none());
    // A timeout without a hover looks like leaving: the party dodged.
    assert_eq!(dodge_states(&events).last(), Some(&DodgeState::PartyDodged));
    assert_eq!(phases(&events).last(), Some(&GameflowPhase::Lobby));
}

#[test]
fn subset_pick_swaps_dodge() {
    let (_, events) = load("16_22_subset_pick_swaps_dodge");
    let offer: SubsetChampionList = events
        .iter()
        .find(|e| e.uri == SUBSET_CHAMPION_LIST_URI && e.kind != EventKind::Delete)
        .map(|e| types::parse(&e.data).unwrap())
        .expect("the offer is recorded");
    assert_eq!(offer.len(), 3);
    let s = sessions(&events);
    assert!(s[0].allow_subset_champion_picks);
    let mut mine: Vec<i64> = s
        .iter()
        .filter_map(|s| s.local_player().and_then(|p| p.champion()))
        .collect();
    mine.dedup();
    assert!(mine.len() >= 3, "picked, then swapped: {mine:?}");
    assert!(
        mine.iter().all(|c| offer.contains(c)),
        "{mine:?} vs {offer:?}"
    );
    assert_eq!(dodge_states(&events).last(), Some(&DodgeState::PartyDodged));
}

#[test]
fn client_closed_mid_champ_select() {
    let (_, events) = load("16_22_client_closed_mid_champ_select");
    assert_eq!(phases(&events).last(), Some(&GameflowPhase::None));
    // Closing the client isn't a dodge.
    assert!(!dodge_states(&events).contains(&DodgeState::PartyDodged));
}

#[test]
fn rcvolus_solo_custom_ban_pick() {
    for name in [
        "rcvolus_2019_solo_custom_ban_pick",
        "rcvolus_2022_solo_custom_ban_pick",
    ] {
        let (header, events) = load(name);
        assert_eq!(header.label, capture::rcvolus::LABEL);
        let s = sessions(&events);
        assert!(
            s.iter()
                .all(|s| s.my_team.len() == 1 && s.their_team.is_empty()),
            "{name}"
        );
        let last = s.last().unwrap();
        assert_eq!(last.timer.phase, TimerPhase::Finalization, "{name}");
        let pick = my_pick(last);
        assert!(pick.completed && pick.champion().is_some(), "{name}");
    }
}

#[test]
fn rcvolus_2019_tournament_draft_spectated() {
    let (_, events) = load("rcvolus_2019_tournament_draft_spectated");
    let s = sessions(&events);
    let last = s.last().unwrap();
    assert!(last.is_spectating);
    assert_eq!(last.local_player_cell_id, -1);
    assert_eq!((last.my_team.len(), last.their_team.len()), (5, 5));
    // Tournament draft: 10 bans and 10 picks, a second ban phase after six picks.
    let order: Vec<ActionType> = last.actions.iter().flatten().map(|a| a.kind).collect();
    assert_eq!(order.len(), 20);
    assert_eq!(order.iter().filter(|k| **k == ActionType::Ban).count(), 10);
    assert_eq!(order[12], ActionType::Ban);
    assert!(
        last.actions
            .iter()
            .flatten()
            .all(|a| a.completed && a.champion().is_some())
    );
    // Spectators see both teams' picks.
    assert!(last.their_team.iter().all(|p| p.champion().is_some()));
}
