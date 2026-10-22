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
pub mod library;
mod overview;
pub mod overrides;
pub mod packs;
pub mod paths;
pub mod ranked;
pub mod repos;
pub mod settings;
pub mod share;
pub mod stock;
pub mod video;
pub mod writer;

use std::sync::Mutex;
use tauri::Manager;

pub fn run() {
    let dir = settings::app_dir();
    diag::init(&dir);
    let state = commands::AppState { settings: Mutex::new(settings::load(&dir)), dir };
    tauri::Builder::default()
        // A second launch (e.g. from a deadlockcfg:// link) focuses this window; the deep-link
        // plugin forwards the link to the running app.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(state)
        .setup(|app| {
            // Registers deadlockcfg:// for this user (the installer does too; this covers portable copies).
            use tauri_plugin_deep_link::DeepLinkExt;
            if let Err(e) = app.deep_link().register_all() {
                diag::error(&format!("deep link registration: {e}"));
            }
            Ok(())
        })
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
            commands::get_tweaks,
            commands::save_tweaks,
            commands::import_tweaks,
            commands::export_tweaks,
            commands::parse_tweak_lines,
            commands::format_tweak_lines,
            commands::list_library,
            commands::save_current,
            commands::import_config,
            commands::export_config,
            commands::delete_config,
            commands::rename_config,
            commands::apply_library,
            commands::share_pack,
            commands::share_config,
            commands::import_code,
            overview::toggle_pin,
            overview::finish_guide,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Deadlock Config Manager");
}
