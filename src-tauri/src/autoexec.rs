//! A pack's `autoexec.cfg`: its lines go into Deadlock's `game\citadel\cfg\autoexec.cfg` inside a marked
//! block, so the user's own lines stay and re-applying replaces the block. The game runs the file when
//! launched with `+exec autoexec` (offered in Launch options).
//!
//! Only plain `setting value` lines are taken. Commands that bind keys, connect, run other files or
//! change cheats are dropped, since a pack shouldn't do those behind the user's back.

use crate::diff::{Change, FileDiff};

const START: &str = "// Deadlock Config Manager: pack autoexec - start";
const END: &str = "// Deadlock Config Manager: pack autoexec - end";
const MAX_LINES: usize = 400;

/// First words that are never copied from a pack.
const BLOCKED: [&str; 16] = [
    "bind", "unbind", "unbindall", "alias", "exec", "connect", "disconnect", "password", "rcon", "rcon_password", "quit", "exit",
    "sv_cheats", "host_timescale", "say", "kill",
];

fn first_word(line: &str) -> &str {
    line.split_whitespace().next().unwrap_or("").trim_matches('"')
}

fn is_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The pack's usable lines and how many were dropped.
pub fn filter(pack: &str) -> (Vec<String>, usize) {
    let mut kept = Vec::new();
    let mut dropped = 0;
    for raw in pack.lines() {
        let line = raw.split("//").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let word = first_word(line);
        let ok = is_name(word)
            && !BLOCKED.iter().any(|b| b.eq_ignore_ascii_case(word))
            && !word.to_ascii_lowercase().starts_with("rcon")
            && !line.contains([';', '\n', '\r'])
            && kept.len() < MAX_LINES;
        if ok {
            kept.push(line.to_string());
        } else {
            dropped += 1;
        }
    }
    (kept, dropped)
}

fn strip_block(src: &str) -> String {
    let (Some(s), Some(e)) = (src.find(START), src.find(END)) else { return src.to_string() };
    if e < s {
        return src.to_string();
    }
    let start = src[..s].rfind('\n').map_or(0, |i| i + 1);
    let end = src[e..].find('\n').map_or(src.len(), |i| e + i + 1);
    format!("{}{}", &src[..start], &src[end..])
}

/// The user's autoexec.cfg with the pack block replaced (or added at the end).
pub fn merge(existing: Option<&str>, lines: &[String]) -> String {
    let base = strip_block(existing.unwrap_or(""));
    if lines.is_empty() {
        return base;
    }
    let nl = if base.contains("\r\n") { "\r\n" } else { "\n" };
    let sep = if base.is_empty() || base.ends_with('\n') { "" } else { nl };
    let block: String = lines.iter().map(|l| format!("{l}{nl}")).collect();
    format!("{base}{sep}{START}{nl}{block}{END}{nl}")
}

fn settings(src: &str) -> Vec<(String, String)> {
    strip_block(src)
        .lines()
        .chain(block_lines(src).iter().map(String::as_str))
        .filter_map(|l| {
            let l = l.split("//").next().unwrap_or("").trim();
            let (k, v) = l.split_once(char::is_whitespace).unwrap_or((l, ""));
            (!k.is_empty()).then(|| (k.trim_matches('"').to_string(), v.trim().trim_matches('"').to_string()))
        })
        .collect()
}

fn block_lines(src: &str) -> Vec<String> {
    match (src.find(START), src.find(END)) {
        (Some(s), Some(e)) if s < e => src[s + START.len()..e].lines().map(str::to_string).collect(),
        _ => Vec::new(),
    }
}

/// Preview of the autoexec.cfg change, in the same shape as the other files.
pub fn diff(existing: Option<&str>, out: &str) -> FileDiff {
    let before = existing.map(settings).unwrap_or_default();
    let (changes, unchanged): (Vec<Change>, usize) = crate::diff::diff_pairs(&before, &settings(out));
    FileDiff { file: "autoexec.cfg".into(), created: existing.is_none(), changes, unchanged }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_settings_and_drops_risky_commands() {
        let pack = "// comment\nfps_max 300\nbind mouse1 +attack\nexec other\nr_fullscreen_gamma \"2.2\"\nconnect 1.2.3.4\nrcon_password x\nsay hi;quit\n";
        let (kept, dropped) = filter(pack);
        assert_eq!(kept, vec!["fps_max 300".to_string(), "r_fullscreen_gamma \"2.2\"".to_string()]);
        assert_eq!(dropped, 5);
    }

    #[test]
    fn merge_keeps_user_lines_and_replaces_the_block() {
        let user = "sensitivity 1.2\n";
        let once = merge(Some(user), &["fps_max 300".into()]);
        assert!(once.starts_with("sensitivity 1.2\n") && once.contains("fps_max 300"));
        let twice = merge(Some(&once), &["fps_max 200".into()]);
        assert_eq!(twice.matches(START).count(), 1);
        assert!(twice.contains("fps_max 200") && !twice.contains("fps_max 300"));
        assert_eq!(merge(Some(&twice), &[]), user);
        assert!(merge(None, &["a 1".into()]).starts_with(START));
    }

    #[test]
    fn diff_lists_new_settings() {
        let out = merge(Some("sensitivity 1\n"), &["fps_max 300".into()]);
        let d = diff(Some("sensitivity 1\n"), &out);
        assert_eq!(d.changes.len(), 1);
        assert_eq!(d.changes[0].key, "fps_max");
        assert_eq!(d.unchanged, 1);
    }
}
