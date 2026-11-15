Session JSON for unit tests in `crates/lcu-model`. All identifying fields are redacted.

- `rcvolus_*`: snapshots from the recordings in
  [RCVolus/lol-pick-ban-ui](https://github.com/RCVolus/lol-pick-ban-ui) (MIT), 2019 and 2022
  clients. They are missing fields that newer clients send.
- `dosxnjos_16_20_*`: snapshots from a real normal-draft (queue 400) champ select on client
  16.20, from the `champselect-real.json` fixture in
  [dosxnjos/lol-client](https://github.com/dosxnjos/lol-client) (MIT). Player identity was
  already removed upstream; `gameId` was replaced with small fakes here.
- `synthetic_16_21_ranked_pick_phase.json`: written by hand from the 16.21 schema in
  `docs/lcu-spec/` (ranked solo queue, simultaneous bans, mid pick phase). The field set
  follows the spec; enum values like `nameVisibilityType: "HIDDEN"` are guesses until real
  captures replace this file.
