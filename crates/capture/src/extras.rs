//! Background work for each new recording, needing the live client: the
//! state sweep (first, so it reflects the start of the recording), then the
//! `/help` dump (only when this client version has none yet). Both binaries
//! use it: `spawn` per new recording, `finish` on exit.

use std::time::Duration;

use lcu::LcuHttp;
use lcu::recording::sweep_path_for;
use tokio::task::JoinSet;

use crate::help_dump::HelpOutcome;
use crate::recorder::RecordingInfo;
use crate::{help_dump, sweep};

#[derive(Default)]
pub struct RecordingExtras {
    tasks: JoinSet<()>,
}

impl RecordingExtras {
    /// How long `finish` waits for unfinished work.
    pub const GRACE: Duration = Duration::from_secs(30);

    pub fn new() -> Self {
        Self::default()
    }

    /// Starts the sweep and `/help` dump for a recording. Needs a tokio runtime.
    pub fn spawn(&mut self, http: LcuHttp, recording: &RecordingInfo) {
        let sweep_dest = sweep_path_for(&recording.path);
        let recording_path = recording.path.clone();
        self.tasks.spawn(async move {
            match sweep::run(&http, &sweep_dest).await {
                Ok(s) => tracing::info!(
                    "saved state sweep to {} ({} ok, {} not found, {} failed)",
                    sweep_dest.display(),
                    s.ok,
                    s.not_found,
                    s.failed
                ),
                Err(e) => tracing::warn!("state sweep failed: {e}"),
            }
            match help_dump::dump_for_recording(&http, &recording_path).await {
                Ok(HelpOutcome::AlreadySaved(p)) => {
                    tracing::info!("/help for this client version: {}", p.display());
                }
                Ok(HelpOutcome::Saved(p)) => tracing::info!("saved /help to {}", p.display()),
                Ok(HelpOutcome::SavedForRecording { path, failed }) => tracing::warn!(
                    "saved /help to {} only ({failed} items failed or version unknown); \
                     the next recording retries",
                    path.display()
                ),
                Err(e) => tracing::warn!("/help dump failed: {e:#}"),
            }
        });
    }

    /// Waits up to [`Self::GRACE`] for running work; the rest is cancelled.
    pub async fn finish(self) {
        if self.tasks.is_empty() {
            return;
        }
        tracing::info!("waiting for state sweep and /help dump to finish…");
        if tokio::time::timeout(Self::GRACE, self.tasks.join_all())
            .await
            .is_err()
        {
            tracing::warn!("sweep or /help dump didn't finish in time; cancelled");
        }
    }
}
