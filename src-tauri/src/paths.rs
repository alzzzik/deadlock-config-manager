//! Finding Steam, the Deadlock library and the per-account video.txt files.

use crate::kv;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEADLOCK_APP_ID: &str = "1422450";
const STEAMID64_BASE: u64 = 76_561_197_960_265_728;
const DEFAULT_STEAM_ROOT: &str = r"C:\Program Files (x86)\Steam";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub persona: Option<String>,
    pub video_path: String,
    /// False when Deadlock has never been launched on this account.
    pub has_video: bool,
}

pub fn detect_steam_root() -> Option<PathBuf> {
    registry_steam_root()
        .into_iter()
        .chain(std::iter::once(PathBuf::from(DEFAULT_STEAM_ROOT)))
        .find(|p| p.is_dir())
}

#[cfg(windows)]
fn registry_steam_root() -> Vec<PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    let read = |hive, path: &str, name: &str| {
        RegKey::predef(hive).open_subkey(path).ok()?.get_value::<String, _>(name).ok()
    };
    [
        read(HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath"),
        read(HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath"),
    ]
    .into_iter()
    .flatten()
    .map(|s| PathBuf::from(s.replace('/', "\\")))
    .collect()
}

#[cfg(not(windows))]
fn registry_steam_root() -> Vec<PathBuf> {
    Vec::new()
}

/// The library folder whose `apps` block lists Deadlock.
pub fn deadlock_library_from_vdf(vdf: &str) -> Option<PathBuf> {
    let nodes = kv::parse(vdf).ok()?;
    let root = kv::get(&nodes, "libraryfolders")?;
    root.children()
        .iter()
        .filter(|lib| kv::get(lib.children(), "apps").is_some_and(|apps| kv::get(apps.children(), DEADLOCK_APP_ID).is_some()))
        .find_map(|lib| kv::get(lib.children(), "path")?.as_str().map(PathBuf::from))
}

/// Steam's build id for Deadlock, read from `appmanifest_1422450.acf` next to the library's `common`
/// folder. `gameinfo` is `<library>\steamapps\common\Deadlock\game\citadel\gameinfo.gi`.
pub fn game_build(gameinfo: &Path) -> Option<String> {
    let steamapps = gameinfo.ancestors().nth(5)?;
    let text = fs::read_to_string(steamapps.join(format!("appmanifest_{DEADLOCK_APP_ID}.acf"))).ok()?;
    build_from_manifest(&text)
}

pub fn build_from_manifest(acf: &str) -> Option<String> {
    let nodes = kv::parse(acf).ok()?;
    let state = kv::get(&nodes, "AppState")?;
    kv::get(state.children(), "buildid")?.as_str().map(str::to_string).filter(|b| !b.is_empty())
}

pub fn gameinfo_in_library(library: &Path) -> PathBuf {
    library.join(r"steamapps\common\Deadlock\game\citadel\gameinfo.gi")
}

/// Locates gameinfo.gi via libraryfolders.vdf, falling back to the Steam root itself.
pub fn detect_gameinfo(steam_root: &Path) -> Option<PathBuf> {
    let vdf = fs::read_to_string(steam_root.join(r"steamapps\libraryfolders.vdf")).ok();
    let candidates: Vec<PathBuf> = vdf
        .as_deref()
        .and_then(deadlock_library_from_vdf)
        .into_iter()
        .chain(std::iter::once(steam_root.to_path_buf()))
        .map(|lib| gameinfo_in_library(&lib))
        .collect();
    // Prefer an existing file; otherwise an install whose citadel folder exists (file can be created).
    candidates
        .iter()
        .find(|p| p.is_file())
        .or_else(|| candidates.iter().find(|p| p.parent().is_some_and(Path::is_dir)))
        .cloned()
}

pub fn video_path_for(steam_root: &Path, account_id: &str) -> PathBuf {
    steam_root
        .join("userdata")
        .join(account_id)
        .join(DEADLOCK_APP_ID)
        .join(r"local\cfg\video.txt")
}

/// Every Steam account under userdata, with persona names from loginusers.vdf when known.
pub fn list_accounts(steam_root: &Path) -> Vec<Account> {
    let logins = fs::read_to_string(steam_root.join(r"config\loginusers.vdf")).unwrap_or_default();
    let personas = login_users(&logins);
    let Ok(entries) = fs::read_dir(steam_root.join("userdata")) else {
        return Vec::new();
    };
    let mut accounts: Vec<Account> = entries
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|id| id.chars().all(|c| c.is_ascii_digit()) && id != "0")
        .map(|id| (video_path_for(steam_root, &id), id))
        .map(|(p, id)| Account {
            persona: personas.iter().find(|u| u.account_id == id).map(|u| u.persona.clone()).filter(|n| !n.is_empty()),
            has_video: p.is_file(),
            video_path: p.to_string_lossy().into_owned(),
            id,
        })
        .collect();
    accounts.sort_by(|a, b| a.id.cmp(&b.id));
    accounts
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoginUser {
    pub account_id: String,
    pub persona: String,
    pub most_recent: bool,
    pub timestamp: u64,
}

pub fn login_users(vdf: &str) -> Vec<LoginUser> {
    let Ok(nodes) = kv::parse(vdf) else {
        return Vec::new();
    };
    let Some(users) = kv::get(&nodes, "users") else {
        return Vec::new();
    };
    users
        .children()
        .iter()
        .filter_map(|u| {
            let steam64: u64 = u.key.parse().ok()?;
            let field = |k: &str| kv::get(u.children(), k).and_then(|n| n.as_str()).unwrap_or("");
            Some(LoginUser {
                account_id: steam64.checked_sub(STEAMID64_BASE)?.to_string(),
                persona: field("PersonaName").to_string(),
                most_recent: field("MostRecent") == "1",
                timestamp: field("Timestamp").parse().unwrap_or(0),
            })
        })
        .collect()
}

/// The most recent login (MostRecent "1", else the newest timestamp) among the given accounts.
pub fn preferred_account(loginusers_vdf: &str, accounts: &[Account]) -> Option<String> {
    let users: Vec<LoginUser> = login_users(loginusers_vdf)
        .into_iter()
        .filter(|u| accounts.iter().any(|a| a.id == u.account_id))
        .collect();
    users
        .iter()
        .find(|u| u.most_recent)
        .or_else(|| users.iter().max_by_key(|u| u.timestamp))
        .map(|u| u.account_id.clone())
        .or_else(|| accounts.iter().find(|a| a.has_video).or(accounts.first()).map(|a| a.id.clone()))
}

pub fn read_loginusers(steam_root: &Path) -> String {
    fs::read_to_string(steam_root.join(r"config\loginusers.vdf")).unwrap_or_default()
}

#[cfg(test)]
mod tests {

    #[test]
    fn reads_build_id_from_app_manifest() {
        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"1422450\"\n\t\"buildid\"\t\t\"26143760\"\n}\n";
        assert_eq!(build_from_manifest(acf).as_deref(), Some("26143760"));
        assert_eq!(build_from_manifest("\"AppState\"\n{\n}\n"), None);
        let tmp = tempfile::tempdir().unwrap();
        let gi = tmp.path().join(r"steamapps\common\Deadlock\game\citadel\gameinfo.gi");
        fs::create_dir_all(gi.parent().unwrap()).unwrap();
        fs::write(tmp.path().join(r"steamapps\appmanifest_1422450.acf"), acf).unwrap();
        assert_eq!(game_build(&gi).as_deref(), Some("26143760"));
    }

    use super::*;

    const LIBS: &str = include_str!("../fixtures/libraryfolders.vdf");
    const LOGINS: &str = include_str!("../fixtures/loginusers.vdf");

    fn account(id: &str) -> Account {
        Account { id: id.into(), persona: None, video_path: String::new(), has_video: false }
    }

    #[test]
    fn finds_library_that_has_deadlock() {
        assert_eq!(deadlock_library_from_vdf(LIBS), Some(PathBuf::from(r"D:\SteamLibrary")));
        assert_eq!(deadlock_library_from_vdf("\"libraryfolders\" { }"), None);
    }

    #[test]
    fn converts_steamid64_to_account_id() {
        let users = login_users(LOGINS);
        assert_eq!(users[0].account_id, "101");
        assert_eq!(users[1].account_id, "39734272");
        assert_eq!(users[1].persona, "Second");
    }

    #[test]
    fn prefers_most_recent_login() {
        let accounts = [account("101"), account("39734272")];
        assert_eq!(preferred_account(LOGINS, &accounts).as_deref(), Some("39734272"));
    }

    #[test]
    fn falls_back_to_newest_timestamp_then_first_account() {
        let no_recent = LOGINS.replace("\"MostRecent\"\t\t\"1\"", "\"MostRecent\"\t\t\"0\"");
        let accounts = [account("101"), account("39734272")];
        assert_eq!(preferred_account(&no_recent, &accounts).as_deref(), Some("101"));
        assert_eq!(preferred_account("", &[account("5")]).as_deref(), Some("5"));
        assert_eq!(preferred_account(LOGINS, &[account("101")]).as_deref(), Some("101"));
    }

    #[test]
    fn lists_all_accounts_and_flags_video_txt() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for id in ["101", "202"] {
            fs::create_dir_all(video_path_for(root, id).parent().unwrap()).unwrap();
        }
        fs::write(video_path_for(root, "101"), "x").unwrap();
        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(root.join(r"config\loginusers.vdf"), LOGINS).unwrap();
        let accounts = list_accounts(root);
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].id, "101");
        assert!(accounts[0].has_video);
        assert_eq!(accounts[0].persona.as_deref(), Some("First"));
        assert_eq!(accounts[1].id, "202");
        assert!(!accounts[1].has_video);
    }
}
