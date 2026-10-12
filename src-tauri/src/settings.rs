//! User settings: one JSON file in the app config dir, owned by Rust. The settings window
//! edits them through `set_settings`; every window hears about changes via `settings:changed`.

use std::path::PathBuf;
use std::sync::{Mutex, RwLock};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

/// Field names match src/lib/types.ts (`Settings`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub display: Display,
    pub notifications: Notifications,
    pub media: Media,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Display {
    /// Monitor name as the OS reports it; `None` follows the primary monitor.
    pub monitor: Option<String>,
    /// Gap between the top of the screen and the notch, in px. 0 attaches it to the edge.
    pub top_offset: f64,
    /// Size multiplier for the whole pill.
    pub scale: f64,
    /// Also open when the pointer rests on the notch; otherwise only pushing the pointer
    /// against the top edge opens it, and pointing at the closed notch fades it out instead.
    pub open_on_hover: bool,
    /// With nothing playing and no card showing, shrink to a thin bar.
    pub compact_idle: bool,
    /// Get out of the way while a fullscreen app covers the notch's monitor.
    pub hide_in_fullscreen: bool,
    /// Reserve a strip at the top of the monitor (like a taskbar) so maximised windows start
    /// below the notch. Windows only.
    pub reserve_space: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Notifications {
    pub enabled: bool,
    /// How long a card stays before collapsing.
    pub duration_secs: f64,
    /// App names (as shown on the card), compared case-insensitively.
    pub blocked_apps: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Media {
    pub enabled: bool,
    /// Player source ids (see `hoverpill_media::Source`).
    pub ignored_players: Vec<String>,
}

pub const TOP_OFFSET_RANGE: (f64, f64) = (0.0, 48.0);
pub const SCALE_RANGE: (f64, f64) = (0.8, 1.4);
pub const DURATION_RANGE: (f64, f64) = (2.0, 15.0);

impl Default for Display {
    fn default() -> Self {
        Self {
            monitor: None,
            top_offset: 0.0,
            scale: 1.0,
            open_on_hover: false,
            compact_idle: true,
            hide_in_fullscreen: true,
            reserve_space: false,
        }
    }
}

impl Default for Notifications {
    fn default() -> Self {
        Self { enabled: true, duration_secs: 4.5, blocked_apps: Vec::new() }
    }
}

impl Default for Media {
    fn default() -> Self {
        Self { enabled: true, ignored_players: Vec::new() }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self { display: Display::default(), notifications: Notifications::default(), media: Media::default() }
    }
}

fn clamp((min, max): (f64, f64), value: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

fn tidy(list: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in list.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(&item)) {
            out.push(item);
        }
    }
    out
}

impl Settings {
    /// Whatever the file or the UI sent, the rest of the app only ever sees sane values.
    pub fn sanitized(mut self) -> Self {
        let d = Display::default();
        self.display.top_offset = clamp(TOP_OFFSET_RANGE, self.display.top_offset, d.top_offset).round();
        self.display.scale = (clamp(SCALE_RANGE, self.display.scale, d.scale) * 100.0).round() / 100.0;
        self.display.monitor = self.display.monitor.filter(|m| !m.trim().is_empty());
        let n = Notifications::default();
        self.notifications.duration_secs = clamp(DURATION_RANGE, self.notifications.duration_secs, n.duration_secs);
        self.notifications.blocked_apps = tidy(self.notifications.blocked_apps);
        self.media.ignored_players = tidy(self.media.ignored_players);
        self
    }

    pub fn is_app_blocked(&self, app: &str) -> bool {
        self.notifications.blocked_apps.iter().any(|b| b.eq_ignore_ascii_case(app.trim()))
    }
}

pub struct SettingsStore {
    path: PathBuf,
    current: RwLock<Settings>,
}

impl SettingsStore {
    /// A missing or unreadable file means defaults; a bad file is left alone until the next save.
    pub fn load(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let path = app.path().app_config_dir()?.join("settings.json");
        let current = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| match serde_json::from_str::<Settings>(&text) {
                Ok(s) => Some(s),
                Err(e) => {
                    eprintln!("hoverpill: ignoring invalid {}: {e}", path.display());
                    None
                }
            })
            .unwrap_or_default()
            .sanitized();
        Ok(Self { path, current: RwLock::new(current) })
    }

    pub fn get(&self) -> Settings {
        self.current.read().unwrap().clone()
    }

    fn save(&self, settings: &Settings) -> Result<(), String> {
        let err = |e: std::io::Error| format!("can't save settings: {e}");
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(err)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(settings).expect("serialising settings")).map_err(err)?;
        std::fs::rename(&tmp, &self.path).map_err(err)
    }
}

/// Pushes settings into the parts of the app that hold their own copy.
pub fn apply(app: &AppHandle, settings: &Settings) {
    let media = app.state::<hoverpill_media::Media>();
    media.set_ignored(settings.media.ignored_players.iter().cloned());
    crate::events::send(
        app,
        "pill:media",
        if settings.media.enabled { media.current().map(crate::events::MediaState::from) } else { None },
    );
    if let Some(pill) = app.get_webview_window(crate::notch::LABEL) {
        if let Err(e) = crate::notch::place_top_center(&pill) {
            eprintln!("hoverpill: placement failed: {e}");
        }
    }
    let _ = app.emit("settings:changed", settings);
}

pub const WINDOW_LABEL: &str = "settings";

/// Opens the Settings window, or brings it forward if it's already open.
pub fn open_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(WINDOW_LABEL) {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }
    // Built off the event-loop thread: creating a WebView2 window from inside a menu/IPC
    // handler can deadlock on Windows.
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let built = WebviewWindowBuilder::new(&app, WINDOW_LABEL, WebviewUrl::App("settings.html".into()))
            .title("HoverPill Settings")
            .inner_size(720.0, 540.0)
            .min_inner_size(600.0, 440.0)
            .center()
            .build();
        if let Err(e) = built {
            crate::events::notify(&app, "Couldn't open Settings", &e.to_string());
        }
    });
}

/// Apps seen in notifications recently (blocked ones too), newest first, so the Settings window
/// can offer them for blocking without anyone typing names.
#[derive(Default)]
pub struct RecentApps(Mutex<Vec<String>>);

const RECENT_APPS_MAX: usize = 30;

impl RecentApps {
    pub fn record(&self, app: &str) {
        let mut list = self.0.lock().unwrap();
        list.retain(|a| !a.eq_ignore_ascii_case(app));
        list.insert(0, app.to_string());
        list.truncate(RECENT_APPS_MAX);
    }
}

#[tauri::command]
pub fn recent_notification_apps(recent: State<RecentApps>) -> Vec<String> {
    recent.0.lock().unwrap().clone()
}

#[derive(Serialize)]
pub struct PlayerInfo {
    id: String,
    label: String,
}

/// Players the OS knows about right now, plus ignored ones that aren't running.
#[tauri::command]
pub fn media_players(media: State<hoverpill_media::Media>, store: State<SettingsStore>) -> Vec<PlayerInfo> {
    let mut sources = media.sources();
    for id in store.get().media.ignored_players {
        if !sources.iter().any(|s| s.id == id) {
            sources.push(hoverpill_media::Source::new(id));
        }
    }
    sources.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    sources.into_iter().map(|s| PlayerInfo { id: s.id, label: s.label }).collect()
}

#[tauri::command]
pub fn open_settings(app: AppHandle) {
    open_window(&app);
}

#[tauri::command]
pub fn get_settings(store: State<SettingsStore>) -> Settings {
    store.get()
}

/// Returns what was actually stored (after clamping), so the UI can snap to it.
#[tauri::command]
pub fn set_settings(app: AppHandle, store: State<SettingsStore>, settings: Settings) -> Result<Settings, String> {
    let settings = settings.sanitized();
    store.save(&settings)?;
    *store.current.write().unwrap() = settings.clone();
    apply(&app, &settings);
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_file_fills_in_defaults() {
        let s: Settings = serde_json::from_str(r#"{ "display": { "scale": 1.2 } }"#).unwrap();
        assert_eq!(s.display.scale, 1.2);
        assert_eq!(s.display.top_offset, 0.0);
        assert!(!s.display.open_on_hover && s.display.compact_idle && s.display.hide_in_fullscreen);
        assert!(!s.display.reserve_space);
        assert!(s.notifications.enabled);
    }

    #[test]
    fn sanitize_clamps_and_dedupes() {
        let mut s = Settings::default();
        s.display.scale = 9.0;
        s.display.top_offset = -5.0;
        s.display.monitor = Some("  ".into());
        s.notifications.duration_secs = f64::NAN;
        s.notifications.blocked_apps = vec!["Discord".into(), " discord ".into(), "".into(), "Slack".into()];
        let s = s.sanitized();
        assert_eq!(s.display.scale, SCALE_RANGE.1);
        assert_eq!(s.display.top_offset, 0.0);
        assert_eq!(s.display.monitor, None);
        assert_eq!(s.notifications.duration_secs, 4.5);
        assert_eq!(s.notifications.blocked_apps, vec!["Discord", "Slack"]);
    }

    #[test]
    fn blocking_ignores_case() {
        let mut s = Settings::default();
        s.notifications.blocked_apps = vec!["WhatsApp".into()];
        assert!(s.is_app_blocked("whatsapp"));
        assert!(!s.is_app_blocked("Discord"));
    }
}
