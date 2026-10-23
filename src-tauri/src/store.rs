//! Config store: one list of config repos and shared configs to browse in the app.
//!
//! The list lives in a public GitHub repo (`index.json` in alzzzik/deadlock-config-index), read through
//! jsDelivr so there's no server and no GitHub API limit. People add entries by pull request or issue.
//! Until that repo is online (or when offline) the app shows the last downloaded list, or a built-in one.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub const INDEX_URL: &str = "https://cdn.jsdelivr.net/gh/alzzzik/deadlock-config-index@main/index.json";
pub const SUBMIT_URL: &str = "https://github.com/alzzzik/deadlock-config-index/issues/new/choose";
const MAX_ENTRIES: usize = 500;
const MAX_TEXT: usize = 300;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Entry {
    /// A GitHub repo with config folders, linked like any other repo.
    #[serde(rename = "repo", rename_all = "camelCase")]
    Repo {
        repo: String,
        title: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        author: String,
    },
    /// A repo config with settings, as a short DCM- share code.
    #[serde(rename = "code", rename_all = "camelCase")]
    Code {
        title: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        author: String,
        code: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Index {
    pub version: u32,
    pub entries: Vec<Entry>,
}

/// Shipped with the app: the repos it already knows how to read.
const BUILT_IN: &str = r#"{"version":1,"entries":[
 {"kind":"repo","repo":"alzzzik/alz-deadlock-cfg","title":"alz","description":"More FPS while staying mod compatible. Stable for any GPU, alz variant with DLSS.","author":"alzzzik"},
 {"kind":"repo","repo":"Sqooky/OptimizationLock","title":"OptimizationLock","description":"Community presets from several authors, from balanced to minimum spec.","author":"Sqooky"},
 {"kind":"repo","repo":"dacooderr/OptiLock","title":"OptiLock","description":"Recommended, Potato and Ultra Potato presets.","author":"dacooderr"},
 {"kind":"repo","repo":"7liv/compylock","title":"compylock","description":"Competitive config.","author":"7liv"}
]}"#;

fn short(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect()
}

/// Parses an index and drops entries that don't check out (bad repo names, broken or unsafe codes).
pub fn parse(json: &str) -> Result<Index, String> {
    let raw: Index = serde_json::from_str(json).map_err(|e| format!("The store list is invalid: {e}"))?;
    if raw.version != 1 {
        return Err("The store list is from a newer app version. Update the app to browse it.".into());
    }
    let entries = raw
        .entries
        .into_iter()
        .take(MAX_ENTRIES)
        .filter_map(|e| match e {
            Entry::Repo { repo, title, description, author } => crate::repos::canonical(&repo)
                .ok()
                .map(|repo| Entry::Repo { repo, title: short(&title), description: short(&description), author: short(&author) }),
            Entry::Code { title, description, author, code } => crate::share::decode(&code)
                .ok()
                .map(|_| Entry::Code { title: short(&title), description: short(&description), author: short(&author), code }),
        })
        .collect();
    Ok(Index { version: 1, entries })
}

pub fn built_in() -> Index {
    parse(BUILT_IN).unwrap_or(Index { version: 1, entries: vec![] })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreView {
    pub entries: Vec<Entry>,
    /// The online list was read just now (false: cached or built-in list).
    pub online: bool,
    pub submit_url: &'static str,
}

/// The store list: online if possible, else the last good download, else the built-in list.
pub fn load(cache_dir: &Path) -> StoreView {
    let cache = cache_dir.join("store-index.json");
    let online = crate::github::get_text(INDEX_URL).ok().flatten().and_then(|t| parse(&t).ok().map(|i| (t, i)));
    if let Some((text, index)) = online {
        let _ = fs::create_dir_all(cache_dir).and_then(|_| fs::write(&cache, text));
        return StoreView { entries: index.entries, online: true, submit_url: SUBMIT_URL };
    }
    let cached = fs::read_to_string(&cache).ok().and_then(|t| parse(&t).ok());
    StoreView { entries: cached.unwrap_or_else(built_in).entries, online: false, submit_url: SUBMIT_URL }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_list_parses() {
        assert_eq!(built_in().entries.len(), 4);
    }

    #[test]
    fn drops_bad_entries() {
        let json = r#"{"version":1,"entries":[
            {"kind":"repo","repo":"not a repo","title":"x"},
            {"kind":"repo","repo":"https://github.com/a/b","title":"ok"},
            {"kind":"code","title":"broken","code":"DCM1-AAAA"},
            {"kind":"mystery","title":"?"}
        ]}"#;
        // Unknown kinds make the file invalid as a whole: the app keeps its previous list.
        assert!(parse(json).is_err());
        let json = r#"{"version":1,"entries":[
            {"kind":"repo","repo":"not a repo","title":"x"},
            {"kind":"repo","repo":"https://github.com/a/b","title":"ok"},
            {"kind":"code","title":"broken","code":"DCM1-AAAA"}
        ]}"#;
        let idx = parse(json).unwrap();
        assert_eq!(idx.entries, vec![Entry::Repo { repo: "a/b".into(), title: "ok".into(), description: String::new(), author: String::new() }]);
        assert!(parse(r#"{"version":2,"entries":[]}"#).is_err());
    }
}
