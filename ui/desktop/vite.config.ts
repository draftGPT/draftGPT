import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

// Tauri loads http://localhost:1420 in development (crates/desktop/tauri.conf.json)
// and the built dist/ in release builds.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, fs: { allow: ["../.."] } },
  preview: { port: 4173, strictPort: true },
  build: { target: "es2022" },
});
