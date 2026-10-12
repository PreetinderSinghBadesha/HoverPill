//! MPRIS: every player that owns an `org.mpris.MediaPlayer2.*` name on the session bus
//! (Spotify, Firefox, Chromium, VLC, mpv with mpv-mpris, ...).

use std::collections::HashMap;

use zbus::blocking::fdo::DBusProxy;
use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::message::Type as MessageType;
use zbus::zvariant::{Array, OwnedValue};
use zbus::MatchRule;

use crate::{data_url, Action, ArtCache, NowPlaying, Notify, MAX_ARTWORK_BYTES};

const BUS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";

type Metadata = HashMap<String, OwnedValue>;

pub struct Backend {
    conn: Connection,
    /// Bus name of the player being shown; kept while it's paused so the pill doesn't jump
    /// between players.
    current: Option<String>,
    art: ArtCache,
}

fn err(e: zbus::Error) -> String {
    e.to_string()
}

impl Backend {
    pub fn new(notify: Notify) -> Result<Self, String> {
        let conn = Connection::session().map_err(err)?;

        // Every MPRIS player emits PropertiesChanged on the same object path.
        let rule = MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface("org.freedesktop.DBus.Properties")
            .and_then(|b| b.member("PropertiesChanged"))
            .and_then(|b| b.path(OBJECT_PATH))
            .map_err(err)?
            .build();
        let signals = MessageIterator::for_match_rule(rule, &conn, Some(64)).map_err(err)?;
        std::thread::Builder::new()
            .name("hoverpill-mpris-signals".into())
            .spawn(move || signals.for_each(|_| notify()))
            .map_err(|e| e.to_string())?;

        Ok(Self { conn, current: None, art: ArtCache::default() })
    }

    fn player(&self, bus_name: &str) -> Result<Proxy<'_>, String> {
        Proxy::new(&self.conn, bus_name.to_string(), OBJECT_PATH, PLAYER_IFACE).map_err(err)
    }

    fn status(&self, bus_name: &str) -> Option<String> {
        self.player(bus_name).ok()?.get_property::<String>("PlaybackStatus").ok()
    }

    /// A playing player wins; then the one already shown if it's merely paused; then any
    /// paused one. Stopped players aren't shown.
    fn pick(&self) -> Result<Option<(String, bool)>, String> {
        let names = DBusProxy::new(&self.conn).map_err(err)?.list_names().map_err(|e| e.to_string())?;
        let players: Vec<String> =
            names.iter().map(|n| n.as_str().to_string()).filter(|n| n.starts_with(BUS_PREFIX)).collect();
        let statuses: Vec<(String, String)> =
            players.into_iter().filter_map(|n| self.status(&n).map(|s| (n, s))).collect();

        if let Some((name, _)) = statuses.iter().find(|(_, s)| s == "Playing") {
            return Ok(Some((name.clone(), true)));
        }
        let paused = |name: &String| statuses.iter().any(|(n, s)| n == name && s == "Paused");
        if let Some(cur) = self.current.as_ref().filter(|c| paused(c)) {
            return Ok(Some((cur.clone(), false)));
        }
        Ok(statuses.into_iter().find(|(_, s)| s == "Paused").map(|(n, _)| (n, false)))
    }

    pub fn read(&mut self) -> Result<Option<NowPlaying>, String> {
        let picked = self.pick()?;
        self.current = picked.as_ref().map(|(n, _)| n.clone());
        let Some((name, playing)) = picked else { return Ok(None) };

        let meta: Metadata = self.player(&name)?.get_property("Metadata").map_err(|e| e.to_string())?;
        let title = meta_str(&meta, "xesam:title").unwrap_or_default();
        if title.is_empty() {
            return Ok(None);
        }
        let artist = meta_artists(&meta).join(", ");
        let art_url = meta_str(&meta, "mpris:artUrl");
        let artwork = self.art.get_or(&title, &artist, || art_url.as_deref().and_then(artwork_from_url));
        Ok(Some(NowPlaying { title, artist, playing, artwork }))
    }

    pub fn command(&mut self, action: Action) -> Result<(), String> {
        let Some(name) = self.current.clone() else { return Ok(()) };
        let method = match action {
            Action::PlayPause => "PlayPause",
            Action::Next => "Next",
            Action::Previous => "Previous",
        };
        self.player(&name)?.call_method(method, &()).map(drop).map_err(err)
    }
}

fn meta_str(meta: &Metadata, key: &str) -> Option<String> {
    meta.get(key)?.downcast_ref::<&str>().ok().map(str::to_string)
}

/// `xesam:artist` is a string list per the spec, but some players send a plain string.
fn meta_artists(meta: &Metadata) -> Vec<String> {
    let Some(value) = meta.get("xesam:artist") else { return Vec::new() };
    if let Ok(single) = value.downcast_ref::<&str>() {
        return vec![single.to_string()];
    }
    match value.downcast_ref::<&Array>() {
        Ok(list) => list.iter().filter_map(|v| v.downcast_ref::<&str>().ok().map(str::to_string)).collect(),
        Err(_) => Vec::new(),
    }
}

/// `https:` URLs (Spotify, browsers) go to the UI as is; local files (VLC, Rhythmbox, Chromium's
/// temp covers) are inlined because the webview can't load `file:` URLs.
fn artwork_from_url(url: &str) -> Option<String> {
    if url.starts_with("https://") {
        return Some(url.to_string());
    }
    let path = percent_decode(url.strip_prefix("file://")?);
    let meta = std::fs::metadata(&path).ok()?;
    if meta.len() as usize > MAX_ARTWORK_BYTES {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    data_url(mime_for(&path), &bytes)
}

fn mime_for(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else {
        "image/jpeg" // most covers, and Chromium writes extension-less JPEGs
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_file_urls() {
        assert_eq!(percent_decode("/home/me/My%20Music/cover%2B1.jpg"), "/home/me/My Music/cover+1.jpg");
        assert_eq!(percent_decode("/bad%zz"), "/bad%zz");
    }

    #[test]
    fn https_art_passes_through() {
        assert_eq!(artwork_from_url("https://i.scdn.co/image/abc").as_deref(), Some("https://i.scdn.co/image/abc"));
        assert_eq!(artwork_from_url("http://insecure/x.jpg"), None);
    }
}
