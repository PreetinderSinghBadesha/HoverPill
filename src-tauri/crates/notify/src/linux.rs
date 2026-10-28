//! Watches the desktop's notification APIs on the session bus with a D-Bus *monitor*
//! connection (`BecomeMonitor`). The desktop's own notification daemon keeps working; to keep
//! its banners off screen, turn on its Do Not Disturb mode. Apps still send their notifications
//! while DND is on, so HoverPill still sees them.
//!
//! Three senders are watched:
//! - `org.freedesktop.Notifications.Notify`: libnotify-style apps.
//! - `org.gtk.Notifications.AddNotification`: GTK/GNotification apps, which never call `Notify`.
//!   On GNOME the portal's own notification backend forwards here too, so Flatpak apps land
//!   here even though they call the portal.
//! - `org.freedesktop.portal.Notification.AddNotification`: the portal frontend, for desktops
//!   whose backend doesn't forward through GNotification (and for anything GNOME's path misses).
//!
//! The desktop re-sends a notification to its own shell to draw the banner, and the portal
//! forwards to GNotification, so one notification can be seen two or three times. The short-lived
//! content dedupe below collapses those hops into a single event.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use zbus::blocking::fdo::MonitoringProxy;
use zbus::blocking::{Connection, MessageIterator};
use zbus::message::Type as MessageType;
use zbus::zvariant::{OwnedValue, Structure};
use zbus::MatchRule;

use crate::{data_url, plain_text, Event, Notification, MAX_ICON_BYTES};

type Hints = HashMap<String, OwnedValue>;
/// `Notify(app_name, replaces_id, app_icon, summary, body, actions, hints, expire_timeout)`
type NotifyArgs = (String, u32, String, String, String, Vec<String>, Hints, i32);
/// `AddNotification(app_id, id, notification)`, from `org.gtk.Notifications`.
type GtkArgs = (String, String, Hints);
/// `AddNotification(id, notification)`, from the portal frontend.
type PortalArgs = (String, Hints);
/// Raw pixels from the `image-data` hint: width, height, rowstride, has_alpha, bits per
/// sample, channels, data.
type ImageData = (i32, i32, i32, bool, i32, i32, Vec<u8>);

fn err(e: zbus::Error) -> String {
    e.to_string()
}

fn rule(interface: &'static str, member: &'static str) -> Result<MatchRule<'static>, String> {
    let builder = MatchRule::builder().msg_type(MessageType::MethodCall).interface(interface).map_err(err)?;
    Ok(builder.member(member).map_err(err)?.build())
}

pub fn run(on_event: &dyn Fn(Event)) -> Result<(), String> {
    let conn = Connection::session().map_err(err)?;
    // A monitor connection must not send calls, so app-id lookups use their own connection.
    let lookup = Connection::session().map_err(err)?;
    let rules = [
        rule("org.freedesktop.Notifications", "Notify")?,
        rule("org.gtk.Notifications", "AddNotification")?,
        rule("org.freedesktop.portal.Notification", "AddNotification")?,
    ];
    // Create the iterator first so nothing slips past between becoming a monitor and reading.
    let messages = MessageIterator::from(&conn);
    MonitoringProxy::new(&conn)
        .map_err(err)?
        .become_monitor(&rules, 0)
        .map_err(|e| format!("can't monitor the session bus: {e}"))?;

    // The portal forwards to GNotification on GNOME, and both calls carry the same id/title/body.
    let mut seen: HashMap<String, Instant> = HashMap::new();

    for msg in messages {
        let Ok(msg) = msg else { continue };
        let header = msg.header();
        if header.message_type() != MessageType::MethodCall {
            continue;
        }
        let interface = header.interface().map(|i| i.as_str()).unwrap_or_default();
        let member = header.member().map(|m| m.as_str()).unwrap_or_default();
        match (interface, member) {
            ("org.freedesktop.Notifications", "Notify") => {
                let Ok((app_name, _replaces, app_icon, summary, body, _actions, hints, _timeout)) =
                    msg.body().deserialize::<NotifyArgs>()
                else {
                    continue;
                };
                let app = if app_name.is_empty() { hint_str(&hints, "desktop-entry").unwrap_or_else(|| "Notification".into()) } else { app_name };
                let title = plain_text(&summary);
                let icon = icon_from_hints(&hints).or_else(|| icon_from_name(&app_icon));
                if fresh(&mut seen, &format!("{app}\u{0}{title}\u{0}{}", plain_text(&body))) {
                    on_event(Event::Notification(Notification { app, title, body: plain_text(&body), icon }));
                }
            }
            ("org.gtk.Notifications", "AddNotification") => {
                let Ok((app_id, id, hints)) = msg.body().deserialize::<GtkArgs>() else { continue };
                let Some(notification) = gtk_notification(&app_id, &hints) else { continue };
                let key = format!("{id}\u{0}{}\u{0}{}", notification.title, notification.body);
                if fresh(&mut seen, &key) {
                    on_event(Event::Notification(notification));
                }
            }
            ("org.freedesktop.portal.Notification", "AddNotification") => {
                let Ok((id, hints)) = msg.body().deserialize::<PortalArgs>() else { continue };
                let app = portal_app(&lookup, &header, &hints);
                let Some(notification) = notification_from(app, &hints) else { continue };
                let key = format!("{id}\u{0}{}\u{0}{}", notification.title, notification.body);
                if fresh(&mut seen, &key) {
                    on_event(Event::Notification(notification));
                }
            }
            _ => {}
        }
    }
    Err("D-Bus monitor connection closed".into())
}

/// Drops a repeat of the same notification (any forwarding hop) within a few seconds. The
/// id, title and body together are specific enough not to swallow two different notifications.
fn fresh(seen: &mut HashMap<String, Instant>, key: &str) -> bool {
    let now = Instant::now();
    seen.retain(|_, at| now.duration_since(*at) < Duration::from_secs(3));
    if seen.contains_key(key) {
        return false;
    }
    seen.insert(key.to_string(), now);
    true
}

fn gtk_notification(app_id: &str, hints: &Hints) -> Option<Notification> {
    let id = if app_id.trim().is_empty() {
        hint_str(hints, "desktop-entry").unwrap_or_default()
    } else {
        app_id.trim().to_string()
    };
    let app = app_name(&id);
    let icon = icon_from_serialized(hints.get("icon")).or_else(|| icon_for_app(&id));
    notification_from(app, hints).map(|mut notification| {
        notification.icon = notification.icon.or(icon);
        notification
    })
}

/// The portal's notification app id usually comes from the Flatpak sandbox the caller runs in.
fn portal_app(conn: &Connection, header: &zbus::message::Header, hints: &Hints) -> String {
    let id = flatpak_app_id(conn, header)
        .or_else(|| hint_str(hints, "desktop-entry"))
        .unwrap_or_default();
    app_name(&id)
}

fn notification_from(app: String, hints: &Hints) -> Option<Notification> {
    let title = hint_str(hints, "title").unwrap_or_default();
    let body = hint_str(hints, "body").unwrap_or_default();
    if title.is_empty() && body.is_empty() {
        return None;
    }
    Some(Notification { app, title: plain_text(&title), body: plain_text(&body), icon: icon_from_serialized(hints.get("icon")) })
}

/// `[Application] name=org.example.App` in a Flatpak sandbox's `/.flatpak-info`, read through
/// the caller's `/proc/<pid>/root`. Falls back to nothing for unsandboxed apps.
fn flatpak_app_id(conn: &Connection, header: &zbus::message::Header) -> Option<String> {
    let sender = header.sender()?.to_string();
    let reply = conn
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetConnectionUnixProcessID",
            &(sender.as_str(),),
        )
        .ok()?;
    let pid: u32 = reply.body().deserialize().ok()?;
    let info = std::fs::read_to_string(format!("/proc/{pid}/root/.flatpak-info")).ok()?;
    let mut in_application = false;
    for line in info.lines().map(str::trim) {
        if line.starts_with('[') {
            in_application = line == "[Application]";
        } else if in_application {
            if let Some(name) = line.strip_prefix("name=") {
                return Some(name.trim().to_string());
            }
        }
    }
    None
}

/// A GTK notification's app id is a desktop file id (`org.gnome.Nautilus`); show the name a
/// person recognizes rather than the id.
fn app_name(app_id: &str) -> String {
    let app_id = app_id.trim();
    if app_id.is_empty() {
        return "Notification".into();
    }
    desktop_value(app_id, "Name").unwrap_or_else(|| app_id.to_string())
}

fn icon_for_app(app_id: &str) -> Option<String> {
    let icon = desktop_value(app_id, "Icon")?;
    icon_from_name(&icon)
}

/// Reads `key` (usually `Name` or `Icon`) from `<app_id>.desktop` in the standard application
/// directories, including the Flatpak and Snap export directories.
fn desktop_value(app_id: &str, key: &str) -> Option<String> {
    let app_id = app_id.trim();
    if app_id.is_empty() {
        return None;
    }
    let file = if app_id.ends_with(".desktop") { app_id.to_string() } else { format!("{app_id}.desktop") };
    let prefix = format!("{key}=");
    for dir in data_dirs() {
        let Ok(text) = std::fs::read_to_string(dir.join("applications").join(&file)) else { continue };
        for line in text.lines() {
            if let Some(value) = line.strip_prefix(&prefix) {
                let value = value.trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

fn data_dirs() -> &'static [PathBuf] {
    static DIRS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    DIRS.get_or_init(|| {
        let mut dirs = Vec::new();
        if let Some(home) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
            dirs.push(home);
        } else if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            dirs.push(home.join(".local/share"));
        }
        let system = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
        dirs.extend(system.split(':').filter(|part| !part.is_empty()).map(PathBuf::from));
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            dirs.push(home.join(".local/share/flatpak/exports/share"));
        }
        dirs.push(PathBuf::from("/var/lib/flatpak/exports/share"));
        dirs.push(PathBuf::from("/var/lib/snapd/desktop"));
        dirs
    })
}

fn hint_str(hints: &Hints, key: &str) -> Option<String> {
    hints.get(key)?.downcast_ref::<&str>().ok().map(str::to_string)
}

/// `image-data` (spec 1.2), with the names older apps use.
fn icon_from_hints(hints: &Hints) -> Option<String> {
    let value = ["image-data", "image_data", "icon_data"].iter().find_map(|k| hints.get(*k))?;
    let image: ImageData = value.try_clone().ok()?.try_into().ok()?;
    encode_png(image)
}

/// A serialized `GIcon`: `(sv)`, e.g. `("themed", ["spotify"])` or `("file", "/path.png")`.
/// Used by both GNotification and the notification portal.
fn icon_from_serialized(value: Option<&OwnedValue>) -> Option<String> {
    let structure: Structure<'static> = value?.try_clone().ok()?.try_into().ok()?;
    let mut fields = structure.into_fields();
    if fields.len() != 2 {
        return None;
    }
    let kind = String::try_from(fields.remove(0)).ok()?;
    let data = fields.remove(0);
    match kind.as_str() {
        "themed" => {
            let names = Vec::<String>::try_from(data).ok()?;
            names
                .iter()
                .find(|name| !name.ends_with("-symbolic"))
                .or_else(|| names.first())
                .and_then(|name| icon_from_name(name))
        }
        "file" => {
            let path: String = data.try_into().ok()?;
            icon_from_name(&path)
        }
        _ => None,
    }
}

fn encode_png((width, height, stride, alpha, bits, channels, data): ImageData) -> Option<String> {
    let expected_channels = if alpha { 4 } else { 3 };
    if bits != 8 || channels != expected_channels || width <= 0 || height <= 0 {
        return None;
    }
    let (w, h, stride, ch) = (width as usize, height as usize, stride as usize, channels as usize);
    if stride < w * ch || data.len() < stride * (h - 1) + w * ch {
        return None;
    }
    // Rows can be padded to `stride`; PNG wants them packed.
    let mut packed = Vec::with_capacity(w * h * ch);
    for row in data.chunks(stride).take(h) {
        packed.extend_from_slice(&row[..w * ch]);
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, w as u32, h as u32);
        encoder.set_color(if alpha { png::ColorType::Rgba } else { png::ColorType::Rgb });
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().ok()?.write_image_data(&packed).ok()?;
    }
    data_url("image/png", &out)
}

/// `app_icon` is a file path/URL or an icon-theme name. Theme lookup is limited to the common
/// hicolor sizes and pixmaps, which covers most apps without a full icon-theme resolver.
fn icon_from_name(icon: &str) -> Option<String> {
    if icon.is_empty() {
        return None;
    }
    if let Some(path) = icon.strip_prefix("file://").or(icon.starts_with('/').then_some(icon)) {
        return read_icon(path);
    }
    let candidates = [
        format!("/usr/share/icons/hicolor/64x64/apps/{icon}.png"),
        format!("/usr/share/icons/hicolor/48x48/apps/{icon}.png"),
        format!("/usr/share/icons/hicolor/128x128/apps/{icon}.png"),
        format!("/usr/share/icons/hicolor/scalable/apps/{icon}.svg"),
        format!("/usr/share/pixmaps/{icon}.png"),
        format!("/usr/share/pixmaps/{icon}.svg"),
    ];
    candidates.iter().find_map(|p| read_icon(p))
}

fn read_icon(path: &str) -> Option<String> {
    if std::fs::metadata(path).ok()?.len() as usize > MAX_ICON_BYTES {
        return None;
    }
    let mime = match path.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "svg" => "image/svg+xml",
        "jpg" | "jpeg" => "image/jpeg",
        _ => "image/png",
    };
    data_url(mime, &std::fs::read(path).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_from_padded_rows() {
        // 2x2 RGB with each row padded to 8 bytes.
        let data = vec![255, 0, 0, 0, 255, 0, 9, 9, 0, 0, 255, 255, 255, 255, 9, 9];
        let url = encode_png((2, 2, 8, false, 8, 3, data)).unwrap();
        assert!(url.starts_with("data:image/png;base64,iVBORw0KGgo"));
    }

    #[test]
    fn rejects_inconsistent_image_data() {
        assert!(encode_png((2, 2, 8, true, 8, 3, vec![0; 16])).is_none()); // alpha but 3 channels
        assert!(encode_png((2, 2, 8, false, 8, 3, vec![0; 4])).is_none()); // too short
    }

    /// The shell redraws a notification it was sent, and the portal forwards to GNotification;
    /// every hop carries the same id/title/body, so only the first one becomes an event.
    #[test]
    fn collapses_forwarding_hops() {
        let mut seen = HashMap::new();
        assert!(fresh(&mut seen, "id\0Title\0Body"));
        assert!(!fresh(&mut seen, "id\0Title\0Body"));
        assert!(fresh(&mut seen, "other\0Title\0Body"));
    }

    /// Talks to the real session bus and desktop notification daemon; run by hand with
    /// `cargo test -p hoverpill-notify live_notifications -- --ignored`.
    #[test]
    #[ignore]
    fn live_notifications() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = run(&|event| {
                if let Event::Notification(notification) = event {
                    let _ = tx.send(notification);
                }
            });
        });
        std::thread::sleep(Duration::from_millis(700));
        let status = std::process::Command::new("notify-send")
            .args(["-a", "HoverPillTest", "HoverPill monitor probe", "one event please"])
            .status()
            .expect("notify-send should be installed");
        assert!(status.success());
        let notification = rx.recv_timeout(Duration::from_secs(5)).expect("no notification event");
        assert_eq!(notification.app, "HoverPillTest");
        assert_eq!(notification.title, "HoverPill monitor probe");
        assert_eq!(notification.body, "one event please");
        // The shell's redraw of the same notification must not arrive as a second event.
        assert!(rx.recv_timeout(Duration::from_millis(1200)).is_err());

        // The portal frontend, which forwards through GNotification on GNOME.
        let status = std::process::Command::new("gdbus")
            .args([
                "call", "--session", "--dest", "org.freedesktop.portal.Desktop",
                "--object-path", "/org/freedesktop/portal/desktop",
                "--method", "org.freedesktop.portal.Notification.AddNotification",
                "hp-portal-probe",
                "{'title': <'Portal monitor probe'>, 'body': <'portal event please'>}",
            ])
            .status()
            .expect("gdbus should be installed");
        assert!(status.success());
        let notification = rx.recv_timeout(Duration::from_secs(5)).expect("no portal notification event");
        assert_eq!(notification.title, "Portal monitor probe");
        assert_eq!(notification.body, "portal event please");
        assert!(rx.recv_timeout(Duration::from_millis(1200)).is_err());
    }

    #[test]
    fn deserializes_gtk_notifications() {
        let mut hints = Hints::new();
        hints.insert("title".into(), OwnedValue::from(zbus::zvariant::Str::from("Hi")));
        let notification = gtk_notification("org.example.App", &hints).unwrap();
        assert_eq!(notification.title, "Hi");
        assert_eq!(notification.app, "org.example.App"); // no .desktop file in the test
    }
}
