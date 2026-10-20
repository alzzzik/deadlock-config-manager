<div align="center">

<img src="src-tauri/icons/128x128.png" width="88" alt="Deadlock Config Manager icon" />

# Deadlock Config Manager

**Install Deadlock FPS configs in one click. Keep your own settings. Undo anytime.**

[![Latest release](https://img.shields.io/github/v/release/alzzzik/deadlock-config-manager?label=release&color=9b87f5)](https://github.com/alzzzik/deadlock-config-manager/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/alzzzik/deadlock-config-manager/total?color=9b87f5)](https://github.com/alzzzik/deadlock-config-manager/releases)
[![Windows 10/11](https://img.shields.io/badge/Windows-10%20%7C%2011-2b2f36)](#install)
[![License: MIT](https://img.shields.io/badge/license-MIT-2b2f36)](LICENSE)

<a href="https://github.com/alzzzik/deadlock-config-manager/releases/latest">
  <img src="https://img.shields.io/badge/Download%20for%20Windows-9b87f5?style=for-the-badge&logoColor=white" alt="Download for Windows" height="36" />
</a>

<br /><br />

<img src="docs/images/home.png" width="760" alt="Deadlock Config Manager: status at the top, config packs grouped by GitHub repo below" />

</div>

<br />

Community FPS configs for Deadlock usually mean copying `gameinfo.gi` into the game folder, then hand-editing
`video.txt` without breaking your GPU lines, and redoing it all after every patch. This app does it for you,
safely.

## Why use it

- **See before you apply.** A preview lists every setting that will change, old → new. Nothing is written
  until you confirm.
- **Your PC stays yours.** Your GPU IDs, resolution, refresh rate and window mode are kept from your own
  `video.txt`.
- **Undo anytime.** A backup is taken before every change, and *Restore original* puts your files back
  exactly as they were.
- **Survives game updates.** When a Deadlock patch replaces `gameinfo.gi`, the app notices and offers
  *Re-apply*.
- **Any pack, any repo.** Pick from popular config repos or paste any GitHub link. Each pack shows when it was
  last updated, so you can spot outdated ones.
- **Plays nice with mods.** Works alongside Deadlock Mod Manager and Grimoire, and keeps the search paths the
  game needs to start.

## Your settings, kept

<img src="docs/images/settings.png" width="380" align="right" alt="Settings: linked pack repos and the video.txt values the app keeps from your PC" />

Packs are made on someone else's PC. The app always keeps your `Version`, `VendorID` and `DeviceID`, and by
default also your resolution, refresh rate, monitor and window mode. Untick anything in **Settings** to take
the pack's value instead.

Settings is also where you manage linked repos: add one by pasting a GitHub link, reorder or remove them, or
pick from the popular ones.

<br clear="right" />

## Install

1. Download **`Deadlock.Config.Manager_x.y.z_x64-setup.exe`** from the
   [latest release](https://github.com/alzzzik/deadlock-config-manager/releases/latest).
2. Run it. It installs for your Windows user only, so no admin rights are needed.
3. If Windows says *"Windows protected your PC"*, click **More info → Run anyway**. That appears because the
   app isn't code-signed yet; the installer is built from this repo by
   [GitHub Actions](https://github.com/alzzzik/deadlock-config-manager/actions).

## How it works

1. **Open the app.** It finds Deadlock, your `video.txt` and your Steam account on its own.
2. **Pick a pack** and click **Apply**. Check the list of changes, then confirm.
3. **Launch Deadlock.** To undo, use **Restore… → Restore original**.

## Config packs

| Repo | Packs |
| --- | --- |
| [alzzzik/alz-deadlock-cfg](https://github.com/alzzzik/alz-deadlock-cfg) | Stable, Experimental (linked by default) |
| [Sqooky/OptimizationLock](https://github.com/Sqooky/OptimizationLock) | Many community presets |
| [dacooderr/OptiLock](https://github.com/dacooderr/OptiLock) | Recommended, Potato, Ultra Potato |
| [7liv/compylock](https://github.com/7liv/compylock) | Competitive-focused config |

Add any other repo by pasting its GitHub link. Pack authors: see
[the pack format](docs/DEVELOPMENT.md#for-pack-authors).

## FAQ

<details>
<summary><b>Can this get me banned?</b></summary>

The app only edits the same two text files people edit by hand (`gameinfo.gi` and `video.txt`). It doesn't
touch the game's code or memory. There are no known bans for config edits. Note that Valve's matchmaking
refuses to queue if `gameinfo.gi` changes certain engine sections, and some packs do; if you can't queue,
switch to a different pack or use *Restore original*.
</details>

<details>
<summary><b>How do I undo everything?</b></summary>

**Restore… → Restore original** puts back the exact files from before your first apply. You can also verify
the game files in Steam (Deadlock → Properties → Installed Files) to get Valve's stock `gameinfo.gi`.
</details>

<details>
<summary><b>A Deadlock update reset my config.</b></summary>

Game updates replace `gameinfo.gi`. Open the app; it shows *Game updated* with a **Re-apply** button.
</details>

<details>
<summary><b>I use Deadlock Mod Manager or Grimoire.</b></summary>

That's fine. The app keeps the extra search paths those tools add, never makes files read-only, and
recognises Deadlock Mod Manager's edits so it doesn't report them as a problem.
</details>

<details>
<summary><b>What does the app send over the internet?</b></summary>

Only requests to GitHub, to list and download packs. No accounts, no telemetry, no background service.
Settings and backups stay in `%APPDATA%\DeadlockConfigManager\`.
</details>

## Contributing

Bug reports and pack-repo suggestions are welcome in
[Issues](https://github.com/alzzzik/deadlock-config-manager/issues). Building from source, tests and the
code layout are in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md); plans are in [docs/ROADMAP.md](docs/ROADMAP.md).

---

<sub>Not affiliated with or endorsed by Valve. Deadlock is a trademark of Valve Corporation. MIT licensed.</sub>
