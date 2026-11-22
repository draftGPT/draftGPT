use super::*;
use serde_json::json;

#[test]
fn summary_entries() {
    let s = json!([
        {"state": "SCOUTING", "tournamentId": 4321, "rosterId": "778899", "bracketId": 55},
        {"state": "UPCOMING", "tournamentId": 4400, "rosterId": "", "bracketId": 0}
    ]);
    assert_eq!(
        tournaments(&s),
        vec![
            Tournament {
                tournament_id: Some(4321),
                roster_id: Some("778899".into()),
                bracket_id: Some(55)
            },
            Tournament {
                tournament_id: Some(4400),
                roster_id: None,
                bracket_id: None
            },
        ]
    );
    assert!(tournaments(&json!(null)).is_empty());
}

#[test]
fn bracket_opponent_and_roster_ids() {
    // We're roster 5 (a string in the summary, a number in the bracket):
    // round 1 is won, round 2 is open against 9.
    let b = json!({
        "rosters": [{"rosterId": 1}, {"rosterId": 2}, {"rosterId": 3}, {"rosterId": 5}, {"rosterId": 9}],
        "matches": [
            {"round": 1, "rosterId1": 5, "rosterId2": 3, "winnerId": 5},
            {"round": 1, "rosterId1": 1, "rosterId2": 2, "winnerId": 0},
            {"round": 2, "rosterId1": 9, "rosterId2": 5, "winnerId": 0}
        ]
    });
    assert_eq!(opponent(&b, "5"), Some("9".into()));
    assert_eq!(opponent(&b, "3"), None);
    // Waiting for the other match to finish: no opponent yet.
    let waiting = json!({"rosters": [{"rosterId": 5}], "matches": [{"round": 2, "rosterId1": 5, "rosterId2": 0, "winnerId": 0}]});
    assert_eq!(opponent(&waiting, "5"), None);
    // A placeholder for a team not decided yet (seen live in 16.23).
    let placeholder = json!({"rosters": [{"rosterId": 5}], "matches": [{"round": 3, "rosterId1": 5, "rosterId2": 51, "winnerId": 0}]});
    assert_eq!(opponent(&placeholder, "5"), None);
    let r = json!({"members": [{"summonerId": 11}, {"summonerId": 0}, {"position": "TOP"}]});
    assert_eq!(member_ids(&r), vec![11]);
}

#[test]
fn encoding() {
    assert_eq!(encode("[1,2]"), "%5B1%2C2%5D");
    assert_eq!(encode("abc-1_2"), "abc-1_2");
    assert_eq!(encode("a/b"), "a%2Fb");
}

#[test]
fn done_keeps_only_changes_and_schedules_the_next_fetch() {
    let mut p = ClashPoller::new();
    p.in_flight = true;
    let same = json!([{"state": "SCOUTING"}]);
    let ev = |uri: &str, data: Value| RawEvent {
        t: 1,
        uri: uri.into(),
        kind: EventKind::Snapshot,
        data,
    };
    let fetched = Fetched {
        events: vec![ev("/a", same.clone()), ev("/b", json!(2))],
        scouted: [("7".to_string(), SCOUT_TRIES)].into(),
        failed: 0,
        generation: 0,
    };
    let known = same.clone();
    let out = p.done(fetched, true, |uri| (uri == "/a").then_some(&known));
    assert_eq!(
        out.iter().map(|e| e.uri.as_str()).collect::<Vec<_>>(),
        vec!["/b"]
    );
    assert!(!p.in_flight);
    assert!(p.next > Instant::now());
    assert_eq!(p.scouted, [("7".to_string(), SCOUT_TRIES)].into());
}

#[test]
fn logs_carry_no_ids_or_urls() {
    assert_eq!(
        log_uri("/lol-ranked/v1/ranked-stats/0a1b2c3d-1111-2222-3333-444455556666"),
        "/lol-ranked/v1/ranked-stats/{id}"
    );
    assert_eq!(
        log_uri("/lol-clash/v1/scouting/champions?puuids=%5B%22x1%22%5D"),
        "/lol-clash/v1/scouting/champions"
    );
    assert_eq!(
        without_url(
            "HTTP status server error (500 Internal Server Error) for url (https://127.0.0.1:1/lol-summoner/v1/summoners/123)"
        ),
        "HTTP status server error (500 Internal Server Error)"
    );
}

#[test]
fn a_fetch_from_before_a_reconnect_is_dropped() {
    let mut p = ClashPoller::new();
    p.in_flight = true;
    p.reset();
    let stale = Fetched {
        events: vec![RawEvent {
            t: 1,
            uri: "/a".into(),
            kind: EventKind::Snapshot,
            data: json!(1),
        }],
        scouted: [("7".to_string(), SCOUT_TRIES)].into(),
        failed: 0,
        generation: 0,
    };
    assert!(p.done(stale, true, |_| None).is_empty());
    assert!(p.scouted.is_empty());
    assert!(!p.in_flight);
}

#[test]
fn capture_source_records_clash() {
    let live = live_source(true);
    assert!((live.also_forward)("/lol-clash/v1/bracket/5"));
    assert!(!(live.also_forward)("/lol-clash/v1/player/chat-rosters"));
    assert!(
        live.resources
            .iter()
            .any(|r| r.uri == CLASH_TOURNAMENT_SUMMARY.uri)
    );
}
