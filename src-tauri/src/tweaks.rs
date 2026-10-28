//! What the player changed on top of the applied config: ConVars lines in the installed gameinfo.gi that
//! differ from the config's own file, whether from Settings (switches, view width, FPS caps) or edited by
//! hand. Lines Valve's file adds (overlay mode) don't count.

use crate::kv;
use std::collections::BTreeMap;

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
    let mut all: Vec<String> = edited.chain(added).collect();
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
        // A switch, a commented-out line and a hand-added line.
        let edited = gi("\t\t// r_farz \"4000\"\n\t\tcl_ragdoll_limit \"-1\"\n\t\tfps_max \"240\"\n");
        assert_eq!(changes(&edited, &config, &stock), vec!["cl_ragdoll_limit=-1", "fps_max=240", "r_farz=off"]);
    }
}
