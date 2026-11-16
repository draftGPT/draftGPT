import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

// The repo's data/ directory is served as-is: /stats/index.json,
// /stats/<patch>.json. That's the mock of the static stats store.
export default defineConfig({
  plugins: [svelte()],
  publicDir: "../../data",
  server: { port: 5174, strictPort: true, fs: { allow: ["..", "../../data"] } },
  preview: { port: 4174, strictPort: true },
});
