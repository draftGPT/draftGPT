use super::*;

#[test]
fn roundtrips_as_json() {
    let ev = RawEvent {
        t: 42,
        uri: "/lol-gameflow/v1/gameflow-phase".into(),
        kind: EventKind::Update,
        data: Value::String("ChampSelect".into()),
    };
    let s = serde_json::to_string(&ev).unwrap();
    assert_eq!(
        s,
        r#"{"t":42,"uri":"/lol-gameflow/v1/gameflow-phase","kind":"Update","data":"ChampSelect"}"#
    );
    assert_eq!(serde_json::from_str::<RawEvent>(&s).unwrap(), ev);
}
