//! Parses events with the typed structs and reports problems, so the dev page
//! shows when the client sends something our types don't expect. Typed parsing
//! is lenient, so besides hard failures this also flags enum values we don't
//! know yet (they parse as `Unknown`).

use lcu::types::{ActionType, ChampSelectSession, GameflowPhase, Position, TimerPhase};
use lcu::{EventKind, GAMEFLOW_PHASE_URI, RawEvent, is_session_uri};
use serde_json::Value;

/// `None` if the event is fine (or not one we type), otherwise a description.
pub fn check(ev: &RawEvent) -> Option<String> {
    if ev.kind == EventKind::Delete {
        return None;
    }
    let problems = match ev.uri.as_str() {
        uri if is_session_uri(uri) => check_session(&ev.data),
        GAMEFLOW_PHASE_URI => check_phase(&ev.data),
        _ => return None,
    };
    (!problems.is_empty()).then(|| problems.join("; "))
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
mod tests {
    use super::*;
    use lcu::SESSION_URI;
    use serde_json::json;

    fn ev(uri: &str, data: Value) -> RawEvent {
        RawEvent {
            t: 0,
            uri: uri.into(),
            kind: EventKind::Update,
            data,
        }
    }

    #[test]
    fn clean_session_has_no_problems() {
        let data = json!({"timer": {"phase": "BAN_PICK"}, "myTeam": [{"assignedPosition": "top"}],
                          "actions": [[{"type": "ban"}]]});
        assert_eq!(check(&ev(SESSION_URI, data)), None);
    }

    #[test]
    fn reports_unknown_values_with_raw_text() {
        let data = json!({"timer": {"phase": "NEW_PHASE"}, "theirTeam": [{"assignedPosition": "support"}],
                          "actions": [[{"type": "ban"}, {"type": "brand_new_action"}]]});
        let msg = check(&ev(SESSION_URI, data)).unwrap();
        assert!(msg.contains("timer.phase: \"NEW_PHASE\""), "{msg}");
        assert!(
            msg.contains("actions[0][1].type: \"brand_new_action\""),
            "{msg}"
        );
        assert!(
            msg.contains("theirTeam[0].assignedPosition: \"support\""),
            "{msg}"
        );
    }

    #[test]
    fn reports_type_errors() {
        let msg = check(&ev(SESSION_URI, json!({"myTeam": "oops"}))).unwrap();
        assert!(msg.contains("session parse failed"), "{msg}");
    }

    #[test]
    fn gameflow_phase() {
        assert_eq!(check(&ev(GAMEFLOW_PHASE_URI, json!("Lobby"))), None);
        assert!(check(&ev(GAMEFLOW_PHASE_URI, json!("Brand New"))).is_some());
    }

    #[test]
    fn ignores_other_uris_and_deletes() {
        assert_eq!(check(&ev("/other", json!(1))), None);
        let mut del = ev(SESSION_URI, json!(null));
        del.kind = EventKind::Delete;
        assert_eq!(check(&del), None);
    }
}
