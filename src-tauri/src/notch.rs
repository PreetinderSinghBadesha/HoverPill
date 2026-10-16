//! The floating window: placement at the top-center of the chosen monitor, staying on top,
//! and selective click-through.
//!
//! The window is always click-through except while the notch is *open* (or showing a card you
//! must click, like a Claude permission request) and the cursor is over it. A closed notch sits
//! on top of browser tab strips and title bars, so it never takes clicks: they go to whatever is
//! underneath. To open it you push the pointer against the top edge of the screen above it, or
//! (if enabled) rest the pointer on it for a moment.
//!
//! A background thread polls the global cursor because a click-through window receives no
//! mouse events. On Windows that poll is how hover is detected. On Linux the window shapes its
//! own X input region to the notch (see `set_input_region`) and the GTK enter/leave signals are
//! used instead: under XWayland the global cursor freezes the moment the real pointer leaves that
//! region, so polling would strand the notch open. Either way the webview reports the notch's box
//! and whether it's interactive via `set_hit_rect`, and is told to open or close via `pill:hover`.

use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Monitor, PhysicalPosition, PhysicalSize, State, WebviewWindow};

use crate::appbar;
use crate::settings::{Display, SettingsStore};

pub const LABEL: &str = "pill";

/// Window size in logical px at scale 1 with no top gap. Must fit the largest notch view (see
/// `SIZES` in src/pill/Pill.tsx) plus its shadow.
const WINDOW_WIDTH: f64 = 720.0;
const WINDOW_HEIGHT: f64 = 340.0;

/// ~60 Hz keeps hover feeling instant; each tick is one GetCursorPos/XQueryPointer.
const POLL: Duration = Duration::from_millis(16);
/// Every ~1s: re-assert z-order, follow monitor/DPI changes, refresh cached geometry.
const SLOW_TICK_EVERY: u64 = 60;
/// Slack around the notch (CSS px) so its edge isn't a hair trigger.
const HIT_PADDING: f64 = 4.0;
/// "Pushed against the top edge": within this many physical px of the monitor's top.
const EDGE_PX: f64 = 2.0;
/// How long the pointer must rest on a closed notch to open it (when that's enabled). Long
/// enough that reaching for a browser tab underneath doesn't open it. Measured in real time:
/// `sleep(16ms)` can take twice that on Windows, so counting ticks would drift.
const DWELL: Duration = Duration::from_millis(600);
/// The top-edge trigger is at least this wide either side of the notch's centre (CSS px), so
/// the thin idle bar is still easy to open.
const EDGE_HALF_WIDTH: f64 = 110.0;
/// How often to look for a fullscreen app (~0.3 s at the poll rate).
const FULLSCREEN_CHECK_EVERY: u64 = 20;
/// Height (CSS px, before scale) of the strip "Keep windows below the notch" reserves: the
/// closed notch with something playing. Cards and the open notch still drop over windows.
const RESERVED_HEIGHT: f64 = 32.0;

/// The notch's box in CSS px, relative to the window's top-left corner.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Default)]
pub struct HitState {
    shape: Mutex<Option<(Rect, bool)>>,
    /// Someone is typing in the notch (asking ChatGPT): it keeps the keyboard and stays open.
    typing: AtomicBool,
}

/// The notch normally never takes keyboard focus. While the user types a question it has to,
/// so this lifts that for as long as the text box is focused, and puts it back afterwards.
#[tauri::command]
pub fn notch_typing(active: bool, app: AppHandle, state: State<HitState>) {
    state.typing.store(active, Ordering::Relaxed);
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.set_focusable(active);
        if active {
            let _ = win.set_focus();
        }
    }
}

/// `interactive`: the notch is open, or shows something that must be clickable. Otherwise it
/// stays click-through even under the cursor.
#[tauri::command]
pub fn set_hit_rect(rect: Rect, interactive: bool, state: State<HitState>) {
    *state.shape.lock().unwrap() = Some((rect, interactive));
}

/// Whether the pointer is over the notch. On Linux this is driven by the window's GTK
/// enter/leave signals (see `set_input_region`): under XWayland the global cursor Rust polls
/// freezes the moment the real pointer leaves the notch's X input region.
#[derive(Default)]
pub struct HoverState(#[cfg_attr(not(target_os = "linux"), allow(dead_code))] pub AtomicBool);

/// The monitor chosen in settings, falling back to the primary one if it's unplugged.
fn chosen_monitor(win: &WebviewWindow, wanted: Option<&str>) -> Result<Monitor, Box<dyn Error>> {
    if let Some(wanted) = wanted {
        if let Some(m) = win.available_monitors()?.into_iter().find(|m| m.name().map(String::as_str) == Some(wanted)) {
            return Ok(m);
        }
    }
    match win.primary_monitor()? {
        Some(m) => Ok(m),
        None => Ok(win.current_monitor()?.ok_or("no monitor found")?),
    }
}

/// Where everything should be, in physical screen pixels.
struct Target {
    /// Window position and size: horizontally centered at the top of the chosen monitor, sized
    /// for the notch's scale and top gap (both applied in CSS; see src/pill/Pill.tsx).
    pos: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    /// The chosen monitor's bounds: x, y, width, height.
    monitor: (i32, i32, i32, i32),
    /// The strip to keep windows out of, if "Keep windows below the notch" is on.
    reserve: Option<appbar::Strip>,
}

fn target(win: &WebviewWindow) -> Result<Target, Box<dyn Error>> {
    let display = win.try_state::<SettingsStore>().map(|s| s.get().display).unwrap_or_default();
    let monitor = chosen_monitor(win, display.monitor.as_deref())?;
    let dpi = monitor.scale_factor();
    let width = WINDOW_WIDTH * display.scale;
    let height = display.top_offset + WINDOW_HEIGHT * display.scale;
    let size = PhysicalSize::new((width * dpi).round() as u32, (height * dpi).round() as u32);
    let origin = *monitor.position();
    let (mw, mh) = (monitor.size().width as i32, monitor.size().height as i32);
    let x = origin.x + (mw - size.width as i32) / 2;
    let reserve = display.reserve_space.then(|| {
        let strip = ((display.top_offset + RESERVED_HEIGHT * display.scale) * dpi).round() as i32;
        (origin.x, origin.y, origin.x + mw, origin.y + strip)
    });
    Ok(Target { pos: PhysicalPosition::new(x, origin.y), size, monitor: (origin.x, origin.y, mw, mh), reserve })
}

/// The native window handle as an integer (0 where there isn't a Win32 one).
fn native_handle(win: &WebviewWindow) -> isize {
    #[cfg(windows)]
    return win.hwnd().map(|h| h.0 as isize).unwrap_or(0);
    #[cfg(not(windows))]
    {
        let _ = win;
        0
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    name: String,
    label: String,
    primary: bool,
}

#[tauri::command]
pub fn list_monitors(app: AppHandle) -> Result<Vec<MonitorInfo>, String> {
    let primary = app.primary_monitor().map_err(|e| e.to_string())?.and_then(|m| m.name().cloned());
    let monitors = app.available_monitors().map_err(|e| e.to_string())?;
    Ok(monitors
        .into_iter()
        .enumerate()
        .filter_map(|(i, m)| {
            let name = m.name()?.clone();
            let size = m.size();
            let is_primary = primary.as_ref() == Some(&name);
            let label = format!(
                "Display {} — {}×{}{}",
                i + 1,
                size.width,
                size.height,
                if is_primary { " (primary)" } else { "" }
            );
            Some(MonitorInfo { name, label, primary: is_primary })
        })
        .collect())
}

/// Moves/resizes the window only if it isn't already where it should be, so this is cheap to
/// call periodically (monitors get plugged in, resolution and DPI scale change).
pub fn place_top_center(win: &WebviewWindow) -> Result<(), Box<dyn Error>> {
    let t = target(win)?;
    // Position first: moving onto a monitor with a different DPI rescales the window.
    if win.outer_position()? != t.pos {
        win.set_position(t.pos)?;
    }
    if win.inner_size()? != t.size {
        win.set_size(t.size)?;
    }
    appbar::sync(native_handle(win), t.reserve);
    Ok(())
}

/// Cached so the 60 Hz loop makes one cross-thread call (cursor) instead of several.
struct Geometry {
    origin: PhysicalPosition<i32>,
    scale: f64,
    monitor: (i32, i32, i32, i32),
}

impl Geometry {
    fn read(win: &WebviewWindow) -> Option<Self> {
        Some(Self {
            origin: win.inner_position().ok()?,
            scale: win.scale_factor().ok()?,
            monitor: target(win).ok()?.monitor,
        })
    }
}

/// Where the cursor is relative to the notch.
#[derive(Default, Clone, Copy)]
struct Pointer {
    /// Over the notch's box.
    over: bool,
    /// Pushed against the top edge of the screen, horizontally within the notch. The notch may
    /// sit lower than the edge (a top gap), so this isn't implied by `over`.
    at_edge: bool,
    interactive: bool,
}

fn read_pointer(win: &WebviewWindow, geo: &Geometry) -> Pointer {
    let Some((r, interactive)) = *win.state::<HitState>().shape.lock().unwrap() else {
        return Pointer::default();
    };
    let Ok(cursor) = win.cursor_position() else {
        return Pointer { interactive, ..Pointer::default() };
    };
    let x = (cursor.x - geo.origin.x as f64) / geo.scale;
    let y = (cursor.y - geo.origin.y as f64) / geo.scale;
    let within_x = x >= r.x - HIT_PADDING && x <= r.x + r.width + HIT_PADDING;
    // The idle bar is narrow, so the top-edge zone is at least EDGE_HALF_WIDTH either side of
    // its centre: easy to hit without aiming.
    let centre = r.x + r.width / 2.0;
    let within_edge_zone = (x - centre).abs() <= (r.width / 2.0 + HIT_PADDING).max(EDGE_HALF_WIDTH);
    // Pushing against the pill's own top edge.
    let at_window_top = cursor.y - geo.origin.y as f64 <= EDGE_PX && cursor.y >= geo.origin.y as f64;
    // On Linux the window manager (e.g. GNOME) keeps the pill below the top bar, so its top edge
    // is lower than the screen's. The README promises opening by pushing against "the top edge of
    // the screen", so also fire from the monitor's own top edge there.
    #[cfg(target_os = "linux")]
    let at_edge = within_edge_zone
        && (at_window_top || (cursor.y - geo.monitor.1 as f64 <= EDGE_PX && cursor.y >= geo.monitor.1 as f64));
    #[cfg(not(target_os = "linux"))]
    let at_edge = within_edge_zone && at_window_top;
    Pointer {
        over: within_x && y >= r.y - HIT_PADDING && y <= r.y + r.height + HIT_PADDING,
        at_edge,
        interactive,
    }
}

/// Linux/XWayland only: shape the window's X *input* region to the notch's own box so the X
/// server actually delivers pointer motion to us there.
///
/// A fully click-through window receives no cursor updates, so under XWayland a real mouse can
/// never be seen near the top: `cursor_position()` just returns wherever the pointer last left an
/// X surface. Shaping the input region (XShape) to the notch's box makes that box a real X
/// surface for the pointer, so hovering it and pushing its top edge work with a real mouse, while
/// everywhere else in the window stays click-through. `None` sets an empty region (fully
/// click-through). GTK must be touched on the main thread, so this hops there; the rect is in
/// window coordinates (GDK scales to device pixels itself, matching `read_pointer`).
#[cfg(target_os = "linux")]
fn set_input_region(win: &WebviewWindow, rect: Option<Rect>) {
    let win = win.clone();
    let handle = win.clone();
    let _ = handle.run_on_main_thread(move || {
        use gtk::prelude::*;
        let Ok(gtk_win) = win.gtk_window() else { return };
        let Some(gdk_win) = gtk_win.window() else { return };
        if !HOVER_SIGNALS_CONNECTED.swap(true, Ordering::Relaxed) {
            // GTK's own crossing events are the authoritative hover signal on Linux: X delivers
            // Enter/LeaveNotify from real pointer motion at the input-region boundary, whereas the
            // cursor Tauri polls (`XQueryPointer`) freezes the moment the real pointer leaves it.
            let app = win.app_handle().clone();
            gtk_win.connect_enter_notify_event(move |_, _| {
                if let Some(s) = app.try_state::<HoverState>() {
                    s.0.store(true, Ordering::Relaxed);
                }
                gtk::glib::Propagation::Proceed
            });
            let app = win.app_handle().clone();
            gtk_win.connect_leave_notify_event(move |_, _| {
                if let Some(s) = app.try_state::<HoverState>() {
                    s.0.store(false, Ordering::Relaxed);
                }
                gtk::glib::Propagation::Proceed
            });
        }
        let region = match rect {
            Some(r) => gtk::gdk::cairo::Region::create_rectangle(&gtk::gdk::cairo::RectangleInt::new(
                r.x.round() as i32,
                r.y.round() as i32,
                r.width.round().max(1.0) as i32,
                r.height.round().max(1.0) as i32,
            )),
            None => gtk::gdk::cairo::Region::create(),
        };
        gdk_win.input_shape_combine_region(&region, 0, 0);
    });
}

#[cfg(target_os = "linux")]
static HOVER_SIGNALS_CONNECTED: AtomicBool = AtomicBool::new(false);

/// The all-workspaces state we last applied on Linux (-1 = not applied yet), so the hit-test loop
/// keeps re-asking until the GTK window exists (`stick` is a no-op before it's realized).
#[cfg(target_os = "linux")]
static STICKY_STATE: std::sync::atomic::AtomicI8 = std::sync::atomic::AtomicI8::new(-1);

/// Keeps the pill on every virtual desktop / workspace. Linux asks the window manager to make the
/// window sticky (`_NET_WM_STATE_STICKY` via GDK); Windows pins the window's view (`winvd`).
/// Best-effort: on a platform/desktop that can't do it, the pill just stays on the current one.
pub fn set_all_workspaces(win: &WebviewWindow, all: bool) {
    #[cfg(target_os = "linux")]
    {
        let win = win.clone();
        let handle = win.clone();
        let _ = handle.run_on_main_thread(move || {
            use gtk::prelude::*;
            // Nothing to do until the GTK window is realized; the loop calls us again until then.
            let Ok(gtk_win) = win.gtk_window() else { return };
            let Some(gdk_win) = gtk_win.window() else { return };
            if all {
                gdk_win.stick();
            } else {
                gdk_win.unstick();
            }
            STICKY_STATE.store(if all { 1 } else { 0 }, Ordering::Relaxed);
        });
    }
    #[cfg(windows)]
    crate::winvd::set_view_on_all_desktops(all, native_handle(win));
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = (win, all);
    }
}

/// Whether the foreground window covers the whole monitor, i.e. a fullscreen video, game or
/// presentation (or a browser in F11). The desktop and the taskbar don't count, and neither
/// does a maximised window: with an auto-hiding taskbar those cover the whole monitor too, but
/// they're ordinary windows (IsZoomed), not fullscreen apps.
#[cfg(windows)]
fn fullscreen_app_on(win: &WebviewWindow, (mx, my, mw, mh): (i32, i32, i32, i32)) -> bool {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowRect, IsZoomed};
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() || fg as isize == native_handle(win) || IsZoomed(fg) != 0 {
            return false;
        }
        let mut class = [0u16; 64];
        let len = GetClassNameW(fg, class.as_mut_ptr(), class.len() as i32).max(0) as usize;
        if matches!(String::from_utf16_lossy(&class[..len]).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd") {
            return false;
        }
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(fg, &mut r) != 0 && r.left <= mx && r.top <= my && r.right >= mx + mw && r.bottom >= my + mh
    }
}

#[cfg(not(windows))]
fn fullscreen_app_on(_win: &WebviewWindow, _monitor: (i32, i32, i32, i32)) -> bool {
    false
}

pub fn spawn_hit_test_loop(win: WebviewWindow) {
    std::thread::spawn(move || {
        let mut geo = Geometry::read(&win);
        let mut open = false;
        #[cfg(not(target_os = "linux"))]
        let mut accepting_input = false;
        let mut resting_since: Option<Instant> = None;
        let mut display = Display::default();
        let mut fullscreen = false;
        let mut peek = false;
        let mut focus = FocusGuard::default();
        // Linux: follow the reported notch box as the X input region; start fully click-through.
        #[cfg(target_os = "linux")]
        let mut last_region: Option<Rect> = None;
        #[cfg(target_os = "linux")]
        set_input_region(&win, None);
        #[cfg(not(target_os = "linux"))]
        let _ = win.set_ignore_cursor_events(true);
        let mut ticks: u64 = 0;
        loop {
            let tick = ticks;
            ticks = ticks.wrapping_add(1);
            std::thread::sleep(POLL);

            if tick.is_multiple_of(SLOW_TICK_EVERY) {
                if let Err(e) = place_top_center(&win) {
                    eprintln!("hoverpill: placement failed: {e}");
                }
                reassert_topmost(&win);
                geo = Geometry::read(&win);
                if let Some(store) = win.try_state::<SettingsStore>() {
                    display = store.get().display;
                }
                // Linux: apply once the GTK window is realized, and re-apply on every change. The
                // startup call in `setup` runs before the window exists, so this is the real one.
                #[cfg(target_os = "linux")]
                if STICKY_STATE.load(Ordering::Relaxed) != i8::from(display.all_workspaces) {
                    set_all_workspaces(&win, display.all_workspaces);
                }
            }

            if tick.is_multiple_of(FULLSCREEN_CHECK_EVERY) {
                let now = display.hide_in_fullscreen && geo.as_ref().is_some_and(|g| fullscreen_app_on(&win, g.monitor));
                if now != fullscreen {
                    fullscreen = now;
                    let _ = win.emit("pill:fullscreen", fullscreen);
                }
            }

            let p = geo.as_ref().map(|g| read_pointer(&win, g)).unwrap_or_default();
            // Linux: the window's GTK enter/leave signal is authoritative (see `HoverState`); the
            // polled cursor sticks at the input region's edge once the real pointer leaves it.
            #[cfg(target_os = "linux")]
            let p = {
                let mut p = p;
                p.over = win.state::<HoverState>().0.load(Ordering::Relaxed);
                p
            };
            if !p.over || p.interactive {
                resting_since = None;
            } else if resting_since.is_none() {
                resting_since = Some(Instant::now());
            }
            let rested = resting_since.is_some_and(|t| t.elapsed() >= DWELL);

            // Interactive (open, or a card with buttons): stays open while the pointer is over it.
            // Closed: only the top-edge push or a long rest opens it; it never takes clicks.
            // Behind a fullscreen app, nothing opens it (video players put their controls at the
            // top edge too).
            // Under XWayland the pill owns its own input region (see `set_input_region`), so a real
            // cursor over the closed pill is a genuine hover: open on it directly. On Windows the
            // pill stays click-through when closed, so hovering it can't be detected and the
            // top-edge push / dwell are the only triggers.
            #[cfg(target_os = "linux")]
            let hover_open = p.over;
            #[cfg(not(target_os = "linux"))]
            let hover_open = false;

            let typing = win.state::<HitState>().typing.load(Ordering::Relaxed);
            let want_open = if typing {
                true
            } else if p.interactive {
                p.over || p.at_edge
            } else if fullscreen {
                false
            } else {
                p.at_edge || hover_open || (display.open_on_hover && rested)
            };
            if want_open != open {
                open = want_open;
                if open {
                    focus.remember(&win);
                }
                let _ = win.emit("pill:hover", open);
            }

            // Pointing at the closed notch fades it out so whatever is underneath shows.
            let want_peek = p.over && !p.interactive && !open;
            if want_peek != peek {
                peek = want_peek;
                let _ = win.emit("pill:peek", peek);
            }

            // Linux: keep the X input region on the reported notch box so a real cursor is seen
            // over the pill (and its clicks land on us) while everything else stays click-through.
            #[cfg(target_os = "linux")]
            {
                let rect = win.state::<HitState>().shape.lock().unwrap().map(|(r, _)| r);
                if rect != last_region || tick % SLOW_TICK_EVERY == 0 {
                    last_region = rect;
                    set_input_region(&win, rect);
                }
            }
            // Windows: the cursor is always readable, so a plain all-or-nothing toggle is fine.
            #[cfg(not(target_os = "linux"))]
            {
                let want_input = p.interactive && p.over;
                if want_input != accepting_input {
                    accepting_input = want_input;
                    let _ = win.set_ignore_cursor_events(!want_input);
                }
            }
            if !typing {
                focus.give_back_if_taken(&win);
            }
        }
    });
}

/// The pill must never keep keyboard focus: clicking Approve shouldn't pull you out of your
/// terminal. `"focusable": false` (WS_EX_NOACTIVATE on Windows, no input hint on X11) prevents
/// most activation, but WebView2 lives in a separate process and its child windows can still
/// activate us when clicked. So remember who had the foreground when the cursor arrived, and
/// hand it straight back if we take it.
#[derive(Default)]
struct FocusGuard {
    /// HWND as an integer so the guard can live on the polling thread.
    #[cfg(windows)]
    previous: Option<isize>,
}

#[cfg(windows)]
impl FocusGuard {
    fn remember(&mut self, win: &WebviewWindow) {
        use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        let Ok(ours) = win.hwnd() else { return };
        let foreground = unsafe { GetForegroundWindow() };
        if !foreground.is_null() && !std::ptr::eq(foreground, ours.0) {
            self.previous = Some(foreground as isize);
        }
    }

    fn give_back_if_taken(&mut self, win: &WebviewWindow) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindow, SetForegroundWindow};
        let Ok(ours) = win.hwnd() else { return };
        if !std::ptr::eq(unsafe { GetForegroundWindow() }, ours.0) {
            return;
        }
        if let Some(previous) = self.previous {
            // We're the foreground window right now, so Windows lets us pass it on.
            unsafe {
                if IsWindow(previous as _) != 0 {
                    SetForegroundWindow(previous as _);
                }
            }
        }
    }
}

#[cfg(not(windows))]
impl FocusGuard {
    fn remember(&mut self, _win: &WebviewWindow) {}
    fn give_back_if_taken(&mut self, _win: &WebviewWindow) {}
}

/// Other "topmost" windows (borderless-fullscreen video players, the taskbar after it's clicked)
/// can end up above us; pushing ourselves back to the top of the topmost band fixes that.
/// Exclusive-fullscreen games bypass the desktop compositor, and nothing can draw over them.
#[cfg(windows)]
fn reassert_topmost(win: &WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
    };
    if let Ok(hwnd) = win.hwnd() {
        unsafe {
            SetWindowPos(
                hwnd.0 as _,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
    }
}

#[cfg(not(windows))]
fn reassert_topmost(win: &WebviewWindow) {
    let _ = win.set_always_on_top(true);
}

/// Native Wayland doesn't let a client position itself, stay above other windows, or read the
/// global cursor position, so run under XWayland until a wlr-layer-shell backend exists
/// (see docs/ARCHITECTURE.md). Must run before GTK initialises.
#[cfg(target_os = "linux")]
pub fn linux_preflight() {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("GDK_BACKEND").is_none() {
        std::env::set_var("GDK_BACKEND", "x11");
    }
}
