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
            tweaks: tweaks.len(),
            overlay_added: added.map(|a| a.convars + a.sections.len()),
        };
        return Ok(ApplyResponse::Preview { preview });
    }

    let backups = state.backups_dir();
    let sources = Sources { gameinfo: &gi_path, video: video_path.as_deref() };
    let meta = backup::create(&backups, &sources, Some(&src.name), &format!("Before applying {}", src.name))?;
    let writes = apply::plan_writes(&plan, &gi_path, video_path.as_deref());
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
    state.update(|s| Settings { applied: Some(record), onboarded: true, ..s })?;
    save_applied_source(&state.dir, &src.files);

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
        quiet: false,
    };
    let label = format!("apply {}#{}", src.repo.as_deref().unwrap_or("?"), src.id);
    logged_apply(&label, apply_source(&state, src, confirmed))
}

#[tauri::command]
pub fn apply_local(state: State<AppState>, folder: String, confirmed: bool, ranked_safe: bool) -> Result<ApplyResponse, String> {
    let dir = PathBuf::from(&folder);
    let files = diag::logged("read local folder", packs::read_local(&dir))?;
    let name = dir.file_name().map(|n| format!("Local: {}", n.to_string_lossy())).unwrap_or_else(|| "Local folder".into());
    let src = Source { repo: None, id: "local".into(), name, revision: None, files, notes: vec![], ranked_safe, quiet: false };
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
        quiet: false,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToggleInfo {
    pub id: &'static str,
    pub group: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    /// What the toggle writes, e.g. `cl_ragdoll_limit "-1"` or `// r_farz`.
    pub lines: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AspectPreset {
    pub label: &'static str,
    pub value: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TweaksView {
    pub overrides: Overrides,
    pub toggles: Vec<ToggleInfo>,
    pub aspect_presets: Vec<AspectPreset>,
    pub max_fps: u32,
}

fn tweaks_view(o: Overrides) -> TweaksView {
    let line = |e: &overrides::Entry| match &e.value {
        Some(v) => format!("{} \"{v}\"", e.name),
        None => format!("// {}", e.name),
    };
    let toggles = overrides::TOGGLES
        .iter()
        .map(|t| ToggleInfo { id: t.id, group: t.group, label: t.label, hint: t.hint, lines: (t.entries)().iter().map(line).collect() })
        .collect();
    let aspect_presets = overrides::ASPECT_PRESETS.iter().map(|(label, value)| AspectPreset { label, value }).collect();
    TweaksView { overrides: o, toggles, aspect_presets, max_fps: overrides::MAX_FPS }
}

#[tauri::command]
pub fn get_tweaks(state: State<AppState>) -> TweaksView {
    tweaks_view(state.snapshot().overrides)
}

/// Saves tweaks. They take effect the next time a pack is applied.
#[tauri::command]
pub fn save_tweaks(state: State<AppState>, overrides: Overrides) -> Result<TweaksView, String> {
    overrides::validate(&overrides.custom)?;
    let aspect = overrides.aspect.as_deref().map(overrides::parse_aspect).transpose()?;
    let known: Vec<String> = overrides.fixes.into_iter().filter(|id| overrides::TOGGLES.iter().any(|t| t.id == id)).collect();
    let clean = Overrides { fixes: known, aspect, ..overrides };
    let s = state.update(|s| Settings { overrides: clean, ..s })?;
    Ok(tweaks_view(s.overrides))
}

/// Reads an overrides.gi file into custom lines (replacing the current custom lines).
#[tauri::command]
pub fn import_tweaks(state: State<AppState>, path: String) -> Result<TweaksView, String> {
    let text = fs::read_to_string(&path).map_err(|e| format!("Cannot read {path}: {e}"))?;
    let custom = overrides::parse_file(&text)?;
    let s = state.update(|s| Settings { overrides: Overrides { custom, ..s.overrides.clone() }, ..s })?;
    Ok(tweaks_view(s.overrides))
}

/// Writes all active tweaks (fixes, FOV and custom lines) as an overrides.gi file.
#[tauri::command]
pub fn export_tweaks(state: State<AppState>, path: String) -> Result<(), String> {
    let text = overrides::to_file(&state.snapshot().overrides.effective());
    crate::fsops::atomic_write(Path::new(&path), text.as_bytes()).map_err(|e| format!("Cannot write {path}: {e}"))
}

/// Parses custom tweak lines typed in the app (overrides.gi format).
#[tauri::command]
pub fn parse_tweak_lines(text: String) -> Result<Vec<overrides::Entry>, String> {
    overrides::parse_file(&text)
}

/// The same lines back as text, for editing.
#[tauri::command]
pub fn format_tweak_lines(entries: Vec<overrides::Entry>) -> String {
    overrides::to_file(&entries).lines().skip(1).map(|l| format!("{l}
")).collect()
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
    };
    logged_apply(&format!("apply saved config {id}"), apply_source(&state, src, confirmed))
}

/// Pack id prefix that marks an applied saved config.
pub const LIBRARY_PREFIX: &str = "lib:";

/// Share message for a repo pack, with your tweaks and the ranked-safe choice.
#[tauri::command]
pub fn share_pack(state: State<AppState>, pack: PackInfo, ranked_safe: bool) -> Result<String, String> {
    let shared = crate::share::Shared::Pack {
        repo: pack.repo.clone(),
        folder: pack.folder.clone(),
        name: pack.name.clone(),
        rev: pack.revision.clone(),
        video_file: pack.video_file.clone(),
        safe: ranked_safe,
        tweaks: state.snapshot().overrides.effective(),
    };
    Ok(crate::share::message(&pack.name, &crate::share::encode(&shared)?))
}

/// Share message for a saved config: its ConVars changes and video.txt settings.
#[tauri::command]
pub fn share_config(state: State<AppState>, id: String) -> Result<String, String> {
    let root = library_dir(&state);
    let entry = crate::library::find(&root, &id)?;
    let files = crate::library::read(&root, &id)?;
    let stock = stock::load(&state.dir.join("cache"), Some(&files.gameinfo));
    let video = files.video.as_deref().map(crate::video::read_values).transpose().map_err(|e| format!("video.txt {e}"))?.unwrap_or_default();
    let shared = crate::share::Shared::Config {
        name: entry.name.clone(),
        notes: entry.notes.clone(),
        convars: crate::share::convar_delta(&stock, &files.gameinfo)?,
        video,
    };
    Ok(crate::share::message(&entry.name, &crate::share::encode(&shared)?))
}

/// Turns a share code (or a message or link containing one) into a saved config.
#[tauri::command]
pub async fn import_code(state: State<'_, AppState>, text: String) -> Result<crate::library::Entry, String> {
    let shared = crate::share::decode(&text)?;
    let dir = state.dir.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<crate::library::Entry, String> {
        let cache = dir.join("cache");
        let root = crate::library::dir(&dir);
        match shared {
            crate::share::Shared::Pack { repo, folder, name, rev, video_file, safe, tweaks } => {
                let pack = PackInfo {
                    repo: repo.clone(),
                    id: packs::slug(&folder),
                    name: name.clone(),
                    description: String::new(),
                    folder,
                    last_commit_date: None,
                    revision: rev,
                    commit_message: None,
                    html_url: String::new(),
                    video_file,
                };
                let fetched = packs::fetch_pack(&repo, &pack, &cache)?;
                let stock = stock::load(&cache, None);
                let gi = if safe { ranked::make_safe(&stock, &fetched.files.gameinfo)? } else { fetched.files.gameinfo };
                let gameinfo = overrides::apply(&gi, &tweaks)?;
                let notes = if tweaks.is_empty() { String::new() } else { format!("Includes {} tweak(s) from the person who shared it.", tweaks.len()) };
                let files = PackFiles { gameinfo, video: fetched.files.video };
                crate::library::save(&root, &format!("{name} (shared)"), &notes, &files, &format!("Shared: {name} from {repo}"))
            }
            crate::share::Shared::Config { name, notes, convars, video } => {
                let stock = stock::load(&cache, None);
                let files = crate::share::config_files(&stock, &convars, &video)?;
                crate::library::save(&root, &format!("{name} (shared)"), &notes, &files, "Shared config")
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    diag::logged("import share code", result)
}

/// The applied pack's own files (before ranked-safe, overlay and tweaks), kept so the app can
/// re-apply after a game update without the network or the original repo.
const APPLIED_DIR: &str = "applied";

fn save_applied_source(app_dir: &Path, files: &PackFiles) {
    let dir = app_dir.join(APPLIED_DIR);
    let _ = fs::remove_dir_all(&dir);
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = crate::fsops::atomic_write(&dir.join("gameinfo.gi"), files.gameinfo.as_bytes());
    if let Some(v) = &files.video {
        let _ = crate::fsops::atomic_write(&dir.join("video.txt"), v.as_bytes());
    }
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
