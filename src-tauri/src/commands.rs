//! Tauri commands that change files: apply, restore, and opening folders.
//! Every command returns `Result<_, String>`; the UI shows the string in a toast.

use crate::apply::{self, PackFiles, VideoInput};
use crate::backup::{self, BackupMeta, Sources};
use crate::diag;
use crate::diff::{self, FileDiff};
use crate::gameinfo;
use crate::overview::resolve;
use crate::packs::{self, PackInfo};
use crate::ranked;
use crate::stock;
use crate::settings::{self, AppliedRecord, Settings};
use crate::writer::{self, FileWrite, WriteOutcome};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::State;

pub struct AppState {
    pub dir: PathBuf,
    pub settings: Mutex<Settings>,
}

impl AppState {
    pub fn snapshot(&self) -> Settings {
        self.settings.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn update(&self, change: impl FnOnce(Settings) -> Settings) -> Result<Settings, String> {
        let mut guard = self.settings.lock().map_err(|_| "Settings are locked by a failed operation.")?;
        let next = change(guard.clone());
        settings::save(&self.dir, &next)?;
        *guard = next.clone();
        Ok(next)
    }

    /// Overview including the checks that read the cache (ranked safety).
    pub fn overview(&self, s: &Settings) -> crate::overview::Overview {
        crate::overview::resolve_with_checks(s, &self.dir.join("cache"))
    }

    fn backups_dir(&self) -> PathBuf {
        self.dir.join("backups")
    }
}

/// What Apply would do, shown before anything is written.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub pack_name: String,
    pub files: Vec<FileDiff>,
    pub warnings: Vec<String>,
    /// Engine-section entries that differ from Valve's file after applying; these can block ranked.
    pub ranked_changes: Vec<diff::Change>,
    /// This preview is for the ranked-safe variant of the pack.
    pub ranked_safe: bool,
    /// Descriptions and "ignored by the game" flags for changed convars, keyed by change path.
    pub convars: std::collections::BTreeMap<String, crate::convars::ConvarInfo>,
    /// Game build the convar data comes from.
    pub convar_build: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ApplyResponse {
    /// Nothing written yet; call again with `confirmed: true` to apply.
    Preview { preview: Preview },
    Done { message: String, notes: Vec<String> },
}

struct Source {
    repo: Option<String>,
    id: String,
    name: String,
    revision: Option<String>,
    files: PackFiles,
    notes: Vec<String>,
    ranked_safe: bool,
}

fn real_writer(jobs: PathBuf) -> impl Fn(&[FileWrite]) -> Result<WriteOutcome, String> {
    move |files| writer::write_files(files, &|rest| writer::elevate_via_runas(&jobs, rest))
}

fn ensure_closed() -> Result<(), String> {
    if crate::fsops::deadlock_running() {
        return Err("Close Deadlock first. Files can't be changed while the game is running.".into());
    }
    Ok(())
}

fn read_optional(path: &Path, what: &str) -> Result<Option<String>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    fs::read_to_string(path).map(Some).map_err(|e| format!("Cannot read {what}: {e}"))
}

fn apply_source(state: &AppState, src: Source, confirmed: bool) -> Result<ApplyResponse, String> {
    ensure_closed()?;
    let s = state.snapshot();
    let ov = resolve(&s);
    let gi_path = ov
        .gameinfo_path
        .filter(|_| ov.gameinfo_found || ov.gameinfo_creatable)
        .map(PathBuf::from)
        .ok_or("Deadlock wasn't found. Use Locate… next to gameinfo.gi to point at your install.")?;
    let video_path = ov.video_path.map(PathBuf::from);
    let installed = read_optional(&gi_path, "gameinfo.gi")?;
    let user_video = match &video_path {
        Some(p) => read_optional(p, "video.txt")?,
        None => None,
    };
    let video_in = match (&video_path, &user_video) {
        (Some(_), Some(text)) => VideoInput::Existing(text),
        (Some(_), None) => VideoInput::Create,
        (None, _) => VideoInput::Skip,
    };

    let stock = stock::load(&state.dir.join("cache"), installed.as_deref());
    let files = if src.ranked_safe {
        PackFiles { gameinfo: ranked::make_safe(&stock, &src.files.gameinfo)?, video: src.files.video.clone() }
    } else {
        src.files
    };
    let plan = apply::build_plan(&files, installed.as_deref(), video_in, &s.preserved_keys)?;
    if !confirmed {
        let gameinfo_diff = diff::diff_file("gameinfo.gi", installed.as_deref(), &plan.gameinfo_out);
        let convars = crate::convars::infos_for(&gameinfo_diff.changes);
        let files = std::iter::once(gameinfo_diff)
            .chain(plan.video_out.as_deref().map(|v| diff::diff_file("video.txt", user_video.as_deref(), v)))
            .collect();
        let ranked_changes = ranked::guarded_changes(&stock, &plan.gameinfo_out).unwrap_or_default();
        let preview = Preview {
            pack_name: src.name,
            files,
            warnings: plan.warnings,
            ranked_changes,
            ranked_safe: src.ranked_safe,
            convars,
            convar_build: crate::convars::build().to_string(),
        };
        return Ok(ApplyResponse::Preview { preview });
    }

    let backups = state.backups_dir();
    let sources = Sources { gameinfo: &gi_path, video: video_path.as_deref() };
    let meta = backup::create(&backups, &sources, Some(&src.name), &format!("Before applying {}", src.name))?;
    let writes = apply::plan_writes(&plan, &gi_path, video_path.as_deref());
    let write = real_writer(state.dir.join("jobs"));
    let rollback = || backup::restore(&backups, &meta, &|files| write(files).map(|_| ()));
    let outcome = apply::write_verified(&writes, &write, &|| apply::verify(&gi_path, video_path.as_deref(), &plan), &rollback)?;

    let record = AppliedRecord {
        repo: src.repo,
        pack_id: src.id,
        pack_name: src.name.clone(),
        revision: src.revision,
        applied_at: chrono::Local::now().to_rfc3339(),
        gameinfo_sha256: gameinfo::sha256_hex(plan.gameinfo_out.as_bytes()),
        gameinfo_normalized_sha256: gameinfo::normalized_hash(&plan.gameinfo_out),
        video_sha256: plan.video_out.as_ref().map(|v| gameinfo::sha256_hex(v.as_bytes())),
        ranked_safe: src.ranked_safe,
    };
    state.update(|s| Settings { applied: Some(record), ..s })?;

    let notes = src
        .notes
        .into_iter()
        .chain(outcome.readonly_cleared.iter().map(|p| format!("Cleared the read-only flag on {p} so Deadlock Mod Manager can still edit it.")))
        .collect();
    let what = if plan.video_out.is_some() { "gameinfo.gi and video.txt" } else { "gameinfo.gi" };
    let mode = if src.ranked_safe { ", ranked-safe" } else { "" };
    Ok(ApplyResponse::Done { message: format!("Applied {} ({what}{mode}). Backup saved.", src.name), notes })
}

#[tauri::command]
pub async fn apply_pack(state: State<'_, AppState>, pack: PackInfo, confirmed: bool, ranked_safe: bool) -> Result<ApplyResponse, String> {
    ensure_closed()?;
    let cache = state.dir.join("cache");
    let p = pack.clone();
    let fetched = tauri::async_runtime::spawn_blocking(move || packs::fetch_pack(&p.repo, &p, &cache))
        .await
        .map_err(|e| e.to_string())?;
    let fetched = diag::logged(&format!("download {}#{}", pack.repo, pack.id), fetched)?;
    let notes = if fetched.from_cache && pack.revision != fetched.revision {
        vec!["GitHub was unreachable, so the cached copy of this pack was used.".to_string()]
    } else {
        vec![]
    };
    let src = Source {
        repo: Some(pack.repo),
        id: pack.id,
        name: pack.name,
        revision: fetched.revision,
        files: fetched.files,
        notes,
        ranked_safe,
    };
    let label = format!("apply {}#{}", src.repo.as_deref().unwrap_or("?"), src.id);
    logged_apply(&label, apply_source(&state, src, confirmed))
}

#[tauri::command]
pub fn apply_local(state: State<AppState>, folder: String, confirmed: bool, ranked_safe: bool) -> Result<ApplyResponse, String> {
    let dir = PathBuf::from(&folder);
    let files = diag::logged("read local folder", packs::read_local(&dir))?;
    let name = dir.file_name().map(|n| format!("Local: {}", n.to_string_lossy())).unwrap_or_else(|| "Local folder".into());
    let src = Source { repo: None, id: "local".into(), name, revision: None, files, notes: vec![], ranked_safe };
    logged_apply("apply local folder", apply_source(&state, src, confirmed))
}

/// Records completed applies and failures; previews aren't worth a log line.
fn logged_apply(what: &str, result: Result<ApplyResponse, String>) -> Result<ApplyResponse, String> {
    if let Ok(ApplyResponse::Done { message, .. }) = &result {
        diag::info(&format!("{what}: {message}"));
    }
    diag::logged(what, result)
}

#[tauri::command]
pub fn list_backups(state: State<AppState>) -> Vec<BackupMeta> {
    backup::list(&state.backups_dir())
}

#[tauri::command]
pub fn restore_backup(state: State<AppState>, id: String) -> Result<String, String> {
    let result = restore_inner(&state, &id);
    if let Ok(msg) = &result {
        diag::info(&format!("restore {id}: {msg}"));
    }
    diag::logged(&format!("restore {id}"), result)
}

fn restore_inner(state: &AppState, id: &str) -> Result<String, String> {
    ensure_closed()?;
    let backups = state.backups_dir();
    let meta = backup::find(&backups, id).ok_or("That backup no longer exists.")?;
    let targets: Vec<PathBuf> = [&meta.gameinfo_path, &meta.video_path]
        .into_iter()
        .flatten()
        .chain(&meta.absent)
        .map(PathBuf::from)
        .collect();
    if targets.is_empty() {
        return Err("That backup contains no files.".into());
    }
    let gi = targets.iter().find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("gameinfo.gi")));
    let vid = targets.iter().find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("video.txt")));
    let current = Sources { gameinfo: gi.map_or(Path::new(""), PathBuf::as_path), video: vid.map(PathBuf::as_path) };
    let safety = backup::create(&backups, &current, None, &format!("Before restoring {}", meta.id))?;

    let write = real_writer(state.dir.join("jobs"));
    let write_unit = |files: &[FileWrite]| write(files).map(|_| ());
    if let Err(e) = backup::restore(&backups, &meta, &write_unit) {
        let undo = backup::restore(&backups, &safety, &write_unit);
        return Err(match undo {
            Ok(()) => format!("{e} Your files were left as they were."),
            Err(u) => format!("{e} Putting the files back also failed ({u})."),
        });
    }
    if !backup::matches_disk(&backups, &meta)? {
        let _ = backup::restore(&backups, &safety, &write_unit);
        return Err("The restored files don't match the backup, so the previous files were put back.".into());
    }
    state.update(|s| Settings { applied: None, ..s })?;
    let label = if meta.original { "your original files".to_string() } else { format!("the backup from {}", meta.id) };
    Ok(format!("Restored {label}."))
}

/// Opens a GitHub page in the default browser. Only github.com URLs are allowed.
#[tauri::command]
pub fn open_github(url: String) -> Result<(), String> {
    let ok = url.starts_with("https://github.com/")
        && url.chars().all(|c| c.is_ascii_alphanumeric() || "/:._-~".contains(c));
    if !ok {
        return Err("Only github.com links can be opened.".into());
    }
    std::process::Command::new("explorer.exe")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open the browser: {e}"))
}

/// Opens Explorer at a known location: "game", "video", "userdata" or "backups".
#[tauri::command]
pub fn open_location(state: State<AppState>, which: String) -> Result<(), String> {
    let ov = resolve(&state.snapshot());
    let target = match which.as_str() {
        "game" => ov.gameinfo_path.map(PathBuf::from),
        "video" => ov.video_path.map(PathBuf::from),
        "userdata" => ov.steam_root.map(|r| PathBuf::from(r).join("userdata")),
        "backups" => {
            let dir = state.backups_dir();
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            Some(dir)
        }
        _ => None,
    }
    .ok_or("That location isn't known yet.")?;
    let mut cmd = std::process::Command::new("explorer.exe");
    if target.is_file() {
        // Explorer only understands `/select,"path"`; the default quoting of the whole
        // argument makes it give up and open Documents instead.
        use std::os::windows::process::CommandExt;
        cmd.raw_arg(format!("/select,\"{}\"", target.display()));
    } else {
        // Walk up to the nearest folder that exists (a new account's cfg folder may not yet).
        let dir = target.ancestors().find(|p| p.is_dir()).map(Path::to_path_buf);
        let Some(dir) = dir else {
            return Err(format!("{} doesn't exist yet.", target.display()));
        };
        cmd.arg(dir);
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("Could not open Explorer: {e}"))
}

/// Plain-text report for bug reports: versions, resolved paths and recent log lines,
/// with the Windows user name replaced by `<user>`.
#[tauri::command]
pub fn get_diagnostics(state: State<AppState>) -> String {
    let s = state.snapshot();
    let ov = resolve(&s);
    let yes_no = |b: bool| if b { "found" } else { "missing" }.to_string();
    let path = |p: &Option<String>| p.clone().unwrap_or_else(|| "(not detected)".into());
    let applied = s
        .applied
        .as_ref()
        .map(|a| format!("{} from {} at {}", a.pack_name, a.repo.as_deref().unwrap_or("local"), a.applied_at))
        .unwrap_or_else(|| "nothing".into());
    let user = std::env::var("USERNAME").ok();
    let windows = diag::windows_version();
    diag::report(&diag::ReportInput {
        app_version: env!("CARGO_PKG_VERSION"),
        windows: &windows,
        lines: vec![
            ("Steam", path(&ov.steam_root)),
            ("gameinfo.gi", format!("{} ({})", path(&ov.gameinfo_path), yes_no(ov.gameinfo_found))),
            ("video.txt", format!("{} ({})", path(&ov.video_path), yes_no(ov.video_found))),
            ("Accounts", ov.accounts.len().to_string()),
            ("Status", format!("{:?}", ov.status)),
            ("Applied", applied),
            ("Repos", s.effective_repos().join(", ")),
            ("Deadlock running", ov.deadlock_running.to_string()),
            ("Install", install_kind().to_string()),
        ],
        log: diag::report_log(&state.dir),
        user: user.as_deref(),
    })
}

/// "installer", "scoop" or "portable"; the UI only offers in-app updates for "installer".
#[tauri::command]
pub fn install_kind() -> &'static str {
    std::env::current_exe().map(|exe| diag::install_kind(&exe)).unwrap_or("portable")
}

/// Puts the engine sections of the installed gameinfo.gi back to Valve's defaults and keeps the rest
/// (ConVars and so on). Same preview/confirm flow and backup as applying a pack; video.txt is untouched.
#[tauri::command]
pub fn make_ranked_safe(state: State<AppState>, confirmed: bool) -> Result<ApplyResponse, String> {
    let ov = resolve(&state.snapshot());
    let path = ov.gameinfo_path.filter(|_| ov.gameinfo_found).ok_or("gameinfo.gi wasn't found.")?;
    let installed = fs::read_to_string(&path).map_err(|e| format!("Cannot read gameinfo.gi: {e}"))?;
    let applied = state.snapshot().applied;
    let src = Source {
        repo: applied.as_ref().and_then(|a| a.repo.clone()),
        id: applied.as_ref().map_or("installed".into(), |a| a.pack_id.clone()),
        name: applied.as_ref().map_or("your current gameinfo.gi".into(), |a| a.pack_name.clone()),
        revision: applied.as_ref().and_then(|a| a.revision.clone()),
        files: PackFiles { gameinfo: installed, video: None },
        notes: vec![],
        ranked_safe: true,
    };
    logged_apply("make ranked-safe", apply_source(&state, src, confirmed))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackCheck {
    /// `owner/name#packId`, same as pin keys.
    pub key: String,
    /// Engine-section entries the pack changes; None when the pack couldn't be read.
    pub ranked_changes: Option<usize>,
}

/// Ranked-safety for each pack, for the badges on pack rows. Uses cached pack files when the
/// revision hasn't changed, so repeated checks don't download anything.
#[tauri::command]
pub async fn check_packs(state: State<'_, AppState>, packs: Vec<PackInfo>) -> Result<Vec<PackCheck>, String> {
    let cache = state.dir.join("cache");
    let installed = resolve(&state.snapshot()).gameinfo_path.and_then(|p| fs::read_to_string(p).ok());
    tauri::async_runtime::spawn_blocking(move || {
        let stock = stock::load(&cache, installed.as_deref());
        packs
            .iter()
            .map(|p| PackCheck {
                key: format!("{}#{}", p.repo, p.id),
                ranked_changes: packs::fetch_pack(&p.repo, p, &cache)
                    .ok()
                    .and_then(|f| ranked::guarded_changes(&stock, &f.files.gameinfo).ok())
                    .map(|c| c.len()),
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}
