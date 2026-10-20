//! Pure operations on the ordered list of linked repos (`owner/name`).

use crate::packs::parse_repo;

/// Canonical `owner/name` for any accepted input form.
pub fn canonical(input: &str) -> Result<String, String> {
    let r = parse_repo(input)?;
    Ok(format!("{}/{}", r.owner, r.name))
}

fn position(list: &[String], repo: &str) -> Option<usize> {
    list.iter().position(|r| r.eq_ignore_ascii_case(repo))
}

pub fn add(list: &[String], input: &str) -> Result<Vec<String>, String> {
    let repo = canonical(input)?;
    if position(list, &repo).is_some() {
        return Err(format!("{repo} is already linked."));
    }
    Ok(list.iter().cloned().chain(std::iter::once(repo)).collect())
}

pub fn remove(list: &[String], repo: &str) -> Vec<String> {
    list.iter().filter(|r| !r.eq_ignore_ascii_case(repo)).cloned().collect()
}

/// Moves `repo` by `delta` places (negative = up), clamped to the list bounds.
pub fn shift(list: &[String], repo: &str, delta: i32) -> Vec<String> {
    let Some(from) = position(list, repo) else {
        return list.to_vec();
    };
    let to = (from as i64 + delta as i64).clamp(0, list.len() as i64 - 1) as usize;
    let mut out = list.to_vec();
    let item = out.remove(from);
    out.insert(to, item);
    out
}

/// Adds `key` if missing, removes it if present.
pub fn toggle(list: &[String], key: &str) -> Vec<String> {
    if list.iter().any(|k| k == key) {
        list.iter().filter(|k| *k != key).cloned().collect()
    } else {
        list.iter().cloned().chain(std::iter::once(key.to_string())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> Vec<String> {
        vec!["a/one".into(), "b/two".into(), "c/three".into()]
    }

    #[test]
    fn adds_canonical_and_rejects_duplicates() {
        let added = add(&list(), "https://github.com/d/four/").unwrap();
        assert_eq!(added.last().unwrap(), "d/four");
        assert!(add(&list(), "B/TWO").unwrap_err().contains("already linked"));
        assert!(add(&list(), "not a repo").is_err());
    }

    #[test]
    fn removes_case_insensitively() {
        assert_eq!(remove(&list(), "A/One"), vec!["b/two", "c/three"]);
    }

    #[test]
    fn toggles_pins() {
        let pinned = toggle(&[], "o/r#stable");
        assert_eq!(pinned, vec!["o/r#stable"]);
        assert!(toggle(&pinned, "o/r#stable").is_empty());
    }

    #[test]
    fn shifts_within_bounds() {
        assert_eq!(shift(&list(), "c/three", -1), vec!["a/one", "c/three", "b/two"]);
        assert_eq!(shift(&list(), "a/one", -1), list());
        assert_eq!(shift(&list(), "a/one", 5), vec!["b/two", "c/three", "a/one"]);
        assert_eq!(shift(&list(), "x/y", 1), list());
    }
}
