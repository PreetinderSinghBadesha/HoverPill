//! Screenshots for ChatGPT: the monitor the notch is on, captured with the notch itself left
//! out, so a question can come with what the user is looking at. Kept in memory only, a few at
//! a time, and sent only with the question they were attached to (and its follow-ups).
//!
//! Windows captures the monitor directly with `xcap`; Linux asks the desktop's Screenshot portal
//! (works on Wayland as well as X11, and every GNOME/KDE/wlroots desktop ships one), then crops
//! the shot to the notch's monitor. GNOME only grants a silent screenshot after asking "Allow
//! HoverPill to take screenshots?", and it only asks the app that has focus, which the notch never
//! is; so when the silent shot is refused, the desktop's own screenshot screen is opened instead
//! and you take the shot there.

use std::io::Cursor;
use std::sync::Mutex;

use base64::Engine;
use image::imageops::{self, FilterType};
use image::{codecs::jpeg::JpegEncoder, RgbImage, RgbaImage};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::{events, notch};

/// The longest side sent to ChatGPT, in px: enough to read text on a 1080p screen without
/// sending a 4K one in full.
const MAX_SIDE: u32 = 1600;
/// The thumbnail shown in the notch.
const PREVIEW_WIDTH: u32 = 320;
/// Screenshots kept for follow-up questions; the oldest go first.
const KEEP: usize = 4;

/// Captured screens as `data:` URLs, by id.
#[derive(Default)]
pub struct Screens(Mutex<Vec<(String, String)>>);

impl Screens {
    pub fn get(&self, id: &str) -> Option<String> {
        self.0.lock().unwrap().iter().find(|(i, _)| i == id).map(|(_, url)| url.clone())
    }

    fn add(&self, id: String, url: String) {
        let mut shots = self.0.lock().unwrap();
        shots.push((id, url));
        let extra = shots.len().saturating_sub(KEEP);
        shots.drain(..extra);
    }
}

#[derive(Serialize)]
pub struct Captured {
    id: String,
    /// A small JPEG `data:` URL to show in the notch.
    preview: String,
}

/// Captures the notch's monitor and keeps it for the next question.
#[tauri::command]
pub async fn capture_screen(app: AppHandle) -> Result<Captured, String> {
    let win = app.get_webview_window(notch::LABEL).ok_or("no pill window")?;
    let monitor = notch::monitor_bounds(&win)?;
    let desktop = notch::desktop_bounds(&win)?;
    notch::exclude_from_capture(&win, true);
    let shot = tauri::async_runtime::spawn_blocking(move || capture(monitor, desktop)).await;
    notch::exclude_from_capture(&win, false);
    let (full, preview) = shot.map_err(|e| e.to_string())??;
    let id = events::next_id("screen");
    app.state::<Screens>().add(id.clone(), full);
    Ok(Captured { id, preview })
}

/// The monitor at `monitor` (x, y, width, height), as a JPEG for ChatGPT and a thumbnail for the
/// notch. `desktop` is every monitor's union, used to find the monitor inside a full-screen shot.
#[cfg(windows)]
fn capture(monitor: (i32, i32, i32, i32), _desktop: (i32, i32, i32, i32)) -> Result<(String, String), String> {
    let (x, y, _, _) = monitor;
    let monitor = xcap::Monitor::from_point(x + 1, y + 1).map_err(|e| format!("couldn't find the screen: {e}"))?;
    let shot = monitor.capture_image().map_err(|e| format!("couldn't capture the screen: {e}"))?;
    encode(&shot)
}

/// Linux: the portal takes one picture of the whole desktop; cut out the notch's monitor. If the
/// desktop won't take one silently, its screenshot screen opens and whatever you capture there is
/// used as it is.
#[cfg(target_os = "linux")]
fn capture(monitor: (i32, i32, i32, i32), desktop: (i32, i32, i32, i32)) -> Result<(String, String), String> {
    let read = |bytes: Vec<u8>| {
        image::load_from_memory(&bytes).map(|i| i.to_rgba8()).map_err(|e| format!("couldn't read the screenshot: {e}"))
    };
    match tauri::async_runtime::block_on(portal::screenshot(false)) {
        Ok(bytes) => encode(&crop(&read(bytes)?, monitor, desktop)),
        Err(portal::Failure::Refused) => match tauri::async_runtime::block_on(portal::screenshot(true)) {
            Ok(bytes) => encode(&read(bytes)?),
            Err(e) => Err(e.to_string()),
        },
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn capture(_monitor: (i32, i32, i32, i32), _desktop: (i32, i32, i32, i32)) -> Result<(String, String), String> {
    Err("Sharing your screen with ChatGPT isn't supported on this system.".into())
}

fn encode(shot: &RgbaImage) -> Result<(String, String), String> {
    Ok((jpeg_url(&fit(shot, MAX_SIDE), 80)?, jpeg_url(&fit(shot, PREVIEW_WIDTH), 70)?))
}

/// The monitor's part of a whole-desktop shot. HiDPI compositors can return logical instead of
/// physical pixels, so the crop is scaled by however much the shot is smaller than the desktop.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn crop(shot: &RgbaImage, monitor: (i32, i32, i32, i32), desktop: (i32, i32, i32, i32)) -> RgbaImage {
    let (mx, my, mw, mh) = monitor;
    let (dx, dy, dw, dh) = desktop;
    let (iw, ih) = shot.dimensions();
    if iw == 0 || ih == 0 {
        return shot.clone();
    }
    let sx = if dw > 0 { iw as f64 / dw as f64 } else { 1.0 };
    let sy = if dh > 0 { ih as f64 / dh as f64 } else { 1.0 };
    let x = (((mx - dx) as f64) * sx).round().clamp(0.0, (iw - 1) as f64) as u32;
    let y = (((my - dy) as f64) * sy).round().clamp(0.0, (ih - 1) as f64) as u32;
    let w = ((mw as f64) * sx).round().clamp(1.0, (iw - x) as f64) as u32;
    let h = ((mh as f64) * sy).round().clamp(1.0, (ih - y) as f64) as u32;
    imageops::crop_imm(shot, x, y, w, h).to_image()
}

/// `shot` scaled down so its longest side is at most `side` (never up), without its alpha.
fn fit(shot: &RgbaImage, side: u32) -> RgbImage {
    let (w, h) = shot.dimensions();
    let scale = (side as f64 / w.max(h) as f64).min(1.0);
    let (nw, nh) = (((w as f64 * scale).round() as u32).max(1), ((h as f64 * scale).round() as u32).max(1));
    let small = if scale < 1.0 { imageops::resize(shot, nw, nh, FilterType::Triangle) } else { shot.clone() };
    image::DynamicImage::ImageRgba8(small).to_rgb8()
}

fn jpeg_url(image: &RgbImage, quality: u8) -> Result<String, String> {
    let mut bytes = Cursor::new(Vec::new());
    JpegEncoder::new_with_quality(&mut bytes, quality).encode_image(image).map_err(|e| e.to_string())?;
    Ok(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())))
}

/// The desktop's `org.freedesktop.portal.Screenshot`: ask for a shot, wait for the portal's
/// `Response`, then read the file it wrote (and remove it, since the name is unique to this
/// request).
#[cfg(target_os = "linux")]
mod portal {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::Duration;

    use std::fmt;
    use std::sync::atomic::{AtomicU32, Ordering};

    use futures_util::StreamExt;
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
    use zbus::{Connection, MatchRule, MessageStream};

    /// A silent shot is quick; the screenshot screen waits for you.
    const SILENT_TIMEOUT: Duration = Duration::from_secs(15);
    const INTERACTIVE_TIMEOUT: Duration = Duration::from_secs(120);

    #[derive(Debug)]
    pub enum Failure {
        /// The desktop said no (on GNOME: it couldn't ask, as the notch doesn't have focus).
        Refused,
        Other(String),
    }

    impl fmt::Display for Failure {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Failure::Refused => f.write_str("the desktop refused the screenshot"),
                Failure::Other(text) => f.write_str(text),
            }
        }
    }

    impl From<String> for Failure {
        fn from(text: String) -> Self {
            Failure::Other(text)
        }
    }

    impl From<&str> for Failure {
        fn from(text: &str) -> Self {
            Failure::Other(text.to_string())
        }
    }

    /// Each request needs its own token: the refused silent one and the interactive retry are
    /// two requests from the same process.
    static NEXT_TOKEN: AtomicU32 = AtomicU32::new(0);

    /// `interactive` opens the desktop's screenshot screen instead of taking the shot silently.
    pub async fn screenshot(interactive: bool) -> Result<Vec<u8>, Failure> {
        let conn = Connection::session().await.map_err(|e| format!("can't reach the desktop portal: {e}"))?;
        let sender = conn
            .unique_name()
            .map(|name| name.as_str().to_string())
            .ok_or("the session bus has no name for us")?;
        let token = format!("hoverpill{}_{}", std::process::id(), NEXT_TOKEN.fetch_add(1, Ordering::Relaxed));

        // Subscribe before asking: the portal may answer before the method call returns.
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.portal.Request")
            .map_err(|e| e.to_string())?
            .member("Response")
            .map_err(|e| e.to_string())?
            .build();
        let mut responses = MessageStream::for_match_rule(rule, &conn, Some(4)).await.map_err(|e| e.to_string())?;

        let mut options: HashMap<&str, Value<'_>> = HashMap::new();
        options.insert("handle_token", Value::from(token.as_str()));
        options.insert("interactive", Value::from(interactive));
        options.insert("modal", Value::from(false));

        let reply = conn
            .call_method(
                Some("org.freedesktop.portal.Desktop"),
                "/org/freedesktop/portal/desktop",
                Some("org.freedesktop.portal.Screenshot"),
                "Screenshot",
                &("", options),
            )
            .await
            .map_err(|e| format!("can't ask the desktop for a screenshot: {e}"))?;
        let (handle,): (OwnedObjectPath,) = reply.body().deserialize().map_err(|e| e.to_string())?;
        let predicted = format!(
            "/org/freedesktop/portal/desktop/request/{}/{}",
            sender.trim_start_matches(':').replace('.', "_"),
            token
        );
        let wanted = [predicted.as_str(), handle.as_str()];

        let wait = async {
            while let Some(message) = responses.next().await {
                let message = message.map_err(|e| e.to_string())?;
                let header = message.header();
                if !wanted.contains(&header.path().map(|path| path.as_str()).unwrap_or_default()) {
                    continue;
                }
                let (response, results): (u32, HashMap<String, OwnedValue>) =
                    message.body().deserialize().map_err(|e| e.to_string())?;
                match response {
                    0 => {}
                    1 => return Err("the screenshot was cancelled".into()),
                    _ => return Err(Failure::Refused),
                }
                let uri = results.get("uri").cloned().ok_or("the screenshot has no file")?;
                let uri = String::try_from(uri).map_err(|e| e.to_string())?;
                return Ok(read_and_remove(&uri)?);
            }
            Err("the desktop stopped answering".into())
        };
        let limit = if interactive { INTERACTIVE_TIMEOUT } else { SILENT_TIMEOUT };
        tokio::time::timeout(limit, wait).await.map_err(|_| Failure::from("the screenshot timed out"))?
    }

    fn read_and_remove(uri: &str) -> Result<Vec<u8>, String> {
        let path = file_path(uri).ok_or_else(|| format!("can't read the screenshot at {uri}"))?;
        let bytes = std::fs::read(&path).map_err(|e| format!("can't read the screenshot: {e}"))?;
        // The portal gives every request a fresh file name, so this one is ours to clean up.
        let _ = std::fs::remove_file(&path);
        Ok(bytes)
    }

    /// The local path in a `file://` URI, percent-decoded (`%20` for the spaces GNOME puts in
    /// screenshot names).
    fn file_path(uri: &str) -> Option<PathBuf> {
        let rest = uri.strip_prefix("file://")?;
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        // `file://host/path` is a file on another machine, not ours.
        if !rest.starts_with('/') {
            return None;
        }
        Some(PathBuf::from(String::from_utf8_lossy(&percent_decode(rest)).into_owned()))
    }

    fn percent_decode(input: &str) -> Vec<u8> {
        let bytes = input.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                if let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    out.push(high * 16 + low);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        out
    }

    fn hex(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::file_path;
        use std::path::PathBuf;

        #[test]
        fn decodes_screenshot_file_uris() {
            assert_eq!(
                file_path("file:///home/me/Pictures/Screenshot%20from%202026-10-27.png"),
                Some(PathBuf::from("/home/me/Pictures/Screenshot from 2026-10-27.png"))
            );
            assert_eq!(file_path("file://localhost/tmp/a%2Bb.png"), Some(PathBuf::from("/tmp/a+b.png")));
            assert_eq!(file_path("file://otherhost/tmp/a.png"), None);
            assert_eq!(file_path("/tmp/a.png"), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_the_longest_side_and_never_enlarges() {
        let wide = RgbaImage::new(3840, 2160);
        assert_eq!(fit(&wide, 1600).dimensions(), (1600, 900));
        let tall = RgbaImage::new(1080, 1920);
        assert_eq!(fit(&tall, 1600).dimensions(), (900, 1600));
        let small = RgbaImage::new(800, 600);
        assert_eq!(fit(&small, 1600).dimensions(), (800, 600));
    }

    #[test]
    fn makes_a_jpeg_data_url() {
        let url = jpeg_url(&RgbImage::new(16, 9), 80).unwrap();
        assert!(url.starts_with("data:image/jpeg;base64,/9j/"));
    }

    #[test]
    fn cuts_the_monitor_out_of_the_desktop() {
        let mut shot = RgbaImage::new(4, 2);
        for x in 2..4 {
            for y in 0..2 {
                shot.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
        }
        let crop = crop(&shot, (2, 0, 2, 2), (0, 0, 4, 2));
        assert_eq!(crop.dimensions(), (2, 2));
        assert_eq!(crop.get_pixel(0, 0), &image::Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn scales_the_crop_for_logical_pixel_screenshots() {
        // A 4x2 physical desktop returned as 2x1 pixels: the right-hand monitor is 1x1 there.
        let shot = RgbaImage::new(2, 1);
        assert_eq!(crop(&shot, (2, 0, 2, 1), (0, 0, 4, 1)).dimensions(), (1, 1));
    }

    /// Talks to the real desktop portal; run by hand with
    /// `cargo test live_portal_screenshot -- --ignored`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore]
    fn live_portal_screenshot() {
        let bytes = tauri::async_runtime::block_on(portal::screenshot(false)).unwrap();
        let shot = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert!(shot.width() > 0 && shot.height() > 0);
    }

    #[test]
    fn keeps_only_the_latest_few() {
        let screens = Screens::default();
        for i in 0..KEEP + 2 {
            screens.add(format!("s{i}"), format!("url{i}"));
        }
        assert_eq!(screens.get("s0"), None);
        assert_eq!(screens.get(&format!("s{}", KEEP + 1)), Some(format!("url{}", KEEP + 1)));
    }
}
