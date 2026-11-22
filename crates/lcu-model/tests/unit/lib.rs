use super::*;

#[test]
fn recorded_clash_uris() {
    for uri in [
        "/lol-clash/v1/tournament-summary",
        "/lol-clash/v1/tournament/3456/player",
        "/lol-clash/v1/bracket/123",
        "/lol-clash/v1/roster/abc-def",
        "/lol-clash/v1/roster/abc-def/stats",
        "/lol-clash/v1/scouting/matchhistory",
        "/lol-clash/v1/thirdparty/team-data",
    ] {
        assert!(is_recorded_clash_uri(uri), "{uri}");
    }
    for uri in [
        "/lol-clash/v1/player/chat-rosters",
        "/lol-clash/v1/historyandwinners",
        "/lol-clash/v1/all-tournaments",
        "/lol-clash/v1/time",
        "/lol-clash/v1/tournament-summary-x",
        "/lol-clash/v1/bracket",
        SESSION_URI,
    ] {
        assert!(!is_recorded_clash_uri(uri), "{uri}");
    }
}
