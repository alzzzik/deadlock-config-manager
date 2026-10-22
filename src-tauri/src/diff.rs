//! "What will change" preview: per-setting differences between the current files and the output.

use crate::kv::{self, Node, Value};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub key: String,
    /// None = the setting is new.
    pub before: Option<String>,
    /// None = the setting is removed.
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub file: String,
    /// The file doesn't exist yet and will be created.
    pub created: bool,
    pub changes: Vec<Change>,
    pub unchanged: usize,
}

pub const SEP: &str = " › ";

/// Flattens a KeyValues tree into (path, value) pairs. The root block name is dropped and
/// SearchPaths is skipped (the app always enforces it, and DMM rewrites it).
pub fn flatten(nodes: &[Node]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for root in nodes {
        match &root.value {
            Value::Block { children, .. } => walk(children, "", &mut out),
            Value::Str(v) => out.push((root.key.clone(), v.clone())),
        }
    }
    out
}

fn walk(nodes: &[Node], prefix: &str, out: &mut Vec<(String, String)>) {
    for n in nodes {
        if n.key.eq_ignore_ascii_case("SearchPaths") {
            continue;
        }
        let path = if prefix.is_empty() { n.key.clone() } else { format!("{prefix}{SEP}{}", n.key) };
        match &n.value {
            Value::Str(v) => out.push((path, v.clone())),
            Value::Block { children, .. } => walk(children, &path, out),
        }
    }
}

/// Differences between two flattened files, in the order of `after` then removed keys.
/// Keys compare case-insensitively; for repeated keys the last value wins.
pub fn diff_pairs(before: &[(String, String)], after: &[(String, String)]) -> (Vec<Change>, usize) {
    let index = |pairs: &[(String, String)]| -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.clone())).collect()
    };
    let (old, new) = (index(before), index(after));
    let mut seen = std::collections::HashSet::new();
    let mut changes = Vec::new();
    let mut unchanged = 0;
    for (k, _) in after {
        let lk = k.to_ascii_lowercase();
        if !seen.insert(lk.clone()) {
            continue;
        }
        let (b, a) = (old.get(&lk), new.get(&lk));
        if b == a {
            unchanged += 1;
        } else {
            changes.push(Change { key: k.clone(), before: b.cloned(), after: a.cloned() });
        }
    }
    for (k, v) in before {
        let lk = k.to_ascii_lowercase();
        if !new.contains_key(&lk) && seen.insert(lk) {
            changes.push(Change { key: k.clone(), before: Some(v.clone()), after: None });
        }
    }
    (changes, unchanged)
}

/// Diff of one file. `current` is None when the file doesn't exist yet.
pub fn diff_file(file: &str, current: Option<&str>, output: &str) -> FileDiff {
    let pairs = |src: &str| kv::parse(src).map(|n| flatten(&n)).unwrap_or_default();
    let before = current.map(pairs).unwrap_or_default();
    let (changes, unchanged) = diff_pairs(&before, &pairs(output));
    FileDiff { file: file.into(), created: current.is_none(), changes, unchanged }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattens_nested_blocks_and_skips_search_paths() {
        let src = "GameInfo\n{\n  game \"citadel\"\n  FileSystem\n  {\n    SearchPaths\n    {\n      Game \"core\"\n    }\n    UserSettingsPathID \"USRLOCAL\"\n  }\n  ConVars\n  {\n    r_fog \"0\"\n  }\n}\n";
        let flat = flatten(&kv::parse(src).unwrap());
        assert_eq!(
            flat,
            vec![
                ("game".to_string(), "citadel".to_string()),
                (format!("FileSystem{SEP}UserSettingsPathID"), "USRLOCAL".to_string()),
                (format!("ConVars{SEP}r_fog"), "0".to_string()),
            ]
        );
    }

    #[test]
    fn reports_changed_added_and_removed_settings() {
        let before = "\"video.cfg\"\n{\n \"a\" \"1\"\n \"b\" \"2\"\n \"gone\" \"x\"\n}\n";
        let after = "\"video.cfg\"\n{\n \"a\" \"1\"\n \"B\" \"3\"\n \"new\" \"y\"\n}\n";
        let d = diff_file("video.txt", Some(before), after);
        assert!(!d.created);
        assert_eq!(d.unchanged, 1);
        assert_eq!(
            d.changes,
            vec![
                Change { key: "B".into(), before: Some("2".into()), after: Some("3".into()) },
                Change { key: "new".into(), before: None, after: Some("y".into()) },
                Change { key: "gone".into(), before: Some("x".into()), after: None },
            ]
        );
    }

    #[test]
    fn new_file_lists_everything_as_added() {
        let d = diff_file("video.txt", None, "\"video.cfg\"\n{\n \"a\" \"1\"\n}\n");
        assert!(d.created);
        assert_eq!(d.changes.len(), 1);
        assert_eq!(d.changes[0].before, None);
    }
}
