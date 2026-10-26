<div align="center">

<img src="src-tauri/icons/128x128.png" width="88" alt="Deadlock Config Manager icon" />

# Deadlock Config Manager

A Windows app for installing Deadlock FPS configs from GitHub.

[![Latest release](https://img.shields.io/github/v/release/alzzzik/deadlock-config-manager?label=release&color=9b87f5)](https://github.com/alzzzik/deadlock-config-manager/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/alzzzik/deadlock-config-manager/total?color=9b87f5)](https://github.com/alzzzik/deadlock-config-manager/releases)
[![Windows 10/11](https://img.shields.io/badge/Windows-10%20%7C%2011-2b2f36)](#install)
[![License: MIT](https://img.shields.io/badge/license-MIT-2b2f36)](LICENSE)

<a href="https://github.com/alzzzik/deadlock-config-manager/releases/latest">
  <img src="https://img.shields.io/badge/Download%20for%20Windows-9b87f5?style=for-the-badge&logoColor=white" alt="Download for Windows" height="36" />
</a>

<br /><br />

<img src="docs/images/home.png" width="760" alt="The main window: current config at the top, configs grouped by GitHub repo below" />

</div>

<br />

Installs a config's `gameinfo.gi` and `video.txt` without editing files by hand: your GPU lines stay, every change
is previewed and backed up, and the config is put back after game updates.

## Features

- Shows every setting a config changes before anything is written, and marks the ones the game ignores.
- Keeps your GPU IDs and display settings from your own `video.txt`.
- Backs up your files before every change. The first backup is kept as "Original".
- Re-applies your config after game updates, in one click or automatically in tray mode, using the latest version from its repo.
- Installs configs from GitHub repos, GameBanana links and share codes. Your own configs are kept in "My configs".
- Shows real-game FPS per config from Deadlock's own reports. Needs the `-condebug` launch option.
- Offers to restore your previous files if Deadlock closes right after a change.

## Settings

<img src="docs/images/settings.png" width="380" align="right" alt="The Settings window: linked repos and the video.txt values to keep" />

Version, VendorID and DeviceID always come from your `video.txt`. Resolution, refresh rate, monitor and window mode
also do by default; untick any of them to use the config's value. Add, reorder and remove linked repos here too.

<br clear="right" />

## Install

1. Download `Deadlock.Config.Manager_x.y.z_x64-setup.exe` from the
   [latest release](https://github.com/alzzzik/deadlock-config-manager/releases/latest).
2. Run it. It installs for your user only and needs no admin rights.
3. If Windows shows "Windows protected your PC", click **More info**, then **Run anyway**. The installer isn't
   code-signed. See [how to check a download](#how-do-i-know-the-installer-is-safe) and the
   [code signing policy](#code-signing-policy).

From v0.3.0 on, the app offers new versions and installs them in one click.

With [Scoop](https://scoop.sh) instead (portable copy, updated by Scoop):

```
scoop bucket add deadlock-config-manager https://github.com/alzzzik/deadlock-config-manager
scoop install deadlock-config-manager
```

To uninstall, use **Settings → Apps → Installed apps** in Windows. Use **Restore original** first if you want your
game files back. `%APPDATA%\DeadlockConfigManager\` (settings, backups) stays unless you delete it.

## Usage

1. Open the app. It finds your Deadlock install, `video.txt` and Steam account.
2. Click **Apply** on a config, check the changes, and confirm.
3. Start Deadlock. To go back, click **Restore…** and pick **Restore original**.

Close Deadlock before applying or restoring.

## Supported repo layouts

The app reads any GitHub repo with one of these layouts. Folders are read from the top level of the repo only.

- **Config folders:** each has a `gameinfo.gi` and an optional `video.txt`. Example:
  [alz-deadlock-cfg](https://github.com/alzzzik/alz-deadlock-cfg) (`stable/`, `alz/`).
- **Config at the repo root:** `gameinfo.gi` and an optional `video.txt` next to the README. Example:
  [compylock](https://github.com/7liv/compylock).
- **Gameinfo variants:** extra files like `gameinfo-EXTREME-LOW.gi` in a folder become their own configs.
  Example: [OptimizationLock](https://github.com/Sqooky/OptimizationLock).
- **Video variants:** several `video*.txt` files in a folder, like `video(No Shadows).txt`, become their own configs.
  Example: [OptiLock](https://github.com/dacooderr/OptiLock) (`OptiLock Ultra Potato Config`).
- **`packs.json`:** at the repo root, sets exact config names, descriptions and folders.

**Browse…** lists more repos and GameBanana configs, recommended first. [OptiLock](https://github.com/dacooderr/OptiLock)
and [alz-deadlock-cfg](https://github.com/alzzzik/alz-deadlock-cfg) are linked by default; remove them in Settings.
Link another repo by pasting its `owner/repo` or GitHub URL under **Link another pack repo** in Settings.

Config authors: see [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md#for-config-authors), including `video.txt` files that
hold only the setting lines.

## FAQ

<details>
<summary><b>Can I get banned for this?</b></summary>

The app only edits the files you'd edit by hand: `gameinfo.gi`, `video.txt`, and `autoexec.cfg` for configs that
include one. I haven't seen reports of bans
for config changes. Valve can refuse ranked matchmaking when `gameinfo.gi` changes certain engine sections, though,
and some configs do that. See the next question.
</details>

<details>
<summary><b>I can't queue for ranked. What now?</b></summary>

If Deadlock refuses ranked matchmaking because of `gameinfo.gi`, the app shows **Make ranked-safe**. It keeps the
config's other settings and resets the checked engine sections to Valve's defaults. You can also pick
**Apply ranked-safe** in any config's preview. The app reads the refusal from Deadlock's log, which needs the
`-condebug` launch option. If you still can't queue, try another config or restore your original.
</details>

<details>
<summary><b>How do I undo everything?</b></summary>

Use **Restore…** and pick **Restore original**. That puts back the exact files from before your first apply.
Verifying the game files in Steam also restores Valve's default `gameinfo.gi`.
</details>

<details>
<summary><b>Does it work with Deadlock Mod Manager or Grimoire?</b></summary>

Yes. It keeps the search paths those tools add, including Grimoire's `citadel/grimoire`, and doesn't make files
read-only.

- Install or update mods first, then apply a config. If a mod manager rewrites `gameinfo.gi` later, the app says
  which tool did it and offers **Re-apply**.
- Deadlock Mod Manager 2.0 can also apply performance presets. Use one tool for configs, not both.
</details>

<details>
<summary><b id="how-do-i-know-the-installer-is-safe">How do I know the installer is safe?</b></summary>

Every release is built from this repo's source by GitHub Actions and lists SHA-256 checksums in `SHA256SUMS.txt`.
In PowerShell, `Get-FileHash .\Deadlock.Config.Manager_*_x64-setup.exe` should print the same value. Each installer
has a GitHub build attestation: `gh attestation verify <file> --repo alzzzik/deadlock-config-manager`. You can also
upload it to [VirusTotal](https://www.virustotal.com) before running it.
</details>

## Privacy policy

Deadlock Config Manager doesn't collect, store or send personal data. It has no telemetry, accounts or background
service. It connects only to:

- **GitHub** (`github.com`, `api.github.com`, `raw.githubusercontent.com`): to list and download configs from your
  linked repos, check this repo's latest release for updates, and download Valve's unmodified `gameinfo.gi` from
  [GameTracking-Deadlock](https://github.com/SteamTracking/GameTracking-Deadlock) for the ranked-safety check.
- **jsDelivr** (`data.jsdelivr.com`, `cdn.jsdelivr.net`): to list files in a linked repo and read the Browse… list.
- **GameBanana** (`gamebanana.com`): only when you add a GameBanana config.

`dacooderr/OptiLock` and `alzzzik/alz-deadlock-cfg` are linked by default and can be removed in Settings. Requests to
GitHub are covered by [GitHub's privacy statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement).

Everything else stays on your PC. Settings, backups, downloaded configs and a small log are in
`%APPDATA%\DeadlockConfigManager\`, along with `perf.json` (Performance FPS numbers). Performance reads Deadlock's
`console.log`, which the game writes only with `-condebug`. The app changes only `gameinfo.gi`, `video.txt`, and
`cfg\autoexec.cfg` for configs that have one, and only after showing you the changes.

## Code signing policy

Windows installers are built from the source in this repository by
[GitHub Actions](.github/workflows/release.yml) and published on the
[Releases](https://github.com/alzzzik/deadlock-config-manager/releases) page. Every release is approved by hand
before it's published.

Releases are not code-signed yet. Each lists SHA-256 checksums and has a GitHub build attestation instead. App
updates are signed with a separate update key, and installed copies refuse updates that aren't.

Team roles:

- Committers and reviewers: [alzzzik](https://github.com/alzzzik)
- Approvers: [alzzzik](https://github.com/alzzzik)

## Contributing

Bug reports and repo suggestions go in [Issues](https://github.com/alzzzik/deadlock-config-manager/issues). For bugs,
paste the output of **Settings → Copy diagnostics** (your Windows user name is removed). Build instructions and code
layout are in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

---

<sub>Not affiliated with Valve. Deadlock is a trademark of Valve Corporation. MIT licensed.</sub>
