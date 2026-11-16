#!/usr/bin/env python3
"""Synthetic recording of a 5v5 normal draft (queue 400), built from repo data.

Writes fixtures/synthetic/normal_draft_5v5.jsonl (gitignored, never committed:
it's made up). Regenerate it any time; it's deterministic.

Nothing here comes from a real game. Field shapes come from a real 16.22
session (fixtures/16_22_draft_ban_and_pick_timeout_autolock.jsonl, already
redacted); champions are picked per position from data/stats/<patch>.json;
names and IDs are placeholders like in redacted recordings.

What it simulates, following what 16.22 captures show:
- PLANNING: every ban action "in progress", teammates declare intent
  (championPickIntent); enemy intents are never visible.
- BAN_PICK: ten simultaneous bans; your team's bans show as they lock, the
  enemy's only at the reveal (ten_bans_reveal), one enemy ban missed (-1).
  The `bans` summary stays empty with simultaneous bans.
- Picks 1-2-2-2-2-1 (blue 0; red 5,6; blue 1,2; red 7,8; blue 3,4; red 9);
  pairs act at the same time. Teammates' hovers show on their in-progress
  pick action; enemy picks appear only once locked.
- The timer is only re-measured when a turn starts; hover and lock updates
  in between repeat the old measurement (internalNowInEpochMs).
- FINALIZATION, GAME_STARTING, then the gameflow phase moves to GameStart and
  the session is deleted. Both session URIs are recorded, like the recorder
  does.

Usage: python3 tools/synth_fixture.py   (from the repo root; stdlib only)
"""

import copy
import json
import random
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TEMPLATE = ROOT / "fixtures" / "16_22_draft_ban_and_pick_timeout_autolock.jsonl"
OUT = ROOT / "fixtures" / "synthetic" / "normal_draft_5v5.jsonl"
STATS_INDEX = ROOT / "data" / "stats" / "index.json"

SESSION = "/lol-champ-select/v1/session"
TEAM_BUILDER = "/lol-lobby-team-builder/champ-select/v1/session"
GAMEFLOW = "/lol-gameflow/v1/gameflow-phase"
MATCHMAKING = "/lol-lobby-team-builder/v1/matchmaking"

STARTED = datetime(2026, 11, 16, 18, 0, 0, tzinfo=timezone.utc)
EPOCH0 = int(STARTED.timestamp() * 1000)
POSITIONS = ["top", "jungle", "middle", "bottom", "utility"]
LOCAL_CELL = 2  # you: blue side, mid
PICK_GROUPS = [[0], [5, 6], [1, 2], [7, 8], [3, 4], [9]]


def load_template():
    lines = TEMPLATE.read_text().splitlines()
    for line in lines[1:]:
        ev = json.loads(line)
        if ev["uri"] == SESSION and ev["data"]:
            return ev["data"]
    raise SystemExit(f"no session in {TEMPLATE}")


def choose_champions(rng):
    """Five champions per team, one per position, from the stats file."""
    patch = json.loads(STATS_INDEX.read_text())["latest"]
    stats = json.loads((ROOT / "data" / "stats" / f"{patch}.json").read_text())
    by_pos = {p: [] for p in POSITIONS}
    for c in stats["champions"]:
        main = max(c["positions"], key=lambda p: p["pickRate"])
        by_pos[main["position"]].append(c["id"])
    used = set()

    def take(pos):
        pool = [c for c in sorted(by_pos[pos]) if c not in used]
        c = rng.choice(pool)
        used.add(c)
        return c

    picks = {cell: take(POSITIONS[cell % 5]) for cell in range(10)}
    alt_hovers = {cell: take(POSITIONS[cell % 5]) for cell in (0, 1, 2, 3, 4)}
    bans = {cell: take(rng.choice(POSITIONS)) for cell in range(10)}
    return picks, alt_hovers, bans


class Draft:
    def __init__(self, template, rng):
        self.rng = rng
        self.events = []
        self.s = {k: copy.deepcopy(v) for k, v in template.items() if k not in ("actions", "myTeam", "theirTeam")}
        self.s.update(
            queueId=400,
            isCustomGame=False,
            gameId=6,
            id="redacted-1",
            localPlayerCellId=LOCAL_CELL,
            hasSimultaneousBans=True,
            hasSimultaneousPicks=False,
            counter=0,
            trades=[{"cellId": c, "id": 100 + c, "state": "INVALID"} for c in range(5) if c != LOCAL_CELL],
            pickOrderSwaps=[],
            positionSwaps=[],
            bans={"myTeamBans": [], "numBans": 10, "theirTeamBans": []},
        )
        member = template["myTeam"][0]
        fake = iter(range(20, 1000))

        def player(cell):
            p = copy.deepcopy(member)
            ally = cell < 5
            p.update(
                cellId=cell,
                team=1 if ally else 2,
                assignedPosition=POSITIONS[cell] if ally else "",
                championId=0,
                championPickIntent=0,
                selectedSkinId=0,
                isHumanoid=False,
                nameVisibilityType="VISIBLE",
                gameName=f"redacted-{next(fake)}" if ally else "",
                tagLine=f"redacted-{next(fake)}" if ally else "",
                puuid=f"redacted-{next(fake)}",
                summonerId=next(fake) if ally else 0,
                spell1Id=4 if ally else 0,
                spell2Id=[12, 11, 14, 7, 3][cell] if ally else 0,
            )
            return p

        self.s["myTeam"] = [player(c) for c in range(5)]
        self.s["theirTeam"] = [player(c) for c in range(5, 10)]
        ids = iter(range(1, 200))
        ban_group = [self.action(next(ids), c, "ban") for c in range(10)]
        reveal = [{**self.action(100, -1, "ten_bans_reveal"), "isAllyAction": False}]
        picks = [[self.action(next(ids), c, "pick") for c in g] for g in PICK_GROUPS]
        self.s["actions"] = [ban_group, reveal, *picks]

    @staticmethod
    def action(aid, cell, kind):
        return {
            "actorCellId": cell,
            "championId": 0,
            "completed": False,
            "duration": 0,
            "id": aid,
            "isAllyAction": 0 <= cell < 5,
            "isInProgress": False,
            "pickTurn": 0,
            "type": kind,
        }

    def find(self, kind, cell):
        for group in self.s["actions"]:
            for a in group:
                if a["type"] == kind and a["actorCellId"] == cell:
                    return a
        raise KeyError((kind, cell))

    def member(self, cell):
        team = self.s["myTeam"] if cell < 5 else self.s["theirTeam"]
        return next(p for p in team if p["cellId"] == cell)

    def timer(self, t, phase, total):
        """A new measurement: only at the start of a phase or turn."""
        self.s["timer"] = {
            "adjustedTimeLeftInPhase": total,
            "internalNowInEpochMs": EPOCH0 + t,
            "isInfinite": False,
            "phase": phase,
            "totalTimeInPhase": total,
        }

    def emit(self, t, kind="Update"):
        self.s["counter"] += 1
        data = copy.deepcopy(self.s)
        for uri in (TEAM_BUILDER, SESSION):
            self.events.append({"t": t, "uri": uri, "kind": kind, "data": data})
            t += 3

    def other(self, t, uri, kind, data):
        self.events.append({"t": t, "uri": uri, "kind": kind, "data": data})


def build():
    rng = random.Random("normal-draft-5v5")
    picks, alt_hovers, bans = choose_champions(rng)
    d = Draft(load_template(), rng)

    # PLANNING: 30 s; all bans "in progress"; teammates declare intent.
    d.timer(0, "PLANNING", 30_000)
    for a in d.s["actions"][0]:
        a["isInProgress"] = True
    d.emit(0, "Snapshot")
    d.other(0, GAMEFLOW, "Snapshot", "ChampSelect")
    d.other(0, MATCHMAKING, "Snapshot", {"dodgeData": {"dodgerId": 0, "state": "Invalid"}, "errors": [],
                                         "estimatedQueueTime": 0.0, "isCurrentlyInQueue": False, "lobbyId": "",
                                         "queueId": 400, "readyCheck": {"state": "Invalid"}, "searchState": "Invalid"})
    t = 1_500
    for cell in (0, 3, 1, LOCAL_CELL, 4):
        d.member(cell)["championPickIntent"] = alt_hovers[cell] if cell == LOCAL_CELL else picks[cell]
        d.emit(t)
        t += rng.randint(1_200, 3_500)
    d.member(LOCAL_CELL)["championPickIntent"] = picks[LOCAL_CELL]  # you change your mind
    d.emit(t)

    # BAN_PICK, bans: 30 s, simultaneous; allies' bans show as they lock.
    t = 30_000
    d.timer(t, "BAN_PICK", 30_000)
    d.emit(t)
    for cell in (1, 4, 0, LOCAL_CELL, 3):
        t += rng.randint(2_000, 5_000)
        a = d.find("ban", cell)
        a["championId"] = bans[cell]
        d.emit(t)  # hovering the ban (visible for your team)
        t += rng.randint(800, 2_000)
        a["completed"], a["isInProgress"] = True, False
        d.emit(t)

    # Reveal: the enemy's bans appear, one of them missed.
    t = 60_000
    for cell in range(5, 10):
        a = d.find("ban", cell)
        a["championId"] = -1 if cell == 8 else bans[cell]
        a["completed"], a["isInProgress"] = True, False
    for a in d.s["actions"][0]:
        a["isInProgress"] = False
    reveal = d.s["actions"][1][0]
    reveal["completed"] = True

    # Picks, 30 s per turn. Like the client, the update that locks a turn's
    # last pick also starts the next turn (or finalization).
    def start(group, t):
        d.timer(t, "BAN_PICK", 30_000)
        for cell in group:
            d.find("pick", cell)["isInProgress"] = True

    start(PICK_GROUPS[0], t)
    d.emit(t)
    for i, group in enumerate(PICK_GROUPS):
        for j, cell in enumerate(group):
            a = d.find("pick", cell)
            if cell < 5:  # teammates' (and your) hovers are visible
                first = alt_hovers[cell] if cell == LOCAL_CELL else picks[cell]
                t += rng.randint(1_500, 4_000)
                a["championId"] = first
                d.emit(t)
                if cell == LOCAL_CELL:
                    t += rng.randint(2_000, 4_000)
                    a["championId"] = picks[cell]
                    d.emit(t)
            t += rng.randint(2_000, 6_000)
            a["championId"] = picks[cell]
            a["completed"], a["isInProgress"] = True, False
            m = d.member(cell)
            m["championId"] = picks[cell]
            m["championPickIntent"] = 0
            m["selectedSkinId"] = picks[cell] * 1000
            if j == len(group) - 1:
                if i + 1 < len(PICK_GROUPS):
                    start(PICK_GROUPS[i + 1], t)
                else:
                    d.timer(t, "FINALIZATION", 60_000)
            d.emit(t)

    # FINALIZATION 60 s (a skin change), then GAME_STARTING.
    d.member(LOCAL_CELL)["selectedSkinId"] = picks[LOCAL_CELL] * 1000 + 3
    d.emit(t + 7_000)
    t += 60_000
    d.timer(t, "GAME_STARTING", 0)
    d.emit(t)
    d.other(t + 900, GAMEFLOW, "Update", "GameStart")
    d.other(t + 1_000, TEAM_BUILDER, "Delete", None)
    d.other(t + 1_003, SESSION, "Delete", None)
    return d.events


def main():
    events = sorted(build(), key=lambda e: e["t"])
    header = {"format_version": 1, "client_version": None,
              "started_at": STARTED.isoformat().replace("+00:00", "Z"), "label": "synthetic"}
    OUT.parent.mkdir(parents=True, exist_ok=True)
    with OUT.open("w") as f:
        f.write(json.dumps(header, separators=(",", ":")) + "\n")
        for e in events:
            f.write(json.dumps(e, separators=(",", ":")) + "\n")
    print(f"wrote {OUT.relative_to(ROOT)} ({len(events)} events, {events[-1]['t'] / 1000:.0f} s)")


if __name__ == "__main__":
    main()
