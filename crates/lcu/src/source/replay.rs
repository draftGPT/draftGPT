//! A recording played back as an event source.

use std::path::Path;
use std::time::Duration;

use super::{ClientInfo, EventSource, SourceEvent, SourceStream};
use crate::raw::RawEvent;
use crate::recording::{self, Header, RecordingError};

/// How fast a [`Replay`] plays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Speed {
    /// Real time multiplied by this factor (`1.0` = as recorded).
    Times(f64),
    /// Every event at once, no waiting.
    Instant,
}

impl std::str::FromStr for Speed {
    type Err = String;

    /// `instant`, or a factor from 0.01 to 1000 such as `1`, `4` or `0.5` (an
    /// `x` suffix is allowed: `4x`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("instant") {
            return Ok(Self::Instant);
        }
        let n = s.strip_suffix(['x', 'X']).unwrap_or(s);
        match n.parse::<f64>() {
            Ok(f) if (Self::MIN..=Self::MAX).contains(&f) => Ok(Self::Times(f)),
            _ => Err(format!(
                "speed must be `instant` or a factor from {} to {}, got `{s}`",
                Self::MIN,
                Self::MAX
            )),
        }
    }
}

impl Speed {
    pub const MIN: f64 = 0.01;
    pub const MAX: f64 = 1000.0;

    /// When an event recorded at `t` ms plays, counted from the start of the
    /// replay; `None` for [`Speed::Instant`]. Saturates instead of panicking
    /// on extreme values (a `Times` built by hand can hold anything).
    fn delay(self, t: u64) -> Option<Duration> {
        let Self::Times(f) = self else { return None };
        #[allow(clippy::cast_precision_loss)] // t is milliseconds; precision loss is far off
        let secs = t as f64 / 1000.0 / f;
        Some(Duration::try_from_secs_f64(secs).unwrap_or(Duration::MAX))
    }
}

/// Plays a recording: [`SourceEvent::Connected`] with the recording's client
/// version, then its events at their recorded times (scaled by [`Speed`]),
/// then the stream ends. Events keep their recorded `t`.
pub struct Replay {
    pub header: Header,
    pub events: Vec<RawEvent>,
    pub speed: Speed,
}

impl Replay {
    /// Reads the whole recording up front, so a broken file fails here, not
    /// halfway through playback.
    pub fn open(path: &Path, speed: Speed) -> Result<Self, RecordingError> {
        let (header, events) = recording::read_file(path)?;
        Ok(Self {
            header,
            events,
            speed,
        })
    }

    fn client_info(&self) -> ClientInfo {
        ClientInfo {
            port: None,
            pid: None,
            version: self.header.client_version.clone(),
        }
    }
}

impl EventSource for Replay {
    fn into_stream(self: Box<Self>) -> SourceStream {
        let connected = SourceEvent::Connected(self.client_info());
        let speed = self.speed;
        let start = tokio::time::Instant::now();
        let events = futures::stream::unfold(self.events.into_iter(), move |mut it| async move {
            let ev = it.next()?;
            if let Some(delay) = speed.delay(ev.t) {
                match start.checked_add(delay) {
                    Some(at) => tokio::time::sleep_until(at).await,
                    // Further away than the clock can count: never comes.
                    None => std::future::pending().await,
                }
            }
            Some((SourceEvent::Event(ev), it))
        });
        Box::pin(futures::StreamExt::chain(
            futures::stream::iter([connected]),
            events,
        ))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/source/replay.rs"]
mod tests;
