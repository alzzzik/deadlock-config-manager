# Deadlock Config Manager

A small Windows app that installs **Deadlock FPS config packs** (`gameinfo.gi` + `video.txt`) from GitHub
with one click, keeps your own GPU and resolution lines, backs everything up first, and can put your
original files back at any time.

> Not affiliated with or endorsed by Valve. Deadlock is a trademark of Valve Corporation.

## Features

- **One-click apply** from any linked GitHub pack repo, with a preview of every setting that will change.
- **Keeps your hardware lines**: `Version`, `VendorID`, `DeviceID` are always kept from your own `video.txt`;
  resolution, refresh rate, monitor and window mode are kept too unless you untick them in Settings.
- **Safe**: a backup is taken before every write, the first one is kept forever as your *Original*, and
  Restore puts any backup back exactly as it was.
- **Game-update aware**: notices when a Deadlock update replaces `gameinfo.gi` and offers *Re-apply*.
  Edits made by Deadlock Mod Manager are recognised and left alone.
- **Linked repos**: add any pack repo (`owner/repo` or a GitHub link). Each pack shows its latest change and
  how long ago its folder was updated, and packs that haven't changed in months are flagged.
- **Pins, search, update checks**: pin favourite packs, search across repos, and get a heads-up when the pack
  you applied has a new version.
- **Mod-manager friendly**: keeps the `Mod`/`Write` search paths the game needs without removing extra
  paths that mod managers (Deadlock Mod Manager, Grimoire) add, and never makes files read-only.

## Install

1. Download `Deadlock Config Manager_x.y.z_x64-setup.exe` from [Releases](../../releases).
2. Run it. It installs for your Windows user only and doesn't need admin rights.
3. Windows may show **"Windows protected your PC"** because the app isn't code-signed yet.
   Click **More info → Run anyway**.

Windows 10/11, 64-bit. Close Deadlock before applying or restoring.

## Using it

1. Check the **files panel** at the top: `gameinfo.gi`, `video.txt` and your Steam account are detected
   automatically. Use **Locate…** to pick a folder by hand, **Auto** to detect again. The **?** next to
   *Steam account* explains how to find yours.
2. Pick a pack and click **Apply**. Review the list of changes, then confirm.
3. Launch Deadlock. To undo, use **Restore… → Restore original**.

If your account has no `video.txt` yet (Deadlock never launched on it), Apply creates one from the pack
without the GPU lines; the game fills those in on first launch.

`gameinfo.gi` usually lives in `Program Files (x86)`. Windows normally lets your account write there; if it
doesn't, you'll see one administrator prompt for the write step only.

## Pack repos

Linked by default: [alzzzik/alz-deadlock-cfg](https://github.com/alzzzik/alz-deadlock-cfg). Also tested with
[dacooderr/OptiLock](https://github.com/dacooderr/OptiLock),
[Sqooky/OptimizationLock](https://github.com/Sqooky/OptimizationLock) and
[7liv/compylock](https://github.com/7liv/compylock).

A repo works if each pack is a folder (or the repo root) with a `gameinfo.gi` and a `video*.txt`:

- `video.txt` can be a full file or just the setting lines to paste under `DeviceID`.
- Several video files in one folder (e.g. `video(Shadows).txt`, `video(No Shadows).txt`) become separate packs.
- Pack authors can add a `packs.json` at the repo root for exact names and descriptions:

```json
{ "packs": [
  { "id": "stable", "name": "Stable", "description": "Safe FPS baseline", "folder": "stable" },
  { "id": "experimental", "name": "Experimental", "description": "Research tweaks", "folder": "experimental" }
] }
```

## Privacy

No accounts, no telemetry, no background service. The app only talks to GitHub (`github.com`,
`api.github.com`, `raw.githubusercontent.com`) to list and download packs. Settings, backups and the download
cache live in `%APPDATA%\DeadlockConfigManager\`.

## Building from source

Needs Node 18+, Rust (MSVC toolchain), and Visual Studio Build Tools with the Windows SDK.

```
npm install
npx tauri build      # installer: src-tauri/target/release/bundle/nsis/
```

Run from a Developer PowerShell (or after `vcvars64.bat`) so the MSVC linker is on `PATH`.

Tests:

```
cd src-tauri
cargo test                                      # unit tests with fixtures
cargo test --test smoke_github -- --ignored     # live checks against real pack repos
```

Releases: push a tag like `v0.1.0` and the GitHub Actions workflow builds the installer and drafts a release.

### Source map (`src-tauri/src`)

| File | Role |
| --- | --- |
| `kv.rs` | Valve KeyValues tokenizer/parser with byte offsets |
| `gameinfo.rs` | SearchPaths checks, PGIVersion, Mod Manager-aware comparison |
| `video.rs` | `video.txt` merge (full files and fragments) |
| `diff.rs` | "What will change" preview |
| `paths.rs` | Steam, library and account detection |
| `github.rs` | GitHub HTTP: ETag cache, commit feeds |
| `packs.rs` | Pack discovery, download and cache |
| `repos.rs` | Linked repo list and pins |
| `backup.rs` | Backups, pruning, restore |
| `writer.rs` | Atomic writes and the elevated write helper |
| `apply.rs` | Build, verify, roll back, status |
| `overview.rs`, `commands.rs` | Tauri commands used by the UI |

## License

[MIT](LICENSE)
