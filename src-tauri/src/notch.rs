//! The floating window: placement at the top-center of the primary monitor, staying on top,
//! and selective click-through.
//!
//! Click-through works the same way on every platform: the window is created click-through
//! (`set_ignore_cursor_events(true)`), and a background thread polls the global cursor. When the
//! cursor enters the pill's on-screen box (reported by the webview via `set_hit_rect`), the
//! window starts accepting input and the UI is told it is hovered; when it leaves, the window
//! goes back to click-through. The webview can't detect hover by itself because a click-through
//! window receives no mouse events at all.

use std::error::Error;
use std::sync::Mutex;
use std::time::Duration;

use serde::Deserialize;
use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewWindow};

pub const LABEL: &str = "pill";

/// Window size in logical px. Must fit the largest pill view (see `SIZES` in src/pill/Pill.tsx)
/// plus its shadow.
const WINDOW_WIDTH: f64 = 560.0;
const WINDOW_HEIGHT: f64 = 240.0;

/// ~60 Hz keeps hover feeling instant; each tick is one GetCursorPos/XQueryPointer.
const POLL: Duration = Duration::from_millis(16);
/// Every ~1s: re-assert z-order, follow monitor/DPI changes, refresh cached geometry.
const SLOW_TICK_EVERY: u64 = 60;
/// Slack around the pill (CSS px) so the edge isn't a hair trigger.
const HIT_PADDING: f64 = 4.0;

/// The pill's box in CSS px, relative to the window's top-left corner.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Default)]
pub struct HitState {
    rect: Mutex<Option<Rect>>,
}

#[tauri::command]
pub fn set_hit_rect(rect: Rect, state: State<HitState>) {
    *state.rect.lock().unwrap() = Some(rect);
}

/// Where the window should be: horizontally centered, flush with the top of the primary monitor.
fn target_geometry(win: &WebviewWindow) -> Result<(PhysicalPosition<i32>, PhysicalSize<u32>), Box<dyn Error>> {
    let monitor = match win.primary_monitor()? {
        Some(m) => m,
        None => win.current_monitor()?.ok_or("no monitor found")?,
    };
    let scale = monitor.scale_factor();
    let size = PhysicalSize::new((WINDOW_WIDTH * scale).round() as u32, (WINDOW_HEIGHT * scale).round() as u32);
    let origin = monitor.position();
    let x = origin.x + (monitor.size().width as i32 - size.width as i32) / 2;
    Ok((PhysicalPosition::new(x, origin.y), size))
}

/// Moves/resizes the window only if it isn't already where it should be, so this is cheap to
/// call periodically (monitors get plugged in, resolution and DPI scale change).
pub fn place_top_center(win: &WebviewWindow) -> Result<(), Box<dyn Error>> {
    let (pos, size) = target_geometry(win)?;
    // Position first: moving onto a monitor with a different DPI rescales the window.
    if win.outer_position()? != pos {
        win.set_position(pos)?;
    }
    if win.inner_size()? != size {
        win.set_size(size)?;
    }
    Ok(())
}

/// Cached so the 60 Hz loop makes one cross-thread call (cursor) instead of three.
struct Geometry {
    origin: PhysicalPosition<i32>,
    scale: f64,
}

impl Geometry {
    fn read(win: &WebviewWindow) -> Option<Self> {
        Some(Self { origin: win.inner_position().ok()?, scale: win.scale_factor().ok()? })
    }
}

fn cursor_over_pill(win: &WebviewWindow, geo: &Geometry) -> bool {
    let Some(r) = *win.state::<HitState>().rect.lock().unwrap() else {
        return false;
    };
    let Ok(cursor) = win.cursor_position() else {
        return false;
    };
    let x = (cursor.x - geo.origin.x as f64) / geo.scale;
    let y = (cursor.y - geo.origin.y as f64) / geo.scale;
    x >= r.x - HIT_PADDING
        && x <= r.x + r.width + HIT_PADDING
        && y >= r.y - HIT_PADDING
        && y <= r.y + r.height + HIT_PADDING
}

pub fn spawn_hit_test_loop(win: WebviewWindow) {
    std::thread::spawn(move || {
        let mut geo = Geometry::read(&win);
        let mut inside = false;
        let mut focus = FocusGuard::default();
        for tick in 0u64.. {
            std::thread::sleep(POLL);

            if tick % SLOW_TICK_EVERY == 0 {
                if let Err(e) = place_top_center(&win) {
                    eprintln!("hoverpill: placement failed: {e}");
                }
                reassert_topmost(&win);
                geo = Geometry::read(&win);
            }

            let now_inside = geo.as_ref().is_some_and(|g| cursor_over_pill(&win, g));
            if now_inside != inside {
                inside = now_inside;
                if inside {
                    focus.remember(&win);
                }
                let _ = win.set_ignore_cursor_events(!inside);
                let _ = win.emit("pill:hover", inside);
            }
            focus.give_back_if_taken(&win);
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
        if !foreground.is_null() && foreground != ours.0 as _ {
            self.previous = Some(foreground as isize);
        }
    }

    fn give_back_if_taken(&mut self, win: &WebviewWindow) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindow, SetForegroundWindow};
        let Ok(ours) = win.hwnd() else { return };
        if unsafe { GetForegroundWindow() } != ours.0 as _ {
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
