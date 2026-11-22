import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig, type Connect, type Plugin } from "vite";

// Demo files (`npm run demo`) for the browser demo mode, served at /demo/ by
// the dev and preview servers only: they're not in public/, so the built
// dist/ that Tauri embeds doesn't contain them.
function demoFiles(): Plugin {
  const dir = join(import.meta.dirname, "demo");
  const serve: Connect.NextHandleFunction = (req, res, next) => {
    const m = /^\/demo\/([\w-]+\.json)$/.exec((req.url ?? "").split("?")[0] ?? "");
    const file = m?.[1] && join(dir, m[1]);
    if (!file || !existsSync(file)) return next();
    res.setHeader("Content-Type", "application/json");
    res.end(readFileSync(file));
  };
  return {
    name: "draftgpt-demo-files",
    configureServer: (server) => void server.middlewares.use(serve),
    configurePreviewServer: (server) => void server.middlewares.use(serve),
  };
}

// Tauri loads http://localhost:1420 in development (crates/desktop/tauri.conf.json)
// and the built dist/ in release builds.
export default defineConfig({
  plugins: [svelte(), demoFiles()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, fs: { allow: ["..", "../../data"] } },
  preview: { port: 4173, strictPort: true },
  build: { target: "es2022" },
});
