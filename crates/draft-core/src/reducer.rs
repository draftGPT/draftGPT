//! Champ select session → [`DraftState`].
//!
//! [`from_session`] is a pure function of one session. [`Tracker`] follows a
//! stream of raw events (live or replayed) and keeps the current state, adding
//! what the session doesn't hold (the champions offered in pick-one-of-few
//! modes).
//!
//! Rules taken from 16.22 captures (see `fixtures/README.md`):
//! - A team member's `championId` is the locked champion in every mode (draft,
//!   ARAM, pick-one-of-few swaps); it stays `0` while hovering.
//! - The hover is the in-progress pick action's champion during the player's
//!   turn, `championPickIntent` before it.
//! - Bans come from completed ban actions (the `bans` summary is empty with
//!   simultaneous bans); the summary is only a fallback for old recordings.
//! - A missed ban completes with `championId: -1`.
//! - In planning every ban action is "in progress"; that isn't a turn.
//! - Clients before about 2020 don't send `isInProgress`; there the current
//!   turn is the first group of actions with unfinished bans or picks.
//! - The client doesn't refresh `timer` on every session update: the same
//!   `adjustedTimeLeftInPhase` comes with later updates, measured at the same
//!   `internalNowInEpochMs`. The tracker counts time from when it first saw
//!   that measurement.
//! - Champ select ends without a delete of the main session URI: the
//!   team-builder copy is deleted, then the gameflow phase leaves
//!   `ChampSelect`. Either ends the draft. Old recordings only have the main
//!   session, which they delete.

use lcu_model::types::GameflowPhase;
use lcu_model::types::{
    self, Action, ActionType, ChampSelectSession, PlayerSelection, SubsetChampionList, TimerPhase,
};
use lcu_model::{
    EventKind, GAMEFLOW_PHASE_URI, RawEvent, SESSION_URI, SUBSET_CHAMPION_LIST_URI,
    TEAM_BUILDER_SESSION_URI,
};

use crate::state::{
    Ban, Bans, ChampionId, DraftState, Phase, Position, Slot, Timer, Turn, TurnKind,
};

/// The draft shown by one session.
pub fn from_session(s: &ChampSelectSession) -> DraftState {
    let local = (!s.is_spectating && s.local_player_cell_id >= 0).then_some(s.local_player_cell_id);
    let phase = phase(s.timer.phase);
    let turn = turn(s, phase, local);
    let picking = |cell: i64| {
        turn.as_ref()
            .is_some_and(|t| t.kind == TurnKind::Pick && t.cells.contains(&cell))
    };
    let slot = |p: &PlayerSelection, own_team: bool| Slot {
        cell_id: p.cell_id,
        is_local: Some(p.cell_id) == local,
        position: position(p.assigned_position),
        champion: p.champion(),
        hover: if own_team && p.champion().is_none() {
            hover(s, p)
        } else {
            None
        },
        picking: picking(p.cell_id),
    };
    DraftState {
        phase,
        queue_id: s.queue_id,
        spectating: s.is_spectating,
        ally: s.my_team.iter().map(|p| slot(p, true)).collect(),
        enemy: s.their_team.iter().map(|p| slot(p, false)).collect(),
        bans: bans(s),
        turn,
        timer: timer(s),
        bench: s.bench_champions.iter().map(|b| b.champion_id).collect(),
        offered: Vec::new(),
        rerolls_remaining: s.allow_rerolling.then_some(s.rerolls_remaining),
    }
}

/// Follows raw events and keeps the current draft. The session itself is read
/// from the main session URI only (the team-builder copy carries the same
/// data); its deletion and the gameflow phase only end the draft.
#[derive(Debug, Default)]
pub struct Tracker {
    session: Option<ChampSelectSession>,
    offered: SubsetChampionList,
    /// The timer measurement in the session (`internalNowInEpochMs`, time
    /// left, phase) and the event time it first arrived at; `None` when the
    /// client sends no measurement time. And the latest applied event's time.
    timer_seen: Option<((i64, i64, TimerPhase), u64)>,
    last_t: u64,
}

impl Tracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies one event. Returns whether the draft may have changed.
    /// A session that fails to parse is ignored (the typed structs are
    /// lenient, so that means the data isn't a session at all).
    pub fn apply(&mut self, ev: &RawEvent) -> bool {
        let gone = ev.kind == EventKind::Delete || ev.data.is_null();
        match ev.uri.as_str() {
            SESSION_URI | TEAM_BUILDER_SESSION_URI if gone => self.end(),
            SESSION_URI => match types::parse::<ChampSelectSession>(&ev.data) {
                Ok(s) => {
                    let t = &s.timer;
                    let measured = (
                        t.internal_now_in_epoch_ms,
                        t.adjusted_time_left_in_phase,
                        t.phase,
                    );
                    if t.internal_now_in_epoch_ms <= 0 {
                        // No measurement time: take the time left as sent.
                        self.timer_seen = None;
                    } else if self.timer_seen.is_none_or(|(m, _)| m != measured) {
                        self.timer_seen = Some((measured, ev.t));
                    }
                    self.last_t = ev.t;
                    self.session = Some(s);
                    true
                }
                Err(_) => false,
            },
            GAMEFLOW_PHASE_URI => match types::parse::<GameflowPhase>(&ev.data) {
                Ok(GameflowPhase::ChampSelect) | Err(_) => false,
                Ok(_) => self.end(),
            },
            SUBSET_CHAMPION_LIST_URI => {
                self.last_t = self.last_t.max(ev.t);
                self.offered = if gone {
                    Vec::new()
                } else {
                    types::parse(&ev.data).unwrap_or_default()
                };
                self.session.is_some()
            }
            _ => false,
        }
    }

    fn end(&mut self) -> bool {
        self.offered.clear();
        self.timer_seen = None;
        self.session.take().is_some()
    }

    /// The current draft; `None` outside champ select.
    pub fn state(&self) -> Option<DraftState> {
        let mut d = from_session(self.session.as_ref()?);
        d.offered.clone_from(&self.offered);
        if let (Some(left), Some((_, seen))) = (d.timer.remaining_ms.as_mut(), self.timer_seen) {
            let since = i64::try_from(self.last_t.saturating_sub(seen)).unwrap_or(i64::MAX);
            *left = left.saturating_sub(since).max(0);
        }
        Some(d)
    }
}

fn phase(p: TimerPhase) -> Phase {
    match p {
        TimerPhase::Planning => Phase::Planning,
        TimerPhase::BanPick => Phase::BanPick,
        TimerPhase::Finalization => Phase::Finalization,
        TimerPhase::GameStarting => Phase::GameStarting,
        TimerPhase::Unknown => Phase::Unknown,
    }
}

fn position(p: types::Position) -> Option<Position> {
    match p {
        types::Position::Top => Some(Position::Top),
        types::Position::Jungle => Some(Position::Jungle),
        types::Position::Middle => Some(Position::Middle),
        types::Position::Bottom => Some(Position::Bottom),
        types::Position::Utility => Some(Position::Utility),
        types::Position::Unassigned | types::Position::Unknown => None,
    }
}

fn actions(s: &ChampSelectSession) -> impl Iterator<Item = &Action> {
    s.actions.iter().flatten()
}

fn hover(s: &ChampSelectSession, p: &PlayerSelection) -> Option<ChampionId> {
    let in_turn = actions(s)
        .find(|a| {
            a.kind == ActionType::Pick
                && a.actor_cell_id == p.cell_id
                && a.is_in_progress
                && !a.completed
        })
        .and_then(Action::champion);
    in_turn.or_else(|| p.hover())
}

fn turn(s: &ChampSelectSession, phase: Phase, local: Option<i64>) -> Option<Turn> {
    if phase != Phase::BanPick {
        return None;
    }
    let open = |a: &&Action| !a.completed && matches!(a.kind, ActionType::Ban | ActionType::Pick);
    let mut now: Vec<&Action> = actions(s)
        .filter(open)
        .filter(|a| a.is_in_progress)
        .collect();
    if now.is_empty() && !actions(s).any(|a| a.is_in_progress) {
        // Old clients without `isInProgress`: the first unfinished group.
        now = s
            .actions
            .iter()
            .map(|group| group.iter().filter(open).collect::<Vec<_>>())
            .find(|group| !group.is_empty())
            .unwrap_or_default();
    }
    let first = now.first()?;
    let kind = if first.kind == ActionType::Ban {
        TurnKind::Ban
    } else {
        TurnKind::Pick
    };
    let mut cells: Vec<i64> = now
        .iter()
        .filter(|a| a.kind == first.kind)
        .map(|a| a.actor_cell_id)
        .collect();
    cells.sort_unstable();
    cells.dedup();
    let yours = local.is_some_and(|l| cells.contains(&l));
    Some(Turn { kind, cells, yours })
}

fn bans(s: &ChampSelectSession) -> Bans {
    let allies: Vec<i64> = s.my_team.iter().map(|p| p.cell_id).collect();
    let mut out = Bans::default();
    for a in actions(s).filter(|a| a.kind == ActionType::Ban && a.completed) {
        let ban = Ban {
            champion: a.champion(),
            cell_id: Some(a.actor_cell_id),
        };
        if allies.contains(&a.actor_cell_id) {
            out.ally.push(ban);
        } else {
            out.enemy.push(ban);
        }
    }
    if out.ally.is_empty() && out.enemy.is_empty() {
        let listed = |ids: &[i64]| -> Vec<Ban> {
            ids.iter()
                .map(|&c| Ban {
                    champion: (c > 0).then_some(c),
                    cell_id: None,
                })
                .collect()
        };
        out.ally = listed(&s.bans.my_team_bans);
        out.enemy = listed(&s.bans.their_team_bans);
    }
    out
}

fn timer(s: &ChampSelectSession) -> Timer {
    let t = &s.timer;
    Timer {
        remaining_ms: (!t.is_infinite).then_some(t.adjusted_time_left_in_phase.max(0)),
        total_ms: t.total_time_in_phase,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session(v: serde_json::Value) -> ChampSelectSession {
        types::parse(&v).unwrap()
    }

    #[test]
    fn hover_comes_from_the_turn_then_the_intent_and_never_for_enemies() {
        let s = session(json!({
            "localPlayerCellId": 0,
            "timer": {"phase": "BAN_PICK"},
            "myTeam": [
                {"cellId": 0, "championPickIntent": 103},
                {"cellId": 1, "championPickIntent": 157}
            ],
            "theirTeam": [{"cellId": 5, "championPickIntent": 99}],
            "actions": [[{"actorCellId": 0, "type": "pick", "championId": 7,
                          "isInProgress": true, "completed": false}]]
        }));
        let d = from_session(&s);
        assert_eq!(d.ally[0].hover, Some(7), "in-turn hover wins");
        assert!(d.ally[0].picking && d.ally[0].is_local);
        assert_eq!(d.ally[1].hover, Some(157));
        assert_eq!(d.enemy[0].hover, None);
        let t = d.turn.unwrap();
        assert_eq!(
            (t.kind, t.cells.as_slice(), t.yours),
            (TurnKind::Pick, &[0][..], true)
        );
    }

    #[test]
    fn locked_champion_hides_the_hover() {
        let s = session(json!({
            "myTeam": [{"cellId": 0, "championId": 103, "championPickIntent": 157}]
        }));
        let d = from_session(&s);
        assert_eq!((d.ally[0].champion, d.ally[0].hover), (Some(103), None));
    }

    #[test]
    fn planning_is_not_a_turn_and_simultaneous_bans_are_one_turn() {
        let ban = |cell: i64| json!({"actorCellId": cell, "type": "ban", "isInProgress": true});
        let actions = json!([[ban(0), ban(1), ban(5)]]);
        let planning = session(json!({"timer": {"phase": "PLANNING"}, "actions": actions}));
        assert_eq!(from_session(&planning).turn, None);
        let banning = session(json!({
            "localPlayerCellId": 1, "timer": {"phase": "BAN_PICK"}, "actions": actions
        }));
        let t = from_session(&banning).turn.unwrap();
        assert_eq!(
            (t.kind, t.cells, t.yours),
            (TurnKind::Ban, vec![0, 1, 5], true)
        );
    }

    #[test]
    fn bans_split_by_team_with_missed_bans_and_old_summary_fallback() {
        let s = session(json!({
            "myTeam": [{"cellId": 0}], "theirTeam": [{"cellId": 5}],
            "actions": [[
                {"actorCellId": 0, "type": "ban", "championId": -1, "completed": true},
                {"actorCellId": 5, "type": "ban", "championId": 266, "completed": true},
                {"actorCellId": 5, "type": "ban", "championId": 0, "completed": false}
            ]]
        }));
        let b = from_session(&s).bans;
        assert_eq!(
            b.ally,
            vec![Ban {
                champion: None,
                cell_id: Some(0)
            }]
        );
        assert_eq!(
            b.enemy,
            vec![Ban {
                champion: Some(266),
                cell_id: Some(5)
            }]
        );
        let old = session(json!({"bans": {"myTeamBans": [1, 2], "theirTeamBans": [3]}}));
        let b = from_session(&old).bans;
        assert_eq!(b.ally.len(), 2);
        assert_eq!(
            b.enemy[0],
            Ban {
                champion: Some(3),
                cell_id: None
            }
        );
    }

    #[test]
    fn spectators_have_no_local_player() {
        let s = session(json!({
            "isSpectating": true, "localPlayerCellId": -1,
            "myTeam": [{"cellId": 0}]
        }));
        let d = from_session(&s);
        assert!(d.spectating && d.local().is_none());
    }

    #[test]
    fn timer_is_time_left_and_none_when_infinite() {
        let s = session(json!({"timer": {
            "internalNowInEpochMs": 1_000_000, "adjustedTimeLeftInPhase": 25_000,
            "totalTimeInPhase": 30_000
        }}));
        assert_eq!(
            from_session(&s).timer,
            Timer {
                remaining_ms: Some(25_000),
                total_ms: 30_000
            }
        );
        let late = session(json!({"timer": {"adjustedTimeLeftInPhase": -300}}));
        assert_eq!(from_session(&late).timer.remaining_ms, Some(0));
        let inf = session(json!({"timer": {"isInfinite": true}}));
        assert_eq!(from_session(&inf).timer.remaining_ms, None);
    }

    #[test]
    fn tracker_follows_session_and_offer_and_ends_on_delete() {
        let ev = |uri: &str, kind: EventKind, data: serde_json::Value| RawEvent {
            t: 0,
            uri: uri.into(),
            kind,
            data,
        };
        let mut tr = Tracker::new();
        assert!(tr.state().is_none());
        assert!(!tr.apply(&ev(
            SUBSET_CHAMPION_LIST_URI,
            EventKind::Create,
            json!([1, 2, 3])
        )));
        assert!(tr.apply(&ev(
            SESSION_URI,
            EventKind::Create,
            json!({"myTeam": [{"cellId": 0}]})
        )));
        assert_eq!(tr.state().unwrap().offered, vec![1, 2, 3]);
        // The team-builder copy's content is ignored.
        assert!(!tr.apply(&ev(TEAM_BUILDER_SESSION_URI, EventKind::Update, json!({}))));
        assert!(!tr.apply(&ev(
            GAMEFLOW_PHASE_URI,
            EventKind::Update,
            json!("ChampSelect")
        )));
        assert!(tr.apply(&ev(SESSION_URI, EventKind::Delete, serde_json::Value::Null)));
        assert!(tr.state().is_none());
    }

    #[test]
    fn team_builder_delete_or_leaving_the_phase_ends_the_draft() {
        let ev = |uri: &str, kind: EventKind, data: serde_json::Value| RawEvent {
            t: 0,
            uri: uri.into(),
            kind,
            data,
        };
        for end in [
            ev(
                TEAM_BUILDER_SESSION_URI,
                EventKind::Delete,
                serde_json::Value::Null,
            ),
            ev(GAMEFLOW_PHASE_URI, EventKind::Update, json!("Lobby")),
        ] {
            let mut tr = Tracker::new();
            tr.apply(&ev(SESSION_URI, EventKind::Update, json!({})));
            assert!(tr.state().is_some());
            assert!(tr.apply(&end), "{}", end.uri);
            assert!(tr.state().is_none(), "{}", end.uri);
        }
    }

    #[test]
    fn old_clients_without_in_progress_use_the_first_open_group() {
        let s = session(json!({
            "timer": {"phase": "BAN_PICK"},
            "actions": [
                [{"actorCellId": 0, "type": "ban", "championId": 1, "completed": true}],
                [{"actorCellId": 5, "type": "ban"}],
                [{"actorCellId": 1, "type": "pick"}]
            ]
        }));
        let t = from_session(&s).turn.unwrap();
        assert_eq!((t.kind, t.cells), (TurnKind::Ban, vec![5]));
    }

    #[test]
    fn a_repeated_timer_measurement_counts_down_from_when_it_was_first_seen() {
        let session = |t: u64, measured: i64, left: i64| RawEvent {
            t,
            uri: SESSION_URI.into(),
            kind: EventKind::Update,
            data: json!({"timer": {"phase": "BAN_PICK", "internalNowInEpochMs": measured,
                                   "adjustedTimeLeftInPhase": left}}),
        };
        let left = |tr: &Tracker| tr.state().unwrap().timer.remaining_ms;
        let mut tr = Tracker::new();
        tr.apply(&session(1_000, 500, 34_000));
        assert_eq!(left(&tr), Some(34_000));
        // Same measurement four seconds later: four seconds less left.
        tr.apply(&session(5_000, 500, 34_000));
        assert_eq!(left(&tr), Some(30_000));
        // A new measurement is taken as is.
        tr.apply(&session(7_000, 6_400, 28_000));
        assert_eq!(left(&tr), Some(28_000));
        tr.apply(&session(99_000, 6_400, 28_000));
        assert_eq!(left(&tr), Some(0));
    }

    #[test]
    fn without_a_measurement_time_the_time_left_is_taken_as_sent() {
        let session = |t: u64, phase: &str, left: i64| RawEvent {
            t,
            uri: SESSION_URI.into(),
            kind: EventKind::Update,
            data: json!({"timer": {"phase": phase, "adjustedTimeLeftInPhase": left}}),
        };
        let mut tr = Tracker::new();
        tr.apply(&session(0, "BAN_PICK", 30_000));
        tr.apply(&session(40_000, "FINALIZATION", 60_000));
        assert_eq!(tr.state().unwrap().timer.remaining_ms, Some(60_000));
    }
}
