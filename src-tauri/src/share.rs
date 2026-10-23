//! Share codes: a short text that recreates a config on someone else's PC.
//!
//! - A pack code names a repo pack (pinned to a commit) plus the sharer's tweaks and ranked-safe choice.
//! - A config code carries a saved config as its ConVars changes against Valve's file plus the
//!   video.txt settings (no GPU lines), so it stays short enough to paste in chat.
//!
//! Format: `DCM1-` + base64url(deflate(JSON)). Importing a code always produces a saved config,
//! which the user previews and applies like any other.

use crate::diff;
use crate::overrides::Entry;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

pub const PREFIX: &str = "DCM1-";
/// Longest code accepted, and the most a code may inflate to; real codes are a few KB.
const MAX_CODE_CHARS: usize = 64 * 1024;
const MAX_JSON_BYTES: u64 = 512 * 1024;
pub const LINK_SCHEME: &str = "deadlockcfg";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "k", rename_all = "camelCase")]
pub enum Shared {
    #[serde(rename = "pack")]
    Pack {
        repo: String,
        folder: String,
        name: String,
        #[serde(default)]
        rev: Option<String>,
        #[serde(default)]
        video_file: Option<String>,
        #[serde(default)]
        gameinfo_file: Option<String>,
        #[serde(default)]
        safe: bool,
        #[serde(default)]
        tweaks: Vec<Entry>,
    },
    #[serde(rename = "cfg")]
    Config {
        name: String,
        #[serde(default)]
        notes: String,
        /// ConVars changes against Valve's gameinfo.gi.
        convars: Vec<Entry>,
        /// video.txt settings without hardware lines.
        #[serde(default)]
        video: Vec<(String, String)>,
    },
}

pub fn encode(s: &Shared) -> Result<String, String> {
    let json = serde_json::to_vec(s).map_err(|e| e.to_string())?;
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
    enc.write_all(&json).map_err(|e| e.to_string())?;
    let bytes = enc.finish().map_err(|e| e.to_string())?;
    Ok(format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes)))
}

/// Finds a code in pasted text (a whole share message or a `deadlockcfg://` link) and decodes it.
pub fn decode(text: &str) -> Result<Shared, String> {
    let start = text.find(PREFIX).ok_or("No share code found. Codes start with DCM1-.")?;
    let body: String = text[start + PREFIX.len()..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if body.is_empty() || body.len() > MAX_CODE_CHARS {
        return Err("That share code is empty or too long.".into());
    }
    let bytes = URL_SAFE_NO_PAD.decode(body.as_bytes()).map_err(|_| "That share code is damaged (not valid base64).".to_string())?;
    let mut json = Vec::new();
    DeflateDecoder::new(&bytes[..])
        .take(MAX_JSON_BYTES)
        .read_to_end(&mut json)
        .map_err(|_| "That share code is damaged.".to_string())?;
    let shared: Shared = serde_json::from_slice(&json).map_err(|_| "That share code is from a newer app version or damaged.".to_string())?;
    validate(&shared)?;
    Ok(shared)
}

/// Settings a shared config may not set: cheats and visual exploits.
const DENIED: [&str; 6] = ["sv_cheats", "host_timescale", "r_drawothermodels", "mat_wireframe", "r_drawworld", "mat_fullbright"];

fn check_denied(entries: &[Entry]) -> Result<(), String> {
    match entries.iter().find(|e| DENIED.iter().any(|d| d.eq_ignore_ascii_case(&e.name)) && e.value.is_some()) {
        Some(e) => Err(format!("That share code sets {}, which isn't allowed.", e.name)),
        None => Ok(()),
    }
}

fn validate(s: &Shared) -> Result<(), String> {
    match s {
        Shared::Pack { repo, rev, .. } if rev.as_deref().is_some_and(|r| !crate::packs::is_revision(r)) => {
            Err(format!("That share code for {repo} has an invalid commit."))
        }
        Shared::Pack { repo, tweaks, .. } => {
            crate::repos::canonical(repo)?;
            crate::overrides::validate(tweaks)?;
            check_denied(tweaks)
        }
        Shared::Config { convars, video, .. } => {
            crate::overrides::validate(convars)?;
            check_denied(convars)?;
            let bad = video.iter().any(|(k, v)| k.is_empty() || k.contains(['"', '\n', '{', '}']) || v.contains(['"', '\n', '{', '}']));
            if bad {
                return Err("That share code has an invalid video.txt line.".into());
            }
            Ok(())
        }
    }
}

/// ConVars differences of `src` against `stock`, as overrides that rebuild `src`'s ConVars from stock.
pub fn convar_delta(stock: &str, src: &str) -> Result<Vec<Entry>, String> {
    let convars = |text: &str| -> Result<Vec<(String, String)>, String> {
        let nodes = crate::kv::parse_document(text).map_err(|e| format!("gameinfo.gi {e}"))?;
        Ok(diff::flatten(&nodes)
            .into_iter()
            .filter_map(|(k, v)| crate::convars::convar_of(&k).map(|n| (format!("ConVars{}{n}", diff::SEP), v)))
            .collect())
    };
    let (changes, _) = diff::diff_pairs(&convars(stock)?, &convars(src)?);
    Ok(changes
        .into_iter()
        .filter_map(|c| {
            let name = crate::convars::convar_of(&c.key)?.to_string();
            Some(Entry { name, value: c.after })
        })
        .collect())
}

/// Rebuilds gameinfo.gi and video.txt for a config code. Engine sections come from `stock`, so the
/// result is always ranked-safe.
pub fn config_files(stock: &str, convars: &[Entry], video: &[(String, String)]) -> Result<crate::apply::PackFiles, String> {
    let gameinfo = crate::overrides::apply(stock, convars)?;
    let video = (!video.is_empty()).then(|| {
        let lines: String = video.iter().map(|(k, v)| format!("\t\"{k}\"\t\t\"{v}\"\r\n")).collect();
        format!("\"video.cfg\"\r\n{{\r\n{lines}}}\r\n")
    });
    Ok(crate::apply::PackFiles { gameinfo, video })
}

/// The message to paste in chat: what it is, the code, and where to get the app.
pub fn message(name: &str, code: &str) -> String {
    format!(
        "{name} (Deadlock config)\nOpen: {LINK_SCHEME}://import/{code}\nOr paste the code in Deadlock Config Manager (My configs > Import code): {code}\nGet the app: https://github.com/alzzzik/deadlock-config-manager"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stock::EMBEDDED;

    #[test]
    fn pack_code_round_trips_and_is_short() {
        let s = Shared::Pack {
            repo: "dacooderr/OptiLock".into(),
            folder: "OptiLock Potato Config".into(),
            name: "Potato".into(),
            rev: Some("0123456789abcdef0123456789abcdef01234567".into()),
            video_file: None,
            gameinfo_file: None,
            safe: true,
            tweaks: vec![Entry { name: "citadel_camera_hero_fov".into(), value: Some("100".into()) }],
        };
        let code = encode(&s).unwrap();
        assert!(code.starts_with(PREFIX) && code.len() < 300, "{} chars", code.len());
        assert_eq!(decode(&code).unwrap(), s);
        // Found inside a whole message or link.
        assert_eq!(decode(&message("Potato", &code)).unwrap(), s);
    }

    #[test]
    fn config_code_rebuilds_the_convars() {
        let saved = crate::overrides::apply(
            EMBEDDED,
            &[Entry { name: "r_ssao".into(), value: Some("false".into()) }, Entry { name: "fps_max".into(), value: Some("300".into()) }],
        )
        .unwrap();
        let delta = convar_delta(EMBEDDED, &saved).unwrap();
        assert_eq!(delta.len(), 2);
        let code = encode(&Shared::Config { name: "x".into(), notes: String::new(), convars: delta.clone(), video: vec![("setting.mat_vsync".into(), "0".into())] }).unwrap();
        let Shared::Config { convars, video, .. } = decode(&code).unwrap() else { panic!("wrong kind") };
        let files = config_files(EMBEDDED, &convars, &video).unwrap();
        assert!(convar_delta(&saved, &files.gameinfo).unwrap().is_empty());
        assert!(files.video.unwrap().contains("setting.mat_vsync"));
    }

    #[test]
    fn rejects_cheats() {
        let code = encode(&Shared::Config {
            name: "x".into(),
            notes: String::new(),
            convars: vec![Entry { name: "sv_cheats".into(), value: Some("1".into()) }],
            video: vec![],
        })
        .unwrap();
        assert!(decode(&code).unwrap_err().contains("sv_cheats"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode("hello").is_err());
        assert!(decode("DCM1-!!!").is_err());
        assert!(decode("DCM1-AAAA").is_err());
    }
}
