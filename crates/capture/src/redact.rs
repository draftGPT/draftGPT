//! Replaces identifying values before anything reaches disk.
//!
//! Values are swapped for stable fakes per recording: the same original value
//! always maps to the same fake, so "this cell is the same player as that action"
//! still holds, across the main recording and its wire file. Empty values (`0`,
//! `""`, `null`) are kept as they are, because they carry meaning (bot, hidden
//! name, not set).
//!
//! Four layers, from narrow to broad:
//! 1. Keys containing an identity or secret fragment ([`SENSITIVE_KEY_PARTS`]).
//! 2. String values that are chat IDs (`<puuid>@<region>.pvp.net`) or JWTs,
//!    whatever the key (a bare value has no key to match).
//! 3. Wire frames for chat resources: every string is replaced (message text by
//!    its length), since friend names, notes and messages are free text.
//! 4. Wire frame URIs: path segments that hold an ID are replaced.
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
    // A game ID plus region finds the match and everyone in it.
    "gameid",
    // Lets anyone spectate or connect to the game.
    "spectatorkey",
    "serverip",
    // Chat rooms and credentials.
    "multiuserchatid",
    "chatroomname",
    "channelclaim",
    "password",
    "token",
    "jwt",
];

pub fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    SENSITIVE_KEY_PARTS.iter().any(|p| lower.contains(p))
}

/// Chat IDs look like `<puuid>@eu1.pvp.net` or `<room>@sec.pvp.net`.
pub fn is_chat_id(s: &str) -> bool {
    s.contains('@') && s.contains(".pvp.net")
}

/// JSON Web Tokens: `eyJ` (base64 of `{"`) plus two more dot-separated parts.
pub fn is_jwt(s: &str) -> bool {
    s.starts_with("eyJ") && s.split('.').count() == 3 && s.len() > 20
}

/// Chat IDs and long opaque tokens like puuids (78 characters). Short values
/// (`v1`, `session`, numeric champion IDs, enum names) don't qualify.
fn is_id_like(s: &str) -> bool {
    let opaque = s.len() >= 30
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    is_chat_id(s) || opaque
}

/// String values redacted whatever key holds them.
fn is_sensitive_value(s: &str) -> bool {
    is_chat_id(s) || is_jwt(s)
}

/// URI prefix of chat resources (friends, conversations, messages, presence).
const CHAT_URI_PREFIX: &str = "/lol-chat/";

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
                    if is_sensitive_key(k) {
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
                if uri.starts_with(CHAT_URI_PREFIX)
                    && let Some(data) = payload.get_mut("data")
                {
                    self.replace_chat_text(data);
                }
                payload["uri"] = Value::String(self.redact_uri(&uri));
            }
        }
        self.redact(frame);
    }

    /// Replaces path segments that look like IDs: chat IDs, and long opaque
    /// tokens like puuids (78 characters). Short segments (`v1`, `session`,
    /// numeric champion IDs) stay.
    pub fn redact_uri(&mut self, uri: &str) -> String {
        uri.split('/')
            .map(|seg| {
                if is_id_like(seg) {
                    self.fake_string(seg.to_owned())
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
            Value::Number(n) if n.as_f64() != Some(0.0) => {
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

fn walk(value: &Value, path: &str, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let p = format!("{path}.{k}");
                if is_sensitive_key(k) {
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
        // Fakes are small counters; real IDs are large.
        Value::Number(n) if n.as_u64().is_none_or(|x| x > 100_000) => {
            found.push(path.to_string());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        let puuid = "a".repeat(78);
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
        let puuid = "x".repeat(78);
        assert_eq!(
            r.redact_uri(&format!("/lol-summoner/v2/summoners/puuid/{puuid}")),
            "/lol-summoner/v2/summoners/puuid/redacted-1"
        );
        assert_eq!(
            r.redact_uri("/lol-champ-select/v1/grid-champions/103"),
            "/lol-champ-select/v1/grid-champions/103"
        );
    }
}
