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
    assert!(!find_unredacted_at("/lol-clash/v1/bracket/577523", &json!({"id": 577523})).is_empty());
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
    let mut v = json!({"lol": {"pty": pty, "regalia": "{\"crestType\":1}", "plain": "{not json"}});
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
