# draftGPT

draftGPT is a read-only League of Legends champ select assistant: it follows the live draft from the League Client and suggests picks and bans.
It never picks, bans or hovers for you, and does nothing outside champ select.

There are two apps built on the same components:

- **Desktop panel** (Tauri): reads the draft live from the client and docks next to its window.
- **Web drafter**: the same view in a browser, with the draft entered by hand.

The stats in `data/stats/` are generated placeholders for now, so the suggestions are not real advice yet.

## Requirements

- Rust 1.85 or newer (edition 2024)
- Node.js with npm
- For the desktop panel: the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS (on Linux, WebKitGTK 4.1)
- For the web drafter: the wasm target and `wasm-bindgen-cli`, at the version in `Cargo.lock`:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130
```

The League Client runs only on Windows and macOS.
On Linux, the desktop panel works with recordings (`--replay`) only.

## Desktop panel

Build the page once, then start the app with a recording or with the live client:

```sh
npm --prefix ui install
npm --prefix ui run build -w @draftgpt/desktop

cargo run -p desktop -- --replay fixtures/16_22_draft_ban_and_pick_timeout_autolock.jsonl --speed 4
cargo run -p desktop                # live: start it with the League Client open
```

| Flag | Meaning |
| --- | --- |
| `--replay FILE` | play a recording (`.jsonl` from `fixtures/`) instead of connecting to the client |
| `--speed N` | replay speed: a factor such as `1`, `4` or `0.5`, or `instant` |
| `--float` | start floating instead of docked (docking is Windows-only; elsewhere it always floats) |
| `--stats FILE` | use this stats file instead of the bundled one |

For a release binary: `cargo build -p desktop --release` (the page must be built first; the build fails without it).

## Web drafter

```sh
npm --prefix ui install
npm --prefix ui run dev -w @draftgpt/web    # http://localhost:5174
```

Click a slot or ban spot, then choose a champion; suggestions update as you go.

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cd ui && npm run check && npm test
```

- Architecture and crate layout: [docs/arch.md](docs/arch.md)
- Project rules and all commands: [AGENTS.md](AGENTS.md)
- Recording drafts from the client (devserver) and turning them into fixtures: [crates/devserver/AGENTS.md](crates/devserver/AGENTS.md)
