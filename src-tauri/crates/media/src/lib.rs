//! What's playing on the system, and play/pause/next/previous for it.
//!
//! - Windows: Global System Media Transport Controls (GSMTC), the API behind the volume flyout.
//! - Linux: MPRIS over the D-Bus session bus.
//!
//! Each backend runs on its own thread. OS change events (plus a slow poll, because both APIs
//! occasionally miss events) trigger a re-read, and the callback fires only when what's shown
//! would actually change.

use std::collections::HashSet;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(not(any(windows, target_os = "linux")), path = "unsupported.rs")]
mod backend;

/// Fallback re-read interval for when an OS event gets lost.
const POLL: Duration = Duration::from_millis(1500);
/// Covers album art for any sane cover; anything larger is skipped rather than shipped to the UI.
const MAX_ARTWORK_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    /// `data:` URL, or an `https:` URL straight from the player.
    pub artwork: Option<String>,
    /// Which player this is (see [`Source`]).
    pub source: String,
}

/// A player as the OS identifies it: the AppUserModelID on Windows (`Spotify.exe`, `chrome`),
/// the MPRIS bus-name suffix on Linux (`spotify`, `firefox`). Stable across restarts, so it's
/// what the "ignore this player" setting stores.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Source {
    pub id: String,
    pub label: String,
}

impl Source {
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        Self { label: label_for(&id), id }
    }
}

/// A readable name for a source id; ids are often exe names, package names or hashes.
fn label_for(id: &str) -> String {
    let app = id.rsplit('!').next().unwrap_or(id); // packaged apps: "Family_hash!AppId"
    let app = app.rsplit(['\\', '/']).next().unwrap_or(app);
    let app = app.strip_suffix(".exe").or_else(|| app.strip_suffix(".EXE")).unwrap_or(app);
    let known = match app.to_ascii_lowercase().as_str() {
        "308046b0af4a39cb" | "firefox" => Some("Firefox"), // Firefox registers under a hash on Windows
        "chrome" | "google-chrome" | "chromium" => Some("Chrome"),
        "msedge" => Some("Microsoft Edge"),
        "brave" | "brave-browser" => Some("Brave"),
        "spotify" => Some("Spotify"),
        "vlc" => Some("VLC"),
        _ => None,
    };
    if let Some(name) = known {
        return name.to_string();
    }
    let last = app.rsplit('.').next().unwrap_or(app);
    let mut chars = last.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => id.to_string(),
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Action {
    PlayPause,
    Next,
    Previous,
}

enum Msg {
    Refresh,
    Command(Action),
}

/// Lets a backend's OS event handlers ask the worker to re-read.
type Notify = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Shared {
    current: Option<NowPlaying>,
    /// Every player seen on the last read, including ignored ones.
    sources: Vec<Source>,
    ignored: HashSet<String>,
}

pub struct Media {
    tx: Sender<Msg>,
    shared: Arc<Mutex<Shared>>,
}

impl Media {
    pub fn spawn(on_change: impl Fn(Option<NowPlaying>) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let worker_shared = Arc::clone(&shared);
        let refresh_tx = tx.clone();
        let notify: Notify = Arc::new(move || {
            let _ = refresh_tx.send(Msg::Refresh);
        });

        std::thread::Builder::new()
            .name("hoverpill-media".into())
            .spawn(move || {
                let mut backend = match backend::Backend::new(notify) {
                    Ok(b) => b,
                    Err(e) => return eprintln!("hoverpill-media: unavailable: {e}"),
                };
                loop {
                    // Handle whatever is queued (a burst of events becomes one re-read).
                    let mut msg = match rx.recv_timeout(POLL) {
                        Ok(m) => Some(m),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => return,
                    };
                    while let Some(m) = msg {
                        if let Msg::Command(action) = m {
                            if let Err(e) = backend.command(action) {
                                eprintln!("hoverpill-media: {action:?} failed: {e}");
                            }
                        }
                        msg = rx.try_recv().ok();
                    }

                    let ignored = worker_shared.lock().unwrap().ignored.clone();
                    let (now, sources) = backend.read(&ignored).unwrap_or_else(|e| {
                        eprintln!("hoverpill-media: read failed: {e}");
                        (None, Vec::new())
                    });
                    let mut shared = worker_shared.lock().unwrap();
                    shared.sources = sources;
                    if shared.current != now {
                        shared.current = now.clone();
                        drop(shared);
                        on_change(now);
                    }
                }
            })
            .expect("spawning the media thread");

        Self { tx, shared }
    }

    /// Latest known state, for a UI that starts listening after the first change.
    pub fn current(&self) -> Option<NowPlaying> {
        self.shared.lock().unwrap().current.clone()
    }

    /// Players currently known to the OS, ignored ones included.
    pub fn sources(&self) -> Vec<Source> {
        self.shared.lock().unwrap().sources.clone()
    }

    /// Players (by [`Source::id`]) to skip when choosing what to show.
    pub fn set_ignored(&self, ids: impl IntoIterator<Item = String>) {
        self.shared.lock().unwrap().ignored = ids.into_iter().collect();
        let _ = self.tx.send(Msg::Refresh);
    }

    pub fn send(&self, action: Action) {
        let _ = self.tx.send(Msg::Command(action));
    }
}

/// Fetching artwork is the slow part, so keep it until the track changes.
#[derive(Default)]
struct ArtCache {
    key: Option<(String, String)>,
    value: Option<String>,
}

impl ArtCache {
    fn get_or(&mut self, title: &str, artist: &str, fetch: impl FnOnce() -> Option<String>) -> Option<String> {
        let key = (title.to_string(), artist.to_string());
        if self.key.as_ref() != Some(&key) {
            self.value = fetch();
            self.key = Some(key);
        }
        self.value.clone()
    }
}

fn data_url(mime: &str, bytes: &[u8]) -> Option<String> {
    use base64::Engine;
    if bytes.is_empty() || bytes.len() > MAX_ARTWORK_BYTES {
        return None;
    }
    Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn art_cache_fetches_once_per_track() {
        let mut cache = ArtCache::default();
        let mut calls = 0;
        for _ in 0..3 {
            cache.get_or("Song", "Artist", || {
                calls += 1;
                Some("a".into())
            });
        }
        assert_eq!(calls, 1);
        let next = cache.get_or("Other", "Artist", || Some("b".into()));
        assert_eq!(next.as_deref(), Some("b"));
    }

    #[test]
    fn source_labels() {
        assert_eq!(Source::new("Spotify.exe").label, "Spotify");
        assert_eq!(Source::new("308046B0AF4A39CB").label, "Firefox");
        assert_eq!(Source::new("MSEdge").label, "Microsoft Edge");
        assert_eq!(Source::new("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic").label, "ZuneMusic");
        assert_eq!(Source::new("C:\\Tools\\foobar2000.exe").label, "Foobar2000");
        assert_eq!(Source::new("rhythmbox").label, "Rhythmbox");
    }

    #[test]
    fn data_url_encodes_and_limits() {
        assert_eq!(data_url("image/png", b"hi").as_deref(), Some("data:image/png;base64,aGk="));
        assert!(data_url("image/png", b"").is_none());
        assert!(data_url("image/png", &vec![0; MAX_ARTWORK_BYTES + 1]).is_none());
    }
}
