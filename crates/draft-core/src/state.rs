//! `DraftState`: the draft as every frontend shows it. Built from the client's
//! champ select session (live or replayed) by [`crate::reducer`], or later from
//! manual picks in the web drafter.
//!
//! Only what a viewer may see: no names or account IDs (the session types
//! don't even model them), and no enemy hovers (the client never sends them).

use serde::{Deserialize, Serialize};

/// Champion IDs as the client uses them (`103` = Ahri). Names and icons come
/// from champion data, not from here.
pub type ChampionId = i64;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DraftState {
    pub phase: Phase,
    pub queue_id: Option<i64>,
    /// Watching someone else's draft; there's no local player then.
    pub spectating: bool,
    /// Your team. In a spectated game, the blue side.
    pub ally: Vec<Slot>,
    /// The other team, in the order the client lists it. Empty when the mode
    /// has no visible enemies (e.g. pick-one-of-few customs).
    pub enemy: Vec<Slot>,
    pub bans: Bans,
    /// Who acts right now; `None` in planning, finalization and between turns.
    pub turn: Option<Turn>,
    pub timer: Timer,
    /// ARAM-style shared bench of champions anyone on your team can take.
    pub bench: Vec<ChampionId>,
    /// Champions offered to you in pick-one-of-few modes.
    pub offered: Vec<ChampionId>,
    pub rerolls_remaining: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// Hovering and declaring intent before any ban or pick.
    Planning,
    BanPick,
    /// Everyone has picked: runes, skins, swaps.
    Finalization,
    GameStarting,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    /// The client's cell ID; turns and bans refer to it.
    pub cell_id: i64,
    pub is_local: bool,
    /// Assigned role in ranked and draft queues; `None` in blind, ARAM, customs.
    pub position: Option<Position>,
    /// Locked in.
    pub champion: Option<ChampionId>,
    /// Hovered but not locked (own team only).
    pub hover: Option<ChampionId>,
    /// This slot is picking right now.
    pub picking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum Position {
    Top,
    Jungle,
    Middle,
    Bottom,
    Utility,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Bans {
    pub ally: Vec<Ban>,
    pub enemy: Vec<Ban>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Ban {
    /// `None` for a ban the timer ran out on.
    pub champion: Option<ChampionId>,
    /// The banning cell, if known (older clients only give the champion list).
    pub cell_id: Option<i64>,
    /// A ban not locked yet: the champion being hovered to ban (your team's
    /// in ranked, both teams' in tournament draft; `champion` is `None` until
    /// it locks).
    #[serde(default)]
    pub hover: Option<ChampionId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub kind: TurnKind,
    /// Cells acting now: one in a normal turn, several with simultaneous bans
    /// or picks.
    pub cells: Vec<i64>,
    /// The local player is one of them.
    pub yours: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum TurnKind {
    Ban,
    Pick,
}

/// Time left in the current phase when this state was produced. A UI counts
/// down from when it received the state (`remaining_ms` from then), which also
/// works for replays and when the client's clock differs from the PC's.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Timer {
    /// `None` if the phase has no time limit.
    pub remaining_ms: Option<i64>,
    pub total_ms: i64,
}

impl Slot {
    /// The champion to show: locked, else hovered.
    pub fn shown(&self) -> Option<ChampionId> {
        self.champion.or(self.hover)
    }
}

impl DraftState {
    pub fn local(&self) -> Option<&Slot> {
        self.ally.iter().find(|s| s.is_local)
    }

    /// Every champion locked or banned by anyone: no longer available in modes
    /// without duplicate picks.
    pub fn unavailable(&self) -> impl Iterator<Item = ChampionId> + '_ {
        let picked = self
            .ally
            .iter()
            .chain(&self.enemy)
            .filter_map(|s| s.champion);
        let banned = self
            .bans
            .ally
            .iter()
            .chain(&self.bans.enemy)
            .filter_map(|b| b.champion);
        picked.chain(banned)
    }
}
