# draftGPT

A read-only League of Legends champion select assistant. It reads the live draft from the
League Client (LCU API) and recommends picks and bans from precomputed statistics.
Architecture, crate responsibilities and the branch plan are in [docs/arch.md](docs/arch.md).
Read it before structural changes and keep it up to date when the design changes.

## Rules that are never negotiable

- **Read-only towards the League Client.** GET requests and WebSocket subscriptions only.
  The one exception is `POST /Help` / `POST /help`, which only reads the client's schema.
  Never call write endpoints: no automated picks, bans, hovers or ready-check accepts. Don't
  suggest them either. `LcuHttp` has no generic request method on purpose; keep it that way.
- **Champ select only.** No in-game features, overlays or game process access.
- **Never reveal hidden data**, such as teammate names in ranked champ select. Never request
  chat, store or auth endpoints. (The wire capture receives their events passively, only
  into local files; nothing uses them.)
- **Nothing identifying in fixtures.** Recordings are redacted on write by
  `capture::redact`: player and game IDs, Riot IDs and names, `obfuscated*`, chat
  credentials, anything named like a password/token/JWT (key fragments, case-insensitive),
  chat IDs by value. `capture::recorder::Recorder` does it, so every file it writes is
  redacted. Don't commit an unredacted capture.
- **Wire and sweep files (`*.wire.jsonl`, `*.sweep.json`) are local-only, never committed**
  (gitignored). Wire files hold every client frame while recording, including chat and lobby
  data; sweep files hold a GET of the spec plugins' endpoints that take no parameters, at the
  start of the first recording of each client launch. Chat resources lose all their text,
  ID-like URI segments and JWTs are replaced, but a free-text field with a generic name
  can't be caught. Fixtures in the repo come only from the main `.jsonl`.
- **The sweep never calls de-anonymizing, credential, chat or lookup endpoints.** The list is
  generated (`crates/capture/sweep-paths.txt`); exclusions live in `SWEEP_EXCLUDE` in
  `tools/lcu_spec.py`. Check new plugins against them before adding.
- devserver redacts what it sends to the page too, and refuses requests whose Host or
  Origin isn't its own (blocks other websites from reading it or driving it).

## Current scope

Which branch builds what, and what's in progress, is in `notes/branches.txt`. It's local and
gitignored; read it at the start of a task and update it as work progresses.
`docs/arch.md` stays stable until the backend is designed, so don't edit it to record
progress, only for real structural changes.

Don't plan or scaffold the web drafter, scoring, the stats database or the analysis pipeline
until asked.

## Working on this repo

- Development happens on Linux, but the League client only runs on Windows. Live behaviour
  can only be confirmed on Windows, so say so when a change hasn't been checked live.
- There's no official LCU documentation. The reference is `docs/lcu-spec/swagger.json`, the
  subset of the community spec (dysolix/hasagi-types, generated from the client's `/help`)
  for the plugins we use. `python3 tools/lcu_spec.py` rebuilds it and saves the full spec to
  `docs/lcu-spec/full/` (gitignored); to keep another plugin, add it to `PLUGINS` there.
  Our own captures' `/help` dumps (`docs/lcu-spec/help/`, one per client build) replace it
  later. Recorded captures win over the spec
  wherever they disagree.
- Keep raw and typed separate: the recorder never parses or types event data (it only
  redacts it and rebases `t`). Typed structs are
  lenient (`Option`, `#[serde(default)]`, `#[serde(other)] Unknown`) because Riot changes the
  schema without notice. Parse with `lcu_model::types::parse`, which also treats `null` like a
  missing field.
- `lcu-model` (raw events, types, recording format, resource constants) has no I/O and must
  stay buildable for `wasm32`; its only dependencies are serde, serde_json and thiserror.
  `draft-core` and anything else that only needs the data depend on `lcu-model`, never on
  `lcu`. Check with `cargo check -p lcu-model --target wasm32-unknown-unknown`.
- `lcu` builds reqwest without a bundled crypto provider (keeps aws-lc out of the Windows
  build). Binaries call `lcu::transport::tls::install_crypto_provider()` first thing, or any
  reqwest client in the process panics.
- `devserver` binds to `127.0.0.1` only, never proxies arbitrary LCU requests and never
  exposes the LCU password.
- Tests: every fixture must parse, golden `DraftState` snapshots (`insta`) from replays, and
  redaction tests (no PII-shaped fields in recorder output).
- Window tracking for the desktop dock is Windows-only and goes behind `#[cfg(windows)]` with
  a stub elsewhere.

## LCU facts the spec doesn't give you

- The `lockfile` has the format `name:pid:port:password:protocol`. The port and password
  change on every client launch. Auth is Basic `riot:<password>`, and the certificate is
  Riot's self-signed root.
- The WebSocket envelope is `{uri, eventType: Create|Update|Delete, data}`. Subscribe with
  `[5, "OnJsonApiEvent_lol-champ-select_v1_session"]` and
  `[5, "OnJsonApiEvent_lol-gameflow_v1_gameflow-phase"]`.
- `session.actions` is a list of lists of actions, one inner list per turn; the spec types it
  as `[object]`. `championId: 0` means none. The string enums (`action.type`,
  `timer.phase`, `assignedPosition`) are not listed in the spec, so take their values from
  captures.
- Enemy hovers are never visible. Enemy picks and bans appear only once they are locked in.
  `theirTeam[].assignedPosition` is `""` in draft.
- The session is served under `/lol-champ-select/v1/session` and
  `/lol-lobby-team-builder/champ-select/v1/session` (same type; both recorded).
  `/lol-champ-select-legacy/` was removed in 16.20.
- A missed ban completes with `championId: -1`. In `PLANNING` every ban action already has
  `isInProgress: true`. Time left is `adjustedTimeLeftInPhase - (now - internalNowInEpochMs)`.
- Values seen in real captures (other projects' fixtures, 2025–2026): `nameVisibilityType`
  `VISIBLE`/`HIDDEN`/`UNHIDDEN` (local player in ranked); action types `vote`,
  `phase_transition`, `vote_transition`, `team_vote_reveal` exist in other code but no
  capture yet.
- `/help`: `format=Full` gives types and fields but no URL or HTTP method; those come only
  from `format=Console`. The dump fetches both.
- Early fixtures are converted RCVolus recordings: 2019–2022, polled about once per second,
  missing newer fields. Keep them as compatibility tests after our own captures replace them.

## Commands

```sh
cargo test --workspace                          # all tests (no League client needed)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo run -p devserver -- --port 8790 --out captures      # dev page at http://127.0.0.1:8790
cargo run -p capture -- --out captures [auto | record --label x | help-dump --file f.json]
# both: --wire true|false (raw frame capture, default on); logs go to <out>/<binary>.log
cargo build -p devserver --release              # on Windows: target/release/devserver.exe
```

`DRAFTGPT_LCU_INSECURE_TLS=1` skips LCU certificate checks; it's a last resort if a client
update breaks the Riot-root verification in `lcu::transport::tls`.
