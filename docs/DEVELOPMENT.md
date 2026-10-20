# Development

Deadlock Config Manager is a Tauri 2 app: a Rust backend (`src-tauri/`) and a React + TypeScript UI (`src/`).

## Build

Needs Node 18+, Rust (MSVC toolchain), and Visual Studio Build Tools with the Windows SDK.

```
npm install
npx tauri dev        # run with hot reload
npx tauri build      # installer: src-tauri/target/release/bundle/nsis/
```

Run from a Developer PowerShell (or after `vcvars64.bat`) so the MSVC linker is on `PATH`. In Git Bash, its own
`link.exe` shadows the MSVC one.

## Tests

```
cd src-tauri
cargo test                                      # unit tests with fixtures
cargo test --test smoke_github -- --ignored     # live checks against real pack repos
npx tsc --noEmit                                # UI type-check (from the repo root)
```

Manual release checks: [TESTING.md](TESTING.md).

## Releasing

1. Bump the version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`
   (then `npm install --package-lock-only` and `cargo check` to refresh the lock files).
2. Commit, push, then push a tag: `git tag -a v0.3.0 -m "v0.3.0" && git push origin v0.3.0`.
3. The **Release** workflow runs the tests, builds the installer and creates a **draft** release.
   Check it, then publish.

## Source map (`src-tauri/src`)

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

UI components live in `src/components/`; design tokens are at the top of `src/styles.css`.

## For pack authors

A repo works if each pack is a folder (or the repo root) with a `gameinfo.gi` and a `video*.txt`:

- `video.txt` can be a full file or just the setting lines to paste under `DeviceID`.
- Several video files in one folder (e.g. `video(Shadows).txt`, `video(No Shadows).txt`) become separate packs.
- Add a `packs.json` at the repo root for exact names and descriptions:

```json
{ "packs": [
  { "id": "stable", "name": "Stable", "description": "Safe FPS baseline", "folder": "stable" },
  { "id": "experimental", "name": "Experimental", "description": "Research tweaks", "folder": "experimental" }
] }
```
