# HoverPill

HoverPill is a smart notch for Windows and Linux, styled after the iPhone's Dynamic Island. A black pill floats at the top-center of your primary monitor. It springs open when you hover over it, when Claude Code needs permission, when a notification arrives, or when media is playing. Everywhere else, clicks pass straight through to the windows underneath.

Built with Tauri 2 (Rust), React, and Motion. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the design and the roadmap.

## Run

Prerequisites:

- Rust (stable) and Node 20+.
- **Windows:** WebView2, which ships with Windows 11.
- **Linux:** `libwebkit2gtk-4.1-dev`, `libayatana-appindicator3-dev`, and a compositing window manager.

```bash
npm install
npm run tauri dev
```

The pill has no taskbar entry. Use the **tray icon** to quit, to turn on **Start at login**, or to fire demo events (a Claude permission request or a notification).

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

To see what the listener picks up:

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
  pill/Pill.tsx          spring-animated pill, view switching, hit-rect reporting
  pill/views.tsx         idle / media / expanded / notification / permission views
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
```

## Status

| | |
|---|---|
| ✅ Done | Floating window, selective click-through, spring animations, all views, Claude permission broker, media controller, notification stream, never steals focus, start at login, single instance |
| 🧪 Untested | Everything on Linux (it compiles but has never run) |
| 🔜 Next | Settings, Wayland layer-shell (see the architecture doc) |
