# draftGPT

draftGPT is a read-only League of Legends champ select assistant: it follows the live draft from the League Client and suggests picks and bans.

There are two apps built on the same components:

- **Desktop panel** (Tauri): reads the draft live from the client and docks next to its window.
- **Web drafter**: the same view in a browser, with the draft entered by hand.

The stats in `data/stats/` are generated placeholders for now, so the suggestions are not real advice yet.

## Where it runs

| | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Desktop panel | yes, docks next to the client | yes, floating window | replays only (no League Client) |
| Web drafter | yes | yes | yes |

## Install

Both apps need Rust (1.85 or newer, via rustup) and Node.js 22 or newer with npm.

**Windows**

1. Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the "Desktop development with C++" workload (Rust and Tauri need the MSVC compiler).
2. Install Rust and Node.js, from [rustup.rs](https://rustup.rs) and [nodejs.org](https://nodejs.org), or with winget:

```powershell
winget install Rustlang.Rustup
winget install OpenJS.NodeJS.LTS
```

The desktop panel also needs WebView2, which Windows 10 and 11 already include.

**macOS**

```sh
xcode-select --install                                          # compiler, needed by Rust and Tauri
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh  # Rust
brew install node                                               # or the installer from nodejs.org
```

**Linux**

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh  # Rust
```

Install Node.js from your distribution's packages or [nodejs.org](https://nodejs.org).
To build the desktop panel for replays, also install the [Tauri Linux packages](https://v2.tauri.app/start/prerequisites/#linux) (WebKitGTK 4.1).

**Web drafter, all systems:** add the wasm target and `wasm-bindgen-cli` at the version in `Cargo.lock`:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130
```

Then, from the repo root, install the UI packages once:

```sh
npm --prefix ui install
```

## Desktop panel

Build the page once, then start the app with the live client or with a recording:

```sh
npm --prefix ui run build -w @draftgpt/desktop

cargo run -p desktop                # live: Windows or macOS, with the League Client open
cargo run -p desktop -- --replay fixtures/16_22_draft_ban_and_pick_timeout_autolock.jsonl --speed 4
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
npm --prefix ui run dev -w @draftgpt/web    # http://localhost:5174
```

Click a slot or ban spot, then choose a champion; suggestions update as you go.

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cd ui && npm run check && npm test
```

Tests replay the recordings in `fixtures/`, so no League Client is needed.

### Recording drafts (capture and devserver)

New fixtures come from recording real champ selects on Windows or macOS.
Everything written is redacted (player names, IDs, chat credentials), and the client is only ever read.

- **devserver**: a local page that shows what the client sends and records each champ select.

  ```sh
  cargo run -p devserver --release    # with the League Client open, then open http://127.0.0.1:8790
  cargo run -p devserver -- --replay fixtures/16_22_draft_missed_ban.jsonl --speed 4   # any OS, no client
  ```

  Options: `--port N`, `--out DIR` (default: the temp folder below), `--auto false` (record only when you press Start), `--wire false` (no raw frame file).

- **capture**: the same recorder without the page.

  ```sh
  cargo run -p capture                                # record every champ select until Ctrl+C
  cargo run -p capture -- record --label ranked_solo  # record everything from now until Ctrl+C
  ```

Recordings go to `draftgpt-captures` in the OS temp folder, never into the repo; both tools print the path when they start.

| OS | Folder |
| --- | --- |
| Windows | `%TEMP%\draftgpt-captures` |
| macOS | `$TMPDIR/draftgpt-captures` |
| Linux | `/tmp/draftgpt-captures` |

The temp folder gets emptied, so hand over the main `.jsonl` files right after the session (never `*.wire.jsonl` or `*.sweep.json`).
**Don't copy recordings into `fixtures/` yourself:** only an agent does that.
How to run a capture session, fix it when it doesn't connect, and the agent's steps for fixtures: [crates/devserver/AGENTS.md](crates/devserver/AGENTS.md).
