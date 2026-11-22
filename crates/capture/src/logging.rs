//! Logging for the capture binaries: the console, plus a log file in the
//! captures directory, so problems on the Windows machine can be looked at
//! later along with the recordings.

use std::fs::OpenOptions;
use std::path::Path;
use std::sync::Mutex;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Libraries whose debug/trace output contains secrets or raw client data:
/// tungstenite traces the handshake request (with the LCU password in the
/// `Authorization` header) and every received message. Pinned to `warn`
/// whatever `RUST_LOG` says; their info output isn't useful here either.
const CAPPED_TARGETS: &[&str] = &[
    "tungstenite",
    "tokio_tungstenite",
    "rustls",
    "hyper",
    "hyper_util",
    "reqwest",
];

/// Logs to stderr and appends to `<dir>/<name>.log`. Level comes from
/// `RUST_LOG` (default `info`). Reports where it logs on stderr; if the file
/// can't be opened, logs to the console only.
pub fn init(dir: &Path, name: &str) {
    let mut filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    for target in CAPPED_TARGETS {
        filter = filter.add_directive(format!("{target}=warn").parse().expect("valid directive"));
    }
    let path = dir.join(format!("{name}.log"));
    let file = std::fs::create_dir_all(dir)
        .and_then(|()| OpenOptions::new().create(true).append(true).open(&path));
    let file_layer = match file {
        Ok(f) => {
            eprintln!("logging to {}", path.display());
            Some(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(Mutex::new(f)),
            )
        }
        Err(e) => {
            eprintln!(
                "can't open log file {}: {e}; logging to console only",
                path.display()
            );
            None
        }
    };
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(file_layer)
        .init();
}
