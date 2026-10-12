import { useEffect, useReducer, useRef } from "react";
import { currentMedia, inTauri, on, pillReady } from "./bridge";
import type { Decision, MediaState, NotificationItem, PastDecision, PermissionRequest, ReceivedNotification, Tab, View } from "./types";

const HOVER_LEAVE_MS = 180;
/** A burst (a busy group chat) shouldn't take minutes to play through: keep the newest few cards. */
const MAX_QUEUED_NOTIFICATIONS = 4;
const MAX_HISTORY = 20;
const MAX_DECISIONS = 8;
const TAB_KEY = "hoverpill.notch.tab";

type State = {
  /** The notch is open (pushed to the top edge, or rested on). */
  open: boolean;
  tab: Tab;
  media: MediaState | null;
  /** Cards still waiting to drop down. */
  queue: NotificationItem[];
  /** Everything received this session, newest first, for the Notifications tab. */
  history: ReceivedNotification[];
  /** Received since the Notifications tab was last looked at. */
  unseen: number;
  permissions: PermissionRequest[];
  decisions: PastDecision[];
};

type Action =
  | { type: "open"; value: boolean }
  | { type: "tab"; value: Tab }
  | { type: "media"; value: MediaState | null }
  | { type: "notify"; value: NotificationItem }
  | { type: "dismissNotification"; id: string }
  | { type: "clearHistory" }
  | { type: "permission"; value: PermissionRequest }
  | { type: "decide"; id: string; decision: Decision }
  | { type: "cancelPermission"; id: string };

function withTab(state: State, tab: Tab): State {
  return { ...state, tab, unseen: tab === "notifications" ? 0 : state.unseen };
}

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case "open":
      if (!action.value) return { ...state, open: false };
      // Opening while cards are queued means "show me": jump to the list and drop the queue.
      if (state.queue.length) return { ...withTab(state, "notifications"), open: true, queue: [] };
      return { ...withTab(state, state.tab), open: true };
    case "tab":
      return withTab(state, action.value);
    case "media":
      return { ...state, media: action.value };
    case "notify": {
      const received = { ...action.value, receivedAt: Date.now() };
      const showingList = state.open && state.tab === "notifications";
      return {
        ...state,
        queue: state.open ? state.queue : [...state.queue, action.value].slice(-MAX_QUEUED_NOTIFICATIONS),
        history: [received, ...state.history].slice(0, MAX_HISTORY),
        unseen: showingList ? 0 : state.unseen + 1,
      };
    }
    case "dismissNotification":
      return { ...state, queue: state.queue.filter((n) => n.id !== action.id) };
    case "clearHistory":
      return { ...state, history: [], unseen: 0 };
    case "permission":
      return { ...state, permissions: [...state.permissions, action.value] };
    case "decide": {
      const request = state.permissions.find((p) => p.id === action.id);
      if (!request) return state;
      return {
        ...state,
        permissions: state.permissions.filter((p) => p.id !== action.id),
        decisions: [{ request, decision: action.decision, at: Date.now() }, ...state.decisions].slice(0, MAX_DECISIONS),
      };
    }
    case "cancelPermission":
      return { ...state, permissions: state.permissions.filter((p) => p.id !== action.id) };
  }
}

function savedTab(): Tab {
  try {
    const t = localStorage.getItem(TAB_KEY);
    if (t === "home" || t === "media" || t === "notifications" || t === "claude") return t;
  } catch {
    // Storage can be unavailable; Home is fine.
  }
  return "home";
}

const initial = (): State => ({
  open: false,
  tab: savedTab(),
  media: null,
  queue: [],
  history: [],
  unseen: 0,
  permissions: [],
  decisions: [],
});

/**
 * Priority: a Claude request you must answer, then the open notch, then a notification card,
 * then what's playing.
 */
export function pickView(s: State): View {
  if (s.permissions.length) return "permission";
  if (s.open) return "expanded";
  if (s.queue.length) return "notification";
  if (s.media) return "media";
  return "idle";
}

export function usePillState(notificationMs: number) {
  const [state, dispatch] = useReducer(reducer, undefined, initial);
  const leaveTimer = useRef<number | undefined>(undefined);

  // Open/close arrives from Rust: while the window is click-through the DOM sees no mouse
  // events at all. Closing is debounced so the spring doesn't stutter when the cursor grazes
  // the edge.
  const setOpen = (value: boolean) => {
    window.clearTimeout(leaveTimer.current);
    if (value) dispatch({ type: "open", value });
    else leaveTimer.current = window.setTimeout(() => dispatch({ type: "open", value }), HOVER_LEAVE_MS);
  };

  useEffect(() => {
    const subs = [
      on("pill:hover", setOpen),
      on("pill:media", (value) => dispatch({ type: "media", value })),
      on("pill:notification", (value) => dispatch({ type: "notify", value })),
      on("pill:permission", (value) => dispatch({ type: "permission", value })),
      on("pill:permission-cancel", ({ id }) => dispatch({ type: "cancelPermission", id })),
    ];
    Promise.all(subs.map((s) => s.registered)).then(pillReady);
    // Anything that started playing before the UI loaded was emitted before we listened.
    currentMedia()
      .then((value) => value && dispatch({ type: "media", value }))
      .catch(console.error);
    return () => subs.forEach((s) => s.off());
  }, []);

  useEffect(() => {
    try {
      localStorage.setItem(TAB_KEY, state.tab);
    } catch {
      // Not remembering the tab is harmless.
    }
  }, [state.tab]);

  // Each card drops down for `notificationMs`, then the next one (or the resting state) follows.
  const head = state.queue[0];
  useEffect(() => {
    if (!head) return;
    const t = window.setTimeout(() => dispatch({ type: "dismissNotification", id: head.id }), notificationMs);
    return () => window.clearTimeout(t);
  }, [head?.id, notificationMs]);

  // Browser-only demo keys, so the UI can be iterated on with `npm run dev`.
  useEffect(() => {
    if (inTauri) return;
    const onKey = (e: KeyboardEvent) => {
      const id = crypto.randomUUID();
      if (e.key === "1")
        dispatch({ type: "permission", value: { id, tool: "Bash", command: "npm install --save-dev vitest", cwd: "D:/projects/HoverPill" } });
      if (e.key === "2") dispatch({ type: "notify", value: { id, app: "Discord", title: "#general", body: "anyone up for a match tonight?" } });
      if (e.key === "3")
        dispatch({ type: "media", value: state.media ? null : { title: "Midnight City", artist: "M83", playing: true, app: "Spotify" } });
      if (e.key === "o") dispatch({ type: "open", value: !state.open });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [state.media, state.open]);

  return { state, view: pickView(state), dispatch, setOpen };
}
