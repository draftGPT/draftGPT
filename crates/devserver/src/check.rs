//! Parses events with the typed structs and reports problems, so the dev page
//! shows when the client sends something our types don't expect. Typed parsing
//! is lenient, so besides hard failures this also flags enum values we don't
//! know yet (they parse as `Unknown`).

use lcu::types::{
    ActionType, ChampSelectSession, DodgeState, GameflowPhase, Matchmaking, Position,
    SubsetChampionList, TimerPhase,
};
use lcu::{
    CLASH_TOURNAMENT_STATE_INFO, CLASH_TOURNAMENT_SUMMARY, EventKind, GAMEFLOW_PHASE_URI,
    MATCHMAKING_URI, RawEvent, SUBSET_CHAMPION_LIST_URI, is_session_uri,
};
use serde_json::Value;

/// `None` if the event is fine (or not one we type), otherwise a description.
pub fn check(ev: &RawEvent) -> Option<String> {
    if ev.kind == EventKind::Delete {
        return None;
    }
    let problems = match ev.uri.as_str() {
        uri if is_session_uri(uri) => check_session(&ev.data),
        GAMEFLOW_PHASE_URI => check_phase(&ev.data),
        SUBSET_CHAMPION_LIST_URI => check_subset(&ev.data),
        MATCHMAKING_URI => check_matchmaking(&ev.data),
        uri if uri == CLASH_TOURNAMENT_SUMMARY.uri || uri == CLASH_TOURNAMENT_STATE_INFO.uri => {
            check_clash_states(&ev.data)
        }
        _ => return None,
    };
    (!problems.is_empty()).then(|| problems.join("; "))
}

/// Clash tournament states auto mode doesn't know (it records during lock-in,
/// scouting and in game; a new state might need adding there).
fn check_clash_states(data: &Value) -> Vec<String> {
    capture::auto::clash_states(data)
        .into_iter()
        .filter(|s| !capture::auto::CLASH_KNOWN_STATES.contains(s))
        .map(|s| format!("unknown Clash tournament state {s:?}"))
        .collect()
}

fn check_subset(data: &Value) -> Vec<String> {
    match lcu::types::parse::<SubsetChampionList>(data) {
        Ok(_) => Vec::new(),
        Err(e) => vec![format!("subset champion list: {e}")],
    }
}

fn check_matchmaking(data: &Value) -> Vec<String> {
    // Outside a queue the resource can be empty, which the client sends as null.
    if data.is_null() {
        return Vec::new();
    }
    match lcu::types::parse::<Matchmaking>(data) {
        Ok(m) if m.dodge_data.state == DodgeState::Unknown => vec![format!(
            "unknown value at dodgeData.state: {}",
            data.pointer("/dodgeData/state").unwrap_or(&Value::Null)
        )],
        Ok(_) => Vec::new(),
        Err(e) => vec![format!("matchmaking: {e}")],
    }
}

fn check_phase(data: &Value) -> Vec<String> {
    match lcu::types::parse::<GameflowPhase>(data) {
        Ok(GameflowPhase::Unknown) => vec![format!("unknown gameflow phase {data}")],
        Ok(_) => Vec::new(),
        Err(e) => vec![format!("gameflow phase: {e}")],
    }
}

fn check_session(data: &Value) -> Vec<String> {
    let session: ChampSelectSession = match lcu::types::parse(data) {
        Ok(s) => s,
        Err(e) => return vec![format!("session parse failed: {e}")],
    };
    let mut problems = Vec::new();
    let mut unknown = |path: String, raw: Option<&Value>| {
        problems.push(format!(
            "unknown value at {path}: {}",
            raw.unwrap_or(&Value::Null)
        ));
    };

    if session.timer.phase == TimerPhase::Unknown {
        unknown("timer.phase".into(), data.pointer("/timer/phase"));
    }
    for (g, group) in session.actions.iter().enumerate() {
        for (i, action) in group.iter().enumerate() {
            if action.kind == ActionType::Unknown {
                unknown(
                    format!("actions[{g}][{i}].type"),
                    data.pointer(&format!("/actions/{g}/{i}/type")),
                );
            }
        }
    }
    for (team, players) in [
        ("myTeam", &session.my_team),
        ("theirTeam", &session.their_team),
    ] {
        for (i, p) in players.iter().enumerate() {
            if p.assigned_position == Position::Unknown {
                unknown(
                    format!("{team}[{i}].assignedPosition"),
                    data.pointer(&format!("/{team}/{i}/assignedPosition")),
                );
            }
        }
    }
    problems
}

#[cfg(test)]
#[path = "../tests/unit/check.rs"]
mod tests;
