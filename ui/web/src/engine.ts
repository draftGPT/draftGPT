// draft-core's recommender, compiled to wasm (crates/draft-wasm).
import type { DraftState, Recommendations } from "@draftgpt/draft-ui";
import init, { Engine } from "./wasm/draft_wasm.js";
import { loadStats } from "./stats";

export type Recommend = (draft: DraftState) => Recommendations;

export async function createRecommender(): Promise<Recommend> {
  await init();
  let engine: Engine | null = null;
  // Building the engine is the validation: only stats it accepts get cached.
  await loadStats((text) => {
    engine = new Engine(text);
  });
  const e = engine as Engine | null;
  if (!e) throw new Error("no stats loaded");
  return (draft) => JSON.parse(e.recommend(JSON.stringify(draft))) as Recommendations;
}
