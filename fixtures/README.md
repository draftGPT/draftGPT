Recordings (`.jsonl`, format in `lcu_model::recording`) of real champ selects, for replay and
tests. Each is the main recording only, redacted by the recorder; wire and sweep files never
come here. `crates/capture/tests/fixtures.rs` checks that every file parses, is redacted, is
understood by the typed structs, and still shows the situation it's named for.

The client's `/help` schema for each client build is in `docs/lcu-spec/help/`, named by the
`client_version` in the recording's header.

Named `<client>_<situation>`. All from client 16.22.834.8910 (Windows, 2026-11-15); customs
have bots, so teammate and enemy cells are filled from the start.

| File | Queue | What it shows |
|---|---|---|
| `16_22_aram_bench_swaps` | 2400 (ARAM, real players) | Shared bench, several bench swaps, finalization only |
| `16_22_draft_planning_hovers` | 3110 (custom draft) | Planning phase, five hovers in a row, `ten_bans_reveal` action; left during planning |
| `16_22_draft_ban_and_pick_timeout_autolock` | 3130 (custom draft) | `phase_transition`, locked ban, hover during pick turn auto-locked when the timer ran out, finalization, then left (`PartyDodged`) |
| `16_22_draft_missed_ban` | 3130 | Ban timer ran out: ban completes with `championId: -1`; `bans` stays empty (simultaneous bans) |
| `16_22_draft_pick_timeout_kicked` | 3130 | Pick timer ran out with no hover: removed from champ select, `PartyDodged` like a manual leave |
| `16_22_subset_pick_swaps_dodge` | 3270 (pick one of three) | `subset-champion-list` offer, pick, swaps between the offered champions, left (`PartyDodged`) |
| `16_22_client_closed_mid_champ_select` | 3270 | Client closed during champ select: phase goes to `None`, no dodge |

The first two were recorded before matchmaking and the subset list were core resources, so
they lack those events. Not covered yet (needs other players): teammate hovers, trades and
swap requests, ranked (hidden names, positions), the simultaneous-ban reveal, another player
dodging.
