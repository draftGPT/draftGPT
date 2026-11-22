use super::*;
use axum::http::HeaderValue;

#[test]
fn capture_file_names() {
    assert!(is_capture_file_name("2026-11-15_14-30-05_auto.jsonl"));
    assert!(is_capture_file_name("2026-11-15_14-30-05_auto.wire.jsonl"));
    assert!(is_capture_file_name("2026-11-15_14-30-05_auto.help.json"));
    assert!(is_capture_file_name("2026-11-15_14-30-05_auto.sweep.json"));
    assert!(!is_capture_file_name(".2026_x.sweep.json.123.0.tmp"));
    assert!(!is_capture_file_name("../secret.jsonl"));
    assert!(!is_capture_file_name("a/b.jsonl"));
    assert!(!is_capture_file_name("a..jsonl"));
    assert!(!is_capture_file_name(".jsonl"));
    assert!(!is_capture_file_name("lockfile"));
    assert!(!is_capture_file_name("x.json"));
    assert!(!is_capture_file_name("CON.jsonl"));
    assert!(!is_capture_file_name("com1.help.json"));
}

fn headers(host: &str, origin: Option<&str>) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(header::HOST, HeaderValue::from_str(host).unwrap());
    if let Some(o) = origin {
        h.insert(header::ORIGIN, HeaderValue::from_str(o).unwrap());
    }
    h
}

#[test]
fn origin_guard() {
    let hosts = allowed_hosts(8790);
    assert!(same_origin(&headers("127.0.0.1:8790", None), &hosts));
    assert!(same_origin(
        &headers("localhost:8790", Some("http://localhost:8790")),
        &hosts
    ));
    assert!(!same_origin(
        &headers("127.0.0.1:8790", Some("https://evil.example")),
        &hosts
    ));
    assert!(!same_origin(
        &headers("127.0.0.1:8790", Some("http://127.0.0.1:9999")),
        &hosts
    ));
    // DNS rebinding: right IP, foreign host name.
    assert!(!same_origin(&headers("evil.example:8790", None), &hosts));
    assert!(!same_origin(&HeaderMap::new(), &hosts));
    // Port 80: browsers send the host without a port.
    let hosts80 = allowed_hosts(80);
    assert!(same_origin(
        &headers("127.0.0.1", Some("http://127.0.0.1")),
        &hosts80
    ));
    assert!(!same_origin(&headers("127.0.0.1", None), &hosts));
}
