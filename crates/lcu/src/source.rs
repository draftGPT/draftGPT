//! Event sources. Every input (live client, replayed recording) produces the same
//! stream of [`SourceEvent`]s. Live-only capabilities, like HTTP access to the
//! client, live on the source's own handle, not in the events.

mod live;
mod replay;

use std::pin::Pin;

use futures::Stream;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::raw::RawEvent;

pub use live::{Live, LiveHandle};
pub use replay::{Replay, Speed};

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ClientInfo {
    pub port: Option<u16>,
    pub pid: Option<u32>,
    pub version: Option<String>,
}

/// A WebSocket text frame exactly as the client sent it, before any parsing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WireFrame {
    /// Milliseconds on the source's clock, like [`RawEvent::t`].
    pub t: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SourceEvent {
    Connected(ClientInfo),
    Disconnected {
        reason: String,
    },
    /// A core resource event (see [`Live::resources`]).
    Event(RawEvent),
    /// Every frame received, only when wire capture is on.
    Wire(WireFrame),
}

pub type SourceStream = Pin<Box<dyn Stream<Item = SourceEvent> + Send>>;

pub trait EventSource: Send {
    fn into_stream(self: Box<Self>) -> SourceStream;
}

/// A source fed by hand through a channel. Used in tests, and by anything that
/// produces events itself.
pub struct ChannelSource {
    rx: mpsc::Receiver<SourceEvent>,
}

impl ChannelSource {
    pub fn new(buffer: usize) -> (mpsc::Sender<SourceEvent>, Self) {
        let (tx, rx) = mpsc::channel(buffer);
        (tx, Self { rx })
    }
}

impl EventSource for ChannelSource {
    fn into_stream(self: Box<Self>) -> SourceStream {
        let mut rx = self.rx;
        Box::pin(futures::stream::poll_fn(move |cx| rx.poll_recv(cx)))
    }
}

#[cfg(test)]
#[path = "../tests/unit/source.rs"]
mod tests;
