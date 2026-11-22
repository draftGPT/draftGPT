//! HTTP and WebSocket routes for the dev page.

use std::path::Path;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as UrlPath, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use capture::{CaptureError, Status};
use lcu::recording::{EXTENSION, HELP_SUFFIX, SWEEP_SUFFIX};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::{AppState, Level, PageMsg};

const INDEX_HTML: &str = include_str!("../static/index.html");
const APP_JS: &str = include_str!("../static/app.js");

/// All routes, accepting only requests addressed to `127.0.0.1:<port>` or
/// `localhost:<port>` and coming from that origin (or with no origin, like
/// curl). This blocks other websites from driving the API (CSRF), reading it
/// through DNS rebinding, or opening the event WebSocket, which browsers don't
/// restrict by origin themselves.
pub fn router(state: Arc<AppState>, port: u16) -> Router {
    let hosts: Arc<[String]> = allowed_hosts(port).into();
    Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        .route(
            "/app.js",
            get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], APP_JS) }),
        )
        .route("/api/status", get(status))
        .route("/api/events", get(events))
        .route("/api/capture/start", post(start))
        .route("/api/capture/stop", post(stop))
        .route("/api/capture/auto", put(set_auto))
        .route("/api/captures", get(list_captures))
        .route("/api/captures/{file}", get(download_capture))
        .layer(middleware::from_fn(move |req: Request, next: Next| {
            let hosts = hosts.clone();
            async move {
                if same_origin(req.headers(), &hosts) {
                    next.run(req).await
                } else {
                    api_error(StatusCode::FORBIDDEN, "cross-origin request refused")
                }
            }
        }))
        .with_state(state)
}

/// Host header values for this server. Browsers leave out the default port.
fn allowed_hosts(port: u16) -> Vec<String> {
    let mut hosts = vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if port == 80 {
        hosts.extend(["127.0.0.1".to_string(), "localhost".to_string()]);
    }
    hosts
}

fn same_origin(headers: &HeaderMap, hosts: &[String]) -> bool {
    let host_ok = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| hosts.iter().any(|a| a == h));
    let origin_ok = match headers.get(header::ORIGIN) {
        None => true,
        Some(o) => o
            .to_str()
            .ok()
            .and_then(|o| o.strip_prefix("http://"))
            .is_some_and(|o| hosts.iter().any(|a| a == o)),
    };
    host_ok && origin_ok
}

async fn status(State(state): State<Arc<AppState>>) -> Json<Status> {
    Json(state.status())
}

async fn events(State(state): State<Arc<AppState>>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| push_to_page(socket, state))
}

async fn push_to_page(mut socket: WebSocket, state: Arc<AppState>) {
    let (initial, mut rx) = state.subscribe();
    for msg in initial {
        if send(&mut socket, &msg).await.is_err() {
            return;
        }
    }
    loop {
        tokio::select! {
            msg = rx.recv() => match msg {
                Ok(msg) => if send(&mut socket, &msg).await.is_err() { return },
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    let notice = PageMsg::Notice {
                        level: Level::Error,
                        message: format!("page fell behind, {n} messages skipped"),
                    };
                    if send(&mut socket, &notice).await.is_err() { return }
                }
                Err(broadcast::error::RecvError::Closed) => return,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(_)) => {} // the page never sends anything we need
                _ => return,
            },
        }
    }
}

async fn send(socket: &mut WebSocket, msg: &PageMsg) -> Result<(), axum::Error> {
    let text = serde_json::to_string(msg).expect("PageMsg serializes");
    socket.send(Message::text(text)).await
}

#[derive(Debug, Deserialize)]
struct StartBody {
    label: Option<String>,
}

#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
}

fn api_error(code: StatusCode, msg: impl Into<String>) -> Response {
    (code, Json(ApiError { error: msg.into() })).into_response()
}

async fn start(State(state): State<Arc<AppState>>, body: Option<Json<StartBody>>) -> Response {
    let label = body
        .and_then(|Json(b)| b.label)
        .unwrap_or_else(|| "manual".into());
    match state.start(&label) {
        Ok(info) => Json(info).into_response(),
        Err(e @ (CaptureError::AlreadyRecording(_) | CaptureError::Disabled(_))) => {
            api_error(StatusCode::CONFLICT, e.to_string())
        }
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn stop(State(state): State<Arc<AppState>>) -> Response {
    match state.stop() {
        Ok(Some(info)) => Json(info).into_response(),
        Ok(None) => api_error(StatusCode::NOT_FOUND, "not recording"),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Debug, Deserialize)]
struct AutoBody {
    on: bool,
}

async fn set_auto(State(state): State<Arc<AppState>>, Json(body): Json<AutoBody>) -> Json<Status> {
    Json(state.set_auto(body.on))
}

#[derive(Debug, Serialize)]
struct CaptureFile {
    pub file: String,
    pub bytes: u64,
    /// Seconds since the Unix epoch.
    pub modified: u64,
}

async fn list_captures(State(state): State<Arc<AppState>>) -> Response {
    // Blocking directory listing; a handful of files, so not worth spawn_blocking.
    match capture_files(&state.captures_dir()) {
        Ok(files) => Json(files).into_response(),
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// Recordings, wire files, sweeps and help dumps in `dir`, newest first. A missing
/// directory is an empty list (nothing recorded yet).
fn capture_files(dir: &Path) -> std::io::Result<Vec<CaptureFile>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry?;
        let file = entry.file_name().to_string_lossy().into_owned();
        if !is_capture_file_name(&file) {
            continue;
        }
        // Not `entry.metadata()`: on Windows that comes from the directory
        // listing, which doesn't update the size of a file another handle is
        // still writing (an active recording shows 0 bytes). This queries the
        // file itself.
        let meta = std::fs::metadata(entry.path())?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        files.push(CaptureFile {
            file,
            bytes: meta.len(),
            modified,
        });
    }
    files.sort_by(|a, b| b.modified.cmp(&a.modified).then(b.file.cmp(&a.file)));
    Ok(files)
}

/// Windows device names, which open a device instead of a file whatever the
/// extension (`CON.jsonl` is the console).
const RESERVED_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Only plain file names of the kind we write: no separators, no `..`, no
/// Windows device names.
fn is_capture_file_name(name: &str) -> bool {
    let safe_chars = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    let has_ext = name.ends_with(&format!(".{EXTENSION}"))
        || name.ends_with(HELP_SUFFIX)
        || name.ends_with(SWEEP_SUFFIX);
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    safe_chars
        && has_ext
        && !name.starts_with('.')
        && !name.contains("..")
        && !RESERVED_NAMES.contains(&stem.as_str())
}

async fn download_capture(
    State(state): State<Arc<AppState>>,
    UrlPath(file): UrlPath<String>,
) -> Response {
    if !is_capture_file_name(&file) {
        return api_error(StatusCode::BAD_REQUEST, "invalid file name");
    }
    let path = state.captures_dir().join(&file);
    let content_type = if file.ends_with(HELP_SUFFIX) || file.ends_with(SWEEP_SUFFIX) {
        "application/json"
    } else {
        "application/x-ndjson"
    };
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, content_type.to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{file}\""),
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            api_error(StatusCode::NOT_FOUND, "no such capture")
        }
        Err(e) => api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[cfg(test)]
mod tests {
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
}
