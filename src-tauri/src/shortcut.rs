//! Keyboard shortcuts that work from any app: one opens the notch, one opens its ChatGPT tab
//! ready to type a question. Which keys (or none) is chosen in Settings: Display for the first,
//! Accounts for the second.

use std::sync::Mutex;

use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::{events, notch};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    /// Open the notch where it was, with the keyboard.
    Open,
    /// Open the ChatGPT tab with the cursor in the question box.
    Ask,
}

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let action = REGISTERED.lock().unwrap().iter().find(|r| r.id == shortcut.id()).map(|r| r.action);
            if let Some(action) = action {
                summon(app, action);
            }
        })
        .build()
}

struct Registered {
    keys: String,
    action: Action,
    id: u32,
}

/// What's registered now, so saving unrelated settings doesn't re-register anything.
static REGISTERED: Mutex<Vec<Registered>> = Mutex::new(Vec::new());

/// Registers the two shortcuts (like "Super+Alt+Space" and "Ctrl+Alt+G") in place of the
/// previous ones; an empty one is off.
pub fn apply(app: &AppHandle, open: &str, ask: &str) {
    let wanted = [(open, Action::Open), (ask, Action::Ask)];
    let mut registered = REGISTERED.lock().unwrap();
    let current: Vec<(&str, Action)> = registered.iter().map(|r| (r.keys.as_str(), r.action)).collect();
    let next: Vec<(&str, Action)> = wanted.iter().copied().filter(|(keys, _)| !keys.is_empty()).collect();
    if current == next {
        return;
    }
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    registered.clear();
    for (keys, action) in next {
        let what = if action == Action::Open { "open the notch" } else { "ask ChatGPT" };
        if registered.iter().any(|r| r.keys == keys) {
            events::notify(app, "Shortcut used twice", &format!("{keys} can't both open the notch and ask ChatGPT. Pick another in HoverPill's Settings."));
            continue;
        }
        let shortcut = match keys.parse::<Shortcut>() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("hoverpill: can't read {keys} as a shortcut: {e}");
                continue;
            }
        };
        match shortcuts.register(shortcut) {
            Ok(()) => registered.push(Registered { keys: keys.to_string(), action, id: shortcut.id() }),
            Err(e) => {
                eprintln!("hoverpill: can't use {keys} to {what}: {e}");
                events::notify(app, "Shortcut not set", &format!("{keys} is already used by another app, so it can't {what}. Pick another in HoverPill's Settings."));
            }
        }
    }
}

/// Opens the notch with the keyboard, as if its question box had been clicked, and tells the
/// pill what to show.
fn summon(app: &AppHandle, action: Action) {
    notch::notch_typing(true, app.clone(), app.state::<notch::HitState>());
    match action {
        Action::Open => events::send(app, "pill:summon", ()),
        Action::Ask => events::send(app, "pill:ask", ()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every choice Settings offers (see `OPEN_SHORTCUTS` in src/settings/SettingsApp.tsx and
    /// `SHORTCUTS` in src/settings/AccountsTab.tsx) must parse.
    #[test]
    fn the_offered_shortcuts_parse() {
        for keys in [
            "Super+Alt+Space",
            "Ctrl+Alt+N",
            "Ctrl+Alt+H",
            "Ctrl+Alt+G",
            "Ctrl+Shift+Space",
            "Alt+Shift+Space",
            "Ctrl+Alt+Space",
            "Alt+Space",
        ] {
            assert!(keys.parse::<Shortcut>().is_ok(), "{keys}");
        }
    }
}
