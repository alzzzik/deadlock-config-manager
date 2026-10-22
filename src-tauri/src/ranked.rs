//! Ranked-safety check. Since build 6417 (2026-03-27) matchmaking refuses to queue when gameinfo.gi
//! has unsupported changes in certain engine sections (`Citadel_StartMatchmaking_UnverifiedPGI`).
//! ConVars edits are allowed. This compares those sections with Valve's stock file and can put
//! them back to stock while keeping everything else from the pack.

use crate::diff::{self, Change, SEP};
use crate::kv::{self, Node, Value};

/// Sections matchmaking checks, as listed in the GameTracking-Deadlock change.
pub const GUARDED: [&str; 7] =
    ["Engine2", "MaterialSystem2", "NetworkSystem", "Particles", "RenderSystem", "SceneSystem", "WorldRenderer"];

fn is_guarded(path: &str) -> bool {
    let section = path.split(SEP).next().unwrap_or(path);
    GUARDED.iter().any(|g| g.eq_ignore_ascii_case(section))
}

fn guarded_pairs(src: &str) -> Result<Vec<(String, String)>, String> {
    let nodes = kv::parse_document(src).map_err(|e| format!("gameinfo.gi {e}"))?;
    Ok(diff::flatten(&nodes).into_iter().filter(|(k, _)| is_guarded(k)).collect())
}

/// "1" and "1.0", ".1" and "0.1" are the same value to the engine.
fn same_value(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b) || matches!((a.trim().parse::<f64>(), b.trim().parse::<f64>()), (Ok(x), Ok(y)) if x == y)
}

/// Entries in the guarded sections that differ from stock: changed, added or removed.
pub fn guarded_changes(stock: &str, src: &str) -> Result<Vec<Change>, String> {
    let (changes, _) = diff::diff_pairs(&guarded_pairs(stock)?, &guarded_pairs(src)?);
    Ok(changes
        .into_iter()
        .filter(|c| !matches!((&c.before, &c.after), (Some(b), Some(a)) if same_value(b, a)))
        .collect())
}

fn game_info(nodes: &[Node]) -> Result<&Node, String> {
    kv::get(nodes, "GameInfo").filter(|n| n.is_block()).ok_or_else(|| "gameinfo.gi has no GameInfo block".to_string())
}

/// Byte range of a block entry: from its key to just after its closing brace.
fn block_range(n: &Node) -> Option<(usize, usize)> {
    match n.value {
        Value::Block { close, .. } => Some((n.key_start, close + 1)),
        Value::Str(_) => None,
    }
}

fn newline_of(src: &str) -> &'static str {
    if src.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Extends `[start, end)` to cover the whole lines it sits on, so a removed block leaves no blank line.
fn whole_lines(src: &str, start: usize, end: usize) -> (usize, usize) {
    let line_start = src[..start].rfind('\n').map_or(0, |i| i + 1);
    let only_space_before = src[line_start..start].chars().all(|c| c == ' ' || c == '\t');
    let line_end = src[end..].find('\n').map_or(src.len(), |i| end + i + 1);
    let only_space_after = src[end..line_end].trim().is_empty();
    (if only_space_before { line_start } else { start }, if only_space_after { line_end } else { end })
}

/// The pack's gameinfo.gi with every guarded section replaced by Valve's stock version.
/// Text outside those sections (ConVars, comments, SearchPaths, DMM markers) is left as it is.
pub fn make_safe(stock: &str, src: &str) -> Result<String, String> {
    let stock_nodes = kv::parse_document(stock).map_err(|e| format!("Stock gameinfo.gi {e}"))?;
    let nodes = kv::parse_document(src).map_err(|e| format!("gameinfo.gi {e}"))?;
    let (stock_gi, gi) = (game_info(&stock_nodes)?, game_info(&nodes)?);
    let Value::Block { close: gi_close, .. } = gi.value else { unreachable!("checked by game_info") };
    let nl = newline_of(src);
    let stock_text = |n: &Node| block_range(n).map(|(s, e)| stock[s..e].replace("\r\n", "\n").replace('\n', nl));

    // (start, end, replacement) edits on `src`, applied back to front.
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for name in GUARDED {
        let ours: Vec<&Node> = gi.children().iter().filter(|n| n.key.eq_ignore_ascii_case(name)).collect();
        let theirs = kv::get(stock_gi.children(), name).and_then(stock_text);
        let mut ours = ours.into_iter();
        match (ours.next(), theirs) {
            (Some(first), Some(text)) => {
                let (s, e) = block_range(first).ok_or_else(|| format!("{name} in gameinfo.gi is not a block."))?;
                edits.push((s, e, text));
            }
            (Some(first), None) => {
                let (s, e) = block_range(first).ok_or_else(|| format!("{name} in gameinfo.gi is not a block."))?;
                let (s, e) = whole_lines(src, s, e);
                edits.push((s, e, String::new()));
            }
            (None, Some(text)) => edits.push((gi_close, gi_close, format!("\t{text}{nl}"))),
            (None, None) => {}
        }
        // Duplicate sections: the engine would read them too, so they go.
        for extra in ours {
            let (s, e) = block_range(extra).ok_or_else(|| format!("{name} in gameinfo.gi is not a block."))?;
            let (s, e) = whole_lines(src, s, e);
            edits.push((s, e, String::new()));
        }
    }
    edits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    let mut out = src.to_string();
    for (s, e, text) in edits {
        out.replace_range(s..e, &text);
    }
    if !guarded_changes(stock, &out)?.is_empty() {
        return Err("Couldn't put the engine sections back to Valve's defaults.".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stock::EMBEDDED;

    fn with_render(value: &str) -> String {
        EMBEDDED.replacen("IndexBufferPoolSizeMB 32", &format!("IndexBufferPoolSizeMB {value}"), 1)
    }

    #[test]
    fn stock_has_no_guarded_changes() {
        assert!(guarded_changes(EMBEDDED, EMBEDDED).unwrap().is_empty());
        // Formatting-only differences don't count.
        assert!(guarded_changes(EMBEDDED, &with_render("32.0")).unwrap().is_empty());
        let convar_only = EMBEDDED.replacen("ConVars\n\t{", "ConVars\n\t{\n\t\t\"r_fog\" \"0\"", 1);
        assert!(guarded_changes(EMBEDDED, &convar_only).unwrap().is_empty());
    }

    #[test]
    fn reports_changed_added_and_removed_engine_entries() {
        let changed = with_render("128");
        let c = guarded_changes(EMBEDDED, &changed).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].key, format!("RenderSystem{SEP}IndexBufferPoolSizeMB"));
        assert_eq!((c[0].before.as_deref(), c[0].after.as_deref()), (Some("32"), Some("128")));

        let added = EMBEDDED.replacen("UseReverseDepth 1", "UseReverseDepth 1\n\t\tExtraThing 5", 1);
        assert_eq!(guarded_changes(EMBEDDED, &added).unwrap()[0].before, None);
    }

    #[test]
    fn make_safe_restores_engine_sections_and_keeps_convars() {
        let pack = with_render("128").replacen("ConVars\n\t{", "ConVars\n\t{\n\t\t\"r_fog\" \"0\"", 1);
        let safe = make_safe(EMBEDDED, &pack).unwrap();
        assert!(guarded_changes(EMBEDDED, &safe).unwrap().is_empty());
        assert!(safe.contains("\"r_fog\" \"0\""));
        // Already safe: unchanged.
        assert_eq!(make_safe(EMBEDDED, EMBEDDED).unwrap(), EMBEDDED);
    }

    #[test]
    fn make_safe_adds_missing_and_drops_extra_sections() {
        let pack = "\"GameInfo\"\n{\n\tFileSystem\n\t{\n\t\tSearchPaths\n\t\t{\n\t\t\tGame citadel\n\t\t}\n\t}\n\tRenderSystem\n\t{\n\t\tX 1\n\t}\n\tRenderSystem\n\t{\n\t\tY 2\n\t}\n\tConVars\n\t{\n\t\tr_fog 0\n\t}\n}\n";
        let safe = make_safe(EMBEDDED, pack).unwrap();
        assert!(guarded_changes(EMBEDDED, &safe).unwrap().is_empty());
        assert_eq!(safe.matches("RenderSystem").count(), 1);
        assert!(safe.contains("r_fog 0") && safe.contains("Engine2"));
    }

    #[test]
    fn keeps_crlf_line_endings() {
        let pack = with_render("128").replace('\n', "\r\n");
        let safe = make_safe(EMBEDDED, &pack).unwrap();
        assert!(!safe.replace("\r\n", "").contains('\n'));
    }
}
