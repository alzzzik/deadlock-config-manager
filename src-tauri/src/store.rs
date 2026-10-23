//! Config store: one list of config repos, GameBanana configs and shared configs to browse in the app.
//!
//! The list lives in a public GitHub repo (`index.json` in alzzzik/deadlock-config-index), read through
//! jsDelivr so there's no server and no GitHub API limit. People add entries by pull request or issue.
//! The app also ships a checked list (`data/config_catalog.json`, same format) that fills in whatever the
//! online list (or its last download) doesn't have.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub const INDEX_URL: &str = "https://cdn.jsdelivr.net/gh/alzzzik/deadlock-config-index@main/index.json";
pub const SUBMIT_URL: &str = "https://github.com/alzzzik/deadlock-config-index/issues/new/choose";
const MAX_ENTRIES: usize = 500;
const MAX_TEXT: usize = 300;
const MAX_CONFIGS: usize = 20;

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
        /// SPDX id, empty when the repo has no licence.
        #[serde(default)]
        license: String,
        /// Date of the last commit, YYYY-MM-DD.
        #[serde(default)]
        updated: Option<String>,
        /// Config folder names, shown in the list and searched.
        #[serde(default)]
        configs: Vec<String>,
    },
    /// A config posted on GameBanana, added to My configs by mod id.
    #[serde(rename = "gamebanana", rename_all = "camelCase")]
    Gamebanana {
        id: u64,
        title: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        author: String,
        /// Date of the newest config download, YYYY-MM-DD.
        #[serde(default)]
        updated: Option<String>,
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

impl Entry {
    /// What makes two entries the same, across the online and built-in lists.
    fn key(&self) -> String {
        match self {
            Entry::Repo { repo, .. } => format!("repo:{}", repo.to_ascii_lowercase()),
            Entry::Gamebanana { id, .. } => format!("gamebanana:{id}"),
            Entry::Code { code, .. } => format!("code:{code}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Index {
    pub version: u32,
    /// When the dates in the list were last checked, YYYY-MM-DD.
    #[serde(default)]
    pub checked: Option<String>,
    pub entries: Vec<Entry>,
}

/// Shipped with the app: repos and GameBanana configs it was checked to read. Plain data, so the list grows
/// without code changes; `scripts/refresh_catalog.py` updates the dates.
const BUILT_IN: &str = include_str!("../data/config_catalog.json");

fn short(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect()
}

/// Keeps a YYYY-MM-DD date, drops anything else.
fn date(d: Option<String>) -> Option<String> {
    d.filter(|d| {
        let b = d.as_bytes();
        b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
    })
}

fn clean(e: Entry) -> Option<Entry> {
    match e {
        Entry::Repo { repo, title, description, author, license, updated, configs } => crate::repos::canonical(&repo).ok().map(|repo| Entry::Repo {
            repo,
            title: short(&title),
            description: short(&description),
            author: short(&author),
            license: short(&license),
            updated: date(updated),
            configs: configs.iter().take(MAX_CONFIGS).map(|c| short(c)).collect(),
        }),
        Entry::Gamebanana { id, title, description, author, updated } => (id > 0).then(|| Entry::Gamebanana {
            id,
            title: short(&title),
            description: short(&description),
            author: short(&author),
            updated: date(updated),
        }),
        Entry::Code { title, description, author, code } => crate::share::decode(&code)
            .ok()
            .map(|_| Entry::Code { title: short(&title), description: short(&description), author: short(&author), code }),
    }
}

/// Parses an index and drops entries that don't check out (bad repo names, broken or unsafe codes) or that
/// this app version doesn't know, so a newer list still works in older apps.
pub fn parse(json: &str) -> Result<Index, String> {
    #[derive(Deserialize)]
    struct Raw {
        version: u32,
        #[serde(default)]
        checked: Option<String>,
        entries: Vec<serde_json::Value>,
    }
    let raw: Raw = serde_json::from_str(json).map_err(|e| format!("The store list is invalid: {e}"))?;
    if raw.version != 1 {
        return Err("The store list is from a newer app version. Update the app to browse it.".into());
    }
    let entries = raw
        .entries
        .into_iter()
        .take(MAX_ENTRIES)
        .filter_map(|v| serde_json::from_value::<Entry>(v).ok())
        .filter_map(clean)
        .collect();
    Ok(Index { version: 1, checked: date(raw.checked), entries })
}

pub fn built_in() -> Index {
    parse(BUILT_IN).unwrap_or(Index { version: 1, checked: None, entries: vec![] })
}

/// The first list's entries, then built-in entries it doesn't have yet.
fn merge(first: Index, built_in: Index) -> Index {
    let mut seen: HashSet<String> = first.entries.iter().map(Entry::key).collect();
    let extra: Vec<Entry> = built_in.entries.into_iter().filter(|e| seen.insert(e.key())).collect();
    let checked = first.checked.max(built_in.checked);
    Index { version: 1, checked, entries: first.entries.into_iter().chain(extra).collect() }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreView {
    pub entries: Vec<Entry>,
    /// When the dates in the list were last checked, YYYY-MM-DD.
    pub checked: Option<String>,
    /// The online list was read just now (false: cached or built-in list).
    pub online: bool,
    pub submit_url: &'static str,
}

/// The store list: online if possible, else the last good download, plus the built-in list.
pub fn load(cache_dir: &Path) -> StoreView {
    let cache = cache_dir.join("store-index.json");
    let online = crate::github::get_text(INDEX_URL).ok().flatten().and_then(|t| parse(&t).ok().map(|i| (t, i)));
    let (index, is_online) = match online {
        Some((text, index)) => {
            let _ = fs::create_dir_all(cache_dir).and_then(|_| fs::write(&cache, text));
            (index, true)
        }
        None => {
            let cached = fs::read_to_string(&cache).ok().and_then(|t| parse(&t).ok());
            (cached.unwrap_or(Index { version: 1, checked: None, entries: vec![] }), false)
        }
    };
    let index = merge(index, built_in());
    StoreView { entries: index.entries, checked: index.checked, online: is_online, submit_url: SUBMIT_URL }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_list_parses_with_dates() {
        let raw: serde_json::Value = serde_json::from_str(BUILT_IN).unwrap();
        let idx = built_in();
        // Nothing in the shipped list is dropped, and every entry says when it was last updated.
        assert_eq!(idx.entries.len(), raw["entries"].as_array().unwrap().len());
        assert!(idx.entries.len() >= 15);
        assert!(idx.checked.is_some());
        assert!(idx.entries.iter().all(|e| match e {
            Entry::Repo { updated, .. } | Entry::Gamebanana { updated, .. } => updated.is_some(),
            Entry::Code { .. } => true,
        }));
        let keys: HashSet<String> = idx.entries.iter().map(Entry::key).collect();
        assert_eq!(keys.len(), idx.entries.len(), "duplicate entries");
    }

    #[test]
    fn drops_bad_and_unknown_entries() {
        let json = r#"{"version":1,"checked":"not a date","entries":[
            {"kind":"repo","repo":"not a repo","title":"x"},
            {"kind":"repo","repo":"https://github.com/a/b","title":"ok","updated":"2026-10-01","configs":["one"]},
            {"kind":"gamebanana","id":5,"title":"gb","updated":"yesterday"},
            {"kind":"gamebanana","id":0,"title":"no id"},
            {"kind":"code","title":"broken","code":"DCM1-AAAA"},
            {"kind":"mystery","title":"?"}
        ]}"#;
        let idx = parse(json).unwrap();
        assert_eq!(idx.checked, None);
        assert_eq!(
            idx.entries,
            vec![
                Entry::Repo {
                    repo: "a/b".into(),
                    title: "ok".into(),
                    description: String::new(),
                    author: String::new(),
                    license: String::new(),
                    updated: Some("2026-10-01".into()),
                    configs: vec!["one".into()],
                },
                Entry::Gamebanana { id: 5, title: "gb".into(), description: String::new(), author: String::new(), updated: None },
            ]
        );
        assert!(parse(r#"{"version":2,"entries":[]}"#).is_err());
        assert!(parse("not json").is_err());
    }

    #[test]
    fn online_list_comes_first_and_built_in_fills_the_gaps() {
        let online = parse(
            r#"{"version":1,"checked":"2099-01-01","entries":[
            {"kind":"repo","repo":"dacooderr/optilock","title":"Online title"},
            {"kind":"repo","repo":"new/repo","title":"New"}
        ]}"#,
        )
        .unwrap();
        let shipped = built_in();
        let merged = merge(online, shipped.clone());
        assert_eq!(merged.checked.as_deref(), Some("2099-01-01"));
        assert_eq!(merged.entries.len(), shipped.entries.len() + 1);
        let optilock: Vec<&Entry> = merged.entries.iter().filter(|e| e.key() == "repo:dacooderr/optilock").collect();
        assert_eq!(optilock.len(), 1);
        assert!(matches!(optilock[0], Entry::Repo { title, .. } if title == "Online title"));
    }
}
