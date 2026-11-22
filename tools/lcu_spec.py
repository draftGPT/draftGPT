#!/usr/bin/env python3
"""Builds docs/lcu-spec/swagger.json: the part of the LCU OpenAPI spec we use.

Downloads the full spec (default: dysolix/hasagi-types, regenerated per patch)
into docs/lcu-spec/full/ (gitignored), then keeps only the paths of PLUGINS
plus every schema they reference, directly or indirectly. Also writes
crates/capture/sweep-paths.txt: the GET endpoints of PLUGINS that take no
parameters at all, minus SWEEP_EXCLUDE, which a recording snapshots once when
it starts.

    python3 tools/lcu_spec.py                 # download, then write the subset
    python3 tools/lcu_spec.py --from FILE     # use a spec you already have

Standard library only.
"""

import argparse
import json
import sys
import urllib.request
from pathlib import Path

SOURCE_URL = "https://raw.githubusercontent.com/dysolix/hasagi-types/main/swagger.json"

# First path segment of the endpoints we keep. Add a plugin here and re-run.
PLUGINS = [
    "lol-champ-select",        # the draft
    "lol-lobby-team-builder",  # the draft, second session endpoint
    "lol-gameflow",            # phase, queue and map
    "lol-lobby",               # queue, role preferences, premade party
    "lol-clash",               # enemy roster for scouting
    "lol-ranked",              # own rank, to pick the stats bracket
    "lol-champions",           # owned champions
    "lol-champion-mastery",    # comfort picks
    "lol-summoner",            # current summoner, key for the above
]

# GETs without parameters the sweep must not call anyway, with the reason.
# (Endpoints with any parameter, path or query, are never swept: those are
# lookups by puuid, name or ID.)
SWEEP_EXCLUDE = {
    # Map hidden (ranked) teammates to IDs; Riot forbids de-anonymizing players.
    "/lol-lobby-team-builder/champ-select/v1/session/obfuscated-puuids": "de-anonymizing",
    "/lol-lobby-team-builder/champ-select/v1/session/obfuscated-summoner-ids": "de-anonymizing",
    # Return credentials as bare values that key-based redaction can't catch.
    "/lol-summoner/v1/current-summoner/jwt": "credential",
    "/lol-lobby/v2/comms/token": "credential",
    "/lol-ranked/v1/signed-ranked-stats": "credential",
    "/lol-gameflow/v1/extra-game-client-args": "credential",
    "/lol-clash/v1/player/chat-rosters": "chat",
    # Real puuid next to the obfuscated one for players you muted (hidden in ranked).
    "/lol-champ-select/v1/muted-players": "de-anonymizing",
    # Puuids as bare strings or map keys, which key-based redaction can't see;
    # none of them help drafting.
    "/lol-lobby/v2/party/eog-status": "bare ids",
    "/lol-lobby/v2/comms/members": "bare ids",
    "/lol-ranked/v1/global-notifications": "bare ids",
    "/lol-clash/v1/player/history": "bare ids",
    # Party tokens and chat jid; the useful part duplicates /lol-lobby/v2/lobby.
    "/lol-lobby/v1/parties/player": "credential",
    # The public custom-game browser: other people's free-text lobby names.
    "/lol-lobby/v1/custom-games": "third-party text",
    # Lookups that need a name or puuid query parameter.
    "/lol-summoner/v1/alias/lookup": "lookup",
    "/lol-summoner/v1/summoners": "lookup",
    "/lol-summoner/v2/summoners": "lookup",
    "/lol-summoner/v2/summoner-names-by-puuids": "lookup",
    "/lol-summoner/v2/summoner-icons-by-puuids": "lookup",
    "/lol-ranked/v1/social-leaderboard-ranked-queue-stats-for-puuids": "lookup",
}

ROOT = Path(__file__).resolve().parent.parent
SPEC_DIR = ROOT / "docs" / "lcu-spec"
FULL_PATH = SPEC_DIR / "full" / "swagger.json"
SUBSET_PATH = SPEC_DIR / "swagger.json"
SWEEP_PATH = ROOT / "crates" / "capture" / "sweep-paths.txt"
REF_PREFIX = "#/components/schemas/"


def schema_refs(node):
    """Names of every schema referenced anywhere inside `node`."""
    if isinstance(node, dict):
        ref = node.get("$ref")
        if isinstance(ref, str) and ref.startswith(REF_PREFIX):
            yield ref[len(REF_PREFIX):]
        for value in node.values():
            yield from schema_refs(value)
    elif isinstance(node, list):
        for value in node:
            yield from schema_refs(value)


def subset(spec):
    paths = {
        p: ops for p, ops in spec["paths"].items()
        if p.split("/")[1] in PLUGINS
    }
    schemas = spec["components"]["schemas"]
    keep, todo = set(), list(schema_refs(paths))
    while todo:
        name = todo.pop()
        if name in keep or name not in schemas:
            continue
        keep.add(name)
        todo.extend(schema_refs(schemas[name]))
    out = {k: v for k, v in spec.items() if k not in ("paths", "components", "tags")}
    out["info"] = dict(spec["info"])
    out["info"]["description"] = (
        "Subset of the LCU schema used by draftGPT, built by tools/lcu_spec.py. "
        f"Plugins: {', '.join(PLUGINS)}.\n\n" + spec["info"].get("description", "")
    )
    out["paths"] = dict(sorted(paths.items()))
    out["components"] = {"schemas": {n: schemas[n] for n in sorted(keep)}}
    out["tags"] = [t for t in spec.get("tags", []) if t.get("name", "").split(" ")[-1] in PLUGINS]
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--from", dest="source", type=Path,
                        help="full spec file to use instead of downloading")
    args = parser.parse_args()

    if args.source:
        full_text = args.source.read_text(encoding="utf-8")
    else:
        print(f"downloading {SOURCE_URL}", file=sys.stderr)
        with urllib.request.urlopen(SOURCE_URL, timeout=60) as resp:
            full_text = resp.read().decode("utf-8")
        FULL_PATH.parent.mkdir(parents=True, exist_ok=True)
        FULL_PATH.write_text(full_text, encoding="utf-8")
        print(f"saved full spec to {FULL_PATH.relative_to(ROOT)}", file=sys.stderr)

    spec = json.loads(full_text)
    out = subset(spec)
    SUBSET_PATH.write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    missing = [p for p in PLUGINS if not any(k.split("/")[1] == p for k in out["paths"])]
    print(
        f"{spec['info'].get('version')}: kept {len(out['paths'])}/{len(spec['paths'])} paths, "
        f"{len(out['components']['schemas'])}/{len(spec['components']['schemas'])} schemas "
        f"-> {SUBSET_PATH.relative_to(ROOT)} ({SUBSET_PATH.stat().st_size // 1024} KB)",
        file=sys.stderr,
    )
    if missing:
        print(f"warning: no paths for plugins {missing}", file=sys.stderr)

    sweep = sorted(
        p for p, ops in out["paths"].items()
        if "get" in ops
        and "{" not in p
        and not ops["get"].get("parameters")
        and p not in SWEEP_EXCLUDE
    )
    stale = sorted(set(SWEEP_EXCLUDE) - set(out["paths"]))
    if stale:
        print(f"note: excluded paths no longer in the spec: {stale}", file=sys.stderr)
    SWEEP_PATH.write_text(
        "# GET endpoints snapshotted when a recording starts (capture::sweep).\n"
        "# Generated by tools/lcu_spec.py from docs/lcu-spec/swagger.json; don't edit.\n"
        "# Only GETs without any parameter; exclusions (de-anonymizing, credentials,\n"
        "# chat, bare ids, third-party text) are in SWEEP_EXCLUDE there.\n"
        + "".join(f"{p}\n" for p in sweep),
        encoding="utf-8",
    )
    print(f"{len(sweep)} sweep endpoints -> {SWEEP_PATH.relative_to(ROOT)}", file=sys.stderr)


if __name__ == "__main__":
    main()
