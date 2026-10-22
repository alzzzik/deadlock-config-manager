"""Builds src-tauri/data/convars.json: descriptions and the "ignored in gameinfo.gi" flag for the
convars that community packs set, from Valve's convar dump in SteamTracking/GameTracking-Deadlock.

    python scripts/gen_convars.py

Plain-language notes in src-tauri/data/convar_notes.json are hand-written and take priority over
Valve's descriptions. Needs network access (GitHub API for the pack lists, ~5 requests).
"""

import json
import re
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "src-tauri" / "data" / "convars.json"
NOTES = ROOT / "src-tauri" / "data" / "convar_notes.json"
DUMP = "https://raw.githubusercontent.com/SteamTracking/GameTracking-Deadlock/master/DumpSource2/convars.txt"
LATEST = "https://api.github.com/repos/SteamTracking/GameTracking-Deadlock/commits?path=DumpSource2/convars.txt&per_page=1"
PACK_REPOS = ["alzzzik/alz-deadlock-cfg", "Sqooky/OptimizationLock", "dacooderr/OptiLock", "7liv/compylock"]
HEADER = re.compile(r"^(\S+) (.*?) \(([^()]*)\)$")
CONVAR_LINE = re.compile(r'^\s*"?([A-Za-z_][A-Za-z0-9_]*)"?\s+"?[^"\s{}]')


def get(url: str) -> str:
    req = urllib.request.Request(url, headers={"User-Agent": "deadlock-config-manager-gen"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.read().decode("utf-8", errors="replace")


def parse_dump(text: str) -> dict:
    """name -> {default, flags, desc} from 'name default (flags)' lines followed by a tab-indented description."""
    out, current = {}, None
    for line in text.splitlines():
        if line.startswith("\t") and current:
            desc = line.strip()
            if desc and desc != "<no description>":
                out[current]["desc"] = (out[current].get("desc", "") + " " + desc).strip()
            continue
        m = HEADER.match(line)
        current = None
        if m:
            current = m.group(1).lower()
            out[current] = {"default": m.group(2), "flags": m.group(3).split()}
    return out


def pack_convars() -> set:
    """Every key set inside a ConVars block of any gameinfo.gi in the known pack repos."""
    names = set()
    for repo in PACK_REPOS:
        tree = json.loads(get(f"https://api.github.com/repos/{repo}/git/trees/HEAD?recursive=1"))
        for item in tree["tree"]:
            if item["type"] != "blob" or not item["path"].lower().endswith("gameinfo.gi"):
                continue
            raw = f"https://raw.githubusercontent.com/{repo}/HEAD/{urllib.parse.quote(item['path'])}"
            text = get(raw)
            block = text.split("ConVars", 1)[1] if "ConVars" in text else ""
            for line in block.splitlines():
                m = CONVAR_LINE.match(line.split("//", 1)[0])
                if m:
                    names.add(m.group(1).lower())
    return names


def main() -> None:
    dump = parse_dump(get(DUMP))
    commit = json.loads(get(LATEST))[0]
    build = commit["commit"]["message"].split("|", 1)[0].strip()
    notes = json.loads(NOTES.read_text(encoding="utf-8"))
    used = pack_convars() | {k.lower() for k in notes}
    ignored = {n for n, d in dump.items() if "gameinfo_cannot_override" in d["flags"]}
    convars = {}
    for name in sorted(used | ignored):
        d = dump.get(name)
        if not d and name not in notes:
            continue
        entry = {}
        if d:
            entry["default"] = d["default"]
            if d.get("desc"):
                entry["valve"] = d["desc"]
        if name in ignored:
            entry["ignored"] = True
        note = notes.get(name)
        if note:
            entry["note"] = note
        convars[name] = entry
    data = {"build": build, "commit": commit["sha"][:10], "convars": convars}
    OUT.write_text(json.dumps(data, indent=1, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(convars)} convars ({len(ignored)} ignored) from build {build} -> {OUT}")


if __name__ == "__main__":
    main()
