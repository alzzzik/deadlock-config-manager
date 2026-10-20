# Release test checklist

Manual checks for a new version, on machines other than the dev PC. Each item says what "pass" looks
like. Copy this file into the release notes or an issue and tick items as you go.

Automated coverage already in place: `cargo test` (61 unit tests: parser, merge, SearchPaths, backups,
restore, rollback) and `cargo test --test smoke_github -- --ignored` (live checks against alz, OptiLock and
compylock). The checks below cover what those can't: real Windows installs, UAC, the game, and the UI.

## Test machines

Try to cover at least three of these. A Windows Sandbox (`.wsb`) or a VM is fine for the "clean" ones.

| # | Machine | Why |
| --- | --- | --- |
| A | Clean Windows 11, Steam + Deadlock installed in the default `C:\Program Files (x86)\Steam` | The common case |
| B | Windows 10 22H2 | Older WebView2 / font rendering |
| C | Deadlock in a **second Steam library** on another drive (e.g. `D:\SteamLibrary`) | libraryfolders.vdf detection |
| D | Steam installed **outside** Program Files (e.g. `D:\Steam`) | Registry detection, no UAC needed |
| E | A PC with **two or more Steam accounts** logged in before | Account picker, MostRecent login |
| F | A PC where Deadlock has **never been launched** on the active account | No `video.txt` yet |
| G | A PC with **Deadlock Mod Manager** and/or **Grimoire** in use | Coexistence with mod managers |
| H | Non-English Windows (e.g. German, Russian) or a username with spaces/non-ASCII letters | Paths with special characters |
| I | Standard (non-admin) Windows user | UAC prompt for the write step |
| J | AMD GPU and NVIDIA GPU machines | Different VendorID/DeviceID kept correctly |

## 1. Download and install

- [ ] Download the installer from the **Releases** page (not a local build). File name is `Deadlock.Config.Manager_x.y.z_x64-setup.exe`.
- [ ] SmartScreen shows "Windows protected your PC"; **More info → Run anyway** works. Note the exact wording for the README.
- [ ] Antivirus (Defender and any other installed) doesn't quarantine the installer or the app. Note any detections.
- [ ] Installer runs **without an admin prompt** and installs for the current user only.
- [ ] Start menu entry "Deadlock Config Manager" exists, with the **violet** icon.
- [ ] Installing over an older version keeps settings, pins, linked repos and backups.
- [ ] Coming from the old "Deadlock Cfg Hub": the old `%APPDATA%\DeadlockCfgHub` folder is moved to `DeadlockConfigManager` on first start (backups and "Original" still listed in Restore).

## 2. First start

- [ ] Splash shows instantly (violet mark, name, moving bar), then fades into the app. No blank window, no layout popping in piece by piece.
- [ ] Without internet on the **first ever** start, the splash still ends within ~3 s and the pack list shows a clear error rather than spinning forever.
- [ ] Title bar, taskbar and Alt+Tab show the violet icon. (If an old icon shows, sign out/in once: Windows icon cache.)
- [ ] Text renders in Geist (crisp sans), paths in Geist Mono. Nothing falls back to Times or a blurry font.
- [ ] Window resizes down to the minimum (720×560) without overlapping or horizontally scrolling.

## 3. Detection

- [ ] Status strip shows `gameinfo.gi` and `video.txt` with green checks on a normal install.
- [ ] **Paths** opens and shows the real paths; it stays open/closed after restarting the app.
- [ ] Machine C: `gameinfo.gi` is found on the second library drive.
- [ ] Machine D: Steam outside Program Files is detected.
- [ ] Machine E: the account dropdown lists every account (persona names where Steam knows them), preselects the most recent login, and switching accounts changes the `video.txt` path.
- [ ] Machine F: `video.txt` shows "will be created" (violet plus icon), and the account is marked "no video.txt yet".
- [ ] **Locate…** for gameinfo.gi accepts the `citadel` folder, the `Deadlock` folder, or the file itself; picking a wrong folder shows a clear message.
- [ ] **Locate…** for video.txt accepts the `cfg` folder.
- [ ] **Auto** forgets a manual path and re-detects.
- [ ] Folder icons open Explorer **at the right folder with the file selected** (not Documents).
- [ ] The **?** next to Account opens the guide; "Open userdata folder" opens `Steam\userdata`.

## 4. Applying packs

For each of: alz Stable, alz Experimental, one OptiLock pack, one OptimizationLock pack, compylock.

- [ ] Clicking **Apply** opens the preview first; nothing is written until **Apply** in the dialog.
- [ ] Preview lists changed settings as old → new, new/removed tags, and sensible counts. Cancel writes nothing (check file modified time).
- [ ] After Apply: success toast, status says "Applied · <pack>", the row shows the green Applied badge.
- [ ] Open `video.txt`: `Version`, `VendorID`, `DeviceID` are **your** values (machine J: correct for AMD and NVIDIA), and resolution/window-mode keys are yours unless unticked in Settings.
- [ ] Open `gameinfo.gi`: `SearchPaths` has `Mod`/`Write` lines for `citadel` and `core`; for OptiLock, `citadel/grimoire` is still there.
- [ ] **Launch Deadlock.** It starts, no "user_keys_default" crash, keybinds work, graphics settings reflect the pack.
- [ ] Machine F: Apply creates `video.txt` (and its `cfg` folder). Launch the game once; check whether settings stuck or the game reset some. **Record the result.** This is the main untested assumption.
- [ ] Machine I: if `gameinfo.gi` isn't writable, **one** UAC prompt appears; accepting writes both files; **declining** writes nothing and says so.
- [ ] A read-only `gameinfo.gi` is made writable again (toast mentions it) so mod managers keep working.
- [ ] **Apply from folder…** works with a local folder containing `gameinfo.gi` + `video.txt`.
- [ ] With Deadlock **running**: the top bar says "Deadlock running" and Apply/Restore refuse with "Close Deadlock first".

## 5. Mod managers and game updates (machine G)

- [ ] Apply a pack, then launch through Deadlock Mod Manager with mods enabled. Game works; refocus the app: status stays "Applied" with "edited by Deadlock Mod Manager".
- [ ] Same with Grimoire, if available.
- [ ] Simulate a game update: Steam → Deadlock → Properties → Installed Files → **Verify integrity**. Refocus the app: status says "Game updated" with a **Re-apply** button, which works.
- [ ] Backups: every Apply adds a backup; the very first one is labelled "Original files".

## 6. Restore

- [ ] **Restore… → Restore original**: both files are byte-identical to before the first apply (compare with a copy you made beforehand, e.g. `fc /b`).
- [ ] Restoring on machine F (where `video.txt` was created) **deletes** the created `video.txt`.
- [ ] Restoring an older backup works; a new "Before restoring…" backup appears.
- [ ] With Deadlock running, restore refuses.

## 7. Repos, pins, search, updates

- [ ] Add each by URL and by `owner/repo`: `dacooderr/OptiLock`, `Sqooky/OptimizationLock`, `7liv/compylock`. Each shows its packs with descriptions and "Updated X ago" per pack.
- [ ] "Popular" chips add the repo in one click and disappear once linked.
- [ ] Adding a repo with no packs, a typo, or a duplicate shows an inline error and doesn't link it.
- [ ] Repo ⋯ menu: Open on GitHub, Move up/down, Remove all work; the order survives a restart.
- [ ] Pin a pack: it appears in **Pinned** (with its repo name) and stays pinned after restart.
- [ ] Search finds packs by name, description, repo and commit message; "No packs match" shows when appropriate.
- [ ] Old packs (3+ months) show the date in amber with an explanatory tooltip.
- [ ] Second start: packs appear **immediately** from the cache, then refresh.
- [ ] Update detection: after a pack author pushes a change (or on a test repo you control), the status shows "Update available · <commit>" and the **Update** button re-applies.

## 8. Offline and rate limits

- [ ] Start with the network disconnected after at least one online run: packs show from cache marked "Offline, cached", and Apply still works for packs you've applied before (others say no cached copy is available).
- [ ] Click refresh many times / link several repos: no blank dates or errors under normal use. If GitHub's hourly limit is hit, existing lists stay visible.

## 9. Settings

- [ ] Unticking a "Keep from my video.txt" key makes the next apply use the pack's value for it.
- [ ] **Open backups folder** opens `%APPDATA%\DeadlockConfigManager\backups`.

## 10. Uninstall

- [ ] Uninstall from Windows Settings → Apps works without admin.
- [ ] Game files remain as last applied (uninstalling doesn't touch the game). Decide whether that's the desired behaviour and document it in the README.
- [ ] `%APPDATA%\DeadlockConfigManager` remains (backups survive); reinstalling picks them up.

## Reporting a failure

Note the machine letter, Windows version, the exact toast text, and attach:
`%APPDATA%\DeadlockConfigManager\settings.json` (no secrets in it), and the current `gameinfo.gi` / `video.txt`.
