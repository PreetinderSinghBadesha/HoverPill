//! Keyboard shortcuts that work from any app: one opens the notch, one opens its ChatGPT tab
//! ready to type a question. Which keys (or none) is chosen in Settings: Display for the first,
//! Accounts for the second.
//!
//! Two backends. Windows and X11 sessions use `tauri-plugin-global-shortcut`, an X key grab.
//! GNOME on Wayland doesn't let an XWayland client grab keys globally, so there the shortcuts
//! are installed as GNOME custom keybindings (`org.gnome.settings-daemon.plugins.media-keys`),
//! which run `hoverpill --summon=open|ask`; the second copy hands over to the running one.

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

impl Action {
    fn what(self) -> &'static str {
        match self {
            Action::Open => "open the notch",
            Action::Ask => "ask ChatGPT",
        }
    }

    /// The value of `--summon=...` for this action, which only the GNOME keybindings pass.
    #[cfg(target_os = "linux")]
    fn arg(self) -> &'static str {
        match self {
            Action::Open => "open",
            Action::Ask => "ask",
        }
    }
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
    #[cfg(target_os = "linux")]
    {
        if crate::shellext::gnome_wayland() && gnome::apply(app, &wanted) {
            // The X key grab can't fire under Wayland, but a registration left over from an X11
            // session must not stay behind and double-fire if the session type changed.
            let mut registered = REGISTERED.lock().unwrap();
            if !registered.is_empty() {
                let _ = app.global_shortcut().unregister_all();
                registered.clear();
            }
            return;
        }
        if crate::shellext::gnome_session() {
            // On GNOME's X11 session the plugin works; drop keybindings an earlier Wayland
            // session installed so the two don't both trigger.
            gnome::clear();
        }
    }
    apply_plugin(app, &wanted);
}

fn apply_plugin(app: &AppHandle, wanted: &[(&str, Action)]) {
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
        let what = action.what();
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

/// Takes HoverPill's GNOME custom shortcuts out again, on quit: they only work while it runs, and
/// left behind they'd point at a binary that may be uninstalled (or an AppImage that moved).
/// The next start installs them again.
pub fn remove_desktop_shortcuts() {
    #[cfg(target_os = "linux")]
    if crate::shellext::gnome_session() {
        gnome::clear();
    }
}

/// Handles `--summon=open` / `--summon=ask`, passed by a GNOME custom keybinding to a second
/// copy of HoverPill. Returns whether an argument matched (so the single-instance handover knows
/// not to open Settings instead).
pub fn summon_from_args(app: &AppHandle, args: &[String]) -> bool {
    let Some(action) = args.iter().find_map(|arg| arg.strip_prefix("--summon=")).and_then(|value| match value {
        "open" => Some(Action::Open),
        "ask" => Some(Action::Ask),
        _ => None,
    }) else {
        return false;
    };
    summon(app, action);
    true
}

/// Opens the notch with the keyboard, as if its question box had been clicked, and tells the
/// pill what to show. The pill decides whether that opens or closes it (a second press puts it
/// away); `notch_typing` keeps it up with the pointer elsewhere until the keyboard is released.
fn summon(app: &AppHandle, action: Action) {
    notch::notch_typing(true, app.clone(), app.state::<notch::HitState>());
    match action {
        Action::Open => events::send(app, "pill:summon", ()),
        Action::Ask => events::send(app, "pill:ask", ()),
    }
}

/// GNOME custom keybindings: the only way an unsandboxed app can have a global shortcut on
/// GNOME Wayland. Each one runs `hoverpill --summon=...`, so the running copy opens the notch.
/// They're only there while HoverPill runs: installed at start, removed on quit (see
/// `remove_desktop_shortcuts`), so none stay behind pointing at a binary that's gone.
#[cfg(target_os = "linux")]
mod gnome {
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::Mutex;

    use tauri::AppHandle;

    use super::{events, Action};

    const SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
    const CUSTOM: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
    const BASE: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings";

    /// What we installed, so a save that doesn't change the shortcuts does nothing.
    static APPLIED: Mutex<Vec<(String, Action)>> = Mutex::new(Vec::new());

    /// Values `gsettings get` prints, as paths. Handles both `[]` and `@as []`.
    fn listed_paths(value: &str) -> Vec<String> {
        let value = value.trim();
        let value = value.strip_prefix("@as").map(str::trim_start).unwrap_or(value);
        let inner = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')).unwrap_or("");
        inner
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(|item| item.trim_matches('\'').to_string())
            .collect()
    }

    /// The list literal `gsettings set` wants. `@as []` because an empty literal has no type.
    fn list_literal(paths: &[String]) -> String {
        if paths.is_empty() {
            "@as []".to_string()
        } else {
            format!("[{}]", paths.iter().map(|path| format!("'{path}'")).collect::<Vec<_>>().join(", "))
        }
    }

    fn path(action: Action) -> String {
        format!("{BASE}/hoverpill-{}/", action.arg())
    }

    fn ours(path: &str) -> bool {
        path.contains("/hoverpill-open/") || path.contains("/hoverpill-ask/")
    }

    /// Runs `gsettings`; `Some(stdout)` when it succeeded.
    fn gsettings(args: &[&str]) -> Option<String> {
        let out = match Command::new("gsettings").args(args).output() {
            Ok(out) => out,
            Err(e) => {
                eprintln!("hoverpill: can't run gsettings: {e}");
                return None;
            }
        };
        if !out.status.success() {
            eprintln!("hoverpill: gsettings {args:?} failed: {}", String::from_utf8_lossy(&out.stderr).trim());
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn set(schema: &str, key: &str, value: &str) -> bool {
        gsettings(&["set", schema, key, value]).is_some()
    }

    /// A string as a GVariant text literal: `gsettings set` parses its value, so a command with
    /// quotes or spaces in it has to be wrapped (and its own quotes escaped).
    fn string_literal(value: &str) -> String {
        let mut out = String::with_capacity(value.len() + 2);
        out.push('\'');
        for c in value.chars() {
            match c {
                '\'' => out.push_str("\\'"),
                '\\' => out.push_str("\\\\"),
                _ => out.push(c),
            }
        }
        out.push('\'');
        out
    }

    /// `Ctrl+Alt+G` as a GNOME accelerator: `<Control><Alt>g`.
    fn accelerator(keys: &str) -> Option<String> {
        let mut modifiers = String::new();
        let mut key = None;
        for token in keys.split('+').map(str::trim) {
            match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers.push_str("<Control>"),
                "alt" | "option" => modifiers.push_str("<Alt>"),
                "shift" => modifiers.push_str("<Shift>"),
                "super" | "cmd" | "command" | "meta" => modifiers.push_str("<Super>"),
                "" => return None,
                other => {
                    key = Some(match other {
                        "space" => "space".to_string(),
                        "left" | "arrowleft" => "Left".to_string(),
                        "right" | "arrowright" => "Right".to_string(),
                        "up" | "arrowup" => "Up".to_string(),
                        "down" | "arrowdown" => "Down".to_string(),
                        "esc" | "escape" => "Escape".to_string(),
                        "enter" | "return" => "Return".to_string(),
                        "tab" => "Tab".to_string(),
                        "backspace" => "BackSpace".to_string(),
                        "delete" => "Delete".to_string(),
                        "home" => "Home".to_string(),
                        "end" => "End".to_string(),
                        "pageup" => "Page_Up".to_string(),
                        "pagedown" => "Page_Down".to_string(),
                        single if single.chars().count() == 1 => single.to_string(),
                        function if function.starts_with('f') && function[1..].parse::<u8>().is_ok() => function.to_ascii_uppercase(),
                        _ => return None,
                    })
                }
            }
        }
        Some(format!("{modifiers}{}", key?))
    }

    /// GNOME's own bindings and other apps' custom shortcuts. The desktop keeps a grab on those,
    /// so ours would never fire (and Settings would show a conflict); check before installing.
    fn taken(accel: &str) -> bool {
        let Some(wanted) = parse_accelerator(accel) else { return false };
        let clashes = |out: &str| quoted(out).iter().any(|value| parse_accelerator(value).as_ref() == Some(&wanted));
        const SCHEMAS: [&str; 7] = [
            "org.gnome.desktop.wm.keybindings",
            "org.gnome.shell.keybindings",
            "org.gnome.mutter.keybindings",
            "org.gnome.mutter.wayland.keybindings",
            "org.gnome.settings-daemon.plugins.media-keys",
            "org.gnome.settings-daemon.plugins.power",
            "org.gnome.desktop.interface",
        ];
        if SCHEMAS.iter().any(|schema| gsettings(&["list-recursively", schema]).is_some_and(|out| clashes(&out))) {
            return true;
        }
        let Some(current) = gsettings(&["get", SCHEMA, "custom-keybindings"]) else { return false };
        listed_paths(&current).into_iter().filter(|path| !ours(path)).any(|path| {
            let schema = format!("{CUSTOM}:{path}");
            gsettings(&["get", &schema, "binding"]).is_some_and(|value| clashes(&value))
        })
    }

    /// Modifier bits for `parse_accelerator`.
    const CONTROL: u8 = 1;
    const ALT: u8 = 2;
    const SHIFT: u8 = 4;
    const SUPER: u8 = 8;

    /// An accelerator as its modifiers and key, so spellings GNOME treats as the same keys
    /// compare equal: `<Control>`, `<Ctrl>`, `<Ctl>` and `<Primary>` are all Ctrl, `<Mod1>` is
    /// Alt, `<Mod4>` is Super, and letters ignore case. `None` for anything that isn't one
    /// (a font name in the same schema, say).
    pub(super) fn parse_accelerator(text: &str) -> Option<(u8, String)> {
        let mut rest = text.trim();
        let mut modifiers = 0u8;
        while let Some(after) = rest.strip_prefix('<') {
            let (name, tail) = after.split_once('>')?;
            modifiers |= match name.to_ascii_lowercase().as_str() {
                "control" | "ctrl" | "ctl" | "primary" => CONTROL,
                "alt" | "mod1" => ALT,
                "shift" => SHIFT,
                "super" | "mod4" => SUPER,
                // `<Meta>`, `<Hyper>` and friends: keep them distinct from everything above.
                _ => 16,
            };
            rest = tail;
        }
        if rest.is_empty() || rest.contains(char::is_whitespace) || rest.contains(['<', '>']) {
            return None;
        }
        let key = if rest.chars().count() == 1 { rest.to_lowercase() } else { rest.to_string() };
        Some((modifiers, key))
    }

    /// The single-quoted strings in `gsettings` output (`['<Super>Home', '<Alt>F1']`).
    fn quoted(out: &str) -> Vec<String> {
        out.split('\'').skip(1).step_by(2).map(str::to_string).collect()
    }

    fn command(action: Action) -> Option<String> {
        Some(format!("\"{}\" --summon={}", launcher()?.display(), action.arg()))
    }

    /// What a shortcut should run. Inside an AppImage, `current_exe()` is the temporary mount
    /// (`/tmp/.mount_…/usr/bin/hoverpill`), which is gone once HoverPill restarts; the AppImage
    /// itself is in `APPIMAGE`.
    fn launcher() -> Option<PathBuf> {
        launcher_from(std::env::var_os("APPIMAGE"), std::env::current_exe().ok())
    }

    pub(super) fn launcher_from(appimage: Option<std::ffi::OsString>, exe: Option<PathBuf>) -> Option<PathBuf> {
        appimage.filter(|path| !path.is_empty()).map(PathBuf::from).or(exe)
    }

    /// Installs (or updates) the custom keybindings. `false` when `gsettings` isn't usable, so
    /// the caller can fall back to the X11 grab.
    pub fn apply(app: &AppHandle, wanted: &[(&str, Action)]) -> bool {
        let mut desired: Vec<(String, Action)> = wanted
            .iter()
            .filter(|(keys, _)| !keys.is_empty())
            .map(|(keys, action)| (keys.to_string(), *action))
            .collect();
        if desired.len() == 2 && desired[0].0 == desired[1].0 {
            events::notify(
                app,
                "Shortcut used twice",
                &format!("{} can't both open the notch and ask ChatGPT. Pick another in HoverPill's Settings.", desired[0].0),
            );
            desired.truncate(1);
        }
        {
            let applied = APPLIED.lock().unwrap();
            if *applied == desired {
                return true;
            }
        }

        let Some(current) = gsettings(&["get", SCHEMA, "custom-keybindings"]) else { return false };

        let mut active: Vec<(String, String, Action)> = Vec::new();
        for (keys, action) in &desired {
            let Some(accel) = accelerator(keys) else {
                eprintln!("hoverpill: can't read {keys} as a GNOME shortcut");
                continue;
            };
            if taken(&accel) {
                events::notify(
                    app,
                    "Shortcut not set",
                    &format!("{keys} is already used by another app, so it can't {}. Pick another in HoverPill's Settings.", action.what()),
                );
                continue;
            }
            active.push((path(*action), accel, *action));
        }

        let mut paths: Vec<String> = listed_paths(&current).into_iter().filter(|path| !ours(path)).collect();
        paths.extend(active.iter().map(|(path, _, _)| path.clone()));
        if listed_paths(&current) != paths && !set(SCHEMA, "custom-keybindings", &list_literal(&paths)) {
            return false;
        }

        for (path, accel, action) in &active {
            let schema = format!("{CUSTOM}:{path}");
            let Some(command) = command(*action) else { return false };
            let name = format!("HoverPill: {}", if *action == Action::Open { "Open the notch" } else { "Ask ChatGPT" });
            if !set(&schema, "name", &string_literal(&name))
                || !set(&schema, "command", &string_literal(&command))
                || !set(&schema, "binding", &string_literal(accel))
            {
                return false;
            }
        }

        *APPLIED.lock().unwrap() = desired;
        true
    }

    /// Removes our keybindings, leaving anyone else's custom shortcuts untouched.
    pub fn clear() {
        APPLIED.lock().unwrap().clear();
        let Some(current) = gsettings(&["get", SCHEMA, "custom-keybindings"]) else { return };
        let paths = listed_paths(&current);
        let kept: Vec<String> = paths.iter().filter(|path| !ours(path)).cloned().collect();
        if kept.len() != paths.len() && !set(SCHEMA, "custom-keybindings", &list_literal(&kept)) {
            eprintln!("hoverpill: can't remove its GNOME keybindings");
        }
    }

    #[cfg(test)]
    mod tests {
        use std::path::PathBuf;

        use super::{accelerator, launcher_from, list_literal, listed_paths, parse_accelerator, quoted, string_literal};

        #[test]
        fn shortcuts_run_the_appimage_not_its_mount() {
            let mount = Some(PathBuf::from("/tmp/.mount_HoverPabc/usr/bin/hoverpill"));
            assert_eq!(
                launcher_from(Some("/home/me/Apps/HoverPill.AppImage".into()), mount.clone()),
                Some(PathBuf::from("/home/me/Apps/HoverPill.AppImage"))
            );
            assert_eq!(launcher_from(Some("".into()), mount.clone()), mount);
            assert_eq!(launcher_from(None, Some(PathBuf::from("/usr/bin/hoverpill"))), Some(PathBuf::from("/usr/bin/hoverpill")));
        }

        #[test]
        fn the_same_keys_match_however_gnome_spells_them() {
            let ours = parse_accelerator("<Control><Alt>g");
            assert_eq!(parse_accelerator("<Primary><Alt>g"), ours);
            assert_eq!(parse_accelerator("<Ctrl><Alt>G"), ours);
            assert_eq!(parse_accelerator("<Alt><Control>g"), ours);
            assert_eq!(parse_accelerator("<Ctl><Mod1>g"), ours);
            assert_ne!(parse_accelerator("<Control><Shift>g"), ours);
            assert_eq!(parse_accelerator("<Super>space"), parse_accelerator("<Mod4>space"));
            assert_ne!(parse_accelerator("<Super>space"), parse_accelerator("<Super>Space_L"));
            assert_eq!(parse_accelerator("Ubuntu Sans 11"), None);
            assert_eq!(parse_accelerator(""), None);
        }

        #[test]
        fn finds_the_bindings_in_gsettings_output() {
            let out = "org.gnome.desktop.wm.keybindings switch-to-workspace-1 ['<Super>Home', '<Alt>F1']\norg.gnome.desktop.wm.keybindings close @as []";
            assert_eq!(quoted(out), vec!["<Super>Home".to_string(), "<Alt>F1".to_string()]);
            assert_eq!(quoted("'<Primary><Alt>t'"), vec!["<Primary><Alt>t".to_string()]);
        }

        #[test]
        fn turns_settings_shortcuts_into_gnome_accelerators() {
            assert_eq!(accelerator("Ctrl+Alt+G").as_deref(), Some("<Control><Alt>g"));
            assert_eq!(accelerator("Super+Alt+Space").as_deref(), Some("<Super><Alt>space"));
            assert_eq!(accelerator("Ctrl+Shift+Space").as_deref(), Some("<Control><Shift>space"));
            assert_eq!(accelerator("Alt+Space").as_deref(), Some("<Alt>space"));
            assert_eq!(accelerator("F11"), Some("F11".to_string()));
            assert_eq!(accelerator("Ctrl+"), None);
        }

        #[test]
        fn reads_and_writes_gsettings_lists() {
            assert_eq!(listed_paths("@as []"), Vec::<String>::new());
            assert_eq!(listed_paths("[]"), Vec::<String>::new());
            assert_eq!(listed_paths("['/a/', '/b/']"), vec!["/a/".to_string(), "/b/".to_string()]);
            assert_eq!(list_literal(&[]), "@as []");
            assert_eq!(list_literal(&["/a/".to_string()]), "['/a/']");
        }

        #[test]
        fn quotes_gvariant_strings() {
            assert_eq!(string_literal("<Control><Alt>g"), "'<Control><Alt>g'");
            assert_eq!(string_literal("\"/opt/a b/hoverpill\" --summon=open"), "'\"/opt/a b/hoverpill\" --summon=open'");
            assert_eq!(string_literal("it's"), "'it\\'s'");
        }
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
