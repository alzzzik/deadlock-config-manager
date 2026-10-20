//! %APPDATA%\DeadlockConfigManager\settings.json

use crate::fsops;
use crate::video::DEFAULT_PRESERVED;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_REPO: &str = "alzzzik/alz-deadlock-cfg";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppliedRecord {
    /// `owner/name` of the source repo; None for local folders and pre-multi-repo records.
    #[serde(default)]
    pub repo: Option<String>,
    pub pack_id: String,
    pub pack_name: String,
    pub revision: Option<String>,
    pub applied_at: String,
    pub gameinfo_sha256: String,
    /// Hash with the DMM region and SearchPaths stripped, see `gameinfo::normalized_hash`.
    pub gameinfo_normalized_sha256: String,
    pub video_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Linked pack repos (`owner/name`), in display order.
    pub repos: Vec<String>,
    /// Pre-multi-repo settings had a single `repo`; read once and migrated into `repos`.
    #[serde(rename = "repo", skip_serializing)]
    pub legacy_repo: Option<String>,
    pub steam_root: Option<String>,
    pub gameinfo_path: Option<String>,
    pub account_id: Option<String>,
    /// Manual override for video.txt; when unset it follows the chosen account.
    pub video_path: Option<String>,
    pub preserved_keys: Vec<String>,
    pub applied: Option<AppliedRecord>,
    /// Pinned packs as `owner/name#packId`, shown at the top of the pack list.
    pub pinned: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            repos: vec![DEFAULT_REPO.into()],
            legacy_repo: None,
            steam_root: None,
            gameinfo_path: None,
            account_id: None,
            video_path: None,
            preserved_keys: DEFAULT_PRESERVED.iter().map(|s| s.to_string()).collect(),
            applied: None,
            pinned: Vec::new(),
        }
    }
}

const APP_FOLDER: &str = "DeadlockConfigManager";
/// Folder used before the app was renamed; moved over on first start.
const LEGACY_APP_FOLDER: &str = "DeadlockCfgHub";

pub fn app_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    migrate_folder(&base.join(LEGACY_APP_FOLDER), &base.join(APP_FOLDER))
}

/// Moves the pre-rename folder (settings, backups incl. the "original", cache) to the new name
/// if the new one doesn't exist yet. If the move fails, keeps using the old folder.
fn migrate_folder(old: &Path, new: &Path) -> PathBuf {
    if new.exists() || !old.is_dir() {
        return new.to_path_buf();
    }
    match fs::rename(old, new) {
        Ok(()) => new.to_path_buf(),
        Err(_) => old.to_path_buf(),
    }
}

fn settings_file(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

/// Loads settings; a missing or corrupt file yields defaults (the corrupt one is kept aside).
pub fn load(dir: &Path) -> Settings {
    let path = settings_file(dir);
    let Ok(text) = fs::read_to_string(&path) else {
        return Settings::default();
    };
    let parsed: Settings = serde_json::from_str(&text).unwrap_or_else(|_| {
        let _ = fs::rename(&path, dir.join("settings.corrupt.json"));
        Settings::default()
    });
    migrate(parsed)
}

/// Moves a legacy single `repo` into `repos`.
fn migrate(s: Settings) -> Settings {
    match s.legacy_repo.clone() {
        Some(old) if !s.repos.iter().any(|r| r.eq_ignore_ascii_case(&old)) => {
            let repos = std::iter::once(old).chain(s.repos.into_iter().filter(|r| r != DEFAULT_REPO)).collect();
            Settings { repos, legacy_repo: None, ..s }
        }
        _ => Settings { legacy_repo: None, ..s },
    }
}

impl Settings {
    /// The repos to show; an emptied list falls back to the default repo.
    pub fn effective_repos(&self) -> Vec<String> {
        if self.repos.is_empty() {
            vec![DEFAULT_REPO.into()]
        } else {
            self.repos.clone()
        }
    }
}

pub fn save(dir: &Path, settings: &Settings) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fsops::atomic_write(&settings_file(dir), json.as_bytes())
        .map_err(|e| format!("Could not save settings: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_fills_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()), Settings::default());
        let s = Settings { account_id: Some("42".into()), ..Settings::default() };
        save(dir.path(), &s).unwrap();
        assert_eq!(load(dir.path()), s);
        fs::write(settings_file(dir.path()), r#"{"repo":"a/b"}"#).unwrap();
        let partial = load(dir.path());
        assert_eq!(partial.repos, vec!["a/b".to_string()]);
        assert_eq!(partial.legacy_repo, None);
        assert_eq!(partial.preserved_keys.len(), DEFAULT_PRESERVED.len());
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(settings_file(dir.path()), "{not json").unwrap();
        assert_eq!(load(dir.path()), Settings::default());
        assert!(dir.path().join("settings.corrupt.json").exists());
    }

    #[test]
    fn moves_legacy_folder_once() {
        let tmp = tempfile::tempdir().unwrap();
        let (old, new) = (tmp.path().join("Old"), tmp.path().join("New"));
        fs::create_dir_all(old.join("backups")).unwrap();
        assert_eq!(migrate_folder(&old, &new), new);
        assert!(new.join("backups").is_dir() && !old.exists());
        // Already migrated (or fresh install): nothing to do.
        assert_eq!(migrate_folder(&old, &new), new);
    }

    #[test]
    fn empty_repo_list_falls_back_to_default() {
        let s = Settings { repos: vec![], ..Settings::default() };
        assert_eq!(s.effective_repos(), vec![DEFAULT_REPO.to_string()]);
    }
}
