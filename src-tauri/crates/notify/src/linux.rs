//! Watches `org.freedesktop.Notifications.Notify` calls on the session bus with a D-Bus
//! *monitor* connection (`BecomeMonitor`). The desktop's own notification daemon keeps working;
//! to keep its banners off screen, turn on its Do Not Disturb mode. Apps still call `Notify`
//! while DND is on, so HoverPill still sees them.

use std::collections::HashMap;

use zbus::blocking::fdo::MonitoringProxy;
use zbus::blocking::{Connection, MessageIterator};
use zbus::message::Type as MessageType;
use zbus::zvariant::OwnedValue;
use zbus::MatchRule;

use crate::{data_url, plain_text, Event, Notification, MAX_ICON_BYTES};

type Hints = HashMap<String, OwnedValue>;
/// `Notify(app_name, replaces_id, app_icon, summary, body, actions, hints, expire_timeout)`
type NotifyArgs = (String, u32, String, String, String, Vec<String>, Hints, i32);
/// Raw pixels from the `image-data` hint: width, height, rowstride, has_alpha, bits per
/// sample, channels, data.
type ImageData = (i32, i32, i32, bool, i32, i32, Vec<u8>);

fn err(e: zbus::Error) -> String {
    e.to_string()
}

pub fn run(on_event: &dyn Fn(Event)) -> Result<(), String> {
    let conn = Connection::session().map_err(err)?;
    let rule = MatchRule::builder()
        .msg_type(MessageType::MethodCall)
        .interface("org.freedesktop.Notifications")
        .and_then(|b| b.member("Notify"))
        .map_err(err)?
        .build();
    // Create the iterator first so nothing slips past between becoming a monitor and reading.
    let messages = MessageIterator::from(&conn);
    MonitoringProxy::new(&conn)
        .map_err(err)?
        .become_monitor(&[rule], 0)
        .map_err(|e| format!("can't monitor the session bus: {e}"))?;

    for msg in messages {
        let Ok(msg) = msg else { continue };
        let header = msg.header();
        if header.message_type() != MessageType::MethodCall || header.member().map(|m| m.as_str()) != Some("Notify") {
            continue;
        }
        let Ok((app_name, _replaces, app_icon, summary, body, _actions, hints, _timeout)) =
            msg.body().deserialize::<NotifyArgs>()
        else {
            continue;
        };

        let app = if app_name.is_empty() { hint_str(&hints, "desktop-entry").unwrap_or_else(|| "Notification".into()) } else { app_name };
        let title = plain_text(&summary);
        let icon = icon_from_hints(&hints).or_else(|| icon_from_name(&app_icon));
        on_event(Event::Notification(Notification { app, title, body: plain_text(&body), icon }));
    }
    Err("D-Bus monitor connection closed".into())
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
}
