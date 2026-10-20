//! video.txt merge: pack values win, except hardware/display keys kept from the user's file.

use crate::kv;

pub const ALWAYS_PRESERVED: [&str; 3] = ["Version", "VendorID", "DeviceID"];

pub const DEFAULT_PRESERVED: [&str; 9] = [
    "setting.defaultres",
    "setting.defaultresheight",
    "setting.refreshrate_numerator",
    "setting.refreshrate_denominator",
    "setting.monitor_index",
    "setting.fullscreen",
    "setting.coop_fullscreen",
    "setting.nowindowborder",
    "setting.aspectratiomode",
];

/// Key/value pairs of the root block, in file order.
pub fn read_values(src: &str) -> Result<Vec<(String, String)>, kv::ParseError> {
    let nodes = kv::parse_document(src)?;
    Ok(nodes[0]
        .children()
        .iter()
        .filter_map(|n| n.as_str().map(|v| (n.key.clone(), v.to_string())))
        .collect())
}

pub fn is_placeholder(value: &str) -> bool {
    let t = value.trim();
    (t.starts_with('[') && t.ends_with(']')) || t.to_ascii_uppercase().contains("YOUR ")
}

/// A `"key" "value" rest` line split into its parts.
struct KvLine<'a> {
    indent: &'a str,
    key: &'a str,
    value: &'a str,
    rest: &'a str,
}

fn split_kv_line(line: &str) -> Option<KvLine<'_>> {
    let body = line.trim_start_matches([' ', '\t']);
    let indent = &line[..line.len() - body.len()];
    let after_key_quote = body.strip_prefix('"')?;
    let key_end = after_key_quote.find('"')?;
    let key = &after_key_quote[..key_end];
    let after_key = after_key_quote[key_end + 1..].trim_start_matches([' ', '\t']);
    let after_value_quote = after_key.strip_prefix('"')?;
    let value_end = after_value_quote.find('"')?;
    Some(KvLine {
        indent,
        key,
        value: &after_value_quote[..value_end],
        rest: &after_value_quote[value_end + 1..],
    })
}

fn format_line(indent: &str, key: &str, value: &str, rest: &str) -> String {
    format!("{indent}\"{key}\"\t\t\"{value}\"{rest}")
}

fn lookup<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
}

/// Full set of preserved keys: the always-kept ones plus the user's chosen extras.
pub fn preserved_set(extra: &[String]) -> Vec<String> {
    let mut keys: Vec<String> = ALWAYS_PRESERVED.iter().map(|s| s.to_string()).collect();
    for k in extra {
        if !keys.iter().any(|e| e.eq_ignore_ascii_case(k)) {
            keys.push(k.clone());
        }
    }
    keys
}

/// The preserved (key, value) pairs from the user's file that the merged output must contain.
pub fn expected_preserved(user: &str, extra: &[String]) -> Result<Vec<(String, String)>, String> {
    let values = read_values(user).map_err(|e| format!("your video.txt {e}"))?;
    Ok(preserved_set(extra)
        .into_iter()
        .filter_map(|k| lookup(&values, &k).map(|v| (k.clone(), v.to_string())))
        .collect())
}

/// Merges the pack's video.txt with the user's. Output uses CRLF line endings.
/// Drops a trailing lone `}`: fragment packs often end with the closing brace of the file
/// they're pasted into ("delete everything under DeviceID and paste this").
fn fragment_text(pack: &str) -> String {
    let lines: Vec<&str> = pack.lines().collect();
    let last = lines.iter().rposition(|l| !l.trim().is_empty());
    match last {
        Some(i) if lines[i].trim() == "}" => lines[..i].join("\n"),
        _ => pack.to_string(),
    }
}

/// A pack video.txt that is only `"key" "value"` lines (no `"video.cfg" { }` wrapper),
/// meant to be pasted into your own file under DeviceID.
pub fn is_fragment(pack: &str) -> bool {
    kv::parse(&fragment_text(pack)).is_ok_and(|nodes| !nodes.is_empty() && nodes.iter().all(|n| !n.is_block()))
}

/// Checks a pack video.txt is usable: a full file or a fragment.
pub fn validate_pack(pack: &str) -> Result<(), String> {
    if is_fragment(pack) {
        return Ok(());
    }
    kv::parse_document(pack).map(|_| ()).map_err(|e| format!("pack video.txt {e}"))
}

/// Fragment pairs, deduplicated (last value wins), in first-seen order.
fn fragment_values(pack: &str) -> Vec<(String, String)> {
    let nodes = kv::parse(&fragment_text(pack)).unwrap_or_default();
    let mut out: Vec<(String, String)> = Vec::new();
    for n in nodes {
        let Some(v) = n.as_str() else { continue };
        match out.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(&n.key)) {
            Some(existing) => existing.1 = v.to_string(),
            None => out.push((n.key.clone(), v.to_string())),
        }
    }
    out
}

/// Applies a fragment pack the way such packs are meant to be used: keep your file's header and
/// preserved lines (Version, VendorID, DeviceID, resolution…), then the pack's settings replace
/// everything else.
fn merge_fragment(pack: &str, user: &str, extra_preserved: &[String]) -> Result<String, String> {
    let user_values = read_values(user).map_err(|e| format!("your video.txt {e}"))?;
    let preserved = preserved_set(extra_preserved);
    let is_preserved = |k: &str| preserved.iter().any(|p| p.eq_ignore_ascii_case(k));

    let kept: Vec<String> = user_values
        .iter()
        .filter(|(k, _)| is_preserved(k))
        .map(|(k, v)| format_line("\t", k, v, ""))
        .collect();
    let settings: Vec<String> = fragment_values(pack)
        .into_iter()
        .filter(|(k, v)| !is_preserved(k) && !is_placeholder(v))
        .map(|(k, v)| format_line("\t", &k, &v, ""))
        .collect();
    let root = user.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("//")).unwrap_or("\"video.cfg\"");
    let body: String = kept.iter().chain(&settings).map(|l| format!("{l}\r\n")).collect();
    let text = format!("{root}\r\n{{\r\n{body}}}\r\n");
    kv::parse_document(&text).map_err(|e| format!("merged video.txt {e}"))?;
    Ok(text)
}

pub fn merge(pack: &str, user: &str, extra_preserved: &[String]) -> Result<String, String> {
    if is_fragment(pack) {
        return merge_fragment(pack, user, extra_preserved);
    }
    kv::parse_document(pack).map_err(|e| format!("pack video.txt {e}"))?;
    let user_values = read_values(user).map_err(|e| format!("your video.txt {e}"))?;
    let preserved = preserved_set(extra_preserved);
    let is_preserved = |k: &str| preserved.iter().any(|p| p.eq_ignore_ascii_case(k));

    let mut out: Vec<String> = Vec::new();
    let mut pack_keys: Vec<String> = Vec::new();
    let mut device_line: Option<(usize, String)> = None;

    for line in pack.lines() {
        let Some(kvl) = split_kv_line(line) else {
            out.push(line.to_string());
            continue;
        };
        pack_keys.push(kvl.key.to_string());
        let user_value = if is_preserved(kvl.key) { lookup(&user_values, kvl.key) } else { None };
        let new_line = match user_value {
            // A placeholder's trailing comment ("copy from your own video.txt") no longer applies.
            Some(v) if is_placeholder(kvl.value) => format_line(kvl.indent, kvl.key, v, ""),
            Some(v) => format_line(kvl.indent, kvl.key, v, kvl.rest),
            None if is_placeholder(kvl.value) => {
                return Err(format!(
                    "The pack's video.txt has a placeholder for \"{}\" and your video.txt has no \"{}\" to fill it with. \
                     Launch Deadlock once so the game writes a complete video.txt, then try again.",
                    kvl.key, kvl.key
                ));
            }
            None => line.to_string(),
        };
        if kvl.key.eq_ignore_ascii_case("DeviceID") {
            device_line = Some((out.len(), kvl.indent.to_string()));
        }
        out.push(new_line);
    }

    let indent = device_line.as_ref().map_or("\t", |(_, ind)| ind.as_str());
    let missing: Vec<String> = user_values
        .iter()
        .filter(|(k, _)| is_preserved(k) && !pack_keys.iter().any(|p| p.eq_ignore_ascii_case(k)))
        .map(|(k, v)| format_line(indent, k, v, ""))
        .collect();
    let insert_at = match &device_line {
        Some((idx, _)) => idx + 1,
        None => out.iter().position(|l| l.trim() == "{").map_or(out.len(), |i| i + 1),
    };
    let merged: Vec<String> = out[..insert_at]
        .iter()
        .cloned()
        .chain(missing)
        .chain(out[insert_at..].iter().cloned())
        .collect();

    let text = format!("{}\r\n", merged.join("\r\n").trim_end_matches(['\r', '\n']));
    kv::parse_document(&text).map_err(|e| format!("merged video.txt {e}"))?;
    Ok(text)
}

/// Hardware lines a pack can't know; the game writes them on first launch.
const HARDWARE_KEYS: [&str; 2] = ["VendorID", "DeviceID"];

/// A fresh video.txt for an account that has none: the pack's file without hardware or
/// placeholder lines. Output uses CRLF line endings.
pub fn new_from_pack(pack: &str) -> Result<String, String> {
    if is_fragment(pack) {
        let lines: String = fragment_values(pack)
            .iter()
            .filter(|(k, v)| !is_placeholder(v) && !HARDWARE_KEYS.iter().any(|h| h.eq_ignore_ascii_case(k)))
            .map(|(k, v)| format!("{}\r\n", format_line("\t", k, v, "")))
            .collect();
        let text = format!("\"video.cfg\"\r\n{{\r\n{lines}}}\r\n");
        kv::parse_document(&text).map_err(|e| format!("new video.txt {e}"))?;
        return Ok(text);
    }
    kv::parse_document(pack).map_err(|e| format!("pack video.txt {e}"))?;
    let kept: Vec<&str> = pack
        .lines()
        .filter(|line| match split_kv_line(line) {
            Some(kvl) => !is_placeholder(kvl.value) && !HARDWARE_KEYS.iter().any(|h| h.eq_ignore_ascii_case(kvl.key)),
            None => true,
        })
        .collect();
    let text = format!("{}\r\n", kept.join("\r\n").trim_end_matches(['\r', '\n']));
    kv::parse_document(&text).map_err(|e| format!("new video.txt {e}"))?;
    Ok(text)
}

/// True if any value in the file is a pack placeholder like `[ YOUR VENDOR ID ]`.
pub fn has_placeholder(src: &str) -> bool {
    read_values(src).map(|v| v.iter().any(|(_, val)| is_placeholder(val))).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACK: &str = include_str!("../fixtures/pack_stable/video.txt");
    const USER: &str = include_str!("../fixtures/user_video.txt");

    fn defaults() -> Vec<String> {
        DEFAULT_PRESERVED.iter().map(|s| s.to_string()).collect()
    }

    fn value(src: &str, key: &str) -> Option<String> {
        lookup(&read_values(src).unwrap(), key).map(str::to_string)
    }

    #[test]
    fn keeps_hardware_and_resolution_from_user_and_rest_from_pack() {
        let merged = merge(PACK, USER, &defaults()).unwrap();
        assert_eq!(value(&merged, "Version").as_deref(), Some("20"));
        assert_eq!(value(&merged, "VendorID").as_deref(), Some("4318"));
        assert_eq!(value(&merged, "DeviceID").as_deref(), Some("12036"));
        assert_eq!(value(&merged, "setting.defaultres").as_deref(), Some("1920"));
        assert_eq!(value(&merged, "setting.defaultresheight").as_deref(), Some("1080"));
        assert_eq!(value(&merged, "setting.fullscreen").as_deref(), Some("1"));
        // Pack wins for everything else.
        assert_eq!(value(&merged, "setting.mat_viewportscale").as_deref(), Some("0.58"));
        assert_eq!(value(&merged, "setting.r_citadel_shadow_quality").as_deref(), Some("0"));
        // User-only, non-preserved keys are dropped.
        assert_eq!(value(&merged, "setting.user_only_key"), None);
        assert!(!merged.contains("YOUR"));
    }

    #[test]
    fn inserts_missing_preserved_keys_after_device_id() {
        // The stable pack has every default preserved key, so use an extra one only the user has.
        let extra = [defaults(), vec!["setting.custom_display".to_string()]].concat();
        let merged = merge(PACK, USER, &extra).unwrap();
        let lines: Vec<&str> = merged.lines().collect();
        let dev = lines.iter().position(|l| l.contains("\"DeviceID\"")).unwrap();
        assert_eq!(lines[dev + 1], "\t\"setting.custom_display\"\t\t\"7\"");
        assert_eq!(merged.matches("setting.custom_display").count(), 1);
    }

    #[test]
    fn output_is_crlf_with_tabs() {
        let merged = merge(PACK, USER, &defaults()).unwrap();
        assert!(merged.ends_with("}\r\n"));
        assert_eq!(merged.matches('\n').count(), merged.matches("\r\n").count());
        assert!(merged.contains("\t\"VendorID\"\t\t\"4318\"\r\n"));
    }

    #[test]
    fn unticked_keys_come_from_pack() {
        let merged = merge(PACK, USER, &[]).unwrap();
        assert_eq!(value(&merged, "setting.defaultres").as_deref(), Some("2560"));
        assert_eq!(value(&merged, "DeviceID").as_deref(), Some("12036"));
    }

    #[test]
    fn refuses_placeholder_without_user_value() {
        let user = "\"video.cfg\"\r\n{\r\n\t\"Version\"\t\t\"20\"\r\n}\r\n";
        let err = merge(PACK, user, &defaults()).unwrap_err();
        assert!(err.contains("VendorID"), "{err}");
    }

    // Ends with the closing brace of the file it's pasted into, like OptiLock's packs.
    const FRAGMENT: &str = "\t\"setting.defaultres\"\t\t\"-1\"\n\t\"setting.mat_viewportscale\"\t\t\"0.5\"\n\t\"setting.r_render_portals\"\t\t\"2\"\n\t\"setting.r_render_portals\"\t\t\"3\"\n}\n\n";

    #[test]
    fn fragment_pack_merges_onto_users_file() {
        assert!(is_fragment(FRAGMENT));
        assert!(!is_fragment(PACK));
        let merged = merge(FRAGMENT, USER, &defaults()).unwrap();
        // Preserved key keeps the user's value; others take the fragment's.
        assert_eq!(value(&merged, "setting.defaultres").as_deref(), Some("1920"));
        assert_eq!(value(&merged, "setting.mat_viewportscale").as_deref(), Some("0.5"));
        // Keys the user doesn't have are added; duplicates in the fragment resolve to the last value.
        assert_eq!(value(&merged, "setting.r_render_portals").as_deref(), Some("3"));
        assert_eq!(merged.matches("r_render_portals").count(), 1);
        // Hardware lines stay; the user's other settings are replaced by the pack's ("delete everything under DeviceID").
        assert_eq!(value(&merged, "DeviceID").as_deref(), Some("12036"));
        assert_eq!(value(&merged, "Version").as_deref(), Some("20"));
        assert_eq!(value(&merged, "setting.user_only_key"), None);
        assert!(merged.starts_with("\"video.cfg\"\r\n{\r\n\t\"Version\""));
        assert!(merged.ends_with("}\r\n"));
    }

    #[test]
    fn fragment_pack_creates_wrapped_file() {
        let created = new_from_pack(FRAGMENT).unwrap();
        assert!(created.starts_with("\"video.cfg\"\r\n{\r\n"));
        assert_eq!(value(&created, "setting.r_render_portals").as_deref(), Some("3"));
        assert!(validate_pack(FRAGMENT).is_ok());
    }

    #[test]
    fn detects_placeholders() {
        assert!(is_placeholder("[ YOUR VENDOR ID ]"));
        assert!(!is_placeholder("4318"));
    }

    #[test]
    fn new_file_from_pack_drops_hardware_and_placeholders() {
        let created = new_from_pack(PACK).unwrap();
        assert!(!created.contains("VendorID") && !created.contains("DeviceID"));
        assert!(!has_placeholder(&created));
        assert!(has_placeholder(PACK));
        assert_eq!(value(&created, "setting.mat_viewportscale").as_deref(), Some("0.58"));
        assert_eq!(created.matches('\n').count(), created.matches("\r\n").count());
    }
}
