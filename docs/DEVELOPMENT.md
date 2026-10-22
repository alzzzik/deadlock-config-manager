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
npm test                                        # UI unit tests (Vitest, from the repo root)
```

## Releasing

1. Bump the version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`
   (then `npm install --package-lock-only` and `cargo check` to refresh the lock files).
2. Commit, push, then push a tag: `git tag -a v0.3.0 -m "v0.3.0" && git push origin v0.3.0`.
3. The **Release** workflow runs the tests, builds the installer and creates a **draft** release with
   `latest.json`, `SHA256SUMS.txt` and a build attestation. Check it, then publish. Installed copies only see
   the update once the release is published.

Publishing a release also runs the **Packages** workflow:

- **Scoop:** `bucket/deadlock-config-manager.json` is updated to the new version and the portable zip's hash
  and committed to `main`. Users add this repo as a bucket (see the README).
- **winget:** submit the first version by hand with
  `wingetcreate new <installer URL>` (identifier `alzzzik.DeadlockConfigManager`, installer type `nullsoft`,
  scope `user`). After Microsoft merges it, add a `WINGET_TOKEN` secret (classic PAT with `public_repo`); from
  then on each published release opens the winget pull request automatically. Add the
  `winget install alzzzik.DeadlockConfigManager` line to the README once it's listed.

App updates are signed with a [Tauri updater key](https://v2.tauri.app/plugin/updater/). The workflow reads
it from the `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repository secrets and fails
without them. The matching public key is in `tauri.conf.json`. If the private key is lost, installed copies
can't be updated anymore, so keep an offline backup.

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
