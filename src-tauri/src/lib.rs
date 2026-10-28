pub mod apply;
pub mod autoexec;
pub mod backup;
pub mod diag;
pub mod diff;
pub mod github;
mod commands;
pub mod convars;
pub mod fsops;
pub mod gamebanana;
pub mod gameinfo;
pub mod kv;
pub mod launch_guard;
pub mod library;
mod overview;
pub mod overlay;
pub mod overrides;
pub mod packs;
pub mod paths;
pub mod perf;
pub mod ranked;
pub mod repos;
pub mod report;
pub mod settings;
pub mod share;
pub mod stock;
pub mod store;
pub mod video;
mod watch;
pub mod writer;

use std::sync::Mutex;
use tauri::Manager;

/// Command-line flag used by "Start with Windows": start hidden in the tray.
const BACKGROUND_FLAG: &str = "--background";

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Tray icon with Open and Quit; a left click opens the window.
fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    let open = MenuItem::with_id(app, "open", "Open Deadlock Config Manager", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Deadlock Config Manager")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

pub fn run() {
    let dir = settings::app_dir();
    diag::init(&dir);
    let state = commands::AppState { settings: Mutex::new(settings::load(&dir)), dir, apply_lock: Mutex::new(()), perf_log: Default::default(), ranked_refused: Default::default() };
    tauri::Builder::default()
        // A second launch (e.g. from a deadlockcfg:// link) focuses this window; the deep-link
        // plugin forwards the link to the running app.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| show_main(app)))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec![BACKGROUND_FLAG])))
        .manage(state)
        .setup(|app| {
            // Registers deadlockcfg:// for this user (the installer does too; this covers portable copies).
            use tauri_plugin_deep_link::DeepLinkExt;
            if let Err(e) = app.deep_link().register_all() {
                diag::error(&format!("deep link registration: {e}"));
            }
            if let Err(e) = build_tray(app) {
                diag::error(&format!("tray icon: {e}"));
            }
            // The window starts hidden (tauri.conf.json) so a Windows-start launch can stay in the tray.
            let background = std::env::args().any(|a| a == BACKGROUND_FLAG);
            if !background {
                show_main(app.handle());
            }
            watch::start(app.handle().clone());
            Ok(())
        })
        // Closing the window exits, unless the player turned that off to keep the app in the tray (to collect
        // performance reports and watch for game updates).
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.app_handle().state::<commands::AppState>().snapshot().keeps_running() {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
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
            commands::get_tweaks,
            commands::set_tweak,
            commands::set_fix,
            commands::list_library,
            commands::save_current,
            commands::import_config,
            commands::export_config,
            commands::delete_config,
            commands::rename_config,
            commands::apply_library,
            commands::share_pack,
            commands::import_code,
            commands::get_automation,
            commands::set_automation,
            commands::update_report,
            commands::import_gamebanana,
            commands::get_store,
            commands::launch_issue,
            commands::dismiss_launch_issue,
            commands::get_perf,
            commands::clear_perf,
            commands::rename_perf,
            overview::toggle_pin,
            overview::finish_guide,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Deadlock Config Manager");
}
