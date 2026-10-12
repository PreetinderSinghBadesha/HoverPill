import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Decision, MediaAction, MediaState, NotificationItem, PermissionRequest, Rect } from "./types";

/** False when the UI is opened in a plain browser (`npm run dev`) for styling work. */
export const inTauri = "__TAURI_INTERNALS__" in window;

export type PillEvents = {
  "pill:hover": boolean;
  "pill:permission": PermissionRequest;
  /** Claude stopped waiting (interrupted or timed out); drop the card. */
  "pill:permission-cancel": { id: string };
  "pill:notification": NotificationItem;
  "pill:media": MediaState | null;
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

export function setHitRect(rect: Rect) {
  if (inTauri) invoke("set_hit_rect", { rect }).catch(console.error);
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
