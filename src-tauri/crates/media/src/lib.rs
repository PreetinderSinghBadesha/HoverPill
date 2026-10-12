//! What's playing on the system, and play/pause/next/previous for it.
//!
//! - Windows: Global System Media Transport Controls (GSMTC), the API behind the volume flyout.
//! - Linux: MPRIS over the D-Bus session bus.
//!
//! Each backend runs on its own thread. OS change events (plus a slow poll, because both APIs
//! occasionally miss events) trigger a re-read, and the callback fires only when what's shown
//! would actually change.

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

pub struct Media {
    tx: Sender<Msg>,
    current: Arc<Mutex<Option<NowPlaying>>>,
}

impl Media {
    pub fn spawn(on_change: impl Fn(Option<NowPlaying>) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let current = Arc::new(Mutex::new(None));
        let shared = Arc::clone(&current);
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

                    let now = backend.read().unwrap_or_else(|e| {
                        eprintln!("hoverpill-media: read failed: {e}");
                        None
                    });
                    let mut current = shared.lock().unwrap();
                    if *current != now {
                        *current = now.clone();
                        drop(current);
                        on_change(now);
                    }
                }
            })
            .expect("spawning the media thread");

        Self { tx, current }
    }

    /// Latest known state, for a UI that starts listening after the first change.
    pub fn current(&self) -> Option<NowPlaying> {
        self.current.lock().unwrap().clone()
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
    fn data_url_encodes_and_limits() {
        assert_eq!(data_url("image/png", b"hi").as_deref(), Some("data:image/png;base64,aGk="));
        assert!(data_url("image/png", b"").is_none());
        assert!(data_url("image/png", &vec![0; MAX_ARTWORK_BYTES + 1]).is_none());
    }
}
