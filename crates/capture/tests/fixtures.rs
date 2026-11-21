//! Checks every recording in `fixtures/`: it parses, it's redacted, the typed
//! structs understand it, and each one still shows the situation it's named
//! for (see `fixtures/README.md`).

use std::path::{Path, PathBuf};

use capture::redact::find_unredacted_at;
use lcu::recording::{Header, read_file};
use lcu::types::{
    self, ActionType, ChampSelectSession, DodgeState, GameflowPhase, Matchmaking, Position,
    SubsetChampionList, SwapState, TimerPhase,
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

/// Sessions under `/lol-champ-select/v1/session`, typed, in order.
fn sessions(events: &[RawEvent]) -> Vec<ChampSelectSession> {
    events
        .iter()
        .filter(|e| e.uri == SESSION_URI && e.kind != EventKind::Delete)
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
        // Live never snapshots a session it got a 404 for, so neither may a
        // fixture. (Other resources can be snapshotted as null: an empty 200.)
        assert!(
            events.iter().all(|e| e.kind != EventKind::Snapshot
                || !is_session_uri(&e.uri)
                || !e.data.is_null()),
            "{name}: null session snapshot"
        );
        assert!(
            events.windows(2).all(|w| w[0].t <= w[1].t),
            "{name}: times go backwards"
        );
        for (i, ev) in events.iter().enumerate() {
            let at = format!("{name} event {i} ({} {:?})", ev.uri, ev.kind);
            assert_eq!(
                find_unredacted_at(&ev.uri, &ev.data),
                Vec::<String>::new(),
                "{at}"
            );
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
                // Clash recordings: untyped for now; redaction checked above.
                uri if lcu::is_recorded_clash_uri(uri)
                    || uri.starts_with("/lol-clash/v1/roster/redacted-") => {}
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
fn normal_draft_pick_order_swap_stranger_dodge() {
    let (header, events) = load("16_23_normal_draft_pick_order_swap_stranger_dodge");
    assert!(header.client_version.unwrap().starts_with("16.23."));
    let s = sessions(&events);
    assert_eq!(s[0].queue_id, Some(400));
    assert_eq!((s[0].my_team.len(), s[0].their_team.len()), (5, 5));
    // The request goes out in planning.
    assert!(s.iter().any(|s| {
        s.timer.phase == TimerPhase::Planning
            && s.pick_order_swaps
                .iter()
                .any(|w| w.state == SwapState::Sent)
    }));
    // Accepted: the local player moves to the other cell, and the two player
    // entries trade cells, positions included.
    let cells: Vec<i64> = s.iter().map(|s| s.local_player_cell_id).collect();
    assert_eq!((cells[0], *cells.last().unwrap()), (6, 5));
    let position = |s: &ChampSelectSession, cell| {
        s.my_team
            .iter()
            .find(|p| p.cell_id == cell)
            .unwrap()
            .assigned_position
    };
    let (first, last) = (&s[0], s.last().unwrap());
    assert_eq!(position(first, 6), position(last, 5));
    assert_eq!(position(first, 5), position(last, 6));
    assert_ne!(position(first, 5), position(first, 6));
    // Teammates' hovers are visible.
    assert!(s.iter().any(|s| {
        s.my_team
            .iter()
            .any(|p| p.cell_id != s.local_player_cell_id && p.hover().is_some())
    }));
    assert!(
        s.iter()
            .any(|s| s.trades.iter().any(|t| t.state == SwapState::Available))
    );
    assert_eq!(
        dodge_states(&events).last(),
        Some(&DodgeState::StrangerDodged)
    );
    assert_eq!(phases(&events).last(), Some(&GameflowPhase::Matchmaking));
}

#[test]
fn draft_position_swap_and_back() {
    let (header, events) = load("16_23_draft_position_swap_and_back");
    assert!(header.client_version.unwrap().starts_with("16.23."));
    let s = sessions(&events);
    assert_eq!(s[0].queue_id, Some(3110));
    // Positions of the local player and the teammate, deduplicated in order.
    let mut positions: Vec<(Position, Position)> = s
        .iter()
        .map(|s| {
            let me = s.local_player().unwrap().assigned_position;
            let other = s
                .my_team
                .iter()
                .find(|p| p.cell_id != s.local_player_cell_id)
                .unwrap()
                .assigned_position;
            (me, other)
        })
        .collect();
    positions.dedup();
    // Swapped, then swapped back; cells stay put (unlike a pick-order swap).
    assert_eq!(
        positions,
        [
            (Position::Bottom, Position::Middle),
            (Position::Middle, Position::Bottom),
            (Position::Bottom, Position::Middle),
        ]
    );
    assert!(s.iter().all(|s| s.local_player_cell_id == 0));
    // Each swap is requested first (`SENT`), and the contract id moves on once done.
    let sent = s
        .iter()
        .filter(|s| s.position_swaps.iter().any(|w| w.state == SwapState::Sent))
        .count();
    assert!(sent >= 2, "two requests: {sent}");
    let mut ids: Vec<i64> = s
        .iter()
        .flat_map(|s| s.position_swaps.iter().map(|w| w.id))
        .collect();
    ids.dedup();
    assert_eq!(ids, [0, 2, 4]);
    // The swap back happens after the local player locked in.
    let back = s
        .iter()
        .position(|s| {
            s.local_player().unwrap().assigned_position == Position::Bottom && my_pick(s).completed
        })
        .expect("back to bottom after locking");
    assert!(
        s[..back].iter().any(
            |s| s.local_player().unwrap().assigned_position == Position::Middle
                && my_pick(s).completed
        )
    );
    assert_eq!(s.last().unwrap().timer.phase, TimerPhase::Finalization);
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
        let ban = last
            .actions
            .iter()
            .flatten()
            .find(|a| a.kind == ActionType::Ban)
            .expect("a ban action");
        assert!(ban.completed && ban.champion().is_some(), "{name}");
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

/// Checks shared by the ranked flex fixtures: 5v5, enemies hidden with empty
/// name fields, teammates' names visible (redacted), a pick-order swap that
/// moves the local player from `from` to `to` keeping their position.
fn ranked_pick_order_swap(events: &[RawEvent], from: i64, to: i64, request: SwapState) {
    // Name fields aren't typed; check them in the raw sessions.
    for e in events
        .iter()
        .filter(|e| e.uri == SESSION_URI && !e.data.is_null())
    {
        for p in e.data["theirTeam"].as_array().unwrap() {
            assert_eq!(p["nameVisibilityType"], "HIDDEN");
            assert_eq!((&p["gameName"], &p["puuid"]), (&"".into(), &"".into()));
        }
        for p in e.data["myTeam"].as_array().unwrap() {
            assert_eq!(p["nameVisibilityType"], "VISIBLE");
            assert!(p["gameName"].as_str().unwrap().starts_with("redacted-"));
        }
    }
    let s = sessions(events);
    assert_eq!(s[0].queue_id, Some(440));
    assert_eq!((s[0].my_team.len(), s[0].their_team.len()), (5, 5));
    let mut cells: Vec<i64> = s.iter().map(|s| s.local_player_cell_id).collect();
    cells.dedup();
    assert_eq!(cells, [from, to]);
    let requested = s
        .iter()
        .position(|s| s.pick_order_swaps.iter().any(|w| w.state == request))
        .expect("the swap request");
    assert_eq!(s[requested].timer.phase, TimerPhase::BanPick);
    let mine = |s: &ChampSelectSession| s.local_player().unwrap().assigned_position;
    assert_eq!(mine(&s[0]), mine(s.last().unwrap()));
    assert_eq!(mine(&s[0]), Position::Utility);
    // Ranked bans are simultaneous, then revealed; the `bans` field stays empty.
    assert!(
        s.iter()
            .flat_map(|s| s.actions.iter().flatten())
            .any(|a| a.kind == ActionType::TenBansReveal)
    );
    assert!(s.iter().all(|s| s.bans.my_team_bans.is_empty()));
}

#[test]
fn ranked_flex_pick_order_swap_received_stranger_dodge() {
    let (_, events) = load("16_23_ranked_flex_pick_order_swap_received_stranger_dodge");
    let s = sessions(&events);
    assert!(s[0].my_team.iter().all(|p| (5..=9).contains(&p.cell_id)));
    ranked_pick_order_swap(&events, 7, 5, SwapState::Received);
    assert_eq!(
        dodge_states(&events).last(),
        Some(&DodgeState::StrangerDodged)
    );
    assert_eq!(phases(&events).last(), Some(&GameflowPhase::Matchmaking));
}

#[test]
fn ranked_flex_pick_order_swap_sent_game_start() {
    let (_, events) = load("16_23_ranked_flex_pick_order_swap_sent_game_start");
    let s = sessions(&events);
    ranked_pick_order_swap(&events, 1, 0, SwapState::Sent);
    let last = s.last().unwrap();
    assert_eq!(last.timer.phase, TimerPhase::GameStarting);
    // Every ban and pick is done; enemy picks became visible once locked.
    for a in last.actions.iter().flatten() {
        if matches!(a.kind, ActionType::Ban | ActionType::Pick) {
            assert!(a.completed, "{a:?}");
        }
    }
    assert!(last.their_team.iter().all(|p| p.champion().is_some()));
    assert!(
        last.their_team
            .iter()
            .all(|p| p.assigned_position == Position::Unassigned)
    );
    assert_eq!(phases(&events).last(), Some(&GameflowPhase::GameStart));
}
