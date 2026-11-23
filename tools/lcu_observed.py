#!/usr/bin/env python3
"""What the recordings show that the schema doesn't: docs/lcu-spec/help/observed.json.

Walks every event in fixtures/*.jsonl alongside its declared type (the newest
/help dump in docs/lcu-spec/help/, else docs/lcu-spec/swagger.json) and keeps,
per `Type.field`, only what the schema leaves out:

- values: the strings seen in enum-like fields (few distinct, short, not IDs);
  with `undeclared` when the schema declares an enum that lacks some of them
- negatives: negative numbers seen (sentinels such as championId -1)
- null: null seen in a field the schema calls required
- shape: arrays of arrays where the schema says `vector of object`
- undocumented: fields the schema doesn't have
- note: hand-written, from docs/lcu-spec/help/notes.json (same keys)

Inputs are the redacted main recordings only; nothing here reads wire or sweep
files. Run from the repo root: python3 tools/lcu_observed.py
"""

import json
import re
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HELP = ROOT / "docs/lcu-spec/help"
SWAGGER = ROOT / "docs/lcu-spec/swagger.json"
FIXTURES = ROOT / "fixtures"
OUT = HELP / "observed.json"
NOTES = HELP / "notes.json"

MAX_VALUES = 24
MAX_LEN = 48
# Fields whose strings are names, IDs or free text even when they repeat.
SKIP_FIELD = re.compile(r"(^|[a-z])(Id|Ids|Name|name|Uuid|uuid|puuid|Puuid|Key|Url|url|Path|Icon|Message|Text|Description)$|^(id|name|puuid|uri)$")
ID_VALUE = re.compile(r"^redacted|^[0-9a-f-]{16,}$|\d{6,}")


def version_key(path):
    return [int(x) for x in re.findall(r"\d+", path.name)]


class Schema:
    """Types by name ({"fields": {name: spec}} or {"enum": [...]}) and GET routes."""

    def __init__(self):
        self.types = {}
        self.routes = []  # (regex, return spec)

    def route(self, uri):
        for rx, spec in self.routes:
            if rx.fullmatch(uri):
                return spec
        return None

    def add_route(self, url, spec):
        pattern = re.sub(r"\\\{[^}]*\\\}", "[^/]+", re.escape(url))
        self.routes.append((re.compile(pattern), spec))


def load_help(path, schema):
    """The /help Console format: specs are type-name strings or inline {Name: {fields|values}}."""
    details = json.loads(path.read_text())["details"]

    def collect(v):
        if isinstance(v, dict):
            for k, x in v.items():
                if isinstance(x, dict) and set(x) == {"fields"}:
                    schema.types.setdefault(k, {"fields": {
                        n: {"spec": norm(f["type"]), "optional": f.get("optional", True)}
                        for item in x["fields"] for n, f in item.items()}})
                elif isinstance(x, dict) and set(x) == {"values"}:
                    schema.types.setdefault(k, {"enum": [e["name"] for e in x["values"]]})
                collect(x)
        elif isinstance(v, list):
            for x in v:
                collect(x)

    collect(details)
    for f in details["functions"].values():
        for c in (f.get("console") or {}).values():
            if c.get("http_method") == "GET" and c.get("url"):
                schema.add_route(c["url"], norm(c.get("returns")))


def norm(spec):
    """Help spec → a type name, "vector of X", "map of X" or a primitive."""
    if isinstance(spec, dict):
        return next(iter(spec), "object")
    return spec or "object"


def load_swagger(path, schema):
    doc = json.loads(path.read_text())

    def spec(s):
        if "$ref" in s:
            return s["$ref"].rsplit("/", 1)[-1]
        if s.get("type") == "array":
            return "vector of " + spec(s.get("items", {}))
        if s.get("type") == "object" and "additionalProperties" in s:
            ap = s["additionalProperties"]
            return "map of " + (spec(ap) if isinstance(ap, dict) else "object")
        return s.get("format") or s.get("type") or "object"

    for name, s in doc["components"]["schemas"].items():
        if "enum" in s:
            schema.types.setdefault(name, {"enum": s["enum"]})
        elif "properties" in s:
            schema.types.setdefault(name, {"fields": {
                n: {"spec": spec(p), "optional": True} for n, p in s["properties"].items()}})
    for url, ops in doc["paths"].items():
        get = ops.get("get", {})
        content = get.get("responses", {}).get("2XX", {}).get("content", {})
        s = content.get("application/json", {}).get("schema")
        if s:
            schema.add_route(url, spec(s))


class Observed:
    def __init__(self, schema):
        self.schema = schema
        self.entries = defaultdict(lambda: {"strings": defaultdict(int), "negatives": set(),
                                            "null": False, "shape": None, "versions": set()})
        self.specs = {}
        self.undocumented = set()

    def walk(self, key, spec, value, version, required=False):
        e = self.entries[key]
        e["versions"].add(version)
        self.specs.setdefault(key, spec)
        if value is None:
            e["null"] |= required
            return
        if isinstance(value, bool):
            return
        if isinstance(value, (int, float)):
            if value < 0:
                e["negatives"].add(value)
            return
        if isinstance(value, str):
            e["strings"][value] += 1
            return
        if isinstance(value, list):
            inner = spec[len("vector of "):] if spec and spec.startswith("vector of ") else None
            if spec == "vector of object" and any(isinstance(x, list) for x in value):
                e["shape"] = "array of arrays"
                inner = None
            for x in value:
                self.walk(key + "[]", inner, x, version)
            return
        t = self.schema.types.get(spec or "")
        if spec and spec.startswith("map of "):
            for x in value.values():
                self.walk(key + "{}", spec[len("map of "):], x, version)
            return
        fields = t.get("fields") if t else None
        for name, x in value.items():
            if fields is not None and name in fields:
                f = fields[name]
                self.walk(f"{spec}.{name}", f["spec"], x, version, not f["optional"])
            else:
                if fields is not None:
                    self.undocumented.add(f"{spec}.{name}")
                self.walk(f"{key}.{name}" if fields is None else f"{spec}.{name}", None, x, version)

    def result(self, notes):
        out = {}
        for key, e in sorted(self.entries.items()):
            spec = self.specs.get(key)
            r = {}
            values = enum_like(key, e["strings"])
            if values:
                r["values"] = values
                declared = (self.schema.types.get(spec or "") or {}).get("enum")
                if declared:
                    extra = [v for v in values if v not in declared]
                    if extra:
                        r["undeclared"] = extra
            if e["negatives"]:
                r["negatives"] = sorted(e["negatives"])
            if e["null"]:
                r["null"] = True
            if e["shape"]:
                r["shape"] = e["shape"]
            if key in self.undocumented:
                r["undocumented"] = True
            if r:
                if spec:
                    r["spec"] = spec
                r["seen_in"] = sorted(e["versions"])
                out[key] = r
        for key, note in notes.items():
            if key not in self.entries and key not in self.schema.types:
                print(f"warning: notes.json key {key!r} matches no field seen in the fixtures")
            out.setdefault(key, {})["note"] = note
        return dict(sorted(out.items()))


def enum_like(key, counts):
    field = re.split(r"[.\[\]{}]+", key.rstrip("[]{}"))[-1]
    if not counts or SKIP_FIELD.search(field) or len(counts) > MAX_VALUES:
        return None
    if any(len(v) > MAX_LEN or ID_VALUE.search(v) for v in counts):
        return None
    if len(counts) > 3 and all(n == 1 for n in counts.values()):
        return None  # every value once: free text, not an enum
    if sum(any(c.isdigit() for c in v) for v in counts) * 2 > len(counts):
        return None  # numbers or counters as strings (kda "2.5", "1:16:1")
    return sorted(counts)


def main():
    schema = Schema()
    dumps = sorted(HELP.glob("client-*.help.json"), key=version_key)
    if dumps:
        load_help(dumps[-1], schema)
    load_swagger(SWAGGER, schema)
    obs = Observed(schema)
    files = sorted(FIXTURES.glob("*.jsonl"))
    versions = set()
    for path in files:
        lines = path.read_text().splitlines()
        version = json.loads(lines[0]).get("client_version") or "unknown"
        versions.add(version)
        for line in lines[1:]:
            ev = json.loads(line)
            if ev.get("data") is None:
                continue
            spec = schema.route(ev["uri"])
            root = spec or "uri:" + re.sub(r"/(redacted-\d+|\d+)(?=/|$)", "/{id}", ev["uri"])
            obs.walk(root, spec, ev["data"], version)
    notes = json.loads(NOTES.read_text()) if NOTES.exists() else {}
    fields = obs.result(notes)
    OUT.write_text(json.dumps({
        "generated_by": "tools/lcu_observed.py",
        "schema": [p.name for p in dumps[-1:]] + [SWAGGER.name],
        "fixtures": len(files),
        "client_versions": sorted(versions),
        "fields": fields,
    }, indent=1, ensure_ascii=False) + "\n")
    print(f"wrote {OUT.relative_to(ROOT)}: {len(fields)} fields from {len(files)} fixtures")


if __name__ == "__main__":
    main()
