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
    if !dir.join("README.md").exists() {
        eprintln!("skipped: fixtures submodule not checked out");
        return;
    }
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
