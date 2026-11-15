//! Recording LCU events to redacted `.jsonl` files, with a raw wire file next
//! to each recording, a state sweep per client launch and a `/help` schema
//! dump per client version. The file format is defined in `lcu::recording`.

pub mod auto;
pub mod controller;
pub mod extras;
mod fsutil;
pub mod help_dump;
pub mod logging;
pub mod recorder;
pub mod redact;
pub mod sweep;

pub use controller::{CaptureError, Controller, Notice, Status};
pub use extras::RecordingExtras;
pub use recorder::RecordingInfo;
