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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommends_from_json() {
        let stats = include_str!("../../../data/stats/16.22.json");
        let engine = Engine::new(stats).unwrap();
        let draft = serde_json::to_string(&DraftState::default()).unwrap();
        let out: serde_json::Value =
            serde_json::from_str(&engine.recommend(&draft).unwrap()).unwrap();
        assert_eq!(out["patch"], "16.22");
        assert!(out["picks"].as_array().unwrap().len() > 1);
    }
}
