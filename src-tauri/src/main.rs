// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod broker;
mod events;
mod notch;
mod tray;

use tauri::Manager;

fn main() {
    #[cfg(target_os = "linux")]
    notch::linux_preflight();

    tauri::Builder::default()
        .manage(notch::HitState::default())
        .manage(events::Outbox::default())
        .invoke_handler(tauri::generate_handler![
            notch::set_hit_rect,
            events::resolve_permission,
            events::media_command,
            events::current_media,
            events::pill_ready,
        ])
        .setup(|app| {
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
            app.manage(hoverpill_media::Media::spawn(move |now| {
                events::send(&handle, "pill:media", now.map(events::MediaState::from));
            }));
            let handle = app.handle().clone();
            hoverpill_notify::spawn(move |event| match event {
                hoverpill_notify::Event::Notification(n) => {
                    events::send(&handle, "pill:notification", events::NotificationItem::from(n))
                }
                hoverpill_notify::Event::Unavailable(reason) => events::notify(&handle, "Notifications unavailable", &reason),
            });
            tray::init(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running HoverPill");
}
