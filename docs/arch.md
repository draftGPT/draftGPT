# Architecture

```
draftGPT/
├── Cargo.toml              workspace
├── crates/
│   ├── lcu-model/          what the client sends, no I/O, native + wasm32
│   │   ├── raw             RawEvent { t, uri, kind, data: Value }
│   │   ├── types           lenient typed structs: ChampSelectSession, GameflowPhase,
│   │   │                   SubsetChampionList, Matchmaking
│   │   ├── recording       file format: header + RawEvent lines, wire lines, reader
│   │   └── (lib)           resources we use (CORE_RESOURCES): event name + URI pairs
│   ├── lcu/                talking to the running client; re-exports lcu-model
│   │   ├── discovery       lockfile / LeagueClientUx args → port + password
│   │   ├── transport       HTTPS (Riot cert, Basic auth) + WebSocket
│   │   └── source          EventSource trait, SourceEvent, ChannelSource,
│   │       │               Replay(.jsonl, 1x / Nx / instant)
│   │       └── live        Live: discover, connect, snapshot, forward, reconnect
│   ├── capture/            recorder lib + small CLI
│   │   ├── recorder        RawEvent → redacted .jsonl, raw frames → redacted .wire.jsonl
│   │   ├── redact          PII fields → stable fake values per recording
│   │   ├── help_dump       client /help (Full + Console) → client-<version>.help.json, once per build
│   │   ├── sweep           GET of sweep-paths.txt, first recording per client launch →
│   │   │                   <name>.sweep.json (compact; big Clash endpoints only in Clash)
│   │   ├── extras          per-recording background work: sweep, then help dump
│   │   ├── auto            record while gameflow phase == ChampSelect
│   │   ├── controller      SourceEvents → recordings; shared by CLI and devserver
│   │   ├── logging         console + <out>/<binary>.log
│   │   └── fsutil          atomic writes
│   ├── devserver/          axum dev server, 127.0.0.1 + same-origin only
│   │   ├── api             status · ws events · capture start/stop/auto ·
│   │   │                   captures list/download · replay
│   │   ├── page            page feed: messages, redaction, logged-once problems
│   │   ├── check           flags parse failures / unknown enum values per event
│   │   └── static/         dev page, plain HTML/JS via include_str!
│   ├── draft-core/         pure Rust, no I/O, native + wasm32; depends on lcu-model
│   │   ├── state           DraftState: picks, bans, hovers, positions, turn, timer
│   │   └── reducer         ChampSelectSession → DraftState
│   └── desktop/            Tauri side panel, live only
│       └── dock            follows client window; floats if fullscreen or toggled
├── tools/
│   ├── lcu_spec.py         full LCU spec → docs/lcu-spec subset + capture/sweep-paths.txt
│   └── convert-rcvolus     RCVolus recordings → our .jsonl (redacted)
├── fixtures/               *.jsonl recordings named by situation (README lists them)
└── docs/
    ├── arch.md             this file
    └── lcu-spec/           LCU spec, subset: champ select, team builder, gameflow,
                            lobby, clash, ranked, champions, mastery, summoner
```

## Notes

- **The boundary is `DraftState`.** Every input produces it. Everything downstream only
  ever sees `DraftState`.
- **The desktop app calls `lcu` and `draft-core` in-process.** `devserver` is a dev tool,
  not a product dependency.
- **`lcu-model` is the type boundary.** Anything that only needs the data (draft-core, the
  web drafter, tools) depends on it, never on `lcu`, so it can't pull in tokio or TLS.
- **Recording format** (`lcu_model::recording`).
  - A header line: `{format_version, client_version, started_at, label}`.
  - Then one `RawEvent` per line, with `t` in milliseconds from the start of the recording,
    for the core resources only: the champ select session (under `lol-champ-select` and
    `lol-lobby-team-builder`), the subset champion list (champions offered in pick-one-of-few
    modes, not in the session), team-builder matchmaking (`dodgeData`: why champ select
    ended early) and the gameflow phase.
  - The client's `/help` schema is saved once per client build as
    `client-<version>.help.json` in the same directory; the header's `client_version`
    names it. Only if the version is unknown or the dump is incomplete does a recording get
    its own `<name>.help.json` (and the next recording retries the version file).
  - A `<name>.wire.jsonl` holds every raw client frame received while recording.
  - A `<name>.sweep.json` holds the client state at the start (GETs of the spec plugins'
    endpoints that take no parameters, minus exclusions). Only the first recording of each
    client launch gets one, since it's mostly account state; later recordings get one only
    in a Clash queue, holding just the large Clash endpoints the full sweep skips elsewhere.
  - Wire and sweep files stay local and are never committed.

## Data flow

```
 League Client ──► lcu Live ────┐
                                ├──► reducer ──┐
 fixtures/*.jsonl ► lcu Replay ─┘              │
                                               ├──► DraftState
 web: manual picks ────────────────────────────┘        │
                                                        │
                                      ┌─────────────────▼────────────┐            ┌──────────────┐
                                      │ draft-core engine            │   fetch    │ DB           │
                                      │ (client-side)                │◄───────────┤ stats per    │◄── analysis/
                                      │ DraftState + stats → recs    │  + cache   │ patch        │
                                      └─────────────────┬────────────┘            └──────────────┘
                                                        │ recommendations
                                           ┌────────────┴────────────┐
                                           ▼                         ▼
                                   Tauri side panel            web drafter
                                   (live, mocked)              (manual)
```

- Every input becomes a `DraftState`. Live and mocked inputs go through the same reducer;
  the web drafter builds `DraftState` directly from manual picks.
- The engine runs inside each frontend (native in Tauri, WASM on the web). It only reads
  from the DB.
- The capture tooling (`capture`, `devserver`, dev page) sits beside this flow. It records
  `RawEvent`s and does not use `DraftState`.
