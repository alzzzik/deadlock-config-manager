//! Local diagnostics: a small rotating log in %APPDATA%\DeadlockConfigManager\logs, a panic hook,
//! and the "Copy diagnostics" report. Nothing is ever sent anywhere; the user pastes the report.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The log rolls over to `app.1.log` once it passes this size.
const MAX_LOG_BYTES: u64 = 512 * 1024;
/// How many trailing log lines go into the diagnostics report.
const REPORT_LOG_LINES: usize = 40;

static LOG_FILE: OnceLock<PathBuf> = OnceLock::new();
static WRITE_LOCK: Mutex<()> = Mutex::new(());

pub fn log_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("logs")
}

/// Sets the log location and installs a panic hook that records panics before the default output.
pub fn init(app_dir: &Path) {
    let dir = log_dir(app_dir);
    let _ = fs::create_dir_all(&dir);
    let _ = LOG_FILE.set(dir.join("app.log"));
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        error(&format!("panic: {info}"));
        default_hook(info);
    }));
    info(&format!("started v{}", env!("CARGO_PKG_VERSION")));
}

pub fn info(msg: &str) {
    write_line("INFO", msg);
}

pub fn error(msg: &str) {
    write_line("ERROR", msg);
}

/// Logs the error side of a command result and passes the result through unchanged.
pub fn logged<T>(what: &str, result: Result<T, String>) -> Result<T, String> {
    if let Err(e) = &result {
        error(&format!("{what}: {e}"));
    }
    result
}

fn write_line(level: &str, msg: &str) {
    let Some(path) = LOG_FILE.get() else { return };
    let _guard = WRITE_LOCK.lock();
    rotate_if_large(path, MAX_LOG_BYTES);
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let line = format!("{stamp} {level} {}\n", msg.replace('\n', " | "));
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

fn rotate_if_large(path: &Path, max: u64) {
    if fs::metadata(path).map(|m| m.len() > max).unwrap_or(false) {
        let _ = fs::rename(path, path.with_file_name("app.1.log"));
    }
}

/// Last `n` lines of the current log (or fewer).
pub fn tail(path: &Path, n: usize) -> Vec<String> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].iter().map(|l| l.to_string()).collect()
}

/// Replaces the Windows user name (alone and inside paths) with `<user>`.
pub fn redact(text: &str, user: Option<&str>) -> String {
    match user.filter(|u| u.len() >= 2) {
        Some(u) => replace_ignore_case(text, u, "<user>"),
        None => text.to_string(),
    }
}

fn replace_ignore_case(text: &str, needle: &str, with: &str) -> String {
    let lower = text.to_lowercase();
    let needle_lower = needle.to_lowercase();
    // Lowercasing can change byte lengths for some scripts; fall back to an exact replace then.
    if lower.len() != text.len() {
        return text.replace(needle, with);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = 0;
    for (i, _) in lower.match_indices(&needle_lower) {
        if i < rest {
            continue;
        }
        out.push_str(&text[rest..i]);
        out.push_str(with);
        rest = i + needle.len();
    }
    out.push_str(&text[rest..]);
    out
}

/// "Windows 11 23H2 (build 22631)" from the registry, or a plain fallback.
pub fn windows_version() -> String {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    let key = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let Ok(key) = key else { return "Windows (unknown version)".into() };
    let build: String = key.get_value("CurrentBuild").unwrap_or_default();
    let display: String = key.get_value("DisplayVersion").unwrap_or_default();
    // ProductName still says "Windows 10" on Windows 11; the build number tells them apart.
    let name = if build.parse::<u32>().unwrap_or(0) >= 22000 { "Windows 11" } else { "Windows 10" };
    format!("{name} {display} (build {build})").replace("  ", " ")
}

/// How this copy was installed. Only the NSIS install updates itself; Scoop and portable copies
/// would otherwise get a second, installed copy from the updater.
pub fn install_kind(exe: &Path) -> &'static str {
    let lower = exe.to_string_lossy().to_lowercase();
    if lower.contains(r"\scoop\apps\") {
        "scoop"
    } else if exe.parent().is_some_and(|d| d.join("uninstall.exe").is_file()) {
        "installer"
    } else {
        "portable"
    }
}

/// Everything a bug report needs, as plain text with the user name removed.
pub struct ReportInput<'a> {
    pub app_version: &'a str,
    pub windows: &'a str,
    pub lines: Vec<(&'a str, String)>,
    pub log: Vec<String>,
    pub user: Option<&'a str>,
}

pub fn report(input: &ReportInput) -> String {
    let mut out = format!("Deadlock Config Manager {}\n{}\n", input.app_version, input.windows);
    for (label, value) in &input.lines {
        out.push_str(&format!("{label}: {value}\n"));
    }
    out.push_str("\nRecent log:\n");
    if input.log.is_empty() {
        out.push_str("(empty)\n");
    }
    for l in &input.log {
        out.push_str(l);
        out.push('\n');
    }
    redact(&out, input.user)
}

pub fn report_log(app_dir: &Path) -> Vec<String> {
    tail(&log_dir(app_dir).join("app.log"), REPORT_LOG_LINES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_user_name_in_paths_case_insensitively() {
        let text = r"C:\Users\Bobby\AppData and c:\users\bobby\x, bobby";
        assert_eq!(redact(text, Some("bobby")), r"C:\Users\<user>\AppData and c:\users\<user>\x, <user>");
        assert_eq!(redact(text, None), text);
        // One-letter names would mangle everything, so they're left alone.
        assert_eq!(redact("a b", Some("a")), "a b");
    }

    #[test]
    fn tells_install_kinds_apart() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("app.exe");
        assert_eq!(install_kind(&exe), "portable");
        fs::write(dir.path().join("uninstall.exe"), "").unwrap();
        assert_eq!(install_kind(&exe), "installer");
        assert_eq!(install_kind(Path::new(r"C:\Users\a\scoop\apps\dcm\current\app.exe")), "scoop");
    }

    #[test]
    fn tail_returns_last_lines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("app.log");
        fs::write(&p, "1\n2\n3\n4\n").unwrap();
        assert_eq!(tail(&p, 2), vec!["3", "4"]);
        assert_eq!(tail(&p, 10).len(), 4);
        assert!(tail(&dir.path().join("missing.log"), 3).is_empty());
    }

    #[test]
    fn rotates_large_log() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("app.log");
        fs::write(&p, "x".repeat(20)).unwrap();
        rotate_if_large(&p, 10);
        assert!(!p.exists() && dir.path().join("app.1.log").exists());
    }

    #[test]
    fn report_lists_fields_and_redacts() {
        let r = report(&ReportInput {
            app_version: "0.3.0",
            windows: "Windows 11 23H2 (build 22631)",
            lines: vec![("gameinfo.gi", r"C:\Users\bob\game\gameinfo.gi".into())],
            log: vec![],
            user: Some("bob"),
        });
        assert!(r.contains("Deadlock Config Manager 0.3.0"));
        assert!(r.contains(r"gameinfo.gi: C:\Users\<user>\game\gameinfo.gi"));
        assert!(r.contains("(empty)"));
    }
}
