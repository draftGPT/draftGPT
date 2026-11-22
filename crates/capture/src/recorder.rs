//! Writes recordings in the format defined in `lcu::recording`.
//!
//! The recorder owns redaction and the clock, so every file it writes is
//! redacted and the main file and wire file share one timeline, whoever uses
//! it. It never parses or types event data. Each line is flushed as it's
//! written, so a crash loses at most the line in progress. Writes are blocking;
//! at the LCU's event rate that's fine even from async code.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use lcu::recording::{EXTENSION, FORMAT_VERSION, Header, Unparsable, WireLine, wire_path_for};
use lcu::{EventKind, RawEvent, WireFrame};
use serde::Serialize;

use crate::redact::Redactor;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecordingInfo {
    pub name: String,
    pub path: PathBuf,
    pub started_at: String,
    pub events: usize,
}

/// A JSON-lines file, flushed after every line.
struct JsonlWriter {
    out: BufWriter<File>,
}

impl JsonlWriter {
    fn create_new(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        Ok(Self {
            out: BufWriter::new(file),
        })
    }

    fn write<T: Serialize>(&mut self, value: &T) -> io::Result<()> {
        serde_json::to_writer(&mut self.out, value)?;
        self.out.write_all(b"\n")?;
        self.out.flush()
    }
}

/// Opened on the first wire frame, so recordings without wire capture don't
/// get an empty wire file.
enum Wire {
    NotOpened,
    Open {
        out: JsonlWriter,
        frames: usize,
    },
    /// Writing failed once; the main recording carries on without it.
    Failed,
}

pub struct Recorder {
    out: JsonlWriter,
    wire: Wire,
    info: RecordingInfo,
    /// One redactor for both files, so fakes match across them.
    redactor: Redactor,
    /// Source time that is `t = 0`: the first live event or frame written.
    start_t: Option<u64>,
    /// Latest `t` in the main file. Events can arrive stamped earlier (a Clash
    /// fetch round stamps each response but delivers them together); they're
    /// written at this time instead, so times never go backwards.
    last_t: u64,
}

impl Recorder {
    /// Creates `<dir>/<timestamp>_<label>.jsonl` and writes the header.
    pub fn create(dir: &Path, label: &str, client_version: Option<String>) -> io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let now = jiff::Zoned::now();
        let label = slug(label);
        let base = format!("{}_{label}", now.strftime("%Y-%m-%d_%H-%M-%S"));
        let (name, mut out) = create_unique(dir, &base)?;
        let header = Header {
            format_version: FORMAT_VERSION,
            client_version,
            started_at: now.timestamp().to_string(),
            label,
        };
        out.write(&header)?;
        Ok(Self {
            out,
            wire: Wire::NotOpened,
            info: RecordingInfo {
                path: dir.join(format!("{name}.{EXTENSION}")),
                name,
                started_at: header.started_at,
                events: 0,
            },
            redactor: Redactor::new(),
            start_t: None,
            last_t: 0,
        })
    }

    /// Writes the current state of a resource at `t = 0`, as a `Snapshot`.
    pub fn write_snapshot(&mut self, mut event: RawEvent) -> io::Result<()> {
        event.kind = EventKind::Snapshot;
        event.t = 0;
        self.write_redacted(event)
    }

    /// Writes a live event, redacted, with `t` relative to the recording start
    /// (never before the previous event's).
    pub fn write(&mut self, mut event: RawEvent) -> io::Result<()> {
        self.last_t = self.relative(event.t).max(self.last_t);
        event.t = self.last_t;
        self.write_redacted(event)
    }

    /// Writes a raw frame to the wire file, redacted (or, if it isn't JSON,
    /// only its size and the error). After a write error the wire file is
    /// abandoned: that error is returned once, later frames are dropped.
    pub fn write_wire(&mut self, frame: &WireFrame) -> io::Result<()> {
        let t = self.relative(frame.t);
        self.write_wire_at(t, &frame.text)
    }

    /// Writes a frame from before the recording started (the controller's
    /// preroll) at `t = 0`. It doesn't start the clock: otherwise the main
    /// file's first live event would sit seconds after its snapshots (12.5 s
    /// in a live capture), the age of the oldest preroll frame. Line order
    /// still gives the sequence.
    pub fn write_preroll_wire(&mut self, frame: &WireFrame) -> io::Result<()> {
        self.write_wire_at(0, &frame.text)
    }

    fn write_wire_at(&mut self, t: u64, text: &str) -> io::Result<()> {
        if matches!(self.wire, Wire::NotOpened) {
            self.wire = match JsonlWriter::create_new(&wire_path_for(&self.info.path)) {
                Ok(out) => Wire::Open { out, frames: 0 },
                Err(e) => {
                    self.wire = Wire::Failed;
                    return Err(e);
                }
            };
        }
        if matches!(self.wire, Wire::Failed) {
            return Ok(());
        }
        let line = WireLine {
            t,
            ..self.wire_line(text)
        };
        let Wire::Open { out, frames } = &mut self.wire else {
            return Ok(());
        };
        match out.write(&line) {
            Ok(()) => {
                *frames += 1;
                Ok(())
            }
            Err(e) => {
                self.wire = Wire::Failed;
                Err(e)
            }
        }
    }

    pub fn info(&self) -> &RecordingInfo {
        &self.info
    }

    pub fn wire_frames(&self) -> usize {
        match &self.wire {
            Wire::Open { frames, .. } => *frames,
            Wire::NotOpened | Wire::Failed => 0,
        }
    }

    pub fn finish(mut self) -> io::Result<RecordingInfo> {
        self.out.out.flush()?;
        Ok(self.info)
    }

    fn write_redacted(&mut self, mut event: RawEvent) -> io::Result<()> {
        // Rules by the original URI, then its ID segments (Clash roster IDs).
        self.redactor.redact_at(&event.uri, &mut event.data);
        event.uri = self.redactor.redact_uri(&event.uri);
        self.out.write(&event)?;
        self.info.events += 1;
        Ok(())
    }

    /// A wire line without its time (filled in by the caller).
    fn wire_line(&mut self, text: &str) -> WireLine {
        match serde_json::from_str(text) {
            Ok(mut frame) => {
                self.redactor.redact_frame(&mut frame);
                WireLine {
                    t: 0,
                    frame: Some(frame),
                    unparsable: None,
                }
            }
            Err(e) => WireLine {
                t: 0,
                frame: None,
                unparsable: Some(Unparsable {
                    bytes: text.len(),
                    error: e.to_string(),
                }),
            },
        }
    }

    fn relative(&mut self, t: u64) -> u64 {
        t.saturating_sub(*self.start_t.get_or_insert(t))
    }
}

fn create_unique(dir: &Path, base: &str) -> io::Result<(String, JsonlWriter)> {
    for n in 1.. {
        let name = if n == 1 {
            base.to_string()
        } else {
            format!("{base}-{n}")
        };
        match JsonlWriter::create_new(&dir.join(format!("{name}.{EXTENSION}"))) {
            Ok(w) => return Ok((name, w)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    unreachable!("an unbounded range always yields a name")
}

/// Lowercase letters, digits, `-` and `_`; anything else becomes `-`.
pub fn slug(label: &str) -> String {
    let s: String = label
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "capture".into() } else { s }
}

#[cfg(test)]
#[path = "../tests/unit/recorder.rs"]
mod tests;
