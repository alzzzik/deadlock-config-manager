//! Which mods are enabled, for performance sessions. Deadlock loads `game\citadel\addons\pakNN_dir.vpk`;
//! Deadlock Mod Manager (DMM) keeps which of its mods are enabled in `addons\.dmm.json`, and their names and
//! chosen variants in `%APPDATA%\dev.stormix.deadlock-mod-manager\state.json`. VPKs DMM doesn't know about
//! (copied in by hand or by another tool) are listed by file name.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Mod {
    /// GameBanana id, DMM's "local-..." id, or the file name for a VPK DMM doesn't manage.
    pub id: String,
    pub name: String,
    /// "dmm" or "manual".
    pub source: String,
    /// The VPKs the game loads for it (pakNN_dir.vpk), lower numbers load first.
    pub vpks: Vec<String>,
    /// The archive(s) the variant was installed from, and the files picked from it: a mod's options.
    pub variant: Option<String>,
    pub files: Vec<String>,
    pub category: Option<String>,
    /// When the mod's files were last updated on GameBanana, to tell versions apart.
    pub updated: Option<String>,
    /// Total size of its VPKs in bytes.
    pub bytes: u64,
}

fn dmm_state_file() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("dev.stormix.deadlock-mod-manager").join("state.json"))
}

/// DMM keeps its store as a JSON string inside JSON: `{"local-config": "{\"state\": {\"localMods\": [...]}}"}`.
fn dmm_local_mods(state_json: &str) -> Vec<Value> {
    let Ok(outer) = serde_json::from_str::<Value>(state_json) else { return Vec::new() };
    let inner = match outer.get("local-config") {
        Some(Value::String(s)) => serde_json::from_str::<Value>(s).ok(),
        Some(v) => Some(v.clone()),
        None => None,
    };
    inner
        .as_ref()
        .and_then(|v| v.pointer("/state/localMods"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
}

/// The enabled mods, in load order. `addons` is `game\citadel\addons`; `dmm_state` is DMM's state.json text.
pub fn enabled_in(addons: &Path, dmm_state: Option<&str>) -> Vec<Mod> {
    let size = |vpk: &str| fs::metadata(addons.join(vpk)).map(|m| m.len()).unwrap_or(0);
    let manifest: Value = fs::read(addons.join(".dmm.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    let details = dmm_state.map(dmm_local_mods).unwrap_or_default();
    let mut out: Vec<Mod> = Vec::new();
    if let Some(mods) = manifest.get("mods").and_then(Value::as_object) {
        for (id, m) in mods {
            if m.get("enabled").and_then(Value::as_bool) != Some(true) {
                continue;
            }
            let d = details.iter().find(|d| d.get("remoteId").and_then(Value::as_str) == Some(id.as_str()));
            let vpks = strings(m.get("currentVpks"));
            let files = d
                .and_then(|d| d.pointer("/installedFileTree/files"))
                .and_then(Value::as_array)
                .map(|fs| {
                    fs.iter()
                        .filter(|f| f.get("is_selected").and_then(Value::as_bool) != Some(false))
                        .filter_map(|f| str_of(f, "path").or_else(|| str_of(f, "name")))
                        .collect()
                })
                .unwrap_or_default();
            out.push(Mod {
                id: id.clone(),
                name: d.and_then(|d| str_of(d, "name")).unwrap_or_else(|| id.clone()),
                source: "dmm".into(),
                bytes: vpks.iter().map(|v| size(v)).sum(),
                vpks,
                variant: d.and_then(|d| str_of(d, "activeVariantArchive")),
                files,
                category: d.and_then(|d| str_of(d, "category")),
                updated: d.and_then(|d| str_of(d, "filesUpdatedAt").or_else(|| str_of(d, "remoteUpdatedAt"))),
            });
        }
    }
    // VPKs the game loads that no DMM mod accounts for.
    let known: Vec<String> = out.iter().flat_map(|m| m.vpks.iter().map(|v| v.to_ascii_lowercase())).collect();
    if let Ok(entries) = fs::read_dir(addons) {
        let mut manual: Vec<String> = entries
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| {
                let l = n.to_ascii_lowercase();
                l.starts_with("pak") && l.ends_with("_dir.vpk") && !known.contains(&l)
            })
            .collect();
        manual.sort();
        out.extend(manual.into_iter().map(|n| Mod { id: n.clone(), name: n.clone(), source: "manual".into(), bytes: size(&n), vpks: vec![n], ..Default::default() }));
    }
    out.sort_by(|a, b| a.vpks.first().cmp(&b.vpks.first()));
    out
}

/// The enabled mods next to this gameinfo.gi, with DMM's names when its state is readable.
pub fn enabled(gameinfo: &Path) -> Vec<Mod> {
    let Some(addons) = gameinfo.parent().map(|d| d.join("addons")) else { return Vec::new() };
    let state = dmm_state_file().and_then(|f| fs::read_to_string(f).ok());
    enabled_in(&addons, state.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_enabled_dmm_mods_with_their_variants_and_hand_copied_vpks() {
        let dir = tempfile::tempdir().unwrap();
        let addons = dir.path();
        fs::write(
            addons.join(".dmm.json"),
            r#"{"version":3,"mods":{
                "622585":{"enabled":true,"order":3,"currentVpks":["pak03_dir.vpk"]},
                "587643":{"enabled":false,"currentVpks":[],"disabledVpks":["587643_pak01_dir.vpk"]},
                "local-91":{"enabled":true,"currentVpks":["pak05_dir.vpk"]}}}"#,
        )
        .unwrap();
        for f in ["pak03_dir.vpk", "pak05_dir.vpk", "pak07_dir.vpk", "587643_pak01_dir.vpk", "pak03_000.vpk"] {
            fs::write(addons.join(f), "1234").unwrap();
        }
        let inner = serde_json::json!({"state": {"localMods": [
            {"remoteId": "622585", "name": "Soul Can", "category": "Model Replacement", "filesUpdatedAt": "2026-02-07",
             "activeVariantArchive": "white.zip",
             "installedFileTree": {"files": [{"name": "pak99_dir.vpk", "path": "white/pak99_dir.vpk", "is_selected": true},
                                             {"name": "pak98_dir.vpk", "path": "black/pak98_dir.vpk", "is_selected": false}]}},
            {"remoteId": "local-91", "name": "dx11outlines"}
        ]}});
        let state = serde_json::json!({"local-config": inner.to_string()}).to_string();
        let mods = enabled_in(addons, Some(&state));
        let names: Vec<(&str, &str)> = mods.iter().map(|m| (m.name.as_str(), m.source.as_str())).collect();
        assert_eq!(names, vec![("Soul Can", "dmm"), ("dx11outlines", "dmm"), ("pak07_dir.vpk", "manual")]);
        assert_eq!(mods[0].variant.as_deref(), Some("white.zip"));
        assert_eq!(mods[0].files, vec!["white/pak99_dir.vpk".to_string()]);
        assert_eq!(mods[0].bytes, 4);
        // Without DMM's state the ids stand in for names.
        assert_eq!(enabled_in(addons, None)[0].name, "622585");
    }
}
