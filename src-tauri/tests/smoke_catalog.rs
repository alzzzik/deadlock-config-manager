//! Checks that every repo and GameBanana config in the built-in list still downloads and builds a plan.
//! Needs network (and GitHub quota), so it is ignored by default:
//! `cargo test --test smoke_catalog -- --ignored --nocapture`

use cfghub::{apply, gamebanana, packs, store, video};

#[test]
#[ignore]
fn every_catalog_entry_is_readable() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let keys: Vec<String> = video::DEFAULT_PRESERVED.iter().map(|s| s.to_string()).collect();
    let mut failures = Vec::new();

    for entry in store::built_in().entries {
        match entry {
            store::Entry::Repo { repo, .. } => {
                let group = packs::list_packs(&repo, &cache);
                if group.packs.is_empty() {
                    failures.push(format!("{repo}: {:?}", group.error));
                }
                for pack in &group.packs {
                    let plan = packs::fetch_pack(&repo, pack, &cache)
                        .and_then(|f| apply::build_plan(&f.files, None, apply::VideoInput::Create, &keys));
                    match plan {
                        Ok(_) => println!("ok   {repo} / {}", pack.name),
                        Err(e) => failures.push(format!("{repo} / {}: {e}", pack.name)),
                    }
                }
            }
            store::Entry::Gamebanana { id, title, .. } => {
                let plan = gamebanana::fetch_info(id)
                    .and_then(|info| gamebanana::fetch_config(&info))
                    .and_then(|(files, _, _)| apply::build_plan(&files, None, apply::VideoInput::Create, &keys));
                match plan {
                    Ok(_) => println!("ok   gamebanana {id} {title}"),
                    Err(e) => failures.push(format!("gamebanana {id} {title}: {e}")),
                }
            }
            store::Entry::Code { .. } => {}
        }
    }
    assert!(failures.is_empty(), "unreadable entries:\n{}", failures.join("\n"));
}
