//! Valve's unmodified gameinfo.gi, used as the baseline for the ranked-safety check.
//!
//! A copy ships with the app (`data/stock_gameinfo.gi`, from SteamTracking/GameTracking-Deadlock).
//! A newer copy is downloaded from the same place at most every few hours and kept in the cache;
//! the PGIVersion in the installed gameinfo.gi decides which copy matches the game.

use crate::gameinfo;
use crate::github;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const EMBEDDED: &str = include_str!("../data/stock_gameinfo.gi");
const STOCK_URL: &str = "https://raw.githubusercontent.com/SteamTracking/GameTracking-Deadlock/master/game/citadel/gameinfo.gi";
/// How long a downloaded copy counts as fresh.
const REFRESH_AFTER: Duration = Duration::from_secs(6 * 60 * 60);

fn cache_file(cache_dir: &Path) -> PathBuf {
    cache_dir.join("stock_gameinfo.gi")
}

/// A stock file must parse and look like Valve's: GameInfo block with SearchPaths and a PGIVersion.
pub fn is_valid(src: &str) -> bool {
    gameinfo::validate(src).is_ok() && gameinfo::pgi_version(src).is_some()
}

/// Downloads the current stock file when the cached one is missing or older than a few hours.
/// Network errors are ignored; the cached or shipped copy stays in use.
pub fn refresh(cache_dir: &Path) {
    let file = cache_file(cache_dir);
    let fresh = fs::metadata(&file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < REFRESH_AFTER);
    if fresh {
        return;
    }
    if let Ok(Some(text)) = github::get_text(STOCK_URL) {
        if is_valid(&text) && fs::create_dir_all(cache_dir).is_ok() {
            let _ = fs::write(&file, text);
        }
    }
}

/// The stock file that best matches the installed game: the copy with the same PGIVersion as
/// `installed`, otherwise the downloaded copy, otherwise the shipped one. Never touches the network.
pub fn load(cache_dir: &Path, installed: Option<&str>) -> String {
    let cached = fs::read_to_string(cache_file(cache_dir)).ok().filter(|t| is_valid(t));
    let installed_pgi = installed.and_then(gameinfo::pgi_version);
    let matches = |src: &str| {
        installed_pgi
            .as_deref()
            .zip(gameinfo::pgi_version(src))
            .is_some_and(|(a, b)| a.eq_ignore_ascii_case(&b))
    };
    match cached {
        Some(c) if matches(&c) || !matches(EMBEDDED) => c,
        _ => EMBEDDED.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_copy_is_valid() {
        assert!(is_valid(EMBEDDED));
    }

    #[test]
    fn prefers_the_copy_matching_the_installed_version() {
        let dir = tempfile::tempdir().unwrap();
        // Nothing downloaded yet: shipped copy.
        assert_eq!(load(dir.path(), None), EMBEDDED);
        let newer = EMBEDDED.replacen(&gameinfo::pgi_version(EMBEDDED).unwrap(), "NEWER", 1);
        fs::write(cache_file(dir.path()), &newer).unwrap();
        // Downloaded copy wins unless the installed file matches the shipped one.
        assert_eq!(load(dir.path(), None), newer);
        assert_eq!(load(dir.path(), Some(EMBEDDED)), EMBEDDED);
        assert_eq!(load(dir.path(), Some(&newer)), newer);
    }

    #[test]
    fn ignores_a_broken_cached_copy() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(cache_file(dir.path()), "<html>rate limited</html>").unwrap();
        assert_eq!(load(dir.path(), None), EMBEDDED);
    }
}
