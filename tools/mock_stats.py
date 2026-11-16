#!/usr/bin/env python3
"""Champion list and MOCK stats for the UIs until the real stats store exists.

- data/champions.json: real champion IDs, names, Data Dragon aliases (for icon
  URLs) and class tags, from Riot's public Data Dragon CDN.
- data/stats/<patch>.json: made-up numbers in the stats format draft-core
  reads (`draft_core::recommend::Stats`), deterministic per champion, marked
  `"mock": true`. Positions are guessed from class tags. Nothing here says
  anything about the real game.

Usage: python3 tools/mock_stats.py [--version 16.22.1]
Stdlib only. Run from the repo root.
"""

import argparse
import json
import random
import urllib.request
from pathlib import Path

DDRAGON = "https://ddragon.leagueoflegends.com"
ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "data"

# Class tag → positions it usually plays (guesses, for mock data only).
TAG_POSITIONS = {
    "Marksman": ["bottom"],
    "Support": ["utility"],
    "Mage": ["middle"],
    "Assassin": ["middle", "jungle"],
    "Fighter": ["top", "jungle"],
    "Tank": ["top", "jungle"],
}


def fetch_json(url):
    req = urllib.request.Request(url, headers={"User-Agent": "draftgpt-tools"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.load(r)


def champions(version):
    data = fetch_json(f"{DDRAGON}/cdn/{version}/data/en_US/champion.json")["data"]
    out = [
        {"id": int(c["key"]), "name": c["name"], "alias": c["id"], "tags": c["tags"]}
        for c in data.values()
    ]
    return sorted(out, key=lambda c: c["name"])


def positions(tags):
    seen = []
    for tag in tags:
        for p in TAG_POSITIONS.get(tag, []):
            if p not in seen:
                seen.append(p)
    return seen[:2] or ["middle"]


def mock_stats(patch, champs):
    ids = [c["id"] for c in champs]
    by_pos = {}
    for c in champs:
        for p in positions(c["tags"]):
            by_pos.setdefault(p, []).append(c["id"])
    out = []
    for c in champs:
        rng = random.Random(f"{patch}:{c['id']}")
        pos = positions(c["tags"])
        rival_pool = sorted({i for p in pos for i in by_pos[p] if i != c["id"]})
        matchups = [
            {"champion": e, "delta": round(rng.uniform(-0.045, 0.045), 4)}
            for e in rng.sample(rival_pool, min(8, len(rival_pool)))
        ]
        synergies = [
            {"champion": a, "delta": round(rng.uniform(-0.01, 0.035), 4)}
            for a in rng.sample([i for i in ids if i != c["id"]], 6)
        ]
        out.append(
            {
                "id": c["id"],
                "banRate": round(rng.uniform(0.0, 0.25) ** 1.5, 4),
                "positions": [
                    {
                        "position": p,
                        "winRate": round(rng.uniform(0.46, 0.54) - 0.008 * k, 4),
                        "pickRate": round(rng.uniform(0.005, 0.12) / (k + 1), 4),
                    }
                    for k, p in enumerate(pos)
                ],
                "matchups": sorted(matchups, key=lambda m: m["champion"]),
                "synergies": sorted(synergies, key=lambda m: m["champion"]),
            }
        )
    return {"patch": patch, "mock": True, "champions": out}


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=1, ensure_ascii=False) + "\n")
    print(f"wrote {path.relative_to(ROOT)}")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--version", help="Data Dragon version (default: latest)")
    args = ap.parse_args()
    version = args.version or fetch_json(f"{DDRAGON}/api/versions.json")[0]
    patch = ".".join(version.split(".")[:2])
    champs = champions(version)
    write(DATA / "champions.json", {"version": version, "champions": champs})
    write(DATA / "stats" / f"{patch}.json", mock_stats(patch, champs))
    write(DATA / "stats" / "index.json", {"latest": patch, "mock": True})


if __name__ == "__main__":
    main()
