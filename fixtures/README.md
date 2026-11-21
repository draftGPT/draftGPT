Recordings (`.jsonl`, format in `lcu_model::recording`) of real champ selects, for replay and
tests. Each is the main recording only, redacted by the recorder; wire and sweep files never
come here. `crates/capture/tests/fixtures.rs` checks that every file parses, is redacted, is
understood by the typed structs, and still shows the situation it's named for.

The client's `/help` schema for each client build is in `docs/lcu-spec/help/`, named by the
`client_version` in the recording's header.

Named `<client>_<situation>`. The `16_22` files are from client 16.22.834.8910 (Windows,
2026-11-15), the `16_23` files from 16.23.840.5218 (Windows, 2026-11-21). Customs have bots,
so teammate and enemy cells are filled from the start.

| File | Queue | What it shows |
|---|---|---|
| `16_22_aram_bench_swaps` | 2400 (ARAM, real players) | Shared bench, several bench swaps, finalization only |
| `16_22_draft_planning_hovers` | 3110 (custom draft) | Planning phase, five hovers in a row, `ten_bans_reveal` action; left during planning |
| `16_22_draft_ban_and_pick_timeout_autolock` | 3130 (custom draft) | `phase_transition`, locked ban, hover during pick turn auto-locked when the timer ran out, finalization, then left (`PartyDodged`) |
| `16_22_draft_missed_ban` | 3130 | Ban timer ran out: ban completes with `championId: -1`; `bans` stays empty (simultaneous bans) |
| `16_22_draft_pick_timeout_kicked` | 3130 | Pick timer ran out with no hover: removed from champ select, `PartyDodged` like a manual leave |
| `16_22_subset_pick_swaps_dodge` | 3270 (pick one of three) | `subset-champion-list` offer, pick, swaps between the offered champions, left (`PartyDodged`) |
| `16_22_client_closed_mid_champ_select` | 3270 | Client closed during champ select: phase goes to `None`, no dodge |
| `16_23_normal_draft_pick_order_swap_stranger_dodge` | 400 (normal draft, real players) | Pick-order swap sent in planning (`SENT`), accepted in ban/pick: `localPlayerCellId` moves 6 → 5 and the two players' entries trade cells (actions stay with the cells); teammate hovers, simultaneous bans then `ten_bans_reveal`, trades `AVAILABLE` after both locked; another player dodged (`StrangerDodged`, back to `Matchmaking`) |
| `16_23_draft_position_swap_and_back` | 3110 (custom draft, 2 players) | Position swap carried out: `positionSwaps` entry `SENT`, then both players' `assignedPosition` flip (bottom ↔ middle) while cells stay put, and the contract `id` moves 0 → 2; local player locks in, then swaps back after locking (`id` 2 → 4); finalization with trades `AVAILABLE`. Recorded during a Clash weekend, so it also holds `/lol-clash/` snapshots |
| `16_23_ranked_flex_pick_order_swap_received_stranger_dodge` | 440 (ranked flex) | Red side (ally cells 5–9). Pick-order swap `RECEIVED` in ban/pick and accepted: `localPlayerCellId` 7 → 5, the local player keeps their position (utility). Enemies `HIDDEN` with empty name fields, teammates `VISIBLE`. Another player dodged (`StrangerDodged`, back to `Matchmaking`) |
| `16_23_ranked_flex_pick_order_swap_sent_game_start` | 440 (ranked flex) | Full ranked draft to the end: planning with teammate intents, ten simultaneous bans (teammates' ban hovers visible) then `ten_bans_reveal`, pick-order swap `SENT` and accepted (`localPlayerCellId` 1 → 0), 1-2-2-2-2-1 picks with enemy picks shown only once locked, `FINALIZATION`, `GAME_STARTING`, gameflow `GameStart`. The session's `bans` field stays empty; bans are only in `actions` |

The first two were recorded before matchmaking and the subset list were core resources, so
they lack those events. Not covered yet: trades actually carried out, ranked solo/duo
(teammates hidden; in the flex fixtures all teammates are `VISIBLE`).

## Clash

None yet (first Clash recordings expected 2026-11-21/22). Named `<client>_clash_<situation>`,
one per round. Besides the champ select resources they hold `/lol-clash/` events: the
tournament summary and state (`LOCK_IN`, `SCOUTING`, `IN_GAME`), the bracket, every
bracket team's roster and stats, team data, and scouting match history, as `Snapshot`
events whenever they changed (also mid-recording). Team names, roster IDs and player IDs
are fakes; the same team has the same fake throughout a file (`redacted-N` as a string,
`N` as a number in brackets). How they're recorded: `crates/capture/src/clash.rs`.

## Converted RCVolus recordings

`rcvolus_*` files are converted from the sample recordings in
[RCVolus/lol-pick-ban-ui](https://github.com/RCVolus/lol-pick-ban-ui) (`recordings/`,
MIT License, Copyright (c) 2020 Riot Community Volunteers; full notice in
[LICENSE-rcvolus](LICENSE-rcvolus)) with `capture convert-rcvolus <in.json> <out.jsonl>`.
They hold only `/lol-champ-select/v1/session`, polled about once per second (identical polls
collapsed, starting at the first poll inside champ select), so there are no gameflow or
matchmaking events, `client_version` is unknown (label `rcvolus`), and newer fields are
missing. They stay as compatibility tests for old session shapes.

| File | What it shows |
|---|---|
| `rcvolus_2019_solo_custom_ban_pick` | Solo custom (one player, no enemies): ban, pick, finalization |
| `rcvolus_2022_solo_custom_ban_pick` | Same situation, 2022 schema (`gameId`, `isAllyAction`, `recoveryCounter`) |
| `rcvolus_2019_tournament_draft_spectated` | Spectated 5v5 tournament draft: 20 turns, second ban phase after six picks, both teams' picks visible, `localPlayerCellId: -1` |
