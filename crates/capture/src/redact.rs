//! Replaces identifying values before anything reaches disk.
//!
//! Values are swapped for stable fakes per recording: the same original value
//! always maps to the same fake, so "this cell is the same player as that action"
//! still holds, across the main recording and its wire file. Empty values (`0`,
//! `""`, `null`) and small negative numbers (above [`NOT_SET_BELOW`]) are kept
//! as they are, because they carry meaning (bot, hidden name, not set;
//! `bracketId: -1` without a team).
//!
//! Four layers, from narrow to broad:
//! 1. Keys containing an identity or secret fragment ([`SENSITIVE_KEY_PARTS`]).
//! 2. String values that are chat IDs (`<puuid>@<region>.pvp.net`) or JWTs,
//!    whatever the key (a bare value has no key to match).
//! 3. Wire frames for chat-like resources (chat, friend hover cards, in-game
//!    buddies): every string is replaced (message text by its length), since
//!    friend names, notes and messages are free text.
//! 4. Wire frame URIs: path segments that hold an ID (also percent-encoded, or
//!    a summoner-sized number) are replaced.
//!
//! JSON nested in a string value (a friend's party `pty`) is parsed, redacted
//! the same way and put back.
//!
//! What this can't catch: a free-text field with a generic name (like `name`)
//! outside chat resources. That's why wire files never leave the machine.

use std::collections::HashMap;

use serde_json::Value;

/// Key fragments (matched anywhere in the key, case-insensitively) whose values
/// identify a player or a game, or grant access to something.
pub const SENSITIVE_KEY_PARTS: &[&str] = &[
    // Player identity, in all the spellings the client uses
    // (summonerId, fromSummonerId, obfuscatedSummonerId, riotIdGameName, ...).
    "puuid",
    "summonerid",
    "accountid",
    "playerid",
    "gamename",
    "tagline",
    "gametag",
    "riotid",
    "playeralias",
    "summonername",
    "displayname",
    "internalname",
    "obfuscated",
    // IDs under names that don't say "summoner" (dodges, declined ready checks).
    "declinerid",
    "dodgerid",
    "dodgeid",
    "participantid",
    // A game ID plus region finds the match and everyone in it; a lobby ID
    // (matchmaking, while queued) the party.
    "gameid",
    "lobbyid",
    // Custom lobby names default to "<owner's name>'s Game" (customLobbyName).
    "lobbyname",
    // Lets anyone spectate or connect to the game.
    "spectatorkey",
    "serverip",
    // Player names in a friend's party, and linked accounts.
    "partysummoners",
    "discord",
    // Clash: a roster (team) ID finds the team and its players through the
    // public API; covers rosterId1/2, forfeitRosterId, winnerRosterId.
    "rosterid",
    "winnerid",
    "inviterid",
    "invitationid",
    // A bracket ID finds all of its teams.
    "bracketid",
    // Chat rooms and credentials.
    "multiuserchatid",
    "chatroomname",
    "channelclaim",
    "password",
    "token",
    "jwt",
];

/// Negative numbers above this are "not set" markers and kept; anything lower
/// could be a real ID and is replaced.
pub const NOT_SET_BELOW: i64 = -1000;

/// `0` and small negative numbers mean "not set".
fn is_not_set(n: &serde_json::Number) -> bool {
    n.as_i64().is_some_and(|x| x <= 0 && x > NOT_SET_BELOW) || n.as_f64() == Some(0.0)
}

/// Keys sensitive only as a whole: as a fragment they'd also hit drafting
/// data (`pinDropSummoners` in champ select holds positions).
pub const SENSITIVE_EXACT_KEYS: &[&str] = &[
    // A party's summoner IDs (inside a friend's `pty`).
    "summoners",
];

pub fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    SENSITIVE_KEY_PARTS.iter().any(|p| lower.contains(p))
        || SENSITIVE_EXACT_KEYS.contains(&lower.as_str())
}

/// Keys replaced only in Clash resources, where they hold user-chosen team
/// names (elsewhere `name` is a champion or queue name).
const CLASH_TEXT_KEYS: &[&str] = &["name", "shortname", "rostername", "rostershortname"];

/// Path segments that are always an ID, whatever they look like: the one after
/// `roster` (Clash team IDs aren't always long or numeric), `bracket`,
/// `ranked-stats` (a puuid) and `summoners` (a summoner ID).
const ID_AFTER_SEGMENT: &[&str] = &["roster", "bracket", "ranked-stats", "summoners"];

/// Whether `seg`, after `previous`, is an ID ([`ID_AFTER_SEGMENT`]). Only
/// `/lol-summoner/` has summoner IDs after `summoners`; champ select's
/// `/lol-champ-select/v1/summoners/{cell}` has a cell index there, and
/// `/lol-summoner/v2/summoners/puuid/{puuid}` a word.
fn is_id_after(uri: &str, previous: &str, seg: &str) -> bool {
    !seg.is_empty()
        && ID_AFTER_SEGMENT.contains(&previous)
        && (previous != "summoners"
            || (uri.starts_with("/lol-summoner/") && seg.bytes().all(|b| b.is_ascii_digit())))
}

/// Opponents' scouting data (`/lol-clash/v1/scouting/`, `/lol-ranked/`): exact
/// mastery points or ranked wins and LP can be looked up on public
/// leaderboards, so these keys are rounded to a multiple of the step.
/// Per-champion game counts, win rates and KDA stay exact.
pub const ROUNDED_KEYS: &[(&str, i64)] = &[
    ("championpoints", 10_000),
    ("championpointssincelastlevel", 10_000),
    ("championpointsuntilnextlevel", 10_000),
    ("totalmasteryscore", 50),
    ("wins", 10),
    ("losses", 10),
    ("leaguepoints", 25),
    ("currentseasonwinsforrewards", 10),
    ("previousseasonwinsforrewards", 10),
    ("currentseasonsplitpoints", 100),
    ("previousseasonsplitpoints", 100),
    ("ratedrating", 100),
];

/// Resources whose numbers get [`ROUNDED_KEYS`].
pub fn is_scouting_uri(uri: &str) -> bool {
    uri.starts_with("/lol-clash/v1/scouting/") || uri.starts_with("/lol-ranked/")
}

fn round_step(key: &str) -> Option<i64> {
    let lower = key.to_ascii_lowercase();
    ROUNDED_KEYS
        .iter()
        .find(|(k, _)| *k == lower)
        .map(|(_, step)| *step)
}

/// Rounds the [`ROUNDED_KEYS`] anywhere in `value` (half up). Zero and
/// negative values (not set) stay; other numbers (floats, huge) become 0, since
/// they can't be rounded to the step.
pub fn round_scouting(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                let step = round_step(k).filter(|_| v.is_number());
                match step {
                    Some(step) => {
                        let rounded = match v.as_i64() {
                            Some(x) if x <= 0 => x,
                            Some(x) => x.saturating_add(step / 2) / step * step,
                            None => 0,
                        };
                        *v = Value::from(rounded);
                    }
                    None => round_scouting(v),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(round_scouting),
        _ => {}
    }
}

fn find_unrounded(value: &Value, path: &str, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let p = format!("{path}.{k}");
                match (round_step(k), v) {
                    (Some(step), Value::Number(n)) => {
                        if n.as_i64().is_none_or(|x| x > 0 && x % step != 0) {
                            found.push(format!("{p} (unrounded)"));
                        }
                    }
                    (_, v) => find_unrounded(v, &p, found),
                }
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                find_unrounded(v, &format!("{path}[{i}]"), found);
            }
        }
        _ => {}
    }
}

/// A Clash bracket's own ID and its matches' IDs (numbers, so the key rules
/// don't catch them; the URI segment is [`ID_AFTER_SEGMENT`]).
fn is_bracket_uri(uri: &str) -> bool {
    uri.starts_with("/lol-clash/v1/bracket/")
}

/// Chat IDs look like `<puuid>@eu1.pvp.net`, `<room>@sec.pvp.net` or
/// `<id>@pvp.net`.
pub fn is_chat_id(s: &str) -> bool {
    s.split_once('@')
        .is_some_and(|(_, host)| host.contains("pvp.net"))
}

/// JSON Web Tokens: `eyJ` (base64 of `{"`) plus two more dot-separated parts.
pub fn is_jwt(s: &str) -> bool {
    s.starts_with("eyJ") && s.split('.').count() == 3 && s.len() > 20
}

/// Chat IDs and long opaque tokens like puuids (UUIDs in the LCU). Short values
/// (`v1`, `session`, numeric champion IDs, enum names) don't qualify, nor do
/// long field names without a digit (`disallowBanningTeammateHoveredChampions`);
/// real IDs practically always contain one.
fn is_id_like(s: &str) -> bool {
    let opaque = s.len() >= 30
        && s.bytes().any(|b| b.is_ascii_digit())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    is_chat_id(s) || opaque
}

/// A bare `id` holding an opaque token, like the champ select session's UUID.
/// Numeric `id`s (actions, champions) are drafting data and stay.
fn is_opaque_id(key: &str, value: &Value) -> bool {
    key == "id" && value.as_str().is_some_and(is_id_like)
}

/// String values redacted whatever key holds them.
fn is_sensitive_value(s: &str) -> bool {
    is_chat_id(s) || is_jwt(s)
}

/// URI prefixes of chat-like resources (friends, conversations, messages,
/// presence, friend hover cards, in-game buddy list): free text about other
/// people, so every string in them is replaced.
const CHAT_URI_PREFIXES: &[&str] = &["/lol-chat/", "/lol-hovercard/", "/lol-game-client-chat/"];

fn is_chat_uri(uri: &str) -> bool {
    CHAT_URI_PREFIXES.iter().any(|p| uri.starts_with(p))
}

/// Summoner and account IDs in URIs (`/buddies/38449345`). Champion, skin and
/// item IDs have at most six digits, cell IDs one.
fn is_numeric_id(s: &str) -> bool {
    s.len() >= 7 && s.bytes().all(|b| b.is_ascii_digit())
}

/// `%40` → `@` and so on, so an encoded chat ID in a URI is still recognised
/// (and gets the same fake as the plain value elsewhere).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A string that holds a JSON object or array (the client nests some payloads,
/// like a party's `pty`, as strings).
fn parse_nested_json(s: &str) -> Option<Value> {
    let t = s.trim_start();
    if !(t.starts_with('{') || t.starts_with('[')) {
        return None;
    }
    serde_json::from_str::<Value>(s)
        .ok()
        .filter(|v| v.is_object() || v.is_array())
}

#[derive(Debug, Default)]
pub struct Redactor {
    fakes: HashMap<String, u64>,
}

impl Redactor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Redacts sensitive keys and chat-ID values anywhere in `value`.
    pub fn redact(&mut self, value: &mut Value) {
        match value {
            Value::Object(map) => {
                // Maps keyed by player (puuid or chat ID keys) get fake keys.
                if map.keys().any(|k| is_id_like(k)) {
                    let entries = std::mem::take(map);
                    for (k, v) in entries {
                        let key = if is_id_like(&k) {
                            self.fake_string(k)
                        } else {
                            k
                        };
                        map.insert(key, v);
                    }
                }
                for (k, v) in map.iter_mut() {
                    if is_sensitive_key(k) || is_opaque_id(k, v) {
                        self.replace_all(v);
                    } else {
                        self.redact(v);
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|v| self.redact(v)),
            Value::String(s) if is_sensitive_value(s) => {
                *value = Value::String(self.fake_string(s.clone()));
            }
            Value::String(s) => {
                if let Some(mut nested) = parse_nested_json(s) {
                    self.redact(&mut nested);
                    *value = Value::String(nested.to_string());
                }
            }
            _ => {}
        }
    }

    /// Redacts an event's data, with the extra rules for the resource at `uri`
    /// (Clash team names and IDs). Use this whenever the URI is known.
    pub fn redact_at(&mut self, uri: &str, value: &mut Value) {
        if lcu::is_clash_uri(uri) {
            self.redact_clash(value);
        }
        if is_bracket_uri(uri) {
            self.redact_bracket(value);
        }
        if is_scouting_uri(uri) {
            round_scouting(value);
        }
        self.redact(value);
    }

    /// The bracket's `id` and each `matches[].id`.
    fn redact_bracket(&mut self, value: &mut Value) {
        let mut ids: Vec<&mut Value> = Vec::new();
        if let Some(Value::Object(map)) = Some(&mut *value) {
            for (k, v) in map.iter_mut() {
                match (k.as_str(), v) {
                    ("id", v) => ids.push(v),
                    ("matches", Value::Array(ms)) => {
                        ids.extend(ms.iter_mut().filter_map(|m| m.get_mut("id")));
                    }
                    _ => {}
                }
            }
        }
        for v in ids {
            self.replace_all(v);
        }
    }

    /// Clash only: team names, string `id`s (roster IDs) and maps keyed by
    /// summoner-sized numbers (per-player stats and bids).
    fn redact_clash(&mut self, value: &mut Value) {
        match value {
            Value::Object(map) => {
                if map.keys().any(|k| is_numeric_id(k)) {
                    let entries = std::mem::take(map);
                    for (k, v) in entries {
                        let key = if is_numeric_id(&k) {
                            self.fake_string(k)
                        } else {
                            k
                        };
                        map.insert(key, v);
                    }
                }
                for (k, v) in map.iter_mut() {
                    let lower = k.to_ascii_lowercase();
                    if CLASH_TEXT_KEYS.contains(&lower.as_str()) || (k == "id" && v.is_string()) {
                        self.replace_all(v);
                    } else {
                        self.redact_clash(v);
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|v| self.redact_clash(v)),
            _ => {}
        }
    }

    /// Redacts a whole wire frame (`[opcode, event, payload]`): the usual keys,
    /// plus ID segments in the payload URI, plus all text of chat resources.
    pub fn redact_frame(&mut self, frame: &mut Value) {
        if let Some(payload) = frame.get_mut(2) {
            let uri = payload
                .get("uri")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if let Some(uri) = uri {
                if let Some(data) = payload.get_mut("data") {
                    if is_chat_uri(&uri) {
                        self.replace_chat_text(data);
                    } else if lcu::is_clash_uri(&uri) {
                        self.redact_clash(data);
                    }
                }
                payload["uri"] = Value::String(self.redact_uri(&uri));
            }
        }
        self.redact(frame);
    }

    /// Replaces path segments that look like IDs: chat IDs (also percent-encoded),
    /// long opaque tokens like puuids, and summoner-sized numbers. Short
    /// segments (`v1`, `session`, champion IDs) stay. Fakes match the ones the
    /// same value gets elsewhere in the recording.
    pub fn redact_uri(&mut self, uri: &str) -> String {
        let mut previous = "";
        uri.split('/')
            .map(|seg| {
                let after_id_segment = is_id_after(uri, previous, seg);
                previous = seg;
                let decoded = percent_decode(seg);
                if after_id_segment || is_id_like(&decoded) {
                    self.fake_string(decoded)
                } else if is_numeric_id(&decoded) {
                    self.fake_for(decoded).to_string()
                } else {
                    seg.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Everything under a sensitive key is replaced, including nested values.
    fn replace_all(&mut self, value: &mut Value) {
        match value {
            Value::Object(map) => map.values_mut().for_each(|v| self.replace_all(v)),
            Value::Array(items) => items.iter_mut().for_each(|v| self.replace_all(v)),
            Value::String(s) if !s.is_empty() => {
                *value = Value::String(self.fake_string(s.clone()));
            }
            Value::Number(n) if !is_not_set(n) => {
                *value = Value::from(self.fake_for(n.to_string()));
            }
            _ => {}
        }
    }

    /// Chat data: message bodies become their length, every other string a fake.
    fn replace_chat_text(&mut self, value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (k, v) in map.iter_mut() {
                    match v {
                        Value::String(s) if k == "body" => {
                            *v = Value::String(format!("<{} chars>", s.chars().count()));
                        }
                        _ => self.replace_chat_text(v),
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|v| self.replace_chat_text(v)),
            Value::String(s) if !s.is_empty() => {
                *value = Value::String(self.fake_string(s.clone()));
            }
            _ => {}
        }
    }

    fn fake_string(&mut self, original: String) -> String {
        format!("redacted-{}", self.fake_for(original))
    }

    fn fake_for(&mut self, original: String) -> u64 {
        let next = self.fakes.len() as u64 + 1;
        *self.fakes.entry(original).or_insert(next)
    }
}

/// Lists sensitive keys that still hold a non-empty value that isn't one of our
/// fakes, and chat IDs anywhere. Tests use it as a safety net; since it shares
/// the key list with the redactor, the tests also assert specific fields.
pub fn find_unredacted(value: &Value) -> Vec<String> {
    let mut found = Vec::new();
    walk(value, "", &mut found);
    found
}

/// [`find_unredacted`] plus the rules for the resource at `uri`: in Clash data,
/// team names, string `id`s and summoner-sized map keys must be fakes, and so
/// must ID segments of the URI.
pub fn find_unredacted_at(uri: &str, value: &Value) -> Vec<String> {
    let mut found = find_unredacted(value);
    if lcu::is_clash_uri(uri) {
        walk_clash(value, "", &mut found);
    }
    if is_bracket_uri(uri) {
        let mut ids: Vec<(&str, &Value)> = value.get("id").map(|v| ("id", v)).into_iter().collect();
        if let Some(ms) = value.get("matches").and_then(Value::as_array) {
            ids.extend(
                ms.iter()
                    .filter_map(|m| m.get("id"))
                    .map(|v| ("matches[].id", v)),
            );
        }
        for (p, v) in ids {
            check_leaves(v, p, &mut found);
        }
    }
    if is_scouting_uri(uri) {
        find_unrounded(value, "", &mut found);
    }
    let mut previous = "";
    for seg in uri.split('/') {
        if is_id_after(uri, previous, seg) && !seg.starts_with("redacted-") {
            found.push(format!("uri segment after {previous}"));
        }
        previous = seg;
    }
    found
}

fn walk_clash(value: &Value, path: &str, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let p = format!("{path}.{k}");
                if is_numeric_id(k) {
                    found.push(format!("{p} (key)"));
                }
                let lower = k.to_ascii_lowercase();
                if CLASH_TEXT_KEYS.contains(&lower.as_str()) || (k == "id" && v.is_string()) {
                    check_leaves(v, &p, found);
                } else {
                    walk_clash(v, &p, found);
                }
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                walk_clash(v, &format!("{path}[{i}]"), found);
            }
        }
        _ => {}
    }
}

fn walk(value: &Value, path: &str, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let p = format!("{path}.{k}");
                // A map keyed by player ID must have fake keys.
                if is_id_like(k) && !k.starts_with("redacted-") {
                    found.push(format!("{p} (key)"));
                }
                if is_sensitive_key(k) || is_opaque_id(k, v) {
                    check_leaves(v, &p, found);
                } else {
                    walk(v, &p, found);
                }
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                walk(v, &format!("{path}[{i}]"), found);
            }
        }
        Value::String(s) if is_sensitive_value(s) => found.push(path.to_string()),
        Value::String(s) => {
            if let Some(nested) = parse_nested_json(s) {
                walk(&nested, &format!("{path}<json>"), found);
            }
        }
        _ => {}
    }
}

fn check_leaves(value: &Value, path: &str, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => map
            .iter()
            .for_each(|(k, v)| check_leaves(v, &format!("{path}.{k}"), found)),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .for_each(|(i, v)| check_leaves(v, &format!("{path}[{i}]"), found)),
        Value::String(s) if !s.is_empty() && !s.starts_with("redacted-") => {
            found.push(path.to_string());
        }
        // Fakes are small counters; real IDs are large. Small negative numbers
        // are "not set" markers.
        Value::Number(n) if n.as_i64().is_none_or(|x| x > 100_000 || x <= NOT_SET_BELOW) => {
            found.push(path.to_string());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Shapes from the 16.23 spec (`LolClashRoster`, `LolClashBracket`, ...).
    #[test]
    fn clash_rosters_brackets_and_stats_are_redacted_consistently() {
        let mut r = Redactor::new();
        let mut summary = json!([{"state": "SCOUTING", "tournamentId": 4321, "rosterId": "778899", "bracketId": 556677}]);
        let mut roster = json!({
            "id": "778899", "tournamentId": 4321, "name": "Team Real Name", "shortName": "TRN",
            "captainSummonerId": 12_345_678_901_u64, "invitationId": "inv-1234", "tier": 3,
            "members": [
                {"summonerId": 12_345_678_901_u64, "position": "MIDDLE", "tier": 3, "inviterId": 98_765_432_101_u64, "replacedSummonerId": 0},
                {"summonerId": 22_345_678_901_u64, "position": "UTILITY", "tier": 3}
            ],
            "suggestedInvites": [{"summonerId": 32_345_678_901_u64}]
        });
        let mut bracket = json!({
            "id": 556677, "tournamentId": 4321, "size": 8,
            "rosters": [{"rosterId": 778899, "name": "Team Real Name", "shortName": "TRN", "logo": 5},
                        {"rosterId": 112233, "name": "Other Team", "shortName": "OTH", "logo": 2}],
            "matches": [{"id": 1, "round": 1, "rosterId1": 778899, "rosterId2": 112233, "winnerId": 0,
                         "gameId": 7_000_000_001_u64, "status": "UPCOMING"}]
        });
        let mut stats = json!({"rosterId": 778899, "rosterName": "Team Real Name", "rosterShortName": "TRN",
            "tournamentNameLocKey": "clash_name_x",
            "playerStats": {"12345678901": {"rawStatsSum": {"KILLS": 4}}},
            "periodStats": [{"period": 1, "playerBids": {"22345678901": 400}}]});
        let mut team_data = json!({"captain": {"summonerId": 12_345_678_901_u64, "role": "MIDDLE"},
            "members": [{"summonerId": 22_345_678_901_u64, "role": "UTILITY"}]});

        let roster_uri = r.redact_uri("/lol-clash/v1/roster/778899");
        let stats_uri = r.redact_uri("/lol-clash/v1/roster/778899/stats");
        for (uri, v) in [
            ("/lol-clash/v1/tournament-summary", &mut summary),
            ("/lol-clash/v1/roster/778899", &mut roster),
            ("/lol-clash/v1/bracket/556677", &mut bracket),
            ("/lol-clash/v1/roster/778899/stats", &mut stats),
            ("/lol-clash/v1/thirdparty/team-data", &mut team_data),
        ] {
            r.redact_at(uri, v);
            assert_eq!(
                find_unredacted_at(&r.redact_uri(uri), v),
                Vec::<String>::new(),
                "{uri}: {v}"
            );
        }
        // Same team, same fake: URI, roster id, summary and bracket.
        let fake = roster["id"].as_str().unwrap().to_string();
        assert!(fake.starts_with("redacted-"), "{fake}");
        assert_eq!(roster_uri, format!("/lol-clash/v1/roster/{fake}"));
        assert_eq!(stats_uri, format!("/lol-clash/v1/roster/{fake}/stats"));
        assert_eq!(summary[0]["rosterId"], json!(fake));
        let n: u64 = fake.trim_start_matches("redacted-").parse().unwrap();
        assert_eq!(bracket["matches"][0]["rosterId1"], json!(n));
        assert_eq!(bracket["rosters"][0]["rosterId"], json!(n));
        // Same player across roster, stats keys and team data.
        assert_eq!(
            roster["members"][0]["summonerId"],
            team_data["captain"]["summonerId"]
        );
        // Team names gone, drafting and tournament data kept.
        assert!(roster["name"].as_str().unwrap().starts_with("redacted-"));
        assert!(
            bracket["rosters"][1]["shortName"]
                .as_str()
                .unwrap()
                .starts_with("redacted-")
        );
        assert!(
            stats["rosterName"]
                .as_str()
                .unwrap()
                .starts_with("redacted-")
        );
        assert!(
            stats["playerStats"]
                .as_object()
                .unwrap()
                .keys()
                .all(|k| k.starts_with("redacted-"))
        );
        assert_eq!(roster["members"][1]["position"], "UTILITY");
        assert_eq!(roster["members"][0]["replacedSummonerId"], 0);
        assert_eq!(summary[0]["state"], "SCOUTING");
        assert_eq!(summary[0]["tournamentId"], 4321);
        // A bracket ID finds all of its teams: faked too.
        assert_ne!(bracket["id"], 556677);
        assert_eq!(bracket["matches"][0]["status"], "UPCOMING");
        assert_eq!(stats["tournamentNameLocKey"], "clash_name_x");
        assert_eq!(
            stats["playerStats"]
                .as_object()
                .unwrap()
                .values()
                .next()
                .unwrap()["rawStatsSum"]["KILLS"],
            4
        );
    }

    #[test]
    fn clash_rules_stay_out_of_other_resources() {
        let mut r = Redactor::new();
        let mut v = json!({"name": "Ahri", "id": "some-id", "map": {"12345678901": 1}});
        r.redact_at("/lol-champ-select/v1/session", &mut v);
        assert_eq!(v["name"], "Ahri");
        assert_eq!(v["id"], "some-id");
        assert!(v["map"].get("12345678901").is_some());
        assert_eq!(
            r.redact_uri("/lol-champ-select/v1/session"),
            "/lol-champ-select/v1/session"
        );
    }

    #[test]
    fn clash_wire_frames_get_clash_rules() {
        let mut r = Redactor::new();
        let mut frame = json!([8, "OnJsonApiEvent", {"uri": "/lol-clash/v1/roster/abc", "eventType": "Update",
            "data": {"id": "abc", "name": "Team Real Name", "members": [{"summonerId": 12_345_678_901_u64}]}}]);
        r.redact_frame(&mut frame);
        let p = &frame[2];
        assert!(
            p["uri"]
                .as_str()
                .unwrap()
                .ends_with(p["data"]["id"].as_str().unwrap())
        );
        assert_eq!(
            find_unredacted_at(p["uri"].as_str().unwrap(), &p["data"]),
            Vec::<String>::new()
        );
    }

    fn is_fake(v: &Value) -> bool {
        match v {
            Value::String(s) => s.starts_with("redacted-"),
            Value::Number(n) => n.as_u64().is_some_and(|x| x < 1000),
            _ => false,
        }
    }

    #[test]
    fn redacts_players_and_chat_credentials() {
        let mut v = json!({
            "gameId": 7_123_456_789_u64,
            "myTeam": [
                {"cellId": 0, "summonerId": 2_394_871_234_u64, "puuid": "abc-123", "gameName": "Someone", "tagLine": "EUW"},
                {"cellId": 1, "summonerId": 0, "puuid": "", "gameName": ""}
            ],
            "chatDetails": {
                "multiUserChatId": "room@pvp.net",
                "multiUserChatPassword": "secret",
                "mucJwtDto": {"jwt": "eyJ...", "channelClaim": "claim", "domain": "pvp.net"}
            },
            "chatRoomName": "c1~abc@sec.pvp.net",
            "chatRoomPassword": "pw"
        });
        Redactor::new().redact(&mut v);
        for path in [
            "/gameId",
            "/myTeam/0/summonerId",
            "/myTeam/0/puuid",
            "/myTeam/0/gameName",
            "/myTeam/0/tagLine",
            "/chatDetails/multiUserChatId",
            "/chatDetails/multiUserChatPassword",
            "/chatDetails/mucJwtDto/jwt",
            "/chatRoomName",
            "/chatRoomPassword",
        ] {
            assert!(
                is_fake(v.pointer(path).unwrap()),
                "{path}: {}",
                v.pointer(path).unwrap()
            );
        }
        assert_eq!(find_unredacted(&v), Vec::<String>::new());
        // Empty values keep their meaning; drafting fields are untouched.
        assert_eq!(v["myTeam"][1]["summonerId"], json!(0));
        assert_eq!(v["myTeam"][1]["puuid"], json!(""));
        assert_eq!(v["myTeam"][0]["cellId"], json!(0));
    }

    #[test]
    fn key_fragments_match_anywhere_case_insensitively() {
        for key in [
            "PUUID",
            "summonerID",
            "fromSummonerId",
            "toSummonerId",
            "fromObfuscatedSummonerId",
            "obfuscatedGameName",
            "riotIdGameName",
            "riotIdTagLine",
            "gameTag",
            "summonerInternalName",
            "remotingAuthToken",
            "chatRoomPassword",
            "mucJWT",
            "partySummoners",
            "discordId",
            "summoners",
            "customLobbyName",
        ] {
            assert!(is_sensitive_key(key), "{key}");
        }
        for key in [
            "championId",
            "championPickIntent",
            "nameVisibilityType",
            "gameMode",
            "queueId",
            "assignedPosition",
            "name",
            "pinDropSummoners",
        ] {
            assert!(!is_sensitive_key(key), "{key}");
        }
    }

    #[test]
    fn chat_ids_are_redacted_whatever_the_key() {
        let mut v =
            json!({"id": "aaaa-bbbb@eu1.pvp.net", "pid": "aaaa-bbbb@eu1.pvp.net", "x": "plain"});
        Redactor::new().redact(&mut v);
        assert!(is_fake(&v["id"]));
        assert_eq!(v["id"], v["pid"], "same original, same fake");
        assert_eq!(v["x"], json!("plain"));
    }

    #[test]
    fn jwts_are_redacted_whatever_the_key_even_bare() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.c2lnbmF0dXJlLWJ5dGVz";
        let mut v = json!({"value": jwt, "list": [jwt]});
        Redactor::new().redact(&mut v);
        assert!(is_fake(&v["value"]) && is_fake(&v["list"][0]));
        let mut bare = json!(jwt);
        Redactor::new().redact(&mut bare);
        assert!(is_fake(&bare));
        assert!(!is_jwt("eyJ"));
        assert!(!is_jwt("plain.text.value"));
    }

    #[test]
    fn maps_keyed_by_puuid_get_fake_keys() {
        let puuid = "a1b2c3-".repeat(11) + "d";
        let mut v = json!({"players": {puuid.clone(): {"puuid": puuid.clone(), "ready": true}}});
        Redactor::new().redact(&mut v);
        let players = v["players"].as_object().unwrap();
        let (key, member) = players.iter().next().unwrap();
        assert!(key.starts_with("redacted-"), "{key}");
        assert_eq!(member["puuid"], json!(key), "key and value share the fake");
        assert_eq!(member["ready"], json!(true));
        assert!(!v.to_string().contains(&puuid));
    }

    #[test]
    fn dodge_and_spectator_fields_are_redacted() {
        let mut v = json!({"readyCheck": {"declinerIds": [123_456_789]}, "dodgeData": {"dodgerId": 987_654_321},
                           "gameDodge": {"dodgeIds": [111_222_333]}, "gameData": {"spectatorKey": "abc"},
                           "gameClient": {"serverIp": "1.2.3.4", "serverPort": 5000}});
        Redactor::new().redact(&mut v);
        for path in [
            "/readyCheck/declinerIds/0",
            "/dodgeData/dodgerId",
            "/gameDodge/dodgeIds/0",
            "/gameData/spectatorKey",
            "/gameClient/serverIp",
        ] {
            assert!(is_fake(v.pointer(path).unwrap()), "{path}");
        }
    }

    #[test]
    fn uuid_puuids_in_uris_and_keys_are_caught() {
        let puuid = "8a7f4c2e-1b3d-4e5f-9a8b-7c6d5e4f3a2b";
        let mut r = Redactor::new();
        assert_eq!(
            r.redact_uri(&format!("/lol-summoner/v2/summoners/puuid/{puuid}")),
            "/lol-summoner/v2/summoners/puuid/redacted-1"
        );
        let leaked = json!({"players": {puuid: {"ready": true}}});
        assert_eq!(
            find_unredacted(&leaked),
            vec![format!(".players.{puuid} (key)")]
        );
        let mut v = leaked.clone();
        r.redact(&mut v);
        assert_eq!(find_unredacted(&v), Vec::<String>::new());
    }

    #[test]
    fn long_field_names_are_not_mistaken_for_ids() {
        let mut v = json!({
            "disallowBanningTeammateHoveredChampions": true,
            "perPositionDisallowedSummonerSpells": {},
            "8a7f4c2e-1b3d-4e5f-9a8b-7c6d5e4f3a2b": {"championId": 1}
        });
        Redactor::new().redact(&mut v);
        assert_eq!(v["disallowBanningTeammateHoveredChampions"], json!(true));
        assert!(v.get("perPositionDisallowedSummonerSpells").is_some());
        assert!(v.get("8a7f4c2e-1b3d-4e5f-9a8b-7c6d5e4f3a2b").is_none());
    }

    #[test]
    fn chat_ids_without_a_subdomain_are_redacted() {
        let mut v = json!({"chatTarget": "abcdef1234@pvp.net", "note": "a@b.com"});
        Redactor::new().redact(&mut v);
        assert!(is_fake(&v["chatTarget"]), "{}", v["chatTarget"]);
        assert_eq!(v["note"], json!("a@b.com"));
    }

    #[test]
    fn opaque_ids_are_redacted_numeric_ids_stay() {
        let mut v = json!({
            "id": "ff0a24d1-c02e-4e8c-84c5-9f65126c6c85",
            "actions": [[{"id": 3, "championId": 157}]]
        });
        assert_eq!(find_unredacted(&v), vec![".id".to_string()]);
        Redactor::new().redact(&mut v);
        assert!(is_fake(&v["id"]), "{}", v["id"]);
        assert_eq!(v["actions"][0][0]["id"], json!(3));
        assert_eq!(find_unredacted(&v), Vec::<String>::new());
    }

    #[test]
    fn fakes_are_stable_within_a_recording() {
        let mut r = Redactor::new();
        let mut a = json!({"summonerId": 555_555_555, "puuid": "p1"});
        let mut b = json!({"summonerId": 555_555_555, "puuid": "p2"});
        r.redact(&mut a);
        r.redact(&mut b);
        assert_eq!(a["summonerId"], b["summonerId"]);
        assert_ne!(a["puuid"], b["puuid"]);
    }

    #[test]
    fn friend_presence_frame_is_fully_cleaned() {
        // The case the validator found leaking: a friend update during champ select.
        let mut frame = json!([8, "OnJsonApiEvent", {
            "uri": "/lol-chat/v1/friends/aaaa-bbbb@eu1.pvp.net",
            "eventType": "Update",
            "data": {
                "name": "FriendName", "gameName": "FriendName", "gameTag": "EUW",
                "id": "aaaa-bbbb@eu1.pvp.net", "pid": "aaaa-bbbb@eu1.pvp.net",
                "note": "my irl friend Bob", "statusMessage": "hi", "availability": "chat",
                "icon": 4567
            }
        }]);
        Redactor::new().redact_frame(&mut frame);
        let data = &frame[2]["data"];
        for key in [
            "name",
            "gameName",
            "gameTag",
            "id",
            "pid",
            "note",
            "statusMessage",
        ] {
            assert!(is_fake(&data[key]), "{key}: {}", data[key]);
        }
        let uri = frame[2]["uri"].as_str().unwrap();
        assert!(uri.starts_with("/lol-chat/v1/friends/redacted-"), "{uri}");
        assert!(!frame.to_string().contains("Bob"));
        assert!(!frame.to_string().contains("pvp.net"));
    }

    #[test]
    fn chat_messages_keep_length_only() {
        let mut frame = json!([8, "OnJsonApiEvent", {
            "uri": "/lol-chat/v1/conversations/x/messages",
            "eventType": "Create",
            "data": {"body": "gl hf, I'm Jäger", "fromSummonerId": 123_456_789_u64, "type": "groupchat"}
        }]);
        Redactor::new().redact_frame(&mut frame);
        let data = &frame[2]["data"];
        assert_eq!(data["body"], json!("<16 chars>"));
        assert!(is_fake(&data["fromSummonerId"]));
    }

    #[test]
    fn non_chat_frames_keep_their_fields() {
        let mut frame = json!([8, "OnJsonApiEvent", {
            "uri": "/lol-champ-select/v1/session",
            "data": {"body": "keep", "name": "Ahri", "timer": {"phase": "BAN_PICK"}}
        }]);
        Redactor::new().redact_frame(&mut frame);
        assert_eq!(frame[2]["data"]["body"], json!("keep"));
        assert_eq!(frame[2]["data"]["name"], json!("Ahri"));
        assert_eq!(frame[2]["uri"], json!("/lol-champ-select/v1/session"));
    }

    #[test]
    fn uri_id_segments() {
        let mut r = Redactor::new();
        let puuid = "x9y8z7_".repeat(11) + "w";
        assert_eq!(
            r.redact_uri(&format!("/lol-summoner/v2/summoners/puuid/{puuid}")),
            "/lol-summoner/v2/summoners/puuid/redacted-1"
        );
        assert_eq!(
            r.redact_uri("/lol-champ-select/v1/grid-champions/103"),
            "/lol-champ-select/v1/grid-champions/103"
        );
        assert_eq!(
            r.redact_uri("/lol-champ-select/v1/skin-carousel-skins/266012"),
            "/lol-champ-select/v1/skin-carousel-skins/266012"
        );
    }

    #[test]
    fn scouting_numbers_are_rounded_and_bracket_ids_faked() {
        let mut r = Redactor::new();
        let mut champs = json!([{"playerId": 1234567, "totalMasteryScore": 669,
            "topMasteries": [{"championId": 13, "championLevel": 105, "championPoints": 1129999}],
            "topSeasonChampions": [{"championId": 101, "winCount": 35, "gameCount": 65, "kda": "4.2"}]}]);
        r.redact_at("/lol-clash/v1/scouting/champions", &mut champs);
        assert_eq!(champs[0]["totalMasteryScore"], 650);
        assert_eq!(champs[0]["topMasteries"][0]["championPoints"], 1_130_000);
        assert_eq!(champs[0]["topMasteries"][0]["championLevel"], 105);
        assert_eq!(champs[0]["topSeasonChampions"][0]["gameCount"], 65);
        assert_eq!(champs[0]["playerId"], 1);
        let mut ranked = json!({"queueMap": {"RANKED_SOLO_5x5": {"wins": 114, "losses": 0, "leaguePoints": 38, "tier": "DIAMOND"}}});
        r.redact_at("/lol-ranked/v1/ranked-stats/abc", &mut ranked);
        let solo = &ranked["queueMap"]["RANKED_SOLO_5x5"];
        assert_eq!(
            (&solo["wins"], &solo["losses"], &solo["leaguePoints"]),
            (&json!(110), &json!(0), &json!(50))
        );
        let mut edge = json!({"wins": -1, "losses": i64::MAX, "leaguePoints": 2.5});
        round_scouting(&mut edge);
        assert_eq!(
            edge,
            json!({"wins": -1, "losses": i64::MAX / 10 * 10, "leaguePoints": 0})
        );
        assert_eq!(
            find_unredacted_at("/lol-ranked/x/redacted-1", &json!({"leaguePoints": 2.5})),
            vec![".leaguePoints (unrounded)"]
        );
        // "Not set" stays; a large negative could be an ID.
        let mut ids = json!({"summonerId": -1, "gameId": -4_000_000});
        r.redact(&mut ids);
        assert_eq!(ids["summonerId"], -1);
        assert!(ids["gameId"].as_u64().is_some());
        // Elsewhere numbers stay exact.
        let mut other = json!({"wins": 7});
        r.redact_at("/lol-clash/v1/roster/x", &mut other);
        assert_eq!(other["wins"], 7);
        assert_eq!(
            find_unredacted_at(
                "/lol-ranked/v1/ranked-stats/redacted-9",
                &json!({"wins": 7})
            ),
            vec![".wins (unrounded)"]
        );

        let mut bracket =
            json!({"id": 577523, "matches": [{"id": 6547731, "rosterId1": 7654321, "round": 1}]});
        r.redact_at("/lol-clash/v1/bracket/577523", &mut bracket);
        let fake = bracket["id"].as_u64().unwrap();
        assert!(fake < 100, "{bracket}");
        assert!(bracket["matches"][0]["id"].as_u64().unwrap() < 100);
        assert_eq!(bracket["matches"][0]["round"], 1);
        // The URI and `bracketId` elsewhere get the same fake.
        assert_eq!(
            r.redact_uri("/lol-clash/v1/bracket/577523"),
            format!("/lol-clash/v1/bracket/redacted-{fake}")
        );
        let mut summary = json!({"bracketId": 577523});
        r.redact_at("/lol-clash/v1/tournament-summary", &mut summary);
        assert_eq!(summary["bracketId"], fake);
        assert!(
            !find_unredacted_at("/lol-clash/v1/bracket/577523", &json!({"id": 577523})).is_empty()
        );
    }

    #[test]
    fn id_segments_after_ranked_stats_and_summoners() {
        let mut r = Redactor::new();
        assert_eq!(
            r.redact_uri("/lol-ranked/v1/ranked-stats/0a1b2c3d-1111-2222-3333-444455556666"),
            "/lol-ranked/v1/ranked-stats/redacted-1"
        );
        // The summoner ID gets the fake the same number gets in data.
        assert_eq!(
            r.redact_uri("/lol-summoner/v1/summoners/123"),
            "/lol-summoner/v1/summoners/redacted-2"
        );
        let mut roster = json!({"members": [{"summonerId": 123}]});
        r.redact_at("/lol-clash/v1/roster/redacted-5", &mut roster);
        assert_eq!(roster["members"][0]["summonerId"], 2);
        // A cell index in champ select is not an ID.
        assert_eq!(
            r.redact_uri("/lol-champ-select/v1/summoners/3"),
            "/lol-champ-select/v1/summoners/3"
        );
        assert!(find_unredacted_at("/lol-champ-select/v1/summoners/3", &json!({})).is_empty());
        assert_eq!(
            find_unredacted_at("/lol-ranked/v1/ranked-stats/rawpuuid", &json!({})),
            vec!["uri segment after ranked-stats"]
        );
    }

    // Leaks found in a live 16.22 wire capture (ARAM, friends online).

    #[test]
    fn percent_encoded_chat_ids_in_uris_get_the_value_fake() {
        let mut r = Redactor::new();
        let mut v = json!({"multiUserChatId": "0aeb-1234@eu2.pvp.net"});
        r.redact(&mut v);
        let uri = r.redact_uri("/lol-chat/v1/conversations/0aeb-1234%40eu2.pvp.net/participants");
        assert_eq!(
            uri,
            format!(
                "/lol-chat/v1/conversations/{}/participants",
                v["multiUserChatId"].as_str().unwrap()
            )
        );
    }

    #[test]
    fn summoner_ids_in_uris_get_the_value_fake() {
        let mut r = Redactor::new();
        let mut v = json!({"summonerId": 38_449_345});
        r.redact(&mut v);
        assert_eq!(
            r.redact_uri("/lol-game-client-chat/v2/buddies/38449345"),
            format!("/lol-game-client-chat/v2/buddies/{}", v["summonerId"])
        );
    }

    #[test]
    fn json_nested_in_strings_is_redacted() {
        let pty = r#"{"partyId":"p","summonerPuuids":["aaaa-bbbb"],"summoners":[79637248,38449345],"queueId":2400}"#;
        let mut v =
            json!({"lol": {"pty": pty, "regalia": "{\"crestType\":1}", "plain": "{not json"}});
        Redactor::new().redact(&mut v);
        let inner: Value = serde_json::from_str(v["lol"]["pty"].as_str().unwrap()).unwrap();
        assert!(is_fake(&inner["summonerPuuids"][0]));
        assert!(inner["summoners"].as_array().unwrap().iter().all(is_fake));
        assert_eq!(inner["queueId"], json!(2400));
        assert_eq!(v["lol"]["regalia"], json!(r#"{"crestType":1}"#));
        assert_eq!(v["lol"]["plain"], json!("{not json"));
        assert_eq!(find_unredacted(&v), Vec::<String>::new());
        // The safety net looks inside nested JSON too.
        let leaky = json!({"pty": r#"{"puuid":"real"}"#});
        assert_eq!(
            find_unredacted(&leaky),
            vec![".pty<json>.puuid".to_string()]
        );
    }

    #[test]
    fn hovercard_frames_are_cleaned_like_chat() {
        let mut frame = json!([8, "OnJsonApiEvent", {
            "uri": "/lol-hovercard/v1/friend-info/aaaa",
            "eventType": "Update",
            "data": {
                "discordId": "Dave 6.0", "statusMessage": "throat goat",
                "partySummoners": ["Friend1", "Friend2"], "summonerId": 38_449_345,
                "lol": {"pty": "{\"summoners\":[38449345]}", "gameMode": "KIWI"}
            }
        }]);
        Redactor::new().redact_frame(&mut frame);
        let s = frame.to_string();
        for leak in ["Dave", "throat", "Friend1", "38449345"] {
            assert!(!s.contains(leak), "{leak} in {s}");
        }
    }
}
