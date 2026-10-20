//! HTTP to GitHub, kept gentle on the unauthenticated API limit (60 requests/hour):
//! API calls are conditional (ETag; a 304 costs nothing) and fall back to the last good
//! response, and per-folder commit info comes from github.com's Atom feeds, which aren't
//! counted against the API limit.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(15);
const USER_AGENT: &str = concat!("DeadlockConfigManager/", env!("CARGO_PKG_VERSION"));

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(TIMEOUT).user_agent(USER_AGENT).build()
}

fn status_error(code: u16, url: &str) -> String {
    match code {
        403 | 429 => "GitHub's hourly limit for anonymous requests is used up; it resets within an hour.".into(),
        _ => format!("GitHub returned HTTP {code} for {url}"),
    }
}

/// GET returning None on 404.
pub fn get_text(url: &str) -> Result<Option<String>, String> {
    match agent().get(url).call() {
        Ok(resp) => resp.into_string().map(Some).map_err(|e| format!("Download failed: {e}")),
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(ureq::Error::Status(code, _)) => Err(status_error(code, url)),
        Err(e) => Err(format!("Network error: {e}")),
    }
}

#[derive(Serialize, Deserialize)]
struct Cached {
    etag: Option<String>,
    body: String,
}

fn cache_file(dir: &Path, url: &str) -> PathBuf {
    let h = url.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    dir.join(format!("{h:016x}.json"))
}

/// Conditional GET for api.github.com: sends the stored ETag, reuses the stored body on 304,
/// and falls back to it when GitHub refuses (rate limit) or the network is down.
pub fn get_text_cached(url: &str, cache_dir: &Path) -> Result<Option<String>, String> {
    let file = cache_file(cache_dir, url);
    let cached: Option<Cached> = fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let mut req = agent().get(url);
    if let Some(etag) = cached.as_ref().and_then(|c| c.etag.as_deref()) {
        req = req.set("If-None-Match", etag);
    }
    match req.call() {
        Ok(resp) if resp.status() == 304 => Ok(cached.map(|c| c.body)),
        Ok(resp) => {
            let etag = resp.header("ETag").map(str::to_string);
            let body = resp.into_string().map_err(|e| format!("Download failed: {e}"))?;
            let _ = fs::create_dir_all(cache_dir);
            let _ = serde_json::to_vec(&Cached { etag, body: body.clone() }).map(|b| fs::write(&file, b));
            Ok(Some(body))
        }
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(ureq::Error::Status(code, _)) => cached.map(|c| Some(c.body)).ok_or_else(|| status_error(code, url)),
        Err(e) => cached.map(|c| Some(c.body)).ok_or_else(|| format!("Network error: {e}")),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Commit {
    pub sha: String,
    pub date: String,
    pub message: String,
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = s.find(start)? + start.len();
    let to = s[from..].find(end)? + from;
    Some(&s[from..to])
}

fn unescape_xml(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Latest commit from a github.com commits Atom feed.
pub fn parse_atom(feed: &str) -> Option<Commit> {
    let entry = between(feed, "<entry>", "</entry>")?;
    let sha = between(entry, "Grit::Commit/", "<")?.trim().to_string();
    if sha.is_empty() || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let date = between(entry, "<updated>", "</updated>")?.trim().to_string();
    let message = between(entry, "<title>", "</title>").map(|t| unescape_xml(t.trim())).unwrap_or_default();
    let message = message.lines().next().unwrap_or("").trim().to_string();
    Some(Commit { sha, date, message })
}

/// Latest commit touching `folder` ("" = whole repo), via the Atom feed.
pub fn latest_commit(owner: &str, name: &str, encoded_folder: &str) -> Option<Commit> {
    let path = if encoded_folder.is_empty() { String::new() } else { format!("/{encoded_folder}") };
    let url = format!("https://github.com/{owner}/{name}/commits/HEAD{path}.atom");
    get_text(&url).ok().flatten().as_deref().and_then(parse_atom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_latest_commit_from_atom() {
        let feed = r#"<feed><entry>
    <id>tag:github.com,2008:Grit::Commit/fca258fa8e53dfe05f1e6b5521d58917f721eff6</id>
    <link type="text/html" rel="alternate" href="https://github.com/o/r/commit/fca258f"/>
    <title>
        added Grimoire &amp; FileSystem flag
    </title>
    <updated>2026-10-16T14:31:44Z</updated>
  </entry><entry><id>tag:github.com,2008:Grit::Commit/0000</id></entry></feed>"#;
        assert_eq!(
            parse_atom(feed),
            Some(Commit {
                sha: "fca258fa8e53dfe05f1e6b5521d58917f721eff6".into(),
                date: "2026-10-16T14:31:44Z".into(),
                message: "added Grimoire & FileSystem flag".into(),
            })
        );
        assert_eq!(parse_atom("<feed></feed>"), None);
    }

    #[test]
    fn cached_get_falls_back_to_stored_body() {
        let dir = tempfile::tempdir().unwrap();
        // Unroutable URL -> network error -> served from the stored copy.
        let url = "http://127.0.0.1:9/never";
        let body = Cached { etag: Some("\"x\"".into()), body: "stored".into() };
        fs::write(cache_file(dir.path(), url), serde_json::to_vec(&body).unwrap()).unwrap();
        assert_eq!(get_text_cached(url, dir.path()).unwrap().as_deref(), Some("stored"));
        assert!(get_text_cached("http://127.0.0.1:9/other", dir.path()).is_err());
    }
}
