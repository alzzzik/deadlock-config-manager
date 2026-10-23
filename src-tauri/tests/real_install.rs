//! Checks against copies of a real Deadlock install and Steam account on the developer's PC.
//! Nothing outside a temp folder is written. Ignored by default:
//! `cargo test --test real_install -- --ignored --nocapture`
//! Set DCM_STEAM (Steam folder) and DCM_ACCOUNT (Steam account id) to point at another PC's files.

use cfghub::{apply, autoexec, launch, overlay, overrides, paths, perf, ranked, stock, video};
use std::fs;
use std::path::PathBuf;

fn steam() -> PathBuf {
    std::env::var_os("DCM_STEAM").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)\Steam"))
}

fn account() -> String {
    // First account that has played Deadlock, unless one is given.
    std::env::var("DCM_ACCOUNT").unwrap_or_else(|_| {
        fs::read_dir(steam().join("userdata"))
            .expect("Steam userdata folder")
            .filter_map(Result::ok)
            .find(|e| e.path().join(r"1422450\local\cfg\video.txt").is_file() && e.path().join(r"config\localconfig.vdf").is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .expect("an account with Deadlock settings")
    })
}

fn citadel() -> PathBuf {
    steam().join(r"steamapps\common\Deadlock\game\citadel")
}

fn read(p: PathBuf) -> String {
    String::from_utf8_lossy(&fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).into_owned()
}

#[test]
#[ignore]
fn launch_options_round_trip_on_a_copy_of_the_real_localconfig() {
    let src = read(launch::localconfig_path(&steam(), &account()));
    let before = launch::read(&src).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let copy = tmp.path().join("localconfig.vdf");
    let wanted = launch::with_flag(&before, "-condebug", true);
    let out = launch::write(&src, &wanted).unwrap();
    fs::write(&copy, &out).unwrap();
    assert_eq!(launch::read(&read(copy)).unwrap(), wanted.trim());
    // Only the one value changed: the file is identical apart from it.
    let back = launch::write(&out, &before).unwrap();
    assert_eq!(back, src, "writing the old value back should restore the exact file");
    println!("launch options: {before:?} -> {wanted:?} and back, {} bytes untouched otherwise", src.len());
}

#[test]
#[ignore]
fn real_gameinfo_ranked_check_overlay_and_tweaks() {
    let gi = read(citadel().join("gameinfo.gi"));
    let cache = tempfile::tempdir().unwrap();
    let stock_text = stock::load(cache.path(), Some(&gi));
    let changes = ranked::guarded_changes(&stock_text, &gi).unwrap();
    println!("installed gameinfo.gi: {} engine-section change(s) vs Valve", changes.len());
    let safe = ranked::make_safe(&stock_text, &gi).unwrap();
    assert!(ranked::guarded_changes(&stock_text, &safe).unwrap().is_empty());
    let (merged, added) = overlay::merge(&stock_text, &safe).unwrap();
    println!("overlay would add {} convar(s) and {:?}", added.convars, added.sections);
    let tweaks = overrides::Overrides { fixes: vec!["doorman_ult".into()], aspect: Some("2.15".into()), fps_max: Some(400), fps_max_ui: Some(120) };
    let out = overrides::apply(&merged, &tweaks.effective()).unwrap();
    assert!(cfghub::gameinfo::validate(&out).is_ok());
    assert!(cfghub::gameinfo::search_paths_ok(&cfghub::gameinfo::enforce(&out).unwrap()).unwrap());
}

#[test]
#[ignore]
fn apply_plan_with_the_real_video_txt_keeps_hardware_lines() {
    let gi = read(citadel().join("gameinfo.gi"));
    let user_video = read(steam().join("userdata").join(account()).join(r"1422450\local\cfg\video.txt"));
    let pack = apply::PackFiles { gameinfo: gi.clone(), video: Some(user_video.clone()) };
    let keys: Vec<String> = video::DEFAULT_PRESERVED.iter().map(|s| s.to_string()).collect();
    let plan = apply::build_plan(&pack, Some(&gi), apply::VideoInput::Existing(&user_video), &keys).unwrap();
    let out = plan.video_out.unwrap();
    for (k, v) in video::expected_preserved(&user_video, &keys).unwrap() {
        assert!(video::read_values(&out).unwrap().iter().any(|(ok, ov)| ok.eq_ignore_ascii_case(&k) && *ov == v), "{k} kept");
    }
}

#[test]
#[ignore]
fn autoexec_merge_on_the_real_autoexec() {
    let existing = fs::read_to_string(citadel().join(r"cfg\autoexec.cfg")).ok();
    let (lines, dropped) = autoexec::filter("fps_max 400\nbind x y\n");
    assert_eq!((lines.len(), dropped), (1, 1));
    let out = autoexec::merge(existing.as_deref(), &lines);
    assert!(out.contains("fps_max 400"));
    // Removing the block gives back exactly what was there.
    assert_eq!(autoexec::merge(Some(&out), &[]), existing.unwrap_or_default());
}

#[test]
#[ignore]
fn game_build_and_console_log() {
    let build = paths::game_build(&citadel().join("gameinfo.gi"));
    println!("Steam build id: {build:?}");
    assert!(build.is_some());
    if let Ok(bytes) = fs::read(citadel().join("console.log")) {
        let reports = perf::parse(&String::from_utf8_lossy(&bytes));
        println!("console.log: {} performance report(s)", reports.len());
    }
}

#[test]
#[ignore]
fn list_engine_changes_in_the_installed_file() {
    let gi = read(citadel().join("gameinfo.gi"));
    let cache = tempfile::tempdir().unwrap();
    let stock_text = stock::load(cache.path(), Some(&gi));
    for c in ranked::guarded_changes(&stock_text, &gi).unwrap() {
        println!("{} : {:?} -> {:?}", c.key, c.before, c.after);
    }
}
