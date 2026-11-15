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

    /// `instant`, or a positive factor such as `1`, `4` or `0.5` (an `x` suffix
    /// is allowed: `4x`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("instant") {
            return Ok(Self::Instant);
        }
        let n = s.strip_suffix(['x', 'X']).unwrap_or(s);
        match n.parse::<f64>() {
            Ok(f) if f.is_finite() && f > 0.0 => Ok(Self::Times(f)),
            _ => Err(format!(
                "speed must be `instant` or a positive number, got `{s}`"
            )),
        }
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
            if let Speed::Times(f) = speed {
                #[allow(clippy::cast_precision_loss)]
                // t is milliseconds; precision loss is far off
                let at = Duration::from_secs_f64(ev.t as f64 / 1000.0 / f);
                tokio::time::sleep_until(start + at).await;
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
mod tests {
    use super::*;
    use crate::raw::EventKind;
    use futures::StreamExt;
    use serde_json::json;

    fn replay(ts: &[u64], speed: Speed) -> Box<Replay> {
        Box::new(Replay {
            header: Header {
                format_version: recording::FORMAT_VERSION,
                client_version: Some("16.22.1".into()),
                started_at: "2026-11-15T00:00:00Z".into(),
                label: "t".into(),
            },
            events: ts
                .iter()
                .map(|&t| RawEvent {
                    t,
                    uri: "/x".into(),
                    kind: EventKind::Update,
                    data: json!(t),
                })
                .collect(),
            speed,
        })
    }

    #[test]
    fn parses_speeds() {
        assert_eq!("instant".parse(), Ok(Speed::Instant));
        assert_eq!("4".parse(), Ok(Speed::Times(4.0)));
        assert_eq!("4x".parse(), Ok(Speed::Times(4.0)));
        assert_eq!("0.5".parse(), Ok(Speed::Times(0.5)));
        for bad in ["0", "-1", "fast", "inf", "NaN", ""] {
            assert!(bad.parse::<Speed>().is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn instant_emits_connected_then_every_event_then_ends() {
        let got: Vec<_> = replay(&[0, 5_000, 60_000], Speed::Instant)
            .into_stream()
            .collect()
            .await;
        assert_eq!(got.len(), 4);
        match &got[0] {
            SourceEvent::Connected(c) => assert_eq!(c.version.as_deref(), Some("16.22.1")),
            other => panic!("{other:?}"),
        }
        let ts: Vec<u64> = got[1..]
            .iter()
            .map(|e| match e {
                SourceEvent::Event(ev) => ev.t,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(ts, [0, 5_000, 60_000]);
    }

    #[tokio::test(start_paused = true)]
    async fn times_waits_scaled_by_speed() {
        let start = tokio::time::Instant::now();
        let mut s = replay(&[0, 4_000, 10_000], Speed::Times(2.0)).into_stream();
        let mut at = Vec::new();
        while let Some(ev) = s.next().await {
            if let SourceEvent::Event(_) = ev {
                at.push(start.elapsed().as_millis());
            }
        }
        assert_eq!(at, [0, 2_000, 5_000]);
    }

    #[test]
    fn every_fixture_opens() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let mut n = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == recording::EXTENSION) {
                let r = Replay::open(&path, Speed::Instant).unwrap();
                assert!(!r.events.is_empty(), "{}", path.display());
                n += 1;
            }
        }
        assert!(n >= 7);
    }
}
