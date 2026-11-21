//! The tray icon's menu: what's applied, the pinned configs (each opens its preview in the window), Open and Quit.
//! The window sends the applied config and the pins whenever they change, so the menu is rebuilt from that.

use serde::Deserialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, Emitter, Manager, Runtime};

pub const TRAY_ID: &str = "main";
/// Sent to the window with a pinned config's key when it's picked in the tray menu.
pub const TRAY_APPLY: &str = "tray-apply";
const PIN_PREFIX: &str = "pin:";
/// A longer menu gets hard to scan; pins past this are left out of the tray (they stay in the window).
const MAX_PINS: usize = 10;

#[derive(Debug, Clone, Deserialize)]
pub struct TrayConfig {
    /// `repo#id`, the same key the pin list uses.
    pub key: String,
    pub name: String,
}

/// The pinned config's key for a menu item id, if the item is a pinned config.
pub fn pin_key(menu_id: &str) -> Option<&str> {
    menu_id.strip_prefix(PIN_PREFIX).filter(|k| !k.is_empty())
}

pub fn menu<R: Runtime, M: Manager<R>>(app: &M, status: Option<&str>, pins: &[TrayConfig]) -> tauri::Result<Menu<R>> {
    let menu = Menu::new(app)?;
    if let Some(s) = status {
        menu.append(&MenuItem::with_id(app, "status", s, false, None::<&str>)?)?;
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    if !pins.is_empty() {
        menu.append(&MenuItem::with_id(app, "pinned", "Pinned configs", false, None::<&str>)?)?;
        for p in pins.iter().take(MAX_PINS) {
            menu.append(&MenuItem::with_id(app, format!("{PIN_PREFIX}{}", p.key), &p.name, true, None::<&str>)?)?;
        }
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    menu.append(&MenuItem::with_id(app, "open", "Open Deadlock Config Manager", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?)?;
    Ok(menu)
}

pub fn on_menu_event(app: &AppHandle, id: &str) {
    match id {
        "open" => crate::show_main(app),
        "quit" => app.exit(0),
        _ => {
            if let Some(key) = pin_key(id) {
                crate::show_main(app);
                let _ = app.emit(TRAY_APPLY, key.to_string());
            }
        }
    }
}

/// Rebuilds the tray menu with the applied config's status line and the pinned configs.
#[tauri::command]
pub fn set_tray_configs(app: AppHandle, status: Option<String>, pins: Vec<TrayConfig>) -> Result<(), String> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(()); // The tray icon failed to start; that's already logged.
    };
    let m = menu(&app, status.as_deref(), &pins).map_err(|e| e.to_string())?;
    tray.set_menu(Some(m)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_ids_carry_the_config_key() {
        assert_eq!(pin_key("pin:alzzzik/alz-deadlock-cfg#alz"), Some("alzzzik/alz-deadlock-cfg#alz"));
        assert_eq!(pin_key("pin:"), None);
        assert_eq!(pin_key("open"), None);
        assert_eq!(pin_key("status"), None);
    }
}
