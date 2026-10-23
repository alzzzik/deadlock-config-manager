//! Share codes: a short code like `DCM-3F9K-T2QD-8W` that recreates a repo config plus the sharer's
//! settings on someone else's PC. No server is involved: the code names the repo (one byte for the
//! known repos), the pack (a 2-byte hash of its id, matched against the repo's current pack list), the
//! ranked-safe choice and the settings (view width, FPS caps, fixes). Importing makes a saved config,
//! which the user previews and applies like any other. Saved configs are shared as zip files instead.

use crate::diff;
use crate::overrides::{Entry, Overrides, FIXES};
use sha2::{Digest, Sha256};

pub const PREFIX: &str = "DCM-";
pub const LINK_SCHEME: &str = "deadlockcfg";
const VERSION: u8 = 1;
/// Repos that get a one-byte id. Append only: codes already shared depend on the positions.
const KNOWN_REPOS: [&str; 4] = ["alzzzik/alz-deadlock-cfg", "Sqooky/OptimizationLock", "dacooderr/OptiLock", "7liv/compylock"];
const OTHER_REPO: u8 = 0xFF;
/// Crockford base32: no I, L, O or U, so codes are easy to read out and type.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const MAX_CODE_CHARS: usize = 200;

const SAFE: u8 = 1;
const HAS_ASPECT: u8 = 2;
const HAS_FPS: u8 = 4;
const HAS_FPS_UI: u8 = 8;
const HAS_FIXES: u8 = 16;

#[derive(Debug, Clone, PartialEq)]
pub struct Shared {
    /// `owner/name`.
    pub repo: String,
    /// First two bytes of SHA-256 of the pack id.
    pub pack_hash: [u8; 2],
    pub safe: bool,
    pub settings: Overrides,
}

pub fn pack_hash(pack_id: &str) -> [u8; 2] {
    let h = Sha256::digest(pack_id.to_ascii_lowercase().as_bytes());
    [h[0], h[1]]
}

fn to_base32(bytes: &[u8]) -> String {
    let mut out = String::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in bytes {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((acc << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn from_base32(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.chars() {
        // Common misreadings map to the digit they look like.
        let c = match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        };
        let v = ALPHABET.iter().position(|&a| a as char == c)? as u32;
        acc = (acc << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

fn checksum(payload: &[u8]) -> u8 {
    Sha256::digest(payload)[0]
}

pub fn encode(s: &Shared) -> Result<String, String> {
    crate::repos::canonical(&s.repo)?;
    let mut b = vec![VERSION];
    match KNOWN_REPOS.iter().position(|r| r.eq_ignore_ascii_case(&s.repo)) {
        Some(i) => b.push(i as u8),
        None => {
            let name = s.repo.as_bytes();
            b.push(OTHER_REPO);
            b.push(u8::try_from(name.len()).map_err(|_| "Repo name too long.")?);
            b.extend_from_slice(name);
        }
    }
    b.extend_from_slice(&s.pack_hash);
    let o = &s.settings;
    let aspect = o.aspect.as_deref().and_then(|a| a.parse::<f64>().ok()).map(|a| (a * 100.0).round() as u16);
    let fixes: u8 = FIXES.iter().enumerate().filter(|(_, f)| o.fixes.iter().any(|x| x == f.id)).map(|(i, _)| 1u8 << i).fold(0, |a, x| a | x);
    let flags = (if s.safe { SAFE } else { 0 })
        | (if aspect.is_some() { HAS_ASPECT } else { 0 })
        | (if o.fps_max.is_some() { HAS_FPS } else { 0 })
        | (if o.fps_max_ui.is_some() { HAS_FPS_UI } else { 0 })
        | (if fixes != 0 { HAS_FIXES } else { 0 });
    b.push(flags);
    for v in [aspect, o.fps_max.map(|v| v.min(u16::MAX as u32) as u16), o.fps_max_ui.map(|v| v.min(u16::MAX as u32) as u16)].into_iter().flatten() {
        b.extend_from_slice(&v.to_be_bytes());
    }
    if fixes != 0 {
        b.push(fixes);
    }
    b.push(checksum(&b));
    let chars = to_base32(&b);
    let groups: Vec<String> = chars.as_bytes().chunks(4).map(|c| String::from_utf8_lossy(c).into_owned()).collect();
    Ok(format!("{PREFIX}{}", groups.join("-")))
}

/// Finds a code in pasted text (or a `deadlockcfg://import/...` link) and decodes it.
pub fn decode(text: &str) -> Result<Shared, String> {
    let start = text.to_ascii_uppercase().find(PREFIX).ok_or("No share code found. Codes start with DCM-.")?;
    let body: String = text[start + PREFIX.len()..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == ' ')
        .filter(|c| c.is_ascii_alphanumeric())
        .take(MAX_CODE_CHARS)
        .collect();
    let bytes = from_base32(&body).ok_or("That share code has a character that doesn't belong in it.")?;
    let (payload, check) = bytes.split_at(bytes.len().saturating_sub(1));
    if payload.len() < 5 || check.first() != Some(&checksum(payload)) {
        return Err("That share code isn't complete or has a typo.".into());
    }
    let mut r = payload.iter().copied();
    let mut next = || r.next().ok_or_else(|| "That share code is cut off.".to_string());
    if next()? != VERSION {
        return Err("That share code is from a newer app version.".into());
    }
    let repo = match next()? {
        OTHER_REPO => {
            let len = next()? as usize;
            let name: Vec<u8> = (0..len).map(|_| next()).collect::<Result<_, _>>()?;
            crate::repos::canonical(&String::from_utf8(name).map_err(|_| "Bad repo name in the share code.")?)?
        }
        i => KNOWN_REPOS.get(i as usize).ok_or("That share code names an unknown repo.")?.to_string(),
    };
    let pack_hash = [next()?, next()?];
    let flags = next()?;
    let mut u16v = || -> Result<u16, String> { Ok(u16::from_be_bytes([next()?, next()?])) };
    let aspect = if flags & HAS_ASPECT != 0 { Some(format!("{:.2}", u16v()? as f64 / 100.0)) } else { None };
    let fps_max = if flags & HAS_FPS != 0 { Some(u16v()? as u32) } else { None };
    let fps_max_ui = if flags & HAS_FPS_UI != 0 { Some(u16v()? as u32) } else { None };
    let fix_bits = if flags & HAS_FIXES != 0 { next()? } else { 0 };
    let fixes = FIXES.iter().enumerate().filter(|(i, _)| fix_bits & (1 << i) != 0).map(|(_, f)| f.id.to_string()).collect();
    let aspect = aspect.map(|a| crate::overrides::parse_aspect(&a)).transpose()?;
    Ok(Shared { repo, pack_hash, safe: flags & SAFE != 0, settings: Overrides { fixes, aspect, fps_max, fps_max_ui } })
}

/// ConVars differences of `src` against `stock`, as overrides that rebuild `src`'s ConVars from stock.
pub fn convar_delta(stock: &str, src: &str) -> Result<Vec<Entry>, String> {
    let convars = |text: &str| -> Result<Vec<(String, String)>, String> {
        let nodes = crate::kv::parse_document(text).map_err(|e| format!("gameinfo.gi {e}"))?;
        Ok(diff::flatten(&nodes)
            .into_iter()
            .filter_map(|(k, v)| crate::convars::convar_of(&k).map(|n| (format!("ConVars{}{n}", diff::SEP), v)))
            .collect())
    };
    let (changes, _) = diff::diff_pairs(&convars(stock)?, &convars(src)?);
    Ok(changes
        .into_iter()
        .filter_map(|c| {
            let name = crate::convars::convar_of(&c.key)?.to_string();
            Some(Entry { name, value: c.after })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared(repo: &str, settings: Overrides) -> Shared {
        Shared { repo: repo.into(), pack_hash: pack_hash("stable"), safe: false, settings }
    }

    #[test]
    fn known_repo_code_is_short_and_round_trips() {
        let plain = shared("alzzzik/alz-deadlock-cfg", Overrides::default());
        let code = encode(&plain).unwrap();
        assert!(code.len() <= 16, "{code}");
        assert_eq!(decode(&code).unwrap(), plain);

        let full = Shared {
            safe: true,
            ..shared(
                "dacooderr/OptiLock",
                Overrides { fixes: vec!["doorman_ult".into(), "decals".into()], aspect: Some("2.15".into()), fps_max: Some(400), fps_max_ui: Some(120) },
            )
        };
        let code = encode(&full).unwrap();
        assert!(code.len() <= 30, "{code}");
        assert_eq!(decode(&code).unwrap(), full);
    }

    #[test]
    fn finds_codes_in_text_and_forgives_case_and_lookalikes() {
        let s = shared("alzzzik/alz-deadlock-cfg", Overrides { fps_max: Some(300), ..Default::default() });
        let code = encode(&s).unwrap();
        assert_eq!(decode(&format!("try {} !", code.to_lowercase())).unwrap(), s);
        assert_eq!(decode(&format!("deadlockcfg://import/{code}")).unwrap(), s);
        assert_eq!(decode(&code.replace('-', " ").replacen(' ', "-", 1)).unwrap(), s);
    }

    #[test]
    fn other_repos_and_typos() {
        let s = shared("someone/their-cfg", Overrides::default());
        assert_eq!(decode(&encode(&s).unwrap()).unwrap(), s);
        let code = encode(&shared("alzzzik/alz-deadlock-cfg", Overrides::default())).unwrap();
        let last = code.chars().last().unwrap();
        let typo = format!("{}{}", &code[..code.len() - 1], if last == 'A' { 'B' } else { 'A' });
        assert!(decode(&typo).is_err());
        assert!(decode("hello").is_err());
        assert!(decode("DCM-!!").is_err());
    }
}
