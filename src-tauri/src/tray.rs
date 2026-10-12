//! Tray icon: the app has no taskbar entry, so this is how you quit it, toggle start at login,
//! and install the Claude Code hook. It can also fire demo events to exercise every pill state.

use std::path::PathBuf;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{App, AppHandle, Manager};

use crate::autostart;
use crate::broker::{install, Broker};
use crate::events::{self, NotificationItem, PermissionRequest};

pub fn init(app: &App) -> tauri::Result<()> {
    let start_at_login =
        CheckMenuItem::with_id(app, "autostart", "Start at login", true, autostart::is_enabled(app.handle()), None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &start_at_login,
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
        .on_menu_event(move |app, event| match event.id.as_ref() {
            // The menu flips the tick itself on click; overwrite it with what actually happened.
            "autostart" => {
                let _ = start_at_login.set_checked(autostart::toggle(app));
            }
            "hook-install" => install_hook(app),
            "hook-remove" => remove_hook(app),
            "demo-permission" => demo_permission(app),
            "demo-notification" => demo_notification(app),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn claude_settings(app: &AppHandle) -> Option<PathBuf> {
    app.path().home_dir().ok().map(|home| home.join(".claude").join("settings.json"))
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
