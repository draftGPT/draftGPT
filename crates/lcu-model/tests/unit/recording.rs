use super::*;
use std::path::PathBuf;

const SAMPLE: &str = concat!(
    r#"{"format_version":1,"client_version":"16.21","started_at":"2026-11-15T13:00:00Z","label":"x"}"#,
    "\n",
    r#"{"t":0,"uri":"/a","kind":"Snapshot","data":1}"#,
    "\n\n",
    r#"{"t":5,"uri":"/b","kind":"Update","data":{"k":true}}"#,
    "\n",
);

#[test]
fn reads_header_and_events() {
    let (header, events) = read(SAMPLE.as_bytes()).unwrap();
    assert_eq!(header.label, "x");
    let events: Vec<_> = events.collect::<Result<_, _>>().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].t, 5);
}

#[test]
fn reports_line_numbers() {
    let text = format!("{SAMPLE}{{not json\n");
    let (_, events) = read(text.as_bytes()).unwrap();
    let err = events.last().unwrap().unwrap_err();
    assert!(matches!(err, RecordingError::Json { line: 5, .. }), "{err}");
}

#[test]
fn reads_wire_lines() {
    let text =
        "{\"t\":1,\"frame\":[8]}\n\n{\"t\":2,\"unparsable\":{\"bytes\":3,\"error\":\"x\"}}\n";
    let lines: Vec<WireLine> = read_wire(text.as_bytes())
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1].unparsable.as_ref().unwrap().bytes, 3);
}

#[test]
fn empty_is_an_error() {
    assert!(matches!(read("".as_bytes()), Err(RecordingError::Empty)));
}

#[test]
fn newer_format_versions_are_refused() {
    let text = r#"{"format_version":99,"client_version":null,"started_at":"x","label":"l"}"#;
    assert!(matches!(
        read(text.as_bytes()),
        Err(RecordingError::UnsupportedVersion(99))
    ));
}

#[test]
fn a_byte_order_mark_is_ignored() {
    let text = "\u{feff}{\"format_version\":1,\"client_version\":null,\"started_at\":\"x\",\"label\":\"l\"}\n\
                    {\"t\":0,\"uri\":\"/x\",\"kind\":\"Snapshot\",\"data\":1}\n";
    let (h, events) = read(text.as_bytes()).unwrap();
    assert_eq!(h.label, "l");
    assert_eq!(events.count(), 1);
}

#[test]
fn sibling_paths() {
    let rec = Path::new("/r/2026_x.jsonl");
    assert_eq!(help_path_for(rec), PathBuf::from("/r/2026_x.help.json"));
    assert_eq!(wire_path_for(rec), PathBuf::from("/r/2026_x.wire.jsonl"));
    assert_eq!(sweep_path_for(rec), PathBuf::from("/r/2026_x.sweep.json"));
    assert_eq!(
        help_path_for_version(Path::new("/r"), "16.22.834.8910"),
        PathBuf::from("/r/client-16-22-834-8910.help.json")
    );
}

#[test]
fn wire_line_shapes() {
    let ok = WireLine {
        t: 1,
        frame: Some(serde_json::json!([8, "e", {}])),
        unparsable: None,
    };
    assert_eq!(
        serde_json::to_string(&ok).unwrap(),
        r#"{"t":1,"frame":[8,"e",{}]}"#
    );
    let bad = WireLine {
        t: 2,
        frame: None,
        unparsable: Some(Unparsable {
            bytes: 10,
            error: "eof".into(),
        }),
    };
    assert_eq!(
        serde_json::to_string(&bad).unwrap(),
        r#"{"t":2,"unparsable":{"bytes":10,"error":"eof"}}"#
    );
}
