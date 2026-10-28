//! Tauri commands that change files: apply, restore, and opening folders.
//! Every command returns `Result<_, String>`; the UI shows the string in a toast.

use crate::apply::{self, PackFiles, VideoInput};
use crate::backup::{self, BackupMeta, Sources};
use crate::diag;
use crate::diff::{self, FileDiff};
use crate::gameinfo;
use crate::overview::resolve;
use crate::packs::{self, PackInfo};
use crate::overrides::{self, Overrides};
use crate::ranked;
use crate::stock;
use crate::settings::{self, AppliedRecord, Settings};
use crate::writer::{self, FileWrite, WriteOutcome};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{Manager, State};

pub struct AppState {
    pub dir: PathBuf,
    pub settings: Mutex<Settings>,
    /// Held for a whole apply or restore so the UI and the background re-apply never write at once.
    pub apply_lock: Mutex<()>,
    /// How far console.log has been read for performance reports.
    pub perf_log: Mutex<crate::perf::LogTail>,
    /// console.log shows matchmaking refused ranked; copied from `perf_log` after each read so status
    /// checks on the window's thread never wait for a read.
    pub ranked_refused: AtomicBool,
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
        crate::overview::resolve_with_checks(s, &self.dir.join("cache"), self.ranked_refused.load(Ordering::Relaxed))
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
    /// Number of personal tweaks applied on top of the pack.
    pub tweaks: usize,
    /// Overlay mode: settings and sections added from Valve's current file. None when overlay is off.
    pub overlay_added: Option<usize>,
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
    /// Background re-apply: no UAC prompt.
    quiet: bool,
    /// The pack's own autoexec.cfg, if it has one.
    autoexec: Option<String>,
    /// Applied because a setting changed: back-to-back changes share one backup.
    settings_change: bool,
}

/// Error from a background re-apply that was dropped because the applied config changed meanwhile.
pub const SUPERSEDED: &str = "The applied config changed while re-applying, so nothing was written.";

fn still_applied(rec: Option<&AppliedRecord>, src: &Source) -> bool {
    rec.is_some_and(|r| r.repo == src.repo && r.pack_id == src.id && r.ranked_safe == src.ranked_safe)
}

/// Runs a command that writes game files off the window's thread, so a UAC prompt or a slow disk
/// doesn't freeze the window.
async fn off_main<T: Send + 'static>(app: tauri::AppHandle, work: impl FnOnce(&AppState) -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || work(&app.state::<AppState>())).await.map_err(|e| e.to_string())?
}

/// `quiet` writes never show a UAC prompt (background re-apply); access denied is an error instead.
fn real_writer(jobs: PathBuf, quiet: bool) -> impl Fn(&[FileWrite]) -> Result<WriteOutcome, String> {
    move |files| {
        if quiet {
            writer::write_files(files, &|_| Err("Windows needs admin rights to change these files. Open the app and re-apply.".into()))
        } else {
            writer::write_files(files, &|rest| writer::elevate_via_runas(&jobs, rest))
        }
    }
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
    let _guard = if confirmed { Some(state.apply_lock.lock().map_err(|_| "Another apply failed; restart the app.")?) } else { None };
    ensure_closed()?;
    let s = state.snapshot();
    // A background re-apply was prepared (often after a download) before taking the lock; if the player
    // restored or applied something else meanwhile, it must not put the old config back.
    if src.quiet && !still_applied(s.applied.as_ref(), &src) {
        return Err(SUPERSEDED.into());
    }
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
    let safe_gameinfo = if src.ranked_safe { ranked::make_safe(&stock, &src.files.gameinfo)? } else { src.files.gameinfo.clone() };
    let (merged, added) = if s.overlay {
        let (text, added) = crate::overlay::merge(&stock, &safe_gameinfo)?;
        (text, Some(added))
    } else {
        (safe_gameinfo, None)
    };
    let tweaks = s.overrides.effective();
    let files = PackFiles {
        gameinfo: overrides::apply(&merged, &tweaks).map_err(|e| format!("Your tweaks couldn't be applied: {e}"))?,
        video: src.files.video.clone(),
    };
    let mut plan = apply::build_plan(&files, installed.as_deref(), video_in, &s.preserved_keys)?;
    // autoexec.cfg: (path, current text, new text), only when the pack has one.
    let autoexec_out = match (&src.autoexec, gi_path.parent()) {
        (Some(pack_exec), Some(dir)) => {
            let path = dir.join("cfg").join("autoexec.cfg");
            let old = read_optional(&path, "autoexec.cfg")?;
            let (lines, dropped) = crate::autoexec::filter(pack_exec);
            if dropped > 0 {
                plan.warnings.push(format!(
                    "{dropped} line(s) in the pack's autoexec.cfg were skipped (key binds, connect, exec and similar commands aren't copied)."
                ));
            }
            plan.warnings.push("This pack has an autoexec.cfg. Deadlock only runs it with the +exec autoexec launch option (Tweaks › Launch options).".into());
            let new = crate::autoexec::merge(old.as_deref(), &lines);
            Some((path, old, new))
        }
        _ => None,
    };
    if !confirmed {
        let gameinfo_diff = diff::diff_file("gameinfo.gi", installed.as_deref(), &plan.gameinfo_out);
        let convars = crate::convars::infos_for(&gameinfo_diff.changes);
        let autoexec_diff = autoexec_out.as_ref().map(|(_, old, new)| crate::autoexec::diff(old.as_deref(), new));
        let files = std::iter::once(gameinfo_diff)
            .chain(plan.video_out.as_deref().map(|v| diff::diff_file("video.txt", user_video.as_deref(), v)))
            .chain(autoexec_diff)
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
            tweaks: tweaks.len(),
            overlay_added: added.map(|a| a.convars + a.sections.len()),
        };
        return Ok(ApplyResponse::Preview { preview });
    }

    let backups = state.backups_dir();
    let sources = Sources { gameinfo: &gi_path, video: video_path.as_deref() };
    // Several setting changes in a row keep one "Before changing settings" backup instead of one each.
    let recent_settings_backup = src
        .settings_change
        .then(|| backup::list(&backups).into_iter().next())
        .flatten()
        .filter(|m| m.reason == SETTINGS_BACKUP_REASON)
        .filter(|m| chrono::DateTime::parse_from_rfc3339(&m.created_at).is_ok_and(|t| chrono::Local::now().signed_duration_since(t) < chrono::Duration::minutes(SETTINGS_BACKUP_MINUTES)));
    let reason = if src.settings_change { SETTINGS_BACKUP_REASON.to_string() } else { format!("Before applying {}", src.name) };
    let meta = match recent_settings_backup {
        Some(m) => m,
        None => backup::create(&backups, &sources, Some(&src.name), &reason)?,
    };
    let mut writes = apply::plan_writes(&plan, &gi_path, video_path.as_deref());
    if let Some((path, old, new)) = &autoexec_out {
        // The old autoexec.cfg is kept next to the backups; Restore… doesn't touch autoexec.cfg.
        if let Some(old) = old {
            let dir = backups.join("autoexec");
            let copy = dir.join(format!("{}.cfg", meta.id));
            // A shared settings backup keeps the copy from before the first change.
            if !copy.exists() {
                fs::create_dir_all(&dir)
                    .and_then(|_| fs::write(&copy, old))
                    .map_err(|e| format!("Could not back up autoexec.cfg, so nothing was changed: {e}"))?;
            }
        }
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        // Written last, so a failed gameinfo.gi/video.txt write never leaves it changed.
        writes.push((path.clone(), new.as_bytes().to_vec()));
    }
    let write = real_writer(state.dir.join("jobs"), src.quiet);
    // Roll back to exactly what was there before this write (the shared settings backup may be older).
    let before: Vec<FileWrite> = std::iter::once(installed.clone().map(|t| (gi_path.clone(), t.into_bytes())))
        .chain(std::iter::once(video_path.clone().zip(user_video.clone()).map(|(p, t)| (p, t.into_bytes()))))
        .flatten()
        .collect();
    let rollback = || {
        if src.settings_change && installed.is_some() {
            write(&before).map(|_| ())
        } else {
            backup::restore(&backups, &meta, &|files| write(files).map(|_| ()))
        }
    };
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
        game_build: crate::paths::game_build(&gi_path),
    };
    crate::launch_guard::arm(&state.dir, &meta.id, &src.name);
    if let Err(e) = save_applied_source(&state.dir, &src.files, src.autoexec.as_deref()) {
        diag::error(&format!("saving the applied config's files: {e}"));
    }
    state.update(|s| Settings { applied: Some(record), onboarded: true, ..s })?;

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
    let fetched = tauri::async_runtime::spawn_blocking(move || {
        packs::fetch_pack(&p.repo, &p, &cache).map(|f| {
            let autoexec = packs::fetch_autoexec(&p, f.revision.as_deref(), &cache);
            (f, autoexec)
        })
    })
    .await
    .map_err(|e| e.to_string())?;
    let (fetched, pack_autoexec) = diag::logged(&format!("download {}#{}", pack.repo, pack.id), fetched)?;
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
        quiet: false,
        autoexec: pack_autoexec,
        settings_change: false,
    };
    let label = format!("apply {}#{}", src.repo.as_deref().unwrap_or("?"), src.id);
    logged_apply(&label, apply_source(&state, src, confirmed))
}

#[tauri::command]
pub async fn apply_local(app: tauri::AppHandle, folder: String, confirmed: bool, ranked_safe: bool) -> Result<ApplyResponse, String> {
    off_main(app, move |state| apply_local_blocking(state, folder, confirmed, ranked_safe)).await
}

fn apply_local_blocking(state: &AppState, folder: String, confirmed: bool, ranked_safe: bool) -> Result<ApplyResponse, String> {
    let dir = PathBuf::from(&folder);
    let files = diag::logged("read local folder", packs::read_local(&dir))?;
    let name = dir.file_name().map(|n| format!("Local: {}", n.to_string_lossy())).unwrap_or_else(|| "Local folder".into());
    let src = Source { repo: None, id: "local".into(), name, revision: None, files, notes: vec![], ranked_safe, quiet: false, autoexec: packs::read_local_autoexec(&dir), settings_change: false };
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
pub async fn restore_backup(app: tauri::AppHandle, id: String) -> Result<String, String> {
    off_main(app, move |state| restore_backup_blocking(state, id)).await
}

fn restore_backup_blocking(state: &AppState, id: String) -> Result<String, String> {
    let result = restore_inner(&state, &id);
    if let Ok(msg) = &result {
        diag::info(&format!("restore {id}: {msg}"));
    }
    diag::logged(&format!("restore {id}"), result)
}

fn restore_inner(state: &AppState, id: &str) -> Result<String, String> {
    let _guard = state.apply_lock.lock().map_err(|_| "Another apply failed; restart the app.")?;
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

    let write = real_writer(state.dir.join("jobs"), false);
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
    let _ = fs::remove_dir_all(state.dir.join(APPLIED_DIR));
    let label = if meta.original { "your original files".to_string() } else { format!("the backup from {}", meta.id) };
    Ok(format!("Restored {label}."))
}

/// Pack folder links are percent-encoded ("OptiLock%20FPS%20Config"), so "%" is allowed too. No "?": links are
/// opened through explorer.exe, which treats a query string as a file path and opens a folder window instead.
fn is_github_url(url: &str) -> bool {
    url.starts_with("https://github.com/") && url.chars().all(|c| c.is_ascii_alphanumeric() || "/:._-~%".contains(c))
}

/// A GameBanana mod page, as linked from the config list: `https://gamebanana.com/mods/<id>`.
fn is_gamebanana_url(url: &str) -> bool {
    url.strip_prefix("https://gamebanana.com/mods/").is_some_and(|id| !id.is_empty() && id.len() <= 12 && id.bytes().all(|b| b.is_ascii_digit()))
}

/// Opens a GitHub page or a GameBanana mod page in the default browser. Other URLs are refused.
#[tauri::command]
pub fn open_github(url: String) -> Result<(), String> {
    if !is_github_url(&url) && !is_gamebanana_url(&url) {
        return Err("Only github.com and GameBanana mod links can be opened.".into());
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
        .map(|a| format!("{} from {} at {} (revision {})", a.pack_name, a.repo.as_deref().unwrap_or("local"), a.applied_at, a.revision.as_deref().unwrap_or("unknown")))
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
            ("Last failed launch", launch_failure_text(crate::launch_guard::load(&state.dir).failure.as_ref())),
            ("Performance", perf_status(&state)),
        ],
        log: diag::report_log(&state.dir),
        user: user.as_deref(),
    })
}

/// Whether the game is logging and how many sessions are stored, for bug reports about an empty Performance view.
fn perf_status(state: &AppState) -> String {
    let log = console_log_path(state).and_then(|p| fs::metadata(p).ok()).map(|m| m.len());
    let (condebug, reports) = {
        let tail = state.perf_log.lock().unwrap_or_else(|e| e.into_inner());
        (tail.condebug(), tail.summaries())
    };
    let stored = crate::perf::load(&state.dir).len();
    match log {
        Some(len) => format!(
            "console.log {} MB, -condebug {}, {reports} reports in this log, {stored} sessions stored",
            len / 1_000_000,
            if condebug { "yes" } else { "no" }
        ),
        None => format!("no console.log, {stored} sessions stored"),
    }
}

fn launch_failure_text(f: Option<&crate::launch_guard::Failure>) -> String {
    f.map_or_else(
        || "none".into(),
        |f| format!("{} closed after {}s at {} (backup {}){}", f.config, f.ran_seconds, f.at, f.backup_id, f.fatal.as_deref().map(|l| format!(": {l}")).unwrap_or_default()),
    )
}

/// The game closed right after starting following the app's last write (None when that didn't happen
/// or the player already chose Restore or Keep).
#[tauri::command]
pub fn launch_issue(state: State<AppState>) -> Option<crate::launch_guard::Failure> {
    crate::launch_guard::load(&state.dir).failure
}

#[tauri::command]
pub fn dismiss_launch_issue(state: State<AppState>) {
    crate::launch_guard::dismiss(&state.dir);
}

/// Called by the background watcher when a watched launch ended. Reads console.log for a fatal error
/// written during that run.
pub fn finish_launch_check(state: &AppState, ran: std::time::Duration, started: std::time::SystemTime) -> Option<crate::launch_guard::Failure> {
    // A long run's log can be hundreds of MB, and only a quick exit is reported.
    let fatal = console_log_path(state)
        .filter(|_| crate::launch_guard::is_quick_exit(ran))
        .filter(|p| fs::metadata(p).and_then(|m| m.modified()).is_ok_and(|m| m >= started))
        .and_then(|p| fs::read(p).ok())
        .and_then(|b| crate::launch_guard::fatal_line(&String::from_utf8_lossy(&b)));
    crate::launch_guard::finish(&state.dir, ran, fatal)
}

/// "installer", "scoop" or "portable"; the UI only offers in-app updates for "installer".
#[tauri::command]
pub fn install_kind() -> &'static str {
    std::env::current_exe().map(|exe| diag::install_kind(&exe)).unwrap_or("portable")
}

/// Puts the engine sections of the installed gameinfo.gi back to Valve's defaults and keeps the rest
/// (ConVars and so on). Same preview/confirm flow and backup as applying a pack; video.txt is untouched.
#[tauri::command]
pub async fn make_ranked_safe(app: tauri::AppHandle, confirmed: bool) -> Result<ApplyResponse, String> {
    off_main(app, move |state| make_ranked_safe_blocking(state, confirmed)).await
}

fn make_ranked_safe_blocking(state: &AppState, confirmed: bool) -> Result<ApplyResponse, String> {
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
        quiet: false,
        autoexec: None,
        settings_change: false,
    };
    logged_apply("make ranked-safe", apply_source(&state, src, confirmed))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TweakRow {
    /// "aspect", "fps_max" or "fps_max_ui".
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// Your value, if you set one.
    pub value: Option<String>,
    /// What the installed gameinfo.gi sets without your value.
    pub config_value: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixRow {
    pub id: &'static str,
    pub label: &'static str,
    /// Shown in game with the current settings.
    pub on: bool,
    /// The applied config shows it on its own.
    pub config_shows: bool,
    /// Can be switched off (plain on/off settings); distance and detail settings only switch to the fix.
    pub can_hide: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TweaksView {
    pub rows: Vec<TweakRow>,
    pub fixes: Vec<FixRow>,
}

const TWEAK_ROWS: [(&str, &str, &str, &str); 3] = [
    ("aspect", "r_aspectratio", "r_aspectratio", "Each +0.5 is about +10 FOV (2.15 = 90, 2.5 = 100, 3.0 = 110)."),
    ("fps_max", "fps_max", "fps_max", "0 = no limit"),
    ("fps_max_ui", "fps_max_ui", "fps_max_ui", "FPS limit in menus"),
];

/// The config's own value: from the applied config's files when known, otherwise the installed file.
fn config_source(state: &AppState) -> Option<String> {
    applied_source(&state.dir).map(|f| f.gameinfo).or_else(|| {
        let ov = resolve(&state.snapshot());
        ov.gameinfo_path.filter(|_| ov.gameinfo_found).and_then(|p| fs::read_to_string(p).ok())
    })
}

fn tweaks_view(state: &AppState) -> TweaksView {
    let o = state.snapshot().overrides;
    let src = config_source(state);
    let rows = TWEAK_ROWS
        .iter()
        .map(|(id, convar, label, description)| TweakRow {
            id,
            label,
            description,
            value: match *id {
                "aspect" => o.aspect.clone(),
                "fps_max" => o.fps_max.map(|v| v.to_string()),
                _ => o.fps_max_ui.map(|v| v.to_string()),
            },
            config_value: src.as_deref().and_then(|s| overrides::convar_value(s, convar)),
        })
        .collect();
    let fixes = overrides::FIXES
        .iter()
        .map(|f| {
            let config_shows = (f.shown)(src.as_deref().unwrap_or_default());
            let on = if o.fixes.iter().any(|x| x == f.id) {
                true
            } else if o.hidden.iter().any(|x| x == f.id) {
                false
            } else {
                config_shows
            };
            FixRow { id: f.id, label: f.label, on, config_shows, can_hide: f.hide.is_some() }
        })
        .collect();
    TweaksView { rows, fixes }
}

#[tauri::command]
pub fn get_tweaks(state: State<AppState>) -> TweaksView {
    tweaks_view(&state)
}

/// Writes the current settings into gameinfo.gi right away: re-applies the applied config with them,
/// or puts them into the installed file when no config was applied by the app.
fn apply_tweaks_now(state: &AppState) -> Result<String, String> {
    if state.snapshot().applied.is_some() && applied_source(&state.dir).is_some() {
        return reapply_saved_with(state, false, true);
    }
    // Applied by an older version (no saved copy): get the config again from its repo, so the settings
    // go on top of the config's own file and Reset can bring its values back.
    if let Some(files) = refetch_applied(state) {
        save_applied_source(&state.dir, &files, None).map_err(|e| format!("Couldn't save the config's files: {e}"))?;
        return reapply_saved_with(state, false, true);
    }
    let ov = resolve(&state.snapshot());
    let path = ov.gameinfo_path.filter(|_| ov.gameinfo_found).ok_or("gameinfo.gi wasn't found.")?;
    let installed = fs::read_to_string(&path).map_err(|e| format!("Cannot read gameinfo.gi: {e}"))?;
    // Keep the applied config's name and source (configs applied by older versions have no saved copy).
    let applied = state.snapshot().applied;
    let src = Source {
        repo: applied.as_ref().and_then(|a| a.repo.clone()),
        id: applied.as_ref().map_or("installed".into(), |a| a.pack_id.clone()),
        name: applied.as_ref().map_or("Your gameinfo.gi".into(), |a| a.pack_name.clone()),
        revision: applied.as_ref().and_then(|a| a.revision.clone()),
        files: PackFiles { gameinfo: installed, video: None },
        notes: vec![],
        ranked_safe: applied.as_ref().is_some_and(|a| a.ranked_safe),
        quiet: false,
        autoexec: None,
        settings_change: true,
    };
    match apply_source(state, src, true)? {
        ApplyResponse::Done { message, .. } => Ok(message),
        ApplyResponse::Preview { .. } => Err("Unexpected preview.".into()),
    }
}

/// Sets one value (None = use the config's value) and applies it.
#[tauri::command]
pub async fn set_tweak(app: tauri::AppHandle, id: String, value: Option<String>) -> Result<TweaksView, String> {
    off_main(app, move |state| set_tweak_blocking(state, id, value)).await
}

fn set_tweak_blocking(state: &AppState, id: String, value: Option<String>) -> Result<TweaksView, String> {
    let value = value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let fps = |v: &Option<String>| -> Result<Option<u32>, String> {
        v.as_deref().map(|t| t.parse::<u32>().map_err(|_| "FPS caps are whole numbers, 0 for no limit.".to_string())).transpose()
    };
    let current = state.snapshot().overrides;
    let next = match id.as_str() {
        "aspect" => Overrides { aspect: value.as_deref().map(overrides::parse_aspect).transpose()?, ..current },
        "fps_max" => Overrides { fps_max: fps(&value)?.map(|v| v.min(overrides::MAX_FPS)), ..current },
        "fps_max_ui" => Overrides { fps_max_ui: fps(&value)?.map(|v| v.min(overrides::MAX_FPS)), ..current },
        _ => return Err("Unknown setting.".into()),
    };
    let previous = state.snapshot().overrides;
    state.update(|s| Settings { overrides: next, ..s })?;
    if let Err(e) = diag::logged("apply setting", apply_tweaks_now(&state)) {
        state.update(|s| Settings { overrides: previous, ..s })?;
        return Err(e);
    }
    Ok(tweaks_view(&state))
}

/// Turns a fix on or off and applies it.
#[tauri::command]
pub async fn set_fix(app: tauri::AppHandle, id: String, on: bool) -> Result<TweaksView, String> {
    off_main(app, move |state| set_fix_blocking(state, id, on)).await
}

fn set_fix_blocking(state: &AppState, id: String, on: bool) -> Result<TweaksView, String> {
    let fix = overrides::FIXES.iter().find(|f| f.id == id).ok_or("Unknown fix.")?;
    let previous = state.snapshot().overrides;
    let config_shows = (fix.shown)(config_source(&state).as_deref().unwrap_or_default());
    let next = overrides::set_switch(&previous, fix, on, config_shows);
    state.update(|s| Settings { overrides: next, ..s })?;
    if let Err(e) = diag::logged("apply fix", apply_tweaks_now(&state)) {
        state.update(|s| Settings { overrides: previous, ..s })?;
        return Err(e);
    }
    Ok(tweaks_view(&state))
}

fn library_dir(state: &AppState) -> PathBuf {
    crate::library::dir(&state.dir)
}

#[tauri::command]
pub fn list_library(state: State<AppState>) -> Vec<crate::library::Entry> {
    crate::library::list(&library_dir(&state))
}

/// Saves the gameinfo.gi and video.txt that are installed now as a named config.
#[tauri::command]
pub fn save_current(state: State<AppState>, name: String, notes: String) -> Result<crate::library::Entry, String> {
    let s = state.snapshot();
    let ov = resolve(&s);
    let gi_path = ov.gameinfo_path.filter(|_| ov.gameinfo_found).ok_or("gameinfo.gi wasn't found, so there's nothing to save.")?;
    let gameinfo = fs::read_to_string(&gi_path).map_err(|e| format!("Cannot read gameinfo.gi: {e}"))?;
    let video = match &ov.video_path {
        Some(p) => read_optional(Path::new(p), "video.txt")?,
        None => None,
    };
    let source = match (&s.applied, &ov.status) {
        (Some(a), crate::apply::Status::Applied { .. } | crate::apply::Status::AppliedDmm { .. }) => {
            format!("Saved from {}{}", a.pack_name, a.repo.as_deref().map(|r| format!(" ({r})")).unwrap_or_default())
        }
        _ => "Saved from the installed files".to_string(),
    };
    let entry = crate::library::save(&library_dir(&state), &name, &notes, &PackFiles { gameinfo, video }, &source)?;
    diag::info(&format!("saved config {}", entry.id));
    Ok(entry)
}

/// Imports a .zip or folder into My configs. The name defaults to the file or folder name.
#[tauri::command]
pub fn import_config(state: State<AppState>, path: String) -> Result<crate::library::Entry, String> {
    let p = PathBuf::from(&path);
    let files = crate::library::read_import(&p)?;
    let name = p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Imported config".into());
    let source = format!("Imported from {}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    diag::logged("import config", crate::library::save(&library_dir(&state), &name, "", &files, &source))
}

#[tauri::command]
pub fn export_config(state: State<AppState>, id: String, path: String) -> Result<(), String> {
    diag::logged("export config", crate::library::export_zip(&library_dir(&state), &id, Path::new(&path)))
}

#[tauri::command]
pub fn delete_config(state: State<AppState>, id: String) -> Result<Vec<crate::library::Entry>, String> {
    crate::library::delete(&library_dir(&state), &id)?;
    Ok(crate::library::list(&library_dir(&state)))
}

#[tauri::command]
pub fn rename_config(state: State<AppState>, id: String, name: String, notes: String) -> Result<crate::library::Entry, String> {
    crate::library::rename(&library_dir(&state), &id, &name, &notes)
}

/// Applies a saved config, with the same preview, tweaks, ranked-safe option and backup as packs.
#[tauri::command]
pub async fn apply_library(app: tauri::AppHandle, id: String, confirmed: bool, ranked_safe: bool) -> Result<ApplyResponse, String> {
    off_main(app, move |state| apply_library_blocking(state, id, confirmed, ranked_safe)).await
}

fn apply_library_blocking(state: &AppState, id: String, confirmed: bool, ranked_safe: bool) -> Result<ApplyResponse, String> {
    let root = library_dir(&state);
    let entry = crate::library::find(&root, &id)?;
    let files = crate::library::read(&root, &id)?;
    let src = Source {
        repo: None,
        id: format!("{LIBRARY_PREFIX}{id}"),
        name: entry.name,
        revision: None,
        files,
        notes: vec![],
        ranked_safe,
        quiet: false,
        autoexec: None,
        settings_change: false,
    };
    logged_apply(&format!("apply saved config {id}"), apply_source(&state, src, confirmed))
}

/// Pack id prefix that marks an applied saved config.
pub const LIBRARY_PREFIX: &str = "lib:";

/// Short share code for a repo pack, with your settings and the ranked-safe choice.
#[tauri::command]
pub fn share_pack(state: State<AppState>, pack: PackInfo, ranked_safe: bool) -> Result<String, String> {
    crate::share::encode(&crate::share::Shared {
        repo: pack.repo.clone(),
        pack_hash: crate::share::pack_hash(&pack.id),
        safe: ranked_safe,
        settings: state.snapshot().overrides,
    })
}

/// Turns a share code (or a link containing one) into a saved config built from the pack's latest version.
#[tauri::command]
pub async fn import_code(state: State<'_, AppState>, text: String) -> Result<crate::library::Entry, String> {
    let shared = crate::share::decode(&text)?;
    let dir = state.dir.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<crate::library::Entry, String> {
        let cache = dir.join("cache");
        let group = packs::list_packs(&shared.repo, &cache);
        let pack = group
            .packs
            .iter()
            .find(|p| crate::share::pack_hash(&p.id) == shared.pack_hash)
            .ok_or_else(|| group.error.clone().unwrap_or_else(|| format!("That config isn't in {} anymore.", shared.repo)))?;
        let fetched = packs::fetch_pack(&shared.repo, pack, &cache)?;
        let stock = stock::load(&cache, None);
        let gi = if shared.safe { ranked::make_safe(&stock, &fetched.files.gameinfo)? } else { fetched.files.gameinfo };
        let entries = shared.settings.effective();
        let gameinfo = overrides::apply(&gi, &entries)?;
        let notes = if entries.is_empty() { String::new() } else { format!("Includes the sharer's settings ({} line(s)).", entries.len()) };
        let files = PackFiles { gameinfo, video: fetched.files.video };
        crate::library::save(&crate::library::dir(&dir), &format!("{} (shared)", pack.name), &notes, &files, &format!("Shared: {} from {}", pack.name, shared.repo))
    })
    .await
    .map_err(|e| e.to_string())?;
    diag::logged("import share code", result)
}

/// The applied pack's own files (before ranked-safe, overlay and tweaks), kept so the app can
/// re-apply after a game update without the network or the original repo.
const APPLIED_DIR: &str = "applied";

/// Writes the files to a fresh folder, then swaps it in, so a reader never sees a half-written set.
fn save_applied_source(app_dir: &Path, files: &PackFiles, autoexec: Option<&str>) -> std::io::Result<()> {
    let dir = app_dir.join(APPLIED_DIR);
    let tmp = app_dir.join(format!("{APPLIED_DIR}.new"));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp)?;
    fs::write(tmp.join("gameinfo.gi"), files.gameinfo.as_bytes())?;
    if let Some(v) = &files.video {
        fs::write(tmp.join("video.txt"), v.as_bytes())?;
    }
    if let Some(a) = autoexec {
        fs::write(tmp.join("autoexec.cfg"), a.as_bytes())?;
    }
    let _ = fs::remove_dir_all(&dir);
    fs::rename(&tmp, &dir)
}

pub fn applied_source(app_dir: &Path) -> Option<PackFiles> {
    let dir = app_dir.join(APPLIED_DIR);
    Some(PackFiles { gameinfo: fs::read_to_string(dir.join("gameinfo.gi")).ok()?, video: fs::read_to_string(dir.join("video.txt")).ok() })
}

/// After a game update replaced gameinfo.gi: re-applies the config's current version from its repo (authors
/// often fix configs for a new game version), or the saved copy when offline or not from a repo. Background:
/// no UAC prompt. Returns the message and whether the repo hasn't changed since the update.
pub fn reapply_after_game_update(state: &AppState) -> Result<(String, bool), String> {
    let rec = state.snapshot().applied.ok_or("Nothing has been applied yet.")?;
    let Some((fetched, autoexec, last_commit)) = latest_repo_pack(state, &rec) else {
        return reapply_saved_with(state, true, false).map(|m| (m, false));
    };
    let game_changed = resolve(&state.snapshot()).gameinfo_path.and_then(|p| fs::metadata(p).and_then(|m| m.modified()).ok());
    let behind = game_changed.is_some_and(|t| repo_behind_update(last_commit.as_deref(), t));
    let src = Source {
        repo: rec.repo,
        id: rec.pack_id,
        name: rec.pack_name.clone(),
        revision: fetched.revision,
        files: fetched.files,
        notes: vec![],
        ranked_safe: rec.ranked_safe,
        quiet: true,
        autoexec,
        settings_change: false,
    };
    match apply_source(state, src, true)? {
        ApplyResponse::Done { message, .. } => Ok((message, behind)),
        ApplyResponse::Preview { .. } => Err("Unexpected preview.".into()),
    }
}

/// The applied pack as it is in its repo now. None when it isn't from a repo, GitHub can't be reached, or
/// the pack is gone, so the caller falls back to the saved copy.
fn latest_repo_pack(state: &AppState, rec: &AppliedRecord) -> Option<(packs::FetchedPack, Option<String>, Option<String>)> {
    let repo = rec.repo.as_deref()?;
    let cache = state.dir.join("cache");
    let group = packs::list_packs(repo, &cache);
    if group.offline || group.error.is_some() {
        return None;
    }
    let pack = group.packs.iter().find(|p| p.id == rec.pack_id)?;
    let fetched = packs::fetch_pack(repo, pack, &cache).ok()?;
    let autoexec = packs::fetch_autoexec(pack, fetched.revision.as_deref(), &cache);
    Some((fetched, autoexec, pack.last_commit_date.clone()))
}

/// The pack's last commit is older than the game's new gameinfo.gi (or unknown dates: not behind).
fn repo_behind_update(last_commit: Option<&str>, game_changed: std::time::SystemTime) -> bool {
    let Some(commit) = last_commit.and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok()) else {
        return false;
    };
    commit.with_timezone(&chrono::Utc) < chrono::DateTime::<chrono::Utc>::from(game_changed)
}

fn reapply_saved_with(state: &AppState, quiet: bool, settings_change: bool) -> Result<String, String> {
    let rec = state.snapshot().applied.ok_or("Nothing has been applied yet.")?;
    let files = applied_source(&state.dir).ok_or("The applied config's files weren't saved (applied with an older version). Re-apply it once by hand.")?;
    let src = Source {
        repo: rec.repo,
        id: rec.pack_id,
        name: rec.pack_name,
        revision: rec.revision,
        files,
        notes: vec![],
        ranked_safe: rec.ranked_safe,
        quiet,
        autoexec: fs::read_to_string(state.dir.join(APPLIED_DIR).join("autoexec.cfg")).ok(),
        settings_change,
    };
    match apply_source(state, src, true)? {
        ApplyResponse::Done { message, .. } => Ok(message),
        ApplyResponse::Preview { .. } => Err("Unexpected preview.".into()),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationSettings {
    pub overlay: bool,
    pub auto_reapply: bool,
    pub start_with_windows: bool,
    pub exit_on_close: bool,
}

#[tauri::command]
pub fn get_automation(state: State<AppState>) -> AutomationSettings {
    let s = state.snapshot();
    AutomationSettings { overlay: s.overlay, auto_reapply: s.auto_reapply, start_with_windows: s.start_with_windows, exit_on_close: s.exit_on_close }
}

#[tauri::command]
pub fn set_automation(app: tauri::AppHandle, state: State<AppState>, overlay: bool, auto_reapply: bool, start_with_windows: bool, exit_on_close: bool) -> Result<AutomationSettings, String> {
    let s = state.update(|s| Settings { overlay, auto_reapply, start_with_windows, exit_on_close, ..s })?;
    {
        use tauri_plugin_autostart::ManagerExt;
        let launcher = app.autolaunch();
        let result = if start_with_windows { launcher.enable() } else { launcher.disable() };
        if let Err(e) = result {
            diag::error(&format!("autostart: {e}"));
        }
    }
    Ok(AutomationSettings { overlay: s.overlay, auto_reapply: s.auto_reapply, start_with_windows: s.start_with_windows, exit_on_close: s.exit_on_close })
}

/// What the last game update changed for the applied config. None unless a game update replaced it.
#[tauri::command]
pub fn update_report(state: State<AppState>) -> Option<crate::report::UpdateReport> {
    let s = state.snapshot();
    let ov = state.overview(&s);
    if ov.changed_by.as_deref() != Some("game") {
        return None;
    }
    let pack = applied_source(&state.dir)?;
    let installed = fs::read_to_string(ov.gameinfo_path?).ok()?;
    let from = s.applied.and_then(|a| a.game_build);
    Some(crate::report::build(&pack.gameinfo, &installed, from, ov.game_build))
}

/// Imports a config posted on GameBanana into My configs (download checked against GameBanana's MD5).
#[tauri::command]
pub async fn import_gamebanana(state: State<'_, AppState>, url: String) -> Result<crate::library::Entry, String> {
    let id = crate::gamebanana::parse_mod_id(&url)?;
    let root = crate::library::dir(&state.dir);
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<crate::library::Entry, String> {
        let info = crate::gamebanana::fetch_info(id)?;
        let (files, file, md5) = crate::gamebanana::fetch_config(&info)?;
        let notes = format!("By {} on GameBanana (gamebanana.com/mods/{}). File {file}, MD5 {md5}.", info.author, info.id);
        crate::library::save(&root, &info.name, &notes, &files, &format!("GameBanana: {} by {}", info.name, info.author))
    })
    .await
    .map_err(|e| e.to_string())?;
    diag::logged("import from GameBanana", result)
}

/// The config store list (online, cached or built in).
#[tauri::command]
pub async fn get_store(state: State<'_, AppState>) -> Result<crate::store::StoreView, String> {
    let cache = state.dir.join("cache");
    tauri::async_runtime::spawn_blocking(move || crate::store::load(&cache)).await.map_err(|e| e.to_string())
}


fn console_log_path(state: &AppState) -> Option<PathBuf> {
    let ov = resolve(&state.snapshot());
    ov.gameinfo_path.map(PathBuf::from).and_then(|p| p.parent().map(|d| d.join("console.log")))
}

/// Reads VProf reports the game added to console.log since the last call, labelled with the config that
/// is applied now. Returns the number of new reports, or None when there's no log.
pub fn collect_perf(state: &AppState) -> Option<usize> {
    let path = console_log_path(state)?;
    // Held until the reports are stored, so the watcher and the Performance view don't record at once.
    let mut tail = state.perf_log.lock().unwrap_or_else(|e| e.into_inner());
    // Put back if the reports can't be stored, so they're read again next time instead of lost.
    let before = tail.clone();
    let read = tail.read_new(&path);
    state.ranked_refused.store(tail.ranked_refused(), Ordering::Relaxed);
    let reports = match read {
        Ok(r) => r,
        Err(e) => {
            // A missing log starts over; other errors (the game holding the file) are retried from here.
            if e.kind() == std::io::ErrorKind::NotFound {
                *tail = Default::default();
                state.ranked_refused.store(false, Ordering::Relaxed);
            } else {
                diag::error(&format!("reading console.log: {e}"));
            }
            return None;
        }
    };
    if reports.is_empty() {
        return Some(0);
    }
    let s = state.snapshot();
    let ov = state.overview(&s);
    let managed = matches!(ov.status, crate::apply::Status::Applied { .. } | crate::apply::Status::AppliedDmm { .. });
    let (key, name, safe) = match (&s.applied, managed) {
        (Some(a), true) => {
            // Sessions with personal changes on top go under "<config> + tweaks #N".
            let fingerprint = crate::perf::fingerprint(&s.overrides.effective());
            let base = format!("{}#{}", a.repo.as_deref().unwrap_or("local"), a.pack_id);
            let (sets, key, name) = crate::perf::label_for(&s.perf_tweaks, &base, &a.pack_name, &fingerprint);
            if sets != s.perf_tweaks {
                if let Err(e) = state.update(|x| Settings { perf_tweaks: sets.clone(), ..x }) {
                    diag::error(&e);
                }
            }
            (key, name, a.ranked_safe)
        }
        _ => ("unmanaged".to_string(), "Not applied by this app".to_string(), false),
    };
    let now = chrono::Local::now().to_rfc3339();
    let label = |r: &crate::perf::Report| crate::perf::Session {
        report: r.clone(),
        config_key: key.clone(),
        config_name: name.clone(),
        ranked_safe: safe,
        game_build: ov.game_build.clone(),
        recorded_at: now.clone(),
    };
    match crate::perf::record(&state.dir, &reports, label) {
        Ok(n) => {
            if n > 0 {
                diag::info(&format!("recorded {n} performance report(s) for {name}"));
            }
            Some(n)
        }
        Err(e) => {
            diag::error(&e);
            *tail = before;
            None
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerfView {
    pub stats: Vec<crate::perf::ConfigStats>,
    /// Latest sessions long enough to count, newest first.
    pub recent: Vec<crate::perf::Numbered>,
    /// console.log exists and its last launch used -condebug.
    pub logging: bool,
    pub min_seconds: u64,
    pub min_sessions: usize,
}

/// Runs off the window's thread: the first read of a long match's console.log takes a moment.
#[tauri::command]
pub async fn get_perf(app: tauri::AppHandle) -> Result<PerfView, String> {
    tauri::async_runtime::spawn_blocking(move || perf_view(&app.state::<AppState>())).await.map_err(|e| e.to_string())
}

fn perf_view(state: &AppState) -> PerfView {
    let _ = collect_perf(state);
    let logging = state.perf_log.lock().unwrap_or_else(|e| e.into_inner()).condebug();
    let names = state.snapshot().perf_names;
    // Names the player gave an entry replace the recorded ones.
    let named = |key: &str, name: &str| names.get(key).cloned().unwrap_or_else(|| name.to_string());
    let sessions = crate::perf::load(&state.dir);
    let stats = crate::perf::summarize(&sessions)
        .into_iter()
        .map(|c| crate::perf::ConfigStats { config_name: named(&c.config_key, &c.config_name), ..c })
        .collect();
    let recent = crate::perf::recent(&sessions, 20)
        .into_iter()
        .map(|n| {
            let config_name = named(&n.session.config_key, &n.session.config_name);
            crate::perf::Numbered { session: crate::perf::Session { config_name, ..n.session }, ..n }
        })
        .collect();
    PerfView { stats, recent, logging, min_seconds: crate::perf::MIN_SECONDS, min_sessions: crate::perf::MIN_SESSIONS }
}

const MAX_PERF_NAME: usize = 40;

/// Names a Performance entry (e.g. "alz + tweaks #1"); an empty name goes back to the recorded one.
#[tauri::command]
pub async fn rename_perf(app: tauri::AppHandle, key: String, name: String) -> Result<PerfView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let clean: String = name.trim().chars().filter(|c| !c.is_control()).take(MAX_PERF_NAME).collect();
        state.update(|s| {
            let mut names = s.perf_names.clone();
            if clean.is_empty() {
                names.remove(&key);
            } else {
                names.insert(key.clone(), clean.clone());
            }
            Settings { perf_names: names, ..s }
        })?;
        Ok(perf_view(&state))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn clear_perf(state: State<AppState>) -> Result<(), String> {
    crate::perf::clear(&state.dir)?;
    // A fresh history numbers tweak sets from 1 again.
    state.update(|s| Settings { perf_tweaks: Vec::new(), perf_names: Default::default(), ..s }).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_encoded_pack_folder_links_only_on_github() {
        assert!(is_github_url(&crate::packs::folder_url("dacooderr/OptiLock", "OptiLock FPS Config (Recommended)")));
        assert!(is_github_url("https://github.com/alzzzik/alz-deadlock-cfg/tree/HEAD/stable"));
        assert!(!is_github_url("https://github.com.evil.example/x"));
        assert!(!is_github_url("https://github.com/a b"));
        assert!(!is_github_url("https://example.com/"));
        assert!(!is_github_url("https://github.com/a/b/issues/new?template=x"));
    }

    #[test]
    fn repo_is_behind_when_its_last_commit_predates_the_game_update() {
        let update = std::time::SystemTime::from(chrono::DateTime::parse_from_rfc3339("2026-10-20T21:00:00Z").unwrap());
        assert!(repo_behind_update(Some("2026-10-13T10:00:00Z"), update));
        assert!(!repo_behind_update(Some("2026-10-21T09:00:00Z"), update));
        assert!(!repo_behind_update(None, update));
        assert!(!repo_behind_update(Some("not a date"), update));
    }

    #[test]
    fn background_reapply_only_runs_for_the_config_still_applied() {
        let src = |id: &str| Source {
            repo: Some("o/r".into()),
            id: id.into(),
            name: "P".into(),
            revision: None,
            files: PackFiles { gameinfo: String::new(), video: None },
            notes: vec![],
            ranked_safe: false,
            quiet: true,
            autoexec: None,
            settings_change: false,
        };
        let rec = AppliedRecord {
            repo: Some("o/r".into()),
            pack_id: "stable".into(),
            pack_name: "P".into(),
            revision: None,
            applied_at: String::new(),
            gameinfo_sha256: String::new(),
            gameinfo_normalized_sha256: String::new(),
            video_sha256: None,
            ranked_safe: false,
            game_build: None,
        };
        assert!(still_applied(Some(&rec), &src("stable")));
        assert!(!still_applied(Some(&rec), &src("other")), "another config was applied meanwhile");
        assert!(!still_applied(None, &src("stable")), "the player restored their files meanwhile");
    }

    #[test]
    fn accepts_only_plain_gamebanana_mod_links() {
        assert!(is_gamebanana_url("https://gamebanana.com/mods/727681"));
        assert!(!is_gamebanana_url("https://gamebanana.com/mods/"));
        assert!(!is_gamebanana_url("https://gamebanana.com/mods/1/../../x"));
        assert!(!is_gamebanana_url("https://gamebanana.com.evil.example/mods/1"));
        assert!(!is_gamebanana_url("http://gamebanana.com/mods/1"));
    }
}

const SETTINGS_BACKUP_REASON: &str = "Before changing settings";
/// Setting changes within this many minutes of the last settings backup reuse it.
const SETTINGS_BACKUP_MINUTES: i64 = 30;

/// The applied repo config's files at the applied revision (from the cache when possible).
fn refetch_applied(state: &AppState) -> Option<PackFiles> {
    let s = state.snapshot();
    let rec = s.applied.as_ref()?;
    let repo = rec.repo.clone()?;
    let cache = state.dir.join("cache");
    let pack = packs::cached_all(&s.effective_repos(), &cache)
        .into_iter()
        .flat_map(|g| g.packs)
        .find(|p| p.repo.eq_ignore_ascii_case(&repo) && p.id == rec.pack_id)?;
    let pack = PackInfo { revision: rec.revision.clone().or(pack.revision.clone()), ..pack };
    packs::fetch_pack(&repo, &pack, &cache).ok().map(|f| f.files)
}
