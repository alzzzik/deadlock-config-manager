//! Deadlock's Steam launch options, stored in `userdata\<account>\config\localconfig.vdf` under
//! UserLocalConfigStore > Software > Valve > Steam > apps > 1422450 > LaunchOptions.
//! Steam rewrites this file on exit, so it's only edited while Steam is closed.

use crate::kv::{self, Node};
use std::path::{Path, PathBuf};

const APP_ID: &str = "1422450";
const STEAM_EXE: &str = "steam.exe";
const MAX_OPTIONS_CHARS: usize = 1000;

pub fn localconfig_path(steam_root: &Path, account_id: &str) -> PathBuf {
    steam_root.join("userdata").join(account_id).join(r"config\localconfig.vdf")
}

pub fn steam_running() -> bool {
    crate::fsops::is_process_running(STEAM_EXE)
}

fn path_get<'a>(nodes: &'a [Node], path: &[&str]) -> Option<&'a Node> {
    let (first, rest) = path.split_first()?;
    let node = kv::get(nodes, first)?;
    if rest.is_empty() {
        Some(node)
    } else {
        path_get(node.children(), rest)
    }
}

const APP_PATH: [&str; 6] = ["UserLocalConfigStore", "Software", "Valve", "Steam", "apps", APP_ID];

/// Current launch options ("" when none are set). Err when the file isn't a Steam localconfig.
pub fn read(src: &str) -> Result<String, String> {
    let nodes = kv::parse(src).map_err(|e| format!("localconfig.vdf {e}"))?;
    kv::get(&nodes, "UserLocalConfigStore").ok_or("localconfig.vdf isn't a Steam config file.")?;
    Ok(path_get(&nodes, &APP_PATH)
        .and_then(|app| kv::get(app.children(), "LaunchOptions"))
        .and_then(|n| n.as_str())
        .unwrap_or_default()
        .to_string())
}

/// Byte range of the quoted value that follows the key starting at `key_start`.
fn value_span(src: &str, key_start: usize) -> Option<(usize, usize)> {
    let bytes = src.as_bytes();
    let mut i = key_start;
    // Skip the key (quoted).
    if bytes.get(i) != Some(&b'"') {
        return None;
    }
    i += 1;
    while i < bytes.len() && bytes[i] != b'"' {
        i += if bytes[i] == b'\\' { 2 } else { 1 };
    }
    i += 1;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if bytes.get(i) != Some(&b'"') {
        return None;
    }
    let start = i;
    i += 1;
    while i < bytes.len() && bytes[i] != b'"' {
        i += if bytes[i] == b'\\' { 2 } else { 1 };
    }
    (i < bytes.len()).then_some((start, i + 1))
}

fn escape(v: &str) -> String {
    v.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `src` with Deadlock's launch options set to `value`. Creates the LaunchOptions line if needed.
pub fn write(src: &str, value: &str) -> Result<String, String> {
    if value.chars().count() > MAX_OPTIONS_CHARS || value.contains(['\n', '\r']) {
        return Err("Launch options must be one line.".into());
    }
    let nodes = kv::parse(src).map_err(|e| format!("localconfig.vdf {e}"))?;
    let app = path_get(&nodes, &APP_PATH).ok_or("Steam has no settings for Deadlock in this account yet. Launch Deadlock once, then try again.")?;
    let quoted = format!("\"{}\"", escape(value.trim()));
    let out = match kv::get(app.children(), "LaunchOptions") {
        Some(n) => {
            let (s, e) = value_span(src, n.key_start).ok_or("Couldn't read the current launch options.")?;
            format!("{}{quoted}{}", &src[..s], &src[e..])
        }
        None => {
            let kv::Value::Block { open, .. } = app.value else { return Err("Unexpected localconfig.vdf layout.".into()) };
            let line_start = src[..app.key_start].rfind('\n').map_or(0, |i| i + 1);
            let indent: String = src[line_start..app.key_start].chars().take_while(|c| c.is_whitespace()).collect();
            let nl = if src.contains("\r\n") { "\r\n" } else { "\n" };
            format!("{}{nl}{indent}\t\"LaunchOptions\"\t\t{quoted}{}", &src[..=open], &src[open + 1..])
        }
    };
    if read(&out)? != value.trim() {
        return Err("Couldn't update the launch options safely.".into());
    }
    Ok(out)
}

/// Adds or removes a single flag such as `-vulkan`, keeping the other options as they are.
pub fn with_flag(options: &str, flag: &str, on: bool) -> String {
    let mut parts: Vec<&str> = options.split_whitespace().filter(|p| !p.eq_ignore_ascii_case(flag)).collect();
    if on {
        parts.push(flag);
    }
    parts.join(" ")
}

pub fn has_flag(options: &str, flag: &str) -> bool {
    options.split_whitespace().any(|p| p.eq_ignore_ascii_case(flag))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: &str = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"1422450\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LastPlayed\"\t\t\"1\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n";

    #[test]
    fn adds_then_replaces_launch_options() {
        assert_eq!(read(CFG).unwrap(), "");
        let once = write(CFG, "-vulkan").unwrap();
        assert_eq!(read(&once).unwrap(), "-vulkan");
        let twice = write(&once, "-novid \"+exec x\"").unwrap();
        assert_eq!(read(&twice).unwrap(), "-novid \"+exec x\"");
        assert!(twice.contains("\"LastPlayed\""));
        assert_eq!(twice.matches("LaunchOptions").count(), 1);
    }

    #[test]
    fn refuses_odd_input() {
        assert!(write(CFG, "a\nb").is_err());
        assert!(write("\"UserLocalConfigStore\"\n{\n}\n", "-vulkan").is_err());
        assert!(read("\"Other\"\n{\n}\n").is_err());
    }

    #[test]
    fn toggles_flags() {
        assert_eq!(with_flag("-novid", "-vulkan", true), "-novid -vulkan");
        assert_eq!(with_flag("-novid -VULKAN", "-vulkan", false), "-novid");
        assert!(has_flag("-a -vulkan", "-Vulkan"));
    }
}
