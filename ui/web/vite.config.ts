import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig, type Connect, type Plugin } from "vite";

// The repo's recordings (fixtures/*.jsonl, redacted) for replay in the
// drafter: /recordings/index.json lists them, /recordings/<name>.jsonl serves
// one. Served by the dev and preview servers and copied into the build.
function recordings(): Plugin {
  const dir = join(import.meta.dirname, "../../fixtures");
  const names = () =>
    readdirSync(dir)
      .filter((f) => f.endsWith(".jsonl"))
      .map((f) => f.slice(0, -".jsonl".length))
      .sort();
  const serve: Connect.NextHandleFunction = (req, res, next) => {
    const path = (req.url ?? "").split("?")[0] ?? "";
    if (path.endsWith("/recordings/index.json")) {
      res.setHeader("Content-Type", "application/json");
      return void res.end(JSON.stringify(names()));
    }
    const m = /\/recordings\/([\w.-]+)\.jsonl$/.exec(path);
    if (!m?.[1] || !names().includes(m[1])) return next();
    res.setHeader("Content-Type", "text/plain; charset=utf-8");
    res.end(readFileSync(join(dir, `${m[1]}.jsonl`)));
  };
  return {
    name: "draftgpt-recordings",
    configureServer: (server) => void server.middlewares.use(serve),
    configurePreviewServer: (server) => void server.middlewares.use(serve),
    generateBundle() {
      const list = names();
      this.emitFile({ type: "asset", fileName: "recordings/index.json", source: JSON.stringify(list) });
      for (const n of list) {
        this.emitFile({ type: "asset", fileName: `recordings/${n}.jsonl`, source: readFileSync(join(dir, `${n}.jsonl`)) });
      }
    },
  };
}

// The repo's data/ directory is served as-is: /stats/index.json,
// /stats/<patch>.json. That's the mock of the static stats store.
export default defineConfig({
  plugins: [svelte(), recordings()],
  publicDir: "../../data",
  server: { port: 5174, strictPort: true, fs: { allow: ["..", "../../data"] } },
  preview: { port: 4174, strictPort: true },
});
