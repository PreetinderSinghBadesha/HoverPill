//! GNOME Shell companion that lets us see native Wayland apps fullscreen.
//!
//! The pill runs under XWayland, so an app talking directly to the compositor
//! (Firefox on Ubuntu, GTK4 apps) paints over it invisibly: it has no X window,
//! and every shell introspection API is allowlisted for the desktop portals
//! alone, so no app can query GNOME about Wayland windows. On GNOME this
//! module can install a tiny companion extension owned above and available on
//! the session bus as `org.hoverpill.Notch`, which answers
//! `IsFullscreen(monitor)` from inside the shell. Whether it exists is the
//! user's call — the `gnomeHelper` display setting is off by default, and the
//! extension is installed and enabled only while it's on, then removed again.
//! GNOME only loads extensions at session start, so a fresh install takes one
//! logout. Elsewhere (KDE, wlroots) there is nothing to install into and
//! polling just fails, so fullscreen hiding stays X11-only until a
//! KWin/foreign-toplevel bridge exists.

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

/// What to do to the helper on disk. `on` is the setting; `installed` says whether its files are
/// already there.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Plan {
    /// Write the files and add our uuid: a helper that wasn't there at all is one the user asked
    /// for.
    Install,
    /// Keep the files current but leave `enabled-extensions` alone. Whether an installed helper
    /// runs is the user's call — a switch-off in GNOME's Extensions app has to stick.
    KeepFiles,
    /// Remove the files and our uuid.
    Remove,
}

fn plan(on: bool, installed: bool) -> Plan {
    match (on, installed) {
        (false, _) => Plan::Remove,
        (true, false) => Plan::Install,
        (true, true) => Plan::KeepFiles,
    }
}

/// Applies the helper setting. Switched on it writes the extension if it's missing or changed and
/// enables it — but only when it wasn't installed at all, so a helper the user turned off in
/// GNOME's own Extensions app stays off. Switched off it removes the files and drops our uuid,
/// which unloads a running helper and its bus name. A no-op off GNOME. Called once at startup and
/// again whenever the setting changes.
pub fn sync(app: &AppHandle, on: bool) {
    if !gnome_session() {
        return;
    }
    let Some(home) = app.path().home_dir().ok() else { return };
    let dir = home.join(".local/share/gnome-shell/extensions").join(UUID);

    let plan = plan(on, dir.exists());
    if plan == Plan::Remove {
        remove(&dir);
        disable_uuid();
        return;
    }

    // Both writes are wanted even when the first one says "already current".
    let wrote = update(&dir.join("metadata.json"), METADATA).unwrap_or(false)
        | update(&dir.join("extension.js"), EXTENSION).unwrap_or(false);
    // Only a helper that wasn't on disk at this start gets its uuid added: one that was already
    // there is the user's to enable, and adding the uuid back would undo their switch-off.
    let enabled = if plan == Plan::Install { enable_uuid(app) } else { Enabled::LeftAlone };
    // GNOME reads the extensions directory once, at session start, so a helper just installed (or
    // just enabled) while the session is running answers nothing until the next login. Say so
    // when that happens — not at every start, which would nag the user who already knows.
    if needs_logout_notice(wrote || matches!(enabled, Enabled::Added), extension_loaded()) {
        events::notify(
            app,
            "HoverPill: one logout to finish setup",
            "Log out and back in once so the Wayland fullscreen helper can load. After that, fullscreen apps hide the notch too.",
        );
    }
}

/// Takes the helper off disk. Quiet: switching the setting off shouldn't produce a notification.
/// A running shell unloads it as soon as `disable_uuid` — which `sync` calls next — drops the
/// uuid, and the bus name goes with it.
fn remove(dir: &Path) {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {}
        // The ordinary case: the setting was already off.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => eprintln!("hoverpill: can't remove {}: {e}", dir.display()),
    }
}

/// Whether to mention the logout: only right after the helper was written or enabled, and only
/// while the running session still hasn't loaded it.
fn needs_logout_notice(changed: bool, loaded: Option<bool>) -> bool {
    changed && loaded == Some(false)
}

/// GNOME sessions list themselves in `XDG_CURRENT_DESKTOP` ("GNOME", but also
/// "ubuntu:GNOME" or "GNOME-Flashback").
pub fn gnome_session() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktop| {
        desktop.to_ascii_lowercase().split(':').any(|part| matches!(part, "gnome" | "unity"))
    })
}

/// A GNOME session running under Wayland (the pill itself uses XWayland, but the session is
/// still Wayland: `XDG_SESSION_TYPE` and `WAYLAND_DISPLAY` are untouched by `GDK_BACKEND`).
/// X key grabs don't work there, so shortcuts go through GNOME's own keybindings instead.
pub fn gnome_wayland() -> bool {
    let wayland = std::env::var("XDG_SESSION_TYPE").is_ok_and(|kind| kind.eq_ignore_ascii_case("wayland"))
        || std::env::var_os("WAYLAND_DISPLAY").is_some();
    wayland && gnome_session()
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

/// The trimmed text of the `enabled-extensions` array and its items, as `gsettings get` prints
/// them. `None` when the text isn't a recognizable array, in which case the user's setting is
/// left alone.
fn array_items(list: &str) -> Option<(&str, Vec<&str>)> {
    // GVariant prints the empty array of a non-defaulted type as `@as []`.
    let value = list.trim();
    let value = value.strip_prefix("@as").map(str::trim_start).unwrap_or(value);
    let inner = value.strip_prefix('[')?.strip_suffix(']')?;
    let items = inner.split(',').map(str::trim).filter(|item| !item.is_empty()).collect();
    Some((value, items))
}

/// Merges `uuid` into the text of the `enabled-extensions` array. The flag says whether the uuid
/// was missing, i.e. whether there is anything worth writing back.
fn merged(list: &str, uuid: &str) -> Option<(String, bool)> {
    let (value, items) = array_items(list)?;
    if items.iter().any(|item| item.contains(uuid)) {
        return Some((value.to_string(), false));
    }
    let text = if items.is_empty() {
        format!("['{uuid}']")
    } else {
        format!("[{}, '{uuid}']", items.join(", "))
    };
    Some((text, true))
}

/// The other direction: drops `uuid` from the text of the `enabled-extensions` array. The flag
/// says whether the uuid was there, i.e. whether there is anything worth writing back.
fn pruned(list: &str, uuid: &str) -> Option<(String, bool)> {
    let (value, items) = array_items(list)?;
    let kept: Vec<&str> = items.iter().copied().filter(|item| !item.contains(uuid)).collect();
    if kept.len() == items.len() {
        return Some((value.to_string(), false));
    }
    // `@as []` rather than `[]`: an empty literal has no type for GVariant to infer.
    let text = if kept.is_empty() { "@as []".to_string() } else { format!("[{}]", kept.join(", ")) };
    Some((text, true))
}

/// What making sure our uuid is in `enabled-extensions` did.
#[derive(Clone, Copy)]
enum Enabled {
    /// It was already there; nothing to write.
    Already,
    /// It was missing and has just been added.
    Added,
    /// The helper was on disk already, so its enablement is the user's to set; untouched.
    LeftAlone,
    /// `gsettings` isn't there, refused, or the list can't be parsed.
    Failed,
}

/// Adds our uuid to the enabled extensions; `Added` when the setting had to change. The running
/// shell only knows about extensions loaded at session start, but the setting persists, so the
/// next login picks both the files and the enablement up in one go.
fn enable_uuid(app: &AppHandle) -> Enabled {
    let out = Command::new("gsettings").args(["get", "org.gnome.shell", "enabled-extensions"]).output();
    let Ok(out) = out else { return Enabled::Failed };
    if !out.status.success() {
        return Enabled::Failed;
    }
    let current = String::from_utf8_lossy(&out.stdout);
    let Some((new_list, adding)) = merged(&current, UUID) else {
        return Enabled::Failed;
    };
    if !adding {
        return Enabled::Already;
    }
    let applied = Command::new("gsettings")
        .args(["set", "org.gnome.shell", "enabled-extensions", &new_list])
        .status()
        .is_ok_and(|status| status.success());
    if !applied {
        events::notify(app, "Can't enable the Wayland fullscreen helper", "Reinstall HoverPill or enable the extension by hand (GNOME Extensions).");
        return Enabled::Failed;
    }
    Enabled::Added
}

/// The other half of `enable_uuid`: drops our uuid from the enabled extensions, which makes a
/// running shell unload the helper and release `org.hoverpill.Notch`. `gsettings` refusing is
/// logged, not notified — the files are gone either way, and a stale uuid in the list is inert.
fn disable_uuid() {
    let out = Command::new("gsettings").args(["get", "org.gnome.shell", "enabled-extensions"]).output();
    let Ok(out) = out else { return };
    if !out.status.success() {
        return;
    }
    let current = String::from_utf8_lossy(&out.stdout);
    let Some((new_list, removing)) = pruned(&current, UUID) else {
        return;
    };
    if !removing {
        return;
    }
    let applied = Command::new("gsettings")
        .args(["set", "org.gnome.shell", "enabled-extensions", &new_list])
        .status()
        .is_ok_and(|status| status.success());
    if !applied {
        eprintln!("hoverpill: can't remove {UUID} from the enabled extensions");
    }
}

/// Whether the running session has the extension loaded (i.e. the user has logged out since it was
/// installed). `None` when there is no `gnome-extensions` tool to ask; its output is captured
/// rather than printed, so a missing extension says nothing on stderr.
fn extension_loaded() -> Option<bool> {
    Command::new("gnome-extensions").args(["info", UUID]).output().ok().map(|out| out.status.success())
}

#[cfg(test)]
mod tests {
    use super::{merged, needs_logout_notice, plan, pruned, Plan};

    #[test]
    fn merges_into_every_shape_gsettings_prints() {
        assert_eq!(merged("[]", "a@b"), Some(("['a@b']".to_string(), true)));
        assert_eq!(merged("@as []", "a@b"), Some(("['a@b']".to_string(), true)));
        assert_eq!(merged("['x@y']", "a@b"), Some(("['x@y', 'a@b']".to_string(), true)));
        assert_eq!(merged("['x@y','z@w']", "a@b"), Some(("['x@y', 'z@w', 'a@b']".to_string(), true)));
    }

    #[test]
    fn merging_is_idempotent_and_leaves_unparsable_settings_alone() {
        // Already there: no write, so `enable_uuid` doesn't report a fresh install either.
        assert_eq!(merged("['a@b']", "a@b"), Some(("['a@b']".to_string(), false)));
        assert_eq!(merged("@as ['a@b']", "a@b"), Some(("['a@b']".to_string(), false)));
        assert_eq!(merged("", "a@b"), None);
        assert_eq!(merged("{'keys': []}", "a@b"), None);
    }

    #[test]
    fn prunes_the_uuid_and_leaves_the_rest_alone() {
        assert_eq!(pruned("['a@b']", "a@b"), Some(("@as []".to_string(), true)));
        assert_eq!(pruned("@as ['a@b']", "a@b"), Some(("@as []".to_string(), true)));
        assert_eq!(pruned("['a@b', 'x@y']", "a@b"), Some(("['x@y']".to_string(), true)));
        assert_eq!(pruned("['x@y']", "a@b"), Some(("['x@y']".to_string(), false)));
        assert_eq!(pruned("@as []", "a@b"), Some(("[]".to_string(), false)));
        assert_eq!(pruned("", "a@b"), None);
        assert_eq!(pruned("{'keys': []}", "a@b"), None);
    }

    #[test]
    fn an_installed_helper_is_left_to_the_user() {
        // Switching off always takes the helper away, installed or not.
        assert_eq!(plan(false, true), Plan::Remove);
        assert_eq!(plan(false, false), Plan::Remove);
        // Switching on installs and enables what isn't there yet, but never puts the uuid back
        // into a helper that is already installed: turning it off in GNOME's own Extensions app
        // has to stick across restarts.
        assert_eq!(plan(true, false), Plan::Install);
        assert_eq!(plan(true, true), Plan::KeepFiles);
    }

    #[test]
    fn the_logout_notice_only_follows_a_fresh_helper() {
        // Just written or just enabled, and the running shell still hasn't loaded it: say it once.
        assert!(needs_logout_notice(true, Some(false)));
        // Loaded already, or nothing changed this start (a second launch before the logout).
        assert!(!needs_logout_notice(true, Some(true)));
        assert!(!needs_logout_notice(false, Some(false)));
        // No `gnome-extensions` tool to ask, so we can't know: stay quiet.
        assert!(!needs_logout_notice(true, None));
    }
}
