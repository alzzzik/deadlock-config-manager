//! Overlay mode: apply a pack on top of Valve's current gameinfo.gi instead of replacing it, so a game
//! update that adds settings doesn't get rolled back. The pack still wins for everything it sets.
//!
//! - ConVars Valve's file has but the pack neither sets nor comments out are added (in a marked block).
//! - Top-level GameInfo sections Valve's file has and the pack lacks are added (engine sections the
//!   matchmaking check looks at are left to the pack and the ranked-safe option).

use crate::kv::{self, Node, Value};
use crate::overrides::{self, Entry, Markers};
use crate::ranked::GUARDED;

const MARKERS: Markers =
    Markers { start: "// Added from Valve's current gameinfo.gi (overlay) - start", end: "// Added from Valve's current gameinfo.gi (overlay) - end" };

fn block_text<'a>(src: &'a str, n: &Node) -> Option<&'a str> {
    match n.value {
        Value::Block { close, .. } => Some(&src[n.key_start..=close]),
        Value::Str(_) => None,
    }
}

/// Number of things overlay mode would add, for the preview.
pub struct Added {
    pub convars: usize,
    pub sections: Vec<String>,
}

pub fn merge(stock: &str, pack: &str) -> Result<(String, Added), String> {
    // A block from an earlier overlay is rebuilt, not counted as the pack's own settings.
    let pack = &overrides::strip_managed_block(pack, &MARKERS);
    let stock_nodes = kv::parse_document(stock).map_err(|e| format!("Stock gameinfo.gi {e}"))?;
    let stock_gi = kv::get(&stock_nodes, "GameInfo").ok_or("Stock gameinfo.gi has no GameInfo block")?;

    // 1. Missing ConVars.
    let (set, commented) = overrides::convar_names(pack);
    let missing: Vec<Entry> = stock_gi
        .children()
        .iter()
        .find(|n| n.key.eq_ignore_ascii_case("ConVars"))
        .map(|c| c.children())
        .unwrap_or_default()
        .iter()
        .filter_map(|n| n.as_str().map(|v| (n.key.clone(), v.to_string())))
        .filter(|(k, _)| {
            let lk = k.to_ascii_lowercase();
            !set.contains(&lk) && !commented.contains(&lk)
        })
        .map(|(name, value)| Entry { name, value: Some(value) })
        .collect();
    let with_convars = overrides::apply_marked(pack, &missing, &MARKERS)?;

    // 2. Missing top-level sections.
    let nodes = kv::parse_document(&with_convars).map_err(|e| format!("gameinfo.gi {e}"))?;
    let gi = kv::get(&nodes, "GameInfo").ok_or("gameinfo.gi has no GameInfo block")?;
    let Value::Block { close, .. } = gi.value else { return Err("GameInfo is not a block".into()) };
    let nl = if with_convars.contains("\r\n") { "\r\n" } else { "\n" };
    let sections: Vec<&Node> = stock_gi
        .children()
        .iter()
        .filter(|n| n.is_block() && !GUARDED.iter().any(|g| g.eq_ignore_ascii_case(&n.key)))
        .filter(|n| !gi.children().iter().any(|p| p.key.eq_ignore_ascii_case(&n.key)))
        .collect();
    let inserted: String = sections
        .iter()
        .filter_map(|n| block_text(stock, n))
        .map(|t| format!("\t{}{nl}", t.replace("\r\n", "\n").replace('\n', nl)))
        .collect();
    let out = format!("{}{inserted}{}", &with_convars[..close], &with_convars[close..]);
    kv::parse_document(&out).map_err(|e| format!("Overlay result {e}"))?;
    Ok((out, Added { convars: missing.len(), sections: sections.iter().map(|n| n.key.clone()).collect() }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stock::EMBEDDED;

    const PACK: &str = "\"GameInfo\"\n{\n\tFileSystem\n\t{\n\t\tSearchPaths\n\t\t{\n\t\t\tGame citadel\n\t\t}\n\t}\n\tConVars\n\t{\n\t\t\"rate\" \"1\"\n\t\t// \"r_fog\" \"0\"\n\t}\n}\n";

    #[test]
    fn adds_new_valve_settings_but_keeps_the_packs() {
        let stock = EMBEDDED.replacen("\"rate\"", "\"new_valve_cvar\" \"7\"\n\t\t\"r_fog\" \"1\"\n\t\t\"rate\"", 1);
        let (out, added) = merge(&stock, PACK).unwrap();
        assert!(out.contains("\"new_valve_cvar\" \"7\""));
        // The pack's own value and its commented-out line win.
        assert!(out.contains("\"rate\" \"1\""));
        assert!(!out.contains("\"r_fog\" \"1\""));
        assert!(added.convars > 0);
        // Non-engine sections the pack lacks are added; engine sections aren't.
        assert!(added.sections.iter().any(|s| s == "NavSystem"));
        assert!(!added.sections.iter().any(|s| s == "RenderSystem"));
        // Running it again doesn't stack another block.
        let (again, _) = merge(&stock, &out).unwrap();
        assert_eq!(again.matches(MARKERS.start).count(), 1);
    }

    #[test]
    fn stock_on_stock_adds_nothing() {
        let (out, added) = merge(EMBEDDED, EMBEDDED).unwrap();
        assert_eq!(added.convars, 0);
        assert!(added.sections.is_empty());
        assert_eq!(out, EMBEDDED);
    }
}
