//! `capture`: records champ select from the running League client without the
//! dev page. `devserver` does the same with a UI. Also converts RCVolus
//! recordings into fixtures.

use std::path::{Path, PathBuf};

use capture::{Controller, Notice, RecordingExtras, help_dump, rcvolus};
use clap::{ArgAction, Parser, Subcommand};
use futures::StreamExt;
use lcu::{Live, SourceEvent};

#[derive(Parser)]
#[command(about = "Record League client champ select events to redacted .jsonl files")]
struct Args {
    /// Directory for recordings and the log file.
    #[arg(long, global = true, default_value = "captures")]
    out: PathBuf,
    /// Also record every raw client frame to <name>.wire.jsonl (local only).
    #[arg(long, global = true, default_value_t = true, action = ArgAction::Set)]
    wire: bool,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Record every champ select until Ctrl+C (default).
    Auto,
    /// Record everything from now until Ctrl+C.
    Record {
        #[arg(long, default_value = "manual")]
        label: String,
    },
    /// Save the client's full /help schema (about 10000 requests) and exit.
    HelpDump {
        /// Output file [default: <out>/help-full.json].
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Convert an RCVolus/lol-pick-ban-ui recording (.json) into a redacted
    /// recording (.jsonl), then exit. Needs no client; never overwrites OUTPUT.
    ConvertRcvolus { input: PathBuf, output: PathBuf },
}

enum Mode {
    Auto,
    Manual { label: String },
}

#[tokio::main]
async fn main() {
    lcu::transport::tls::install_crypto_provider();
    let args = Args::parse();
    if let Some(Cmd::ConvertRcvolus { input, output }) = &args.cmd {
        if let Err(e) = convert_rcvolus(input, output) {
            eprintln!("capture: {e}");
            std::process::exit(1);
        }
        return;
    }
    capture::logging::init(&args.out, "capture");
    let result = match args.cmd.unwrap_or(Cmd::Auto) {
        Cmd::HelpDump { file } => {
            let file = file.unwrap_or_else(|| args.out.join("help-full.json"));
            full_help(&file).await
        }
        Cmd::Auto => {
            record(&args.out, args.wire, Mode::Auto).await;
            Ok(())
        }
        Cmd::Record { label } => {
            record(&args.out, args.wire, Mode::Manual { label }).await;
            Ok(())
        }
        Cmd::ConvertRcvolus { .. } => unreachable!("handled before logging starts"),
    };
    if let Err(e) = result {
        tracing::error!("{e:#}");
        std::process::exit(1);
    }
}

async fn record(out: &Path, wire: bool, mode: Mode) {
    let mut controller = Controller::new(out, matches!(mode, Mode::Auto));
    let (mut stream, live) = Live {
        wire,
        ..Live::default()
    }
    .spawn();
    let mut manual_label = match mode {
        Mode::Manual { label } => Some(label),
        Mode::Auto => None,
    };
    let mut extras = RecordingExtras::new();
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);
    tracing::info!("writing recordings to {}", out.display());
    loop {
        tokio::select! {
            ev = stream.next() => {
                let Some(ev) = ev else { break };
                let mut notices = controller.handle(&ev);
                // A manual recording starts once connected, so its first
                // events are the snapshots sent on connect.
                if matches!(ev, SourceEvent::Connected(_)) && let Some(label) = manual_label.take() {
                    notices.push(match controller.start(&label) {
                        Ok(info) => Notice::Started(info),
                        Err(e) => Notice::Error(e.to_string()),
                    });
                }
                for n in notices {
                    match &n {
                        Notice::Started(info) => {
                            if let Some(http) = live.http() {
                                extras.spawn(http, info);
                            }
                        }
                        // Started/stopped are already logged by the controller.
                        Notice::Stopped(_) => {}
                        Notice::Error(_) => tracing::error!("{n}"),
                    }
                }
            }
            _ = &mut ctrl_c => break,
        }
    }
    if let Err(e) = controller.stop() {
        tracing::error!("finishing recording failed: {e}");
    }
    extras.finish().await;
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

fn convert_rcvolus(input: &Path, output: &Path) -> Result<(), BoxError> {
    let text =
        std::fs::read_to_string(input).map_err(|e| format!("reading {}: {e}", input.display()))?;
    let (header, events) =
        rcvolus::convert(&text).map_err(|e| format!("{}: {e}", input.display()))?;
    // Never replace an existing file (such as a fixture) by accident.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|e| format!("creating {}: {e}", output.display()))?;
    std::io::Write::write_all(&mut file, rcvolus::to_jsonl(&header, &events).as_bytes())
        .map_err(|e| format!("writing {}: {e}", output.display()))?;
    println!("{} events → {}", events.len(), output.display());
    Ok(())
}

async fn full_help(file: &Path) -> Result<(), BoxError> {
    let creds = tokio::task::spawn_blocking(lcu::discovery::discover).await??;
    let http = lcu::LcuHttp::new(&creds)?;
    let version = http.client_version().await.ok();
    tracing::info!("fetching full /help schema, this takes a while…");
    let stats = help_dump::dump(&http, file, help_dump::Scope::Full, version.as_deref()).await?;
    tracing::info!(
        "saved {} items ({} failed) to {}",
        stats.items,
        stats.failed,
        file.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Args::command().debug_assert();
    }

    #[test]
    fn documented_invocations_parse() {
        for argv in [
            vec!["capture"],
            vec!["capture", "--out", "c", "auto"],
            vec!["capture", "record", "--label", "x", "--out", "c"],
            vec!["capture", "--wire", "false", "auto"],
            vec!["capture", "help-dump", "--file", "f.json"],
            vec!["capture", "convert-rcvolus", "in.json", "out.jsonl"],
        ] {
            Args::try_parse_from(&argv).unwrap_or_else(|e| panic!("{argv:?}: {e}"));
        }
    }
}
