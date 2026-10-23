//! Config packs from a GitHub repo (packs.json, or every top-level folder with a gameinfo.gi),
//! plus local folders. Downloads are cached per pack and commit so offline re-applies work.

use crate::apply::PackFiles;
use crate::github::{self, get_text};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};


#[derive(Debug, Clone, PartialEq)]
pub struct Repo {
    pub owner: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PackInfo {
    /// `owner/name` of the repo the pack comes from (filled in after loading).
    #[serde(default)]
    pub repo: String,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub folder: String,
    #[serde(default)]
    pub last_commit_date: Option<String>,
    #[serde(default)]
    pub revision: Option<String>,
    /// First line of the latest commit that touched the pack folder.
    #[serde(default)]
    pub commit_message: Option<String>,
    #[serde(default)]
    pub html_url: String,
    /// Exact file name of the pack's video file (e.g. "Video.txt", "video(Shadows).txt"); None = "video.txt".
    #[serde(default)]
    pub video_file: Option<String>,
    /// Exact file name of a gameinfo variant (e.g. "gameinfo-EXTREME-LOW.gi"); None = "gameinfo.gi".
    #[serde(default)]
    pub gameinfo_file: Option<String>,
}

/// One linked repo and its packs, as shown in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoGroup {
    pub repo: String,
    pub url: String,
    /// Newest commit date among the repo's packs.
    pub updated_at: Option<String>,
    pub packs: Vec<PackInfo>,
    pub offline: bool,
    pub error: Option<String>,
}

pub struct FetchedPack {
    pub files: PackFiles,
    pub revision: Option<String>,
    pub from_cache: bool,
}

/// A git commit SHA (or "HEAD"), safe to use as a cache folder name and in URLs.
pub fn is_revision(s: &str) -> bool {
    s == "HEAD" || ((7..=40).contains(&s.len()) && s.chars().all(|c| c.is_ascii_hexdigit()))
}

fn is_safe_segment(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

/// Pack folder names may contain spaces, brackets and non-ASCII letters
/// (e.g. "OptiLock FPS Config (Recommended)"); they are only ever used URL-encoded.
fn is_safe_folder(folder: &str) -> bool {
    // "" = the repo root (repos that keep gameinfo.gi next to their README).
    folder.is_empty() || folder.split('/').all(|seg| {
        !seg.is_empty() && seg != "." && seg != ".." && seg.len() <= 120 && !seg.chars().any(|c| c.is_control() || c == '\\')
    })
}

/// Percent-encodes each path segment (keeps `/` between segments).
fn encode_path(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            seg.bytes()
                .map(|b| match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
                    _ => format!("%{b:02X}"),
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Stable, filesystem-safe id for a folder: "OptiLock FPS Config (Recommended)" -> "optilock-fps-config-recommended".
pub fn slug(folder: &str) -> String {
    let raw: String = folder
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let joined = raw.split('-').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("-");
    if joined.is_empty() {
        // Non-Latin names: fall back to a short hash so the id is still unique and safe.
        let h = folder.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
        format!("pack-{:x}", h & 0xffff_ffff)
    } else {
        joined
    }
}

/// Accepts `owner/repo` or a github.com URL.
pub fn parse_repo(input: &str) -> Result<Repo, String> {
    let trimmed = input.trim().trim_end_matches('/').trim_end_matches(".git");
    let path = ["https://github.com/", "http://github.com/", "github.com/"]
        .iter()
        .find_map(|p| trimmed.strip_prefix(p))
        .unwrap_or(trimmed);
    match path.split('/').collect::<Vec<_>>().as_slice() {
        [owner, name] if is_safe_segment(owner) && is_safe_segment(name) => {
            Ok(Repo { owner: owner.to_string(), name: name.to_string() })
        }
        _ => Err(format!("\"{input}\" is not a GitHub repo. Use owner/repo, e.g. alzzzik/alz-deadlock-cfg.")),
    }
}

/// api.github.com JSON through the ETag cache in `cache_root/http`.
fn get_api_json(url: &str, cache_root: &Path) -> Result<serde_json::Value, String> {
    let text = github::get_text_cached(url, &cache_root.join("http"))?.ok_or_else(|| format!("Not found: {url}"))?;
    serde_json::from_str(&text).map_err(|e| format!("Bad response from GitHub: {e}"))
}

/// `folder/file`, or just `file` for a pack at the repo root.
fn in_folder(folder: &str, file: &str) -> String {
    if folder.is_empty() {
        file.to_string()
    } else {
        format!("{folder}/{file}")
    }
}

fn raw_url(repo: &Repo, rev: &str, path: &str) -> String {
    format!("https://raw.githubusercontent.com/{}/{}/{rev}/{}", repo.owner, repo.name, encode_path(path))
}

/// Link to a pack folder on github.com.
pub fn folder_url(repo_label: &str, folder: &str) -> String {
    if folder.is_empty() {
        return format!("https://github.com/{repo_label}");
    }
    format!("https://github.com/{repo_label}/tree/HEAD/{}", encode_path(folder))
}

fn api_url(repo: &Repo, path: &str) -> String {
    format!("https://api.github.com/repos/{}/{}/{path}", repo.owner, repo.name)
}

/// True for text that reads like words, not ASCII art or separators.
fn is_wordy(s: &str) -> bool {
    let letters = s.chars().filter(|c| c.is_alphabetic()).count();
    letters >= 8 && letters * 2 >= s.chars().filter(|c| !c.is_whitespace()).count()
}

/// First `//` comment line of a file, used as a description when there is no packs.json.
/// ASCII-art banners don't count.
pub fn header_comment(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .and_then(|l| l.strip_prefix("//"))
        .map(|l| l.trim().to_string())
        .filter(|l| is_wordy(l) && l.split_whitespace().count() >= SUMMARY_MIN_WORDS)
        .unwrap_or_default()
}

const DESCRIPTION_MAX: usize = 220;

const SUMMARY_MIN_WORDS: usize = 5;

/// First sentence-like line of a readme (headings included, since some readmes put the
/// description there), with simple markdown removed. Short titles and labels like "Directions:" are skipped.
pub fn readme_summary(md: &str) -> String {
    let line = md
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('!') && !l.starts_with('<') && !l.starts_with("---"))
        .map(|l| l.trim_start_matches(['#', '>', '-', '*', ' ']).replace("**", "").replace('`', ""))
        .find(|l| is_wordy(l) && l.split_whitespace().count() >= SUMMARY_MIN_WORDS && !l.trim_end().ends_with(':'))
        .unwrap_or_default();
    match line.char_indices().nth(DESCRIPTION_MAX) {
        Some((i, _)) => format!("{}…", line[..i].trim_end()),
        None => line,
    }
}

/// Pack description: the gameinfo.gi header comment or the folder's readme. A pack at the repo
/// root reads the README first, since that describes the whole repo.
fn describe(repo: &Repo, folder: &str) -> String {
    let from_gameinfo = || get_text(&raw_url(repo, "HEAD", &in_folder(folder, "gameinfo.gi"))).ok().flatten().map(|t| header_comment(&t));
    let from_readme = || {
        ["readme.md", "README.md", "Readme.md"]
            .iter()
            .find_map(|name| get_text(&raw_url(repo, "HEAD", &in_folder(folder, name))).ok().flatten())
            .map(|md| readme_summary(&md))
    };
    let is_root = folder.is_empty();
    let primary = if is_root { from_readme() } else { from_gameinfo() };
    primary
        .filter(|d| !d.is_empty())
        .or_else(|| if is_root { from_gameinfo() } else { from_readme() })
        .unwrap_or_default()
}

#[derive(Deserialize)]
struct IndexFile {
    packs: Vec<PackInfo>,
}

pub fn parse_index(json: &str) -> Result<Vec<PackInfo>, String> {
    let idx: IndexFile = serde_json::from_str(json).map_err(|e| format!("packs.json is invalid: {e}"))?;
    Ok(idx.packs.into_iter().filter(|p| is_safe_segment(&p.id) && is_safe_folder(&p.folder)).collect())
}

/// Folders that contain gameinfo.gi, from a git tree listing: top-level folders, plus ""
/// (listed first) when the repo root itself is a pack.
pub fn folders_from_tree(tree: &serde_json::Value) -> Vec<String> {
    let paths = tree_paths(tree);
    let mut folders: Vec<String> = paths
        .iter()
        .filter_map(|p| p.rsplit_once('/'))
        .filter(|(f, name)| is_gameinfo_name(name) && !f.contains('/') && is_safe_folder(f))
        .map(|(f, _)| f.to_string())
        .collect();
    folders.sort();
    folders.dedup();
    if paths.iter().any(|p| !p.contains('/') && is_gameinfo_name(p)) {
        folders.insert(0, String::new());
    }
    folders
}

/// A plain file name (no folders, not "." or ".."), e.g. "video(No Shadows).txt".
fn is_safe_file_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':']) && !name.chars().any(char::is_control)
}

/// `gameinfo.gi` or a variant such as `gameinfo-EXTREME-LOW.gi` / `gameinfo (low).gi`.
fn is_gameinfo_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("gameinfo") && lower.ends_with(".gi") && !name.contains(['/', '\\'])
}

/// Variant gameinfo files directly in `folder`, not counting plain `gameinfo.gi`.
pub fn gameinfo_variants(paths: &[String], folder: &str) -> Vec<String> {
    let prefix = in_folder(folder, "");
    let mut files: Vec<String> = paths
        .iter()
        .filter_map(|p| p.strip_prefix(&prefix))
        .filter(|name| !name.contains('/') && is_gameinfo_name(name) && !name.eq_ignore_ascii_case("gameinfo.gi"))
        .map(str::to_string)
        .collect();
    files.sort();
    files
}

/// "gameinfo-EXTREME-LOW.gi" -> "EXTREME-LOW".
pub fn gameinfo_variant_label(file: &str) -> String {
    let stem = file.get(8..file.len().saturating_sub(3)).unwrap_or("");
    let label = stem.trim_matches(|c: char| "()[]_- ".contains(c));
    if label.is_empty() {
        file.to_string()
    } else {
        label.to_string()
    }
}

fn tree_paths(tree: &serde_json::Value) -> Vec<String> {
    tree["tree"]
        .as_array()
        .map(|a| a.iter().filter_map(|e| e["path"].as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// Video files directly in `folder`: `video.txt` in any case, or variants like `video(Shadows).txt`.
pub fn video_files(paths: &[String], folder: &str) -> Vec<String> {
    let prefix = in_folder(folder, "");
    let mut files: Vec<String> = paths
        .iter()
        .filter_map(|p| p.strip_prefix(&prefix))
        .filter(|name| !name.contains('/'))
        .filter(|name| {
            let lower = name.to_ascii_lowercase();
            lower.starts_with("video") && lower.ends_with(".txt")
        })
        .map(str::to_string)
        .collect();
    files.sort();
    files
}

/// "video(No Shadows).txt" -> "No Shadows"; "video_low.txt" -> "low".
pub fn variant_label(file: &str) -> String {
    let stem = file.get(5..file.len().saturating_sub(4)).unwrap_or("");
    let label = stem.trim_matches(|c: char| c == '(' || c == ')' || c == '_' || c == '-' || c == ' ' || c == '[' || c == ']');
    if label.is_empty() {
        file.to_string()
    } else {
        label.to_string()
    }
}

fn fetch_index(repo: &Repo, cache_root: &Path) -> Result<Vec<PackInfo>, String> {
    let base = match get_text(&raw_url(repo, "HEAD", "packs.json"))? {
        Some(json) => parse_index(&json)?,
        None => {
            // jsDelivr first: it lists files without touching GitHub's hourly API limit.
            let tree = match jsdelivr_tree(repo, cache_root) {
                Some(t) => t,
                None => match get_api_json(&api_url(repo, "git/trees/HEAD?recursive=1"), cache_root) {
                    Ok(t) => t,
                    // API refused and nothing cached: a root pack can still be found via raw files (no API limit).
                    Err(e) => root_only_tree(repo).ok_or(e)?,
                },
            };
            let paths = tree_paths(&tree);
            let mut used = std::collections::HashSet::new();
            let mut unique = |base: String| {
                (1..).map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") }).find(|c| used.insert(c.clone())).unwrap_or(base)
            };
            folders_from_tree(&tree)
                .into_iter()
                .flat_map(|folder| {
                    let description = describe(repo, &folder);
                    // A pack at the repo root is named after the repo.
                    let label = if folder.is_empty() { repo.name.clone() } else { folder.clone() };
                    let videos = video_files(&paths, &folder);
                    // One pack per video variant; a single (or missing) video file is just the folder.
                    let variants: Vec<Option<String>> = if videos.len() > 1 { videos.into_iter().map(Some).collect() } else { vec![videos.into_iter().next()] };
                    let many = variants.len() > 1;
                    let has_main = paths.iter().any(|p| p.eq_ignore_ascii_case(&in_folder(&folder, "gameinfo.gi")));
                    let main_packs: Vec<PackInfo> = if has_main {
                        variants
                            .into_iter()
                            .map(|video| {
                                let (name, id) = match (&video, many) {
                                    (Some(v), true) => (format!("{} · {}", label.clone(), variant_label(v)), slug(&format!("{label} {}", variant_label(v)))),
                                    _ => (label.clone(), slug(&label)),
                                };
                                PackInfo {
                                    repo: String::new(),
                                    commit_message: None,
                                    html_url: String::new(),
                                    description: description.clone(),
                                    id: unique(id),
                                    name,
                                    folder: folder.clone(),
                                    last_commit_date: None,
                                    revision: None,
                                    video_file: video,
                                    gameinfo_file: None,
                                }
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                    // Extra gameinfo files in the same folder (e.g. an "EXTREME-LOW" version) are packs of
                    // their own, sharing the folder's first video file.
                    let first_video = video_files(&paths, &folder).into_iter().next();
                    let variant_packs = gameinfo_variants(&paths, &folder).into_iter().map(|gi| {
                        let v = gameinfo_variant_label(&gi);
                        PackInfo {
                            repo: String::new(),
                            commit_message: None,
                            html_url: String::new(),
                            description: description.clone(),
                            id: unique(slug(&format!("{label} {v}"))),
                            name: format!("{} · {v}", label.clone()),
                            folder: folder.clone(),
                            last_commit_date: None,
                            revision: None,
                            video_file: first_video.clone(),
                            gameinfo_file: Some(gi),
                        }
                    });
                    main_packs.into_iter().chain(variant_packs).collect::<Vec<_>>()
                })
                .collect()
        }
    };
    if base.is_empty() {
        return Err("No config packs found in this repo.".into());
    }
    // Each folder's latest commit, fetched in parallel; variants of one folder share a lookup.
    let folders: Vec<String> = base.iter().map(|p| p.folder.clone()).fold(Vec::new(), |mut acc, f| {
        if !acc.contains(&f) {
            acc.push(f);
        }
        acc
    });
    let commits: Vec<(String, Option<github::Commit>)> = std::thread::scope(|s| {
        let handles: Vec<_> = folders
            .iter()
            .map(|f| s.spawn(move || (f.clone(), last_commit(repo, f, cache_root))))
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    Ok(base
        .into_iter()
        .map(|p| {
            let c = commits.iter().find(|(f, _)| *f == p.folder).and_then(|(_, c)| c.clone());
            with_commit(p, c)
        })
        .collect())
}

/// jsDelivr's flat file list (`{"files":[{"name":"/a/b"}]}`) in the shape of a git tree listing.
pub fn tree_from_jsdelivr(json: &serde_json::Value) -> Option<serde_json::Value> {
    let files = json["files"].as_array()?;
    let tree: Vec<serde_json::Value> = files
        .iter()
        .filter_map(|f| f["name"].as_str())
        .map(|n| serde_json::json!({ "path": n.trim_start_matches('/') }))
        .collect();
    Some(serde_json::json!({ "tree": tree }))
}

/// File list for the repo's latest commit via jsDelivr, pinned to the SHA from the Atom feed.
/// A listing for a given SHA never changes, so it's cached on disk forever.
fn jsdelivr_tree(repo: &Repo, cache_root: &Path) -> Option<serde_json::Value> {
    let sha = github::latest_commit(&repo.owner, &repo.name, "")?.sha;
    let cache = repo_cache(cache_root, repo).join(format!("tree-{sha}.json"));
    if let Some(t) = fs::read(&cache).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
        return Some(t);
    }
    let url = format!("https://data.jsdelivr.com/v1/packages/gh/{}/{}@{sha}?structure=flat", repo.owner, repo.name);
    let json: serde_json::Value = serde_json::from_str(&get_text(&url).ok()??).ok()?;
    let tree = tree_from_jsdelivr(&json).filter(|t| !tree_paths(t).is_empty())?;
    if fs::create_dir_all(repo_cache(cache_root, repo)).is_ok() {
        // Older listings of this repo are no longer needed.
        if let Ok(rd) = fs::read_dir(repo_cache(cache_root, repo)) {
            rd.filter_map(Result::ok)
                .filter(|e| e.file_name().to_string_lossy().starts_with("tree-"))
                .for_each(|e| drop(fs::remove_file(e.path())));
        }
        let _ = serde_json::to_vec(&tree).map(|b| fs::write(&cache, b));
    }
    Some(tree)
}

/// A minimal tree for a repo whose root is a pack, built from raw downloads.
fn root_only_tree(repo: &Repo) -> Option<serde_json::Value> {
    get_text(&raw_url(repo, "HEAD", "gameinfo.gi")).ok().flatten()?;
    let mut paths = vec!["gameinfo.gi"];
    if get_text(&raw_url(repo, "HEAD", "video.txt")).ok().flatten().is_some() {
        paths.push("video.txt");
    }
    Some(serde_json::json!({ "tree": paths.iter().map(|p| serde_json::json!({ "path": p })).collect::<Vec<_>>() }))
}

/// Latest commit for a folder: the Atom feed first (no API cost), then the API through the ETag cache.
fn last_commit(repo: &Repo, folder: &str, cache_root: &Path) -> Option<github::Commit> {
    github::latest_commit(&repo.owner, &repo.name, &encode_path(folder)).or_else(|| {
        let path = if folder.is_empty() { String::new() } else { format!("path={}&", encode_path(folder)) };
        let json = get_api_json(&api_url(repo, &format!("commits?{path}per_page=1")), cache_root).ok()?;
        let first = json.get(0)?;
        Some(github::Commit {
            sha: first["sha"].as_str()?.to_string(),
            date: first["commit"]["committer"]["date"].as_str().unwrap_or_default().to_string(),
            message: first["commit"]["message"].as_str().and_then(|m| m.lines().next()).unwrap_or_default().trim().to_string(),
        })
    })
}

fn with_commit(pack: PackInfo, commit: Option<github::Commit>) -> PackInfo {
    match commit {
        Some(c) => PackInfo {
            revision: Some(c.sha).filter(|s| is_safe_segment(s)),
            last_commit_date: Some(c.date).filter(|d| !d.is_empty()),
            commit_message: Some(c.message).filter(|m| !m.is_empty()),
            ..pack
        },
        None => pack,
    }
}

fn repo_cache(cache_root: &Path, repo: &Repo) -> PathBuf {
    cache_root.join(format!("{}__{}", repo.owner, repo.name))
}

/// Stamps repo-level fields onto packs (also fixes up caches written by older versions).
fn with_repo_fields(repo: &Repo, packs: Vec<PackInfo>) -> Vec<PackInfo> {
    let label = format!("{}/{}", repo.owner, repo.name);
    packs
        .into_iter()
        .map(|p| PackInfo {
            html_url: folder_url(&label, &p.folder),
            repo: label.clone(),
            ..p
        })
        .collect()
}

fn group(repo: &Repo, packs: Vec<PackInfo>, offline: bool, error: Option<String>) -> RepoGroup {
    let label = format!("{}/{}", repo.owner, repo.name);
    RepoGroup {
        url: format!("https://github.com/{label}"),
        // RFC 3339 UTC dates from GitHub sort correctly as strings.
        updated_at: packs.iter().filter_map(|p| p.last_commit_date.clone()).max(),
        repo: label,
        packs,
        offline,
        error,
    }
}

pub fn list_packs(repo_input: &str, cache_root: &Path) -> RepoGroup {
    let repo = match parse_repo(repo_input) {
        Ok(r) => r,
        Err(e) => {
            return RepoGroup { repo: repo_input.into(), url: String::new(), updated_at: None, packs: vec![], offline: false, error: Some(e) }
        }
    };
    let index_file = repo_cache(cache_root, &repo).join("index.json");
    match fetch_index(&repo, cache_root) {
        Ok(packs) => {
            let packs = with_repo_fields(&repo, packs);
            let _ = fs::create_dir_all(repo_cache(cache_root, &repo));
            let _ = serde_json::to_vec_pretty(&packs).map(|b| fs::write(&index_file, b));
            group(&repo, packs, false, None)
        }
        Err(e) => {
            let cached: Vec<PackInfo> = fs::read(&index_file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
            let cached = with_repo_fields(&repo, cached);
            let offline = !cached.is_empty();
            group(&repo, cached, offline, Some(e))
        }
    }
}

/// Loads every repo in parallel, keeping the given order.
/// The last saved pack list for each repo, read from disk only (no network), so the UI can show
/// packs immediately on startup and refresh them in the background. Repos never loaded are skipped.
pub fn cached_all(repos: &[String], cache_root: &Path) -> Vec<RepoGroup> {
    repos
        .iter()
        .filter_map(|r| parse_repo(r).ok())
        .filter_map(|repo| {
            let bytes = fs::read(repo_cache(cache_root, &repo).join("index.json")).ok()?;
            let packs: Vec<PackInfo> = serde_json::from_slice(&bytes).ok()?;
            Some(group(&repo, with_repo_fields(&repo, packs), false, None))
        })
        .collect()
}

pub fn list_all(repos: &[String], cache_root: &Path) -> Vec<RepoGroup> {
    std::thread::scope(|s| {
        let handles: Vec<_> = repos.iter().map(|r| s.spawn(move || list_packs(r, cache_root))).collect();
        handles
            .into_iter()
            .zip(repos)
            .map(|(h, r)| {
                h.join().unwrap_or_else(|_| RepoGroup {
                    repo: r.clone(),
                    url: String::new(),
                    updated_at: None,
                    packs: vec![],
                    offline: false,
                    error: Some("Loading this repo crashed.".into()),
                })
            })
            .collect()
    })
}

fn read_cached(dir: &Path) -> Option<PackFiles> {
    Some(PackFiles {
        gameinfo: fs::read_to_string(dir.join("gameinfo.gi")).ok()?,
        video: fs::read_to_string(dir.join("video.txt")).ok(),
    })
}

fn newest_cached(pack_dir: &Path) -> Option<(PathBuf, String)> {
    fs::read_dir(pack_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.path().join("gameinfo.gi").is_file())
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| (e.path(), e.file_name().to_string_lossy().into_owned()))
}

pub fn fetch_pack(repo_input: &str, pack: &PackInfo, cache_root: &Path) -> Result<FetchedPack, String> {
    let repo = parse_repo(repo_input)?;
    let bad_file = |f: &Option<String>| f.as_deref().is_some_and(|n| !is_safe_file_name(n));
    if !is_safe_segment(&pack.id)
        || !is_safe_folder(&pack.folder)
        || pack.revision.as_deref().is_some_and(|r| !is_revision(r))
        || bad_file(&pack.video_file)
        || bad_file(&pack.gameinfo_file)
    {
        return Err("Invalid pack.".into());
    }
    let pack_dir = repo_cache(cache_root, &repo).join(&pack.id);
    if let Some(rev) = &pack.revision {
        if let Some(files) = read_cached(&pack_dir.join(rev)) {
            return Ok(FetchedPack { files, revision: Some(rev.clone()), from_cache: true });
        }
    }
    let rev = pack.revision.clone().unwrap_or_else(|| "HEAD".into());
    let download = || -> Result<PackFiles, String> {
        let gi_name = pack.gameinfo_file.as_deref().filter(|g| is_gameinfo_name(g)).unwrap_or("gameinfo.gi");
        let gi = get_text(&raw_url(&repo, &rev, &in_folder(&pack.folder, gi_name)))?
            .ok_or("The pack has no gameinfo.gi.")?;
        let video_name = pack.video_file.as_deref().unwrap_or("video.txt");
        let video = get_text(&raw_url(&repo, &rev, &in_folder(&pack.folder, video_name)))?;
        Ok(PackFiles { gameinfo: gi, video })
    };
    match download() {
        Ok(files) => {
            let key = if rev == "HEAD" { chrono::Local::now().format("head-%Y%m%d%H%M%S").to_string() } else { rev.clone() };
            let dir = pack_dir.join(&key);
            if fs::create_dir_all(&dir).is_ok() {
                let _ = fs::write(dir.join("gameinfo.gi"), &files.gameinfo);
                if let Some(v) = &files.video {
                    let _ = fs::write(dir.join("video.txt"), v);
                }
            }
            Ok(FetchedPack { files, revision: pack.revision.clone(), from_cache: false })
        }
        Err(e) => match newest_cached(&pack_dir).and_then(|(dir, key)| read_cached(&dir).map(|f| (f, key))) {
            Some((files, key)) => Ok(FetchedPack { files, revision: Some(key), from_cache: true }),
            None => Err(format!("{e} No cached copy of this pack is available.")),
        },
    }
}

/// The pack folder's autoexec.cfg at `rev`, cached next to the pack files. None if it has none.
pub fn fetch_autoexec(pack: &PackInfo, rev: Option<&str>, cache_root: &Path) -> Option<String> {
    let repo = parse_repo(&pack.repo).ok()?;
    let rev = rev.filter(|r| is_revision(r)).unwrap_or("HEAD");
    if !is_safe_segment(&pack.id) || !is_safe_folder(&pack.folder) {
        return None;
    }
    let cached = repo_cache(cache_root, &repo).join(&pack.id).join(rev).join("autoexec.cfg");
    let missing_marker = cached.with_extension("none");
    if rev != "HEAD" {
        if let Ok(t) = fs::read_to_string(&cached) {
            return Some(t);
        }
        if missing_marker.exists() {
            return None;
        }
    }
    let text = get_text(&raw_url(&repo, rev, &in_folder(&pack.folder, "autoexec.cfg"))).ok()?;
    if let Some(parent) = cached.parent() {
        let _ = fs::create_dir_all(parent);
        let _ = match &text {
            Some(t) => fs::write(&cached, t),
            None => fs::write(&missing_marker, ""),
        };
    }
    text
}

pub fn read_local_autoexec(folder: &Path) -> Option<String> {
    fs::read_to_string(folder.join("autoexec.cfg")).ok()
}

/// A pack from a local folder (offline/testing).
pub fn read_local(folder: &Path) -> Result<PackFiles, String> {
    let gi = fs::read_to_string(folder.join("gameinfo.gi"))
        .map_err(|_| format!("{} has no readable gameinfo.gi.", folder.display()))?;
    Ok(PackFiles { gameinfo: gi, video: fs::read_to_string(folder.join("video.txt")).ok() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repo_forms() {
        let want = Repo { owner: "alzzzik".into(), name: "alz-deadlock-cfg".into() };
        assert_eq!(parse_repo("alzzzik/alz-deadlock-cfg").unwrap(), want);
        assert_eq!(parse_repo("https://github.com/alzzzik/alz-deadlock-cfg/").unwrap(), want);
        assert_eq!(parse_repo("github.com/alzzzik/alz-deadlock-cfg.git").unwrap(), want);
        assert!(parse_repo("alzzzik").is_err());
        assert!(parse_repo("../../etc/x").is_err());
    }

    #[test]
    fn reads_jsdelivr_listing_as_tree() {
        let json = serde_json::json!({ "files": [{ "name": "/stable/gameinfo.gi" }, { "name": "/stable/video.txt" }, { "name": "/gameinfo.gi" }] });
        let tree = tree_from_jsdelivr(&json).unwrap();
        assert_eq!(folders_from_tree(&tree), vec![String::new(), "stable".to_string()]);
        assert!(tree_from_jsdelivr(&serde_json::json!({})).is_none());
    }

    #[test]
    fn rejects_unsafe_revisions_and_file_names() {
        let tmp = tempfile::tempdir().unwrap();
        let base = PackInfo {
            repo: "a/b".into(), id: "p".into(), name: "P".into(), description: String::new(), folder: "p".into(),
            last_commit_date: None, revision: Some("../../../x/y/z".into()), commit_message: None, html_url: String::new(),
            video_file: None, gameinfo_file: None,
        };
        assert_eq!(fetch_pack("a/b", &base, tmp.path()).err().as_deref(), Some("Invalid pack."));
        let bad_video = PackInfo { revision: None, video_file: Some("..".into()), ..base.clone() };
        assert!(fetch_pack("a/b", &bad_video, tmp.path()).is_err());
        assert!(is_revision("0123abc") && is_revision("HEAD") && !is_revision("main") && !is_revision(".."));
    }

    #[test]
    fn finds_gameinfo_variants() {
        let paths: Vec<String> = ["kaiz/gameinfo.gi", "kaiz/gameinfo-EXTREME-LOW.gi", "kaiz/readme.md", "only/gameinfo (low).gi"].map(String::from).to_vec();
        assert_eq!(gameinfo_variants(&paths, "kaiz"), vec!["gameinfo-EXTREME-LOW.gi".to_string()]);
        assert_eq!(gameinfo_variant_label("gameinfo-EXTREME-LOW.gi"), "EXTREME-LOW");
        assert_eq!(gameinfo_variant_label("gameinfo (low).gi"), "low");
        let tree = serde_json::json!({ "tree": paths.iter().map(|p| serde_json::json!({ "path": p })).collect::<Vec<_>>() });
        assert_eq!(folders_from_tree(&tree), vec!["kaiz".to_string(), "only".to_string()]);
    }

    #[test]
    fn finds_pack_folders_in_tree() {
        let tree = serde_json::json!({"tree": [
            {"path": "README.md"}, {"path": "stable"}, {"path": "stable/gameinfo.gi"},
            {"path": "experimental/gameinfo.gi"}, {"path": "reports/old/gameinfo.gi"},
            {"path": "OptiLock FPS Config (Recommended)/gameinfo.gi"}
        ]});
        assert_eq!(folders_from_tree(&tree), vec!["OptiLock FPS Config (Recommended)", "experimental", "stable"]);
        let root = serde_json::json!({"tree": [{"path": "README.md"}, {"path": "gameinfo.gi"}, {"path": "video.txt"}, {"path": "x/gameinfo.gi"}]});
        assert_eq!(folders_from_tree(&root), vec!["", "x"]);
        assert_eq!(video_files(&tree_paths(&root), ""), vec!["video.txt"]);
        assert_eq!(folder_url("o/r", ""), "https://github.com/o/r");
        assert_eq!(in_folder("", "gameinfo.gi"), "gameinfo.gi");
        assert!(is_safe_folder(""));
    }

    #[test]
    fn finds_video_variants_in_any_case() {
        let paths: Vec<String> = [
            "A/gameinfo.gi", "A/video(No Shadows).txt", "A/video(Shadows).txt", "A/videotxtexample.png",
            "B/Video.txt", "B/sub/video.txt", "C/gameinfo.gi",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(video_files(&paths, "A"), vec!["video(No Shadows).txt", "video(Shadows).txt"]);
        assert_eq!(video_files(&paths, "B"), vec!["Video.txt"]);
        assert!(video_files(&paths, "C").is_empty());
        assert_eq!(variant_label("video(No Shadows).txt"), "No Shadows");
        assert_eq!(variant_label("video_low.txt"), "low");
    }

    #[test]
    fn parses_index_and_drops_unsafe_entries() {
        let json = r#"{"packs":[{"id":"stable","name":"Stable","description":"d","folder":"stable"},
                                 {"id":"bad","name":"B","folder":"../x"}]}"#;
        let packs = parse_index(json).unwrap();
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].name, "Stable");
    }

    #[test]
    fn uses_header_comment_as_description() {
        assert_eq!(header_comment("// alz cfg - stable build\n\nGameInfo"), "alz cfg - stable build");
        assert_eq!(header_comment("GameInfo\n{"), "");
        assert_eq!(header_comment("//   ____        __  _ __   \\\\\n//  / __ \\____  / /_(_) /"), "");
    }

    #[test]
    fn summarises_readme_and_handles_spaced_folders() {
        let md = "# OptiLock\n\n![img](x.png)\n\n**Recommended** config for most PCs, balancing FPS and clarity.\n";
        assert_eq!(readme_summary(md), "Recommended config for most PCs, balancing FPS and clarity.");
        assert_eq!(slug("OptiLock FPS Config (Recommended)"), "optilock-fps-config-recommended");
        assert!(slug("Картофельный пресет").starts_with("pack-"));
        assert_eq!(encode_path("A B (C)/video.txt"), "A%20B%20%28C%29/video.txt");
        assert_eq!(folder_url("o/r", "A B"), "https://github.com/o/r/tree/HEAD/A%20B");
        assert!(is_safe_folder("OptiLock FPS Config (Recommended)"));
        assert!(!is_safe_folder("../etc") && !is_safe_folder("a\\b"));
    }

    #[test]
    fn cached_list_reads_saved_index_only() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("o__r");
        fs::create_dir_all(&dir).unwrap();
        let json = r#"[{"id":"stable","name":"Stable","folder":"stable","lastCommitDate":"2026-10-19T00:00:00Z"}]"#;
        fs::write(dir.join("index.json"), json).unwrap();
        let groups = cached_all(&["o/r".to_string(), "never/loaded".to_string()], tmp.path());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].repo, "o/r");
        assert_eq!(groups[0].packs[0].repo, "o/r");
        assert_eq!(groups[0].updated_at.as_deref(), Some("2026-10-19T00:00:00Z"));
    }

    #[test]
    fn reads_cached_pack_by_revision_without_network() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("alzzzik__alz-deadlock-cfg").join("stable").join("abc1234");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("gameinfo.gi"), "GameInfo { }").unwrap();
        let pack = PackInfo {
            repo: "alzzzik/alz-deadlock-cfg".into(), id: "stable".into(), name: "Stable".into(), description: String::new(),
            folder: "stable".into(), last_commit_date: None, revision: Some("abc1234".into()), commit_message: None,
            html_url: String::new(), video_file: None, gameinfo_file: None,
        };
        let got = fetch_pack("alzzzik/alz-deadlock-cfg", &pack, tmp.path()).unwrap();
        assert!(got.from_cache);
        assert_eq!(got.files.gameinfo, "GameInfo { }");
        assert!(got.files.video.is_none());
    }

    #[test]
    fn group_reports_newest_update_and_stamps_links() {
        let repo = parse_repo("o/r").unwrap();
        let pack = |id: &str, date: &str| PackInfo {
            repo: String::new(), id: id.into(), name: id.into(), description: String::new(), folder: id.into(),
            last_commit_date: Some(date.into()), revision: None, commit_message: None, html_url: String::new(), video_file: None, gameinfo_file: None,
        };
        let packs = with_repo_fields(&repo, vec![pack("a", "2026-09-01T00:00:00Z"), pack("b", "2026-10-19T00:00:00Z")]);
        let g = group(&repo, packs, false, None);
        assert_eq!(g.updated_at.as_deref(), Some("2026-10-19T00:00:00Z"));
        assert_eq!(g.url, "https://github.com/o/r");
        assert_eq!(g.packs[0].repo, "o/r");
        assert_eq!(g.packs[1].html_url, "https://github.com/o/r/tree/HEAD/b");
    }
}
