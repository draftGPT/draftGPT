//! Background work for each new recording, needing the live client: the
//! state sweep (first, so it reflects the start of the recording), then the
//! `/help` dump. Both binaries use it: `spawn` per new recording, `finish` on
//! exit.

use std::path::PathBuf;
use std::time::Duration;

use lcu::LcuHttp;
use lcu::recording::{help_path_for, sweep_path_for};
use tokio::task::JoinSet;

use crate::recorder::RecordingInfo;
use crate::{help_dump, sweep};

pub struct RecordingExtras {
    captures: PathBuf,
    tasks: JoinSet<()>,
}

impl RecordingExtras {
    /// How long `finish` waits for unfinished work.
    pub const GRACE: Duration = Duration::from_secs(30);

    pub fn new(captures: impl Into<PathBuf>) -> Self {
        Self {
            captures: captures.into(),
            tasks: JoinSet::new(),
        }
    }

    /// Starts the sweep and `/help` dump for a recording. Needs a tokio runtime.
    pub fn spawn(&mut self, http: LcuHttp, recording: &RecordingInfo) {
        let sweep_dest = sweep_path_for(&recording.path);
        let help_dest = help_path_for(&recording.path);
        let captures = self.captures.clone();
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
            match help_dump::dump_for_recording(&http, &help_dest, &captures).await {
                Ok(()) => tracing::info!("saved /help to {}", help_dest.display()),
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
