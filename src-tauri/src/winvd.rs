//! Windows-only: keep the pill's window on every virtual desktop.
//!
//! There is no documented API for "show on all desktops", so this talks to the shell's
//! undocumented `IVirtualDesktopPinnedApps` service (the same one Task View uses for its
//! "Show this window on all desktops" toggle) through the `ImmersiveShell` COM class. We pin the
//! window's *view* (by HWND), which works for a plain Win32 window with no Start-menu shortcut.
//!
//! The service/interface GUID changed between Windows versions, so we try each known value. It is
//! entirely best-effort: on a build we don't recognise, or before the shell is ready, this logs
//! and falls back to the pill living on a single desktop.

use core::ffi::c_void;

use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::{HWND, RPC_E_CHANGED_MODE, S_OK};
use windows_sys::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};

/// `CLSID_ImmersiveShell`: the shell object exposing `IServiceProvider`.
const CLSID_IMMERSIVE_SHELL: GUID = GUID::from_u128(0xc2f03a33_21f5_47fa_b4bb_156362a2f239);
/// `IID_IServiceProvider`.
const IID_ISERVICE_PROVIDER: GUID = GUID::from_u128(0x6d5140c1_7436_11ce_8034_00aa006009fa);
/// `SID`/`IID_VirtualDesktopPinnedApps` on Windows 10 1903+ and Windows 11.
const IID_VIRTUAL_DESKTOP_PINNED_APPS_1903: GUID = GUID::from_u128(0xb5a399e7_1c87_46b8_88e9_fc5747b171bd);
/// Same, on Windows 10 1809.
const IID_VIRTUAL_DESKTOP_PINNED_APPS_1809: GUID = GUID::from_u128(0x4ce81583_1e4c_4632_a621_07a53543148f);

type QueryInterfaceFn = unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32;
type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;

/// The leading part of every COM interface's vtable.
#[repr(C)]
struct IUnknownVtbl {
    query_interface: QueryInterfaceFn,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: ReleaseFn,
}

#[repr(C)]
struct IServiceProviderVtbl {
    base: IUnknownVtbl,
    query_service: unsafe extern "system" fn(*mut c_void, *const GUID, *const GUID, *mut *mut c_void) -> i32,
}

/// `IVirtualDesktopPinnedApps`, in declaration order (MScholtes/VirtualDesktop). Only the two
/// methods we call are typed; the earlier entries hold the vtable slots in place.
#[repr(C)]
struct IVirtualDesktopPinnedAppsVtbl {
    base: IUnknownVtbl,
    is_app_id_pinned: *const c_void,
    pin_app_id: *const c_void,
    unpin_app_id: *const c_void,
    is_view_pinned: *const c_void,
    pin_view: unsafe extern "system" fn(*mut c_void, HWND) -> i32,
    unpin_view: unsafe extern "system" fn(*mut c_void, HWND) -> i32,
}

/// Reads the vtable pointer out of a COM interface pointer.
unsafe fn vtbl<T>(obj: *mut c_void) -> &'static T {
    &**(obj as *mut *const T)
}

/// Pins (or unpins) `hwnd` to every virtual desktop. `hwnd` is the pill's native window handle.
pub fn set_view_on_all_desktops(all: bool, hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    let hwnd = hwnd as HWND;
    // SAFETY: all raw COM below is guarded by the vtable layouts and HRESULT checks above.
    let outcome = unsafe { pin(all, hwnd) };
    if let Err(why) = outcome {
        eprintln!("hoverpill: can't set all-desktops ({why}); pill stays on one desktop");
    }
}

unsafe fn pin(all: bool, hwnd: HWND) -> Result<(), &'static str> {
    let hr = CoInitializeEx(core::ptr::null(), COINIT_APARTMENTTHREADED as u32);
    // RPC_E_CHANGED_MODE: an earlier caller already initialised COM on this thread with another
    // model. We may use it, but must not uninitialise it.
    if hr != S_OK && hr != RPC_E_CHANGED_MODE {
        return Err("CoInitializeEx failed");
    }
    let owns_com = hr == S_OK;

    let result = {
        let mut shell: *mut c_void = core::ptr::null_mut();
        let hr = CoCreateInstance(
            &CLSID_IMMERSIVE_SHELL,
            core::ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_ISERVICE_PROVIDER,
            &mut shell,
        );
        if hr != S_OK || shell.is_null() {
            Err("ImmersiveShell unavailable")
        } else {
            let provider = vtbl::<IServiceProviderVtbl>(shell);
            let mut applied = Err("no known IVirtualDesktopPinnedApps on this build");
            for iid in [IID_VIRTUAL_DESKTOP_PINNED_APPS_1903, IID_VIRTUAL_DESKTOP_PINNED_APPS_1809] {
                let mut pinned_apps: *mut c_void = core::ptr::null_mut();
                let hr = (provider.query_service)(shell, &iid, &iid, &mut pinned_apps);
                if hr != S_OK || pinned_apps.is_null() {
                    continue;
                }
                let api = vtbl::<IVirtualDesktopPinnedAppsVtbl>(pinned_apps);
                let hr = if all { (api.pin_view)(pinned_apps, hwnd) } else { (api.unpin_view)(pinned_apps, hwnd) };
                (api.base.release)(pinned_apps);
                applied = if hr == S_OK { Ok(()) } else { Err("pin/unpin view was rejected") };
                break;
            }
            (provider.base.release)(shell);
            applied
        }
    };

    if owns_com {
        CoUninitialize();
    }
    result
}
