# Multi-repo packs and UI redesign

Date: 2026-10-20. Builds on the v1 app (`deadlock-cfg-hub-spec.md`).

## Goals

1. Link several GitHub pack repos; show their packs grouped by repo with update time and links.
2. Redesign the status area into a clear files panel.
3. Nicer pack cards, slimmer bottom bar, overall polish (keep the dark brass theme).
4. Create missing `gameinfo.gi` / `video.txt` on Apply instead of refusing.

## 1. Linked repos

- `settings.json`: `repo: String` becomes `repos: Vec<String>` (ordered, `owner/name`).
  Migration: an old `repo` value becomes `repos[0]`. Empty list falls back to `alzzzik/alz-deadlock-cfg`.
- Add: inline "Add repo" field under the last group (accepts `owner/repo` or a github.com URL), and a repo
  list in Settings (add, remove, move up/down). Duplicates are rejected. Adding validates the repo by
  listing its packs; if it has none, show the error and don't add it.
- Remove / reorder: header ⋯ menu (Open on GitHub, Move up, Move down, Remove) and the Settings list.
- `list_packs` returns one group per repo: `{ repo, url, updatedAt, packs, offline, error }`.
  Groups load independently, so one failing repo shows its own error without hiding the others.
- Repo `updatedAt` = newest pack commit date in that repo.
- GitHub budget: 1 tree call + 1 commits call per pack, per repo. Refresh only on app start, on the
  group's refresh button, and after adding a repo. Cached index per repo is used when offline.

## 2. Pack identity and updates

- `PackInfo` gains `repo`, `commitMessage` (first line of the latest commit touching the folder) and
  `htmlUrl` (`https://github.com/{repo}/tree/HEAD/{folder}`).
- `AppliedRecord` gains `repo`. A pack is "applied" only when both `repo` and `id` match. Old records
  without `repo` are treated as the first repo.
- Update available: when the applied pack's current `revision` (latest commit sha) differs from the
  recorded `revision`, the status shows an "Update available" tag and the Re-apply button.

## 3. Files panel (replaces the status card's path rows)

Header: status title (Applied / Applied, modified by DMM / Game updated / No pack applied / Deadlock not found),
one-line detail (repo, applied time), Re-apply button when changed or an update is available.

Rows:

| Row | Content | Actions |
| --- | --- | --- |
| gameinfo.gi | dot, path (tail-truncated) or "Not found" | Locate… (folder picker), Auto (clear override), folder icon (Explorer) |
| video.txt | dot, path or "Will be created on Apply" | Locate…, Auto, folder icon |
| Steam account | dropdown (persona · id), "N accounts found" | (?) help popover, Open userdata |

- The account dropdown lists every numeric folder in `Steam\userdata` (not only ones that have `video.txt`);
  entries without a Deadlock `video.txt` are marked "no video.txt yet".
- Help popover text: the account ID is the folder name under `Steam\userdata`; it equals your Steam friend
  code; Deadlock creates `video.txt` after you launch it once; button "Open userdata folder".

## 4. Creating missing files

- `gameinfo.gi` missing: allowed only when its folder (`...\game\citadel`) exists. Otherwise Apply stays
  blocked with "Deadlock not found. Use Locate…".
- `video.txt` missing (folder may also be missing): Apply writes the pack's `video.txt` with the
  `VendorID` / `DeviceID` lines removed (and any other placeholder lines), creating the `cfg` folder if needed.
  The confirm dialog says the game fills in hardware lines on first launch and may reset some
  graphics settings; re-apply after one launch if so. (Untested assumption about the game's behavior.)
- Backups record a missing file as absent; Restore → Original deletes files that did not exist originally
  (so "Original" truly means "as before the first apply").
- Verification for a created `video.txt`: it parses, has no placeholders.

## 5. Pack cards

Name, description (3-line clamp), latest commit message + date, "View on GitHub" link, Apply / Re-apply.
Applied card: green border and "Applied" badge. Only one filled brass button per view (Re-apply in the
status header when shown, otherwise none: card buttons are outlined, with brass on hover).

## 6. Bottom bar

Restore…, Apply from folder…, spacer, Deadlock running/closed indicator, Settings.
The "Open game folder" / "Open video.txt folder" buttons move into the files panel rows.

## 7. Settings dialog

Sections: Repositories (list + add), Keep from my video.txt (unchanged), Backups (open folder).

## Error handling

- Add repo: invalid input → inline error under the field; repo with no packs → inline error, not added.
- One repo failing to load → error line inside its group with a Retry button; cached packs shown if any.
- Missing-file creation errors surface as toasts with the path.

## Testing

Rust unit tests:
- Settings migration `repo` → `repos`.
- Repo list add/remove/move/dedupe.
- Video creation without user file: placeholder lines dropped, CRLF, parses.
- Backup/restore of an originally-absent file deletes it on restore.
- Applied matching by repo + id; update-available when revision differs.
- Account listing includes accounts without `video.txt`.
UI: manual check in the built app.

## Out of scope

Change preview/diff, background update polling, search/filter, favorites.
