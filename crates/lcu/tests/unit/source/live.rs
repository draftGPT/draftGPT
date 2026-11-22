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
