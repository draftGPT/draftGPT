# Clash: research notes (2026-11-20, client 16.23.840.5218)

Why: in Clash the opponent team is known before champ select (scouting phase), and the
draft itself is tournament-style with full information. A panel could show the enemy
roster and their champion pools before the draft even starts, then use them in draft.
Nothing here is built yet. Sources: our full `/help` dump of 16.23 (scope full, not in
git; regenerate with `capture help-dump`), Riot's developer pages (links at the end).

## Clash flow (what the client goes through)

`/lol-clash/v1/tournament-state-info` → `state`: `UPCOMING → LOCK_IN → SCOUTING → IN_GAME →
RESULTS → IDLE`, with `nextStateChangeTime`, `currentPhaseId`, `numRemainingPeriods`.
Scouting starts when lock-in closes; it's several minutes (older sources say ~7, Riot
staggers it) and is where the client shows the opponent team. Then queue → champ select
(tournament draft, queue 700 SR / 720 ARAM) → game → next bracket round (up to 3).

## Getting from "me" to the opponent team (all GET, no parameters we don't already have)

1. `/lol-clash/v1/tournament-summary` (event exists) → per tournament `{state, tournamentId,
   rosterId, bracketId}`. Also `/lol-clash/v1/tournament/{tournamentId}/player` →
   `{state: BRACKET_ROSTER|…, rosterId, bracketId}`.
2. `/lol-clash/v1/bracket/{bracketId}` → `matches[]` `{round, rosterId1, rosterId2, status:
   UPCOMING|STARTED|COMPLETED, winnerId, roundStartTime, gameStartTime}` + `rosters[]`
   `{rosterId, name, shortName, logo}`. The current match with our rosterId gives the
   opponent's rosterId.
3. `/lol-clash/v1/roster/{rosterId}` → `name`, `tier`, `wins/losses`, `members[]`
   `{summonerId, position: TOP|JUNGLE|MIDDLE|BOTTOM|UTILITY|FILL, tier, state}`.
   `/roster/{id}/stats` → per-player aggregated Clash stats (`rawStatsSum/Max` maps).
4. Champion pools, what the client's scouting tab shows:
   - `/lol-clash/v1/scouting/champions?puuids=[…]` → per player `totalMasteryScore`,
     `topMasteries[] {championId, championLevel, championPoints}`, `topSeasonChampions[]
     {championId, winCount, gameCount, winRate, kda}`.
   - `/lol-clash/v1/scouting/matchhistory?summonerIds=[…]` → untyped object (capture it).
   - `/lol-clash/v1/thirdparty/team-data` → `{captain, members[] {summonerId, role}}`; a
     Riot-made endpoint for third-party apps, but apparently *our own* team only. Verify.

**Gap:** rosters give `summonerId`, scouting/champions wants `puuid`. **Decided
2026-11-21 (live in Clash): the summoner lookup**, for the five Clash opponents only,
during scouting (they're shown by name in the client's scouting tab), never in champ select,
ranked or the sweep; the lookup is recorded reduced to `{summonerId, puuid}`. The options
were:
- `/lol-summoner/v1/summoners/{id}` is a lookup endpoint (our rules keep lookups out of
  the sweep). In Clash the opponent is shown by name in the client, so it's visible
  information, but it still crosses the "lookup" line we drew.
- The Clash champ select session may carry `puuid` for both teams (names are visible in
  Clash, unlike ranked). Needs a recording; then pools could load at champ select only.
- `POST /lol-champion-mastery/v1/scouting` and `POST /lol-summoner-profiles/v1/
  ranked-scouting-view/request` take puuids, but they're POSTs: out (GET-only rule).

## Riot policy (read 2026-11-20)

- Clash public API (DevRel post, 2020): `clash-v1` players/teams/tournaments; new features
  using it "must be reviewed by the DevRel team" before release. Says nothing about
  scouting either way.
- LoL developer policy: no rule names Clash or scouting. Relevant limits: products may
  not give "game-session-specific information that would be previously unknown to the
  player" or use "information not present in the game client" for a competitive edge;
  cannot identify players "deliberately hidden by the game". Products using the League
  Client API must be registered (and audited) through the Developer Portal.
- Reading: showing what the client's own scouting tab shows (enemy roster, positions,
  mastery, season champions) is information present in the client → low risk. Going
  beyond it (pulling the enemy's match history from the Riot API, deriving pools the
  client doesn't show) is a grey area → ask DevRel before shipping. Ranked rules stay:
  never de-anonymize.

## Capture for Clash (built 2026-11-20, not yet tested in a live Clash)

Runbook for whoever records: "Clash weekends" in `crates/devserver/AGENTS.md`.
- Recorded: `lol-clash` resources in `CLASH_RECORDED` (`lcu-model`), snapshots of summary,
  state-info and player on connect (`CLASH_RESOURCES`).
- Auto mode (`capture::auto`): records while a tournament is `LOCK_IN|SCOUTING|IN_GAME`,
  except during the game itself; label `clash`. One recording per round.
- `capture::clash::ClashPoller`: every 10 s while recording in Clash: summary, state,
  player, tournament/{id} (+player), bracket. Once per opponent (up to 3 tries while
  incomplete): both rosters, summoner lookups (recorded as `{summonerId, puuid}`),
  scouting/champions, ranked-stats per opponent. Writes only changes, as `Snapshot`
  events (so snapshots can appear mid-recording); scouting data isn't carried into the
  next recording. (Until 2026-11-21: team-data, every bracket team's roster + stats,
  matchhistory; see round 1.)
- Redaction: roster IDs (`rosterid`, `winnerid`), `inviterid`, `invitationid`; in Clash
  data also team names (`name`, `shortName`, `rosterName`, `rosterShortName`), string
  `id`s and summoner-ID map keys; the URI segment after `roster` (event URIs are now
  redacted in main recordings too). Same team = same fake everywhere (`redacted-N` as
  string, `N` as number). Bracket IDs (`bracketId`, the URI, the bracket's `id`) and
  match IDs are faked, scouting numbers rounded (`ROUNDED_KEYS`); tournament IDs kept.
- Smoke-tested live without a team (16.23, 2026-11-20): shapes match the spec;
  no team gives `bracketId: -1`, `rosterId: ""`, `tournament/{id}/player` →
  `NO_ROSTER`; `thirdparty/team-data` fails without a team. `tournament-state-info` also
  lists tournaments the player isn't in, so auto may record "clash" for a player without
  a team (harmless). Tournament 29241: next state change Sat 2026-11-21 17:15 CET.
- First live Clash, 16.23, 2026-11-21, round 1 (`fixtures/16_23_clash_round1_scouting_to_draft`):
  auto started at `LOCK_IN`, kept recording through `SCOUTING` and stopped at
  `GameStart`. Answers:
  - Champ select session (queue 700): enemies `HIDDEN`, `puuid` `""`, `summonerId` 0;
    teammates `VISIBLE` with puuids. **No way across the puuid gap at champ select.**
  - Tournament draft: `phase_transition` before each of the 4 blocks, bans by the captain
    cell only, `bans` field empty.
  - Rosters: every bracket team's members come back, but other teams' `position` is
    `FILL` (the client's scouting tab shows no positions either). `/roster/{id}/stats`
    is empty (`playerStats {}`, `periodStats []`) for every team, ours included; our own
    roster's `/stats` got 400 for its UUID during lock-in.
  - `thirdparty/team-data`: 500 on every poll (log spam; stop fetching it or back off).
  - `scouting/matchhistory?summonerIds=[…]`: 403 `RBAC: access denied` for any input (all
    players, the five opponents, one, our own, empty): Riot-side. Dropped.
  - The client's scouting tab (rank, top season champions with win rate/KDA, mastery,
    Clash history) isn't pushed as events: the UI fetches it over HTTP, so neither the
    main nor the wire file has it. Now fetched once per opponent (scouting snapshot, see
    `capture::clash`), checked live in round 2: every enemy pick was in their lists.
  - Bracket: the match pairings match the roster list; only slots of rounds not decided
    yet hold placeholder roster IDs. The bracket ID and match IDs are now faked, scouting
    numbers rounded (leaderboard lookups).
  - Recorder bug (fixed): a fetch round's snapshots were stamped per response but
    written together ~2 s later, behind champ select events; times went backwards.
  - Still open: timer values per Clash phase, how fast the bracket updates between rounds.

## Product ideas once data exists

- Scouting view (state `SCOUTING`): enemy roster by position, top champions per player
  (mastery + season games/win rate), suggested target bans for the whole team.
- In champ select: use the enemy pools to weight ban suggestions and flag likely picks
  (still only what the client shows).
- Between rounds: next opponent as soon as the bracket updates.

## Links

- https://www.riotgames.com/en/DevRel/clash-apis
- https://developer.riotgames.com/policies/general
- https://developer.riotgames.com/docs/lol (Game Policy section)
- https://support-leagueoflegends.riotgames.com/hc/en-us/articles/360000951548 (What is Clash?)
