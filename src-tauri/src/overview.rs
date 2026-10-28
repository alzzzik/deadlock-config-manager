//! Path/account detection shown in the files panel, plus the settings and repo-list commands.

use crate::apply::{self, Status};
use crate::commands::AppState;
use crate::packs::{self, RepoGroup};
use crate::paths::{self, Account};
use crate::repos;
use crate::settings::{AppliedRecord, Settings};
use crate::video;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::State;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub steam_root: Option<String>,
    pub gameinfo_path: Option<String>,
    pub gameinfo_found: bool,
    /// File missing but its folder exists, so Apply can create it.
    pub gameinfo_creatable: bool,
    pub video_path: Option<String>,
    pub video_found: bool,
    pub accounts: Vec<Account>,
    pub account_id: Option<String>,
    pub status: Status,
    pub applied: Option<AppliedRecord>,
    pub deadlock_running: bool,
    pub repos: Vec<String>,
    pub pinned: Vec<String>,
    pub preserved_keys: Vec<String>,
    pub optional_preserved: Vec<String>,
    pub always_preserved: Vec<String>,
    /// Engine-section entries in the installed gameinfo.gi that differ from Valve's; None if unreadable.
    pub ranked_changes: Option<usize>,
    /// Show the first-run guide: never finished or skipped, and nothing applied yet.
    pub show_guide: bool,
    /// Who replaced the applied gameinfo.gi when status is "changed": "game", "dmm", "grimoire" or "other".
    pub changed_by: Option<String>,
    /// Steam build id of the installed game.
    pub game_build: Option<String>,
    /// console.log shows Deadlock refused ranked matchmaking because of gameinfo.gi.
    pub ranked_blocked: bool,
    /// ConVars lines changed on top of the applied config (from Settings or by hand); 0 when none.
    pub tweaks: usize,
    /// Suggested name when saving those changes, e.g. "alz + tweaks #1".
    pub tweak_name: Option<String>,
    /// The changed lines joined, for grouping Performance sessions; empty when none.
    #[serde(skip)]
    pub tweak_fingerprint: String,
}

pub fn to_string(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Resolves paths: manual overrides first, then auto-detection.
pub fn resolve(s: &Settings) -> Overview {
    let steam_root = s.steam_root.as_ref().map(PathBuf::from).filter(|p| p.is_dir()).or_else(paths::detect_steam_root);
    let gameinfo = match &s.gameinfo_path {
        Some(p) => Some(PathBuf::from(p)),
        None => steam_root.as_deref().and_then(paths::detect_gameinfo),
    };
    let accounts = steam_root.as_deref().map(paths::list_accounts).unwrap_or_default();
    let account_id = s
        .account_id
        .clone()
        .filter(|id| accounts.iter().any(|a| &a.id == id))
        .or_else(|| steam_root.as_deref().and_then(|r| paths::preferred_account(&paths::read_loginusers(r), &accounts)));
    let video = match (&s.video_path, &steam_root, &account_id) {
        (Some(p), _, _) => Some(PathBuf::from(p)),
        (None, Some(root), Some(id)) => Some(paths::video_path_for(root, id)),
        _ => None,
    };
    let gameinfo_found = gameinfo.as_ref().is_some_and(|p| p.is_file());
    Overview {
        steam_root: steam_root.as_deref().map(to_string),
        gameinfo_found,
        gameinfo_creatable: !gameinfo_found && gameinfo.as_ref().and_then(|p| p.parent()).is_some_and(Path::is_dir),
        status: apply::status(gameinfo.as_deref(), s.applied.as_ref()),
        gameinfo_path: gameinfo.as_deref().map(to_string),
        video_found: video.as_ref().is_some_and(|p| p.is_file()),
        video_path: video.as_deref().map(to_string),
        accounts,
        account_id,
        applied: s.applied.clone(),
        deadlock_running: crate::fsops::deadlock_running(),
        repos: s.effective_repos(),
        pinned: s.pinned.clone(),
        preserved_keys: s.preserved_keys.clone(),
        optional_preserved: video::DEFAULT_PRESERVED.iter().map(|k| k.to_string()).collect(),
        always_preserved: video::ALWAYS_PRESERVED.iter().map(|k| k.to_string()).collect(),
        ranked_changes: None,
        show_guide: !s.onboarded && s.applied.is_none(),
        changed_by: None,
        game_build: None,
        ranked_blocked: false,
        tweaks: 0,
        tweak_name: None,
        tweak_fingerprint: String::new(),
    }
}

/// Best guess at who changed an applied gameinfo.gi. Mod manager markers win. Otherwise it's a game
/// update when the ConVars match Valve's file, or when the build changed and none of the config's own
/// ConVars changes (`applied`: the config's file) are left. Anything else is "other", which the
/// background re-apply never touches.
pub fn changed_by(current: &str, stock: &str, applied: Option<&str>, applied_build: Option<&str>, game_build: Option<&str>) -> &'static str {
    if crate::gameinfo::has_dmm_markers(current) {
        return "dmm";
    }
    if current.to_ascii_lowercase().contains("grimoire") {
        return "grimoire";
    }
    let convars = |src: &str| -> Vec<(String, String)> {
        crate::kv::parse(src)
            .map(|n| crate::diff::flatten(&n))
            .unwrap_or_default()
            .into_iter()
            .filter(|(k, _)| crate::convars::convar_of(k).is_some())
            .collect()
    };
    let (changes, _) = crate::diff::diff_pairs(&convars(stock), &convars(current));
    if changes.is_empty() {
        return "game";
    }
    let build_changed = applied_build.zip(game_build).is_some_and(|(a, b)| a != b);
    let ours_gone = applied.and_then(|a| crate::share::convar_delta(stock, a).ok()).is_some_and(|delta| {
        let now = convars(current);
        !delta.is_empty()
            && delta.iter().all(|e| {
                let value = now.iter().find(|(k, _)| crate::convars::convar_of(k).is_some_and(|n| n.eq_ignore_ascii_case(&e.name))).map(|(_, v)| v);
                value != e.value.as_ref()
            })
    });
    if build_changed && ours_gone {
        "game"
    } else {
        "other"
    }
}

/// `resolve` plus the ranked check of the installed file, which needs the cache folder. `ranked_refused`:
/// console.log shows matchmaking refused ranked (read by the background watcher, see `perf::LogTail`).
pub fn resolve_with_checks(s: &Settings, cache_dir: &Path, ranked_refused: bool) -> Overview {
    let ov = resolve(s);
    let installed = ov.gameinfo_path.as_ref().filter(|_| ov.gameinfo_found).and_then(|p| std::fs::read_to_string(p).ok());
    let stock = crate::stock::load(cache_dir, installed.as_deref());
    let ranked_changes = installed.as_deref().and_then(|src| crate::ranked::guarded_changes(&stock, src).ok().map(|c| c.len()));
    let game_build = ov.gameinfo_path.as_deref().and_then(|p| paths::game_build(Path::new(p)));
    let changed_by = match (&ov.status, installed.as_deref()) {
        (apply::Status::Changed { .. }, Some(src)) => {
            let applied_build = s.applied.as_ref().and_then(|a| a.game_build.as_deref());
            let applied_src = cache_dir.parent().and_then(crate::commands::applied_source).map(|f| f.gameinfo);
            Some(changed_by(src, &stock, applied_src.as_deref(), applied_build, game_build.as_deref()).to_string())
        }
        _ => None,
    };
    // Only the installed file's engine changes can cause the refusal.
    let ranked_blocked = ranked_changes.is_some_and(|n| n > 0) && ranked_refused;
    // Changes on top of the config: the app's own write, or an edit by hand (not a game update or mod manager).
    let tweakable = matches!(ov.status, apply::Status::Applied { .. } | apply::Status::AppliedDmm { .. })
        || (matches!(ov.status, apply::Status::Changed { .. }) && changed_by.as_deref() == Some("other"));
    let config = cache_dir.parent().and_then(crate::commands::applied_source).map(|f| f.gameinfo);
    let changes = match (tweakable, installed.as_deref(), config.as_deref()) {
        (true, Some(now), Some(own)) => crate::tweaks::changes(now, own, &stock),
        _ => Vec::new(),
    };
    let tweak_fingerprint = changes.join(";");
    let tweak_name = s.applied.as_ref().filter(|_| !changes.is_empty()).map(|a| {
        crate::perf::label_for(&s.perf_tweaks, &crate::commands::perf_key(a), &a.pack_name, &tweak_fingerprint).2
    });
    Overview { ranked_changes, changed_by, game_build, ranked_blocked, tweaks: changes.len(), tweak_name, tweak_fingerprint, ..ov }
}

#[tauri::command]
pub fn get_overview(state: State<AppState>) -> Overview {
    state.overview(&state.snapshot())
}

#[tauri::command]
pub fn set_account(state: State<AppState>, id: String) -> Result<Overview, String> {
    let s = state.update(|s| Settings { account_id: Some(id), video_path: None, ..s })?;
    Ok(state.overview(&s))
}

fn is_named(p: &Path, name: &str) -> bool {
    p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(name))
}

/// Turns whatever the user picked (the file itself or a folder above it) into the file path.
pub fn resolve_pick(kind: &str, picked: &Path) -> Result<PathBuf, String> {
    match kind {
        "steam" if picked.is_dir() => Ok(picked.to_path_buf()),
        "steam" => Err("Pick your Steam install folder.".into()),
        "gameinfo" => {
            let candidates = [
                picked.to_path_buf(),
                picked.join("gameinfo.gi"),
                picked.join(r"citadel\gameinfo.gi"),
                picked.join(r"game\citadel\gameinfo.gi"),
                paths::gameinfo_in_library(picked),
            ];
            let named: Vec<&PathBuf> = candidates.iter().filter(|p| is_named(p, "gameinfo.gi")).collect();
            named
                .iter()
                .find(|p| p.is_file())
                // Missing file but the citadel folder exists: Apply will create it.
                .or_else(|| named.iter().find(|p| p.parent().is_some_and(|d| d.is_dir() && is_named(d, "citadel"))))
                .map(|p| p.to_path_buf())
                .ok_or_else(|| "No gameinfo.gi in that folder. Pick Deadlock\\game\\citadel (or the Deadlock folder).".into())
        }
        "video" => {
            let found = [picked.to_path_buf(), picked.join("video.txt"), picked.join(r"local\cfg\video.txt")]
                .into_iter()
                .find(|p| p.is_file() && is_named(p, "video.txt"));
            match found {
                Some(p) => Ok(p),
                // The game may not have created it yet; accept the cfg folder itself.
                None if picked.is_dir() && is_named(picked, "cfg") => Ok(picked.join("video.txt")),
                None => Err("No video.txt in that folder. Pick ...\\userdata\\<id>\\1422450\\local\\cfg.".into()),
            }
        }
        _ => Err("Unknown location.".into()),
    }
}

/// `kind` is "steam", "gameinfo" or "video"; `None` clears the override and re-detects.
#[tauri::command]
pub fn set_path(state: State<AppState>, kind: String, path: Option<String>) -> Result<Overview, String> {
    let path = match path {
        Some(p) => Some(to_string(&resolve_pick(&kind, Path::new(&p))?)),
        None => None,
    };
    let s = state.update(|s| match kind.as_str() {
        "steam" => Settings { steam_root: path, gameinfo_path: None, video_path: None, account_id: None, ..s },
        "gameinfo" => Settings { gameinfo_path: path, ..s },
        _ => Settings { video_path: path, ..s },
    })?;
    Ok(state.overview(&s))
}

#[tauri::command]
pub fn save_settings(state: State<AppState>, preserved_keys: Vec<String>) -> Result<Overview, String> {
    let keys: Vec<String> = preserved_keys
        .into_iter()
        .filter(|k| video::DEFAULT_PRESERVED.iter().any(|d| d.eq_ignore_ascii_case(k)))
        .collect();
    let s = state.update(|s| Settings { preserved_keys: keys, ..s })?;
    Ok(state.overview(&s))
}

#[tauri::command]
pub async fn list_packs(state: State<'_, AppState>) -> Result<Vec<RepoGroup>, String> {
    let repos = state.snapshot().effective_repos();
    let cache = state.dir.join("cache");
    tauri::async_runtime::spawn_blocking(move || {
        crate::stock::refresh(&cache);
        packs::list_all(&repos, &cache)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Packs from the on-disk cache only; instant, used to fill the list before the network refresh.
#[tauri::command]
pub fn cached_packs(state: State<AppState>) -> Vec<RepoGroup> {
    packs::cached_all(&state.snapshot().effective_repos(), &state.dir.join("cache"))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedRepo {
    pub overview: Overview,
    pub group: RepoGroup,
}

/// Validates the repo by loading its packs; only a repo with packs is linked.
#[tauri::command]
pub async fn add_repo(state: State<'_, AppState>, input: String) -> Result<AddedRepo, String> {
    let current = state.snapshot().effective_repos();
    let next = repos::add(&current, &input)?;
    let repo = next.last().cloned().unwrap_or_default();
    let cache = state.dir.join("cache");
    let group = tauri::async_runtime::spawn_blocking(move || packs::list_packs(&repo, &cache))
        .await
        .map_err(|e| e.to_string())?;
    if group.packs.is_empty() {
        return Err(group.error.unwrap_or_else(|| format!("{} has no config packs.", group.repo)));
    }
    let s = state.update(|s| Settings { repos: repos::add(&s.effective_repos(), &input).unwrap_or(next), ..s })?;
    Ok(AddedRepo { overview: state.overview(&s), group })
}

#[tauri::command]
pub fn remove_repo(state: State<AppState>, repo: String) -> Result<Overview, String> {
    let s = state.update(|s| Settings { repos: repos::remove(&s.effective_repos(), &repo), ..s })?;
    Ok(state.overview(&s))
}

#[tauri::command]
pub fn move_repo(state: State<AppState>, repo: String, delta: i32) -> Result<Overview, String> {
    let s = state.update(|s| Settings { repos: repos::shift(&s.effective_repos(), &repo, delta), ..s })?;
    Ok(state.overview(&s))
}

/// Pins or unpins a pack; `key` is `owner/name#packId`.
#[tauri::command]
pub fn toggle_pin(state: State<AppState>, key: String) -> Result<Overview, String> {
    let valid = key.split_once('#').is_some_and(|(repo, id)| repos::canonical(repo).is_ok() && !id.is_empty());
    if !valid {
        return Err("That pack can't be pinned.".into());
    }
    let s = state.update(|s| Settings { pinned: repos::toggle(&s.pinned, &key), ..s })?;
    Ok(state.overview(&s))
}

/// Hides the first-run guide for good (finished or skipped).
#[tauri::command]
pub fn finish_guide(state: State<AppState>) -> Result<Overview, String> {
    let s = state.update(|s| Settings { onboarded: true, ..s })?;
    Ok(state.overview(&s))
}

#[cfg(test)]
mod tests {

    #[test]
    fn tells_who_changed_the_file() {
        let stock = crate::stock::EMBEDDED;
        assert_eq!(changed_by(stock, stock, None, Some("1"), Some("2")), "game");
        assert_eq!(changed_by(stock, stock, None, None, None), "game");
        let tweaked = stock.replacen("\"rate\"", "\"r_fog\" \"0\"\n\t\t\"rate\"", 1);
        assert_eq!(changed_by(&tweaked, stock, None, Some("1"), Some("1")), "other");
        // A new build alone isn't enough: someone may have edited the file after the update.
        assert_eq!(changed_by(&tweaked, stock, Some(&tweaked), Some("1"), Some("2")), "other");
        // New build and the config's own changes are gone (Valve's newer file, stock snapshot stale): game.
        let pack = stock.replacen("\"rate\"", "\"r_ssao\" \"0\"\n\t\t\"rate\"", 1);
        assert_eq!(changed_by(&tweaked, stock, Some(&pack), Some("1"), Some("2")), "game");
        let dmm = format!("{tweaked}\n{}\n{}\n", crate::gameinfo::DMM_START, crate::gameinfo::DMM_END);
        assert_eq!(changed_by(&dmm, stock, Some(&pack), Some("1"), Some("2")), "dmm");
        let grim = tweaked.replacen("Game_LowViolence", "Game citadel/grimoire\n\t\t\tGame_LowViolence", 1);
        assert_eq!(changed_by(&grim, stock, None, None, None), "grimoire");
    }

    use super::*;
    use std::fs;

    #[test]
    fn resolves_gameinfo_and_video_from_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let deadlock = tmp.path().join("Deadlock");
        let citadel = deadlock.join(r"game\citadel");
        fs::create_dir_all(&citadel).unwrap();
        let gi = citadel.join("gameinfo.gi");
        // Missing file inside an existing citadel folder is accepted (Apply creates it).
        assert_eq!(resolve_pick("gameinfo", &deadlock).unwrap(), gi);
        fs::write(&gi, "x").unwrap();
        for pick in [&gi, &citadel, &deadlock.join("game"), &deadlock] {
            assert_eq!(resolve_pick("gameinfo", pick).unwrap(), gi, "{}", pick.display());
        }
        assert!(resolve_pick("gameinfo", tmp.path()).is_err());

        let cfg = tmp.path().join(r"1422450\local\cfg");
        fs::create_dir_all(&cfg).unwrap();
        assert_eq!(resolve_pick("video", &cfg).unwrap(), cfg.join("video.txt"));
        fs::write(cfg.join("video.txt"), "x").unwrap();
        assert_eq!(resolve_pick("video", &tmp.path().join("1422450")).unwrap(), cfg.join("video.txt"));
        assert!(resolve_pick("video", &deadlock).is_err());
    }
}


