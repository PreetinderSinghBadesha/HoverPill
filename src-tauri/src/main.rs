// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod appbar;
mod autostart;
mod broker;
mod events;
mod notch;
mod settings;
mod tray;

use tauri::Manager;

use settings::{RecentApps, SettingsStore};

fn main() {
    #[cfg(target_os = "linux")]
    notch::linux_preflight();

    // If this turns out to be a second copy, it will hand over to the running one, which then
    // opens Settings. Windows only lets the process the user just launched take the foreground,
    // so pass that permission on; otherwise Settings opens behind everything else.
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};
        AllowSetForegroundWindow(ASFW_ANY);
    }

    tauri::Builder::default()
        // Must be first: a second copy (e.g. launched by hand after starting at login) exits
        // here instead of opening another pill and fighting over the broker port. Launching
        // HoverPill again is a natural way to look for its settings, so open them.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| settings::open_window(app)))
        .plugin(autostart::plugin())
        .manage(notch::HitState::default())
        .manage(events::Outbox::default())
        .manage(RecentApps::default())
        .invoke_handler(tauri::generate_handler![
            notch::set_hit_rect,
            notch::list_monitors,
            events::resolve_permission,
            events::media_command,
            events::current_media,
            events::pill_ready,
            settings::get_settings,
            settings::open_settings,
            settings::set_settings,
            settings::recent_notification_apps,
            settings::media_players,
            autostart::get_autostart,
            autostart::set_autostart,
        ])
        .setup(|app| {
            // Placement, media and notifications all read settings, so load them first.
            app.manage(SettingsStore::load(app.handle())?);

            let pill = app
                .get_webview_window(notch::LABEL)
                .ok_or("pill window missing from tauri.conf.json")?;
            notch::place_top_center(&pill)?;
            pill.set_ignore_cursor_events(true)?;
            pill.show()?;
            notch::spawn_hit_test_loop(pill);

            app.manage(broker::Broker::load(app.handle())?);
            broker::start(app.handle().clone());

            let handle = app.handle().clone();
            let media = hoverpill_media::Media::spawn(move |now| {
                let enabled = handle.state::<SettingsStore>().get().media.enabled;
                events::send(&handle, "pill:media", now.filter(|_| enabled).map(events::MediaState::from));
            });
            media.set_ignored(app.state::<SettingsStore>().get().media.ignored_players);
            app.manage(media);

            let handle = app.handle().clone();
            hoverpill_notify::spawn(move |event| match event {
                hoverpill_notify::Event::Notification(n) => {
                    handle.state::<RecentApps>().record(&n.app);
                    let settings = handle.state::<SettingsStore>().get();
                    if settings.notifications.enabled && !settings.is_app_blocked(&n.app) {
                        events::send(&handle, "pill:notification", events::NotificationItem::from(n));
                    }
                }
                hoverpill_notify::Event::Unavailable(reason) => events::notify(&handle, "Notifications unavailable", &reason),
            });

            autostart::refresh(app.handle());
            tray::init(app)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building HoverPill")
        .run(|_app, event| {
            // Give the reserved top strip back, or Windows keeps it until it notices we're gone.
            if let tauri::RunEvent::Exit = event {
                appbar::release();
            }
        });
}
