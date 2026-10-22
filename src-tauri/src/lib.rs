pub mod apply;
pub mod backup;
pub mod diag;
pub mod diff;
pub mod github;
mod commands;
pub mod convars;
pub mod fsops;
pub mod gameinfo;
pub mod kv;
mod overview;
pub mod packs;
pub mod paths;
pub mod ranked;
pub mod repos;
pub mod settings;
pub mod stock;
pub mod video;
pub mod writer;

use std::sync::Mutex;

pub fn run() {
    let dir = settings::app_dir();
    diag::init(&dir);
    let state = commands::AppState { settings: Mutex::new(settings::load(&dir)), dir };
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            overview::get_overview,
            overview::set_account,
            overview::set_path,
            overview::save_settings,
            overview::list_packs,
            overview::cached_packs,
            overview::add_repo,
            overview::remove_repo,
            overview::move_repo,
            commands::apply_pack,
            commands::apply_local,
            commands::list_backups,
            commands::restore_backup,
            commands::open_location,
            commands::open_github,
            commands::get_diagnostics,
            commands::install_kind,
            commands::make_ranked_safe,
            commands::check_packs,
            overview::toggle_pin,
            overview::finish_guide,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Deadlock Config Manager");
}
