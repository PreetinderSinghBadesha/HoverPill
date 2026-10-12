//! "Keep windows below the notch": registers the notch window as an application desktop
//! toolbar (AppBar) on the top edge, the same mechanism the taskbar uses, so Windows shrinks
//! the work area and maximised windows start below it.
//!
//! Windows reserves the full width of the monitor (AppBars are always edge-to-edge), so this is
//! opt-in. The reservation must be released on exit or the shell keeps the strip reserved until
//! it notices the window is gone.

/// The strip to reserve, in physical screen pixels: left, top, right, bottom.
pub type Strip = (i32, i32, i32, i32);

#[cfg(windows)]
mod imp {
    use std::sync::Mutex;

    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::Shell::{SHAppBarMessage, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, APPBARDATA};
    use windows_sys::Win32::UI::WindowsAndMessaging::WM_USER;

    use super::Strip;

    /// Notifications the shell sends to the AppBar window; nothing needs handling, so it falls
    /// through to DefWindowProc.
    const CALLBACK: u32 = WM_USER + 0x4850;

    /// The registered window (as an integer, so it can live in a static) and the strip last set.
    static REGISTERED: Mutex<Option<(isize, Strip)>> = Mutex::new(None);

    fn data(hwnd: isize, strip: Strip) -> APPBARDATA {
        APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: hwnd as _,
            uCallbackMessage: CALLBACK,
            uEdge: ABE_TOP,
            rc: RECT { left: strip.0, top: strip.1, right: strip.2, bottom: strip.3 },
            lParam: 0,
        }
    }

    /// Reserves `strip` for `hwnd`, or releases the reservation with `None`. Cheap to call
    /// repeatedly: it only talks to the shell when something changed.
    pub fn sync(hwnd: isize, strip: Option<Strip>) {
        let mut registered = REGISTERED.lock().unwrap();
        match (strip, *registered) {
            (None, None) => {}
            (None, Some((old, s))) => unsafe {
                SHAppBarMessage(ABM_REMOVE, &mut data(old, s));
                *registered = None;
            },
            (Some(strip), current) => unsafe {
                if current == Some((hwnd, strip)) {
                    return;
                }
                if current.is_none() && SHAppBarMessage(ABM_NEW, &mut data(hwnd, strip)) == 0 {
                    eprintln!("hoverpill: couldn't register the top AppBar");
                    return;
                }
                // Ask where a top bar may go (others, like a top taskbar, may already be there),
                // keep our height, then claim it.
                let height = strip.3 - strip.1;
                let mut abd = data(hwnd, strip);
                SHAppBarMessage(ABM_QUERYPOS, &mut abd);
                abd.rc.bottom = abd.rc.top + height;
                SHAppBarMessage(ABM_SETPOS, &mut abd);
                *registered = Some((hwnd, strip));
            },
        }
    }

    pub fn release() {
        let mut registered = REGISTERED.lock().unwrap();
        if let Some((hwnd, strip)) = registered.take() {
            unsafe {
                SHAppBarMessage(ABM_REMOVE, &mut data(hwnd, strip));
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Strip;

    pub fn sync(_hwnd: isize, _strip: Option<Strip>) {}
    pub fn release() {}
}

pub use imp::{release, sync};
