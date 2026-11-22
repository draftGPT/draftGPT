//! Typed views of the LCU resources we use, parsed from `RawEvent.data`.
//!
//! Every struct is lenient: missing fields default, unknown fields are ignored,
//! and string enums fall back to `Unknown`, because Riot changes the schema
//! without notice. Player-identifying fields (puuid, names, summoner IDs) are
//! intentionally not modelled. Nothing downstream needs them.
//!
//! Reference: `docs/lcu-spec/` (`TeamBuilderDirect-ChampSelectSession` and
//! friends). The spec doesn't list enum values; those come from captures.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Parses LCU data into a typed struct. Object fields that are `null` are
/// dropped first, so they fall back to their defaults like missing fields do.
/// (Plain `#[serde(default)]` only covers missing fields; a `null` in a
/// non-`Option` field would fail the whole parse.)
pub fn parse<T: DeserializeOwned>(data: &Value) -> Result<T, serde_json::Error> {
    serde_json::from_value(without_nulls(data))
}

fn without_nulls(v: &Value) -> Value {
    match v {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k.clone(), without_nulls(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(without_nulls).collect()),
        other => other.clone(),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChampSelectSession {
    pub game_id: Option<u64>,
    pub queue_id: Option<i64>,
    pub timer: Timer,
    pub my_team: Vec<PlayerSelection>,
    pub their_team: Vec<PlayerSelection>,
    /// One inner list per turn. The spec types this as `[object]`.
    pub actions: Vec<Vec<Action>>,
    pub bans: Bans,
    pub local_player_cell_id: i64,
    pub is_spectating: bool,
    pub is_custom_game: bool,
    pub has_simultaneous_bans: bool,
    pub has_simultaneous_picks: bool,
    pub allow_duplicate_picks: bool,
    pub bench_enabled: bool,
    pub bench_champions: Vec<BenchChampion>,
    /// Pick-one-of-a-few modes (queue 3270); the offered champions come from
    /// `SUBSET_CHAMPION_LIST`, not the session.
    pub allow_subset_champion_picks: bool,
    /// ARAM-style modes; `rerolls_remaining` is the local player's count.
    pub allow_rerolling: bool,
    pub rerolls_remaining: i64,
    pub trades: Vec<SwapContract>,
    pub pick_order_swaps: Vec<SwapContract>,
    pub position_swaps: Vec<SwapContract>,
    pub counter: i64,
}

impl ChampSelectSession {
    pub fn local_player(&self) -> Option<&PlayerSelection> {
        self.my_team
            .iter()
            .find(|p| p.cell_id == self.local_player_cell_id)
    }

    /// Actions currently in progress (more than one with simultaneous bans).
    pub fn in_progress(&self) -> impl Iterator<Item = &Action> {
        self.actions.iter().flatten().filter(|a| a.is_in_progress)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Timer {
    pub phase: TimerPhase,
    pub adjusted_time_left_in_phase: i64,
    pub total_time_in_phase: i64,
    pub is_infinite: bool,
    pub internal_now_in_epoch_ms: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TimerPhase {
    Planning,
    BanPick,
    Finalization,
    GameStarting,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlayerSelection {
    pub cell_id: i64,
    pub team: i64,
    pub champion_id: i64,
    /// Hovered champion (own team only; the enemy's hovers are never sent).
    pub champion_pick_intent: i64,
    pub assigned_position: Position,
    pub spell1_id: i64,
    pub spell2_id: i64,
    pub is_autofilled: bool,
    pub pick_turn: i64,
}

impl PlayerSelection {
    /// Locked or picked champion; `0` means none.
    pub fn champion(&self) -> Option<i64> {
        champion_id(self.champion_id)
    }

    pub fn hover(&self) -> Option<i64> {
        champion_id(self.champion_pick_intent)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Position {
    Top,
    Jungle,
    Middle,
    Bottom,
    Utility,
    /// `""`: blind pick, custom games, or not assigned yet.
    #[default]
    #[serde(rename = "")]
    Unassigned,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Action {
    pub id: i64,
    pub actor_cell_id: i64,
    pub champion_id: i64,
    #[serde(rename = "type")]
    pub kind: ActionType,
    pub completed: bool,
    pub is_in_progress: bool,
    /// Missing in older clients.
    pub is_ally_action: Option<bool>,
    pub pick_turn: i64,
    pub duration: i64,
}

impl Action {
    pub fn champion(&self) -> Option<i64> {
        champion_id(self.champion_id)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    Pick,
    Ban,
    /// Reveals simultaneous bans; `actorCellId` is -1.
    TenBansReveal,
    // Seen only in other projects' code (LeagueAkari, leaguebroadcast), not in
    // captures yet; voting modes like URF.
    Vote,
    PhaseTransition,
    VoteTransition,
    TeamVoteReveal,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Bans {
    pub my_team_bans: Vec<i64>,
    pub their_team_bans: Vec<i64>,
    pub num_bans: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BenchChampion {
    pub champion_id: i64,
    pub is_priority: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SwapContract {
    pub id: i64,
    pub cell_id: i64,
    pub state: SwapState,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SwapState {
    Accepted,
    Cancelled,
    Declined,
    Sent,
    Received,
    Invalid,
    Busy,
    Available,
    #[default]
    #[serde(other)]
    Unknown,
}

/// `/lol-gameflow/v1/gameflow-phase`; the data is a bare JSON string.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, serde::Serialize)]
pub enum GameflowPhase {
    #[default]
    None,
    Lobby,
    Matchmaking,
    CheckedIntoTournament,
    ReadyCheck,
    ChampSelect,
    GameStart,
    FailedToLaunch,
    InProgress,
    Reconnect,
    WaitingForStats,
    PreEndOfGame,
    EndOfGame,
    TerminatedInError,
    #[serde(other)]
    Unknown,
}

/// `/lol-lobby-team-builder/champ-select/v1/subset-champion-list`: champions
/// offered to the local player, a bare array of IDs.
pub type SubsetChampionList = Vec<i64>;

/// `/lol-lobby-team-builder/v1/matchmaking`. Only what explains how a champ
/// select ended; IDs (`dodgerId`, `declinerIds`) aren't modelled.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Matchmaking {
    pub search_state: String,
    pub is_currently_in_queue: bool,
    pub queue_id: i64,
    pub dodge_data: DodgeData,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DodgeData {
    pub state: DodgeState,
}

/// Values from 16.22 and 16.23 captures. `Invalid` is "no dodge".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub enum DodgeState {
    Invalid,
    /// The local party (you) left champ select.
    PartyDodged,
    /// Another player left champ select; you're put back in the queue.
    StrangerDodged,
    #[default]
    #[serde(other)]
    Unknown,
}

/// Champion IDs are positive. `0` means none; a missed ban completes with `-1`.
fn champion_id(id: i64) -> Option<i64> {
    (id > 0).then_some(id)
}

#[cfg(test)]
#[path = "../tests/unit/types.rs"]
mod tests;
