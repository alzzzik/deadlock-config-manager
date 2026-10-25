//! Hands-off mode: while the app runs (window open or in the tray), check every few seconds whether a
//! game update replaced the applied gameinfo.gi, and re-apply the config when Deadlock is closed.

use crate::commands::{self, AppState};
use crate::diag;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

/// Short enough to finish between Steam's update and the player pressing Play.
const CHECK_EVERY: Duration = Duration::from_secs(10);
/// While a launch check is pending, Deadlock is looked for this often, so a quick crash isn't missed.
const LAUNCH_TICK: Duration = Duration::from_secs(2);
/// UI event telling the window to refresh its status.
pub const FILES_CHANGED: &str = "files-changed";
/// UI event: new performance reports were recorded.
pub const PERF_CHANGED: &str = "perf-changed";
/// UI event: Deadlock closed right after starting following the app's last write.
pub const LAUNCH_FAILED: &str = "launch-failed";

pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        // Hash of the installed gameinfo.gi we last failed to fix, so a failure isn't retried every tick.
        let mut gave_up_on: Option<String> = None;
        let mut log_seen: Option<std::time::SystemTime> = None;
        let mut launch = crate::launch_guard::LaunchWatch::default();
        loop {
            let state = app.state::<AppState>();
            if let Some(new) = commands::collect_perf(&state, &mut log_seen) {
                if new > 0 {
                    let _ = app.emit(PERF_CHANGED, ());
                }
            }
            for _ in 0..(CHECK_EVERY.as_secs() / LAUNCH_TICK.as_secs()) {
                std::thread::sleep(LAUNCH_TICK);
                check_launch(&app, &state, &mut launch);
            }
            let s = state.snapshot();
            if !s.auto_reapply || s.applied.is_none() || crate::fsops::deadlock_running() {
                continue;
            }
            let ov = state.overview(&s);
            let replaced_by_game = matches!(ov.status, crate::apply::Status::Changed { .. }) && ov.changed_by.as_deref() == Some("game");
            if !replaced_by_game {
                gave_up_on = None;
                continue;
            }
            let current = ov.gameinfo_path.as_deref().and_then(|p| std::fs::read(p).ok()).map(|b| crate::gameinfo::sha256_hex(&b));
            if current.is_some() && current == gave_up_on {
                continue;
            }
            let pack = s.applied.as_ref().map(|a| a.pack_name.clone()).unwrap_or_default();
            match commands::reapply_after_game_update(&state) {
                Ok((msg, behind)) => {
                    diag::info(&format!("auto re-apply: {msg}{}", if behind { " (repo not updated since the game update)" } else { "" }));
                    let body = if behind {
                        format!("A game update replaced gameinfo.gi, so {pack} was applied again. Its repo hasn't been updated since the game update yet.")
                    } else {
                        format!("A game update replaced gameinfo.gi, so the latest {pack} was applied again.")
                    };
                    notify(&app, "Config re-applied", &body);
                    gave_up_on = None;
                }
                Err(e) => {
                    diag::error(&format!("auto re-apply: {e}"));
                    notify(&app, "Couldn't re-apply your config", &format!("{e}"));
                    gave_up_on = current;
                }
            }
            let _ = app.emit(FILES_CHANGED, ());
        }
    });
}

/// Times Deadlock's next run after a write; a run under a minute raises the "didn't start cleanly" prompt.
fn check_launch(app: &AppHandle, state: &AppState, launch: &mut crate::launch_guard::LaunchWatch) {
    if crate::launch_guard::load(&state.dir).pending.is_none() {
        *launch = Default::default();
        return;
    }
    let Some(ran) = launch.tick(crate::fsops::deadlock_running(), std::time::Instant::now()) else {
        return;
    };
    let started = std::time::SystemTime::now() - ran;
    match commands::finish_launch_check(state, ran, started) {
        Some(f) => {
            diag::error(&format!("Deadlock closed {}s after starting with {} applied{}", f.ran_seconds, f.config, f.fatal.as_deref().map(|l| format!(": {l}")).unwrap_or_default()));
            notify(app, "Deadlock didn't start cleanly", "Open Deadlock Config Manager to restore your previous files.");
            let _ = app.emit(LAUNCH_FAILED, ());
        }
        None => diag::info(&format!("launch check passed ({}s)", ran.as_secs())),
    }
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        diag::error(&format!("notification: {e}"));
    }
}
