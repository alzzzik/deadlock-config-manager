//! Performance tracking from real matches. With the `-condebug` launch option Deadlock writes
//! `game\citadel\console.log`, and its built-in VProfLite prints a report each time a map unloads:
//!
//! ```text
//! [VProf] Summary of 123650 frames and 180 1-second intervals.  (8930 frames excluded from analysis.)
//! [VProf] FPS: Avg=718.6, P1=276.6
//! [VProf]           FrameTotal   1.39   3.62   123650 ...
//! ```
//!
//! The app reads those reports, labels each with the config that was applied at the time and keeps them in
//! `perf.json`, so players can compare configs on their own PC with their own games.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Shorter sessions (menus, quick hideout visits) say little about a config.
pub const MIN_SECONDS: u64 = 60;
/// Comparisons need at least this many sessions on each side.
pub const MIN_SESSIONS: usize = 3;
const MAX_STORED: usize = 2000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// Log timestamp, e.g. "10/22 23:01:26" (the log has no year).
    pub time: String,
    pub map: String,
    pub frames: u64,
    pub seconds: u64,
    pub avg_fps: f64,
    /// 1% low FPS.
    pub p1_fps: f64,
    /// Average and 99th percentile frame time in ms (FrameTotal row).
    pub frame_ms: f64,
    pub frame_ms_p99: f64,
}

fn after<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    line.find(marker).map(|i| &line[i + marker.len()..])
}

fn timestamp(line: &str) -> String {
    line.get(..14).filter(|t| t.as_bytes().get(2) == Some(&b'/')).unwrap_or("").to_string()
}

/// Every complete VProf report in a console.log, with the map that was being played.
pub fn parse(log: &str) -> Vec<Report> {
    let mut out = Vec::new();
    let mut last_map = String::new();
    let mut session_map = String::new();
    let mut current: Option<Report> = None;
    for line in log.lines() {
        if let Some(rest) = after(line, "Loading map \"") {
            last_map = rest.split('"').next().unwrap_or("").to_string();
        } else if line.contains("[VProf] VProfLite started.") {
            session_map = last_map.clone();
        } else if let Some(rest) = after(line, "[VProf] Summary of ") {
            // "123650 frames and 180 1-second intervals."
            let mut words = rest.split_whitespace();
            let frames = words.next().and_then(|w| w.parse().ok());
            let seconds = words.nth(2).and_then(|w| w.parse().ok());
            current = frames.zip(seconds).map(|(frames, seconds)| Report {
                time: timestamp(line),
                map: session_map.clone(),
                frames,
                seconds,
                avg_fps: 0.0,
                p1_fps: 0.0,
                frame_ms: 0.0,
                frame_ms_p99: 0.0,
            });
        } else if let (Some(rest), Some(r)) = (after(line, "[VProf] FPS: Avg="), current.as_mut()) {
            // "718.6, P1=276.6"
            let (avg, p1) = rest.split_once(", P1=").unwrap_or((rest, ""));
            r.avg_fps = avg.trim().parse().unwrap_or(0.0);
            r.p1_fps = p1.trim().parse().unwrap_or(0.0);
        } else if let (Some(rest), Some(r)) = (after(line, "[VProf]"), current.as_mut()) {
            let cols: Vec<&str> = rest.split_whitespace().collect();
            if cols.first() == Some(&"FrameTotal") && cols.len() >= 3 {
                r.frame_ms = cols[1].parse().unwrap_or(0.0);
                r.frame_ms_p99 = cols[2].parse().unwrap_or(0.0);
                if r.avg_fps > 0.0 {
                    out.push(r.clone());
                }
                current = None;
            }
        }
    }
    out
}

/// True when the log shows the game was started with -condebug (it always is if the file exists, but
/// the command line line confirms it's current).
pub fn condebug_on(log: &str) -> bool {
    log.lines().filter(|l| l.contains("[CommandLine]")).any(|l| l.split_whitespace().any(|w| w.eq_ignore_ascii_case("-condebug")))
}

/// A stored report with the config that was applied when it was recorded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    #[serde(flatten)]
    pub report: Report,
    /// "repo#pack" or "none" when nothing was applied.
    pub config_key: String,
    pub config_name: String,
    #[serde(default)]
    pub ranked_safe: bool,
    #[serde(default)]
    pub game_build: Option<String>,
    /// When the app picked it up (RFC 3339), which gives the year the log lacks.
    pub recorded_at: String,
}

fn store_file(app_dir: &Path) -> PathBuf {
    app_dir.join("perf.json")
}

pub fn load(app_dir: &Path) -> Vec<Session> {
    fs::read(store_file(app_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn same(a: &Report, b: &Report) -> bool {
    a.time == b.time && a.frames == b.frames && a.map == b.map
}

/// Adds reports that aren't stored yet. Returns how many were new.
pub fn record(app_dir: &Path, reports: &[Report], label: impl Fn(&Report) -> Session) -> Result<usize, String> {
    let mut all = load(app_dir);
    let new: Vec<Session> = reports.iter().filter(|r| !all.iter().any(|s| same(&s.report, r))).map(label).collect();
    if new.is_empty() {
        return Ok(0);
    }
    let count = new.len();
    all.extend(new);
    let skip = all.len().saturating_sub(MAX_STORED);
    let kept: Vec<Session> = all.into_iter().skip(skip).collect();
    let json = serde_json::to_vec_pretty(&kept).map_err(|e| e.to_string())?;
    crate::fsops::atomic_write(&store_file(app_dir), &json).map_err(|e| format!("Couldn't save performance data: {e}"))?;
    Ok(count)
}

pub fn clear(app_dir: &Path) -> Result<(), String> {
    match fs::remove_file(store_file(app_dir)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigStats {
    pub config_key: String,
    pub config_name: String,
    pub sessions: usize,
    pub minutes: u64,
    pub median_avg_fps: f64,
    pub median_p1_fps: f64,
    pub median_frame_ms_p99: f64,
    pub last_played: String,
}

/// Per-config medians over sessions of at least [`MIN_SECONDS`], most recently played first.
pub fn summarize(sessions: &[Session]) -> Vec<ConfigStats> {
    let mut groups: BTreeMap<&str, Vec<&Session>> = BTreeMap::new();
    for s in sessions.iter().filter(|s| s.report.seconds >= MIN_SECONDS && s.report.avg_fps > 0.0) {
        groups.entry(&s.config_key).or_default().push(s);
    }
    let mut out: Vec<ConfigStats> = groups
        .into_iter()
        .map(|(key, list)| ConfigStats {
            config_key: key.to_string(),
            config_name: list.last().map(|s| s.config_name.clone()).unwrap_or_default(),
            sessions: list.len(),
            minutes: list.iter().map(|s| s.report.seconds).sum::<u64>() / 60,
            median_avg_fps: median(list.iter().map(|s| s.report.avg_fps).collect()),
            median_p1_fps: median(list.iter().map(|s| s.report.p1_fps).collect()),
            median_frame_ms_p99: median(list.iter().map(|s| s.report.frame_ms_p99).collect()),
            last_played: list.iter().map(|s| s.recorded_at.clone()).max().unwrap_or_default(),
        })
        .collect();
    out.sort_by(|a, b| b.last_played.cmp(&a.last_played));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = include_str!("../fixtures/console_vprof.txt");

    #[test]
    fn parses_real_vprof_reports_with_their_maps() {
        let r = parse(LOG);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].map, "dl_hideout");
        assert_eq!((r[0].frames, r[0].seconds), (435, 1));
        assert_eq!(r[1].map, "jump_school");
        assert_eq!(r[1].time, "10/22 23:01:26");
        assert_eq!((r[1].frames, r[1].seconds), (123650, 180));
        assert_eq!((r[1].avg_fps, r[1].p1_fps), (718.6, 276.6));
        assert_eq!((r[1].frame_ms, r[1].frame_ms_p99), (1.39, 3.62));
        assert!(condebug_on(LOG));
        assert!(parse("nothing here").is_empty());
    }

    fn session(key: &str, avg: f64, secs: u64, time: &str) -> Session {
        Session {
            report: Report { time: time.into(), map: "street_test".into(), frames: 1, seconds: secs, avg_fps: avg, p1_fps: avg / 2.0, frame_ms: 1.0, frame_ms_p99: 3.0 },
            config_key: key.into(),
            config_name: key.into(),
            ranked_safe: false,
            game_build: None,
            recorded_at: time.into(),
        }
    }

    #[test]
    fn summarizes_medians_per_config_and_skips_short_sessions() {
        let s = vec![session("a", 300.0, 900, "1"), session("a", 320.0, 900, "2"), session("a", 900.0, 10, "3"), session("b", 400.0, 600, "4")];
        let stats = summarize(&s);
        assert_eq!(stats[0].config_key, "b");
        let a = stats.iter().find(|x| x.config_key == "a").unwrap();
        assert_eq!(a.sessions, 2);
        assert_eq!(a.median_avg_fps, 310.0);
        assert_eq!(a.minutes, 30);
    }

    #[test]
    fn records_each_report_once() {
        let dir = tempfile::tempdir().unwrap();
        let reports = parse(LOG);
        let label = |r: &Report| Session {
            report: r.clone(),
            config_key: "k".into(),
            config_name: "K".into(),
            ranked_safe: false,
            game_build: None,
            recorded_at: "now".into(),
        };
        assert_eq!(record(dir.path(), &reports, label).unwrap(), 2);
        assert_eq!(record(dir.path(), &reports, label).unwrap(), 0);
        assert_eq!(load(dir.path()).len(), 2);
        clear(dir.path()).unwrap();
        assert!(load(dir.path()).is_empty());
    }

    #[test]
    #[ignore = "reads this PC's real Deadlock console.log"]
    fn parses_this_pcs_console_log() {
        let path = r"C:\\Program Files (x86)\\Steam\\steamapps\\common\\Deadlock\\game\\citadel\\console.log";
        let bytes = fs::read(path).expect("console.log");
        let text = String::from_utf8_lossy(&bytes);
        let reports = parse(&text);
        assert!(!reports.is_empty());
        for r in &reports {
            assert!(r.avg_fps > 0.0 && r.frames > 0, "{r:?}");
        }
        println!("{reports:#?}");
    }
}
