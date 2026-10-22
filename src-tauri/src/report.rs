//! "What the game update changed" for the applied config, shown after a patch replaced gameinfo.gi.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReport {
    /// Steam build ids before and after, when known.
    pub from_build: Option<String>,
    pub to_build: Option<String>,
    /// Settings Valve's new file has that the config doesn't; re-applying without overlay removes them.
    pub new_settings: Vec<String>,
    /// Config settings the game ignores in gameinfo.gi.
    pub ignored: Vec<String>,
    /// Engine-section entries where the config differs from the new Valve file (can block ranked).
    pub engine_changes: usize,
}

/// Compares the applied config's own gameinfo.gi with Valve's new one (the installed file after a patch).
pub fn build(pack: &str, new_stock: &str, from_build: Option<String>, to_build: Option<String>) -> UpdateReport {
    let (set, commented) = crate::overrides::convar_names(pack);
    let (stock_set, _) = crate::overrides::convar_names(new_stock);
    let new_settings = stock_set.into_iter().filter(|k| !set.contains(k) && !commented.contains(k)).collect();
    let ignored = set.into_iter().filter(|k| crate::convars::lookup(k).is_some_and(|i| i.ignored)).collect();
    let engine_changes = crate::ranked::guarded_changes(new_stock, pack).map(|c| c.len()).unwrap_or(0);
    UpdateReport { from_build, to_build, new_settings, ignored, engine_changes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stock::EMBEDDED;

    #[test]
    fn lists_new_ignored_and_engine_changes() {
        let pack = EMBEDDED
            .replacen("\"rate\"", "\"citadel_player_glow_disabled\" \"true\"\n\t\t\"rate\"", 1)
            .replacen("IndexBufferPoolSizeMB 32", "IndexBufferPoolSizeMB 64", 1);
        let new_stock = EMBEDDED.replacen("\"rate\"", "\"brand_new_cvar\" \"1\"\n\t\t\"rate\"", 1);
        let r = build(&pack, &new_stock, Some("1".into()), Some("2".into()));
        assert_eq!(r.new_settings, vec!["brand_new_cvar".to_string()]);
        assert_eq!(r.ignored, vec!["citadel_player_glow_disabled".to_string()]);
        assert_eq!(r.engine_changes, 1);
        assert_eq!(build(EMBEDDED, EMBEDDED, None, None), UpdateReport::default());
    }
}
