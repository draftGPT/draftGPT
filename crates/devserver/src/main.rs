//! `devserver`: capture test tool. Connects to the running League client,
//! records champ selects, and serves a dev page at http://127.0.0.1:<port>.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use clap::{ArgAction, Parser};
use lcu::Live;

#[derive(Parser)]
#[command(about = "Dev page for testing LCU capture (binds to 127.0.0.1 only)")]
struct Args {
    #[arg(long, default_value_t = 8790)]
    port: u16,
    /// Directory for recordings and the log file.
    #[arg(long, default_value = "captures")]
    out: PathBuf,
    /// Record every champ select automatically.
    #[arg(long, default_value_t = true, action = ArgAction::Set)]
    auto: bool,
    /// Also record every raw client frame to <name>.wire.jsonl (local only).
    #[arg(long, default_value_t = true, action = ArgAction::Set)]
    wire: bool,
}

#[tokio::main]
async fn main() {
    lcu::transport::tls::install_crypto_provider();
    let args = Args::parse();
    capture::logging::init(&args.out, "devserver");

    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, args.port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("can't listen on {addr}: {e} (try --port)");
            std::process::exit(1);
        }
    };

    let (stream, live) = Live {
        wire: args.wire,
        ..Live::default()
    }
    .spawn();
    let state = devserver::AppState::new(&args.out, args.auto, Some(live));
    let source_task = tokio::spawn(devserver::run_source(state.clone(), stream));

    println!(
        "devserver: open http://{addr}  (recordings in {})",
        args.out.display()
    );
    let server = axum::serve(listener, devserver::router(state.clone(), args.port));
    if let Err(e) = server
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
    {
        tracing::error!("server error: {e}");
    }
    // Stop the source first so no new recording starts while shutting down.
    source_task.abort();
    let _ = source_task.await;
    state.shutdown().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Args::command().debug_assert();
    }
}
