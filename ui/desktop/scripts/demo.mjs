// Writes demo/<fixture>.json: the updates the desktop app would emit while
// replaying a recording, for the browser demo mode (`?demo=<fixture>`) and the
// end-to-end tests. vite.config.ts serves them in dev and preview only, so
// they never end up in the app bundle.
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "../../..");
const out = join(here, "../demo");
const fixtures = [
  "16_22_draft_ban_and_pick_timeout_autolock",
  "16_22_aram_bench_swaps",
  "rcvolus_2019_tournament_draft_spectated",
];
mkdirSync(out, { recursive: true });
for (const f of fixtures) {
  const json = execFileSync(
    "cargo",
    ["run", "-q", "-p", "draft-core", "--example", "updates", "--", `fixtures/${f}.jsonl`, "data/stats/16.22.json"],
    { cwd: repo, maxBuffer: 64 << 20 },
  );
  writeFileSync(join(out, `${f}.json`), json);
  console.log(`wrote demo/${f}.json`);
}
