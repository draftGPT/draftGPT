//! The LCU WebSocket (WAMP 1.0 style messages).
//!
//! - subscribe:  `[5, "<event name>"]`
//! - event:      `[8, "<event name>", {"uri": ..., "eventType": ..., "data": ...}]`

use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

use super::TransportError;
use super::tls::client_config;
use crate::discovery::Credentials;
use crate::raw::EventKind;

pub type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

const OPCODE_SUBSCRIBE: u8 = 5;
const OPCODE_EVENT: u8 = 8;

const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub async fn connect(creds: &Credentials) -> Result<WsStream, TransportError> {
    let mut request = format!("wss://127.0.0.1:{}/", creds.port).into_client_request()?;
    let auth = HeaderValue::from_str(&creds.auth_header())
        .map_err(|e| TransportError::Unexpected(e.to_string()))?;
    request.headers_mut().insert("Authorization", auth);
    let connector = Connector::Rustls(client_config()?);
    let connecting =
        tokio_tungstenite::connect_async_tls_with_config(request, None, false, Some(connector));
    let (stream, _resp) = tokio::time::timeout(CONNECT_TIMEOUT, connecting)
        .await
        .map_err(|_| TransportError::Timeout("WebSocket connect"))??;
    Ok(stream)
}

pub fn subscribe_message(event: &str) -> String {
    serde_json::json!([OPCODE_SUBSCRIBE, event]).to_string()
}

#[derive(Debug, Clone, PartialEq)]
pub struct WireEvent {
    pub uri: String,
    pub kind: EventKind,
    pub data: Value,
}

/// Parses one text frame. `Ok(None)` for frames that aren't events (the LCU
/// sends an empty frame after a subscribe, for example).
pub fn parse_frame(text: &str) -> Result<Option<WireEvent>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    let Some(arr) = v.as_array() else {
        return Err("frame is not an array".into());
    };
    if arr.first().and_then(Value::as_u64) != Some(u64::from(OPCODE_EVENT)) {
        return Ok(None);
    }
    if !arr.get(1).is_some_and(Value::is_string) {
        return Err("event frame without name".into());
    }
    let payload = arr.get(2).ok_or("event frame without payload")?;
    let uri = payload
        .get("uri")
        .and_then(Value::as_str)
        .ok_or("payload without uri")?
        .to_string();
    let kind = match payload.get("eventType").and_then(Value::as_str) {
        Some("Create") => EventKind::Create,
        Some("Update") => EventKind::Update,
        Some("Delete") => EventKind::Delete,
        other => return Err(format!("unknown eventType {other:?}")),
    };
    let data = payload.get("data").cloned().unwrap_or(Value::Null);
    Ok(Some(WireEvent { uri, kind, data }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `OnJsonApiEvent_lol-champ-select_v1_session` → `/lol-champ-select/v1/session`.
    /// Returns `None` for names that aren't per-resource JSON API events.
    fn event_to_uri(event: &str) -> Option<String> {
        let rest = event.strip_prefix("OnJsonApiEvent_")?;
        if rest.is_empty() {
            return None;
        }
        Some(format!("/{}", rest.replace('_', "/")))
    }

    #[test]
    fn subscribe_format() {
        assert_eq!(
            subscribe_message("OnJsonApiEvent_lol-champ-select_v1_session"),
            r#"[5,"OnJsonApiEvent_lol-champ-select_v1_session"]"#
        );
    }

    #[test]
    fn core_resources_are_consistent() {
        // Each resource's event delivers its URI: the event maps to the URI
        // itself or to a prefix of it.
        for r in crate::CORE_RESOURCES {
            let prefix = event_to_uri(r.event).unwrap();
            assert!(r.uri.starts_with(&prefix), "{r:?}");
        }
    }

    #[test]
    fn event_names_map_to_uris() {
        assert_eq!(
            event_to_uri("OnJsonApiEvent_lol-champ-select_v1_session").as_deref(),
            Some("/lol-champ-select/v1/session")
        );
        assert_eq!(
            event_to_uri("OnJsonApiEvent_lol-gameflow_v1_gameflow-phase").as_deref(),
            Some("/lol-gameflow/v1/gameflow-phase")
        );
        assert_eq!(event_to_uri("OnJsonApiEvent"), None);
        assert_eq!(event_to_uri("OnJsonApiEvent_"), None);
        assert_eq!(event_to_uri("OnLog"), None);
    }

    #[test]
    fn parses_event_frame() {
        let frame = r#"[8,"OnJsonApiEvent_lol-gameflow_v1_gameflow-phase",{"data":"ChampSelect","eventType":"Update","uri":"/lol-gameflow/v1/gameflow-phase"}]"#;
        let ev = parse_frame(frame).unwrap().unwrap();
        assert_eq!(ev.uri, "/lol-gameflow/v1/gameflow-phase");
        assert_eq!(ev.kind, EventKind::Update);
        assert_eq!(ev.data, Value::String("ChampSelect".into()));
    }

    #[test]
    fn delete_frame_without_data() {
        let frame = r#"[8,"OnJsonApiEvent_lol-champ-select_v1_session",{"eventType":"Delete","uri":"/lol-champ-select/v1/session"}]"#;
        let ev = parse_frame(frame).unwrap().unwrap();
        assert_eq!(ev.kind, EventKind::Delete);
        assert_eq!(ev.data, Value::Null);
    }

    #[test]
    fn ignores_non_events() {
        assert_eq!(parse_frame(""), Ok(None));
        assert_eq!(parse_frame(r#"[0,"session-id",1,"server"]"#), Ok(None));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_frame("{not json").is_err());
        assert!(parse_frame(r#"{"a":1}"#).is_err());
        assert!(parse_frame(r#"[8,"x",{"eventType":"Update"}]"#).is_err());
        assert!(parse_frame(r#"[8,"x",{"uri":"/a","eventType":"Weird"}]"#).is_err());
    }
}
