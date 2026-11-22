use super::*;

#[test]
fn relevance_filter() {
    assert!(is_relevant("GetLolChampSelectV1Session"));
    assert!(is_relevant("OnJsonApiEvent_lol-champ-select_v1_session"));
    assert!(is_relevant("LolGameflowGameflowPhase"));
    assert!(!is_relevant("GetLolChatV1Me"));
}

#[test]
fn index_names_accepts_object_or_list() {
    let idx = json!({"types": {"A": "", "B": ""}, "events": ["E1"]});
    let mut t = index_names(&idx, "types");
    t.sort();
    assert_eq!(t, vec!["A", "B"]);
    assert_eq!(index_names(&idx, "events"), vec!["E1"]);
    assert!(index_names(&idx, "functions").is_empty());
}
