//! Tray icon: the app has no taskbar entry, so this is how you open Settings, quit, toggle
//! start at login, and install the Claude Code hook. It can also fire demo events to exercise
//! every pill state.

use std::path::PathBuf;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{App, AppHandle, Manager};

use crate::broker::{install, Broker};
use crate::{autostart, settings};
use crate::events::{self, NotificationItem, PermissionRequest};

pub fn init(app: &App) -> tauri::Result<()> {
    let start_at_login =
        CheckMenuItem::with_id(app, "autostart", "Start at login", true, autostart::is_enabled(app.handle()), None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?,
            &start_at_login,
            &MenuItem::with_id(app, "update", "Check for updates", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "hook-install", "Install Claude Code hook", true, None::<&str>)?,
            &MenuItem::with_id(app, "hook-remove", "Remove Claude Code hook", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "demo-permission", "Demo: Claude permission request", true, None::<&str>)?,
            &MenuItem::with_id(app, "demo-notification", "Demo: notification", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "Quit HoverPill", true, None::<&str>)?,
        ],
    )?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("HoverPill")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "settings" => settings::open_window(app),
            // The menu has already flipped the tick; `set` puts it back to what really happened.
            "autostart" => {
                if let Err(e) = autostart::set(app, !autostart::is_enabled(app)) {
                    events::notify(app, "Couldn't change start at login", &e);
                }
            }
            "hook-install" => install_hook(app),
            "hook-remove" => remove_hook(app),
            "demo-permission" => demo_permission(app),
            "demo-notification" => demo_notification(app),
            "update" => crate::updater::check_by_hand(app),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    app.manage(autostart::TrayCheck(start_at_login));
    Ok(())
}

fn claude_settings(app: &AppHandle) -> Option<PathBuf> {
    app.path().home_dir().ok().map(|home| home.join(".claude").join("settings.json"))
}

/// For Settings: whether Claude Code's requests come to the notch.
#[tauri::command]
pub fn claude_hook_installed(app: AppHandle) -> bool {
    claude_settings(&app).is_some_and(|path| install::is_installed(&path))
}

/// For Settings: installs or removes the hook, and says whether it's installed afterwards.
#[tauri::command]
pub fn set_claude_hook(app: AppHandle, installed: bool) -> Result<bool, String> {
    let path = claude_settings(&app).ok_or("Home directory not found.")?;
    if installed {
        install::install(&path, app.state::<Broker>().token())?;
    } else {
        install::uninstall(&path)?;
    }
    Ok(install::is_installed(&path))
}

fn install_hook(app: &AppHandle) {
    let Some(path) = claude_settings(app) else {
        return events::notify(app, "Couldn't install hook", "Home directory not found.");
    };
    match install::install(&path, app.state::<Broker>().token()) {
        Ok(()) => events::notify(app, "Claude Code hook installed", "New Claude Code sessions will ask through HoverPill."),
        Err(e) => events::notify(app, "Couldn't install hook", &e),
    }
}

fn remove_hook(app: &AppHandle) {
    let Some(path) = claude_settings(app) else {
        return events::notify(app, "Couldn't remove hook", "Home directory not found.");
    };
    match install::uninstall(&path) {
        Ok(()) => events::notify(app, "Claude Code hook removed", "Claude Code will ask in the terminal again."),
        Err(e) => events::notify(app, "Couldn't remove hook", &e),
    }
}

fn demo_permission(app: &AppHandle) {
    events::send(
        app,
        "pill:permission",
        PermissionRequest {
            id: events::next_id("demo"),
            tool: "Bash".into(),
            command: "git push --force-with-lease origin feature/hoverpill".into(),
            cwd: Some("D:/projects/HoverPill".into()),
            permission_suggestions: None,
        },
    );
}

fn demo_notification(app: &AppHandle) {
    events::send(
        app,
        "pill:notification",
        NotificationItem {
            id: events::next_id("demo"),
            app: "Discord".into(),
            title: "#general".into(),
            body: "anyone up for a match tonight?".into(),
            icon: None,
        },
    );
}
