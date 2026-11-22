use super::*;
use crate::redact::find_unredacted;

#[test]
fn path_list_is_sane() {
    let all: Vec<_> = paths().collect();
    assert!(all.len() > 50, "{}", all.len());
    for p in &all {
        assert!(p.starts_with("/lol-"), "{p}");
        assert!(!p.contains('{'), "{p}");
        for banned in [
            "obfuscated",
            "jwt",
            "token",
            "chat",
            "by-puuids",
            "muted",
            "custom-games",
        ] {
            assert!(!p.contains(banned), "{p} must be excluded");
        }
    }
    assert!(all.contains(&"/lol-champions/v1/owned-champions-minimal"));
    assert!(all.contains(&"/lol-lobby/v2/lobby"));
}

#[tokio::test]
async fn results_are_redacted_and_counted() {
    let (results, stats) = collect(["/a", "/b", "/c"], |path| async move {
        match path.as_str() {
            "/a" => Fetched::Ok(json!({"puuid": "real", "queueId": 420})),
            "/b" => Fetched::NotFound,
            _ => Fetched::Failed("boom".into()),
        }
    })
    .await;
    assert_eq!(
        stats,
        SweepStats {
            ok: 1,
            not_found: 1,
            failed: 1,
            skipped: 0,
        }
    );
    assert_eq!(results["/a"]["data"]["queueId"], 420);
    assert_eq!(find_unredacted(&results), Vec::<String>::new());
    assert_eq!(results["/b"]["status"], "not_found");
    assert_eq!(results["/c"]["error"], "boom");
}

fn gameflow(queue: Value) -> Fetched {
    Fetched::Ok(json!({"gameData": {"queue": queue}}))
}

#[tokio::test]
async fn clash_only_paths_are_skipped_outside_clash() {
    // The Clash path comes before the gameflow session in the list; the
    // session is still fetched first and decides.
    let paths = ["/lol-clash/v1/historyandwinners", GAMEFLOW_SESSION, "/x"];
    let (results, stats) = collect(paths, |path| async move {
        match path.as_str() {
            GAMEFLOW_SESSION => gameflow(json!({"id": 3130, "type": "NORMAL"})),
            "/lol-clash/v1/historyandwinners" => panic!("must not be fetched"),
            _ => Fetched::Ok(json!(1)),
        }
    })
    .await;
    assert_eq!((stats.ok, stats.skipped), (2, 1));
    assert_eq!(
        results["/lol-clash/v1/historyandwinners"]["status"],
        "skipped"
    );
}

#[tokio::test]
async fn clash_only_paths_are_fetched_in_clash() {
    let paths = [GAMEFLOW_SESSION, "/lol-clash/v1/historyandwinners"];
    let (results, stats) = collect(paths, |path| async move {
        match path.as_str() {
            GAMEFLOW_SESSION => gameflow(json!({"id": 700, "type": "CLASH"})),
            _ => Fetched::Ok(json!({"history": []})),
        }
    })
    .await;
    assert_eq!((stats.ok, stats.skipped), (2, 0));
    assert_eq!(results["/lol-clash/v1/historyandwinners"]["status"], "ok");
}

#[test]
fn clash_detection() {
    let s = |q: Value| json!({"gameData": {"queue": q}});
    assert!(is_clash_queue(Some(&s(json!({"id": 700})))));
    assert!(is_clash_queue(Some(&s(json!({"id": 720})))));
    assert!(is_clash_queue(Some(&s(
        json!({"id": 1, "type": "ARAM_CLASH"})
    ))));
    // Seen in 16.22 captures: ARAM (KIWI) and draft customs.
    assert!(!is_clash_queue(Some(&s(
        json!({"id": 2400, "type": "KIWI"})
    ))));
    assert!(!is_clash_queue(Some(&s(
        json!({"id": 3130, "type": "NORMAL"})
    ))));
    // No session: keep Clash data rather than risk losing it.
    assert!(is_clash_queue(None));
    assert!(is_clash_queue(Some(&json!({}))));
}

#[test]
fn clash_only_paths_are_in_the_sweep_list() {
    let all: Vec<_> = paths().collect();
    for p in CLASH_ONLY {
        assert!(all.contains(p), "{p} isn't swept, so skipping it is moot");
    }
    assert!(all.contains(&GAMEFLOW_SESSION));
}
