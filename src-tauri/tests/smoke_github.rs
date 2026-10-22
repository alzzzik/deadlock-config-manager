//! End-to-end check against the real pack repo. Needs network, so it is ignored by default:
//! `cargo test --test smoke_github -- --ignored`

use cfghub::{apply, backup, gameinfo, packs, settings::DEFAULT_REPO, video, writer};
use std::fs;

#[test]
#[ignore]
fn applies_every_pack_from_github_to_temp_copies_and_restores() {
    let tmp = tempfile::tempdir().unwrap();
    let list = packs::list_packs(DEFAULT_REPO, &tmp.path().join("cache"));
    assert!(list.updated_at.is_some());
    assert!(list.packs.iter().all(|p| p.repo == DEFAULT_REPO && p.commit_message.is_some()));
    assert!(list.error.is_none(), "{:?}", list.error);
    assert!(list.packs.iter().any(|p| p.id == "stable"));

    let user_video = include_str!("../fixtures/user_video.txt");
    let installed = include_str!("../fixtures/gameinfo_dmm.gi");
    let keys: Vec<String> = video::DEFAULT_PRESERVED.iter().map(|s| s.to_string()).collect();

    for pack in &list.packs {
        let fetched = packs::fetch_pack(DEFAULT_REPO, pack, &tmp.path().join("cache")).unwrap();
        assert!(!fetched.from_cache);
        let plan = apply::build_plan(&fetched.files, Some(installed), apply::VideoInput::Existing(user_video), &keys).unwrap();

        let dir = tmp.path().join(&pack.id);
        fs::create_dir_all(&dir).unwrap();
        let gi = dir.join("gameinfo.gi");
        let vid = dir.join("video.txt");
        fs::write(&gi, installed).unwrap();
        fs::write(&vid, user_video).unwrap();

        let backups = dir.join("backups");
        let meta = backup::create(&backups, &backup::Sources { gameinfo: &gi, video: Some(&vid) }, Some(&pack.name), "smoke").unwrap();
        let writes = apply::plan_writes(&plan, &gi, Some(&vid));
        apply::write_verified(&writes, &writer::write_without_elevation, &|| apply::verify(&gi, Some(&vid), &plan), &|| Ok(())).unwrap();

        let written = fs::read_to_string(&gi).unwrap();
        assert!(gameinfo::search_paths_ok(&written).unwrap());
        let values = video::read_values(&fs::read_to_string(&vid).unwrap()).unwrap();
        assert!(values.iter().any(|(k, v)| k == "DeviceID" && v == "12036"));

        // Restore -> Original brings back byte-identical files.
        backup::restore(&backups, &meta, &|f| writer::write_without_elevation(f).map(|_| ())).unwrap();
        assert_eq!(fs::read_to_string(&gi).unwrap(), installed);
        assert_eq!(fs::read_to_string(&vid).unwrap(), user_video);
    }

    // Second fetch of the same revision is served from the cache.
    let again = packs::fetch_pack(DEFAULT_REPO, &list.packs[0], &tmp.path().join("cache")).unwrap();
    assert!(again.from_cache);
}

/// A repo with spaced folder names, fragment video.txt files and a Grimoire search path.
#[test]
#[ignore]
fn handles_optilock_style_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = "dacooderr/OptiLock";
    let list = packs::list_packs(repo, &tmp.path().join("cache"));
    assert!(list.error.is_none(), "{:?}", list.error);
    assert!(list.packs.len() >= 3, "{:?}", list.packs.iter().map(|p| &p.folder).collect::<Vec<_>>());
    let user_video = include_str!("../fixtures/user_video.txt");
    let installed = include_str!("../fixtures/gameinfo_dmm.gi");
    let keys: Vec<String> = video::DEFAULT_PRESERVED.iter().map(|s| s.to_string()).collect();

    for pack in &list.packs {
        println!("{} | {} | {}", pack.name, pack.id, pack.description);
        assert!(!pack.description.is_empty(), "no description for {}", pack.folder);
        let fetched = packs::fetch_pack(repo, pack, &tmp.path().join("cache")).unwrap();
        let plan = apply::build_plan(&fetched.files, Some(installed), apply::VideoInput::Existing(user_video), &keys).unwrap();
        assert!(gameinfo::search_paths_ok(&plan.gameinfo_out).unwrap());
        if fetched.files.gameinfo.contains("citadel/grimoire") {
            assert!(plan.gameinfo_out.contains("citadel/grimoire"), "Grimoire path dropped in {}", pack.folder);
        }
        let merged = plan.video_out.unwrap();
        let values = video::read_values(&merged).unwrap();
        assert!(values.iter().any(|(k, v)| k == "DeviceID" && v == "12036"));
        assert!(values.iter().any(|(k, v)| k == "setting.defaultres" && v == "1920"));
    }
}

/// A repo whose root is the pack (gameinfo.gi next to the README).
#[test]
#[ignore]
fn handles_pack_at_repo_root() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = "7liv/compylock";
    let list = packs::list_packs(repo, &tmp.path().join("cache"));
    assert!(list.error.is_none(), "{:?}", list.error);
    let root = list.packs.iter().find(|p| p.folder.is_empty()).expect("root pack");
    println!("{} | {} | {:?} | {:?} | {}", root.name, root.id, root.last_commit_date, root.commit_message, root.description);
    assert_eq!(root.name, "Compylock");
    assert!(root.last_commit_date.is_some(), "no date");
    let fetched = packs::fetch_pack(repo, root, &tmp.path().join("cache")).unwrap();
    let keys: Vec<String> = video::DEFAULT_PRESERVED.iter().map(|s| s.to_string()).collect();
    let plan = apply::build_plan(
        &fetched.files,
        Some(include_str!("../fixtures/gameinfo_dmm.gi")),
        apply::VideoInput::Existing(include_str!("../fixtures/user_video.txt")),
        &keys,
    )
    .unwrap();
    assert!(gameinfo::search_paths_ok(&plan.gameinfo_out).unwrap());
}

#[test]
#[ignore]
fn handles_gameinfo_variants_in_one_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let list = packs::list_packs("Sqooky/OptimizationLock", &cache);
    assert!(list.error.is_none(), "{:?}", list.error);
    let variant = list.packs.iter().find(|p| p.gameinfo_file.is_some()).expect("a gameinfo-*.gi variant pack");
    let fetched = packs::fetch_pack(&variant.repo, variant, &cache).unwrap();
    assert!(gameinfo::validate(&fetched.files.gameinfo).is_ok(), "{}", variant.name);
}
