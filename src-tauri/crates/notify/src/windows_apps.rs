//! Which apps Windows lets send notifications, by the name you'd recognise.
//!
//! Windows keeps one registry key per app that has ever sent a toast (the same list as
//! Settings › System › Notifications), keyed by AppUserModelID. IDs are named through the
//! Start menu's app folder (`shell:AppsFolder`), which covers desktop apps like Discord and
//! Brave, then through the Store app catalogue for packaged apps without a Start menu entry.

use std::collections::HashMap;

use windows::core::{Interface, HSTRING, PWSTR};
use windows::ApplicationModel::AppInfo;
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{BHID_EnumItems, IEnumShellItems, IShellItem, IShellItem2, SHCreateItemFromParsingName, SIGDN_NORMALDISPLAY};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

const SENDERS_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Notifications\Settings";

pub fn notifying_apps() -> Vec<String> {
    // The shell APIs need COM on the calling thread; a fresh thread means the caller's
    // apartment (or lack of one) doesn't matter.
    std::thread::spawn(|| {
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        collect()
    })
    .join()
    .unwrap_or_default()
}

fn collect() -> Vec<String> {
    let start_menu = start_menu_names();
    let mut names: Vec<String> = registered_senders()
        .into_iter()
        .filter_map(|id| start_menu.get(&id).cloned().or_else(|| store_name(&id)))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}

/// AppUserModelIDs that may send toasts, minus ones turned off in Windows Settings and
/// Windows' own system toasts and tray-icon placeholders, which nobody would recognise.
fn registered_senders() -> Vec<String> {
    let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(SENDERS_KEY) else {
        return Vec::new();
    };
    key.enum_keys()
        .filter_map(Result::ok)
        .filter(|id| !id.starts_with("NotifyIconGeneratedAumid_") && !id.starts_with("Windows.SystemToast."))
        .filter(|id| key.open_subkey(id).ok().and_then(|k| k.get_value::<u32, _>("Enabled").ok()) != Some(0))
        .collect()
}

/// AppUserModelID → name for everything in the Start menu's app list.
fn start_menu_names() -> HashMap<String, String> {
    let mut names = HashMap::new();
    unsafe {
        let Ok(folder) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from("shell:AppsFolder"), None) else {
            return names;
        };
        let Ok(items) = folder.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems) else {
            return names;
        };
        loop {
            let mut batch = [None];
            let mut fetched = 0u32;
            if items.Next(&mut batch, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(item) = batch[0].take() else { break };
            let Ok(item2) = item.cast::<IShellItem2>() else { continue };
            let Ok(id) = item2.GetString(&PKEY_AppUserModel_ID) else { continue };
            let id = take(id);
            if let Ok(name) = item.GetDisplayName(SIGDN_NORMALDISPLAY) {
                names.insert(id, take(name));
            }
        }
    }
    names
}

/// Copies a shell-allocated string and frees the original.
unsafe fn take(p: PWSTR) -> String {
    let s = p.to_string().unwrap_or_default();
    CoTaskMemFree(Some(p.0 as _));
    s
}

fn store_name(id: &str) -> Option<String> {
    let name = AppInfo::GetFromAppUserModelId(&HSTRING::from(id)).ok()?.DisplayInfo().ok()?.DisplayName().ok()?;
    Some(name.to_string_lossy()).filter(|n| !n.is_empty())
}
