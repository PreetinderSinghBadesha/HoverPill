//! The keyboard shortcut that opens the notch's ChatGPT tab, ready to type a question. It works
//! from any app; which keys (or none) is chosen in Settings, Accounts.

use std::sync::Mutex;

use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::{events, notch};

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                ask(app);
            }
        })
        .build()
}

/// The keys registered now, so saving unrelated settings doesn't re-register them.
static CURRENT: Mutex<Option<String>> = Mutex::new(None);

/// Registers `keys` (like "Ctrl+Alt+G") in place of the previous shortcut; empty turns it off.
pub fn apply(app: &AppHandle, keys: &str) {
    let mut current = CURRENT.lock().unwrap();
    if current.as_deref() == Some(keys) {
        return;
    }
    *current = Some(keys.to_string());
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    if keys.is_empty() {
        return;
    }
    let registered = keys.parse::<Shortcut>().map_err(|e| e.to_string()).and_then(|s| shortcuts.register(s).map_err(|e| e.to_string()));
    if let Err(e) = registered {
        eprintln!("hoverpill: can't use {keys} as the ChatGPT shortcut: {e}");
        events::notify(app, "ChatGPT shortcut not set", &format!("{keys} is already used by another app. Pick another in HoverPill's Settings, Accounts."));
    }
}

/// Opens the notch with the keyboard, as if its question box had been clicked, and tells the
/// pill to show the ChatGPT tab and put the cursor in it.
fn ask(app: &AppHandle) {
    notch::notch_typing(true, app.clone(), app.state::<notch::HitState>());
    events::send(app, "pill:ask", ());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every choice Settings offers (see `SHORTCUTS` in src/settings/AccountsTab.tsx) must parse.
    #[test]
    fn the_offered_shortcuts_parse() {
        for keys in ["Ctrl+Alt+G", "Ctrl+Shift+Space", "Alt+Shift+Space", "Ctrl+Alt+Space", "Alt+Space"] {
            assert!(keys.parse::<Shortcut>().is_ok(), "{keys}");
        }
    }
}
