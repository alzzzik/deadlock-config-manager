//! Personal tweaks applied on top of any pack: common FAQ fixes, FOV and custom convar lines.
//! They only touch the ConVars block, so they never affect ranked safety. The file format for import
//! and export is the `overrides.gi` of Sqooky's OptimizationLock updater:
//! `name value` sets a convar, `// name` comments it out, `#` starts a comment.

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
    /// Ids from [`FIXES`] that are turned on.
    pub fixes: Vec<String>,
    /// Hero camera field of view (`citadel_camera_hero_fov`).
    pub fov: Option<u32>,
    pub custom: Vec<Entry>,
}

pub struct Fix {
    pub id: &'static str,
    pub label: &'static str,
    pub entries: fn() -> Vec<Entry>,
}

/// Fixes from the OptimizationLock FAQ for things aggressive configs hide.
pub const FIXES: [Fix; 8] = [
    Fix { id: "portraits_dark", label: "Heroes look dark in the shop and end screen", entries: || vec![Entry::set("lb_enable_dynamic_lights", "true")] },
    Fix { id: "portraits_missing", label: "Heroes missing in the shop and end screen", entries: || vec![Entry::off("citadel_portrait_world_renderer_off")] },
    Fix { id: "doorman_ult", label: "Can't see the Doorman ult indicator", entries: || vec![Entry::set("cl_ragdoll_limit", "-1")] },
    Fix { id: "far_boxes", label: "Boxes and health bars vanish at a distance", entries: || vec![Entry::set("r_size_cull_threshold", "0.7")] },
    Fix { id: "buildings_pop", label: "Buildings pop in and out", entries: || vec![Entry::off("r_farz"), Entry::off("r_mapextents")] },
    Fix { id: "lod_holes", label: "Holes in Victor and Paige, Sinner's lights are triangles", entries: || vec![Entry::off("sc_screen_size_lod_scale_override")] },
    Fix { id: "decals", label: "Can't see Lash's ground slam", entries: || vec![Entry::off("r_drawdecals")] },
    Fix { id: "far_effects", label: "Can't see blast vent wind or in-world text at range", entries: || vec![Entry::off("sc_fade_distance_scale_override"), Entry::off("citadel_in_world_item_panel_dpi")] },
];

pub const MIN_FOV: u32 = 60;
pub const MAX_FOV: u32 = 120;

impl Overrides {
    pub fn is_empty(&self) -> bool {
        self.effective().is_empty()
    }

    /// All entries in apply order: fixes, then FOV, then custom lines (later ones win).
    pub fn effective(&self) -> Vec<Entry> {
        let fixes = FIXES.iter().filter(|f| self.fixes.iter().any(|id| id == f.id)).flat_map(|f| (f.entries)());
        let fov = self.fov.map(|v| Entry::set("citadel_camera_hero_fov", &v.clamp(MIN_FOV, MAX_FOV).to_string()));
        let all: Vec<Entry> = fixes.chain(fov).chain(self.custom.iter().cloned()).collect();
        // Keep only the last entry per convar.
        all.iter()
            .enumerate()
            .filter(|(i, e)| !all[i + 1..].iter().any(|later| later.name.eq_ignore_ascii_case(&e.name)))
            .map(|(_, e)| e.clone())
            .collect()
    }
}

fn is_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Value text safe to write inside quotes.
fn is_value(s: &str) -> bool {
    !s.is_empty() && !s.contains(['"', '\n', '\r', '{', '}'])
}

/// Checks custom lines typed in the app or read from a file.
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

/// Reads an `overrides.gi` file (Sqooky's format).
pub fn parse_file(text: &str) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        let line = line.split(" #").next().unwrap_or(line).trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("//") {
            let name = rest.split_whitespace().next().unwrap_or("");
            if !is_name(name) {
                return Err(format!("Line {}: \"{raw}\" isn't a setting name.", i + 1));
            }
            out.push(Entry::off(name));
            continue;
        }
        let (name, value) = line.split_once(char::is_whitespace).ok_or_else(|| format!("Line {}: \"{raw}\" has no value.", i + 1))?;
        out.push(Entry::set(name.trim_matches('"'), value.trim().trim_matches('"')));
    }
    validate(&out)?;
    Ok(out)
}

pub fn to_file(entries: &[Entry]) -> String {
    let mut out = String::from("# Deadlock Config Manager overrides (same format as OptimizationLock's overrides.gi)\n");
    for e in entries {
        match &e.value {
            Some(v) => out.push_str(&format!("{} \"{}\"\n", e.name, v)),
            None => out.push_str(&format!("// {}\n", e.name)),
        }
    }
    out
}

const BLOCK_START: &str = "// Deadlock Config Manager overrides - start";
const BLOCK_END: &str = "// Deadlock Config Manager overrides - end";

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

fn strip_managed_block(src: &str) -> String {
    let (Some(s), Some(e)) = (src.find(BLOCK_START), src.find(BLOCK_END)) else { return src.to_string() };
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
    validate(entries)?;
    let src = strip_managed_block(src);
    if entries.is_empty() {
        return Ok(src);
    }
    let nodes = kv::parse_document(&src).map_err(|e| format!("gameinfo.gi {e}"))?;
    let gi = kv::get(&nodes, "GameInfo").ok_or("gameinfo.gi has no GameInfo block")?;
    let nl = if src.contains("\r\n") { "\r\n" } else { "\n" };
    let Some(convars) = kv::get(gi.children(), "ConVars") else {
        let Value::Block { close, .. } = gi.value else { return Err("GameInfo is not a block".into()) };
        let block = format!("\tConVars{nl}\t{{{nl}\t}}{nl}");
        return apply(&format!("{}{block}{}", &src[..close], &src[close..]), entries);
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
    let managed = if added.is_empty() { String::new() } else { format!("\t\t{BLOCK_START}{nl}{added}\t\t{BLOCK_END}{nl}") };
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
    fn effective_merges_fixes_fov_and_custom() {
        let o = Overrides {
            fixes: vec!["doorman_ult".into(), "buildings_pop".into()],
            fov: Some(200),
            custom: vec![Entry::set("cl_ragdoll_limit", "5")],
        };
        let e = o.effective();
        assert!(e.contains(&Entry::set("citadel_camera_hero_fov", "120")));
        assert!(e.contains(&Entry::set("cl_ragdoll_limit", "5")));
        assert!(!e.contains(&Entry::set("cl_ragdoll_limit", "-1")));
        assert!(e.contains(&Entry::off("r_mapextents")));
        assert!(Overrides::default().is_empty());
    }

    #[test]
    fn reads_and_writes_sqooky_format() {
        let text = "# comment\n// citadel_camera_hero_fov\nr_size_cull_threshold \"0.7\"\nr_aspectratio 2.4 # stretched\n";
        let e = parse_file(text).unwrap();
        assert_eq!(e, vec![Entry::off("citadel_camera_hero_fov"), Entry::set("r_size_cull_threshold", "0.7"), Entry::set("r_aspectratio", "2.4")]);
        assert_eq!(parse_file(&to_file(&e)).unwrap(), e);
        assert!(parse_file("bad-name 1").is_err());
        assert!(validate(&[Entry::set("x", "a\"b")]).is_err());
    }
}
