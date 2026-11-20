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

   Options: `--port N`, `--out DIR` (default `draftgpt-captures` in the OS temp directory,
   see below), `--auto false` (only record when
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

Everything is logged to `devserver.log` in the output folder (the path is printed at start). Our crates log at `info` and above, so
`RUST_LOG=debug` adds nothing yet; when you need more detail, add `tracing::debug!` lines
where you're looking and run with `RUST_LOG=debug` (cmd: `set RUST_LOG=debug`, PowerShell:
`$env:RUST_LOG="debug"`). The client password never reaches the log; keep it that way if
you add logging.

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

## Clash weekends

Clash is the reason to record on a Clash evening: the opponent team is known before
champ select (the scouting phase), and we want all of it on file so the panel can be
built and tested against replays later. **Live behaviour of this code is unconfirmed**
(written 2026-11-20 from the 16.23 spec, no Clash recording existed yet). The first
evening is its test; expect to hotfix. Background: [dev_notes/clash.md](../../dev_notes/clash.md).

### What happens during a Clash evening

| Clash `state` (`/lol-clash/v1/tournament-summary`) | In the client | What capture does |
|---|---|---|
| `UPCOMING` | Team registered, waiting | Nothing |
| `LOCK_IN` | Lock-in window (up to ~2 h, tier-dependent) | Starts a recording `<date>_<time>_clash.jsonl` |
| `SCOUTING` | Opponent known, scouting tab (minutes) | Same recording; fetches bracket and rosters |
| `IN_GAME` | Queue, champ select, game, waiting for next round | Records queue and champ select; **stops at game start** (no game time recorded); starts a new `_clash` recording after the game if the tournament is still on |
| `RESULTS` / `IDLE` | Done | Stops |

While a Clash recording runs, `crates/capture/src/clash.rs` fetches every 10 s (GET only):
tournament summary and state, `/lol-clash/v1/player`, `thirdparty/team-data`,
`tournament/{id}` (+ `/player`), `bracket/{bracketId}`, and `roster/{id}` + `/stats` for
**every team in the bracket**, plus `scouting/matchhistory` once per set of opponents. Only
changed data is written. Clash events the client pushes by itself are recorded too
(`is_recorded_clash_uri` in `crates/lcu-model/src/lib.rs`). Not fetched yet, on purpose:
anything by puuid (`scouting/champions`), summoner lookups, POST endpoints.

### Running it

Same setup as "Running it" above, on the branch you were asked to test. Start it **before
lock-in** and leave it running all evening:

```sh
cargo run -p capture --release            # auto mode; logs to <temp>/draftgpt-captures/capture.log
# or, with the dev page (problem flags visible): cargo run -p devserver --release
```

Run one of the two, not both (each would record and poll). Ctrl+C at the end of the
evening finishes the open recording.

**Fallback if auto doesn't start at lock-in** (no `recording started: …_clash.jsonl` line
within a minute of lock-in): stop it and record the whole evening by hand, which records
everything including the Clash fetches until Ctrl+C:

```sh
cargo run -p capture --release -- record --label clash_evening
```

### Is it working? (check after lock-in and after the first champ select)

In `capture.log`:
- `recording started: …_clash.jsonl` at lock-in (or at scouting).
- `Clash fetch: N changed, 0 failed` lines: N > 0 at the start and whenever something
  changes, then quiet. Repeated `failed` counts mean an endpoint misbehaves (below).
- `recording stopped` at game start, a new `recording started` after the game.

In the recording (agent; PowerShell or bash, any tool that counts lines):
```sh
grep -o '"uri":"[^"]*"' <file>.jsonl | sort | uniq -c
```
Expect `/lol-clash/v1/tournament-summary`, `/lol-clash/v1/bracket/<n>`,
`/lol-clash/v1/roster/redacted-N` (several teams), `/lol-clash/v1/roster/redacted-N/stats`
and the champ select session. Open one roster event: `members[].position` readable,
`summonerId`, `name`, `shortName`, `id` all `redacted-…` or small numbers.

### When it doesn't work (Clash)

Find the truth in the wire file first: it holds every frame the client sent while
recording. `grep -o '"uri":"/lol-clash[^"]*"' <name>.wire.jsonl | sort | uniq -c` lists
which Clash resources the client really uses and how often.

| Symptom | Cause and fix |
|---|---|
| No recording at lock-in or scouting | Auto starts on states in `CLASH_ACTIVE_STATES` (`crates/capture/src/auto.rs`). Look at the real `state` values in the wire file / log (the dev page flags unknown ones). Add the missing one there and to `CLASH_KNOWN_STATES`, with a test. Meanwhile use the manual fallback. |
| `Clash fetch … failed` repeatedly | An endpoint's URI or parameters differ from the spec. Compare with the URIs in the wire file and adjust `fetch` in `crates/capture/src/clash.rs`. A failed fetch never stops the recording; if unsure, leave it failing. |
| No bracket / roster events | `tournament-summary` may carry no `bracketId`/`rosterId` (or names differ). Check its data in the recording; `tournaments()` in `clash.rs` reads `tournamentId`, `rosterId`, `bracketId`. `/lol-clash/v1/tournament/{id}/player` also has `rosterId` and `bracketId`. |
| `scouting/matchhistory` fails | Its query format (`summonerIds=[…]`) is a guess. Harmless; fix only if it's trivial. |
| Recording runs during the game | `in_game()` in `auto.rs` lists the gameflow phases treated as the game. |
| Something identifying in a roster/bracket event | Fix `crates/capture/src/redact.rs` (`CLASH_TEXT_KEYS`, `SENSITIVE_KEY_PARTS`, `ID_AFTER_SEGMENT`) with a test; the recording stays local until fixed. Never weaken redaction. |

Hotfix rules are the ones above ("Hotfixing on the capture machine"); for Clash changes
run `cargo test -p capture -p lcu-model -p devserver` at least. Say in the commit that it
was found live in Clash, with the client version.

### Handing over after a Clash evening

Hand over all `*_clash.jsonl` (and `*_auto.jsonl` from the same evening),
`client-<version>.help.json` if new, and `capture.log`. Keep the wire files locally
until the project owner says whether they're needed for debugging; never commit or post
them. Note for the hand-over: tier, how many rounds were played, anything odd (a
forfeit, a bye, a dodge, a reconnect). Open questions the recordings should answer (see
`dev_notes/clash.md`): does the Clash champ select session carry `puuid`s, what
`nameVisibilityType` is in Clash, what `thirdparty/team-data` returns, how fast the
bracket updates between rounds.

## After the session: recordings to fixtures

Recordings and logs go to `draftgpt-captures` in the OS temp directory, never into the repo:

| OS | Folder |
|---|---|
| Windows | `%TEMP%\draftgpt-captures` (usually `C:\Users\<you>\AppData\Local\Temp\draftgpt-captures`) |
| macOS | `$TMPDIR/draftgpt-captures` (a per-user folder under `/var/folders/`; `echo $TMPDIR`) |
| Linux | `/tmp/draftgpt-captures` |

The temp folder gets emptied, so hand recordings over right after the session.

**If you recorded:** hand over the main `<name>.jsonl` files (and
`client-<version>.help.json` if there is one). Don't copy anything into the repo yourself.

**Only an agent saves recordings into `fixtures/`**, with the steps below.

The folder holds, per recording `<date>_<time>_<label>`:

| File | What happens to it |
|---|---|
| `<name>.jsonl` | The recording, redacted on write. Candidates for `fixtures/`. |
| `<name>.wire.jsonl` | Every raw client frame. **Never commit.** Keep locally (or hand over privately) for debugging. |
| `<name>.sweep.json` | Client state at the start. **Never commit.** Same as wire. |
| `client-<version>.help.json` | The client's schema, one per client build, with dashes for dots (`client-16-22-834-8910.help.json`). Commit if `docs/lcu-spec/help/` doesn't have that version yet. |
| `<name>.help.json` | Only when the version was unknown. Normally not needed. |
| `devserver.log` | Not committed. Keep it with the wire files if something went wrong. |

Wire and sweep files are gitignored, but never copy them anywhere under the repo.

Steps (for the agent):

1. **Pick the recordings worth keeping**: one per situation not yet in `fixtures/README.md`
   (for example teammate hovers, a trade, a ranked draft, a dodge by someone else). Skip
   duplicates of situations already covered.
2. **Copy and rename** each into `fixtures/` as `<client>_<situation>.jsonl`: `<client>` is
   major and minor from the header's `client_version` (`16.23.x.y` → `16_23`), `<situation>`
   says what happens, in snake case (`16_23_ranked_teammate_trade.jsonl`). Copy, don't edit
   the content. Clash recordings: `<client>_clash_<situation>`, one file per round as
   recorded (`16_23_clash_round1_scouting_to_draft.jsonl`, `16_23_clash_round2_bye`). Keep
   at least one full round (lock-in or scouting through champ select) even if a later one
   looks similar: the bracket and rosters differ per round. Don't trim the long lock-in
   wait at the start; it's the realistic input for a replay.
3. **Copy a new schema dump** `client-<version>.help.json` into `docs/lcu-spec/help/`.
4. **Check for leaks yourself**, besides the tests: ask the person who recorded for their
   Riot ID, summoner name and tag and anyone else's they saw in the lobby, and search the
   new files for each (`grep -il "theirname" fixtures/*.jsonl`). Nothing may match. All identity fields
   should read `redacted-N`, and numeric IDs should be small numbers. For Clash files also
   ask for their team's name and tag and the opponents' they saw (on the scouting screen
   and in the bracket) and search for each; every `name`/`shortName` in `/lol-clash/`
   events and every roster URI must read `redacted-…`.
5. **Document and test**: add a row per file to the table in `fixtures/README.md`, and a
   test in `crates/capture/tests/fixtures.rs` that checks the situation the file is named
   for (see the existing ones). For Clash: a summary in `SCOUTING` (or `LOCK_IN`), a
   bracket, rosters of both teams with readable `members[].position`, and the champ
   select session. If a Clash fixture shows something the typed structs or the code don't
   expect, write it into `dev_notes/clash.md` (open questions there).
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
