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
use serde_json::Value;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// True when the fixtures submodule isn't checked out (`git submodule update --init`);
/// fixture tests then return early and pass.
fn missing() -> bool {
    let missing = !fixtures_dir().join("README.md").exists();
    if missing {
        eprintln!("skipped: fixtures submodule not checked out");
    }
    missing
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
    if missing() {
        return;
    }
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
                    || uri.starts_with("/lol-clash/v1/roster/redacted-")
                    || uri.starts_with("/lol-ranked/v1/ranked-stats/redacted-")
                    || uri.starts_with("/lol-summoner/v1/summoners/redacted-") => {}
                other => panic!("{at}: not a core resource: {other}"),
            }
        }
    }
}

#[test]
fn aram_bench_swaps() {
    if missing() {
        return;
    }
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
    if missing() {
        return;
    }
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
    if missing() {
        return;
    }
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
    if missing() {
        return;
    }
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
    if missing() {
        return;
    }
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
    if missing() {
        return;
    }
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
    if missing() {
        return;
    }
    let (_, events) = load("16_22_client_closed_mid_champ_select");
    assert_eq!(phases(&events).last(), Some(&GameflowPhase::None));
    // Closing the client isn't a dodge.
    assert!(!dodge_states(&events).contains(&DodgeState::PartyDodged));
}

#[test]
fn normal_draft_pick_order_swap_stranger_dodge() {
    if missing() {
        return;
    }
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
    if missing() {
        return;
    }
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
fn ranked_flex_position_swap_teammate_pick_order_swap() {
    if missing() {
        return;
    }
    let (header, events) = load("16_23_ranked_flex_position_swap_teammate_pick_order_swap");
    assert!(header.client_version.unwrap().starts_with("16.23."));
    let raw: Vec<&serde_json::Value> = events
        .iter()
        .filter(|e| e.uri == SESSION_URI && e.kind != EventKind::Delete)
        .map(|e| &e.data)
        .collect();
    let s = sessions(&events);
    assert_eq!(s[0].queue_id, Some(440));
    assert_eq!((s[0].my_team.len(), s[0].their_team.len()), (5, 5));
    // The local player isn't part of either swap.
    assert!(s.iter().all(|s| s.local_player_cell_id == 5));
    let position = |s: &ChampSelectSession, cell| {
        s.my_team
            .iter()
            .find(|p| p.cell_id == cell)
            .unwrap()
            .assigned_position
    };
    // Who sits in a cell (redacted, but stable within the file).
    let player = |i: usize, cell: i64| {
        raw[i]["myTeam"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["cellId"] == cell)
            .unwrap()["puuid"]
            .clone()
    };
    let last = s.len() - 1;
    let busy = |swaps: &[types::SwapContract], a, b| {
        [a, b].iter().all(|c| {
            swaps
                .iter()
                .any(|w| w.cell_id == *c && w.state == SwapState::Busy)
        })
    };
    // Position swap 8 <-> 9: asked twice (the first went nowhere), then done.
    // Players keep their cells; only the positions trade.
    let busy_runs = s
        .windows(2)
        .filter(|w| !busy(&w[0].position_swaps, 8, 9) && busy(&w[1].position_swaps, 8, 9))
        .count();
    assert_eq!(busy_runs, 2);
    // A bystander never sees the request itself, only `BUSY` on both cells.
    assert!(s.iter().all(|s| {
        s.position_swaps
            .iter()
            .chain(&s.pick_order_swaps)
            .all(|w| w.state != SwapState::Sent && w.state != SwapState::Received)
    }));
    assert_eq!(position(&s[0], 8), position(&s[last], 9));
    assert_eq!(position(&s[0], 9), position(&s[last], 8));
    assert_ne!(position(&s[0], 8), position(&s[0], 9));
    assert_eq!(player(0, 8), player(last, 8));
    assert_eq!(player(0, 9), player(last, 9));
    // Pick-order swap 6 <-> 7 between two teammates: the player entries trade
    // cells, positions included.
    assert!(s.iter().any(|s| busy(&s.pick_order_swaps, 6, 7)));
    assert_eq!(position(&s[0], 6), position(&s[last], 7));
    assert_eq!(position(&s[0], 7), position(&s[last], 6));
    assert_eq!(player(0, 6), player(last, 7));
    assert_eq!(player(0, 7), player(last, 6));
    // Ranked flex: teammates' names visible, the enemy's hidden.
    let visibility = |team: &str| {
        raw[0][team]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["nameVisibilityType"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert!(visibility("myTeam").iter().all(|v| v == "VISIBLE"));
    assert!(visibility("theirTeam").iter().all(|v| v == "HIDDEN"));
    // A full draft through to the game.
    assert_eq!(s[last].timer.phase, TimerPhase::GameStarting);
    let actions: Vec<_> = s[last].actions.iter().flatten().collect();
    assert_eq!(
        actions
            .iter()
            .filter(|a| a.kind == ActionType::Ban && a.completed)
            .count(),
        10
    );
    assert_eq!(
        actions
            .iter()
            .filter(|a| a.kind == ActionType::Pick && a.completed && a.champion().is_some())
            .count(),
        10
    );
    assert!(s[last].their_team.iter().all(|p| p.champion().is_some()));
    assert_eq!(phases(&events).last(), Some(&GameflowPhase::GameStart));
    assert!(
        dodge_states(&events)
            .iter()
            .all(|d| *d == DodgeState::Invalid)
    );
}

#[test]
fn rcvolus_solo_custom_ban_pick() {
    if missing() {
        return;
    }
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
    if missing() {
        return;
    }
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
    if missing() {
        return;
    }
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
    if missing() {
        return;
    }
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

#[test]
fn clash_round1_scouting_to_draft() {
    if missing() {
        return;
    }
    let (header, events) = load("16_23_clash_round1_scouting_to_draft");
    assert_eq!(header.label, "clash");
    assert!(header.client_version.unwrap().starts_with("16.23."));
    // Clash data is untyped: read it raw, last value per URI.
    let latest = |uri: &str| {
        events
            .iter()
            .rev()
            .find(|e| e.uri == uri && !e.data.is_null())
            .map(|e| &e.data)
    };
    let mut states: Vec<String> = events
        .iter()
        .filter(|e| e.uri == "/lol-clash/v1/tournament-summary" && !e.data.is_null())
        .map(|e| e.data[0]["state"].as_str().unwrap().to_owned())
        .collect();
    states.dedup();
    assert_eq!(states, ["LOCK_IN", "SCOUTING", "IN_GAME"]);
    let summary = &latest("/lol-clash/v1/tournament-summary").unwrap()[0];
    let own_roster = summary["rosterId"].as_str().unwrap();
    assert!(own_roster.starts_with("redacted-"));
    // The bracket lists 16 teams, and every one of their rosters is recorded.
    let bracket = latest(&format!(
        "/lol-clash/v1/bracket/redacted-{}",
        summary["bracketId"]
    ))
    .expect("the bracket");
    let teams = bracket["rosters"].as_array().unwrap();
    assert_eq!(teams.len(), 16);
    let positions = |uri: &str| -> Vec<String> {
        latest(uri).unwrap_or_else(|| panic!("{uri}"))["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["position"].as_str().unwrap().to_owned())
            .collect()
    };
    // The same fake throughout: bracket roster N is `roster/redacted-N`.
    for team in teams {
        assert!(team["name"].as_str().unwrap().starts_with("redacted-"));
        let uri = format!("/lol-clash/v1/roster/redacted-{}", team["rosterId"]);
        let p = positions(&uri);
        assert_eq!(p.len(), 5, "{uri}");
        // The client gives other teams' positions only as FILL.
        if !uri.ends_with(own_roster) {
            assert!(p.iter().all(|p| p == "FILL"), "{uri}: {p:?}");
        }
    }
    // Our own roster has five real positions.
    let mut own = positions(&format!("/lol-clash/v1/roster/{own_roster}"));
    own.sort();
    assert_eq!(own, ["BOTTOM", "JUNGLE", "MIDDLE", "TOP", "UTILITY"]);
    clash_tournament_draft(&events);
    assert_eq!(
        phases(&events),
        [
            GameflowPhase::None,
            GameflowPhase::CheckedIntoTournament,
            GameflowPhase::ReadyCheck,
            GameflowPhase::ChampSelect,
            GameflowPhase::GameStart,
        ]
    );
}

/// Clash champ select: a tournament draft to the end, enemies hidden like in
/// ranked. Returns the last session.
fn clash_tournament_draft(events: &[RawEvent]) -> ChampSelectSession {
    for e in events
        .iter()
        .filter(|e| e.uri == SESSION_URI && !e.data.is_null())
    {
        for p in e.data["theirTeam"].as_array().unwrap() {
            assert_eq!(p["nameVisibilityType"], "HIDDEN");
            assert_eq!(p["puuid"], "");
        }
        for p in e.data["myTeam"].as_array().unwrap() {
            assert_eq!(p["nameVisibilityType"], "VISIBLE");
        }
    }
    let mut s = sessions(events);
    assert_eq!(s[0].queue_id, Some(700));
    let last = s.pop().unwrap();
    assert_eq!(last.timer.phase, TimerPhase::GameStarting);
    // A phase transition, then 3 ban pairs, 3 picks each, 2 ban pairs,
    // 2 picks each, each block after its own transition.
    use ActionType::{Ban as B, PhaseTransition as T, Pick as P};
    let order: Vec<ActionType> = last.actions.iter().flatten().map(|a| a.kind).collect();
    assert_eq!(
        order,
        [
            T, B, B, B, B, B, B, T, P, P, P, P, P, P, T, B, B, B, B, T, P, P, P, P
        ]
    );
    assert!(last.actions.iter().flatten().all(|a| a.completed));
    // Bans by each team's captain only: one cell per side.
    for ally in [true, false] {
        let mut cells: Vec<i64> = last
            .actions
            .iter()
            .flatten()
            .filter(|a| a.kind == B && a.is_ally_action == Some(ally))
            .map(|a| a.actor_cell_id)
            .collect();
        cells.dedup();
        assert_eq!(cells.len(), 1, "ally {ally}: {cells:?}");
    }
    assert!(last.their_team.iter().all(|p| p.champion().is_some()));
    assert!(last.bans.my_team_bans.is_empty() && last.bans.their_team_bans.is_empty());
    last
}

#[test]
fn clash_round3_scouting_snapshot_to_draft() {
    if missing() {
        return;
    }
    let (header, events) = load("16_23_clash_round3_scouting_snapshot_to_draft");
    assert_eq!(header.label, "clash");
    let uris = |prefix: &str| -> Vec<&RawEvent> {
        events
            .iter()
            .filter(|e| e.uri.starts_with(prefix) && !e.data.is_null())
            .collect()
    };
    let mut states: Vec<&str> = uris("/lol-clash/v1/tournament-summary")
        .iter()
        .map(|e| e.data[0]["state"].as_str().unwrap())
        .collect();
    states.dedup();
    assert_eq!(states, ["SCOUTING", "IN_GAME"]);
    // The scouting snapshot, taken once: what the client's scouting tab
    // shows for each opponent.
    let champions = uris("/lol-clash/v1/scouting/champions");
    assert_eq!(champions.len(), 1);
    let players = champions[0].data.as_array().unwrap();
    assert_eq!(players.len(), 5);
    for p in players {
        assert!(p["totalMasteryScore"].as_i64().unwrap() > 0);
        assert!(!p["topMasteries"].as_array().unwrap().is_empty());
        assert!(!p["topSeasonChampions"].as_array().unwrap().is_empty());
    }
    let ranked = uris("/lol-ranked/v1/ranked-stats/");
    assert_eq!(ranked.len(), 5);
    let mut distinct: Vec<&str> = ranked.iter().map(|e| e.uri.as_str()).collect();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), 5);
    for e in &ranked {
        assert!(e.uri.starts_with("/lol-ranked/v1/ranked-stats/redacted-"));
        assert!(e.data["queueMap"]["RANKED_SOLO_5x5"]["tier"].is_string());
    }
    // The scouted players are the opponent's roster: the other side of our
    // open match in the bracket.
    let latest = |uri: &str| {
        events
            .iter()
            .rev()
            .find(|e| e.uri == uri && !e.data.is_null())
            .map(|e| &e.data)
            .unwrap_or_else(|| panic!("{uri}"))
    };
    let summary = &latest("/lol-clash/v1/tournament-summary")[0];
    let ours: u64 = summary["rosterId"].as_str().unwrap()["redacted-".len()..]
        .parse()
        .unwrap();
    let bracket = latest(&format!(
        "/lol-clash/v1/bracket/redacted-{}",
        summary["bracketId"]
    ));
    let open = bracket["matches"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["winnerId"] == 0 && (m["rosterId1"] == ours || m["rosterId2"] == ours))
        .max_by_key(|m| m["round"].as_i64())
        .expect("our open match");
    let opp = if open["rosterId1"] == ours {
        &open["rosterId2"]
    } else {
        &open["rosterId1"]
    };
    let members = |roster: &Value| -> Vec<(u64, String)> {
        roster["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                (
                    m["summonerId"].as_u64().unwrap(),
                    m["position"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    };
    let theirs = members(latest(&format!("/lol-clash/v1/roster/redacted-{opp}")));
    let mut scouted: Vec<u64> = players
        .iter()
        .map(|p| p["playerId"].as_u64().unwrap())
        .collect();
    let mut roster_ids: Vec<u64> = theirs.iter().map(|(id, _)| *id).collect();
    scouted.sort_unstable();
    roster_ids.sort_unstable();
    assert_eq!(scouted, roster_ids);
    let mut own: Vec<String> = members(latest(&format!("/lol-clash/v1/roster/redacted-{ours}")))
        .into_iter()
        .map(|(_, p)| p)
        .collect();
    own.sort();
    assert_eq!(own, ["BOTTOM", "JUNGLE", "MIDDLE", "TOP", "UTILITY"]);
    // Clash match history is refused by the server (403); not recorded.
    assert!(uris("/lol-clash/v1/scouting/matchhistory").is_empty());
    let last = clash_tournament_draft(&events);
    // Every enemy pick was in some opponent's scouting lists.
    for c in last.their_team.iter().filter_map(|p| p.champion()) {
        assert!(
            players
                .iter()
                .any(|p| ["topMasteries", "topSeasonChampions"]
                    .iter()
                    .flat_map(|k| p[*k].as_array().unwrap())
                    .any(|x| x["championId"] == c)),
            "pick {c} not scouted"
        );
    }
    assert_eq!(
        phases(&events),
        [
            GameflowPhase::CheckedIntoTournament,
            GameflowPhase::ReadyCheck,
            GameflowPhase::ChampSelect,
            GameflowPhase::GameStart,
        ]
    );
}
