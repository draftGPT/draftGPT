use super::*;

/// `OnJsonApiEvent_lol-champ-select_v1_session` → `/lol-champ-select/v1/session`.
/// Returns `None` for names that aren't per-resource JSON API events.
fn event_to_uri(event: &str) -> Option<String> {
    let rest = event.strip_prefix("OnJsonApiEvent_")?;
    if rest.is_empty() {
        return None;
    }
    Some(format!("/{}", rest.replace('_', "/")))
}

#[test]
fn subscribe_format() {
    assert_eq!(
        subscribe_message("OnJsonApiEvent_lol-champ-select_v1_session"),
        r#"[5,"OnJsonApiEvent_lol-champ-select_v1_session"]"#
    );
}

#[test]
fn core_resources_are_consistent() {
    // Each resource's event delivers its URI: the event maps to the URI
    // itself or to a prefix of it.
    for r in crate::CORE_RESOURCES {
        let prefix = event_to_uri(r.event).unwrap();
        assert!(r.uri.starts_with(&prefix), "{r:?}");
    }
}

#[test]
fn event_names_map_to_uris() {
    assert_eq!(
        event_to_uri("OnJsonApiEvent_lol-champ-select_v1_session").as_deref(),
        Some("/lol-champ-select/v1/session")
    );
    assert_eq!(
        event_to_uri("OnJsonApiEvent_lol-gameflow_v1_gameflow-phase").as_deref(),
        Some("/lol-gameflow/v1/gameflow-phase")
    );
    assert_eq!(event_to_uri("OnJsonApiEvent"), None);
    assert_eq!(event_to_uri("OnJsonApiEvent_"), None);
    assert_eq!(event_to_uri("OnLog"), None);
}

#[test]
fn parses_event_frame() {
    let frame = r#"[8,"OnJsonApiEvent_lol-gameflow_v1_gameflow-phase",{"data":"ChampSelect","eventType":"Update","uri":"/lol-gameflow/v1/gameflow-phase"}]"#;
    let ev = parse_frame(frame).unwrap().unwrap();
    assert_eq!(ev.uri, "/lol-gameflow/v1/gameflow-phase");
    assert_eq!(ev.kind, EventKind::Update);
    assert_eq!(ev.data, Value::String("ChampSelect".into()));
}

#[test]
fn delete_frame_without_data() {
    let frame = r#"[8,"OnJsonApiEvent_lol-champ-select_v1_session",{"eventType":"Delete","uri":"/lol-champ-select/v1/session"}]"#;
    let ev = parse_frame(frame).unwrap().unwrap();
    assert_eq!(ev.kind, EventKind::Delete);
    assert_eq!(ev.data, Value::Null);
}

#[test]
fn ignores_non_events() {
    assert_eq!(parse_frame(""), Ok(None));
    assert_eq!(parse_frame(r#"[0,"session-id",1,"server"]"#), Ok(None));
}

#[test]
fn rejects_garbage() {
    assert!(parse_frame("{not json").is_err());
    assert!(parse_frame(r#"{"a":1}"#).is_err());
    assert!(parse_frame(r#"[8,"x",{"eventType":"Update"}]"#).is_err());
    assert!(parse_frame(r#"[8,"x",{"uri":"/a","eventType":"Weird"}]"#).is_err());
}
