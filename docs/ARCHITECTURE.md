# HoverPill architecture

## 1. Stack

| Layer | Choice | Why |
|---|---|---|
| Shell | **Tauri 2** (Rust) | Uses the OS webview (WebView2 on Windows, WebKitGTK on Linux): about 10 MB on disk and about 40 MB of RAM, compared with roughly 150 MB for Electron. Rust can reach every OS API we need directly: WinRT through the `windows` crate and D-Bus through `zbus`. |
| UI | **React 19 + Motion** | Motion's physics springs animate `width`/`height`/`borderRadius` the way the iPhone island does, and `AnimatePresence` cross-fades the views. |
| IPC | Tauri events (Rust → UI) and commands (UI → Rust) | Typed on both sides (`src/lib/types.ts` and `src-tauri/src/events.rs`). |

**Alternatives I rejected**

- **Electron.** Its click-through trick, `setIgnoreMouseEvents(true, { forward: true })`, does not forward mouse moves on Linux, so hover detection would need the same Rust-style cursor polling anyway. You would pay Electron's memory cost without avoiding the hard part.
- **Qt/QML.** This is a strong option for the window itself, and Qt Quick has good animation. But the media and notification integrations would be C++ against WinRT and D-Bus, and that is more work than Rust.
- **Flutter desktop.** Transparent windows and click-through windows are still plugin territory and unreliable on Linux.

## 2. The window (built in this repo)

```
┌──────────────── transparent window, 720×340 logical, always on top ───────────────┐
│                       ◟╭──────────── notch ───────────╮◞                          │
│                         ╰──────────────────────────────╯   ← takes input only open │
│     click-through everywhere else (WS_EX_TRANSPARENT / empty X11 input shape)      │
└───────────────────────────────────────────────────────────────────────────────────┘
```

- **Fixed window, morphing notch.** The OS window never resizes. Resizing native windows at 60 fps tears and flickers, especially with WebView2. Only the DOM notch animates, inside a window that is already large enough for the biggest view.
- **Selective click-through** (`src-tauri/src/notch.rs`):
  1. The window starts click-through.
  2. The UI reports, every time the notch changes size (including every frame of the spring), its box and whether it's *interactive*. It is interactive when open, or when showing the Claude card with buttons.
  3. A Rust thread polls the global cursor at 60 Hz.
     - **Closed:** the notch never takes input, even under the cursor, so browser tabs and title bars beneath it stay clickable. It opens (`pill:hover = true`) when the pointer is pushed against the top edge of the monitor within the notch's width (≤ 2 px), or, if "open on rest" is enabled, after resting on it for 600 ms. The 600 ms is measured in real time, because `sleep(16ms)` can last up to ~31 ms on Windows. Notification cards are closed-state too: they pass clicks through.
     - **Interactive:** it takes input while the pointer is over it and closes when the pointer leaves.

  Rust has to decide all of this because a click-through window gets no DOM mouse events at all.
- **Measured on Windows:**
  - Resting on the closed notch: click-through for the first 644 ms (600 ms dwell plus UI latency), then open.
  - Pushed against the top edge: open within 350 ms.
  - Back to click-through after the pointer leaves.
  - All four tabs clickable. They were found through UI Automation and clicked with real mouse input.
- **Staying out of the way** (the closed notch otherwise hides whatever is under it, e.g. browser tab titles):
  - *Fade.* Rust emits `pill:peek` while the pointer is over the closed notch, and the notch drops to 12% opacity. "Open on rest" now defaults to off, because the two would fight; when it's on, the fade is skipped.
  - *Idle bar.* With nothing playing and no card, the notch is a 120×6 bar. The top-edge trigger stays at least ±110 px wide around its centre so the bar is still easy to open.
  - *Fullscreen.* Every ~0.3 s the loop checks whether the foreground window covers the notch's monitor. The desktop, the taskbar and maximised windows (`IsZoomed`) don't count: with an auto-hiding taskbar a maximised window covers the whole monitor too, and the first version mistook those for fullscreen apps. While a fullscreen app is up, the notch is hidden and the edge push and rest don't open it, but Claude permission cards still show. Linux has no check yet.
  - *Reserved strip* (`appbar.rs`, opt-in, Windows only). The notch window is registered as a top AppBar (`ABM_NEW`, `ABM_QUERYPOS`, `ABM_SETPOS`) for the closed notch's height plus the top gap, so the work area shrinks and maximised windows start below. AppBars always span the full monitor width. The strip is re-synced on every placement pass and released (`ABM_REMOVE`) when the setting is turned off or the app exits (`RunEvent::Exit`).
  - *Measured on Windows (125% scaling, auto-hiding taskbar):*
    - Work area top went 0 → 40 with the strip on, and back to 0 after quitting.
    - The notch hid under a fullscreen window and came back when it closed.
    - It stayed visible with a maximised window in front.
    - Pointing at it faded it.
- **Timer and stopwatch** (`src/lib/useTimers.ts`, Timer tab).
  - *Timing.* Both live in the pill webview, which never unloads, so they keep running while the notch is closed. They're anchored to wall-clock times (`endsAt`, `startedAt`), never to counted ticks, so they stay exact even when the page is throttled. The finish is a single `setTimeout` for exactly `endsAt`.
  - *Where they show.* View priority puts `alarm` right after a Claude request, ahead of everything else, and it's interactive and shown even during fullscreen. A running clock replaces the visualizer in the closed media notch, or gets its own 210 px `timer` notch when nothing is playing.
  - *Chime.* Three sine notes from Web Audio. The `AudioContext` is created on the Start click, because Chromium only allows sound after a user gesture.
  - *Tested* in the browser preview: start, pause, resume, laps, the closed-notch clock (with and without media), the alarm after a real minute, +1 min and Done.
- **Look.** True black, flat against the top edge, with concave "ears" (radial-gradient fillets) where it meets the edge. The only colour is the album art's: `useArtworkColor` samples the cover into a 24×24 canvas, weighting saturated pixels. With a top gap set, it becomes a floating pill with all corners rounded and a shadow.
- **Staying on top.** The window uses `alwaysOnTop`. Every second, a Windows-only call (`SetWindowPos(HWND_TOPMOST)`) puts it back on top, because borderless-fullscreen apps and the taskbar can rise above it. The same tick re-centers the window when monitors, resolution, or DPI change.
- **Limits:**
  - *Exclusive-fullscreen games* (DirectX/Vulkan exclusive mode) bypass the compositor, so nothing can draw over them. Borderless-fullscreen games, browsers, and video players are fine.
  - *Wayland* does not let a client place itself, stay above other windows, or read the global cursor position. `linux_preflight()` forces XWayland (`GDK_BACKEND=x11`), which works on GNOME, KDE, Sway, and Hyprland. The proper fix is a native backend built on **wlr-layer-shell** (`gtk-layer-shell`), on the OVERLAY layer with top anchoring. KDE and wlroots compositors support it; GNOME does not.
- **Focus (fixed).** Clicking the pill must not take focus away from your terminal.
  - `"focusable": false` in `tauri.conf.json` handles it. On Windows that adds `WS_EX_NOACTIVATE`; on Linux it calls GTK's `set_accept_focus(false)`.
  - On Windows, clicking Deny on a card left focus where it was. I sampled the foreground window every ~3 ms and the pill never took it. Buttons still work.
  - As a fallback (`FocusGuard` in `notch.rs`), the hit-test loop remembers which window had focus when the cursor arrived. If the pill ever becomes the foreground window anyway, it hands focus straight back. This is meant for WebView2 builds where the separate-process child window activates its parent. It never had to engage in testing.
  - Keyboard input to the pill isn't possible, which is fine because it has no text fields.
  - Linux hasn't been tested yet.

## 3. Services (next steps)

Each service is a Rust module that owns a background task, emits `pill:*` events, and exposes commands.

```
 Claude Code ──HTTP hook──▶ ClaudeBroker ─┐
 GSMTC / MPRIS ◀──────────▶ MediaService ──┼──▶ Tauri events ──▶ React pill
 Toasts / D-Bus Notify ───▶ NotifService ─┘        ▲
                                                    └── commands (resolve_permission, media_command)
```

### A. Claude Code permission broker

**Status: built** (`src-tauri/src/broker/`).

Claude Code has a **`PermissionRequest` hook**, which fires exactly when Claude would show its permission prompt. It also has a native **`http` hook type**, so no helper script is needed. The tray item **Install Claude Code hook** (`broker/install.rs`) merges this into `~/.claude/settings.json`:

```json
{
  "hooks": {
    "PermissionRequest": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "http",
            "url": "http://127.0.0.1:47823/claude/permission",
            "timeout": 600,
            "headers": { "Authorization": "Bearer <token>" }
          }
        ]
      }
    ]
  }
}
```

The token is written into the file literally, not through `allowedEnvVars`, so it works in every terminal without any shell setup. The installer:

- recognises its own entry by URL, so reinstalling replaces it rather than adding a duplicate;
- leaves every other setting and hook untouched;
- saves the previous file as `settings.json.hoverpill-backup`;
- refuses to modify a file that isn't valid JSON.

The flow:

1. Claude Code POSTs the hook input (`tool_name`, `tool_input`, `cwd`, ...). The broker is a small async HTTP/1.1 server on Tauri's Tokio runtime (`broker/http.rs`; it handles both `Content-Length` and chunked bodies).
2. The broker registers a `oneshot` channel under a new id, emits `pill:permission`, and **holds the HTTP response open**. It waits for whichever of these comes first:
   - **A decision from the pill.** The `resolve_permission` command fires the channel:
     - Approve replies `{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}`.
     - Deny replies with `"behavior":"deny"` and a message telling Claude not to try to work around the denial.
     - **Answer in terminal** replies with an empty `200`, which means "no decision", so Claude Code shows its usual prompt.
   - **The client hangs up.** This happens when the user interrupts Claude. The broker emits `pill:permission-cancel` and the card disappears.
   - **590 s pass.** This is just under the hook's 600 s timeout. The broker gives an empty reply, which falls back to the terminal prompt, and clears the card.
3. If HoverPill isn't running, the connection fails. That is a non-blocking error, so Claude Code falls back to its normal prompt.

**Card text** (`broker/summary.rs`):

| Tool | Card shows |
|---|---|
| `Bash` / `PowerShell` | the command |
| `Edit` | the file path and a diff stat |
| `MultiEdit` | the file path and the number of edits |
| `Write` | the file path and the number of lines |
| `WebFetch` | the URL |
| `WebSearch` | the query |
| `mcp__server__tool` | a "server · tool" label and compact JSON |
| anything else | compact JSON |

Text is cut off at 1500 characters.

**Security:**

- The server binds to `127.0.0.1` only.
- A random 256-bit token is generated on first run and stored in the app data directory (`claude-hook-token`, mode 0600 on Linux). It is compared in constant time.
- Requests with an `Origin` header (browsers) are rejected, as are requests whose `Host` isn't `127.0.0.1`/`localhost:47823` (protection against DNS rebinding).
- A request with a bad token returns `401` and shows a "reinstall the hook" notice in the pill.

**Remaining risk:** on a *multi-user* machine, another user could bind port 47823 before HoverPill starts and answer your prompts. The token doesn't help there, because Claude Code sends the token to whoever is listening. HoverPill does notice when the port is already taken and shows "Claude broker is off". If shared machines matter, switch to a `command` hook that talks over a per-user Unix socket or named pipe. (A process running as *you* can edit `settings.json` directly anyway, so this risk is specific to other users.)

**Ideas:** an "Always allow" button driven by `permission_suggestions`; a session label for concurrent Claude sessions.

### B. Media controller

**Status: built** (`src-tauri/crates/media/`). This is a separate crate with no Tauri dependency, so the Linux backend can be compile-checked from Windows with `cargo check -p hoverpill-media --target x86_64-unknown-linux-gnu`.

- **One worker thread.** OS change events, plus a 1.5 s poll (both APIs occasionally drop events), trigger a re-read. `pill:media` is emitted only when the visible state changes. The UI calls `current_media` once on load.
- **Which player is shown:** one that is playing; otherwise the one already shown, if it's paused; otherwise Windows' current session or any paused MPRIS player. Stopped players are hidden. Controls act on the player the pill is showing.
- **Artwork** is fetched once per track:
  - Windows: the GSMTC thumbnail stream, as a `data:` URL.
  - Linux: an `https:` `mpris:artUrl` is passed through; a `file://` cover is read and inlined.
  - Anything over 4 MB is skipped.

| | Windows | Linux |
|---|---|---|
| API | **GSMTC**: `Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager` (`windows` crate, feature `Media_Control`) | **MPRIS** over D-Bus (`zbus`): every `org.mpris.MediaPlayer2.*` bus name |
| Now playing | `GetCurrentSession` → `TryGetMediaPropertiesAsync` (title, artist, thumbnail stream → data URL) | `Metadata` property: `xesam:title`, `xesam:artist`, `mpris:artUrl` |
| Live updates | `CurrentSessionChanged`, `MediaPropertiesChanged`, `PlaybackInfoChanged` | `PropertiesChanged` on `/org/mpris/MediaPlayer2`; `NameOwnerChanged` for players starting and quitting |
| Control | `TryTogglePlayPauseAsync`, `TrySkipNextAsync`, `TrySkipPreviousAsync` | `PlayPause`, `Next`, `Previous` on `org.mpris.MediaPlayer2.Player` |

Both cover Spotify, browsers (YouTube and others), VLC, and most players. On Linux, prefer the player whose `PlaybackStatus` is `Playing`.

**Real visualizer (optional).** Capture loopback audio: on Windows, WASAPI loopback (`cpal` supports it); on Linux, the PipeWire/PulseAudio monitor source. Run a small FFT on 4 to 8 bands and emit levels at 30 fps. Only capture while the pill is visible and media is playing. The current bars are CSS-only.

### C. Notification stream

**Status: built** (`src-tauri/crates/notify/`). As with media, it's a separate crate so the Linux backend compile-checks from Windows.

**Windows.** Uses `UserNotificationListener` (`Windows.UI.Notifications.Management`). Measured on Windows 11 26200, what actually happens for an **unpackaged** app:

- **Access works without package identity.** `GetAccessStatus` returns `Allowed`, and reading notifications works. No MSIX or sparse package is needed. The user can revoke access under Settings › Privacy & security › Notifications; HoverPill then shows a "Notifications unavailable" message.
- **`NotificationChanged` fails** with `0x80070490` ("Element not found"), so there are no change events.
- **`GetNotificationsAsync` takes about 4.5 s** per call (with about 50 items in Notification Center), so diffing the full list each time would make every notification late.
- **Notification ids come from a system-wide counter that only goes up**, and `GetNotification(id)` is a synchronous lookup taking roughly 80–170 ms.

So the backend works like this:

- **Fast path:** every 600 ms it looks at the next 4 ids past the newest one it knows. Measured delay was 0.3–1 s, including a two-toast burst.
- **Safety net:** the full list is re-read every 15 s. It catches anything the fast path missed, such as a gap in ids wider than the look-ahead, and re-anchors the counter.
- **No replay:** the first full read only sets the starting point, so existing notifications aren't shown again.
- **Fragility:** the counter behaviour isn't documented. If it ever changes, the 15 s safety net still delivers everything, only later. Package identity (a sparse package) would make the event-based approach possible.

Text comes from the `ToastGeneric` binding: the first line is the title and the rest is the body. The app name and logo come from `AppInfo.DisplayInfo`.

Windows does not let a third-party app suppress other apps' banners. The clean setup is to turn off **"Show notification banners"** for each app in Settings. Its toasts still go to Notification Center, where the listener reads them, so only HoverPill shows them. Gmail through Chrome or Edge appears as the browser app, with the site in the attribution text.

**Linux.** Notifications arrive as `org.freedesktop.Notifications.Notify` calls on the session bus. There are two modes:

- *Observe (built).* `BecomeMonitor` with a match rule for `Notify` method calls (zbus `MonitoringProxy`). It sees every notification, and the desktop's own daemon keeps working. Hide the daemon's banners with the desktop's Do Not Disturb setting; apps still call `Notify` while it's on.
- *Replace (not built).* Own the `org.freedesktop.Notifications` name and *be* the notification daemon. This only works where no daemon is already running, such as on Hyprland or Sway where you would otherwise run dunst or mako. GNOME Shell and Plasma own that name permanently.

Icons, in order of preference:

1. The `image-data` hint: raw pixels, re-encoded as PNG.
2. `app_icon` as a file path.
3. `app_icon` as an icon name, looked up in the common hicolor sizes and pixmaps.
4. The app's initial.

Body markup (`<b>`, `<a>`, entities) is stripped.

The Linux backend compiles, but I haven't run it yet.

**App side:**

- **Startup outbox.** Events emitted before the webview has registered its listeners would be lost. This affects notifications and access errors, and also Claude permission requests arriving during startup. They wait in an outbox until the UI calls `pill_ready`.
- **Queue limit.** The UI keeps only the newest 4 pending notifications, so a busy group chat doesn't take minutes to play through.

## 4. Suggested build order

1. ~~Claude broker~~ (done).
2. ~~Media: GSMTC first, then MPRIS~~ (done).
3. ~~Notifications~~ (done; no packaging needed after all).
4. Polish: ~~focus~~ (done), ~~autostart~~ (done), ~~settings~~ (done), and a layer-shell backend.

**Settings** (`src-tauri/src/settings.rs`, UI in `src/settings/`):

- **Rust owns the data.** It's stored in `settings.json` in the app config dir. Fields left out of the file fall back to defaults; an invalid file is ignored in favour of defaults; values are clamped and de-duplicated on every save. The Settings window edits through `get_settings`/`set_settings`, and every window hears `settings:changed`.
- **Display.** `notch.rs` places the window on the chosen monitor (by OS name, falling back to the primary) and sizes it for the scale and top gap. The pill applies those in CSS: a `scale()` wrapper around the spring-animated pill, plus padding. It re-reports its hit rect on window resize, because a scale or monitor change moves the pill without resizing it.
- **Notifications.** Filtering happens in Rust before an event reaches the pill; HoverPill's own messages are never filtered. The per-app list in Settings has two sources:
  - *Recently sent notifications* (`RecentApps`, saved to `seen-apps.json`, newest 40). This also counts the apps behind notifications already waiting in Notification Center at startup (`Event::Seen`). The first version only counted live notifications, so WhatsApp was missing although it had one waiting.
  - *Other apps that can send notifications* (Windows): `hoverpill_notify::notifying_apps()` reads the registry list of notification senders (`HKCU\...\Notifications\Settings`, the same list as Windows Settings). It skips senders turned off there, tray placeholders and system toasts, then names each AppUserModelID through the Start menu's `shell:AppsFolder` (desktop apps like Discord and Brave) and falls back to `AppInfo::GetFromAppUserModelId` (Store apps). On this machine that turned 70 registry entries into 31 recognisable apps.
  - *Websites* such as Gmail notify as the browser. The toast's site line (`placement="attribution"`) isn't exposed by `UserNotificationListener`: a probe toast came back with only its title and body. So sites can't be told apart unless they're installed as web apps.
- **Media.** The media crate tags every session with a `Source` (Windows AppUserModelID, or the MPRIS bus-name suffix with any `.instance…` part dropped) plus a readable label. Ignored sources are skipped when choosing what to show, so the pill moves on to the next player. When media is off, the pill gets `null`.
- **Saving from the UI.** The editable hook saves 150 ms after a slider stops moving. While a save is queued or in flight, it ignores `settings:changed` echoes so a dragged slider doesn't snap back.
- **Opening the window.** The Settings window is built off the event-loop thread (building a WebView2 window inside a menu or IPC handler can deadlock). A second launch of HoverPill opens Settings. The second copy calls `AllowSetForegroundWindow(ASFW_ANY)` before handing over so the window can come to the front. That last part is unverified: test launches came from a background process, which has no foreground rights to pass on.

**Autostart** (`src-tauri/src/autostart.rs`):

- **Plugin:** `tauri-plugin-autostart` 2.x. It writes the HKCU `Run` value on Windows and an XDG autostart `.desktop` entry on Linux, under the name "HoverPill".
- **Off by default.** It's toggled from a check item in the tray menu.
- **Debug builds** refuse to *enable* it, because they depend on the Vite dev server. Disabling works in any build.
- **Release builds** re-register on every launch while it's enabled, so the entry follows the executable if it moves or is reinstalled.
- **Single instance:** `tauri-plugin-single-instance` is registered first. A second launch exits immediately, and the running copy shows "already running". This stops a manual launch after a login launch from opening a second pill and failing to bind the broker port.
- **Not exposed to the webview:** the plugin's own JavaScript commands aren't granted in `capabilities/default.json`, so only the tray can change the setting.
