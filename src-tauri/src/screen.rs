//! Screenshots for ChatGPT: the monitor the notch is on, captured with the notch itself left
//! out, so a question can come with what the user is looking at. Kept in memory only, a few at
//! a time, and sent only with the question they were attached to (and its follow-ups).
//!
//! Windows only for now (see `xcap` in Cargo.toml); the notch hides the button elsewhere.

use std::io::Cursor;
use std::sync::Mutex;

use base64::Engine;
use image::imageops::{self, FilterType};
use image::{codecs::jpeg::JpegEncoder, RgbImage, RgbaImage};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::{events, notch};

// The image helpers below are only reached from the Windows capture (and the tests).

/// The longest side sent to ChatGPT, in px: enough to read text on a 1080p screen without
/// sending a 4K one in full.
#[cfg_attr(not(windows), allow(dead_code))]
const MAX_SIDE: u32 = 1600;
/// The thumbnail shown in the notch.
#[cfg_attr(not(windows), allow(dead_code))]
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
    let (x, y, _, _) = notch::monitor_bounds(&win)?;
    notch::exclude_from_capture(&win, true);
    let shot = tauri::async_runtime::spawn_blocking(move || capture(x, y)).await;
    notch::exclude_from_capture(&win, false);
    let (full, preview) = shot.map_err(|e| e.to_string())??;
    let id = events::next_id("screen");
    app.state::<Screens>().add(id.clone(), full);
    Ok(Captured { id, preview })
}

/// The monitor at (`x`, `y`), as a JPEG for ChatGPT and a thumbnail for the notch.
#[cfg(windows)]
fn capture(x: i32, y: i32) -> Result<(String, String), String> {
    let monitor = xcap::Monitor::from_point(x + 1, y + 1).map_err(|e| format!("couldn't find the screen: {e}"))?;
    let shot = monitor.capture_image().map_err(|e| format!("couldn't capture the screen: {e}"))?;
    Ok((jpeg_url(&fit(&shot, MAX_SIDE), 80)?, jpeg_url(&fit(&shot, PREVIEW_WIDTH), 70)?))
}

#[cfg(not(windows))]
fn capture(_x: i32, _y: i32) -> Result<(String, String), String> {
    Err("Sharing your screen with ChatGPT works on Windows for now.".into())
}

/// `shot` scaled down so its longest side is at most `side` (never up), without its alpha.
#[cfg_attr(not(windows), allow(dead_code))]
fn fit(shot: &RgbaImage, side: u32) -> RgbImage {
    let (w, h) = shot.dimensions();
    let scale = (side as f64 / w.max(h) as f64).min(1.0);
    let (nw, nh) = (((w as f64 * scale).round() as u32).max(1), ((h as f64 * scale).round() as u32).max(1));
    let small = if scale < 1.0 { imageops::resize(shot, nw, nh, FilterType::Triangle) } else { shot.clone() };
    image::DynamicImage::ImageRgba8(small).to_rgb8()
}

#[cfg_attr(not(windows), allow(dead_code))]
fn jpeg_url(image: &RgbImage, quality: u8) -> Result<String, String> {
    let mut bytes = Cursor::new(Vec::new());
    JpegEncoder::new_with_quality(&mut bytes, quality).encode_image(image).map_err(|e| e.to_string())?;
    Ok(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())))
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
    fn keeps_only_the_latest_few() {
        let screens = Screens::default();
        for i in 0..KEEP + 2 {
            screens.add(format!("s{i}"), format!("url{i}"));
        }
        assert_eq!(screens.get("s0"), None);
        assert_eq!(screens.get(&format!("s{}", KEEP + 1)), Some(format!("url{}", KEEP + 1)));
    }
}
