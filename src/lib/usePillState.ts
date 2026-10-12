import { useEffect, useReducer, useRef } from "react";
import { currentMedia, inTauri, on, pillReady } from "./bridge";
import type { MediaState, NotificationItem, PermissionRequest, View } from "./types";

const NOTIFICATION_MS = 4500;
const HOVER_LEAVE_MS = 180;
const MAX_QUEUED_NOTIFICATIONS = 4;

type State = {
  hovered: boolean;
  media: MediaState | null;
  notifications: NotificationItem[];
  permissions: PermissionRequest[];
};

type Action =
  | { type: "hover"; value: boolean }
  | { type: "media"; value: MediaState | null }
  | { type: "notify"; value: NotificationItem }
  | { type: "dismissNotification"; id: string }
  | { type: "permission"; value: PermissionRequest }
  | { type: "resolvePermission"; id: string };

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case "hover":
      return { ...state, hovered: action.value };
    case "media":
      return { ...state, media: action.value };
    case "notify":
      // A burst (a busy group chat) shouldn't take minutes to play through: keep the newest few.
      return { ...state, notifications: [...state.notifications, action.value].slice(-MAX_QUEUED_NOTIFICATIONS) };
    case "dismissNotification":
      return { ...state, notifications: state.notifications.filter((n) => n.id !== action.id) };
    case "permission":
      return { ...state, permissions: [...state.permissions, action.value] };
    case "resolvePermission":
      return { ...state, permissions: state.permissions.filter((p) => p.id !== action.id) };
  }
}

const initial: State = { hovered: false, media: null, notifications: [], permissions: [] };

/** Priority: a pending Claude request beats a notification, which beats hover, which beats media. */
export function pickView(s: State): View {
  if (s.permissions.length) return "permission";
  if (s.notifications.length) return "notification";
  if (s.hovered) return "expanded";
  if (s.media) return "media";
  return "idle";
}

export function usePillState() {
  const [state, dispatch] = useReducer(reducer, initial);
  const leaveTimer = useRef<number | undefined>(undefined);

  // Hover arrives from Rust: while the window is click-through the DOM sees no mouse events at all.
  // Leaving is debounced so the spring doesn't stutter when the cursor grazes the edge.
  const setHover = (value: boolean) => {
    window.clearTimeout(leaveTimer.current);
    if (value) dispatch({ type: "hover", value });
    else leaveTimer.current = window.setTimeout(() => dispatch({ type: "hover", value }), HOVER_LEAVE_MS);
  };

  useEffect(() => {
    const subs = [
      on("pill:hover", setHover),
      on("pill:media", (value) => dispatch({ type: "media", value })),
      on("pill:notification", (value) => dispatch({ type: "notify", value })),
      on("pill:permission", (value) => dispatch({ type: "permission", value })),
      on("pill:permission-cancel", ({ id }) => dispatch({ type: "resolvePermission", id })),
    ];
    Promise.all(subs.map((s) => s.registered)).then(pillReady);
    // Anything that started playing before the UI loaded was emitted before we listened.
    currentMedia()
      .then((value) => value && dispatch({ type: "media", value }))
      .catch(console.error);
    return () => subs.forEach((s) => s.off());
  }, []);

  // Auto-collapse the head notification; hovering holds it open.
  const head = state.notifications[0];
  useEffect(() => {
    if (!head || state.hovered) return;
    const t = window.setTimeout(() => dispatch({ type: "dismissNotification", id: head.id }), NOTIFICATION_MS);
    return () => window.clearTimeout(t);
  }, [head?.id, state.hovered]);

  // Browser-only demo keys, so the UI can be iterated on with `npm run dev`.
  useEffect(() => {
    if (inTauri) return;
    const onKey = (e: KeyboardEvent) => {
      const id = crypto.randomUUID();
      if (e.key === "1")
        dispatch({ type: "permission", value: { id, tool: "Bash", command: "npm install --save-dev vitest", cwd: "D:/projects/HoverPill" } });
      if (e.key === "2")
        dispatch({ type: "notify", value: { id, app: "Discord", title: "#general", body: "anyone up for a match tonight?" } });
      if (e.key === "3")
        dispatch({ type: "media", value: state.media ? null : { title: "Midnight City", artist: "M83", playing: true } });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [state.media]);

  return { state, view: pickView(state), dispatch, setHover };
}
