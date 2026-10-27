//! Personal settings applied on top of any config: view width, FPS caps and FAQ fixes.
//! They only touch the ConVars block, so they never affect ranked safety.

use crate::kv::{self, Value};
use serde::{Deserialize, Serialize};

/// One override: set `name` to `value`, or comment the convar out when `value` is None.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub value: Option<String>,
}

impl Entry {
    fn set(name: &str, value: &str) -> Self {
        Self { name: name.into(), value: Some(value.into()) }
    }
    fn off(name: &str) -> Self {
        Self { name: name.into(), value: None }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Overrides {
    /// Ids from [`FIXES`] switched on (shown) where the config hides them.
    pub fixes: Vec<String>,
    /// Ids from [`FIXES`] switched off (hidden) where the config shows them; only fixes with `hide`.
    pub hidden: Vec<String>,
    /// View width through `r_aspectratio`. None keeps the config's value.
    pub aspect: Option<String>,
    /// Frame rate cap in matches (`fps_max`, 0 = no cap). None keeps the config's value.
    pub fps_max: Option<u32>,
    /// Frame rate cap in menus (`fps_max_ui`). None keeps the config's value.
    pub fps_max_ui: Option<u32>,
}

pub struct Fix {
    pub id: &'static str,
    pub label: &'static str,
    /// Lines that make it show.
    pub entries: fn() -> Vec<Entry>,
    /// Lines that hide it, for things that are plainly on or off. None for distance and detail settings,
    /// where "off" means the config's own value.
    pub hide: Option<fn() -> Vec<Entry>>,
    /// Whether a gameinfo.gi shows it.
    pub shown: fn(&str) -> bool,
}

fn is_true(v: Option<String>) -> bool {
    v.is_some_and(|v| matches!(v.trim(), "true" | "1"))
}

fn is_false(v: Option<String>) -> bool {
    v.is_some_and(|v| matches!(v.trim(), "false" | "0"))
}

fn number(v: Option<String>) -> Option<f64> {
    v.and_then(|v| v.trim().parse().ok())
}

/// Things aggressive configs hide (fixes from the OptimizationLock FAQ), as switches. Share codes store
/// them by position, so new ones go at the end.
pub const FIXES: [Fix; 8] = [
    Fix {
        id: "portraits_dark",
        label: "Hero lighting in the shop and end screen",
        entries: || vec![Entry::set("lb_enable_dynamic_lights", "true")],
        hide: Some(|| vec![Entry::set("lb_enable_dynamic_lights", "false")]),
        shown: |src| !is_false(convar_value(src, "lb_enable_dynamic_lights")),
    },
    Fix {
        id: "portraits_missing",
        label: "Hero models in the shop and end screen",
        entries: || vec![Entry::off("citadel_portrait_world_renderer_off")],
        hide: Some(|| vec![Entry::set("citadel_portrait_world_renderer_off", "true")]),
        shown: |src| !is_true(convar_value(src, "citadel_portrait_world_renderer_off")),
    },
    Fix {
        id: "doorman_ult",
        label: "Doorman's ult indicator",
        entries: || vec![Entry::set("cl_ragdoll_limit", "-1")],
        hide: Some(|| vec![Entry::set("cl_ragdoll_limit", "0")]),
        shown: |src| number(convar_value(src, "cl_ragdoll_limit")) != Some(0.0),
    },
    Fix {
        id: "far_boxes",
        label: "Boxes and health bars at a distance",
        entries: || vec![Entry::set("r_size_cull_threshold", "0.7")],
        hide: None,
        shown: |src| number(convar_value(src, "r_size_cull_threshold")).is_none_or(|v| v <= 0.7),
    },
    Fix {
        id: "buildings_pop",
        label: "Distant buildings (no popping in and out)",
        entries: || vec![Entry::off("r_farz"), Entry::off("r_mapextents")],
        hide: None,
        shown: |src| {
            number(convar_value(src, "r_farz")).is_none_or(|v| v <= 0.0) && number(convar_value(src, "r_mapextents")).is_none_or(|v| v >= 16384.0)
        },
    },
    Fix {
        id: "lod_holes",
        label: "Full-detail models (no holes in Victor and Paige, round lights on Sinner)",
        entries: || vec![Entry::off("sc_screen_size_lod_scale_override")],
        hide: None,
        shown: |src| convar_value(src, "sc_screen_size_lod_scale_override").is_none(),
    },
    Fix {
        id: "decals",
        label: "Ground marks, like Lash's ground slam",
        entries: || vec![Entry::off("r_drawdecals")],
        hide: Some(|| vec![Entry::set("r_drawdecals", "0")]),
        shown: |src| !is_false(convar_value(src, "r_drawdecals")),
    },
    Fix {
        id: "far_effects",
        label: "Blast vent wind and in-world text at range",
        entries: || vec![Entry::off("sc_fade_distance_scale_override"), Entry::off("citadel_in_world_item_panel_dpi")],
        hide: None,
        shown: |src| convar_value(src, "sc_fade_distance_scale_override").is_none() && convar_value(src, "citadel_in_world_item_panel_dpi").is_none(),
    },
];

/// The overrides after flipping switch `fix` to `on`, for a config that shows it or not (`config_shows`).
/// Only differences from the config are kept, so another config later starts from its own values.
pub fn set_switch(o: &Overrides, fix: &Fix, on: bool, config_shows: bool) -> Overrides {
    let without = |ids: &[String]| ids.iter().filter(|x| *x != fix.id).cloned().collect::<Vec<_>>();
    let (mut fixes, mut hidden) = (without(&o.fixes), without(&o.hidden));
    if on && !config_shows {
        fixes.push(fix.id.into());
    } else if !on && config_shows && fix.hide.is_some() {
        hidden.push(fix.id.into());
    }
    Overrides { fixes, hidden, ..o.clone() }
}

const MAX_ASPECT: f64 = 4.0;
pub const MAX_FPS: u32 = 1000;

/// Accepts a number from 0 to [`MAX_ASPECT`] ("0" is the game's own view), written with a dot or comma.
pub fn parse_aspect(v: &str) -> Result<String, String> {
    let t = v.trim().replace(',', ".");
    match t.parse::<f64>() {
        Ok(x) if (0.0..=MAX_ASPECT).contains(&x) => Ok(t),
        _ => Err(format!("View width must be a number from 0 to {MAX_ASPECT}, e.g. 2.15.")),
    }
}

impl Overrides {
    pub fn is_empty(&self) -> bool {
        self.effective().is_empty()
    }

    /// All entries in apply order: fixes, then view width and FPS caps.
    pub fn effective(&self) -> Vec<Entry> {
        let has = |ids: &[String], f: &Fix| ids.iter().any(|id| id == f.id);
        let shown = FIXES.iter().filter(|f| has(&self.fixes, f)).flat_map(|f| (f.entries)());
        let hidden = FIXES.iter().filter(|f| has(&self.hidden, f)).filter_map(|f| f.hide).flat_map(|hide| hide());
        let fixes = shown.chain(hidden);
        let aspect = self.aspect.as_deref().and_then(|a| parse_aspect(a).ok()).map(|a| Entry::set("r_aspectratio", &a));
        let fps = self.fps_max.map(|v| Entry::set("fps_max", &v.min(MAX_FPS).to_string()));
        let fps_ui = self.fps_max_ui.map(|v| Entry::set("fps_max_ui", &v.min(MAX_FPS).to_string()));
        fixes.chain(aspect).chain(fps).chain(fps_ui).collect()
    }
}

/// The value a gameinfo.gi sets for a convar (None when it's not set or commented out).
pub fn convar_value(src: &str, name: &str) -> Option<String> {
    let nodes = kv::parse(src).ok()?;
    let gi = kv::get(&nodes, "GameInfo")?;
    kv::get(kv::get(gi.children(), "ConVars")?.children(), name)?.as_str().map(str::to_string)
}

fn is_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Value text safe to write inside quotes.
fn is_value(s: &str) -> bool {
    !s.is_empty() && !s.contains(['"', '\n', '\r', '{', '}'])
}

/// Checks setting names and values before they're written.
pub fn validate(entries: &[Entry]) -> Result<(), String> {
    for e in entries {
        if !is_name(&e.name) {
            return Err(format!("\"{}\" isn't a valid setting name.", e.name));
        }
        if e.value.as_deref().is_some_and(|v| !is_value(v)) {
            return Err(format!("The value for {} can't contain quotes, braces or line breaks.", e.name));
        }
    }
    Ok(())
}

const BLOCK_START: &str = "// Deadlock Config Manager overrides - start";
const BLOCK_END: &str = "// Deadlock Config Manager overrides - end";

/// Markers around a block of added lines, so re-applying replaces it instead of stacking copies.
pub struct Markers {
    pub start: &'static str,
    pub end: &'static str,
}

const TWEAK_MARKERS: Markers = Markers { start: BLOCK_START, end: BLOCK_END };

/// Splits a ConVars body line into (indent, commented, name, rest-after-name).
fn split_line(line: &str) -> Option<(&str, bool, &str, &str)> {
    let indent_len = line.len() - line.trim_start().len();
    let (indent, body) = line.split_at(indent_len);
    let (commented, body) = match body.strip_prefix("//") {
        Some(b) => (true, b.trim_start()),
        None => (false, body),
    };
    let (name, rest) = match body.strip_prefix('"') {
        Some(b) => {
            let end = b.find('"')?;
            (&b[..end], &b[end + 1..])
        }
        None => {
            let end = body.find(|c: char| c.is_whitespace()).unwrap_or(body.len());
            (&body[..end], &body[end..])
        }
    };
    is_name(name).then_some((indent, commented, name, rest))
}

/// Keeps a trailing `// comment` from the original value part.
fn trailing_comment(rest: &str) -> &str {
    let mut in_quotes = false;
    let bytes = rest.as_bytes();
    for i in 0..bytes.len() {
        match bytes[i] {
            b'"' => in_quotes = !in_quotes,
            b'/' if !in_quotes && bytes.get(i + 1) == Some(&b'/') => return &rest[i..],
            _ => {}
        }
    }
    ""
}

pub fn strip_managed_block(src: &str, m: &Markers) -> String {
    let (Some(s), Some(e)) = (src.find(m.start), src.find(m.end)) else { return src.to_string() };
    if e < s {
        return src.to_string();
    }
    let start = src[..s].rfind('\n').map_or(0, |i| i + 1);
    let end = src[e..].find('\n').map_or(src.len(), |i| e + i + 1);
    format!("{}{}", &src[..start], &src[end..])
}

/// Applies the entries to the ConVars block of `src`. Matching lines are edited in place (and
/// uncommented when set); entries with no line are added in a marked block at the top of ConVars.
pub fn apply(src: &str, entries: &[Entry]) -> Result<String, String> {
    apply_marked(src, entries, &TWEAK_MARKERS)
}

/// Names set in the ConVars block, and names that are there but commented out.
pub fn convar_names(src: &str) -> (Vec<String>, Vec<String>) {
    let Ok(nodes) = kv::parse_document(src) else { return (vec![], vec![]) };
    let Some(Value::Block { open, close, .. }) = kv::get(&nodes, "GameInfo").and_then(|gi| kv::get(gi.children(), "ConVars")).map(|c| c.value.clone()) else {
        return (vec![], vec![]);
    };
    let (mut set, mut commented) = (Vec::new(), Vec::new());
    let mut depth = 0i32;
    for line in src[open + 1..close].lines() {
        let code = line.split("//").next().unwrap_or("");
        let at_top = depth == 0;
        depth += code.matches('{').count() as i32 - code.matches('}').count() as i32;
        if let Some((_, is_commented, name, _)) = at_top.then(|| split_line(line)).flatten() {
            if is_commented { commented.push(name.to_ascii_lowercase()) } else { set.push(name.to_ascii_lowercase()) }
        }
    }
    (set, commented)
}

/// [`apply`] with its own block markers.
pub fn apply_marked(src: &str, entries: &[Entry], markers: &Markers) -> Result<String, String> {
    validate(entries)?;
    let src = strip_managed_block(src, markers);
    if entries.is_empty() {
        return Ok(src);
    }
    let nodes = kv::parse_document(&src).map_err(|e| format!("gameinfo.gi {e}"))?;
    let gi = kv::get(&nodes, "GameInfo").ok_or("gameinfo.gi has no GameInfo block")?;
    let nl = if src.contains("\r\n") { "\r\n" } else { "\n" };
    let Some(convars) = kv::get(gi.children(), "ConVars") else {
        let Value::Block { close, .. } = gi.value else { return Err("GameInfo is not a block".into()) };
        let block = format!("\tConVars{nl}\t{{{nl}\t}}{nl}");
        return apply_marked(&format!("{}{block}{}", &src[..close], &src[close..]), entries, markers);
    };
    let Value::Block { open, close, .. } = convars.value else { return Err("ConVars is not a block".into()) };

    let body_start = src[open..].find('\n').map_or(open + 1, |i| open + i + 1);
    let body = &src[body_start.min(close)..close];
    let mut matched = vec![false; entries.len()];
    let mut depth = 0i32;
    let mut out = String::new();
    for line in body.split_inclusive('\n') {
        let code = line.split("//").next().unwrap_or("");
        let at_top = depth == 0;
        depth += code.matches('{').count() as i32 - code.matches('}').count() as i32;
        let parsed = if at_top { split_line(line.trim_end_matches(['\r', '\n'])) } else { None };
        let Some((indent, commented, name, rest)) = parsed else {
            out.push_str(line);
            continue;
        };
        let Some(idx) = entries.iter().position(|e| e.name.eq_ignore_ascii_case(name)) else {
            out.push_str(line);
            continue;
        };
        matched[idx] = true;
        let eol = if line.ends_with("\r\n") { "\r\n" } else if line.ends_with('\n') { "\n" } else { "" };
        match &entries[idx].value {
            Some(v) => {
                let note = if commented { "" } else { trailing_comment(rest) };
                let sep = if note.is_empty() { "" } else { " " };
                out.push_str(&format!("{indent}\"{name}\" \"{v}\"{sep}{note}{eol}"));
            }
            None if commented => out.push_str(line),
            None => out.push_str(&format!("{indent}// {}{eol}", line.trim_start())),
        }
    }
    let added: String = entries
        .iter()
        .zip(&matched)
        .filter(|(e, m)| !**m && e.value.is_some())
        .map(|(e, _)| format!("\t\t\"{}\" \"{}\"{nl}", e.name, e.value.as_deref().unwrap_or_default()))
        .collect();
    let managed = if added.is_empty() { String::new() } else { format!("\t\t{}{nl}{added}\t\t{}{nl}", markers.start, markers.end) };
    Ok(format!("{}{managed}{out}{}", &src[..body_start.min(close)], &src[close..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GI: &str = "\"GameInfo\"\n{\n\tFileSystem\n\t{\n\t\tSearchPaths\n\t\t{\n\t\t\tGame citadel\n\t\t}\n\t}\n\tConVars\n\t{\n\t\t\"r_farz\" \"4000\" // far clip\n\t\tcl_ragdoll_limit 0\n\t\t// citadel_camera_hero_fov 100\n\t\tnested\n\t\t{\n\t\t\tr_farz 1\n\t\t}\n\t}\n}\n";

    fn values(src: &str) -> Vec<(String, String)> {
        crate::diff::flatten(&kv::parse(src).unwrap())
    }

    #[test]
    fn sets_comments_out_and_adds_missing() {
        let out = apply(
            GI,
            &[Entry::off("r_farz"), Entry::set("cl_ragdoll_limit", "-1"), Entry::set("citadel_camera_hero_fov", "105"), Entry::set("r_new", "1")],
        )
        .unwrap();
        assert!(out.contains("\t\t// \"r_farz\" \"4000\" // far clip"));
        assert!(out.contains("\t\t\"cl_ragdoll_limit\" \"-1\""));
        // A commented-out line is uncommented when set.
        assert!(out.contains("\t\t\"citadel_camera_hero_fov\" \"105\""));
        assert!(out.contains(BLOCK_START) && out.contains("\"r_new\" \"1\""));
        // Nested blocks are left alone.
        assert!(out.contains("\t\t\tr_farz 1"));
        assert!(values(&out).iter().any(|(k, v)| k.ends_with("r_new") && v == "1"));
    }

    #[test]
    fn reapplying_replaces_the_managed_block() {
        let once = apply(GI, &[Entry::set("r_new", "1")]).unwrap();
        let twice = apply(&once, &[Entry::set("r_new", "2")]).unwrap();
        assert_eq!(twice.matches(BLOCK_START).count(), 1);
        assert!(twice.contains("\"r_new\" \"2\"") && !twice.contains("\"r_new\" \"1\""));
        assert_eq!(apply(&once, &[]).unwrap(), GI);
    }

    #[test]
    fn adds_a_convars_block_when_missing() {
        let gi = "\"GameInfo\"\n{\n\tgame citadel\n}\n";
        let out = apply(gi, &[Entry::set("fps_max", "300")]).unwrap();
        assert!(values(&out).iter().any(|(k, v)| k.ends_with("fps_max") && v == "300"));
    }

    #[test]
    fn effective_merges_fixes_view_and_fps() {
        let o = Overrides { fixes: vec!["doorman_ult".into(), "buildings_pop".into()], hidden: vec!["decals".into()], aspect: Some("2,15".into()), fps_max: Some(5000), fps_max_ui: Some(60) };
        let e = o.effective();
        assert!(e.contains(&Entry::set("r_aspectratio", "2.15")));
        assert!(e.contains(&Entry::set("fps_max", "1000")));
        assert!(e.contains(&Entry::set("fps_max_ui", "60")));
        assert!(e.contains(&Entry::set("cl_ragdoll_limit", "-1")));
        assert!(e.contains(&Entry::off("r_mapextents")));
        assert!(e.contains(&Entry::set("r_drawdecals", "0")));
        assert!(Overrides::default().is_empty());
        assert!(Overrides { aspect: Some("wide".into()), ..Default::default() }.is_empty());
        assert!(parse_aspect("9").is_err() && parse_aspect("0").is_ok());
    }

    fn fix(id: &str) -> &'static Fix {
        FIXES.iter().find(|f| f.id == id).unwrap()
    }

    #[test]
    fn switches_read_what_a_config_shows() {
        // GI: r_farz 4000 hides distant buildings, cl_ragdoll_limit 0 hides Doorman's ult, the rest is unset.
        assert!(!(fix("buildings_pop").shown)(GI));
        assert!(!(fix("doorman_ult").shown)(GI));
        assert!((fix("portraits_missing").shown)(GI) && (fix("decals").shown)(GI) && (fix("far_boxes").shown)(GI));
        let hidden = apply(GI, &(fix("portraits_missing").hide.unwrap())()).unwrap();
        assert!(!(fix("portraits_missing").shown)(&hidden));
        let shown = apply(GI, &(fix("doorman_ult").entries)()).unwrap();
        assert!((fix("doorman_ult").shown)(&shown));
    }

    #[test]
    fn switches_keep_only_differences_from_the_config() {
        let none = Overrides::default();
        let portraits = fix("portraits_missing");
        // Config shows the models: turning them off is stored, turning them back on clears it.
        let off = set_switch(&none, portraits, false, true);
        assert_eq!(off.hidden, vec!["portraits_missing".to_string()]);
        assert_eq!(set_switch(&off, portraits, true, true), none);
        // Config hides them: turning them on is stored.
        assert_eq!(set_switch(&none, portraits, true, false).fixes, vec!["portraits_missing".to_string()]);
        // Distance settings have no "hide" value: off keeps the config's own.
        assert_eq!(set_switch(&none, fix("far_boxes"), false, true), none);
    }

    #[test]
    fn reads_a_convar_value() {
        assert_eq!(convar_value(GI, "cl_ragdoll_limit").as_deref(), Some("0"));
        assert_eq!(convar_value(GI, "citadel_camera_hero_fov"), None);
    }
}

