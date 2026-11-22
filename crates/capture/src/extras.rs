//! Background work for each new recording, needing the live client: the
//! state sweep (first, so it reflects the start of the recording; only for the
//! first recording of each client launch), then the `/help` dump (only when
//! this client version has none yet). Both binaries use it: `spawn` per new
//! recording, `finish` on exit.

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
    /// Client launch (its base URL, which has the per-launch port) the last
    /// sweep was taken for.
    swept_launch: Option<String>,
}

impl RecordingExtras {
    /// How long `finish` waits for unfinished work.
    pub const GRACE: Duration = Duration::from_secs(30);

    pub fn new() -> Self {
        Self::default()
    }

    /// Starts the sweep and `/help` dump for a recording. Needs a tokio runtime.
    pub fn spawn(&mut self, http: LcuHttp, recording: &RecordingInfo) {
        let full_sweep = self.claim_sweep(http.base_url());
        let dest = sweep_path_for(&recording.path);
        let recording_path = recording.path.clone();
        self.tasks.spawn(async move {
            match full_sweep {
                false => match sweep::run_clash_only(&http, &dest).await {
                    Ok(Some(s)) => tracing::info!(
                        "Clash queue: saved Clash endpoints to {} ({} ok, {} failed)",
                        dest.display(),
                        s.ok,
                        s.failed
                    ),
                    Ok(None) => tracing::info!("state sweep already taken this client launch"),
                    Err(e) => tracing::warn!("Clash sweep failed: {e}"),
                },
                true => match sweep::run(&http, &dest).await {
                    Ok(s) => tracing::info!(
                        "saved state sweep to {} ({} ok, {} not found, {} failed, {} skipped)",
                        dest.display(),
                        s.ok,
                        s.not_found,
                        s.failed,
                        s.skipped
                    ),
                    Err(e) => tracing::warn!("state sweep failed: {e}"),
                },
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

    /// True if `launch` hasn't been swept yet (and marks it swept).
    fn claim_sweep(&mut self, launch: &str) -> bool {
        if self.swept_launch.as_deref() == Some(launch) {
            return false;
        }
        self.swept_launch = Some(launch.to_owned());
        true
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

#[cfg(test)]
#[path = "../tests/unit/extras.rs"]
mod tests;
