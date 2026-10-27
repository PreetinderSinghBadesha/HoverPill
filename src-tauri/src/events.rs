//! Payloads pushed to the pill UI and the commands it calls back with.
//! Keep the field names in sync with src/lib/types.ts.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use hoverpill_media::{Action, Media, NowPlaying};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::broker::Broker;
use crate::settings::SettingsStore;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_id(prefix: &str) -> String {
    format!("{prefix}-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

/// Events emitted before the webview has registered its listeners would be lost, so they wait
/// here until the UI calls `pill_ready`.
pub struct Outbox(Mutex<Option<Vec<(String, Value)>>>);

impl Default for Outbox {
    fn default() -> Self {
        Self(Mutex::new(Some(Vec::new())))
    }
}

/// Emits to the pill, or queues until it's listening.
pub fn send<S: Serialize + Clone>(app: &AppHandle, event: &str, payload: S) {
    if let Some(queue) = app.state::<Outbox>().0.lock().unwrap().as_mut() {
        queue.push((event.to_string(), serde_json::to_value(payload).unwrap_or(Value::Null)));
        return;
    }
    let _ = app.emit(event, payload);
}

/// Puts text on the clipboard: a ChatGPT answer or one of its code blocks. The browser's own
/// clipboard API won't work in the notch, which never takes focus while you're just pointing at it.
#[tauri::command]
pub fn copy_text(app: AppHandle, text: String) -> Result<(), String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard().write_text(text).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pill_ready(app: AppHandle, outbox: State<Outbox>) {
    let queued = outbox.0.lock().unwrap().take();
    for (event, payload) in queued.into_iter().flatten() {
        let _ = app.emit(&event, payload);
    }
}

/// Claude Code sends these in camelCase (`toolName`, `ruleContent`), and expects the same names
/// back in `updatedPermissions`; the field names stay snake_case in Rust.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRule {
    pub tool_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_content: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionSuggestion {
    pub r#type: Option<String>,
    #[serde(default)]
    pub rules: Vec<PermissionRule>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behavior: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default)]
    pub directories: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct PermissionRequest {
    pub id: String,
    pub tool: String,
    pub command: String,
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_suggestions: Option<Vec<PermissionSuggestion>>,
}

/// The request went away without an answer from the pill (Claude was interrupted, or timed out).
#[derive(Clone, Serialize)]
pub struct PermissionCancel {
    pub id: String,
}

#[derive(Clone, Serialize)]
pub struct NotificationItem {
    pub id: String,
    pub app: String,
    pub title: String,
    pub body: String,
    pub icon: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct MediaState {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub artwork: Option<String>,
    /// Readable player name, e.g. "Spotify".
    pub app: Option<String>,
}

impl From<NowPlaying> for MediaState {
    fn from(n: NowPlaying) -> Self {
        let app = (!n.source.is_empty()).then(|| hoverpill_media::Source::new(n.source).label);
        Self { title: n.title, artist: n.artist, playing: n.playing, artwork: n.artwork, app }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Allow,
    Deny,
    /// Hand the question back to Claude Code's own terminal prompt.
    Ask,
    #[serde(rename = "always_allow")]
    AlwaysAllow,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaAction {
    PlayPause,
    Next,
    Previous,
}

/// Shows a message from HoverPill itself through the notification view.
pub fn notify(app: &AppHandle, title: &str, body: &str) {
    send(
        app,
        "pill:notification",
        NotificationItem { id: next_id("hoverpill"), app: "HoverPill".into(), title: title.into(), body: body.into(), icon: None },
    );
}

impl From<hoverpill_notify::Notification> for NotificationItem {
    fn from(n: hoverpill_notify::Notification) -> Self {
        Self { id: next_id("os"), app: n.app, title: n.title, body: n.body, icon: n.icon }
    }
}

#[tauri::command]
pub fn resolve_permission(id: String, decision: Decision, broker: State<Broker>) {
    if !broker.resolve(&id, decision) {
        // Demo cards from the tray, or a request Claude already abandoned.
        println!("hoverpill: no pending Claude request {id}");
    }
}

#[tauri::command]
pub fn media_command(action: MediaAction, media: State<Media>) {
    media.send(match action {
        MediaAction::PlayPause => Action::PlayPause,
        MediaAction::Next => Action::Next,
        MediaAction::Previous => Action::Previous,
    });
}

/// The UI asks once on load; after that, changes arrive as `pill:media` events.
#[tauri::command]
pub fn current_media(media: State<Media>, settings: State<SettingsStore>) -> Option<MediaState> {
    media.current().filter(|_| settings.get().media.enabled).map(MediaState::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisions_deserialize_from_the_ui() {
        let parse = |s: &str| serde_json::from_str::<Decision>(s).unwrap();
        assert!(matches!(parse(r#""allow""#), Decision::Allow));
        assert!(matches!(parse(r#""deny""#), Decision::Deny));
        assert!(matches!(parse(r#""ask""#), Decision::Ask));
        assert!(matches!(parse(r#""always_allow""#), Decision::AlwaysAllow));
    }
}
