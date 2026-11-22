// Builds crates/draft-wasm for the browser into src/wasm/: cargo (release,
// wasm32) then wasm-bindgen (`cargo install wasm-bindgen-cli --version
// <the wasm-bindgen version in Cargo.lock>`). Asks cargo where its target
// directory is, so CARGO_TARGET_DIR and any OS work.
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const run = (cmd, args) => execFileSync(cmd, args, { cwd: here, stdio: ["ignore", "pipe", "inherit"] });

run("cargo", ["build", "-q", "-p", "draft-wasm", "--target", "wasm32-unknown-unknown", "--release"]);
const meta = JSON.parse(run("cargo", ["metadata", "--format-version", "1", "--no-deps"]).toString());
const wasm = join(meta.target_directory, "wasm32-unknown-unknown", "release", "draft_wasm.wasm");
run("wasm-bindgen", [wasm, "--target", "web", "--out-dir", join(here, "../src/wasm")]);
