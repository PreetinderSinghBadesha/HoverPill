//! Camera and microphone use, for the little dots in the closed notch and the app names in the
//! open one.
//!
//! Windows keeps a record of every app that uses a camera or microphone: the
//! `CapabilityAccessManager\ConsentStore` keys under HKCU are written when an app starts using a
//! device (`LastUsedTimeStart`) and when it stops (`LastUsedTimeStop`). Packaged apps get a key
//! named after their package family name; desktop apps are under `NonPackaged`, named after their
//! executable. That is the same information the camera/microphone icons in Windows' own taskbar
//! use, and it names the app.
//!
//! Linux is a follow-up: PipeWire/PulseAudio can say whether the microphone is in use, but not
//! which app has it, so nothing is emitted there yet.

#![cfg_attr(not(windows), allow(dead_code))]

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

/// An app currently using a capability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessEntry {
    /// App name to show, e.g. "Camera" or "Google Chrome".
    pub app: String,
    /// What Windows calls it: the package family name, or the executable's path.
    pub id: String,
    /// When it started, Unix ms.
    pub started_at: u64,
}

/// The apps using one capability.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessState {
    pub entries: Vec<AccessEntry>,
    /// Any app at all: what the dot follows.
    pub active: bool,
}

/// Camera and microphone together.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyState {
    pub camera: AccessState,
    pub microphone: AccessState,
}

#[cfg(windows)]
mod platform {
    use super::*;

    use std::time::Duration;

    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    use winreg::RegKey;

    use crate::events::send;

    /// Comfortably quicker than someone notices a video call starting; a registry read is cheap.
    const POLL: Duration = Duration::from_millis(700);

    fn capability(name: &str) -> AccessState {
        // HKCU, because the per-user record is the one that names the app; the machine-wide
        // policy key below `HKLM` only says whether the capability is allowed at all.
        let path = format!(
            r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\{name}"
        );
        let Ok(store) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(&path, KEY_READ) else {
            // No key: nothing has ever used it, or this Windows version doesn't keep the record.
            return AccessState::default();
        };

        let mut entries = Vec::new();
        // Packaged (Store/MSIX) apps: the key is the package family name.
        for id in store.enum_keys().flatten() {
            if id.eq_ignore_ascii_case("NonPackaged") {
                continue;
            }
            if let Ok(key) = store.open_subkey_with_flags(&id, KEY_READ) {
                if let Some(entry) = entry(&key, &id, None) {
                    entries.push(entry);
                }
            }
        }
        // Desktop apps: `NonPackaged\<executable path, with # for \>`.
        if let Ok(desktop) = store.open_subkey_with_flags("NonPackaged", KEY_READ) {
            for id in desktop.enum_keys().flatten() {
                if let Ok(key) = desktop.open_subkey_with_flags(&id, KEY_READ) {
                    if let Some(entry) = entry(&key, &id, Some(&id)) {
                        entries.push(entry);
                    }
                }
            }
        }

        entries.sort_by_key(|e| e.started_at);
        AccessState { active: !entries.is_empty(), entries }
    }

    /// One app's record, if it is using the capability *right now*: started and not stopped.
    fn entry(key: &RegKey, id: &str, path: Option<&str>) -> Option<AccessEntry> {
        let started = timestamp(key, "LastUsedTimeStart")?;
        // Some builds leave the stop value out while an app is active; absent means "still on".
        let stopped = timestamp(key, "LastUsedTimeStop").unwrap_or(0);
        if started == 0 || stopped != 0 {
            return None;
        }
        let app = path
            .map(friendly_path)
            .or_else(|| key.get_value::<String, _>("DisplayName").ok().filter(|s| !s.trim().is_empty()))
            .unwrap_or_else(|| friendly_package(id));
        Some(AccessEntry { app, id: id.to_string(), started_at: filetime_ms(started) })
    }

    /// `LastUsedTimeStart`/`Stop` are FILETIMEs; Windows has written them both as a binary value
    /// and as a QWORD over the years, and both are eight little-endian bytes.
    fn timestamp(key: &RegKey, name: &str) -> Option<u64> {
        let raw = key.get_raw_value(name).ok()?;
        let bytes: [u8; 8] = raw.bytes.as_ref().try_into().ok()?;
        Some(u64::from_le_bytes(bytes))
    }

    /// FILETIME (100 ns since 1601) to Unix milliseconds.
    pub(super) fn filetime_ms(filetime: u64) -> u64 {
        (filetime / 10_000).saturating_sub(11_644_473_600_000)
    }

    /// `C:#Program Files#Google#Chrome#Application#chrome.exe` → "Chrome".
    pub(super) fn friendly_path(id: &str) -> String {
        let path = id.replace('#', r"\");
        let name = path.rsplit(['\\', '/']).next().unwrap_or(&path);
        let name = name.strip_suffix(".exe").or_else(|| name.strip_suffix(".EXE")).unwrap_or(name);
        split_words(name)
    }

    /// `Microsoft.WindowsCamera_8wekyb3d8bbwe` → "Windows Camera".
    pub(super) fn friendly_package(id: &str) -> String {
        let name = id.split('_').next().unwrap_or(id);
        let name = name.rsplit('.').next().unwrap_or(name);
        let name = name.strip_prefix("Microsoft.").unwrap_or(name);
        split_words(name)
    }

    /// `WindowsCamera` → "Windows Camera"; leaves names that already have spaces alone.
    fn split_words(name: &str) -> String {
        let mut out = String::with_capacity(name.len() + 4);
        for (i, c) in name.char_indices() {
            if i > 0 && c.is_uppercase() {
                let prev = name[..i].chars().next_back().unwrap_or(' ');
                if prev.is_lowercase() || prev.is_ascii_digit() {
                    out.push(' ');
                }
            }
            out.push(if i == 0 { c.to_ascii_uppercase() } else { c });
        }
        out
    }

    pub fn read() -> Result<PrivacyState, String> {
        Ok(PrivacyState { camera: capability("webcam"), microphone: capability("microphone") })
    }

    pub fn spawn_worker(app: AppHandle) {
        std::thread::Builder::new()
            .name("hoverpill-access".into())
            .spawn(move || {
                let mut last = PrivacyState::default();
                loop {
                    match read() {
                        Ok(state) => {
                            if state != last {
                                send(&app, "pill:privacy", state.clone());
                                last = state;
                            }
                        }
                        Err(e) => eprintln!("hoverpill: reading camera/microphone use failed: {e}"),
                    }
                    std::thread::sleep(POLL);
                }
            })
            .expect("spawning the camera/microphone thread");
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    /// Nothing to watch yet. PipeWire/PulseAudio for the microphone is the follow-up.
    pub fn spawn_worker(_app: AppHandle) {}
}

/// Watches camera and microphone use and pushes `pill:privacy` whenever it changes.
pub fn spawn_worker(app: AppHandle) {
    platform::spawn_worker(app);
}

#[cfg(all(test, windows))]
mod tests {
    use super::platform;

    #[test]
    fn names_read_well() {
        assert_eq!(platform::friendly_path(r"C:#Program Files#Google#Chrome#Application#chrome.exe"), "Chrome");
        assert_eq!(platform::friendly_package("Microsoft.WindowsCamera_8wekyb3d8bbwe"), "Windows Camera");
        assert_eq!(platform::friendly_package("Microsoft.SkypeApp_kzf8qxf38zg5c"), "Skype App");
        assert_eq!(platform::friendly_package("SpotifyAB.SpotifyMusic_zpdnekdrzrea0"), "Spotify Music");
    }

    #[test]
    fn filetimes_become_unix_milliseconds() {
        // 2024-01-01T00:00:00Z, as a FILETIME.
        assert_eq!(platform::filetime_ms(133_485_408_000_000_000), 1_704_067_200_000);
    }
}
