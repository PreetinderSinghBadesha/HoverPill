//! Mirrors other apps' desktop notifications so the pill can show them.
//!
//! - Windows: `UserNotificationListener`, polled and diffed, since its change event doesn't
//!   fire for desktop apps.
//! - Linux: a D-Bus *monitor* on `org.freedesktop.Notifications.Notify` calls. It observes
//!   alongside the desktop's own notification daemon instead of replacing it.
//!
//! Only notifications that arrive after `spawn` are shown; the existing backlog is reported
//! once as `Event::Seen` (which apps it's from) so settings can list those apps.

#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(not(any(windows, target_os = "linux")), path = "unsupported.rs")]
mod backend;

#[cfg(windows)]
mod windows_apps;

/// Apps the OS lets send notifications, by display name, whether or not they've sent one yet.
/// Windows only (empty elsewhere: Linux has no registry of notification senders). Takes a
/// few hundred milliseconds; call it off the UI thread.
pub fn notifying_apps() -> Vec<String> {
    #[cfg(windows)]
    return windows_apps::notifying_apps();
    #[cfg(not(windows))]
    Vec::new()
}

/// App icons are small; anything bigger than this is dropped rather than sent to the UI.
const MAX_ICON_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Notification {
    pub app: String,
    pub title: String,
    pub body: String,
    /// `data:` URL.
    pub icon: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Event {
    Notification(Notification),
    /// Apps with notifications already waiting when the listener started (not shown).
    Seen(Vec<String>),
    /// The listener can't run (no permission, no session bus, ...). Sent at most once.
    Unavailable(String),
}

/// Starts listening on a background thread; `on_event` is called from that thread.
pub fn spawn(on_event: impl Fn(Event) + Send + 'static) {
    std::thread::Builder::new()
        .name("hoverpill-notify".into())
        .spawn(move || {
            if let Err(reason) = backend::run(&on_event) {
                on_event(Event::Unavailable(reason));
            }
        })
        .expect("spawning the notification thread");
}

fn data_url(mime: &str, bytes: &[u8]) -> Option<String> {
    use base64::Engine;
    if bytes.is_empty() || bytes.len() > MAX_ICON_BYTES {
        return None;
    }
    Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// Notification bodies may carry a little HTML-ish markup (`<b>`, `<a href>`, entities).
fn plain_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_markup_and_entities() {
        assert_eq!(plain_text("<b>Alice</b>: see <a href=\"x\">this</a> &amp; that"), "Alice: see this & that");
        assert_eq!(plain_text("multi\nline\n\n  text"), "multi line text");
        assert_eq!(plain_text("1 &lt; 2"), "1 < 2");
    }
}
