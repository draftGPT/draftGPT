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
    // Lets anyone spectate or connect to the game.
    "spectatorkey",
    "serverip",
    // Player names in a friend's party, and linked accounts.
    "partysummoners",
    "discord",
    // Chat rooms and credentials.
    "multiuserchatid",
    "chatroomname",
    "channelclaim",
    "password",
    "token",
    "jwt",
];

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

    /// Redacts a whole wire frame (`[opcode, event, payload]`): the usual keys,
    /// plus ID segments in the payload URI, plus all text of chat resources.
    pub fn redact_frame(&mut self, frame: &mut Value) {
        if let Some(payload) = frame.get_mut(2) {
            let uri = payload
                .get("uri")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if let Some(uri) = uri {
                if is_chat_uri(&uri)
                    && let Some(data) = payload.get_mut("data")
                {
                    self.replace_chat_text(data);
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
        uri.split('/')
            .map(|seg| {
                let decoded = percent_decode(seg);
                if is_id_like(&decoded) {
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
            "partySummoners",
            "discordId",
            "summoners",
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
        let puuid = "x".repeat(78);
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
