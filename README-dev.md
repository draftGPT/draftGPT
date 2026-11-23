# draftGPT development

How to install and run the apps is in [README.md](README.md).
Project rules and the full command list are in [AGENTS.md](AGENTS.md); architecture is in [docs/arch.md](docs/arch.md).

## Checks

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cd ui && npm run check && npm test
CHROMIUM_PATH=/usr/bin/chromium npm run e2e   # browser tests; or `npx playwright install chromium` once
```

Tests replay the recordings in `fixtures/`, so no League Client is needed.
`fixtures/` and `docs/lcu-spec/help/` are submodules of private repos (`draftGPT/fixtures`, `draftGPT/lcu-spec`): `git submodule update --init`.
Without them the fixture tests print `skipped: fixtures submodule not checked out` and pass.

## Release build

```sh
npm --prefix ui run build -w @draftgpt/desktop   # the page first; the release build fails without it
cargo build -p desktop --release
cargo build -p devserver --release
```

## Recording drafts (capture and devserver)

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

**Clash evenings** are recorded too, by both tools in auto mode: start one **before lock-in** and leave it running. It records each round from lock-in and scouting (the opponent team, the bracket, every team's roster) through champ select, stops when the game starts, and starts again for the next round. Files are named `<date>_<time>_clash.jsonl`. Team names and IDs are redacted like player names. Not yet tested in a live Clash: the runbook ("Clash weekends" in [crates/devserver/AGENTS.md](crates/devserver/AGENTS.md)) says how to check it's working and what to do if it isn't.

Recordings go to `draftgpt-captures` in the OS temp folder, never into the repo; both tools print the path when they start.

| OS | Folder |
| --- | --- |
| Windows | `%TEMP%\draftgpt-captures` |
| macOS | `$TMPDIR/draftgpt-captures` |
| Linux | `/tmp/draftgpt-captures` |

The temp folder gets emptied, so hand over the main `.jsonl` files right after the session (after Clash: all `*_clash.jsonl` and `capture.log`, plus your team's name and tag and the opponents' you saw, for the leak check).
**Don't copy recordings into `fixtures/` yourself:** only an agent does that.
How to run a capture session, fix it when it doesn't connect, and the agent's steps for fixtures: [crates/devserver/AGENTS.md](crates/devserver/AGENTS.md).
