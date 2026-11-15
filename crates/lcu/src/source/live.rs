//! The live League client as an event source.

use std::collections::HashSet;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;

use super::{ClientInfo, EventSource, SourceEvent, SourceStream, WireFrame};
use crate::discovery::{self, Credentials, DiscoveryError};
use crate::raw::{EventKind, RawEvent};
use crate::transport::http::LcuHttp;
use crate::transport::ws;
use crate::{CORE_RESOURCES, Resource, WIRE_EVENTS};

/// The running League client. Finds it, connects, subscribes, emits a snapshot
/// of each core resource, then forwards events. Reconnects (and finds the
/// client again, since port and password change) whenever the connection drops.
pub struct Live {
    /// Resources emitted as [`SourceEvent::Event`].
    pub resources: Vec<Resource>,
    /// Also subscribe to every client event and emit each frame as
    /// [`SourceEvent::Wire`].
    pub wire: bool,
    pub retry: Duration,
    /// How to find the client. Tests replace it so they never touch a real one.
    pub discover: fn() -> Result<Credentials, DiscoveryError>,
}

impl Default for Live {
    fn default() -> Self {
        Self {
            resources: CORE_RESOURCES.to_vec(),
            wire: false,
            retry: Duration::from_secs(2),
            discover: discovery::discover,
        }
    }
}

/// Access to the live client while connected.
#[derive(Debug, Clone)]
pub struct LiveHandle {
    http: watch::Receiver<Option<LcuHttp>>,
}

impl LiveHandle {
    /// HTTP access to the client, `None` while disconnected. Set before the
    /// matching [`SourceEvent::Connected`] is emitted.
    pub fn http(&self) -> Option<LcuHttp> {
        self.http.borrow().clone()
    }
}

impl Live {
    /// Starts the connection task. It stops when the returned stream is dropped.
    pub fn spawn(self) -> (SourceStream, LiveHandle) {
        let (tx, mut rx) = mpsc::channel(256);
        let (http_tx, http_rx) = watch::channel(None);
        let task = AbortOnDrop(tokio::spawn(run_live(self, tx, http_tx)));
        let stream = futures::stream::poll_fn(move |cx| {
            let _keep_task_alive = &task;
            rx.poll_recv(cx)
        });
        (Box::pin(stream), LiveHandle { http: http_rx })
    }
}

impl EventSource for Live {
    fn into_stream(self: Box<Self>) -> SourceStream {
        self.spawn().0
    }
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Why a connection ended.
enum End {
    ConsumerGone,
    Lost { reason: String, was_connected: bool },
}

/// Decides which failures to report: each distinct reason once per outage, not
/// on every retry. A successful connection ends the outage, so the next failure
/// is reported even if its reason is the same as before.
#[derive(Debug, Default)]
struct Outage {
    last: Option<String>,
}

impl Outage {
    fn should_report(&mut self, reason: &str, was_connected: bool) -> bool {
        if was_connected {
            self.last = None;
        }
        if self.last.as_deref() == Some(reason) {
            return false;
        }
        self.last = Some(reason.to_owned());
        true
    }
}

async fn run_live(live: Live, tx: mpsc::Sender<SourceEvent>, http: watch::Sender<Option<LcuHttp>>) {
    let mut outage = Outage::default();
    loop {
        let end = tokio::select! {
            () = tx.closed() => End::ConsumerGone,
            end = connection(&live, &tx, &http) => end,
        };
        http.send_replace(None);
        let End::Lost {
            reason,
            was_connected,
        } = end
        else {
            return;
        };
        if outage.should_report(&reason, was_connected) {
            tracing::info!("LCU: {reason}");
            if tx.send(SourceEvent::Disconnected { reason }).await.is_err() {
                return;
            }
        }
        tokio::select! {
            () = tx.closed() => return,
            () = tokio::time::sleep(live.retry) => {}
        }
    }
}

/// One connection lifetime: discover, connect, subscribe, snapshot, forward.
async fn connection(
    live: &Live,
    tx: &mpsc::Sender<SourceEvent>,
    http_slot: &watch::Sender<Option<LcuHttp>>,
) -> End {
    let (http, socket, info) = match connect(live).await {
        Ok(c) => c,
        Err(reason) => {
            return End::Lost {
                reason,
                was_connected: false,
            };
        }
    };
    tracing::info!("LCU connected: {info:?}");
    http_slot.send_replace(Some(http.clone()));
    if tx.send(SourceEvent::Connected(info)).await.is_err() {
        return End::ConsumerGone;
    }
    match forward(live, tx, &http, socket).await {
        Forwarded::ConsumerGone => End::ConsumerGone,
        Forwarded::Lost(reason) => End::Lost {
            reason,
            was_connected: true,
        },
    }
}

/// Finds the client, connects and subscribes. Errors are human-readable reasons.
async fn connect(live: &Live) -> Result<(LcuHttp, ws::WsStream, ClientInfo), String> {
    // Process listing is blocking work; keep it off the async workers.
    let creds = tokio::task::spawn_blocking(live.discover)
        .await
        .map_err(|e| format!("discovery task failed: {e}"))?
        .map_err(|e| e.to_string())?;
    let http = LcuHttp::new(&creds).map_err(|e| e.to_string())?;
    let mut socket = ws::connect(&creds)
        .await
        .map_err(|e| format!("WebSocket connect failed: {e}"))?;
    let subscriptions: Vec<&str> = if live.wire {
        WIRE_EVENTS.to_vec()
    } else {
        live.resources.iter().map(|r| r.event).collect()
    };
    for sub in subscriptions {
        socket
            .send(Message::text(ws::subscribe_message(sub)))
            .await
            .map_err(|e| format!("subscribe failed: {e}"))?;
    }
    let version = http
        .client_version()
        .await
        .inspect_err(|e| tracing::warn!("couldn't read client version: {e}"))
        .ok();
    let info = ClientInfo {
        port: Some(creds.port),
        pid: creds.pid,
        version,
    };
    Ok((http, socket, info))
}

enum Forwarded {
    ConsumerGone,
    Lost(String),
}

/// Sends snapshots of the core resources, then forwards frames until the
/// socket closes.
async fn forward(
    live: &Live,
    tx: &mpsc::Sender<SourceEvent>,
    http: &LcuHttp,
    mut socket: ws::WsStream,
) -> Forwarded {
    // Current state, since events only arrive on change. An event already
    // buffered on the socket may be older than this snapshot; champ select
    // sessions update every second, so that self-corrects almost immediately.
    for r in &live.resources {
        match http.get_json(r.uri).await {
            Ok(Some(data)) => {
                let ev = RawEvent {
                    t: now_ms(),
                    uri: r.uri.to_string(),
                    kind: EventKind::Snapshot,
                    data,
                };
                if tx.send(SourceEvent::Event(ev)).await.is_err() {
                    return Forwarded::ConsumerGone;
                }
            }
            Ok(None) => {}
            Err(e) => tracing::warn!("snapshot of {} failed: {e}", r.uri),
        }
    }

    let core: HashSet<&str> = live.resources.iter().map(|r| r.uri).collect();
    while let Some(msg) = socket.next().await {
        let text = match msg {
            Ok(Message::Text(t)) => t,
            Ok(Message::Close(_)) => break,
            Ok(_) => continue,
            Err(e) => return Forwarded::Lost(format!("WebSocket error: {e}")),
        };
        let t = now_ms();
        if live.wire {
            let frame = WireFrame {
                t,
                text: text.to_string(),
            };
            if tx.send(SourceEvent::Wire(frame)).await.is_err() {
                return Forwarded::ConsumerGone;
            }
        }
        match ws::parse_frame(&text) {
            Ok(Some(w)) if core.contains(w.uri.as_str()) => {
                let ev = RawEvent {
                    t,
                    uri: w.uri,
                    kind: w.kind,
                    data: w.data,
                };
                if tx.send(SourceEvent::Event(ev)).await.is_err() {
                    return Forwarded::ConsumerGone;
                }
            }
            Ok(_) => {}
            // The frame may hold player data, so log its size, not its content.
            Err(e) => tracing::warn!("unparsable LCU frame ({} bytes): {e}", text.len()),
        }
    }
    Forwarded::Lost("connection closed".into())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dropping_live_stream_stops_its_task() {
        let live = Live {
            retry: Duration::from_millis(10),
            discover: || Err(DiscoveryError::NotFound),
            ..Live::default()
        };
        let (stream, handle) = live.spawn();
        assert!(handle.http().is_none());
        drop(stream);
        // The watch sender lives in the task; once the task is gone the
        // receiver sees the channel closed.
        let mut rx = handle.http.clone();
        let closed = async { while rx.changed().await.is_ok() {} };
        tokio::time::timeout(Duration::from_secs(5), closed)
            .await
            .expect("live task should stop when its stream is dropped");
    }

    #[test]
    fn outage_reports_each_reason_once_until_a_connection_succeeds() {
        let mut o = Outage::default();
        assert!(o.should_report("not running", false));
        assert!(!o.should_report("not running", false));
        assert!(o.should_report("TLS failed", false));
        // Connected, then closed: reported.
        assert!(o.should_report("connection closed", true));
        // Reconnected and closed again with the same reason: still reported.
        assert!(o.should_report("connection closed", true));
        // Retrying without connecting: not repeated.
        assert!(!o.should_report("connection closed", false));
    }

    #[tokio::test]
    async fn live_reports_discovery_failure_once() {
        let live = Live {
            retry: Duration::from_millis(5),
            discover: || Err(DiscoveryError::NotFound),
            ..Live::default()
        };
        let (mut stream, _handle) = live.spawn();
        let first = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap();
        assert!(
            matches!(first, Some(SourceEvent::Disconnected { .. })),
            "{first:?}"
        );
        // Several retries later, nothing more was reported.
        let more = tokio::time::timeout(Duration::from_millis(100), stream.next()).await;
        assert!(more.is_err(), "unexpected {more:?}");
    }
}
