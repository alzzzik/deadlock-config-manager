//! What the player changed on top of the applied config: ConVars lines in the installed gameinfo.gi that
//! differ from the config's own file, whether from the "What you see in game" switches or edited by hand.
//! Lines Valve's file adds (overlay mode) and the personal settings rows don't count.

use crate::kv;
use std::collections::BTreeMap;

/// The settings rows (view width, FPS caps) go on top of every config, so they're the player's own, not a
/// change to this config.
const PERSONAL: [&str; 3] = ["r_aspectratio", "fps_max", "fps_max_ui"];

/// ConVars as name (lower case) → value; nested blocks are skipped.
fn convars(src: &str) -> BTreeMap<String, String> {
    let Ok(nodes) = kv::parse(src) else { return BTreeMap::new() };
    let Some(block) = kv::get(&nodes, "GameInfo").and_then(|gi| kv::get(gi.children(), "ConVars")) else { return BTreeMap::new() };
    block.children().iter().filter_map(|n| Some((n.key.to_ascii_lowercase(), n.as_str()?.to_string()))).collect()
}

/// The changed lines as "name=value" ("name=off" when the config's line is gone), sorted. Empty when the
/// installed file sets the same as the config.
pub fn changes(installed: &str, config: &str, stock: &str) -> Vec<String> {
    let (now, own, valve) = (convars(&crate::gameinfo::strip_dmm_region(installed)), convars(config), convars(stock));
    let edited = own.iter().filter(|(k, v)| now.get(*k) != Some(*v)).map(|(k, _)| format!("{k}={}", now.get(k).map_or("off", String::as_str)));
    let added = now.iter().filter(|(k, v)| !own.contains_key(*k) && valve.get(*k) != Some(*v)).map(|(k, v)| format!("{k}={v}"));
    let mut all: Vec<String> = edited.chain(added).filter(|c| !PERSONAL.iter().any(|p| c.split('=').next() == Some(*p))).collect();
    all.sort();
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gi(convars: &str) -> String {
        format!("\"GameInfo\"\n{{\n\tConVars\n\t{{\n{convars}\t}}\n}}\n")
    }

    #[test]
    fn finds_changes_from_settings_or_by_hand() {
        let config = gi("\t\tr_farz \"4000\"\n\t\tcl_ragdoll_limit \"0\"\n");
        let stock = gi("\t\tfps_max \"400\"\n\t\tr_new_valve_line \"1\"\n");
        // Same as the config, plus a line Valve's file has (overlay): nothing changed.
        assert!(changes(&gi("\t\tr_farz \"4000\"\n\t\tcl_ragdoll_limit \"0\"\n\t\tr_new_valve_line \"1\"\n"), &config, &stock).is_empty());
        // A switch, a commented-out line and a hand-added line; the FPS cap is a personal setting.
        let edited = gi("\t\t// r_farz \"4000\"\n\t\tcl_ragdoll_limit \"-1\"\n\t\tfps_max \"240\"\n\t\tr_hand_added \"1\"\n");
        assert_eq!(changes(&edited, &config, &stock), vec!["cl_ragdoll_limit=-1", "r_farz=off", "r_hand_added=1"]);
    }
}
