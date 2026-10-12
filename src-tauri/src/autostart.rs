//! "Start at login": a per-user Run registry value on Windows, a `~/.config/autostart` entry on
//! Linux (via tauri-plugin-autostart). Off until the user turns it on from the tray.

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

use crate::events;

pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_autostart::Builder::new().app_name("HoverPill").build()
}

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

/// Flips the setting and returns the resulting state.
pub fn toggle(app: &AppHandle) -> bool {
    let manager = app.autolaunch();
    let result = if is_enabled(app) {
        manager.disable()
    } else if cfg!(debug_assertions) {
        // A debug build only works with the Vite dev server running; registering it would leave
        // a blank pill at every login.
        events::notify(app, "Start at login needs a release build", "Build with `npm run tauri build` and enable it there.");
        Ok(())
    } else {
        manager.enable()
    };
    if let Err(e) = result {
        events::notify(app, "Couldn't change start at login", &e.to_string());
    }
    is_enabled(app)
}
