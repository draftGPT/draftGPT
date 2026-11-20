//! Recording LCU events to redacted `.jsonl` files, with a raw wire file next
//! to each recording, a state sweep per client launch and a `/help` schema
//! dump per client version. The file format is defined in `lcu::recording`.

pub mod auto;
pub mod clash;
pub mod controller;
pub mod extras;
mod fsutil;
pub mod help_dump;
pub mod logging;
pub mod rcvolus;
pub mod recorder;
pub mod redact;
pub mod sweep;

pub use controller::{CaptureError, Controller, Notice, Status};
pub use extras::RecordingExtras;
pub use recorder::RecordingInfo;

/// Where recordings and logs go by default: `draftgpt-captures` in the OS temp
/// directory (`/tmp` on Linux, `$TMPDIR` on macOS, `%TEMP%` on Windows), never
/// inside the repo. Only an agent copies recordings into `fixtures/`.
pub fn default_out_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("draftgpt-captures")
}
