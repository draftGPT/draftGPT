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
#[path = "../tests/unit/redact.rs"]
mod tests;
