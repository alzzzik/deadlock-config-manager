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
    }
}

#[tauri::command]
pub fn get_overview(state: State<AppState>) -> Overview {
    resolve(&state.snapshot())
}

#[tauri::command]
pub fn set_account(state: State<AppState>, id: String) -> Result<Overview, String> {
    let s = state.update(|s| Settings { account_id: Some(id), video_path: None, ..s })?;
    Ok(resolve(&s))
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
    Ok(resolve(&s))
}

#[tauri::command]
pub fn save_settings(state: State<AppState>, preserved_keys: Vec<String>) -> Result<Overview, String> {
    let keys: Vec<String> = preserved_keys
        .into_iter()
        .filter(|k| video::DEFAULT_PRESERVED.iter().any(|d| d.eq_ignore_ascii_case(k)))
        .collect();
    let s = state.update(|s| Settings { preserved_keys: keys, ..s })?;
    Ok(resolve(&s))
}

#[tauri::command]
pub async fn list_packs(state: State<'_, AppState>) -> Result<Vec<RepoGroup>, String> {
    let repos = state.snapshot().effective_repos();
    let cache = state.dir.join("cache");
    tauri::async_runtime::spawn_blocking(move || packs::list_all(&repos, &cache))
        .await
        .map_err(|e| e.to_string())
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
    Ok(AddedRepo { overview: resolve(&s), group })
}

#[tauri::command]
pub fn remove_repo(state: State<AppState>, repo: String) -> Result<Overview, String> {
    let s = state.update(|s| Settings { repos: repos::remove(&s.effective_repos(), &repo), ..s })?;
    Ok(resolve(&s))
}

#[tauri::command]
pub fn move_repo(state: State<AppState>, repo: String, delta: i32) -> Result<Overview, String> {
    let s = state.update(|s| Settings { repos: repos::shift(&s.effective_repos(), &repo, delta), ..s })?;
    Ok(resolve(&s))
}

/// Pins or unpins a pack; `key` is `owner/name#packId`.
#[tauri::command]
pub fn toggle_pin(state: State<AppState>, key: String) -> Result<Overview, String> {
    let valid = key.split_once('#').is_some_and(|(repo, id)| repos::canonical(repo).is_ok() && !id.is_empty());
    if !valid {
        return Err("That pack can't be pinned.".into());
    }
    let s = state.update(|s| Settings { pinned: repos::toggle(&s.pinned, &key), ..s })?;
    Ok(resolve(&s))
}

#[cfg(test)]
mod tests {
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
