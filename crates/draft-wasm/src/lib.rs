//! `draft-core` for the browser. JSON strings in and out, typed on the
//! TypeScript side by the types generated from draft-core.

use draft_core::{DraftState, Recommender, StatsRecommender};
use wasm_bindgen::prelude::*;

/// A recommender for one patch's stats.
#[wasm_bindgen]
pub struct Engine(StatsRecommender);

#[wasm_bindgen]
impl Engine {
    /// `stats`: the text of a `data/stats/<patch>.json` file.
    #[wasm_bindgen(constructor)]
    pub fn new(stats: &str) -> Result<Engine, JsError> {
        Ok(Self(StatsRecommender::from_json(stats)?))
    }

    /// `draft`: a `DraftState` as JSON. Returns `Recommendations` as JSON.
    pub fn recommend(&self, draft: &str) -> Result<String, JsError> {
        let draft: DraftState = serde_json::from_str(draft)?;
        Ok(serde_json::to_string(&self.0.recommend(&draft))?)
    }
}

/// `recording`: the text of a recording (`.jsonl`). Returns a `Replay` as JSON:
/// the drafts it goes through, for stepping.
#[wasm_bindgen]
pub fn replay(recording: &str) -> Result<String, JsError> {
    Ok(serde_json::to_string(&draft_core::replay(recording)?)?)
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
