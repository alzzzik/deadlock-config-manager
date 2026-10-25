//! Crash guard: after the app writes game files, the next Deadlock launch is watched. If the game closes
//! within a minute of starting, the app offers to restore the files from before that write. It never
//! restores by itself; the player decides.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

/// A launch shorter than this counts as "didn't start cleanly".
pub const QUICK_EXIT: Duration = Duration::from_secs(60);
const FILE: &str = "launch-guard.json";
const MAX_FATAL_LEN: usize = 300;

/// A write whose next launch hasn't been seen yet.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    /// Backup taken right before the write; "Restore previous files" puts it back.
    pub backup_id: String,
    pub config: String,
    pub written_at: String,
}

/// The game closed right after starting following a write.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub backup_id: String,
    pub config: String,
    pub ran_seconds: u64,
    /// Last "FATAL ERROR" line from console.log, when the game wrote one.
    pub fatal: Option<String>,
    pub at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Guard {
    pub pending: Option<Pending>,
    /// Shown until the player picks Restore or Keep; also goes into Copy diagnostics.
    pub failure: Option<Failure>,
}

pub fn load(dir: &Path) -> Guard {
    fs::read(dir.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(dir: &Path, guard: &Guard) {
    if let Err(e) = serde_json::to_vec_pretty(guard).map_err(|e| e.to_string()).and_then(|b| fs::write(dir.join(FILE), b).map_err(|e| e.to_string())) {
        crate::diag::error(&format!("saving the launch check: {e}"));
    }
}

/// Called after the app wrote game files: watch the next launch. A newer write replaces an older one.
pub fn arm(dir: &Path, backup_id: &str, config: &str) {
    let pending = Pending { backup_id: backup_id.into(), config: config.into(), written_at: chrono::Local::now().to_rfc3339() };
    save(dir, &Guard { pending: Some(pending), failure: None });
}

/// The player chose Restore or Keep.
pub fn dismiss(dir: &Path) {
    save(dir, &Guard { failure: None, ..load(dir) });
}

/// The watched launch ended. Records a failure when it was short; either way the write is no longer pending.
pub fn finish(dir: &Path, ran: Duration, fatal: Option<String>) -> Option<Failure> {
    let guard = load(dir);
    let pending = guard.pending?;
    let failure = is_quick_exit(ran).then(|| Failure {
        backup_id: pending.backup_id,
        config: pending.config,
        ran_seconds: ran.as_secs(),
        fatal,
        at: chrono::Local::now().to_rfc3339(),
    });
    save(dir, &Guard { pending: None, failure: failure.clone().or(guard.failure) });
    failure
}

pub fn is_quick_exit(ran: Duration) -> bool {
    ran < QUICK_EXIT
}

/// Follows deadlock.exe across ticks: when a run ends, returns how long it lasted. Only runs it saw start
/// count, so a game that was already running when the app opened isn't timed from the app's start.
#[derive(Debug, Default)]
pub struct LaunchWatch {
    seen_closed: bool,
    started: Option<Instant>,
}

impl LaunchWatch {
    pub fn tick(&mut self, running: bool, now: Instant) -> Option<Duration> {
        if !running {
            self.seen_closed = true;
            return self.started.take().map(|start| now.saturating_duration_since(start));
        }
        if self.seen_closed && self.started.is_none() {
            self.started = Some(now);
        }
        None
    }
}

/// The last "FATAL ERROR" line of a console.log, shortened.
pub fn fatal_line(log: &str) -> Option<String> {
    log.lines().rev().find(|l| l.contains("FATAL ERROR")).map(|l| l.trim().chars().take(MAX_FATAL_LEN).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_is_timed_from_start_to_exit() {
        let t0 = Instant::now();
        let mut w = LaunchWatch::default();
        assert_eq!(w.tick(false, t0), None);
        assert_eq!(w.tick(true, t0), None);
        assert_eq!(w.tick(true, t0 + Duration::from_secs(5)), None);
        assert_eq!(w.tick(false, t0 + Duration::from_secs(12)), Some(Duration::from_secs(12)));
        assert_eq!(w.tick(false, t0 + Duration::from_secs(20)), None);
    }

    #[test]
    fn a_game_already_running_when_watching_starts_is_not_timed() {
        let t0 = Instant::now();
        let mut w = LaunchWatch::default();
        assert_eq!(w.tick(true, t0), None);
        assert_eq!(w.tick(false, t0 + Duration::from_secs(10)), None);
        assert_eq!(w.tick(true, t0 + Duration::from_secs(20)), None);
        assert_eq!(w.tick(false, t0 + Duration::from_secs(30)), Some(Duration::from_secs(10)));
    }

    #[test]
    fn only_short_runs_count_as_failures() {
        assert!(is_quick_exit(Duration::from_secs(12)));
        assert!(!is_quick_exit(Duration::from_secs(60)));
        assert!(!is_quick_exit(Duration::from_secs(1800)));
    }

    #[test]
    fn a_failure_is_recorded_once_per_write() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(finish(dir.path(), Duration::from_secs(5), None), None, "nothing armed");
        arm(dir.path(), "2026-10-25_10-00-00", "OptiLock");
        let f = finish(dir.path(), Duration::from_secs(8), Some("FATAL ERROR: x".into())).unwrap();
        assert_eq!((f.backup_id.as_str(), f.ran_seconds), ("2026-10-25_10-00-00", 8));
        assert_eq!(load(dir.path()).pending, None);
        // The next short run isn't blamed on the same write again.
        assert_eq!(finish(dir.path(), Duration::from_secs(3), None), None);
        assert!(load(dir.path()).failure.is_some());
        dismiss(dir.path());
        assert_eq!(load(dir.path()), Guard::default());
    }

    #[test]
    fn a_long_run_clears_the_check_without_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        arm(dir.path(), "b", "alz");
        assert_eq!(finish(dir.path(), Duration::from_secs(900), None), None);
        assert_eq!(load(dir.path()), Guard::default());
    }

    #[test]
    fn finds_the_last_fatal_error_line() {
        let log = "a\nFATAL ERROR: old\nb\n  FATAL ERROR: Unable to load layout file 'file://{resources}/layout/hud.xml'.  \nc";
        assert_eq!(fatal_line(log).as_deref(), Some("FATAL ERROR: Unable to load layout file 'file://{resources}/layout/hud.xml'."));
        assert_eq!(fatal_line("nothing here"), None);
    }
}
