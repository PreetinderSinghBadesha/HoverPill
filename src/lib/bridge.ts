import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  DEFAULT_SETTINGS,
  type Decision,
  type MediaAction,
  type MediaState,
  type MonitorInfo,
  type NotificationItem,
  type PermissionRequest,
  type PlayerInfo,
  type Rect,
  type Settings,
} from "./types";

/** False when the UI is opened in a plain browser (`npm run dev`) for styling work. */
export const inTauri = "__TAURI_INTERNALS__" in window;

export type PillEvents = {
  /** Open (true) or close the notch. */
  "pill:hover": boolean;
  /** The pointer is over the closed notch: fade it so what's underneath shows. */
  "pill:peek": boolean;
  /** A fullscreen app covers the notch's monitor. */
  "pill:fullscreen": boolean;
  "pill:permission": PermissionRequest;
  /** Claude stopped waiting (interrupted or timed out); drop the card. */
  "pill:permission-cancel": { id: string };
  "pill:notification": NotificationItem;
  "pill:media": MediaState | null;
  /** Sent to every window after a save, with the values Rust actually stored. */
  "settings:changed": Settings;
};

export type Subscription = { off: () => void; registered: Promise<void> };

export function on<K extends keyof PillEvents>(event: K, cb: (payload: PillEvents[K]) => void): Subscription {
  if (!inTauri) return { off: () => {}, registered: Promise.resolve() };
  let disposed = false;
  let unlisten: (() => void) | undefined;
  const registered = listen<PillEvents[K]>(event, (e) => cb(e.payload)).then((fn) => {
    if (disposed) fn();
    else unlisten = fn;
  });
  return {
    off: () => {
      disposed = true;
      unlisten?.();
    },
    registered,
  };
}

/** Releases events Rust queued while the UI was loading. Call once every listener is registered. */
export function pillReady() {
  if (inTauri) invoke("pill_ready").catch(console.error);
}

/** `interactive`: the notch should take clicks while the pointer is over it (open, or a card with buttons). */
export function setHitRect(rect: Rect, interactive: boolean) {
  if (inTauri) invoke("set_hit_rect", { rect, interactive }).catch(console.error);
}

export function openSettings() {
  if (inTauri) invoke("open_settings").catch(console.error);
}

export function resolvePermission(id: string, decision: Decision) {
  if (inTauri) invoke("resolve_permission", { id, decision }).catch(console.error);
}

export function mediaCommand(action: MediaAction) {
  if (inTauri) invoke("media_command", { action }).catch(console.error);
}

/** What's playing right now; later changes arrive as `pill:media`. */
export async function currentMedia(): Promise<MediaState | null> {
  return inTauri ? invoke<MediaState | null>("current_media") : null;
}

// ---------- settings ----------

export async function getSettings(): Promise<Settings> {
  return inTauri ? invoke<Settings>("get_settings") : structuredClone(DEFAULT_SETTINGS);
}

/** Resolves to what was stored after Rust clamped it. */
export async function saveSettings(settings: Settings): Promise<Settings> {
  return inTauri ? invoke<Settings>("set_settings", { settings }) : settings;
}

export async function listMonitors(): Promise<MonitorInfo[]> {
  return inTauri ? invoke<MonitorInfo[]>("list_monitors") : [{ name: "DISPLAY1", label: "Display 1 — 1920×1080 (primary)", primary: true }];
}

export async function recentNotificationApps(): Promise<string[]> {
  return inTauri ? invoke<string[]>("recent_notification_apps") : ["Discord", "WhatsApp", "Gmail"];
}

/** Every app Windows lets send notifications, whether or not it has sent one yet (empty on Linux). */
export async function notifyingApps(): Promise<string[]> {
  return inTauri ? invoke<string[]>("notifying_apps") : ["Brave", "Discord", "Google Chrome", "Slack", "WhatsApp"];
}

export async function mediaPlayers(): Promise<PlayerInfo[]> {
  return inTauri ? invoke<PlayerInfo[]>("media_players") : [{ id: "Spotify.exe", label: "Spotify" }, { id: "chrome", label: "Chrome" }];
}

export async function getAutostart(): Promise<boolean> {
  return inTauri ? invoke<boolean>("get_autostart") : false;
}

/** Resolves to the resulting state; rejects with a readable message (e.g. in debug builds). */
export async function setAutostart(enabled: boolean): Promise<boolean> {
  return inTauri ? invoke<boolean>("set_autostart", { enabled }) : enabled;
}
