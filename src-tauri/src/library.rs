//! Saved configs ("My configs"): configs that don't live in any repo, kept in
//! %APPDATA%\DeadlockConfigManager\library\<id>\ as gameinfo.gi, video.txt and meta.json.

use crate::apply::PackFiles;
use crate::fsops;
use crate::video;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Largest file accepted from a zip; real configs are a few KB.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_NAME_CHARS: usize = 60;
const MAX_NOTES_CHARS: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub notes: String,
    pub created_at: String,
    pub has_video: bool,
    /// Where it came from, e.g. "Saved from alzzzik/alz-deadlock-cfg Stable" or "Imported from x.zip".
    #[serde(default)]
    pub source: String,
}

pub fn dir(app_dir: &Path) -> PathBuf {
    app_dir.join("library")
}

fn is_safe_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn entry_dir(root: &Path, id: &str) -> Result<PathBuf, String> {
    if !is_safe_id(id) {
        return Err("Invalid config id.".into());
    }
    Ok(root.join(id))
}

fn clean_name(name: &str) -> Result<String, String> {
    let n: String = name.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME_CHARS).collect();
    if n.is_empty() {
        return Err("Give the config a name.".into());
    }
    Ok(n)
}

fn new_id(name: &str) -> String {
    let slug: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .take(5)
        .collect::<Vec<_>>()
        .join("-");
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S%3f");
    if slug.is_empty() {
        format!("config-{stamp}")
    } else {
        format!("{slug}-{stamp}")
    }
}

/// Saves a config. The video.txt is stored without hardware lines (VendorID/DeviceID); display
/// settings are kept in the file but your own are still preserved when it's applied.
pub fn save(root: &Path, name: &str, notes: &str, files: &PackFiles, source: &str) -> Result<Entry, String> {
    crate::gameinfo::validate(&files.gameinfo)?;
    let video = files.video.as_deref().map(video::new_from_pack).transpose()?;
    let name = clean_name(name)?;
    let entry = Entry {
        id: new_id(&name),
        name,
        notes: notes.trim().chars().take(MAX_NOTES_CHARS).collect(),
        created_at: chrono::Local::now().to_rfc3339(),
        has_video: video.is_some(),
        source: source.to_string(),
    };
    let d = root.join(&entry.id);
    fs::create_dir_all(&d).map_err(|e| format!("Could not create {}: {e}", d.display()))?;
    let write = |file: &str, bytes: &[u8]| fsops::atomic_write(&d.join(file), bytes).map_err(|e| format!("Could not save {file}: {e}"));
    write("gameinfo.gi", files.gameinfo.as_bytes())?;
    if let Some(v) = &video {
        write("video.txt", v.as_bytes())?;
    }
    write("meta.json", serde_json::to_string_pretty(&entry).map_err(|e| e.to_string())?.as_bytes())?;
    Ok(entry)
}

/// All saved configs, newest first. Broken folders are skipped.
pub fn list(root: &Path) -> Vec<Entry> {
    let Ok(rd) = fs::read_dir(root) else { return Vec::new() };
    let mut out: Vec<Entry> = rd
        .filter_map(Result::ok)
        .filter_map(|e| fs::read_to_string(e.path().join("meta.json")).ok())
        .filter_map(|t| serde_json::from_str::<Entry>(&t).ok())
        .filter(|e| is_safe_id(&e.id) && root.join(&e.id).join("gameinfo.gi").is_file())
        .collect();
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    out
}

pub fn find(root: &Path, id: &str) -> Result<Entry, String> {
    list(root).into_iter().find(|e| e.id == id).ok_or_else(|| "That saved config no longer exists.".into())
}

pub fn read(root: &Path, id: &str) -> Result<PackFiles, String> {
    let d = entry_dir(root, id)?;
    let gameinfo = fs::read_to_string(d.join("gameinfo.gi")).map_err(|_| "That saved config is missing its gameinfo.gi.".to_string())?;
    Ok(PackFiles { gameinfo, video: fs::read_to_string(d.join("video.txt")).ok() })
}

pub fn delete(root: &Path, id: &str) -> Result<(), String> {
    let d = entry_dir(root, id)?;
    fs::remove_dir_all(&d).map_err(|e| format!("Could not delete the config: {e}"))
}

pub fn rename(root: &Path, id: &str, name: &str, notes: &str) -> Result<Entry, String> {
    let entry = Entry { name: clean_name(name)?, notes: notes.trim().chars().take(MAX_NOTES_CHARS).collect(), ..find(root, id)? };
    let json = serde_json::to_string_pretty(&entry).map_err(|e| e.to_string())?;
    fsops::atomic_write(&entry_dir(root, id)?.join("meta.json"), json.as_bytes()).map_err(|e| format!("Could not save: {e}"))?;
    Ok(entry)
}

fn read_zip_file<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, wanted: &str) -> Result<Option<String>, String> {
    // Shortest matching path wins, so "pack/gameinfo.gi" beats "pack/old/gameinfo.gi".
    let mut names: Vec<String> = zip
        .file_names()
        .filter(|n| Path::new(n).file_name().is_some_and(|f| f.to_string_lossy().eq_ignore_ascii_case(wanted)))
        .map(str::to_string)
        .collect();
    names.sort_by_key(|n| n.len());
    let Some(name) = names.first() else { return Ok(None) };
    let file = zip.by_name(name).map_err(|e| format!("Can't read {name} in the zip: {e}"))?;
    if file.size() > MAX_FILE_BYTES {
        return Err(format!("{name} in the zip is too large to be a config."));
    }
    let mut text = String::new();
    file.take(MAX_FILE_BYTES).read_to_string(&mut text).map_err(|e| format!("Can't read {name}: {e}"))?;
    Ok(Some(text))
}

/// Reads a config from a folder or a .zip with gameinfo.gi (and optionally video.txt) inside.
pub fn read_import(path: &Path) -> Result<PackFiles, String> {
    if path.is_dir() {
        return crate::packs::read_local(path);
    }
    let is_zip = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    if !is_zip {
        return Err("Pick a .zip file or a folder with gameinfo.gi in it.".into());
    }
    let bytes = fs::read(path).map_err(|e| format!("Can't open {}: {e}", path.display()))?;
    files_from_archive("x.zip", &bytes)
}

/// Shortest-path entry named `wanted` (any case) from a list of (path, text) pairs.
fn pick<'a>(entries: &'a [(String, String)], wanted: &str) -> Option<&'a String> {
    entries
        .iter()
        .filter(|(p, _)| Path::new(p).file_name().is_some_and(|f| f.to_string_lossy().eq_ignore_ascii_case(wanted)))
        .min_by_key(|(p, _)| p.len())
        .map(|(_, t)| t)
}

/// gameinfo.gi (and video.txt) from a downloaded file: a .zip, a .7z or a bare .gi.
pub fn files_from_archive(name: &str, bytes: &[u8]) -> Result<PackFiles, String> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".gi") {
        let gameinfo = String::from_utf8_lossy(bytes).into_owned();
        return Ok(PackFiles { gameinfo, video: None });
    }
    if lower.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("That isn't a readable zip file: {e}"))?;
        let gameinfo = read_zip_file(&mut zip, "gameinfo.gi")?.ok_or("The zip has no gameinfo.gi.")?;
        let video = read_zip_file(&mut zip, "video.txt")?;
        return Ok(PackFiles { gameinfo, video });
    }
    if lower.ends_with(".7z") {
        let mut found: Vec<(String, String)> = Vec::new();
        let mut reader = sevenz_rust::SevenZReader::new(std::io::Cursor::new(bytes), bytes.len() as u64, sevenz_rust::Password::empty())
            .map_err(|e| format!("That isn't a readable 7z file: {e}"))?;
        reader
            .for_each_entries(|entry, r| {
                let file = Path::new(entry.name()).file_name().map(|f| f.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                if !entry.is_directory() && (file == "gameinfo.gi" || file == "video.txt") && entry.size() <= MAX_FILE_BYTES {
                    let mut text = String::new();
                    r.take(MAX_FILE_BYTES).read_to_string(&mut text)?;
                    found.push((entry.name().to_string(), text));
                } else {
                    std::io::copy(r, &mut std::io::sink())?;
                }
                Ok(true)
            })
            .map_err(|e| format!("Couldn't unpack the 7z file: {e}"))?;
        let gameinfo = pick(&found, "gameinfo.gi").cloned().ok_or("The 7z file has no gameinfo.gi.")?;
        return Ok(PackFiles { gameinfo, video: pick(&found, "video.txt").cloned() });
    }
    Err(format!("{name} isn't a .zip, .7z or .gi file."))
}

/// Writes a saved config as a zip with gameinfo.gi, video.txt (if any) and a short README.
pub fn export_zip(root: &Path, id: &str, out: &Path) -> Result<(), String> {
    let entry = find(root, id)?;
    let files = read(root, id)?;
    let file = fs::File::create(out).map_err(|e| format!("Can't create {}: {e}", out.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut add = |name: &str, text: &str| -> Result<(), String> {
        zip.start_file(name, opts).map_err(|e| e.to_string())?;
        zip.write_all(text.as_bytes()).map_err(|e| e.to_string())
    };
    add("gameinfo.gi", &files.gameinfo)?;
    if let Some(v) = &files.video {
        add("video.txt", v)?;
    }
    let readme = format!(
        "{}\r\n\r\n{}\r\nExported from Deadlock Config Manager. Import this zip in the app (My configs > Import),\r\nor copy gameinfo.gi to Deadlock\\game\\citadel. video.txt has no GPU lines; the game adds them.\r\n",
        entry.name, entry.notes
    );
    add("README.txt", &readme)?;
    zip.finish().map_err(|e| format!("Couldn't finish the zip: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GI: &str = include_str!("../fixtures/pack_stable/gameinfo.gi");
    const VIDEO: &str = include_str!("../fixtures/user_video.txt");

    fn files() -> PackFiles {
        PackFiles { gameinfo: GI.into(), video: Some(VIDEO.into()) }
    }

    #[test]
    fn saves_lists_reads_and_deletes() {
        let tmp = tempfile::tempdir().unwrap();
        let e = save(tmp.path(), "  My comp config ", "for ranked", &files(), "Saved from test").unwrap();
        assert_eq!(e.name, "My comp config");
        assert!(e.id.starts_with("my-comp-config-"));
        assert_eq!(list(tmp.path()), vec![e.clone()]);
        let back = read(tmp.path(), &e.id).unwrap();
        assert_eq!(back.gameinfo, GI);
        // Hardware lines are not stored.
        assert!(!back.video.unwrap().contains("DeviceID"));
        let renamed = rename(tmp.path(), &e.id, "Other", "").unwrap();
        assert_eq!(find(tmp.path(), &e.id).unwrap().name, renamed.name);
        delete(tmp.path(), &e.id).unwrap();
        assert!(list(tmp.path()).is_empty());
    }

    #[test]
    fn rejects_bad_names_and_ids() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(save(tmp.path(), "   ", "", &files(), "").is_err());
        assert!(read(tmp.path(), "../settings").is_err());
        assert!(delete(tmp.path(), "..").is_err());
    }

    #[test]
    fn zip_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let e = save(tmp.path(), "Zip me", "", &files(), "").unwrap();
        let out = tmp.path().join("out.zip");
        export_zip(tmp.path(), &e.id, &out).unwrap();
        let back = read_import(&out).unwrap();
        assert_eq!(back.gameinfo, GI);
        assert!(back.video.is_some());
        assert!(read_import(&tmp.path().join("nope.txt")).is_err());
    }
}
