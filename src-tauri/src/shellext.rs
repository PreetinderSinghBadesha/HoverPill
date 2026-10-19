//! GNOME Shell companion that lets us see native Wayland apps fullscreen.
//!
//! The pill runs under XWayland, so an app talking directly to the compositor
//! (Firefox on Ubuntu, GTK4 apps) paints over it invisibly: it has no X window,
//! and every shell introspection API is allowlisted for the desktop portals
//! alone, so no app can query GNOME about Wayland windows. On GNOME this
//! module installs a tiny companion extension owned above and available on the
//! session bus as `org.hoverpill.Notch`, which answers
//! `IsFullscreen(monitor)` from inside the shell; GNOME only loads extensions
//! at session start, so a fresh install takes one logout. Elsewhere (KDE,
//! wlroots) there is nothing to install into and polling just fails, so
//! fullscreen hiding stays X11-only until a KWin/foreign-toplevel bridge
//! exists.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use zbus::blocking::{Connection, Proxy};

use crate::events;

pub const UUID: &str = "hoverpill@hoverpill.app";
const METADATA: &str = include_str!("../gnome-shell/hoverpill@hoverpill.app/metadata.json");
const EXTENSION: &str = include_str!("../gnome-shell/hoverpill@hoverpill.app/extension.js");
const NAME: &str = "org.hoverpill.Notch";
const PATH: &str = "/org/hoverpill/Notch";
/// How long a failed poll keeps us from trying again, so a system without the
/// extension costs one D-Bus round trip every few seconds rather than per tick.
const RETRY_AFTER: Duration = Duration::from_secs(2);

thread_local! {
    static BUS: RefCell<Option<Connection>> = const { RefCell::new(None) };
    static FAILED_AT: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// `true` when a fullscreen app covers monitor `index` on the active workspace,
/// as answered by the extension. `false` when there's no bus, no extension, or
/// the extension isn't loaded yet — the X11 fallback covers X clients.
pub fn fullscreen_on(monitor: usize) -> bool {
    if FAILED_AT.with(|t| t.get().is_some_and(|at| at.elapsed() < RETRY_AFTER)) {
        return false;
    }
    BUS.with(|bus| {
        let mut slot = bus.borrow_mut();
        if slot.is_none() {
            *slot = Connection::session().ok();
        }
        let Some(conn) = slot.as_ref() else {
            note_failure(|| "hoverpill: no session bus, so Wayland fullscreen apps can't be detected".into());
            return false;
        };
        let proxy = match Proxy::new(conn, NAME, PATH, NAME) {
            Ok(proxy) => proxy,
            Err(e) => {
                note_failure(|| format!("hoverpill: can't reach {NAME}: {e}"));
                return false;
            }
        };
        match proxy.call::<_, (i32,), (bool,)>("IsFullscreen", &(monitor as i32,)) {
            Ok((on,)) => {
                FAILED_AT.with(|t| t.set(None));
                on
            }
            Err(e) => {
                note_failure(|| format!("hoverpill: {NAME}.IsFullscreen: {e}"));
                false
            }
        }
    })
}

/// Records a failed poll. Only the first failure in a run of them is reported: on a system
/// without the extension (or before the logout a fresh install needs) polling fails every few
/// seconds, and a line each time would drown the log.
fn note_failure(message: impl FnOnce() -> String) {
    FAILED_AT.with(|t| {
        if t.get().is_none() {
            eprintln!("{}", message());
        }
        t.set(Some(Instant::now()));
    });
}

/// Writes the extension if it's missing or changed, makes sure GNOME will load
/// it, and nudges the user about the one logout a fresh install needs. A no-op
/// off GNOME. Called once at startup.
pub fn install(app: &AppHandle) {
    if !gnome_session() {
        return;
    }
    let Some(home) = app.path().home_dir().ok() else { return };
    let dir = home.join(".local/share/gnome-shell/extensions").join(UUID);

    let _ = update(&dir.join("metadata.json"), METADATA);
    let _ = update(&dir.join("extension.js"), EXTENSION);
    let enabled = enable_uuid(app);
    // GNOME reads the extensions directory once, at session start, so a helper installed (or
    // re-enabled) while the session is running answers nothing until the next login. Say so on
    // every start until it's loaded: someone who installs HoverPill, quits it and starts it again
    // days later would otherwise never learn why fullscreen hiding does nothing on Wayland.
    if enabled && extension_loaded() == Some(false) {
        events::notify(
            app,
            "HoverPill: one logout to finish setup",
            "Log out and back in once so the Wayland fullscreen helper can load. After that, fullscreen apps hide the notch too.",
        );
    }
}

/// GNOME sessions list themselves in `XDG_CURRENT_DESKTOP` ("GNOME", but also
/// "ubuntu:GNOME" or "GNOME-Flashback").
fn gnome_session() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktop| {
        desktop.to_ascii_lowercase().split(':').any(|part| matches!(part, "gnome" | "unity"))
    })
}

/// Writes `contents` only if the file on disk differs; `true` when written.
fn update(path: &Path, contents: &str) -> std::io::Result<bool> {
    if std::fs::read_to_string(path).ok().as_deref() == Some(contents) {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, contents)?;
    Ok(true)
}

/// Merges `uuid` into the text of the `enabled-extensions` array as `gsettings
/// get` prints it. `None` when the text isn't a recognizable array, in which
/// case the user's setting is left alone.
fn merged(list: &str, uuid: &str) -> Option<String> {
    // GVariant prints the empty array of a non-defaulted type as `@as []`.
    let value = list.trim();
    let value = value.strip_prefix("@as").map(str::trim_start).unwrap_or(value);
    let inner = value.strip_prefix('[')?.strip_suffix(']')?;
    let items: Vec<&str> = inner.split(',').map(str::trim).filter(|item| !item.is_empty()).collect();
    if items.iter().any(|item| item.contains(uuid)) {
        return Some(value.to_string());
    }
    Some(if items.is_empty() {
        format!("['{uuid}']")
    } else {
        format!("[{}, '{uuid}']", items.join(", "))
    })
}

/// Adds our uuid to the enabled extensions; `true` once the setting is as we want it. The running
/// shell only knows about extensions loaded at session start, but the setting persists, so the
/// next login picks both the files and the enablement up in one go.
fn enable_uuid(app: &AppHandle) -> bool {
    let out = Command::new("gsettings").args(["get", "org.gnome.shell", "enabled-extensions"]).output();
    let Ok(out) = out else { return false };
    if !out.status.success() {
        return false;
    }
    let Some(new_list) = merged(&String::from_utf8_lossy(&out.stdout), UUID) else {
        return false;
    };
    let applied = Command::new("gsettings")
        .args(["set", "org.gnome.shell", "enabled-extensions", &new_list])
        .status()
        .is_ok_and(|status| status.success());
    if !applied {
        events::notify(app, "Can't enable the Wayland fullscreen helper", "Reinstall HoverPill or enable the extension by hand (GNOME Extensions).");
    }
    applied
}

/// Whether the running session has the extension loaded (i.e. the user has logged out since it was
/// installed). `None` when there is no `gnome-extensions` tool to ask; its output is captured
/// rather than printed, so a missing extension says nothing on stderr.
fn extension_loaded() -> Option<bool> {
    Command::new("gnome-extensions").args(["info", UUID]).output().ok().map(|out| out.status.success())
}

#[cfg(test)]
mod tests {
    use super::merged;

    #[test]
    fn merges_into_every_shape_gsettings_prints() {
        assert_eq!(merged("[]", "a@b"), Some("['a@b']".to_string()));
        assert_eq!(merged("@as []", "a@b"), Some("['a@b']".to_string()));
        assert_eq!(merged("['x@y']", "a@b"), Some("['x@y', 'a@b']".to_string()));
        assert_eq!(merged("['x@y','z@w']", "a@b"), Some("['x@y', 'z@w', 'a@b']".to_string()));
    }

    #[test]
    fn merging_is_idempotent_and_leaves_unparsable_settings_alone() {
        assert_eq!(merged("['a@b']", "a@b"), Some("['a@b']".to_string()));
        assert_eq!(merged("", "a@b"), None);
        assert_eq!(merged("{'keys': []}", "a@b"), None);
    }
}
