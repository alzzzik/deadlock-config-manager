//! Performance export: one JSON file with every recorded session and the setup it was played with, made to be
//! handed to an assistant (or a spreadsheet) for analysis.
//!
//! Each session keeps a [`Context`] captured when it was recorded. The full contents of gameinfo.gi (as its
//! differences from Valve's file) and video.txt are kept once per hash in `perf-files.json`, so 2000 sessions
//! don't hold 2000 copies of the same files.

use crate::perf::{Context, Session};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Kept file contents, by SHA-256 of the file.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Files {
    /// gameinfo.gi as its differences from Valve's file: "Section › key: stock → installed".
    pub gameinfo: BTreeMap<String, Vec<String>>,
    /// video.txt as key → value.
    pub video: BTreeMap<String, BTreeMap<String, String>>,
}

fn files_path(app_dir: &Path) -> PathBuf {
    app_dir.join("perf-files.json")
}

pub fn load_files(app_dir: &Path) -> Files {
    fs::read(files_path(app_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// gameinfo.gi's differences from Valve's file, the mod manager's SearchPaths left out.
pub fn gameinfo_diff(installed: &str, stock: &str) -> Vec<String> {
    let pairs = |src: &str| crate::kv::parse(&crate::gameinfo::strip_dmm_region(src)).map(|n| crate::diff::flatten(&n)).unwrap_or_default();
    let (changes, _) = crate::diff::diff_pairs(&pairs(stock), &pairs(installed));
    changes
        .into_iter()
        .map(|c| {
            let side = |v: Option<String>| v.unwrap_or_else(|| "(not set)".into());
            format!("{}: {} → {}", c.key, side(c.before), side(c.after))
        })
        .collect()
}

fn video_values(src: &str) -> BTreeMap<String, String> {
    crate::video::read_values(src).map(|v| v.into_iter().collect()).unwrap_or_default()
}

/// Adds the files to perf-files.json if their hashes aren't there yet.
fn remember(app_dir: &Path, gameinfo: Option<(&str, &str, &str)>, video: Option<(&str, &str)>) {
    let mut files = load_files(app_dir);
    let mut changed = false;
    if let Some((sha, installed, stock)) = gameinfo {
        if !files.gameinfo.contains_key(sha) {
            files.gameinfo.insert(sha.to_string(), gameinfo_diff(installed, stock));
            changed = true;
        }
    }
    if let Some((sha, src)) = video {
        if !files.video.contains_key(sha) {
            files.video.insert(sha.to_string(), video_values(src));
            changed = true;
        }
    }
    if changed {
        if let Ok(json) = serde_json::to_vec_pretty(&files) {
            if let Err(e) = crate::fsops::atomic_write(&files_path(app_dir), &json) {
                crate::diag::error(&format!("saving perf-files.json: {e}"));
            }
        }
    }
}

fn status_name(s: &crate::apply::Status) -> &'static str {
    use crate::apply::Status::*;
    match s {
        NotApplied => "not applied",
        Missing => "missing",
        Applied { .. } => "applied",
        AppliedDmm { .. } => "applied with DMM",
        Changed { .. } => "changed",
    }
}

/// The setup right now: applied config, changes on top, the app's switches, installed files and enabled mods.
pub fn capture(app_dir: &Path, s: &crate::settings::Settings, ov: &crate::overview::Overview) -> Context {
    let gi_path = ov.gameinfo_path.as_deref().filter(|_| ov.gameinfo_found).map(Path::new);
    let installed = gi_path.and_then(|p| fs::read_to_string(p).ok());
    let video = ov.video_path.as_deref().and_then(|p| fs::read_to_string(p).ok());
    let gi_sha = installed.as_deref().map(|t| crate::gameinfo::sha256_hex(t.as_bytes()));
    let video_sha = video.as_deref().map(|t| crate::gameinfo::sha256_hex(t.as_bytes()));
    let stock = crate::stock::load(&app_dir.join("cache"), installed.as_deref());
    remember(
        app_dir,
        gi_sha.as_deref().zip(installed.as_deref()).map(|(h, t)| (h, t, stock.as_str())),
        video_sha.as_deref().zip(video.as_deref()),
    );
    Context {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        status: status_name(&ov.status).to_string(),
        applied: s.applied.clone(),
        tweaks: ov.tweak_fingerprint.split(';').filter(|t| !t.is_empty()).map(str::to_string).collect(),
        overrides: s.overrides.clone(),
        gameinfo_sha256: gi_sha,
        video_sha256: video_sha,
        mods: gi_path.map(crate::mods::enabled).unwrap_or_default(),
    }
}

fn round(x: f64, places: i32) -> f64 {
    let m = 10f64.powi(places);
    (x * m).round() / m
}

/// "11/25 17:35:52" in the year of `recorded_at` (the log has no year). A report from late December picked
/// up in January belongs to the year before.
fn played_at(time: &str, recorded_at: &str) -> Option<chrono::NaiveDateTime> {
    let rec = chrono::DateTime::parse_from_rfc3339(recorded_at).ok()?.naive_local();
    let with_year = |y: i32| chrono::NaiveDateTime::parse_from_str(&format!("{y}/{time}"), "%Y/%m/%d %H:%M:%S").ok();
    use chrono::Datelike;
    let t = with_year(rec.year())?;
    if t > rec + chrono::Duration::days(1) {
        with_year(rec.year() - 1)
    } else {
        Some(t)
    }
}

fn session_json(n: Option<usize>, s: &Session) -> Value {
    let r = &s.report;
    let played = played_at(&r.time, &s.recorded_at);
    let lag = played.zip(chrono::DateTime::parse_from_rfc3339(&s.recorded_at).ok()).map(|(p, rec)| (rec.naive_local() - p).num_minutes());
    let total = r.rows.iter().find(|row| row.name == "FrameTotal");
    let breakdown: Vec<Value> = r
        .rows
        .iter()
        .filter(|row| row.name != "FrameTotal")
        .map(|row| {
            json!({
                "name": row.name, "avg": row.avg, "p99": row.p99,
                "activeFrames": row.active_frames, "activeAvg": row.active_avg, "activeP99": row.active_p99,
                "worstPerSecondP50": row.max_p50, "worstPerSecondP95": row.max_p95,
            })
        })
        .collect();
    let setup = s.context.as_ref().map(|c| {
        json!({
            "status": c.status,
            "tweaks": c.tweaks,
            "overrides": c.overrides,
            "gameinfo": c.gameinfo_sha256,
            "video": c.video_sha256,
            "mods": c.mods.iter().map(|m| json!({"name": m.name, "id": m.id, "variant": m.variant, "files": m.files})).collect::<Vec<_>>(),
            "appVersion": c.app_version,
        })
    });
    json!({
        "number": n,
        "playedAt": played.map(|p| p.format("%Y-%m-%d %H:%M:%S").to_string()),
        "recordedAt": s.recorded_at,
        "setupCapturedMinutesAfter": lag,
        "map": r.map,
        "online": r.online,
        "minutes": round(r.seconds as f64 / 60.0, 1),
        "frames": r.frames,
        "excludedFrames": r.excluded_frames,
        "fps": {
            "avg": r.avg_fps,
            "low1pct": r.p1_fps,
            "low1pctOverAvg": if r.avg_fps > 0.0 { Some(round(r.p1_fps / r.avg_fps, 3)) } else { None },
        },
        "frametimeMs": {
            "avg": r.frame_ms,
            "p99": r.frame_ms_p99,
            "worstPerSecondP50": total.map(|t| t.max_p50),
            "worstPerSecondP95": total.map(|t| t.max_p95),
        },
        "breakdownMs": breakdown,
        "config": { "key": s.config_key, "name": s.config_name, "rankedSafe": s.ranked_safe },
        "gameBuild": s.game_build,
        "launchOptions": if r.command_line.is_empty() { None } else { Some(&r.command_line) },
        "setup": setup,
    })
}

fn spread(v: &[f64]) -> Value {
    if v.is_empty() {
        return Value::Null;
    }
    let n = v.len() as f64;
    let mean = v.iter().sum::<f64>() / n;
    let sd = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt();
    let min = v.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    json!({ "mean": round(mean, 1), "sd": round(sd, 1), "min": min, "max": max })
}

const ABOUT: &str = "Deadlock Config Manager performance export. Each session is one map Deadlock played, measured by \
the game's own VProfLite report (written to console.log with -condebug when the map unloads). fps.low1pct is VProf's \
P1 FPS (1% low). frametimeMs.p99 is the 99th percentile frame time. worstPerSecondP50/P95 take the slowest frame of \
every second and give their median and 95th percentile: a stutter measure. breakdownMs splits the frame into the \
engine's parts (Client Rendering, PanoramaUI = UI, Prediction, Sleeping = waiting on a frame cap, ...): avg/p99 over \
all frames, active* over frames where that part ran. setup is what was installed when the app recorded the session; \
if setupCapturedMinutesAfter is large the app wasn't running when the match ended and setup may be newer than the \
match. gameinfo and video hold the files by hash: gameinfo as differences from Valve's file, video as video.txt values. \
Not available from VProf: per-frame times, so no 0.1% low or histogram.";

/// The export as JSON. `sessions` are all stored sessions (oldest first), with their current config names.
pub fn build(app_dir: &Path, current: &Context, sessions: &[Session]) -> Value {
    let counted: Vec<&Session> = sessions.iter().filter(|s| s.report.seconds >= crate::perf::MIN_SECONDS && s.report.avg_fps > 0.0).collect();
    let numbered: Vec<Value> = counted.iter().enumerate().rev().map(|(i, s)| session_json(Some(i + 1), s)).collect();
    let mut configs: Vec<Value> = crate::perf::summarize(sessions)
        .into_iter()
        .map(|c| {
            let list: Vec<&&Session> = counted.iter().filter(|s| s.config_key == c.config_key).collect();
            let low: Vec<f64> = list.iter().map(|s| s.report.p1_fps).collect();
            let avg: Vec<f64> = list.iter().map(|s| s.report.avg_fps).collect();
            json!({
                "key": c.config_key, "name": list.last().map(|s| s.config_name.clone()).unwrap_or(c.config_name),
                "sessions": c.sessions, "minutes": c.minutes, "lastPlayed": c.last_played,
                "median": { "avgFps": c.median_avg_fps, "low1pctFps": c.median_p1_fps, "frametimeP99Ms": c.median_frame_ms_p99 },
                "low1pctFps": spread(&low), "avgFps": spread(&avg),
            })
        })
        .collect();
    configs.sort_by_key(|c| std::cmp::Reverse(c["sessions"].as_u64().unwrap_or(0)));
    // Only the files sessions in this export refer to.
    let files = load_files(app_dir);
    let used = |pick: fn(&Context) -> Option<&String>| -> Vec<String> {
        sessions.iter().filter_map(|s| s.context.as_ref()).chain(std::iter::once(current)).filter_map(|c| pick(c).cloned()).collect()
    };
    let gi_used = used(|c| c.gameinfo_sha256.as_ref());
    let video_used = used(|c| c.video_sha256.as_ref());
    let gameinfo: BTreeMap<&String, &Vec<String>> = files.gameinfo.iter().filter(|(k, _)| gi_used.contains(k)).collect();
    let video: BTreeMap<&String, &BTreeMap<String, String>> = files.video.iter().filter(|(k, _)| video_used.contains(k)).collect();
    json!({
        "format": "dcm-perf-export/1",
        "about": ABOUT,
        "exportedAt": chrono::Local::now().to_rfc3339(),
        "appVersion": env!("CARGO_PKG_VERSION"),
        "current": current,
        "counts": { "sessions": counted.len(), "tooShort": sessions.len() - counted.len(), "minMinutes": crate::perf::MIN_SECONDS / 60 },
        "configs": configs,
        "sessions": numbered,
        "gameinfo": gameinfo,
        "video": video,
    })
}

/// Where the export goes: the Downloads folder, or the app's folder if there's none.
pub fn target(app_dir: &Path) -> PathBuf {
    let name = format!("deadlock-performance-{}.json", chrono::Local::now().format("%Y-%m-%d-%H%M"));
    let downloads = std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Downloads")).filter(|d| d.is_dir());
    downloads.unwrap_or_else(|| app_dir.to_path_buf()).join(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perf::{ProfRow, Report};

    fn session(time: &str, recorded: &str, p1: f64, context: Option<Context>) -> Session {
        Session {
            report: Report {
                time: time.into(),
                map: "dl_midtown".into(),
                online: true,
                frames: 100_000,
                seconds: 1500,
                avg_fps: 200.0,
                p1_fps: p1,
                frame_ms: 5.0,
                frame_ms_p99: 9.0,
                rows: vec![
                    ProfRow { name: "FrameTotal".into(), max_p50: 12.0, max_p95: 30.0, ..Default::default() },
                    ProfRow { name: "Client Rendering".into(), avg: 3.0, p99: 7.0, ..Default::default() },
                ],
                ..Default::default()
            },
            config_key: "local#apex".into(),
            config_name: "apex".into(),
            ranked_safe: false,
            game_build: Some("26907316".into()),
            recorded_at: recorded.into(),
            context,
        }
    }

    #[test]
    fn exports_sessions_newest_first_with_setup_and_files() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Context { gameinfo_sha256: Some("aa".into()), video_sha256: Some("vv".into()), tweaks: vec!["r_farz=4000".into()], ..Default::default() };
        let files = Files {
            gameinfo: [("aa".to_string(), vec!["ConVars › r_farz: (not set) → 4000".to_string()]), ("zz".to_string(), vec![])].into(),
            video: [("vv".to_string(), [("setting.mat_viewportscale".to_string(), "0.58".to_string())].into())].into(),
        };
        fs::write(files_path(dir.path()), serde_json::to_vec(&files).unwrap()).unwrap();
        let mut short = session("11/25 10:00:00", "2026-11-25T10:00:10+01:00", 100.0, None);
        short.report.seconds = 60;
        let sessions = vec![
            session("12/31 23:50:00", "2027-01-01T00:20:00+01:00", 100.0, None),
            short,
            session("01/02 18:00:00", "2027-01-02T18:01:00+01:00", 120.0, Some(ctx.clone())),
        ];
        let out = build(dir.path(), &ctx, &sessions);
        assert_eq!(out["counts"]["sessions"], 2);
        assert_eq!(out["counts"]["tooShort"], 1);
        let s = &out["sessions"];
        assert_eq!(s[0]["number"], 2);
        assert_eq!(s[0]["fps"]["low1pct"], 120.0);
        assert_eq!(s[0]["fps"]["low1pctOverAvg"], 0.6);
        assert_eq!(s[0]["frametimeMs"]["worstPerSecondP95"], 30.0);
        assert_eq!(s[0]["breakdownMs"][0]["name"], "Client Rendering");
        assert_eq!(s[0]["setup"]["tweaks"][0], "r_farz=4000");
        assert_eq!(s[0]["setupCapturedMinutesAfter"], 1);
        // New Year's Eve, picked up after midnight.
        assert_eq!(s[1]["playedAt"], "2026-12-31 23:50:00");
        assert_eq!(s[1]["setupCapturedMinutesAfter"], 30);
        assert!(s[1]["setup"].is_null());
        assert_eq!(out["configs"][0]["sessions"], 2);
        assert_eq!(out["configs"][0]["low1pctFps"]["min"], 100.0);
        // Only files a session refers to.
        assert!(out["gameinfo"].get("aa").is_some() && out["gameinfo"].get("zz").is_none());
        assert_eq!(out["video"]["vv"]["setting.mat_viewportscale"], "0.58");
    }

    #[test]
    fn gameinfo_diff_lists_changes_from_valve() {
        let stock = "\"GameInfo\"\n{\n\tConVars\n\t{\n\t\tr_farz \"-1\"\n\t\tfps_max \"400\"\n\t}\n}\n";
        let installed = "\"GameInfo\"\n{\n\tConVars\n\t{\n\t\tr_farz \"4000\"\n\t\tfps_max \"400\"\n\t\tcl_new \"1\"\n\t}\n}\n";
        assert_eq!(gameinfo_diff(installed, stock), vec!["ConVars › r_farz: -1 → 4000".to_string(), "ConVars › cl_new: (not set) → 1".to_string()]);
    }

    #[test]
    fn remembers_each_file_once() {
        let dir = tempfile::tempdir().unwrap();
        let v1 = "\"video.cfg\"\n{\n\t\"setting.mat_viewportscale\"\t\t\"0.58\"\n}\n";
        remember(dir.path(), None, Some(("h1", v1)));
        remember(dir.path(), None, Some(("h1", "garbage")));
        let f = load_files(dir.path());
        assert_eq!(f.video["h1"]["setting.mat_viewportscale"], "0.58");
    }
}
