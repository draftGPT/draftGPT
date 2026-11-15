//! Auto mode: record each champ select. Starts when the gameflow phase becomes
//! `ChampSelect` and stops when it leaves. Recordings started by hand are never
//! stopped by auto mode.

use lcu::types::GameflowPhase;
use lcu::{EventKind, GAMEFLOW_PHASE_URI, RawEvent};

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

/// What auto mode wants done after a phase change.
/// `recording_by_auto` is `Some(true)` for an auto recording in progress,
/// `Some(false)` for a manual one, `None` when nothing is recording.
pub fn decide(
    enabled: bool,
    phase: GameflowPhase,
    recording_by_auto: Option<bool>,
) -> Option<AutoAction> {
    let in_champ_select = phase == GameflowPhase::ChampSelect;
    match recording_by_auto {
        None if enabled && in_champ_select => Some(AutoAction::Start),
        Some(true) if !in_champ_select => Some(AutoAction::Stop),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use GameflowPhase as P;
    use serde_json::json;

    #[test]
    fn starts_only_when_enabled_and_idle() {
        assert_eq!(decide(true, P::ChampSelect, None), Some(AutoAction::Start));
        assert_eq!(decide(false, P::ChampSelect, None), None);
        assert_eq!(decide(true, P::ChampSelect, Some(false)), None);
        assert_eq!(decide(true, P::ChampSelect, Some(true)), None);
        assert_eq!(decide(true, P::Lobby, None), None);
    }

    #[test]
    fn stops_only_its_own_recordings() {
        assert_eq!(
            decide(true, P::GameStart, Some(true)),
            Some(AutoAction::Stop)
        );
        assert_eq!(decide(true, P::Lobby, Some(true)), Some(AutoAction::Stop));
        // Auto turned off mid-recording: still finishes its own recording.
        assert_eq!(decide(false, P::Lobby, Some(true)), Some(AutoAction::Stop));
        assert_eq!(decide(true, P::Lobby, Some(false)), None);
    }

    #[test]
    fn phase_extraction() {
        let ev = |kind, data| RawEvent {
            t: 0,
            uri: GAMEFLOW_PHASE_URI.into(),
            kind,
            data,
        };
        assert_eq!(
            phase_of(&ev(EventKind::Update, json!("ChampSelect"))),
            Some(P::ChampSelect)
        );
        assert_eq!(phase_of(&ev(EventKind::Delete, json!(null))), Some(P::None));
        assert_eq!(
            phase_of(&ev(EventKind::Update, json!(42))),
            Some(P::Unknown)
        );
        let other = RawEvent {
            uri: lcu::SESSION_URI.into(),
            ..ev(EventKind::Update, json!({}))
        };
        assert_eq!(phase_of(&other), None);
    }
}
