import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  DEFAULT_SETTINGS,
  type AccountsStatus,
  type CalendarEvent,
  type ChatChunk,
  type ChatTurn,
  type Decision,
  type ModelInfo,
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
  /** Upcoming Google Calendar events (empty when disconnected or turned off). */
  "pill:calendar": CalendarEvent[];
  /** A piece of a streamed ChatGPT answer, or its end. */
  "pill:chatgpt": ChatChunk;
  /** Google or ChatGPT connected, disconnected, or hit a problem. */
  "accounts:changed": AccountsStatus;
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

/** While true the notch keeps the keyboard (and stays open) so a question can be typed. */
export function notchTyping(active: boolean) {
  if (inTauri) invoke("notch_typing", { active }).catch(console.error);
}

// ---------- calendar ----------

export async function calendarEvents(): Promise<CalendarEvent[]> {
  return inTauri ? invoke<CalendarEvent[]>("calendar_events") : demoEvents();
}

export function refreshCalendar() {
  if (inTauri) invoke("calendar_refresh").catch(console.error);
}

/** For `npm run dev`: a few events relative to now. */
function demoEvents(): CalendarEvent[] {
  const at = (h: number) => new Date(Date.now() + h * 3600_000).toISOString();
  return [
    { id: "a", title: "Design review", start: at(0.6), end: at(1.4), allDay: false, link: "https://meet.google.com/", calendar: "Work", color: "#4E9A5E" },
    { id: "b", title: "Dentist", start: at(5), end: at(6), allDay: false, location: "Sector 17", calendar: "Personal" },
    { id: "c", title: "Team standup", start: at(24), end: at(24.25), allDay: false, calendar: "Work", color: "#4E9A5E" },
  ];
}

// ---------- accounts ----------

const NOT_CONNECTED = { connected: false, busy: false, planUsage: false };

export async function accountsStatus(): Promise<AccountsStatus> {
  return inTauri ? invoke<AccountsStatus>("accounts_status") : { google: NOT_CONNECTED, chatgpt: NOT_CONNECTED };
}

/** Opens the browser to sign in; resolves when the user is done there. Rejects with a readable message. */
export async function connectGoogle(): Promise<void> {
  if (inTauri) await invoke("google_connect");
}

export async function disconnectGoogle(): Promise<void> {
  if (inTauri) await invoke("google_disconnect");
}

/** `reconsent` asks for every permission again, e.g. to turn on plan usage after it was declined. */
export async function connectChatGpt(reconsent = false): Promise<void> {
  if (inTauri) await invoke("chatgpt_connect", { reconsent });
}

export async function disconnectChatGpt(): Promise<void> {
  if (inTauri) await invoke("chatgpt_disconnect");
}

export async function chatGptModels(): Promise<ModelInfo[]> {
  return inTauri ? invoke<ModelInfo[]>("chatgpt_models") : [{ id: "demo-model", label: "Demo model" }];
}

/** Sends the conversation; the answer streams back on `pill:chatgpt` under `id`. */
export function askChatGpt(id: string, turns: ChatTurn[]) {
  if (inTauri) invoke("chatgpt_ask", { id, turns }).catch(console.error);
}

export function stopChatGpt(id: string) {
  if (inTauri) invoke("chatgpt_stop", { id }).catch(console.error);
}

/** Opens an https link (a calendar event, a video call) in the default browser. */
export function openLink(url: string) {
  if (inTauri) invoke("open_link", { url }).catch(console.error);
  else window.open(url, "_blank", "noopener");
}

/** ChatGPT's own settings page, where plan usage is shown. */
export function openChatGptUsage() {
  if (inTauri) invoke("chatgpt_open_usage").catch(console.error);
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
