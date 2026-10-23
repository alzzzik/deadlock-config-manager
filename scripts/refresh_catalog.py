"""Updates the "updated" dates in src-tauri/data/config_catalog.json (or another index file).

Repos: date of the last commit (GitHub API through the `gh` CLI, so it uses your login's quota).
GameBanana: upload date of the newest download on the mod page.
Sets "checked" to today. Entries whose source is gone are reported, not removed.

    python scripts/refresh_catalog.py [path/to/index.json]
"""

import datetime
import json
import pathlib
import subprocess
import sys
import urllib.request

DEFAULT = pathlib.Path(__file__).resolve().parent.parent / "src-tauri" / "data" / "config_catalog.json"
GB_API = "https://gamebanana.com/apiv11/Mod/{}?_csvProperties=_aFiles"


def repo_date(repo):
    out = subprocess.run(
        ["gh", "api", f"repos/{repo}/commits?per_page=1", "-q", ".[0].commit.committer.date"],
        capture_output=True, text=True, check=False,
    )
    return out.stdout.strip()[:10] or None


def gamebanana_date(mod_id):
    req = urllib.request.Request(GB_API.format(mod_id), headers={"User-Agent": "refresh_catalog"})
    with urllib.request.urlopen(req, timeout=20) as resp:
        files = json.load(resp).get("_aFiles") or []
    newest = max((f.get("_tsDateAdded", 0) for f in files), default=0)
    return datetime.datetime.fromtimestamp(newest, datetime.UTC).date().isoformat() if newest else None


def main():
    path = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT
    index = json.loads(path.read_text(encoding="utf-8"))
    for entry in index["entries"]:
        try:
            if entry["kind"] == "repo":
                date, name = repo_date(entry["repo"]), entry["repo"]
            elif entry["kind"] == "gamebanana":
                date, name = gamebanana_date(entry["id"]), f"gamebanana {entry['id']}"
            else:
                continue
        except OSError as e:
            date, name = None, f"{entry.get('repo') or entry.get('id')} ({e})"
        if date:
            if date != entry.get("updated"):
                print(f"{name}: {entry.get('updated')} -> {date}")
            entry["updated"] = date
        else:
            print(f"{name}: couldn't read, left as is")
    index["checked"] = datetime.date.today().isoformat()
    path.write_text(json.dumps(index, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
