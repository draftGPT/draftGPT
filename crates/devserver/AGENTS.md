# devserver: capture sessions

How to run the devserver on the machine with the League client, what to do when capture
doesn't work, and how to turn a session's recordings into fixtures in git. The rules in the
root [AGENTS.md](../../AGENTS.md) still apply, above all: read-only towards the client, and
nothing identifying gets committed.

## Running it

The League client runs on Windows and macOS. Build on that machine, not cross-compiled.

1. Install Rust with [rustup](https://rustup.rs) (stable).
   - Windows: the default MSVC toolchain. rustup offers the Visual Studio Build Tools if
     they're missing; accept them.
   - macOS: Xcode command line tools (`xcode-select --install`).
2. Clone the repo and check out the branch you were asked to test (usually `master`).
3. Start the League client and log in.
4. Run it, from the repo root:

   ```sh
   cargo run -p devserver --release            # http://127.0.0.1:8790
   ```

   Options: `--port N`, `--out DIR` (default `captures/`), `--auto false` (only record when
   you press Start), `--wire false` (no raw frame file).
5. Open `http://127.0.0.1:8790`. The top line should say "League client connected" with the
   client version.
6. Play. Auto mode records every champ select by itself. Press Start before queueing to
   also get the gameflow and matchmaking events before champ select (lobby data itself only
   goes to the wire file), Stop to end a manual recording.
7. Watch the page for red problem flags: values our types don't know yet. They don't stop
   anything; write down what you did at that moment.
8. Stop with Ctrl+C. It finishes the open recording and waits briefly for the background
   dumps.

To replay a recording instead (no client needed, any OS):
`cargo run -p devserver -- --replay fixtures/16_22_draft_missed_ban.jsonl --speed 4`
(`--speed` from 0.01 to 1000, or `instant`). Nothing is recorded during a replay; the page
disables its recording controls.

## When it doesn't work

Everything is logged to `captures/devserver.log`. For more detail, start it with
`RUST_LOG=debug` (cmd: `set RUST_LOG=debug`, PowerShell: `$env:RUST_LOG="debug"`). The
client password never reaches the log; keep it that way if you add logging.

| Symptom | Cause and fix |
|---|---|
| Red dot, page says *League client: League client not running (no LeagueClientUx process and no lockfile found)* | Discovery reads the `LeagueClientUx` process arguments, then a `lockfile` in the install directory. Built-in paths: `C:\Riot Games\League of Legends` and `/Applications/League of Legends.app/Contents/LoL`. Elsewhere, set `LEAGUE_DIR` to the folder that contains `lockfile` while the client runs. Code: `crates/lcu/src/discovery.rs`. |
| *malformed lockfile* | A lockfile left by a crashed client, or a new format. Restart the client. The format is `name:pid:port:password:protocol`. |
| TLS or certificate errors in the log | The client's certificate no longer chains to Riot's root in `crates/lcu/certs/riotgames.pem`. To keep recording, set `DRAFTGPT_LCU_INSECURE_TLS=1` (skips the check; the connection is loopback-only). Save the log lines for a proper fix in `crates/lcu/src/transport/tls.rs`. |
| *can't listen on 127.0.0.1:8790* | Port taken: `--port 8791`. |
| Connected, but no recording starts | Auto mode starts on gameflow phase `ChampSelect`. Check the phase shown on the page; if it never changes, press Start to record by hand and note it. |
| Red problem flags | Not a failure. Keep recording; the recording is still complete, only our typed view is behind. Fixing the types (`crates/lcu-model/src/types.rs`) can wait. |
| Panic mentioning a crypto provider | A binary forgot `lcu::transport::tls::install_crypto_provider()` at the top of `main`. |
| `/help` dump or sweep failed | Logged, but the recording itself is unaffected. |
| Build fails on macOS in `ring` | Install the Xcode command line tools, then `cargo clean -p ring` and rebuild. |

### Hotfixing on the capture machine

When a code change is needed to get recording at all:

- Keep it as small as possible, on a branch (`fix/<what>`), and run
  `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`
  before committing.
- Never make the client do anything to get past a problem: GET requests and WebSocket
  subscriptions only (the root AGENTS.md lists the one exception). No new endpoints in the
  sweep without checking `SWEEP_EXCLUDE` in `tools/lcu_spec.py`.
- Never weaken redaction to get a recording through. If redaction gets in the way, record
  anyway and fix it afterwards; unredacted files stay local.
- Say in the commit or the hand-off what was only checked on this OS. Platform-specific code
  goes behind `#[cfg(...)]` with the other platforms unchanged.

## After the session: recordings to fixtures

`captures/` holds, per recording `<date>_<time>_<label>`:

| File | What happens to it |
|---|---|
| `<name>.jsonl` | The recording, redacted on write. Candidates for `fixtures/`. |
| `<name>.wire.jsonl` | Every raw client frame. **Never commit.** Keep locally (or hand over privately) for debugging. |
| `<name>.sweep.json` | Client state at the start. **Never commit.** Same as wire. |
| `client-<version>.help.json` | The client's schema, one per client build, with dashes for dots (`client-16-22-834-8910.help.json`). Commit if `docs/lcu-spec/help/` doesn't have that version yet. |
| `<name>.help.json` | Only when the version was unknown. Normally not needed. |
| `devserver.log` | Not committed. Keep it with the wire files if something went wrong. |

Wire and sweep files are gitignored, but don't copy them anywhere under the repo by hand.

Steps:

1. **Pick the recordings worth keeping**: one per situation not yet in `fixtures/README.md`
   (for example teammate hovers, a trade, a ranked draft, a dodge by someone else). Skip
   duplicates of situations already covered.
2. **Copy and rename** each into `fixtures/` as `<client>_<situation>.jsonl`: `<client>` is
   major and minor from the header's `client_version` (`16.23.x.y` → `16_23`), `<situation>`
   says what happens, in snake case (`16_23_ranked_teammate_trade.jsonl`). Copy, don't edit
   the content.
3. **Copy a new schema dump** `client-<version>.help.json` into `docs/lcu-spec/help/`.
4. **Check for leaks yourself**, besides the tests: search the new files for your Riot ID,
   summoner name, tag and anyone else's you saw in the lobby
   (`grep -il "yourname" fixtures/*.jsonl`). Nothing may match. All identity fields
   should read `redacted-N`, and numeric IDs should be small numbers.
5. **Document and test**: add a row per file to the table in `fixtures/README.md`, and a
   test in `crates/capture/tests/fixtures.rs` that checks the situation the file is named
   for (see the existing ones).
6. **Run the checks**:

   ```sh
   cargo test -p capture --test fixtures   # every fixture parses, is redacted, is typed
   cargo test --workspace
   ```

   A redaction failure means the redactor missed a field. Fix `crates/capture/src/redact.rs`
   (and its tests) first. Then replace the leaked value in the file by hand with a new
   `redacted-N`, numbered above the file's highest (as in commit `cd02112`). Don't run the
   redactor over a redacted file again: it restarts the numbering and can merge different
   values. Never commit the file as is.
7. **Commit** on a branch, only `fixtures/`, `docs/lcu-spec/help/` and the tests, with a
   one-line message (subject only, no body), for example
   `test: add 16.23 fixtures for ranked trades and dodges`. Check with `git status` that no
   `*.wire.jsonl` or `*.sweep.json` is staged, then push and open a PR.
