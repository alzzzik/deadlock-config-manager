//! gameinfo.gi checks and edits: SearchPaths enforcement, PGIVersion, DMM-aware comparison.

use crate::kv::{self, Node, Value};
use sha2::{Digest, Sha256};

/// Without explicit Mod/Write lines the engine makes `citadel/addons` the MOD path and
/// dies with "Unable to read default keybinding configuration (user_keys_default)".
pub const REQUIRED_SEARCH_PATHS: [(&str, &str); 9] = [
    ("Game_UILanguage", "citadel_*LANGUAGE*"),
    ("Game_LowViolence", "citadel_lv"),
    ("Game", "citadel/addons"),
    ("Mod", "citadel"),
    ("Write", "citadel"),
    ("Game", "citadel"),
    ("Mod", "core"),
    ("Write", "core"),
    ("Game", "core"),
];

const REQUIRED_FILESYSTEM_KEYS: [(&str, &str); 2] =
    [("UserSettingsPathID", "USRLOCAL"), ("LegacyUserSettingsPathID", "MOD")];

pub const DMM_START: &str = "// Deadlock Mod Manager - Start";
pub const DMM_END: &str = "// Deadlock Mod Manager - End";

/// Parses and checks the GameInfo > FileSystem > SearchPaths structure.
pub fn validate(src: &str) -> Result<Vec<Node>, String> {
    let nodes = kv::parse_document(src).map_err(|e| format!("gameinfo.gi {e}"))?;
    let game_info = kv::get(&nodes, "GameInfo").ok_or("gameinfo.gi has no GameInfo block")?;
    let fs = kv::get(game_info.children(), "FileSystem").ok_or("gameinfo.gi has no FileSystem block")?;
    kv::get(fs.children(), "SearchPaths")
        .filter(|n| n.is_block())
        .ok_or("gameinfo.gi has no SearchPaths block")?;
    Ok(nodes)
}

fn filesystem(nodes: &[Node]) -> Option<&Node> {
    kv::get(kv::get(nodes, "GameInfo")?.children(), "FileSystem")
}

fn search_paths(nodes: &[Node]) -> Option<&Node> {
    kv::get(filesystem(nodes)?.children(), "SearchPaths")
}

/// The explicit MOD/WRITE entries whose absence crashes the game.
const REQUIRED_MOD_WRITE: [(&str, &str); 4] = [("Mod", "citadel"), ("Write", "citadel"), ("Mod", "core"), ("Write", "core")];

fn is_entry(n: &Node, key: &str, value: &str) -> bool {
    n.key.eq_ignore_ascii_case(key) && n.as_str().is_some_and(|v| v.eq_ignore_ascii_case(value))
}

/// A block is fine when it has the explicit Mod/Write lines. Extra paths (DMM's `Game_Language`,
/// Grimoire's `citadel/grimoire`, …) are allowed and left alone.
fn has_mod_write(block: &Node) -> bool {
    REQUIRED_MOD_WRITE.iter().all(|(k, v)| block.children().iter().any(|n| is_entry(n, k, v)))
}

/// True when the SearchPaths block has the explicit Mod/Write lines the game needs.
pub fn search_paths_ok(src: &str) -> Result<bool, String> {
    let nodes = validate(src)?;
    Ok(search_paths(&nodes).is_some_and(has_mod_write))
}

/// Required block with the original's extra entries kept: extra `Game` paths go before
/// `citadel/addons` (mod managers need theirs first), other extras after the language lines.
fn rebuilt_entries(block: &Node) -> Vec<(String, String)> {
    let extras: Vec<(String, String)> = block
        .children()
        .iter()
        .filter(|n| !REQUIRED_SEARCH_PATHS.iter().any(|(k, v)| is_entry(n, k, v)))
        .filter_map(|n| n.as_str().map(|v| (n.key.clone(), v.to_string())))
        .collect();
    let (extra_game, extra_other): (Vec<_>, Vec<_>) = extras.into_iter().partition(|(k, _)| k.eq_ignore_ascii_case("Game"));
    let required = |i: usize| (REQUIRED_SEARCH_PATHS[i].0.to_string(), REQUIRED_SEARCH_PATHS[i].1.to_string());
    (0..2)
        .map(required)
        .chain(extra_other)
        .chain(extra_game)
        .chain((2..REQUIRED_SEARCH_PATHS.len()).map(required))
        .collect()
}

pub fn pgi_version(src: &str) -> Option<String> {
    let nodes = kv::parse(src).ok()?;
    let gi = kv::get(&nodes, "GameInfo")?;
    kv::get(gi.children(), "PGIVersion")?.as_str().map(str::to_string)
}

pub fn has_dmm_markers(src: &str) -> bool {
    src.contains(DMM_START) && src.contains(DMM_END)
}

fn newline_of(src: &str) -> &'static str {
    if src.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Whitespace between the start of the line and `pos`.
fn indent_at(src: &str, pos: usize) -> String {
    let line_start = src[..pos].rfind('\n').map_or(0, |i| i + 1);
    src[line_start..pos].chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

/// Returns the pack's gameinfo.gi with the required SearchPaths block and FileSystem keys.
/// Text outside the edited spots (comments, DMM markers, formatting) is left untouched,
/// and an already-correct file comes back byte-identical.
pub fn enforce(src: &str) -> Result<String, String> {
    let with_paths = enforce_search_paths(src)?;
    ensure_filesystem_keys(&with_paths)
}

fn enforce_search_paths(src: &str) -> Result<String, String> {
    let nodes = validate(src)?;
    let block = search_paths(&nodes).ok_or("gameinfo.gi has no SearchPaths block")?;
    if has_mod_write(block) {
        return Ok(src.to_string());
    }
    let Value::Block { open, close, .. } = block.value else {
        return Err("SearchPaths is not a block".into());
    };
    let nl = newline_of(src);
    let outer = indent_at(src, block.key_start);
    let inner = format!("{outer}    ");
    let body: String = rebuilt_entries(block)
        .iter()
        .map(|(k, v)| format!("{inner}{k:<16} \"{v}\"{nl}"))
        .collect();
    Ok(format!("{}{nl}{body}{outer}{}", &src[..=open], &src[close..]))
}

fn ensure_filesystem_keys(src: &str) -> Result<String, String> {
    let nodes = validate(src)?;
    let fs = filesystem(&nodes).ok_or("gameinfo.gi has no FileSystem block")?;
    let missing: Vec<_> = REQUIRED_FILESYSTEM_KEYS
        .iter()
        .filter(|(k, _)| kv::get(fs.children(), k).is_none())
        .collect();
    if missing.is_empty() {
        return Ok(src.to_string());
    }
    let Value::Block { close, .. } = fs.value else {
        return Err("FileSystem is not a block".into());
    };
    let nl = newline_of(src);
    let inner = format!("{}    ", indent_at(src, fs.key_start));
    let insert_at = src[..close].rfind('\n').map_or(close, |i| i + 1);
    let lines: String = missing.iter().map(|(k, v)| format!("{inner}{k:<24} \"{v}\"{nl}")).collect();
    Ok(format!("{}{lines}{}", &src[..insert_at], &src[insert_at..]))
}

/// Removes the DMM-managed region and any SearchPaths block, then normalizes whitespace,
/// so a file that DMM rewrote compares equal to the one we wrote.
pub fn normalized_for_compare(src: &str) -> String {
    let without_dmm = strip_dmm_region(src);
    let without_paths = match kv::parse(&without_dmm).ok().as_deref().and_then(search_paths) {
        Some(Node { key_start, value: Value::Block { close, .. }, .. }) => {
            format!("{}{}", &without_dmm[..*key_start], &without_dmm[close + 1..])
        }
        _ => without_dmm,
    };
    without_paths
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_dmm_region(src: &str) -> String {
    let mut inside = false;
    src.split_inclusive('\n')
        .filter(|line| {
            let t = line.trim();
            if t.starts_with(DMM_START) {
                inside = true;
                return false;
            }
            if t.starts_with(DMM_END) {
                inside = false;
                return false;
            }
            !inside
        })
        .collect()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn normalized_hash(src: &str) -> String {
    sha256_hex(normalized_for_compare(src).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BROKEN: &str = include_str!("../fixtures/gameinfo_no_mod_write.gi");
    const PACK: &str = include_str!("../fixtures/pack_stable/gameinfo.gi");
    const DMM: &str = include_str!("../fixtures/gameinfo_dmm.gi");

    #[test]
    fn enforce_adds_mod_and_write_lines() {
        assert!(!search_paths_ok(BROKEN).unwrap());
        let fixed = enforce(BROKEN).unwrap();
        assert!(search_paths_ok(&fixed).unwrap());
        assert!(fixed.contains("        Mod              \"citadel\"\n"));
        // Untouched parts survive.
        assert!(fixed.contains("// keep this comment"));
        assert!(fixed.contains("UserSettingsPathID"));
    }

    #[test]
    fn enforce_leaves_correct_block_byte_identical() {
        assert!(search_paths_ok(PACK).unwrap());
        assert_eq!(enforce(PACK).unwrap(), PACK);
        let fixed_once = enforce(BROKEN).unwrap();
        assert_eq!(enforce(&fixed_once).unwrap(), fixed_once);
    }

    #[test]
    fn keeps_extra_mod_manager_paths() {
        // OptiLock-style block: Grimoire path first, Mod/Write present -> left untouched.
        let ok = "GameInfo\n{\n    FileSystem\n    {\n        SearchPaths\n        {\n            Game citadel/grimoire\n            Game citadel/addons\n            Mod citadel\n            Write citadel\n            Game citadel\n            Mod core\n            Write core\n            Game core\n        }\n        UserSettingsPathID \"USRLOCAL\"\n        LegacyUserSettingsPathID \"MOD\"\n    }\n}\n";
        assert!(search_paths_ok(ok).unwrap());
        assert_eq!(enforce(ok).unwrap(), ok);

        // Same block without Mod/Write -> rebuilt, Grimoire still first among Game paths.
        let broken = ok.replace("            Mod citadel\n            Write citadel\n", "");
        let fixed = enforce(&broken).unwrap();
        assert!(search_paths_ok(&fixed).unwrap());
        let grimoire = fixed.find("citadel/grimoire").unwrap();
        assert!(grimoire < fixed.find("citadel/addons").unwrap());
    }

    #[test]
    fn enforce_keeps_dmm_markers_and_crlf() {
        let crlf = DMM.replace('\n', "\r\n");
        let fixed = enforce(&crlf).unwrap();
        assert!(fixed.contains(DMM_START) && fixed.contains(DMM_END));
        assert!(search_paths_ok(&fixed).unwrap());
        assert!(!fixed.replace("\r\n", "").contains('\n'), "mixed line endings");
    }

    #[test]
    fn enforce_inserts_missing_filesystem_keys() {
        let src = "GameInfo\n{\n    FileSystem\n    {\n        SearchPaths\n        {\n            Game \"core\"\n        }\n    }\n}\n";
        let fixed = enforce(src).unwrap();
        assert!(fixed.contains("UserSettingsPathID       \"USRLOCAL\""));
        assert!(fixed.contains("LegacyUserSettingsPathID \"MOD\""));
        assert!(validate(&fixed).is_ok());
    }

    #[test]
    fn validate_rejects_missing_blocks() {
        assert!(validate("GameInfo\n{\n    game \"citadel\"\n}\n").unwrap_err().contains("FileSystem"));
        assert!(validate("Other\n{\n    a \"b\"\n}\n").unwrap_err().contains("GameInfo"));
    }

    #[test]
    fn reads_pgi_version() {
        assert_eq!(
            pgi_version(PACK).as_deref(),
            Some("39A735A413003C88A806B364C32DFC6D077E551B9E4EC6C7B11D41E8E67BFA0C")
        );
        assert_eq!(pgi_version(BROKEN), None);
    }

    #[test]
    fn dmm_rewrite_compares_equal_after_normalizing() {
        let ours = enforce(PACK).unwrap();
        let dmm_version = ours.replacen(
            "        SearchPaths",
            &format!("{DMM_START}\n\t\tSearchPaths"),
            1,
        );
        // Put the end marker right after the SearchPaths block closes.
        let end = dmm_version.find("            Game  \"core\"\n        }\n").unwrap()
            + "            Game  \"core\"\n        }\n".len();
        let dmm_version = format!("{}{DMM_END}\n{}", &dmm_version[..end], &dmm_version[end..]);
        assert!(has_dmm_markers(&dmm_version));
        assert_ne!(sha256_hex(ours.as_bytes()), sha256_hex(dmm_version.as_bytes()));
        assert_eq!(normalized_hash(&ours), normalized_hash(&dmm_version));
        assert_ne!(normalized_hash(&ours), normalized_hash(&ours.replace("\"citadel\"\n    title", "\"x\"\n    title")));
    }
}
