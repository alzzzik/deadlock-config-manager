//! Backups in %APPDATA%\DeadlockConfigManager\backups\<yyyy-MM-dd_HH-mm-ss>\.
//! The first backup ever is tagged `original` and never pruned; the newest 20 others are kept.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const KEEP_NON_ORIGINAL: usize = 20;
const GAMEINFO: &str = "gameinfo.gi";
const VIDEO: &str = "video.txt";
const META: &str = "meta.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupMeta {
    pub id: String,
    pub created_at: String,
    pub original: bool,
    /// Pack that was about to be applied when this backup was taken.
    pub pack_name: Option<String>,
    pub reason: String,
    /// Where each file was copied from (and where restore puts it back). None = file absent.
    pub gameinfo_path: Option<String>,
    pub video_path: Option<String>,
    /// Targets that did not exist when the backup was taken; restoring deletes them.
    #[serde(default)]
    pub absent: Vec<String>,
}

pub struct Sources<'a> {
    pub gameinfo: &'a Path,
    pub video: Option<&'a Path>,
}

fn unique_dir(root: &Path) -> (String, PathBuf) {
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    (1..)
        .map(|n| if n == 1 { stamp.clone() } else { format!("{stamp}_{n}") })
        .map(|id| (id.clone(), root.join(id)))
        .find(|(_, p)| !p.exists())
        .expect("unbounded iterator always yields a free name")
}

fn copy_if_exists(src: Option<&Path>, dest: &Path) -> Result<Option<String>, String> {
    match src {
        Some(p) if p.is_file() => {
            fs::copy(p, dest).map_err(|e| format!("Backup of {} failed: {e}", p.display()))?;
            Ok(Some(p.to_string_lossy().into_owned()))
        }
        _ => Ok(None),
    }
}

pub fn create(root: &Path, sources: &Sources, pack_name: Option<&str>, reason: &str) -> Result<BackupMeta, String> {
    fs::create_dir_all(root).map_err(|e| format!("Could not create backup folder: {e}"))?;
    let is_first = !list(root).iter().any(|b| b.original);
    let (id, dir) = unique_dir(root);
    fs::create_dir_all(&dir).map_err(|e| format!("Could not create backup folder: {e}"))?;

    let result = (|| {
        let meta = BackupMeta {
            id: id.clone(),
            created_at: chrono::Local::now().to_rfc3339(),
            original: is_first,
            pack_name: pack_name.map(str::to_string),
            reason: reason.to_string(),
            gameinfo_path: copy_if_exists(Some(sources.gameinfo), &dir.join(GAMEINFO))?,
            video_path: copy_if_exists(sources.video, &dir.join(VIDEO))?,
            absent: [Some(sources.gameinfo), sources.video]
                .into_iter()
                .flatten()
                .filter(|p| !p.as_os_str().is_empty() && !p.exists())
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
        };
        let json = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
        fs::write(dir.join(META), json).map_err(|e| format!("Could not write backup info: {e}"))?;
        Ok(meta)
    })();
    match result {
        Ok(meta) => {
            prune(root, KEEP_NON_ORIGINAL);
            Ok(meta)
        }
        Err(e) => {
            let _ = fs::remove_dir_all(&dir);
            Err(e)
        }
    }
}

/// All readable backups, newest first.
pub fn list(root: &Path) -> Vec<BackupMeta> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut metas: Vec<BackupMeta> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let meta: BackupMeta = serde_json::from_str(&fs::read_to_string(e.path().join(META)).ok()?).ok()?;
            (e.file_name() == meta.id.as_str() && is_sane(&meta)).then_some(meta)
        })
        .collect();
    metas.sort_by(|a, b| b.id.cmp(&a.id));
    metas
}

fn named(path: &str, names: &[&str]) -> bool {
    Path::new(path).file_name().and_then(|n| n.to_str()).is_some_and(|n| names.iter().any(|w| w.eq_ignore_ascii_case(n)))
}

/// Restore writes and deletes the paths in meta.json, so only the files the app backs up are accepted.
fn is_sane(meta: &BackupMeta) -> bool {
    meta.gameinfo_path.as_deref().is_none_or(|p| named(p, &[GAMEINFO]))
        && meta.video_path.as_deref().is_none_or(|p| named(p, &[VIDEO]))
        && meta.absent.iter().all(|p| named(p, &[GAMEINFO, VIDEO]))
}

pub fn prune(root: &Path, keep: usize) {
    list(root)
        .into_iter()
        .filter(|b| !b.original)
        .skip(keep)
        .for_each(|b| {
            let _ = fs::remove_dir_all(root.join(&b.id));
        });
}

/// (target path, backed-up bytes) for every file in the backup.
pub fn files_to_restore(root: &Path, meta: &BackupMeta) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    let dir = root.join(&meta.id);
    [(meta.gameinfo_path.as_ref(), GAMEINFO), (meta.video_path.as_ref(), VIDEO)]
        .into_iter()
        .filter_map(|(target, name)| target.map(|t| (PathBuf::from(t), dir.join(name))))
        .map(|(target, src)| {
            fs::read(&src)
                .map(|bytes| (target, bytes))
                .map_err(|e| format!("Backup file {} is unreadable: {e}", src.display()))
        })
        .collect()
}

/// Puts a backup back: writes its files with `write` and deletes files that did not exist then.
pub fn restore(root: &Path, meta: &BackupMeta, write: &dyn Fn(&[(PathBuf, Vec<u8>)]) -> Result<(), String>) -> Result<(), String> {
    let files = files_to_restore(root, meta)?;
    if !files.is_empty() {
        write(&files)?;
    }
    meta.absent.iter().map(PathBuf::from).filter(|p| p.exists()).try_for_each(|p| {
        fs::remove_file(&p).map_err(|e| format!("Could not remove {} (it didn't exist before): {e}", p.display()))
    })
}

/// True when the files on disk match the backup exactly (absent ones absent).
pub fn matches_disk(root: &Path, meta: &BackupMeta) -> Result<bool, String> {
    let files = files_to_restore(root, meta)?;
    let same = files.iter().all(|(p, bytes)| fs::read(p).ok().as_ref() == Some(bytes));
    Ok(same && meta.absent.iter().all(|p| !Path::new(p).exists()))
}

pub fn find(root: &Path, id: &str) -> Option<BackupMeta> {
    list(root).into_iter().find(|b| b.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_backup_is_original_and_survives_pruning() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("backups");
        let gi = tmp.path().join("gameinfo.gi");
        fs::write(&gi, "v0").unwrap();
        let sources = Sources { gameinfo: &gi, video: None };

        let first = create(&root, &sources, Some("Stable"), "apply").unwrap();
        assert!(first.original);
        assert_eq!(first.video_path, None);
        for i in 1..=4 {
            fs::write(&gi, format!("v{i}")).unwrap();
            assert!(!create(&root, &sources, None, "apply").unwrap().original);
        }
        prune(&root, 2);
        let left = list(&root);
        assert_eq!(left.len(), 3);
        assert!(left.iter().any(|b| b.original && b.id == first.id));

        let files = files_to_restore(&root, &find(&root, &first.id).unwrap()).unwrap();
        assert_eq!(files, vec![(gi.clone(), b"v0".to_vec())]);
    }

    #[test]
    fn ignores_backups_whose_info_was_tampered_with() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("backups");
        let gi = tmp.path().join("gameinfo.gi");
        fs::write(&gi, "v0").unwrap();
        let meta = create(&root, &Sources { gameinfo: &gi, video: None }, None, "apply").unwrap();
        let write_meta = |m: &BackupMeta| fs::write(root.join(&meta.id).join(META), serde_json::to_string(m).unwrap()).unwrap();

        write_meta(&BackupMeta { absent: vec![tmp.path().join("notes.docx").to_string_lossy().into_owned()], ..meta.clone() });
        assert!(list(&root).is_empty());
        write_meta(&BackupMeta { id: "..".into(), ..meta.clone() });
        assert!(list(&root).is_empty());
        write_meta(&meta);
        assert_eq!(list(&root), vec![meta]);
    }

    #[test]
    fn restoring_deletes_files_that_did_not_exist() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("backups");
        let gi = tmp.path().join("gameinfo.gi");
        let vid = tmp.path().join(r"cfg\video.txt");
        fs::write(&gi, "orig").unwrap();
        let meta = create(&root, &Sources { gameinfo: &gi, video: Some(&vid) }, None, "apply").unwrap();
        assert_eq!(meta.absent, vec![vid.to_string_lossy().into_owned()]);

        fs::write(&gi, "new").unwrap();
        fs::create_dir_all(vid.parent().unwrap()).unwrap();
        fs::write(&vid, "created").unwrap();
        assert!(!matches_disk(&root, &meta).unwrap());

        let write = |files: &[(PathBuf, Vec<u8>)]| {
            files.iter().try_for_each(|(p, b)| fs::write(p, b).map_err(|e| e.to_string()))
        };
        restore(&root, &meta, &write).unwrap();
        assert_eq!(fs::read_to_string(&gi).unwrap(), "orig");
        assert!(!vid.exists());
        assert!(matches_disk(&root, &meta).unwrap());
    }
}
