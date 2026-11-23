// draft-core compiled to wasm (crates/draft-wasm): the recommender and the
// recording replay.
import type { DraftState, Recommendations, Replay } from "@draftgpt/draft-ui";
import init, { Engine, replay } from "./wasm/draft_wasm.js";
import { loadStats } from "./stats";

export type Recommend = (draft: DraftState) => Recommendations;

let ready: Promise<unknown> | null = null;
const wasm = () => (ready ??= init());

export async function createRecommender(): Promise<Recommend> {
  await wasm();
  let engine: Engine | null = null;
  // Building the engine is the validation: only stats it accepts get cached.
  await loadStats((text) => {
    engine = new Engine(text);
  });
  const e = engine as Engine | null;
  if (!e) throw new Error("no stats loaded");
  return (draft) => JSON.parse(e.recommend(JSON.stringify(draft))) as Recommendations;
}

/** The drafts a recording (`.jsonl` text) goes through. Throws on a broken file. */
export async function replayRecording(text: string): Promise<Replay> {
  await wasm();
  return JSON.parse(replay(text)) as Replay;
}

/** Names of the repo's fixtures, served in dev and preview only (empty in a build). */
export async function listRecordings(base: string = import.meta.env.BASE_URL): Promise<string[]> {
  const res = await fetch(`${base}recordings/index.json`);
  if (!res.ok) return [];
  return (await res.json()) as string[];
}

export async function fetchRecording(name: string, base: string = import.meta.env.BASE_URL): Promise<string> {
  const res = await fetch(`${base}recordings/${encodeURIComponent(name)}.jsonl`);
  if (!res.ok) throw new Error(`${name}: HTTP ${res.status}`);
  return res.text();
}
