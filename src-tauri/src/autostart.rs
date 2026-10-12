//! "Start at login": a per-user Run registry value on Windows, a `~/.config/autostart` entry on
//! Linux (via tauri-plugin-autostart). Off until the user turns it on from the tray or Settings.

use tauri::menu::CheckMenuItem;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_autostart::Builder::new().app_name("HoverPill").build()
}

/// The tray's check item, so a change made in Settings also moves the tick.
pub struct TrayCheck(pub CheckMenuItem<Wry>);

pub fn is_enabled(app: &AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

/// The login entry stores the executable's path. If HoverPill was moved or reinstalled, point
/// the entry at this copy.
pub fn refresh(app: &AppHandle) {
    if !cfg!(debug_assertions) && is_enabled(app) {
        if let Err(e) = app.autolaunch().enable() {
            eprintln!("hoverpill: couldn't refresh the login entry: {e}");
        }
    }
}

/// Turns start at login on or off and returns the resulting state.
pub fn set(app: &AppHandle, enabled: bool) -> Result<bool, String> {
    let result = if !enabled {
        app.autolaunch().disable().map_err(|e| e.to_string())
    } else if cfg!(debug_assertions) {
        // A debug build only works with the Vite dev server running; registering it would leave
        // a blank pill at every login.
        Err("Start at login needs a release build (npm run tauri build).".into())
    } else {
        app.autolaunch().enable().map_err(|e| e.to_string())
    };
    let now = is_enabled(app);
    if let Some(check) = app.try_state::<TrayCheck>() {
        let _ = check.0.set_checked(now);
    }
    result.map(|()| now)
}

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> bool {
    is_enabled(&app)
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    set(&app, enabled)
}
