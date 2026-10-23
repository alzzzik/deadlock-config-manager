//! Configs posted on GameBanana: a mod page's small download (.7z, .zip or .gi) is fetched, its MD5 is
//! checked against the one GameBanana publishes, and gameinfo.gi/video.txt become a saved config.

use crate::apply::PackFiles;
use md5::{Digest, Md5};
use std::io::Read;
use std::time::Duration;

const API: &str = "https://gamebanana.com/apiv11/Mod";
/// Config downloads are a few KB; anything bigger is a mod (models, sounds), not a config.
const MAX_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq)]
pub struct ModFile {
    pub name: String,
    pub size: u64,
    pub md5: String,
    pub url: String,
    /// Upload time (Unix seconds); 0 when GameBanana doesn't say.
    pub added: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModInfo {
    pub id: u64,
    pub name: String,
    pub author: String,
    pub files: Vec<ModFile>,
}

/// "https://gamebanana.com/mods/729604", "gamebanana.com/mods/729604/" or "729604".
pub fn parse_mod_id(input: &str) -> Result<u64, String> {
    let t = input.trim().trim_end_matches('/');
    let tail = t.rsplit('/').next().unwrap_or(t);
    let is_gb = t.chars().all(|c| c.is_ascii_digit()) || t.contains("gamebanana.com/mods/");
    match tail.parse::<u64>() {
        Ok(id) if is_gb && id > 0 => Ok(id),
        _ => Err("Paste a GameBanana mod link like https://gamebanana.com/mods/729604.".into()),
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(TIMEOUT).user_agent(concat!("DeadlockConfigManager/", env!("CARGO_PKG_VERSION"))).build()
}

pub fn parse_info(id: u64, json: &serde_json::Value) -> Result<ModInfo, String> {
    let name = json["_sName"].as_str().ok_or("GameBanana didn't return that mod.")?.to_string();
    let author = json["_aSubmitter"]["_sName"].as_str().unwrap_or("unknown").to_string();
    let files = json["_aFiles"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|f| {
                    Some(ModFile {
                        name: f["_sFile"].as_str()?.to_string(),
                        size: f["_nFilesize"].as_u64().unwrap_or(u64::MAX),
                        md5: f["_sMd5Checksum"].as_str().unwrap_or_default().to_ascii_lowercase(),
                        url: f["_sDownloadUrl"].as_str()?.to_string(),
                        added: f["_tsDateAdded"].as_u64().unwrap_or(0),
                    })
                })
                .filter(|f| f.url.starts_with("https://gamebanana.com/dl/"))
                .collect()
        })
        .unwrap_or_default();
    Ok(ModInfo { id, name, author, files })
}

pub fn fetch_info(id: u64) -> Result<ModInfo, String> {
    let url = format!("{API}/{id}?_csvProperties=_sName,_aFiles,_aSubmitter");
    let json: serde_json::Value = agent()
        .get(&url)
        .call()
        .map_err(|e| format!("Couldn't reach GameBanana: {e}"))?
        .into_json()
        .map_err(|e| format!("Unexpected answer from GameBanana: {e}"))?;
    parse_info(id, &json)
}

/// Candidate files: small archives or .gi files, newest upload first (authors keep old versions on the
/// page), then smallest.
pub fn config_files(info: &ModInfo) -> Vec<&ModFile> {
    let mut files: Vec<&ModFile> = info
        .files
        .iter()
        .filter(|f| f.size <= MAX_DOWNLOAD_BYTES)
        .filter(|f| {
            let n = f.name.to_ascii_lowercase();
            n.ends_with(".7z") || n.ends_with(".zip") || n.ends_with(".gi")
        })
        .collect();
    files.sort_by_key(|f| (std::cmp::Reverse(f.added), f.size));
    files
}

pub fn md5_hex(bytes: &[u8]) -> String {
    hex::encode(Md5::digest(bytes))
}

fn download(file: &ModFile) -> Result<Vec<u8>, String> {
    let resp = agent().get(&file.url).call().map_err(|e| format!("Download from GameBanana failed: {e}"))?;
    let mut bytes = Vec::new();
    resp.into_reader().take(MAX_DOWNLOAD_BYTES + 1).read_to_end(&mut bytes).map_err(|e| format!("Download failed: {e}"))?;
    if bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err("That download is too large to be a config.".into());
    }
    if !file.md5.is_empty() && md5_hex(&bytes) != file.md5 {
        return Err("The download doesn't match GameBanana's checksum, so it wasn't used.".into());
    }
    Ok(bytes)
}

/// Downloads the first file of the mod that contains a gameinfo.gi.
/// Returns the files plus the file name and MD5 they came from.
pub fn fetch_config(info: &ModInfo) -> Result<(PackFiles, String, String), String> {
    let candidates = config_files(info);
    if candidates.is_empty() {
        return Err(format!("\"{}\" has no small .7z, .zip or .gi download, so it doesn't look like a config.", info.name));
    }
    let mut last_err = String::new();
    for f in candidates {
        match download(f).and_then(|b| crate::library::files_from_archive(&f.name, &b).map(|files| (files, md5_hex(&b)))) {
            Ok((files, md5)) => {
                crate::gameinfo::validate(&files.gameinfo).map_err(|e| format!("{} from GameBanana: {e}", f.name))?;
                return Ok((files, f.name.clone(), md5));
            }
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mod_links() {
        assert_eq!(parse_mod_id("https://gamebanana.com/mods/729604").unwrap(), 729604);
        assert_eq!(parse_mod_id("gamebanana.com/mods/729604/").unwrap(), 729604);
        assert_eq!(parse_mod_id("729604").unwrap(), 729604);
        assert!(parse_mod_id("https://example.com/mods/1").is_err());
        assert!(parse_mod_id("hello").is_err());
    }

    #[test]
    fn picks_small_config_files() {
        let json = serde_json::json!({
            "_sName": "FPS Config", "_aSubmitter": { "_sName": "someone" },
            "_aFiles": [
                { "_sFile": "pak12_dir.7z", "_nFilesize": 17962342, "_sMd5Checksum": "a", "_sDownloadUrl": "https://gamebanana.com/dl/2" },
                { "_sFile": "gameinfo.7z", "_nFilesize": 6719, "_sMd5Checksum": "B", "_sDownloadUrl": "https://gamebanana.com/dl/1" },
                { "_sFile": "evil.zip", "_nFilesize": 10, "_sMd5Checksum": "c", "_sDownloadUrl": "https://elsewhere.example/x" }
            ]
        });
        let info = parse_info(1, &json).unwrap();
        assert_eq!(info.author, "someone");
        let picked = config_files(&info);
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].name, "gameinfo.7z");
        assert_eq!(picked[0].md5, "b");
    }

    #[test]
    fn prefers_the_newest_upload() {
        let json = serde_json::json!({
            "_sName": "Config", "_aFiles": [
                { "_sFile": "v1.zip", "_nFilesize": 100, "_tsDateAdded": 1000, "_sDownloadUrl": "https://gamebanana.com/dl/1" },
                { "_sFile": "v3.zip", "_nFilesize": 300, "_tsDateAdded": 3000, "_sDownloadUrl": "https://gamebanana.com/dl/3" },
                { "_sFile": "v2.zip", "_nFilesize": 200, "_tsDateAdded": 2000, "_sDownloadUrl": "https://gamebanana.com/dl/2" }
            ]
        });
        let info = parse_info(1, &json).unwrap();
        let names: Vec<&str> = config_files(&info).iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["v3.zip", "v2.zip", "v1.zip"]);
    }

    #[test]
    #[ignore = "needs network access to gamebanana.com"]
    fn imports_a_real_config() {
        let info = fetch_info(729604).unwrap();
        let (files, name, _) = fetch_config(&info).unwrap();
        assert!(name.ends_with(".7z"));
        assert!(files.gameinfo.contains("GameInfo"));
    }
}
