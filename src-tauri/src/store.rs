//! Config store: one list of config repos, GameBanana configs and shared configs to browse in the app.
//!
//! The list is `src-tauri/data/config_catalog.json` in this app's repo. The app ships the copy it was built
//! with and also reads the current one from `main` through jsDelivr (no server, no GitHub API limit), so new
//! entries show up without a release. People suggest entries with the "Submit a config" issue form.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub const INDEX_URL: &str = "https://cdn.jsdelivr.net/gh/alzzzik/deadlock-config-manager@main/src-tauri/data/config_catalog.json";
pub const SUBMIT_URL: &str = "https://github.com/alzzzik/deadlock-config-manager/issues/new?template=submit_config.yml";
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

    fn updated(&self) -> Option<&str> {
        match self {
            Entry::Repo { updated, .. } | Entry::Gamebanana { updated, .. } => updated.as_deref(),
            Entry::Code { .. } => None,
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

/// Keeps a plausible YYYY-MM-DD date (2024 on, month 1-12, day 1-31), drops anything else, so a bad date
/// can't pin an entry to the top of the newest-first list.
fn date(d: Option<String>) -> Option<String> {
    d.filter(|d| {
        let b = d.as_bytes();
        let shape = b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() });
        let num = |r: std::ops::Range<usize>| d[r].parse::<u32>().unwrap_or(0);
        shape && (2024..=2100).contains(&num(0..4)) && (1..=12).contains(&num(5..7)) && (1..=31).contains(&num(8..10))
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
        .filter_map(|v| serde_json::from_value::<Entry>(v).ok())
        .filter_map(clean)
        .take(MAX_ENTRIES)
        .collect();
    Ok(Index { version: 1, checked: date(raw.checked), entries })
}

pub fn built_in() -> Index {
    parse(BUILT_IN).unwrap_or(Index { version: 1, checked: None, entries: vec![] })
}

/// The first list's entries, then built-in entries it doesn't have yet. For an entry in both, the copy with the
/// newer date wins (the online copy can lag behind a new release while jsDelivr caches it).
fn merge(first: Index, built_in: Index) -> Index {
    let (mut shared, extra): (Vec<Entry>, Vec<Entry>) = {
        let keys: HashSet<String> = first.entries.iter().map(Entry::key).collect();
        built_in.entries.into_iter().partition(|e| keys.contains(&e.key()))
    };
    let entries = first
        .entries
        .into_iter()
        .map(|e| match shared.iter().position(|b| b.key() == e.key()) {
            Some(i) if e.updated().is_some() && shared[i].updated() > e.updated() => shared.swap_remove(i),
            _ => e,
        })
        .chain(extra)
        .take(MAX_ENTRIES)
        .collect();
    Index { version: 1, checked: first.checked.max(built_in.checked), entries }
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
        assert_eq!(date(Some("2026-10-23".into())).as_deref(), Some("2026-10-23"));
        for bad in ["2026-99-99", "0000-00-00", "9999-12-31", "2026-1-01", "2026-10-32"] {
            assert_eq!(date(Some(bad.into())), None, "{bad}");
        }
    }

    #[test]
    fn invalid_entries_do_not_use_up_the_entry_limit() {
        let bad = r#"{"kind":"repo","repo":"not a repo","title":"x"}"#;
        let json = format!(r#"{{"version":1,"entries":[{},{{"kind":"repo","repo":"a/b","title":"ok"}}]}}"#, vec![bad; MAX_ENTRIES].join(","));
        assert_eq!(parse(&json).unwrap().entries.len(), 1);
    }

    #[test]
    fn newer_built_in_copy_beats_a_stale_online_one() {
        let online = parse(r#"{"version":1,"entries":[{"kind":"repo","repo":"dacooderr/OptiLock","title":"Old","updated":"2026-01-01"}]}"#).unwrap();
        let merged = merge(online, built_in());
        let optilock = merged.entries.iter().find(|e| e.key() == "repo:dacooderr/optilock").unwrap();
        assert!(optilock.updated() > Some("2026-01-01"));
        assert_eq!(merged.entries.len(), built_in().entries.len());
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
