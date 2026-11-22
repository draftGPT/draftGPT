use super::*;
use serde_json::json;

#[test]
fn empty_body_is_null() {
    assert_eq!(decode_body(b"").unwrap(), Value::Null);
    assert_eq!(decode_body(b" \r\n").unwrap(), Value::Null);
}

#[test]
fn json_body_parses() {
    assert_eq!(decode_body(br#"{"a":1}"#).unwrap(), json!({"a": 1}));
    assert_eq!(decode_body(b"null").unwrap(), Value::Null);
}

#[test]
fn non_json_body_errors_without_echoing_it() {
    let err = decode_body(b"secret-ish text").unwrap_err().to_string();
    assert!(err.contains("15 bytes"), "{err}");
    assert!(!err.contains("secret"), "{err}");
}
