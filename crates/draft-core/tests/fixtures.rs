//! Replays every recording in `fixtures/` through the [`Tracker`] and
//! snapshots each distinct draft state (golden files in `tests/snapshots/`).
//! A change in a snapshot is a change in what every UI shows: review it with
//! `cargo insta review` or by reading the diff, then accept with
//! `INSTA_UPDATE=always cargo test -p draft-core`.
//!
//! The situation tests below check the facts each recording is named for, so a
//! wrongly accepted snapshot still fails.

use std::path::{Path, PathBuf};

use draft_core::{DraftState, Phase, Tracker, TurnKind};
use lcu_model::recording::read_file;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// The draft after every event that changed it, consecutive duplicates
/// dropped; `None` when champ select ended.
fn replay(name: &str) -> Vec<Option<DraftState>> {
    let path = fixtures_dir().join(format!("{name}.jsonl"));
    let (_, events) = read_file(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut tracker = Tracker::new();
    let mut states: Vec<Option<DraftState>> = Vec::new();
    for ev in &events {
        if tracker.apply(ev) {
            let s = tracker.state();
            if states.last() != Some(&s) {
                states.push(s);
            }
        }
    }
    states
}

fn drafts(name: &str) -> Vec<DraftState> {
    replay(name).into_iter().flatten().collect()
}

#[test]
fn every_fixture_matches_its_snapshot() {
    let mut names: Vec<String> = std::fs::read_dir(fixtures_dir())
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            (p.extension()? == "jsonl").then(|| p.file_stem().unwrap().to_string_lossy().into())
        })
        .collect();
    names.sort();
    assert!(names.len() >= 10, "{names:?}");
    for name in names {
        let states = replay(&name);
        assert!(
            states.iter().any(Option::is_some),
            "{name}: no draft at all"
        );
        insta::assert_yaml_snapshot!(name.clone(), states);
    }
}

#[test]
fn missed_ban_is_a_ban_without_champion() {
    let last = drafts("16_22_draft_missed_ban").pop().unwrap();
    let local = last.local().unwrap().cell_id;
    let mine = last
        .bans
        .ally
        .iter()
        .find(|b| b.cell_id == Some(local))
        .expect("the local player's ban");
    assert_eq!(mine.champion, None);
}

#[test]
fn planning_hovers_show_as_hovers_without_a_turn() {
    let d = drafts("16_22_draft_planning_hovers");
    assert!(
        d.iter()
            .all(|s| s.phase == Phase::Planning && s.turn.is_none())
    );
    let mut hovers: Vec<i64> = d.iter().filter_map(|s| s.local()?.hover).collect();
    hovers.dedup();
    assert!(hovers.len() >= 3, "{hovers:?}");
}

#[test]
fn autolock_turns_the_hover_into_the_pick() {
    let d = drafts("16_22_draft_ban_and_pick_timeout_autolock");
    let hovered = d
        .iter()
        .rev()
        .find_map(|s| s.local().filter(|l| l.picking)?.hover)
        .expect("a hover during the pick turn");
    let last = d.last().unwrap();
    assert_eq!(last.phase, Phase::Finalization);
    assert_eq!(last.local().unwrap().champion, Some(hovered));
    assert!(d.iter().any(|s| {
        s.turn
            .as_ref()
            .is_some_and(|t| t.yours && t.kind == TurnKind::Pick)
    }));
}

#[test]
fn kicked_on_pick_timeout_ends_the_draft() {
    let states = replay("16_22_draft_pick_timeout_kicked");
    assert_eq!(states.last(), Some(&None), "champ select ended");
    let last = states.iter().flatten().last().unwrap();
    assert!(last.local().unwrap().champion.is_none());
}

#[test]
fn aram_has_a_bench_and_rerolls() {
    let d = drafts("16_22_aram_bench_swaps");
    assert!(d.iter().all(|s| s.rerolls_remaining.is_some()));
    let mut benches: Vec<&Vec<i64>> = d.iter().map(|s| &s.bench).collect();
    benches.dedup();
    assert!(benches.len() >= 3, "bench changes with swaps");
    assert!(
        d.iter()
            .all(|s| s.ally.iter().all(|p| p.champion.is_some()))
    );
}

#[test]
fn subset_offer_reaches_the_draft() {
    let d = drafts("16_22_subset_pick_swaps_dodge");
    let offered = &d.last().unwrap().offered;
    assert_eq!(offered.len(), 3);
    let mut mine: Vec<i64> = d.iter().filter_map(|s| s.local()?.champion).collect();
    mine.dedup();
    assert!(
        mine.len() >= 3 && mine.iter().all(|c| offered.contains(c)),
        "{mine:?}"
    );
}

#[test]
fn pick_order_swap_moves_the_local_slot() {
    let states = replay("16_23_normal_draft_pick_order_swap_stranger_dodge");
    assert_eq!(states.last(), Some(&None), "a stranger dodged");
    let d: Vec<DraftState> = states.into_iter().flatten().collect();
    let (first, last) = (d.first().unwrap(), d.last().unwrap());
    let (before, after) = (first.local().unwrap(), last.local().unwrap());
    assert_eq!((before.cell_id, after.cell_id), (6, 5));
    // You keep your position; the other player takes yours over.
    assert_eq!(before.position, after.position);
    assert_eq!(last.ally.iter().filter(|s| s.is_local).count(), 1);
}

#[test]
fn spectated_tournament_draft_ends_with_ten_bans_and_ten_picks() {
    let d = drafts("rcvolus_2019_tournament_draft_spectated");
    let last = d.last().unwrap();
    assert!(last.spectating && last.local().is_none());
    assert_eq!((last.bans.ally.len(), last.bans.enemy.len()), (5, 5));
    assert!(
        last.ally
            .iter()
            .chain(&last.enemy)
            .all(|s| s.champion.is_some())
    );
    // Both kinds of turn occur, and none is "yours" for a spectator.
    assert!(d.iter().filter_map(|s| s.turn.as_ref()).all(|t| !t.yours));
    let kinds: Vec<TurnKind> = d
        .iter()
        .filter_map(|s| Some(s.turn.as_ref()?.kind))
        .collect();
    assert!(kinds.contains(&TurnKind::Ban) && kinds.contains(&TurnKind::Pick));
}
