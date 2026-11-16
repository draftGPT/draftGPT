// draft-core's recommender, compiled to wasm (crates/draft-wasm).
import type { DraftState, Recommendations } from "@draftgpt/draft-ui";
import init, { Engine } from "./wasm/draft_wasm.js";
import { loadStats } from "./stats";

export type Recommend = (draft: DraftState) => Recommendations;

export async function createRecommender(base = ""): Promise<Recommend> {
  const [{ text }] = await Promise.all([loadStats(base), init()]);
  const engine = new Engine(text);
  return (draft) => JSON.parse(engine.recommend(JSON.stringify(draft))) as Recommendations;
}
