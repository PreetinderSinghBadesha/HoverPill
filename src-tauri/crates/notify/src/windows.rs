//! `Windows.UI.Notifications.Management.UserNotificationListener`: read access to the toasts
//! in Notification Center. Windows asks the user for consent once, and the user can revoke it
//! under Settings › Privacy & security › Notifications.
//!
//! Without package identity the listener's `NotificationChanged` event fails ("Element not
//! found"), and the full `GetNotificationsAsync` list takes about 4-5 s per call. So:
//! - **Fast path:** notification ids come from a system-wide counter that only goes up, and
//!   `GetNotification(id)` is a quick synchronous lookup. Every 600 ms we look at the next few
//!   ids past the newest one we know.
//! - **Safety net:** the full list is re-read every 15 s. It catches anything the fast path
//!   missed (e.g. a gap in ids wider than the look-ahead) and re-anchors the counter.

use std::time::{Duration, Instant};

use windows::core::Interface;
use windows::Foundation::Size;
use windows::Storage::Streams::{DataReader, IRandomAccessStreamWithContentType};
use windows::UI::Notifications::Management::{UserNotificationListener, UserNotificationListenerAccessStatus as Access};
use windows::UI::Notifications::{KnownNotificationBindings, NotificationKinds, UserNotification};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows_future::IAsyncOperation;

use crate::{data_url, plain_text, Event, Notification, MAX_ICON_BYTES};

const PROBE_EVERY: Duration = Duration::from_millis(600);
/// How far past the newest known id to look; ids sometimes skip a number or two.
const LOOKAHEAD: u32 = 4;
const RESYNC_EVERY: Duration = Duration::from_secs(15);

fn err(e: windows::core::Error) -> String {
    format!("{} ({:#x})", e.message(), e.code().0)
}

pub fn run(on_event: &dyn Fn(Event)) -> Result<(), String> {
    let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    let listener = UserNotificationListener::Current().map_err(err)?;

    let access = match listener.GetAccessStatus().map_err(err)? {
        Access::Allowed => Access::Allowed,
        _ => listener.RequestAccessAsync().and_then(|op| op.join()).map_err(err)?,
    };
    if access != Access::Allowed {
        return Err(format!(
            "notification access is {} (Settings › Privacy & security › Notifications)",
            if access == Access::Denied { "denied" } else { "not granted" }
        ));
    }

    let emit = |n: &UserNotification| {
        if let Some(parsed) = parse(n) {
            on_event(Event::Notification(parsed));
        }
    };

    // Everything below `next` has been handled. `None` until we've seen at least one id.
    // The first full read only anchors it: the existing backlog isn't replayed.
    let mut next: Option<u32> = None;
    let mut first_sync = true;
    let mut last_sync: Option<Instant> = None;

    loop {
        let resync_due = last_sync.is_none_or(|t| t.elapsed() >= RESYNC_EVERY);
        // With no anchor the fast path can't work, so keep re-reading the full list.
        if resync_due || next.is_none() {
            match all_toasts(&listener) {
                Ok(mut list) => {
                    list.sort_by_key(|(id, _)| *id);
                    for (id, n) in &list {
                        if !first_sync && next.is_none_or(|nx| *id >= nx) {
                            emit(n);
                        }
                    }
                    if let Some((max, _)) = list.last() {
                        next = Some(next.map_or(max + 1, |nx| nx.max(max + 1)));
                    }
                    first_sync = false;
                }
                Err(e) => eprintln!("hoverpill-notify: full read failed: {e}"),
            }
            last_sync = Some(Instant::now());
        } else if let Some(start) = next {
            let mut newest = None;
            for id in start..start.saturating_add(LOOKAHEAD) {
                if let Ok(n) = listener.GetNotification(id) {
                    emit(&n);
                    newest = Some(id);
                }
            }
            if let Some(id) = newest {
                next = Some(id + 1);
            }
        }
        std::thread::sleep(PROBE_EVERY);
    }
}

fn all_toasts(listener: &UserNotificationListener) -> Result<Vec<(u32, UserNotification)>, String> {
    let list = listener.GetNotificationsAsync(NotificationKinds::Toast).and_then(|op| op.join()).map_err(err)?;
    let len = list.Size().map_err(err)?;
    Ok((0..len).filter_map(|i| list.GetAt(i).ok()).filter_map(|n| Some((n.Id().ok()?, n))).collect())
}

fn parse(n: &UserNotification) -> Option<Notification> {
    let display = n.AppInfo().ok()?.DisplayInfo().ok()?;
    let app = display.DisplayName().ok()?.to_string_lossy();

    // ToastGeneric: the first text element is the title, the rest is the body.
    let binding = n.Notification().ok()?.Visual().ok()?.GetBinding(&KnownNotificationBindings::ToastGeneric().ok()?).ok()?;
    let texts = binding.GetTextElements().ok()?;
    let lines: Vec<String> = (0..texts.Size().ok()?)
        .filter_map(|i| texts.GetAt(i).ok()?.Text().ok())
        .map(|t| plain_text(&t.to_string_lossy()))
        .filter(|t| !t.is_empty())
        .collect();
    let (title, body) = match lines.split_first() {
        Some((first, rest)) => (first.clone(), rest.join(" · ")),
        None => (app.clone(), String::new()),
    };

    let icon = display
        .GetLogo(Size { Width: 64.0, Height: 64.0 })
        .ok()
        .and_then(|logo| logo.OpenReadAsync().ok()?.join().ok())
        .and_then(|stream| read_image(&stream));

    Some(Notification { app, title, body, icon })
}

fn read_image(stream: &IRandomAccessStreamWithContentType) -> Option<String> {
    let size = usize::try_from(stream.Size().ok()?).ok().filter(|&s| s > 0 && s <= MAX_ICON_BYTES)?;
    let mime = stream
        .ContentType()
        .map(|m| m.to_string_lossy())
        .ok()
        .filter(|m| m.starts_with("image/"))
        .unwrap_or_else(|| "image/png".into());
    let reader = DataReader::CreateDataReader(stream).ok()?;
    let load: IAsyncOperation<u32> = reader.LoadAsync(size as u32).ok()?.cast().ok()?;
    let mut bytes = vec![0u8; load.join().ok()? as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    data_url(&mime, &bytes)
}
