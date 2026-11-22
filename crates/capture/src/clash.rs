//! Clash data for recordings. The tournament state has events of its own (see
//! `lcu::CLASH_RESOURCES`), but brackets, rosters and scouting are served under
//! URIs with IDs and arrive by event rarely if ever. While a Clash recording
//! runs, [`ClashPoller`] fetches the tournament state and bracket every
//! [`POLL`] (GET only) and feeds the results to the controller as `Snapshot`
//! events; only data that changed since the last fetch is recorded.
//!
//! The chain: `tournament-summary` gives each tournament's `rosterId` (our
//! team) and `bracketId`; `bracket/{id}` lists the bracket's rosters and
//! matches. Once the bracket names our next opponent, the scouting snapshot is
//! taken once (see [`scout`]): both rosters (members come as summoner IDs), the
//! summoner lookup for the opponents' puuids, `scouting/champions` and
//! `ranked-stats/{puuid}`, what the client's scouting tab shows.
//!
//! All of it is redacted by the recorder like everything else (team names,
//! roster and bracket IDs, summoner IDs, puuids; scouting numbers rounded; see
//! `redact`).

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use lcu::{
    CLASH_EVENTS, CLASH_PLAYER, CLASH_RESOURCES, CLASH_TOURNAMENT_STATE_INFO,
    CLASH_TOURNAMENT_SUMMARY, CORE_RESOURCES, EventKind, LcuHttp, Live, RawEvent,
};
use serde_json::Value;
use tokio::sync::mpsc;

/// How often the tournament state and bracket are fetched during a Clash
/// recording.
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

/// How often a scouting snapshot is tried for the same opponent before giving
/// up (a roster or lookup that fails keeps failing).
pub const SCOUT_TRIES: u8 = 3;

/// One round of fetches.
#[derive(Debug, Default)]
pub struct Fetched {
    pub events: Vec<RawEvent>,
    /// Scouting tries per opponent roster ID; [`SCOUT_TRIES`] once done.
    pub scouted: BTreeMap<String, u8>,
    pub failed: usize,
    /// The [`ClashPoller`] generation this fetch started in.
    pub generation: u64,
}

/// Fetches the Clash state and bracket every time; the scouting snapshot
/// (both rosters and what the client's scouting tab shows) once per opponent,
/// tried again on the next fetches while it's incomplete.
pub async fn fetch(http: &LcuHttp, scouted: &BTreeMap<String, u8>) -> Fetched {
    let mut f = Fetched {
        scouted: scouted.clone(),
        ..Fetched::default()
    };
    let summary = get(http, CLASH_TOURNAMENT_SUMMARY.uri, &mut f).await;
    get(http, CLASH_TOURNAMENT_STATE_INFO.uri, &mut f).await;
    get(http, CLASH_PLAYER.uri, &mut f).await;

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
        let (Some(ours), Some(bid)) = (t.roster_id, t.bracket_id) else {
            continue;
        };
        let Some(bracket) = get(http, &format!("/lol-clash/v1/bracket/{bid}"), &mut f).await else {
            continue;
        };
        let Some(opp) = opponent(&bracket, &ours) else {
            tracing::debug!("Clash scouting: no open match for our roster in the bracket");
            continue;
        };
        let tries = f.scouted.get(&opp).copied().unwrap_or(0);
        if tries >= SCOUT_TRIES {
            continue;
        }
        tracing::info!(
            "Clash scouting: next opponent known, taking the snapshot (try {})",
            tries + 1
        );
        let done = scout(http, &ours, &opp, &mut f).await;
        if !done && tries + 1 == SCOUT_TRIES {
            tracing::warn!("Clash scouting: giving up after {SCOUT_TRIES} incomplete tries");
        }
        f.scouted
            .insert(opp, if done { SCOUT_TRIES } else { tries + 1 });
    }
    f
}

/// The scouting snapshot: both rosters, then for the opponent's players
/// (puuids from the summoner lookup; opponents are shown by name in Clash)
/// what the client's scouting tab shows: top season champions and mastery,
/// ranked stats. True when everything came back.
///
/// Each lookup is recorded reduced to `{summonerId, puuid}` under its URI, so
/// a ranked-stats entry (by puuid) can be matched to a roster member (by
/// summoner ID) after redaction; the rest of the summoner data isn't kept.
async fn scout(http: &LcuHttp, ours: &str, opp: &str, f: &mut Fetched) -> bool {
    let own = get(http, &format!("/lol-clash/v1/roster/{}", encode(ours)), f).await;
    let Some(roster) = get(http, &format!("/lol-clash/v1/roster/{}", encode(opp)), f).await else {
        tracing::info!(
            "Clash scouting: opponent roster empty or not found (ours found: {})",
            own.is_some()
        );
        return false;
    };
    let ids = member_ids(&roster);
    tracing::info!("Clash scouting: opponent roster has {} players", ids.len());
    if ids.is_empty() {
        return false;
    }
    // Not fetched: `scouting/matchhistory` answers 403 "RBAC: access denied"
    // for any summonerIds, even our own (16.23); the client's History tab is
    // empty too.
    let mut puuids = Vec::new();
    for id in &ids {
        let uri = format!("/lol-summoner/v1/summoners/{id}");
        let summoner = get_raw(http, &uri, f).await;
        if let Some(p) = summoner
            .as_ref()
            .and_then(|s| s.get("puuid"))
            .and_then(Value::as_str)
            .filter(|p| !p.is_empty())
        {
            push(f, &uri, serde_json::json!({"summonerId": id, "puuid": p}));
            puuids.push(p.to_owned());
        }
    }
    tracing::info!(
        "Clash scouting: {} of {} puuids from the summoner lookup",
        puuids.len(),
        ids.len()
    );
    if puuids.is_empty() {
        return false;
    }
    let quoted: Vec<String> = puuids.iter().map(|p| format!("\"{p}\"")).collect();
    let query = encode(&format!("[{}]", quoted.join(",")));
    let uri = format!("/lol-clash/v1/scouting/champions?puuids={query}");
    let mut done = puuids.len() == ids.len();
    match get_raw(http, &uri, f).await {
        Some(data) => push(f, "/lol-clash/v1/scouting/champions", data),
        None => done = false,
    }
    for p in &puuids {
        let ranked = get(
            http,
            &format!("/lol-ranked/v1/ranked-stats/{}", encode(p)),
            f,
        )
        .await;
        done &= ranked.is_some();
    }
    done
}

/// The roster we play next: the other side of our unfinished match in the
/// latest round (a bye or a finished bracket gives none). Slots of rounds not
/// decided yet hold placeholder IDs that aren't in the bracket's `rosters`;
/// those don't count.
fn opponent(bracket: &Value, ours: &str) -> Option<String> {
    let teams: Vec<String> = bracket
        .get("rosters")?
        .as_array()?
        .iter()
        .filter_map(|r| r.get("rosterId").and_then(id_string))
        .collect();
    bracket
        .get("matches")?
        .as_array()?
        .iter()
        .filter(|m| m.get("winnerId").and_then(Value::as_i64).unwrap_or(0) == 0)
        .filter_map(|m| {
            let a = m.get("rosterId1").and_then(id_string)?;
            let b = m.get("rosterId2").and_then(id_string)?;
            let round = m.get("round").and_then(Value::as_i64).unwrap_or(0);
            if a == ours {
                Some((round, b))
            } else if b == ours {
                Some((round, a))
            } else {
                None
            }
        })
        .max_by_key(|(round, _)| *round)
        .map(|(_, opp)| opp)
        .filter(|opp| teams.contains(opp))
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
/// without the URI's IDs and query (roster IDs identify a team, puuids a
/// player) and without the URL the HTTP error carries.
async fn get_raw(http: &LcuHttp, uri: &str, f: &mut Fetched) -> Option<Value> {
    match http.get_json(uri).await {
        Ok(Some(data)) if !data.is_null() => Some(data),
        Ok(_) => None,
        Err(e) => {
            f.failed += 1;
            tracing::warn!(
                "Clash fetch {} failed: {}",
                log_uri(uri),
                without_url(&e.to_string())
            );
            None
        }
    }
}

/// A URI for the log: the query dropped, ID segments as `{id}` (version
/// segments like `v1` stay).
fn log_uri(uri: &str) -> String {
    let path = uri.split('?').next().unwrap_or(uri);
    path.split('/')
        .map(|s| {
            let version = s
                .strip_prefix('v')
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
            if s.bytes().any(|b| b.is_ascii_digit()) && !version {
                "{id}"
            } else {
                s
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// reqwest errors end in ` for url (<full url>)`, IDs and query included.
fn without_url(error: &str) -> &str {
    error.split(" for url (").next().unwrap_or(error)
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
    scouted: BTreeMap<String, u8>,
    /// Bumped by [`reset`](Self::reset): a fetch from before it is dropped.
    generation: u64,
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
            scouted: BTreeMap::new(),
            generation: 0,
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
        let generation = self.generation;
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut fetched = fetch(&http, &scouted).await;
            fetched.generation = generation;
            let _ = tx.send(fetched).await;
        });
    }

    /// Takes a finished fetch; returns the events whose data changed, for the
    /// controller. `latest`: the controller's current data for a URI. A fetch
    /// started before the last [`reset`](Self::reset) (another connection) is
    /// dropped whole. `recording`: false when the recording stopped while the
    /// fetch ran; its scouting isn't counted then, so the next recording takes
    /// the snapshot again.
    pub fn done<'a>(
        &mut self,
        fetched: Fetched,
        recording: bool,
        latest: impl Fn(&str) -> Option<&'a Value>,
    ) -> Vec<RawEvent> {
        self.in_flight = false;
        if fetched.generation != self.generation {
            return Vec::new();
        }
        self.next = Instant::now() + POLL;
        if recording {
            self.scouted = fetched.scouted;
        }
        fetched
            .events
            .into_iter()
            .filter(|ev| latest(&ev.uri) != Some(&ev.data))
            .collect()
    }

    /// A new client connection: everything is fetched again.
    pub fn reset(&mut self) {
        self.scouted.clear();
        self.generation += 1;
        self.next = Instant::now();
    }
}

/// Scouting snapshot data (scouting tab, ranked stats, summoner links): fetched
/// once per opponent, not state, so it's recorded but never carried into the
/// next recording's snapshots.
pub fn is_scouting_snapshot(uri: &str) -> bool {
    crate::redact::is_scouting_uri(uri) || uri.starts_with("/lol-summoner/")
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
}
