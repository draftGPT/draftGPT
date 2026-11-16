//! Replays a recording through the tracker and recommender and prints every
//! change as JSON: `[{t, draft, recs}]`. The desktop UI's demo mode plays these
//! in a browser, without Tauri or a client.
//!
//! `cargo run -q -p draft-core --example updates -- fixtures/x.jsonl data/stats/16.22.json`

use draft_core::{Recommender, StatsRecommender, Tracker};
use lcu_model::recording::read_file;
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (Some(recording), Some(stats)) = (args.next(), args.next()) else {
        return Err("usage: updates <recording.jsonl> <stats.json>".into());
    };
    let (_, events) = read_file(recording.as_ref())?;
    let rec = StatsRecommender::from_json(&std::fs::read_to_string(stats)?)?;
    let mut tracker = Tracker::new();
    let mut out = Vec::new();
    let mut last = None;
    for ev in &events {
        if tracker.apply(ev) {
            let draft = tracker.state();
            if draft != last {
                let recs = draft.as_ref().map(|d| rec.recommend(d));
                out.push(json!({"t": ev.t, "draft": draft, "recs": recs}));
                last = draft;
            }
        }
    }
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}
