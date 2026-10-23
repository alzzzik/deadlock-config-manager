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
use std::sync::Mutex;
use tauri::State;

pub struct AppState {
    pub dir: PathBuf,
    pub settings: Mutex<Settings>,
    /// Held for a whole apply or restore so the UI and the background re-apply never write at once.
    pub apply_lock: Mutex<()>,
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
    let meta = backup::create(&backups, &sources, Some(&src.name), &format!("Before applying {}", src.name))?;
    let mut writes = apply::plan_writes(&plan, &gi_path, video_path.as_deref());
    if let Some((path, old, new)) = &autoexec_out {
        // The old autoexec.cfg is kept next to the backups; Restore… doesn't touch autoexec.cfg.
        if let Some(old) = old {
            let dir = backups.join("autoexec");
            let _ = fs::create_dir_all(&dir).and_then(|_| fs::write(dir.join(format!("{}.cfg", meta.id)), old));
        }
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        // Written last, so a failed gameinfo.gi/video.txt write never leaves it changed.
        writes.push((path.clone(), new.as_bytes().to_vec()));
    }
    let write = real_writer(state.dir.join("jobs"), src.quiet);
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
        game_build: crate::paths::game_build(&gi_path),
    };
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
    };
    let label = format!("apply {}#{}", src.repo.as_deref().unwrap_or("?"), src.id);
    logged_apply(&label, apply_source(&state, src, confirmed))
}

#[tauri::command]
pub fn apply_local(state: State<AppState>, folder: String, confirmed: bool, ranked_safe: bool) -> Result<ApplyResponse, String> {
    let dir = PathBuf::from(&folder);
    let files = diag::logged("read local folder", packs::read_local(&dir))?;
    let name = dir.file_name().map(|n| format!("Local: {}", n.to_string_lossy())).unwrap_or_else(|| "Local folder".into());
    let src = Source { repo: None, id: "local".into(), name, revision: None, files, notes: vec![], ranked_safe, quiet: false, autoexec: packs::read_local_autoexec(&dir) };
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

/// Pack folder links are percent-encoded ("OptiLock%20FPS%20Config"), so "%" is allowed too.
fn is_github_url(url: &str) -> bool {
    url.starts_with("https://github.com/") && url.chars().all(|c| c.is_ascii_alphanumeric() || "/:._-~%".contains(c))
}

/// Opens a GitHub page in the default browser. Only github.com URLs are allowed.
#[tauri::command]
pub fn open_github(url: String) -> Result<(), String> {
    if !is_github_url(&url) {
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
        quiet: false,
        autoexec: None,
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
    pub on: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TweaksView {
    pub rows: Vec<TweakRow>,
    pub fixes: Vec<FixRow>,
}

const TWEAK_ROWS: [(&str, &str, &str, &str); 3] = [
    ("aspect", "r_aspectratio", "r_aspectratio", "Higher shows more around you. 2.15 ≈ 90 FOV, 2.5 ≈ 100 FOV."),
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
    let fixes = overrides::FIXES.iter().map(|f| FixRow { id: f.id, label: f.label, on: o.fixes.iter().any(|x| x == f.id) }).collect();
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
        return reapply_saved(state, false);
    }
    let ov = resolve(&state.snapshot());
    let path = ov.gameinfo_path.filter(|_| ov.gameinfo_found).ok_or("gameinfo.gi wasn't found.")?;
    let installed = fs::read_to_string(&path).map_err(|e| format!("Cannot read gameinfo.gi: {e}"))?;
    let src = Source {
        repo: None,
        id: "installed".into(),
        name: "Your gameinfo.gi".into(),
        revision: None,
        files: PackFiles { gameinfo: installed, video: None },
        notes: vec![],
        ranked_safe: false,
        quiet: false,
        autoexec: None,
    };
    match apply_source(state, src, true)? {
        ApplyResponse::Done { message, .. } => Ok(message),
        ApplyResponse::Preview { .. } => Err("Unexpected preview.".into()),
    }
}

/// Sets one value (None = use the config's value) and applies it.
#[tauri::command]
pub fn set_tweak(state: State<AppState>, id: String, value: Option<String>) -> Result<TweaksView, String> {
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
pub fn set_fix(state: State<AppState>, id: String, on: bool) -> Result<TweaksView, String> {
    if !overrides::FIXES.iter().any(|f| f.id == id) {
        return Err("Unknown fix.".into());
    }
    let previous = state.snapshot().overrides;
    let fixes: Vec<String> = previous.fixes.iter().filter(|x| **x != id).cloned().chain(on.then(|| id.clone())).collect();
    state.update(|s| Settings { overrides: Overrides { fixes, ..s.overrides.clone() }, ..s })?;
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
pub fn apply_library(state: State<AppState>, id: String, confirmed: bool, ranked_safe: bool) -> Result<ApplyResponse, String> {
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

/// Re-applies the last applied config from its saved files, with the same ranked-safe choice and the
/// current tweaks and overlay setting. `quiet` = background: no UAC prompt.
pub fn reapply_saved(state: &AppState, quiet: bool) -> Result<String, String> {
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
}

#[tauri::command]
pub fn get_automation(state: State<AppState>) -> AutomationSettings {
    let s = state.snapshot();
    AutomationSettings { overlay: s.overlay, auto_reapply: s.auto_reapply, start_with_windows: s.start_with_windows }
}

#[tauri::command]
pub fn set_automation(app: tauri::AppHandle, state: State<AppState>, overlay: bool, auto_reapply: bool, start_with_windows: bool) -> Result<AutomationSettings, String> {
    let start = auto_reapply && start_with_windows;
    let s = state.update(|s| Settings { overlay, auto_reapply, start_with_windows: start, ..s })?;
    {
        use tauri_plugin_autostart::ManagerExt;
        let launcher = app.autolaunch();
        let result = if start { launcher.enable() } else { launcher.disable() };
        if let Err(e) = result {
            diag::error(&format!("autostart: {e}"));
        }
    }
    Ok(AutomationSettings { overlay: s.overlay, auto_reapply: s.auto_reapply, start_with_windows: s.start_with_windows })
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchView {
    pub options: String,
    pub steam_running: bool,
    /// Why the options can't be read or changed (no account found, file missing, …).
    pub problem: Option<String>,
}

fn localconfig(state: &AppState) -> Result<PathBuf, String> {
    let ov = resolve(&state.snapshot());
    let root = ov.steam_root.ok_or("Steam wasn't found.")?;
    let id = ov.account_id.ok_or("No Steam account was found.")?;
    Ok(crate::launch::localconfig_path(Path::new(&root), &id))
}

#[tauri::command]
pub fn get_launch_options(state: State<AppState>) -> LaunchView {
    let steam_running = crate::launch::steam_running();
    let read = localconfig(&state)
        .and_then(|p| fs::read_to_string(&p).map_err(|_| "Steam hasn't saved settings for this account yet.".to_string()))
        .and_then(|t| crate::launch::read(&t));
    match read {
        Ok(options) => LaunchView { options, steam_running, problem: None },
        Err(e) => LaunchView { options: String::new(), steam_running, problem: Some(e) },
    }
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


#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubStatus {
    /// This build has an OAuth client ID, so sign-in can be offered.
    pub available: bool,
    pub login: Option<String>,
}

#[tauri::command]
pub async fn github_status() -> Result<GithubStatus, String> {
    if !crate::auth::available() {
        return Ok(GithubStatus { available: false, login: None });
    }
    let login = tauri::async_runtime::spawn_blocking(crate::auth::current_login).await.map_err(|e| e.to_string())?;
    Ok(GithubStatus { available: true, login })
}

/// Starts the device flow; the UI shows the code and opens github.com/login/device.
#[tauri::command]
pub async fn github_sign_in() -> Result<crate::auth::DeviceCode, String> {
    tauri::async_runtime::spawn_blocking(crate::auth::start).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn github_poll(device_code: String, interval: u64) -> Result<crate::auth::PollResult, String> {
    let result = tauri::async_runtime::spawn_blocking(move || crate::auth::poll(&device_code, interval)).await.map_err(|e| e.to_string())?;
    if let crate::auth::PollResult::Done { login } = &result {
        diag::info(&format!("signed in to GitHub as {login}"));
    }
    Ok(result)
}

#[tauri::command]
pub fn github_sign_out() -> Result<(), String> {
    crate::auth::sign_out()
}

fn console_log_path(state: &AppState) -> Option<PathBuf> {
    let ov = resolve(&state.snapshot());
    ov.gameinfo_path.map(PathBuf::from).and_then(|p| p.parent().map(|d| d.join("console.log")))
}

/// Reads new VProf reports from console.log when it changed since `seen`, labelled with the config that
/// is applied now. Returns the number of new reports, or None when there's no log.
pub fn collect_perf(state: &AppState, seen: &mut Option<std::time::SystemTime>) -> Option<usize> {
    let path = console_log_path(state)?;
    let modified = fs::metadata(&path).and_then(|m| m.modified()).ok()?;
    if *seen == Some(modified) {
        return Some(0);
    }
    *seen = Some(modified);
    let text = String::from_utf8_lossy(&fs::read(&path).ok()?).into_owned();
    let reports = crate::perf::parse(&text);
    let s = state.snapshot();
    let ov = state.overview(&s);
    let managed = matches!(ov.status, crate::apply::Status::Applied { .. } | crate::apply::Status::AppliedDmm { .. });
    let (key, name, safe) = match (&s.applied, managed) {
        (Some(a), true) => (format!("{}#{}", a.repo.as_deref().unwrap_or("local"), a.pack_id), a.pack_name.clone(), a.ranked_safe),
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
            None
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerfView {
    pub stats: Vec<crate::perf::ConfigStats>,
    /// Latest sessions long enough to count, newest first.
    pub recent: Vec<crate::perf::Session>,
    /// console.log exists and its last launch used -condebug.
    pub logging: bool,
    pub min_seconds: u64,
    pub min_sessions: usize,
}

#[tauri::command]
pub fn get_perf(state: State<AppState>) -> PerfView {
    let mut seen = None;
    let _ = collect_perf(&state, &mut seen);
    let logging = console_log_path(&state)
        .and_then(|p| fs::read(p).ok())
        .is_some_and(|b| crate::perf::condebug_on(&String::from_utf8_lossy(&b)));
    let sessions = crate::perf::load(&state.dir);
    let recent = sessions.iter().rev().filter(|s| s.report.seconds >= crate::perf::MIN_SECONDS).take(20).cloned().collect();
    PerfView { stats: crate::perf::summarize(&sessions), recent, logging, min_seconds: crate::perf::MIN_SECONDS, min_sessions: crate::perf::MIN_SESSIONS }
}

#[tauri::command]
pub fn clear_perf(state: State<AppState>) -> Result<(), String> {
    crate::perf::clear(&state.dir)
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
    }
}
