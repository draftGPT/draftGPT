//! Clash data for recordings. The tournament state has events of its own (see
//! `lcu::CLASH_RESOURCES`), but brackets, rosters and scouting are served under
//! URIs with IDs and arrive by event rarely if ever. While a Clash recording
//! runs, [`ClashPoller`] fetches them every [`POLL`] (GET only) and feeds the
//! results to the controller as `Snapshot` events, so replays later have the
//! opponent team, the whole bracket and how it changed. Only data that changed
//! since the last fetch is recorded.
//!
//! The chain: `tournament-summary` gives each tournament's `rosterId` (our
//! team) and `bracketId`; `bracket/{id}` lists the bracket's rosters and
//! matches; `roster/{id}` (+ `/stats`) gives each team's members and
//! positions. Players come as summoner IDs; scouting by puuid
//! (`scouting/champions`) is left for later, `scouting/matchhistory` takes
//! summoner IDs and is fetched once per set of opponents.
//!
//! All of it is redacted by the recorder like everything else (team names,
//! roster IDs, summoner IDs; see `redact`).

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use lcu::{
    CLASH_EVENTS, CLASH_PLAYER, CLASH_RESOURCES, CLASH_TOURNAMENT_STATE_INFO,
    CLASH_TOURNAMENT_SUMMARY, CORE_RESOURCES, EventKind, LcuHttp, Live, RawEvent,
};
use serde_json::Value;
use tokio::sync::mpsc;

/// How often the bracket and rosters are fetched during a Clash recording.
pub const POLL: Duration = Duration::from_secs(10);

/// The live source for capture: the core resources plus Clash.
pub fn live_source(wire: bool) -> Live {
    let mut resources = CORE_RESOURCES.to_vec();
    resources.extend_from_slice(CLASH_RESOURCES);
    Live {
        resources,
        also_forward: lcu::is_recorded_clash_uri,
        also_subscribe: CLASH_EVENTS,
        wire,
        ..Live::default()
    }
}

/// One round of fetches.
#[derive(Debug, Default)]
pub struct Fetched {
    pub events: Vec<RawEvent>,
    /// Opponent summoner IDs whose match history was fetched (or tried).
    pub scouted: BTreeSet<u64>,
    pub failed: usize,
}

/// Fetches the Clash state, bracket and rosters. `scouted`: opponents whose
/// match history is already recorded (not fetched again).
pub async fn fetch(http: &LcuHttp, scouted: &BTreeSet<u64>) -> Fetched {
    let mut f = Fetched {
        scouted: scouted.clone(),
        ..Fetched::default()
    };
    let summary = get(http, CLASH_TOURNAMENT_SUMMARY.uri, &mut f).await;
    get(http, CLASH_TOURNAMENT_STATE_INFO.uri, &mut f).await;
    get(http, CLASH_PLAYER.uri, &mut f).await;
    get(http, "/lol-clash/v1/thirdparty/team-data", &mut f).await;

    let mut ours = BTreeSet::new();
    let mut rosters = BTreeSet::new();
    for t in summary.as_ref().map(tournaments).unwrap_or_default() {
        if let Some(tid) = t.tournament_id {
            get(http, &format!("/lol-clash/v1/tournament/{tid}"), &mut f).await;
            get(
                http,
                &format!("/lol-clash/v1/tournament/{tid}/player"),
                &mut f,
            )
            .await;
        }
        if let Some(rid) = t.roster_id {
            ours.insert(rid.clone());
            rosters.insert(rid);
        }
        if let Some(bid) = t.bracket_id
            && let Some(bracket) = get(http, &format!("/lol-clash/v1/bracket/{bid}"), &mut f).await
        {
            rosters.extend(bracket_rosters(&bracket));
        }
    }

    let mut opponents = BTreeSet::new();
    for rid in &rosters {
        let uri = format!("/lol-clash/v1/roster/{}", encode(rid));
        if let Some(roster) = get(http, &uri, &mut f).await
            && !ours.contains(rid)
        {
            opponents.extend(member_ids(&roster));
        }
        get(http, &format!("{uri}/stats"), &mut f).await;
    }

    // Match history of the opponents, once per new set of players.
    if !opponents.is_empty() && !opponents.is_subset(&f.scouted) {
        let ids: Vec<String> = opponents.iter().map(u64::to_string).collect();
        let query = encode(&format!("[{}]", ids.join(",")));
        let uri = format!("/lol-clash/v1/scouting/matchhistory?summonerIds={query}");
        if let Some(data) = get_raw(http, &uri, &mut f).await {
            // Recorded without the query: the IDs are in the data anyway.
            push(&mut f, "/lol-clash/v1/scouting/matchhistory", data);
        }
        f.scouted.extend(opponents);
    }
    f
}

/// A tournament entry of the summary.
#[derive(Debug, Default, PartialEq)]
struct Tournament {
    tournament_id: Option<i64>,
    roster_id: Option<String>,
    bracket_id: Option<i64>,
}

fn tournaments(summary: &Value) -> Vec<Tournament> {
    let items: Vec<&Value> = match summary {
        Value::Array(items) => items.iter().collect(),
        Value::Object(_) => vec![summary],
        _ => Vec::new(),
    };
    items
        .into_iter()
        .map(|t| Tournament {
            tournament_id: t
                .get("tournamentId")
                .and_then(Value::as_i64)
                .filter(|&x| x > 0),
            roster_id: t.get("rosterId").and_then(id_string),
            bracket_id: t
                .get("bracketId")
                .and_then(Value::as_i64)
                .filter(|&x| x > 0),
        })
        .collect()
}

/// Roster IDs come as strings (summary, roster) or numbers (bracket).
fn id_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() && s != "0" => Some(s.clone()),
        Value::Number(n) if n.as_i64().is_some_and(|x| x > 0) => Some(n.to_string()),
        _ => None,
    }
}

/// Every roster in a bracket: its roster list and both sides of each match.
fn bracket_rosters(bracket: &Value) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let list = |key: &str| {
        bracket
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    for r in list("rosters") {
        ids.extend(r.get("rosterId").and_then(id_string));
    }
    for m in list("matches") {
        for key in ["rosterId1", "rosterId2"] {
            ids.extend(m.get(key).and_then(id_string));
        }
    }
    ids
}

fn member_ids(roster: &Value) -> Vec<u64> {
    roster
        .get("members")
        .and_then(Value::as_array)
        .map(|ms| {
            ms.iter()
                .filter_map(|m| m.get("summonerId").and_then(Value::as_u64))
                .filter(|&id| id > 0)
                .collect()
        })
        .unwrap_or_default()
}

/// Percent-encodes everything but unreserved characters (for path segments
/// and query values).
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

async fn get(http: &LcuHttp, uri: &str, f: &mut Fetched) -> Option<Value> {
    let data = get_raw(http, uri, f).await?;
    push(f, uri, data.clone());
    Some(data)
}

/// A GET; `None` for not found (404, empty) and errors. Errors are logged
/// without the URI's IDs (roster IDs identify a team).
async fn get_raw(http: &LcuHttp, uri: &str, f: &mut Fetched) -> Option<Value> {
    match http.get_json(uri).await {
        Ok(Some(data)) if !data.is_null() => Some(data),
        Ok(_) => None,
        Err(e) => {
            f.failed += 1;
            let what = uri.split('?').next().unwrap_or(uri);
            let what: String = what
                .split('/')
                .map(|s| {
                    if s.bytes().any(|b| b.is_ascii_digit()) {
                        "{id}"
                    } else {
                        s
                    }
                })
                .collect::<Vec<_>>()
                .join("/");
            tracing::warn!("Clash fetch {what} failed: {e}");
            None
        }
    }
}

fn push(f: &mut Fetched, uri: &str, data: Value) {
    f.events.push(RawEvent {
        t: now_ms(),
        uri: uri.to_string(),
        kind: EventKind::Snapshot,
        data,
    });
}

/// Same clock as `lcu::Live` (milliseconds since the epoch).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Runs the fetches on a schedule: every [`POLL`] while a Clash recording is
/// on, right away after [`poke`](Self::poke). One fetch at a time; results come
/// back through the channel given to [`maybe_start`](Self::maybe_start).
#[derive(Debug)]
pub struct ClashPoller {
    in_flight: bool,
    next: Instant,
    scouted: BTreeSet<u64>,
}

impl Default for ClashPoller {
    fn default() -> Self {
        Self::new()
    }
}

impl ClashPoller {
    pub fn new() -> Self {
        Self {
            in_flight: false,
            next: Instant::now(),
            scouted: BTreeSet::new(),
        }
    }

    /// Fetch as soon as possible (a recording started, the Clash state changed).
    pub fn poke(&mut self) {
        self.next = Instant::now();
    }

    /// Starts a fetch if one is due and `wanted` (recording, Clash active).
    pub fn maybe_start(&mut self, wanted: bool, http: Option<LcuHttp>, tx: &mpsc::Sender<Fetched>) {
        if !wanted || self.in_flight || Instant::now() < self.next {
            return;
        }
        let Some(http) = http else { return };
        self.in_flight = true;
        let scouted = self.scouted.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(fetch(&http, &scouted).await).await;
        });
    }

    /// Takes a finished fetch; returns the events whose data changed, for the
    /// controller. `latest`: the controller's current data for a URI.
    pub fn done<'a>(
        &mut self,
        fetched: Fetched,
        latest: impl Fn(&str) -> Option<&'a Value>,
    ) -> Vec<RawEvent> {
        self.in_flight = false;
        self.next = Instant::now() + POLL;
        self.scouted = fetched.scouted;
        fetched
            .events
            .into_iter()
            .filter(|ev| latest(&ev.uri) != Some(&ev.data))
            .collect()
    }

    /// A new client connection: everything is fetched again.
    pub fn reset(&mut self) {
        self.scouted.clear();
        self.next = Instant::now();
    }
}

/// True for events that should trigger a fetch right away.
pub fn is_trigger(ev: &RawEvent) -> bool {
    ev.uri == CLASH_TOURNAMENT_SUMMARY.uri || ev.uri == CLASH_TOURNAMENT_STATE_INFO.uri
}

#[cfg(test)]
mod tests {
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
    fn bracket_and_roster_ids() {
        let b = json!({
            "rosters": [{"rosterId": 1}, {"rosterId": 2}],
            "matches": [{"rosterId1": 1, "rosterId2": 3}, {"rosterId1": 2, "rosterId2": 0}]
        });
        assert_eq!(
            bracket_rosters(&b),
            ["1", "2", "3"].map(String::from).into()
        );
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
            scouted: [7].into(),
            failed: 0,
        };
        let known = same.clone();
        let out = p.done(fetched, |uri| (uri == "/a").then_some(&known));
        assert_eq!(
            out.iter().map(|e| e.uri.as_str()).collect::<Vec<_>>(),
            vec!["/b"]
        );
        assert!(!p.in_flight);
        assert!(p.next > Instant::now());
        assert_eq!(p.scouted, [7].into());
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
}
