//! Auto mode: record each champ select, and each Clash evening from lock-in
//! to the last round. Starts when the gameflow phase becomes `ChampSelect`, or
//! when a Clash tournament the player is in is in lock-in, scouting or in
//! game; pauses while the game itself runs (champ select only); stops when
//! neither holds any more. Recordings started by hand are never stopped by
//! auto mode.

use lcu::types::GameflowPhase;
use lcu::{EventKind, GAMEFLOW_PHASE_URI, RawEvent};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoAction {
    Start,
    Stop,
}

/// The phase carried by a gameflow-phase event, or `None` for other events.
/// A `Delete` (client shutting down) counts as phase `None`.
pub fn phase_of(event: &RawEvent) -> Option<GameflowPhase> {
    if event.uri != GAMEFLOW_PHASE_URI {
        return None;
    }
    if event.kind == EventKind::Delete {
        return Some(GameflowPhase::None);
    }
    Some(lcu::types::parse(&event.data).unwrap_or(GameflowPhase::Unknown))
}

/// Clash tournament states (`tournament-summary`, `tournament-state-info`)
/// during which a Clash recording runs: lock-in (waiting for the bracket),
/// scouting (opponent known) and in game (champ select, game, waiting for the
/// next round).
pub const CLASH_ACTIVE_STATES: &[&str] = &["LOCK_IN", "SCOUTING", "IN_GAME"];

/// Every Clash tournament state in the 16.23 spec.
pub const CLASH_KNOWN_STATES: &[&str] = &[
    "UPCOMING", "LOCK_IN", "SCOUTING", "IN_GAME", "RESULTS", "IDLE",
];

/// The `state` values in a Clash summary or state-info payload (a list of
/// tournaments; a single object is accepted too).
pub fn clash_states(data: &Value) -> Vec<&str> {
    let items: Vec<&Value> = match data {
        Value::Array(items) => items.iter().collect(),
        Value::Object(_) => vec![data],
        _ => Vec::new(),
    };
    items
        .into_iter()
        .filter_map(|t| t.get("state").and_then(Value::as_str))
        .collect()
}

/// True when any of the payloads shows a tournament in an active state.
pub fn clash_active<'a>(payloads: impl IntoIterator<Item = &'a Value>) -> bool {
    payloads.into_iter().any(|d| {
        clash_states(d)
            .iter()
            .any(|s| CLASH_ACTIVE_STATES.contains(s))
    })
}

/// The game itself (from loading to the end screen): never recorded.
fn in_game(phase: GameflowPhase) -> bool {
    use GameflowPhase as P;
    matches!(
        phase,
        P::GameStart
            | P::InProgress
            | P::Reconnect
            | P::WaitingForStats
            | P::PreEndOfGame
            | P::EndOfGame
    )
}

/// Whether auto mode wants a recording running.
pub fn wanted(phase: GameflowPhase, clash: bool) -> bool {
    phase == GameflowPhase::ChampSelect || (clash && !in_game(phase))
}

/// What auto mode wants done after a phase or Clash state change.
/// `recording_by_auto` is `Some(true)` for an auto recording in progress,
/// `Some(false)` for a manual one, `None` when nothing is recording.
pub fn decide(
    enabled: bool,
    phase: GameflowPhase,
    clash: bool,
    recording_by_auto: Option<bool>,
) -> Option<AutoAction> {
    let want = wanted(phase, clash);
    match recording_by_auto {
        None if enabled && want => Some(AutoAction::Start),
        Some(true) if !want => Some(AutoAction::Stop),
        _ => None,
    }
}

#[cfg(test)]
#[path = "../tests/unit/auto.rs"]
mod tests;
