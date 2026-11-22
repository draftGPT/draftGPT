use super::*;
use crate::raw::EventKind;
use futures::StreamExt;

#[tokio::test]
async fn channel_source_forwards_events() {
    let (tx, src) = ChannelSource::new(8);
    let mut stream = Box::new(src).into_stream();
    tx.send(SourceEvent::Event(RawEvent {
        t: 1,
        uri: "/x".into(),
        kind: EventKind::Update,
        data: serde_json::json!(1),
    }))
    .await
    .unwrap();
    drop(tx);
    match stream.next().await {
        Some(SourceEvent::Event(ev)) => assert_eq!(ev.uri, "/x"),
        other => panic!("unexpected {other:?}"),
    }
    assert!(stream.next().await.is_none());
}

#[test]
fn source_events_serialize_tagged() {
    let ev = SourceEvent::Disconnected { reason: "x".into() };
    assert_eq!(
        serde_json::to_string(&ev).unwrap(),
        r#"{"type":"disconnected","reason":"x"}"#
    );
}
