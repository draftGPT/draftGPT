//! Runs the real router on a random port, feeds it events through a
//! `ChannelSource`, and drives it over HTTP and WebSocket like the page does.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use capture::redact::find_unredacted;
use devserver::AppState;
use futures::StreamExt;
use lcu::{ChannelSource, ClientInfo, EventKind, EventSource, RawEvent, SourceEvent, WireFrame};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

struct Harness {
    base: String,
    tx: mpsc::Sender<SourceEvent>,
    http: reqwest::Client,
    clock: AtomicU64,
    _dir: tempfile::TempDir,
}

async fn start(auto: bool) -> Harness {
    serve(|dir| AppState::new(dir, auto, None)).await
}

async fn serve(make: impl FnOnce(&std::path::Path) -> Arc<AppState>) -> Harness {
    lcu::transport::tls::install_crypto_provider();
    let dir = tempfile::tempdir().unwrap();
    let state = make(dir.path());
    let (tx, source) = ChannelSource::new(64);
    tokio::spawn(devserver::run_source(
        state.clone(),
        Box::new(source).into_stream(),
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, devserver::router(state, addr.port()))
            .await
            .unwrap();
    });
    Harness {
        base: format!("http://{addr}"),
        tx,
        http: reqwest::Client::new(),
        clock: AtomicU64::new(1_000_000),
        _dir: dir,
    }
}

impl Harness {
    fn next_t(&self) -> u64 {
        self.clock.fetch_add(100, Ordering::Relaxed)
    }

    async fn send(&self, ev: SourceEvent) {
        self.tx.send(ev).await.unwrap();
    }

    async fn event(&self, uri: &str, data: Value) {
        let t = self.next_t();
        self.send(SourceEvent::Event(RawEvent {
            t,
            uri: uri.into(),
            kind: EventKind::Update,
            data,
        }))
        .await;
    }

    async fn get(&self, path: &str) -> reqwest::Response {
        self.http
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap()
    }

    async fn post(&self, path: &str, body: Option<Value>) -> reqwest::Response {
        let req = self.http.post(format!("{}{path}", self.base));
        let req = match body {
            Some(b) => req.json(&b),
            None => req,
        };
        req.send().await.unwrap()
    }

    async fn status(&self) -> Value {
        self.get("/api/status").await.json().await.unwrap()
    }

    /// Polls status until `pred` holds (events are processed asynchronously).
    async fn wait_status(&self, pred: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..100 {
            let s = self.status().await;
            if pred(&s) {
                return s;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("status never matched; last: {}", self.status().await);
    }

    fn ws_url(&self) -> String {
        format!("{}/api/events", self.base.replace("http", "ws"))
    }
}

fn connected() -> SourceEvent {
    SourceEvent::Connected(ClientInfo {
        port: Some(50000),
        pid: Some(1),
        version: Some("16.21.829.8620".into()),
    })
}

fn session() -> Value {
    json!({
        "timer": {"phase": "BAN_PICK", "adjustedTimeLeftInPhase": 20000},
        "localPlayerCellId": 0,
        "myTeam": [{"cellId": 0, "championPickIntent": 103, "assignedPosition": "middle",
                    "summonerId": 98_765_432_101_u64, "puuid": "real-puuid", "gameName": "RealName"}],
        "actions": [[{"id": 1, "actorCellId": 0, "type": "pick", "isInProgress": true}]],
        "chatDetails": {"multiUserChatPassword": "pw"}
    })
}

#[tokio::test]
async fn serves_page_and_assets() {
    let h = start(true).await;
    let page = h.get("/").await.text().await.unwrap();
    assert!(page.contains("draftGPT devserver"));
    let js = h.get("/app.js").await;
    assert_eq!(js.headers()["content-type"], "text/javascript");
}

#[tokio::test]
async fn refuses_cross_origin_requests() {
    let h = start(false).await;
    let resp = h
        .http
        .post(format!("{}/api/capture/start", h.base))
        .header("Origin", "https://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
    assert!(h.status().await["recording"].is_null());

    let resp = h
        .http
        .get(format!("{}/api/status", h.base))
        .header("Host", "evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);

    let mut req = h.ws_url().into_client_request().unwrap();
    req.headers_mut()
        .insert("Origin", "https://evil.example".parse().unwrap());
    assert!(tokio_tungstenite::connect_async(req).await.is_err());
}

#[tokio::test]
async fn auto_recording_end_to_end() {
    let h = start(true).await;
    assert_eq!(h.status().await["connection"]["state"], "waiting");

    h.send(connected()).await;
    h.event(lcu::GAMEFLOW_PHASE_URI, json!("ChampSelect")).await;
    h.event(lcu::SESSION_URI, session()).await;
    h.send(SourceEvent::Wire(WireFrame {
        t: h.next_t(),
        text: r#"[8,"OnJsonApiEvent",{"uri":"/lol-lobby/v2/lobby","eventType":"Update","data":{"puuid":"x"}}]"#.into(),
    }))
    .await;
    let s = h
        .wait_status(|s| s["recording"]["wire_frames"].as_u64() == Some(1))
        .await;
    assert_eq!(s["connection"]["state"], "connected");
    assert_eq!(s["connection"]["client"]["version"], "16.21.829.8620");
    assert_eq!(s["phase"], "ChampSelect");
    assert_eq!(s["recording"]["by_auto"], true);
    assert_eq!(s["recording"]["events"], 2);

    // Manual start conflicts with the auto recording.
    let resp = h
        .post("/api/capture/start", Some(json!({"label": "x"})))
        .await;
    assert_eq!(resp.status(), 409);

    h.event(lcu::GAMEFLOW_PHASE_URI, json!("GameStart")).await;
    h.wait_status(|s| s["recording"].is_null()).await;

    let files: Vec<Value> = h.get("/api/captures").await.json().await.unwrap();
    let names: Vec<&str> = files.iter().map(|f| f["file"].as_str().unwrap()).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    let main = names.iter().find(|n| n.ends_with("_auto.jsonl")).unwrap();
    let wire = names
        .iter()
        .find(|n| n.ends_with("_auto.wire.jsonl"))
        .unwrap();

    let resp = h.get(&format!("/api/captures/{main}")).await;
    assert_eq!(resp.headers()["content-type"], "application/x-ndjson");
    let body = resp.text().await.unwrap();
    let lines: Vec<Value> = body
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["client_version"], "16.21.829.8620");
    let events = &lines[1..];
    assert_eq!(events.len(), 3, "snapshot phase, session, phase change");
    for ev in events {
        assert_eq!(find_unredacted(&ev["data"]), Vec::<String>::new(), "{ev}");
    }
    assert_eq!(events[1]["data"]["myTeam"][0]["championPickIntent"], 103);

    let wire_body = h
        .get(&format!("/api/captures/{wire}"))
        .await
        .text()
        .await
        .unwrap();
    let frame: Value = serde_json::from_str(wire_body.lines().next().unwrap()).unwrap();
    assert_eq!(find_unredacted(&frame), Vec::<String>::new(), "{frame}");
}

#[tokio::test]
async fn manual_recording_and_auto_toggle() {
    let h = start(false).await;
    assert_eq!(h.post("/api/capture/stop", None).await.status(), 404);

    let started: Value = h
        .post(
            "/api/capture/start",
            Some(json!({"label": "Blind Pick test"})),
        )
        .await
        .json()
        .await
        .unwrap();
    assert!(
        started["name"]
            .as_str()
            .unwrap()
            .ends_with("_blind-pick-test")
    );

    let stopped: Value = h
        .post("/api/capture/stop", None)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(stopped["events"], 0);

    let s: Value = h
        .http
        .put(format!("{}/api/capture/auto", h.base))
        .json(&json!({"on": true}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(s["auto"], true);
}

#[tokio::test]
async fn download_rejects_bad_names() {
    let h = start(false).await;
    assert_eq!(
        h.get("/api/captures/..%2F..%2Fetc.jsonl").await.status(),
        400
    );
    assert_eq!(h.get("/api/captures/lockfile").await.status(), 400);
    assert_eq!(h.get("/api/captures/CON.jsonl").await.status(), 400);
    assert_eq!(h.get("/api/captures/missing.jsonl").await.status(), 404);
}

#[tokio::test]
async fn websocket_sends_redacted_state_then_live_events() {
    let h = start(false).await;
    // The channel is FIFO: once status shows "connected", the session sent
    // before it is already stored as latest.
    h.event(lcu::SESSION_URI, session()).await;
    h.send(connected()).await;
    h.wait_status(|s| s["connection"]["state"] == "connected")
        .await;

    let (mut ws, _) = tokio_tungstenite::connect_async(h.ws_url()).await.unwrap();
    let mut next = async || -> Value {
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(2), ws.next())
                .await
                .expect("websocket message")
                .unwrap()
                .unwrap();
            if let Message::Text(t) = msg {
                return serde_json::from_str(&t).unwrap();
            }
        }
    };

    // On connect: status, then the latest state of each resource, redacted.
    assert_eq!(next().await["type"], "status");
    let snapshot = next().await;
    assert_eq!(snapshot["type"], "event");
    assert_eq!(snapshot["event"]["uri"], lcu::SESSION_URI);
    assert!(snapshot["problem"].is_null());
    assert_eq!(
        find_unredacted(&snapshot["event"]["data"]),
        Vec::<String>::new()
    );
    assert_eq!(
        snapshot["event"]["data"]["myTeam"][0]["championPickIntent"],
        103
    );

    // Then live events, with problems flagged.
    h.event(
        lcu::SESSION_URI,
        json!({"timer": {"phase": "SOMETHING_NEW"}}),
    )
    .await;
    let live = loop {
        let m = next().await;
        if m["type"] == "event" {
            break m;
        }
    };
    let problem = live["problem"].as_str().unwrap();
    assert!(problem.contains("timer.phase"), "{problem}");
}

#[tokio::test]
async fn replay_state_refuses_every_kind_of_recording() {
    let h = serve(|dir| AppState::replay(dir)).await;
    h.send(connected()).await;
    let res = h
        .post("/api/capture/start", Some(json!({"label": "x"})))
        .await;
    assert_eq!(res.status(), reqwest::StatusCode::CONFLICT);
    let res = h
        .http
        .put(format!("{}/api/capture/auto", h.base))
        .json(&json!({"on": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    h.event(lcu::GAMEFLOW_PHASE_URI, json!("ChampSelect")).await;
    let s = h.wait_status(|s| s["phase"] == "ChampSelect").await;
    assert_eq!(s["auto"], json!(false));
    assert!(s["recording"].is_null());
    assert_eq!(s["recording_disabled"], json!("replaying a recording"));
    assert_eq!(std::fs::read_dir(h._dir.path()).unwrap().count(), 0);
}
