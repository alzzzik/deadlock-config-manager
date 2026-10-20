//! Building the output files, writing them with verification, and status checks.

use crate::gameinfo;
use crate::settings::AppliedRecord;
use crate::video;
use crate::writer::{FileWrite, WriteOutcome};
use serde::Serialize;
use std::fs;
use std::path::Path;

pub struct PackFiles {
    pub gameinfo: String,
    pub video: Option<String>,
}

#[derive(Debug)]
pub struct Plan {
    pub gameinfo_out: String,
    pub video_out: Option<String>,
    pub expected_preserved: Vec<(String, String)>,
    pub warnings: Vec<String>,
}

/// What to do about the account's video.txt.
pub enum VideoInput<'a> {
    /// The user's current file, to merge with the pack.
    Existing(&'a str),
    /// No file yet: create one from the pack (without hardware lines).
    Create,
    /// No account/location known: write gameinfo.gi only.
    Skip,
}

pub const CREATE_VIDEO_WARNING: &str = "This account has no video.txt yet, so one will be created from the pack \
without the GPU lines (VendorID/DeviceID). Deadlock fills those in on first launch and may reset some graphics \
settings; if it does, launch once and Re-apply.";

/// Validates the pack and builds both output files. Nothing is written here.
pub fn build_plan(pack: &PackFiles, installed_gameinfo: Option<&str>, video_in: VideoInput, preserved: &[String]) -> Result<Plan, String> {
    let gameinfo_out = gameinfo::enforce(&pack.gameinfo).map_err(|e| format!("Pack {e}"))?;
    let mut warnings = Vec::new();

    let pack_pgi = gameinfo::pgi_version(&pack.gameinfo);
    let installed_pgi = installed_gameinfo.and_then(gameinfo::pgi_version);
    if let (Some(p), Some(i)) = (&pack_pgi, &installed_pgi) {
        if !p.eq_ignore_ascii_case(i) {
            warnings.push("This pack was made for a different game version (PGIVersion differs from your installed gameinfo.gi).".into());
        }
    }
    if installed_gameinfo.is_none() {
        warnings.push("gameinfo.gi is missing, so the pack's file will be created in the game folder.".into());
    }

    let (video_out, expected_preserved) = match (pack.video.as_deref(), video_in) {
        (Some(pv), VideoInput::Existing(uv)) => (Some(video::merge(pv, uv, preserved)?), video::expected_preserved(uv, preserved)?),
        (Some(pv), VideoInput::Create) => {
            warnings.push(CREATE_VIDEO_WARNING.into());
            (Some(video::new_from_pack(pv)?), Vec::new())
        }
        (Some(pv), VideoInput::Skip) => {
            video::validate_pack(pv)?;
            warnings.push("No Steam account was found for video.txt, so only gameinfo.gi will be written.".into());
            (None, Vec::new())
        }
        (None, _) => {
            warnings.push("This pack has no video.txt; only gameinfo.gi will be written.".into());
            (None, Vec::new())
        }
    };
    Ok(Plan { gameinfo_out, video_out, expected_preserved, warnings })
}

/// Re-reads the written files and checks they parse, keep the preserved keys, have no placeholders
/// and have the required SearchPaths.
pub fn verify(gameinfo_path: &Path, video_path: Option<&Path>, plan: &Plan) -> Result<(), String> {
    let gi = fs::read_to_string(gameinfo_path).map_err(|e| format!("Cannot re-read gameinfo.gi: {e}"))?;
    if !gameinfo::search_paths_ok(&gi)? {
        return Err("gameinfo.gi on disk does not have the required SearchPaths block.".into());
    }
    if let (Some(path), Some(_)) = (video_path, &plan.video_out) {
        let text = fs::read_to_string(path).map_err(|e| format!("Cannot re-read video.txt: {e}"))?;
        let values = video::read_values(&text).map_err(|e| format!("Written video.txt {e}"))?;
        if values.iter().any(|(_, v)| video::is_placeholder(v)) {
            return Err("video.txt on disk still has a placeholder value.".into());
        }
        for (k, v) in &plan.expected_preserved {
            let found = values.iter().find(|(key, _)| key.eq_ignore_ascii_case(k)).map(|(_, val)| val);
            if found != Some(v) {
                return Err(format!("video.txt on disk lost your \"{k}\" value."));
            }
        }
    }
    Ok(())
}

pub fn plan_writes(plan: &Plan, gameinfo_path: &Path, video_path: Option<&Path>) -> Vec<FileWrite> {
    let gi = (gameinfo_path.to_path_buf(), plan.gameinfo_out.clone().into_bytes());
    let vid = video_path.zip(plan.video_out.as_ref()).map(|(p, v)| (p.to_path_buf(), v.clone().into_bytes()));
    std::iter::once(gi).chain(vid).collect()
}

type Writer<'a> = &'a dyn Fn(&[FileWrite]) -> Result<WriteOutcome, String>;

/// Writes, verifies and on any failure calls `rollback` (restore the backup just taken).
/// After a failed write, rollback only runs if something actually changed on disk.
pub fn write_verified(files: &[FileWrite], write: Writer, check: &dyn Fn() -> Result<(), String>, rollback: &dyn Fn() -> Result<(), String>) -> Result<WriteOutcome, String> {
    let before: Vec<Option<Vec<u8>>> = files.iter().map(|(p, _)| fs::read(p).ok()).collect();
    let restore = |reason: String| -> String {
        match rollback() {
            Ok(()) => format!("{reason} Your previous files were restored from the backup."),
            Err(e) => format!("{reason} Restoring the backup also failed ({e}); use Restore to put it back."),
        }
    };
    let outcome = write(files).map_err(|e| {
        // A declined or failed write may have partially succeeded before the failing file.
        let changed = files.iter().zip(&before).any(|((path, new), old)| {
            old.as_ref() != Some(new) && fs::read(path).ok().as_ref() == Some(new)
        });
        if changed {
            restore(e)
        } else {
            e
        }
    })?;
    check().map_err(|e| restore(format!("Verification failed: {e}")))?;
    Ok(outcome)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Status {
    NotApplied,
    Missing,
    Applied { pack: String },
    AppliedDmm { pack: String },
    Changed { pack: String },
}

pub fn status(gameinfo_path: Option<&Path>, applied: Option<&AppliedRecord>) -> Status {
    let Some(text) = gameinfo_path.and_then(|p| fs::read(p).ok()) else {
        return Status::Missing;
    };
    let Some(rec) = applied else {
        return Status::NotApplied;
    };
    let pack = rec.pack_name.clone();
    if gameinfo::sha256_hex(&text) == rec.gameinfo_sha256 {
        return Status::Applied { pack };
    }
    let text = String::from_utf8_lossy(&text);
    if gameinfo::has_dmm_markers(&text) && gameinfo::normalized_hash(&text) == rec.gameinfo_normalized_sha256 {
        return Status::AppliedDmm { pack };
    }
    Status::Changed { pack }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::{write_without_elevation, FileWrite};
    use std::path::PathBuf;

    const PACK_GI: &str = include_str!("../fixtures/pack_stable/gameinfo.gi");
    const PACK_VIDEO: &str = include_str!("../fixtures/pack_stable/video.txt");
    const USER_VIDEO: &str = include_str!("../fixtures/user_video.txt");
    const DMM: &str = include_str!("../fixtures/gameinfo_dmm.gi");

    fn pack() -> PackFiles {
        PackFiles { gameinfo: PACK_GI.into(), video: Some(PACK_VIDEO.into()) }
    }

    fn plan() -> Plan {
        let keys: Vec<String> = video::DEFAULT_PRESERVED.iter().map(|s| s.to_string()).collect();
        build_plan(&pack(), Some(DMM), VideoInput::Existing(USER_VIDEO), &keys).unwrap()
    }

    /// Rollback that writes the given originals back and deletes paths that didn't exist.
    fn rollback_to(originals: Vec<FileWrite>, absent: Vec<PathBuf>) -> impl Fn() -> Result<(), String> {
        move || {
            write_without_elevation(&originals)?;
            absent.iter().filter(|p| p.exists()).try_for_each(|p| fs::remove_file(p).map_err(|e| e.to_string()))
        }
    }

    #[test]
    fn warns_on_pgi_mismatch_and_skipped_video() {
        let p = plan();
        assert!(p.warnings.iter().any(|w| w.contains("different game version")));
        let skipped = build_plan(&pack(), Some(PACK_GI), VideoInput::Skip, &[]).unwrap();
        assert!(skipped.video_out.is_none());
        assert_eq!(skipped.warnings.len(), 1);
    }

    #[test]
    fn creates_video_and_gameinfo_when_missing() {
        let p = build_plan(&pack(), None, VideoInput::Create, &[]).unwrap();
        let video_out = p.video_out.as_deref().unwrap();
        assert!(!video_out.contains("VendorID"));
        assert!(p.warnings.iter().any(|w| w.contains("gameinfo.gi is missing")));
        assert!(p.warnings.iter().any(|w| w == CREATE_VIDEO_WARNING));

        let dir = tempfile::tempdir().unwrap();
        let gi = dir.path().join("gameinfo.gi");
        let vid = dir.path().join(r"cfg\video.txt");
        write_verified(&plan_writes(&p, &gi, Some(&vid)), &write_without_elevation, &|| verify(&gi, Some(&vid), &p), &|| Ok(()))
            .unwrap();
        assert!(vid.is_file());
    }

    #[test]
    fn refuses_unparseable_pack_with_line() {
        let bad = PackFiles { gameinfo: "GameInfo\n{\n  a \"b\"\n".into(), video: None };
        let err = build_plan(&bad, None, VideoInput::Skip, &[]).unwrap_err();
        assert!(err.contains("line 2"), "{err}");
    }

    #[test]
    fn writes_and_verifies_then_reports_applied() {
        let dir = tempfile::tempdir().unwrap();
        let gi = dir.path().join("gameinfo.gi");
        let vid = dir.path().join("video.txt");
        fs::write(&gi, DMM).unwrap();
        fs::write(&vid, USER_VIDEO).unwrap();
        let p = plan();
        write_verified(&plan_writes(&p, &gi, Some(&vid)), &write_without_elevation, &|| verify(&gi, Some(&vid), &p), &|| Ok(()))
            .unwrap();

        let rec = AppliedRecord {
            repo: Some("alzzzik/alz-deadlock-cfg".into()),
            pack_id: "stable".into(),
            pack_name: "Stable".into(),
            revision: None,
            applied_at: String::new(),
            gameinfo_sha256: gameinfo::sha256_hex(p.gameinfo_out.as_bytes()),
            gameinfo_normalized_sha256: gameinfo::normalized_hash(&p.gameinfo_out),
            video_sha256: None,
        };
        assert_eq!(status(Some(&gi), Some(&rec)), Status::Applied { pack: "Stable".into() });
        fs::write(&gi, DMM).unwrap();
        assert_eq!(status(Some(&gi), Some(&rec)), Status::Changed { pack: "Stable".into() });
        assert_eq!(status(Some(&gi), None), Status::NotApplied);
        assert_eq!(status(Some(&dir.path().join("nope")), Some(&rec)), Status::Missing);
    }

    #[test]
    fn verification_failure_restores_backup_and_removes_created_files() {
        let dir = tempfile::tempdir().unwrap();
        let gi = dir.path().join("gameinfo.gi");
        let vid = dir.path().join(r"cfg\video.txt");
        fs::write(&gi, DMM).unwrap();
        let p = build_plan(&pack(), Some(DMM), VideoInput::Create, &[]).unwrap();
        let rollback = rollback_to(vec![(gi.clone(), DMM.as_bytes().to_vec())], vec![vid.clone()]);

        let err = write_verified(&plan_writes(&p, &gi, Some(&vid)), &write_without_elevation, &|| Err("simulated".into()), &rollback)
            .unwrap_err();
        assert!(err.contains("simulated") && err.contains("restored"), "{err}");
        assert_eq!(fs::read_to_string(&gi).unwrap(), DMM);
        assert!(!vid.exists());
    }

    #[test]
    fn failed_write_that_changed_nothing_skips_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let gi = dir.path().join("gameinfo.gi");
        fs::write(&gi, DMM).unwrap();
        let p = plan();
        let rolled_back = std::cell::Cell::new(false);
        let err = write_verified(
            &plan_writes(&p, &gi, None),
            &|_| Err("declined".into()),
            &|| Ok(()),
            &|| {
                rolled_back.set(true);
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(err, "declined");
        assert!(!rolled_back.get());
    }
}
