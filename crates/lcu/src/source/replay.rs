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
        assert_eq!("0.01".parse(), Ok(Speed::Times(0.01)));
        assert_eq!("1000x".parse(), Ok(Speed::Times(1000.0)));
        for bad in [
            "0", "-1", "fast", "inf", "NaN", "", "1e-30", "5e-324", "0.001", "1001", "1e309",
        ] {
            assert!(bad.parse::<Speed>().is_err(), "{bad}");
        }
    }

    #[test]
    fn extreme_speeds_saturate_instead_of_panicking() {
        assert_eq!(Speed::Times(1e-300).delay(u64::MAX), Some(Duration::MAX));
        assert_eq!(
            Speed::Times(f64::MIN_POSITIVE).delay(1),
            Some(Duration::MAX)
        );
        assert_eq!(Speed::Times(1e300).delay(5), Some(Duration::ZERO));
        assert_eq!(Speed::Instant.delay(5), None);
        assert_eq!(
            Speed::Times(2.0).delay(3_000),
            Some(Duration::from_millis(1_500))
        );
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
        assert!(n > 0);
    }
}
