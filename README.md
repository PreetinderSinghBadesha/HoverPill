<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/hoverpill-wordmark-dark.png">
  <img src="assets/brand/hoverpill-wordmark-light.png" alt="HoverPill" width="390">
</picture>

HoverPill is a smart notch for Windows and Linux, styled after the iPhone's Dynamic Island. A black pill floats at the top-center of your primary monitor. It springs open when you hover over it, when Claude Code needs permission, when a notification arrives, or when media is playing. Everywhere else, clicks pass straight through to the windows underneath.

Built with Tauri 2 (Rust), React, and Motion. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the design and the roadmap.

## Install

Download the latest installer from [GitHub Releases](https://github.com/PreetinderSinghBadesha/HoverPill/releases/latest) and run it. Each release lists what's new, and earlier versions are on the [releases page](https://github.com/PreetinderSinghBadesha/HoverPill/releases).

The installer isn't code-signed yet, so Windows SmartScreen may stop it: choose **More info**, then **Run anyway**.

### Build the installer yourself

```bash
npm run tauri build
```

This produces `src-tauri/target/release/bundle/nsis/HoverPill_<version>_x64-setup.exe`, a 3 MB per-user installer. It needs no admin rights, adds HoverPill to the Start menu, and puts an uninstaller under Windows Settings › Apps. The release build uses about 40 MB of memory.

While the installed HoverPill is running, `npm run tauri dev` can't start a second copy. Only one HoverPill runs at a time, so the dev build just opens the installed copy's Settings and exits. Quit the installed one from its tray icon first.

## Run from source

Prerequisites:

- Rust (stable) and Node 20+.
- **Windows:** WebView2, which ships with Windows 11.
- **Linux:** `libwebkit2gtk-4.1-dev`, `libayatana-appindicator3-dev`, and a compositing window manager.

```bash
npm install
npm run tauri dev
```

**Using the notch.** It sits flat against the top edge of the screen, like a laptop's camera notch.

- **When it's closed, clicks go straight through it** to whatever is underneath, so browser tabs and title bars stay usable.
- **To open it,** push the pointer against the top edge of the screen above the notch.
- **Staying out of the way:**
  - Pointing at the closed notch fades it out so you can see what's underneath.
  - With nothing happening, it shrinks to a thin bar.
  - It hides while a fullscreen app (video, game, presentation, F11 browser) covers its screen.
  - "Keep windows below the notch" in Settings reserves a strip at the top, like a taskbar, so maximised windows start below it. Windows only.
  - If you prefer opening the notch by resting the pointer on it for about half a second, turn that on in Settings. It replaces the fade.
- **Inside the open notch,** tabs switch between:
  - **Home:** what's playing, the latest notification, and a calendar
  - **Media:** large cover art and controls
  - **Timer:** a countdown (presets from 1 minute to 1 hour, ±1 minute) and a stopwatch with laps. While either runs, the closed notch shows the time. When a timer ends, a "Time's up" card drops down with a chime and +1 min / Done buttons, even over fullscreen apps.
  - **Notifications:** everything received this session
  - **Claude:** your recent permission decisions

  The gear button opens Settings.

The pill has no taskbar entry. Use the **tray icon** to open **Settings…**, to quit, to turn on **Start at login**, or to fire demo events (a Claude permission request or a notification). Launching HoverPill again while it's running also opens Settings.

**Settings** has four tabs, and changes apply immediately:

| Tab | What's there |
|---|---|
| General | Start at login, version |
| Display | Which monitor, distance from the top edge (0 = attached notch, more = floating pill), size (80–140%), open on rest, shrink when idle, hide during fullscreen, keep windows below the notch |
| Notifications | On/off, how long each card stays (2–15 s), per-app on/off: apps that recently sent notifications, plus every other app Windows lets notify |
| Media | On/off, per-player on/off (e.g. hide a browser so the pill shows Spotify) |

Settings are stored in `settings.json` in the app's config folder (`%APPDATA%\dev.hoverpill.app` on Windows, `~/.config/dev.hoverpill.app` on Linux).

**Start at login:**

- How it's stored: a per-user Run entry in the registry on Windows, an entry in `~/.config/autostart` on Linux.
- Release builds only. Run `npm run tauri build` and enable it from the installed or built app. Debug builds refuse to turn it on, because they need the dev server and would show a blank pill at login.
- Only one copy of HoverPill can run at a time. Launching it again shows "already running" in the pill.

Media works without any setup. Whatever is playing (Spotify, a browser tab, VLC, ...) appears in the pill, and hovering shows the controls. If a player doesn't show up, see what the backend detects:

```bash
cargo run --manifest-path src-tauri/Cargo.toml -p hoverpill-media --example now_playing 30
```

Notifications from other apps (Discord, WhatsApp, Gmail in your browser, ...) drop the pill down for a few seconds. Windows may ask once for notification access.

- **Windows:** to stop the normal Windows banners from appearing as well, open Settings › System › Notifications, pick each app, and turn off **Show notification banners**. Keep the app's notifications themselves on: HoverPill reads them from Notification Center.
- **Linux:** turn on your desktop's Do Not Disturb.

**Gmail and other websites** notify through your browser, so Windows reports them as the browser (e.g. Brave), and they're listed under it. The notification API doesn't expose which site sent them. To give Gmail its own entry, install it as an app from the browser's menu.

To see what the listener picks up (and which apps Windows lets notify):

```bash
cargo run --manifest-path src-tauri/Cargo.toml -p hoverpill-notify --example listen 60
```

### Connect Claude Code

1. With HoverPill running, open the tray menu and choose **Install Claude Code hook**.
   This adds an `http` `PermissionRequest` hook to `~/.claude/settings.json`. The rest of the file is left alone, and the previous version is saved as `settings.json.hoverpill-backup`.
2. Start a new Claude Code session. When Claude needs permission, the pill drops down with the command. Click **Approve** or **Deny**, or **Answer in terminal** to get Claude's normal prompt.

If HoverPill isn't running, Claude Code shows its usual terminal prompt. To undo the setup, choose **Remove Claude Code hook** from the tray menu.

To work on the UI in a normal browser, run `npm run dev` and open http://localhost:1420. Hover the pill, or press `1` (permission), `2` (notification), or `3` (media).

## Layout

```
src/                     React UI
  pill/Pill.tsx          spring-animated notch, view switching, hit-rect reporting
  pill/views.tsx         closed states, notification/permission cards, the tabbed open notch
  pill/icons.tsx         stroke icon set
  lib/useArtworkColor.ts accent colour sampled from the album art
  lib/usePillState.ts    event-driven state and view priority
  lib/bridge.ts          Tauri events and commands (no-ops in a plain browser)
src-tauri/src/
  notch.rs               placement, always-on-top, cursor-polled click-through
  broker/mod.rs          Claude Code permission broker (local HTTP server, pending decisions)
  broker/summary.rs      tool_input → readable card text
  broker/install.rs      add/remove the hook in ~/.claude/settings.json
  crates/media/          now playing + transport controls (GSMTC on Windows, MPRIS on Linux)
  crates/notify/         other apps' notifications (UserNotificationListener / D-Bus monitor)
  events.rs              shared payloads and commands
  tray.rs                tray menu: start at login, hook install, demo events, quit
  autostart.rs           start-at-login toggle (tauri-plugin-autostart)
  settings.rs            settings model, storage, Settings window, lists for the UI
src/settings/            the tabbed Settings window (settings.html)
```

## Status

| | |
|---|---|
| ✅ Done | Floating window, selective click-through, spring animations, all views, Claude permission broker, media controller, notification stream, never steals focus, start at login, single instance, tabbed Settings window |
| 🧪 Untested | Everything on Linux (it compiles but has never run) |
| 🔜 Next | Wayland layer-shell (see the architecture doc) |
