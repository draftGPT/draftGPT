//! `draftgpt`: the desktop side panel. Shows the live champ select (or a
//! replayed recording) with recommendations, docked next to the League client.
//! Read-only towards the client, like everything in this repo.

// No console window behind the panel in Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod dock;
mod engine;

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use draft_core::StatsRecommender;
use lcu::{EventSource, Live, Replay, Speed};
use tauri::{Emitter, Manager, State};

use engine::{Engine, Shared, Update};

/// The stats bundled into the app (mock data until the real store exists).
const BUNDLED_STATS: &str = include_str!("../../../data/stats/16.22.json");

#[derive(Parser)]
#[command(name = "draftgpt", about = "Champ select side panel (read-only)")]
struct Args {
    /// Play this recording (.jsonl) instead of connecting to the client.
    #[arg(long, value_name = "FILE")]
    replay: Option<PathBuf>,
    /// Replay speed: a factor such as 1, 4 or 0.5, or `instant`.
    #[arg(long, default_value = "1", requires = "replay")]
    speed: Speed,
    /// Stats file (data/stats/<patch>.json format) instead of the bundled one.
    #[arg(long, value_name = "FILE")]
    stats: Option<PathBuf>,
    /// Start floating instead of docked to the client window.
    #[arg(long)]
    float: bool,
}

fn main() {
    lcu::transport::tls::install_crypto_provider();
    let args = Args::parse();
    if let Err(e) = run(args) {
        eprintln!("draftgpt: {e}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let stats = match &args.stats {
        Some(p) => std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?,
        None => BUNDLED_STATS.to_string(),
    };
    let recommender = StatsRecommender::from_json(&stats).map_err(|e| format!("stats: {e}"))?;
    // Open the replay before the window, so a bad file fails right away.
    let replay = match &args.replay {
        Some(p) => Some((
            Replay::open(p, args.speed).map_err(|e| format!("{}: {e}", p.display()))?,
            name_of(p),
        )),
        None => None,
    };
    let floating = args.float || !cfg!(windows);
    let status = if replay.is_some() {
        "Loading replay…"
    } else {
        "Waiting for League client…"
    };
    let shared = Arc::new(Shared::new(floating, status));

    tauri::Builder::default()
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![latest, set_floating])
        .setup(move |app| {
            if let Ok(dir) = app.path().app_log_dir() {
                capture::logging::init(&dir, "draftgpt");
            }
            // `setup` runs outside Tokio; Live::spawn needs a runtime to spawn its task on.
            let runtime = tauri::async_runtime::handle();
            let _in_runtime = runtime.inner().enter();
            let (stream, name) = match replay {
                Some((r, name)) => (Box::new(r).into_stream(), Some(name)),
                None => (Box::new(Live::default()).into_stream(), None),
            };
            let handle = app.handle().clone();
            let engine = Engine::new(recommender, name);
            let for_engine = shared.clone();
            tauri::async_runtime::spawn(async move {
                engine
                    .run(stream, &for_engine, |u| {
                        if let Err(e) = handle.emit("draft-update", u) {
                            tracing::warn!("couldn't send update to the page: {e}");
                        }
                    })
                    .await;
            });
            dock::spawn(app.handle().clone(), shared.clone());
            Ok(())
        })
        .run(tauri::generate_context!())?;
    Ok(())
}

fn name_of(p: &std::path::Path) -> String {
    p.file_stem().map_or_else(
        || p.display().to_string(),
        |s| s.to_string_lossy().into_owned(),
    )
}

/// The current update, for a page that just loaded.
#[tauri::command]
fn latest(shared: State<'_, Arc<Shared>>) -> Update {
    shared.latest()
}

/// Floating on: the panel stays where the user puts it. Off: it docks to the
/// client window (Windows only; elsewhere it stays floating).
#[tauri::command]
fn set_floating(on: bool, app: tauri::AppHandle, shared: State<'_, Arc<Shared>>) {
    let on = on || !cfg!(windows);
    let u = shared.change(|u| u.floating = on);
    let _ = app.emit("draft-update", &u);
}

#[cfg(test)]
#[path = "../tests/unit/bin.rs"]
mod tests;
