// Shapes shared with the Rust side (src-tauri/src/events.rs). Keep in sync.

export type Rect = { x: number; y: number; width: number; height: number };

export type PermissionRule = {
  toolName: string;
  ruleContent?: string;
};

export type PermissionSuggestion = {
  type: string;
  rules?: PermissionRule[];
  behavior?: "allow" | "deny" | "ask";
  destination?: string;
  mode?: string;
  directories?: string[];
};

export type PermissionRequest = {
  id: string;
  tool: string;
  /** Human-readable summary of what Claude wants to do (shell command, file path, ...). */
  command: string;
  cwd?: string;
  permission_suggestions?: PermissionSuggestion[];
};

/** "ask" hands the question back to Claude Code's own terminal prompt. */
export type Decision = "allow" | "deny" | "ask" | "always_allow";

export type NotificationItem = {
  id: string;
  app: string;
  title: string;
  body: string;
  /** data: URL or http(s) URL; falls back to the app's initial. */
  icon?: string;
};

export type MediaState = {
  title: string;
  artist: string;
  playing: boolean;
  artwork?: string;
  /** Readable player name, e.g. "Spotify". */
  app?: string;
};

export type MediaAction = "play_pause" | "next" | "previous";

/** `timer`: closed, nothing playing, timer or stopwatch running. `alarm`: the timer just ended. */
export type View = "idle" | "media" | "timer" | "expanded" | "notification" | "permission" | "alarm";

/** Tabs inside the open notch. */
export type Tab = "home" | "media" | "timer" | "notifications" | "claude" | "chatgpt";

/** An upcoming Google Calendar event (src-tauri/src/accounts/google.rs). */
export type CalendarEvent = {
  id: string;
  title: string;
  /** RFC 3339 date-time, or a plain date for all-day events. */
  start: string;
  end: string;
  allDay: boolean;
  location?: string;
  /** A video call link if there is one, else the event's page in Google Calendar. */
  link?: string;
  calendar: string;
  color?: string;
};

export type AccountStatus = {
  connected: boolean;
  email?: string;
  /** Waiting for the user in the browser. */
  busy: boolean;
  error?: string;
  /** ChatGPT only: requests can use the user's ChatGPT plan. */
  planUsage: boolean;
};

export type AccountsStatus = { google: AccountStatus; chatgpt: AccountStatus };

/** A streamed answer from ChatGPT, matched to its question by `id`. */
export type ChatChunk =
  | { kind: "delta"; id: string; text: string }
  | { kind: "done"; id: string }
  | { kind: "error"; id: string; message: string; usage: boolean };

export type ChatTurn = { role: "user" | "assistant"; text: string };

export type ModelInfo = { id: string; label: string };

export type ReceivedNotification = NotificationItem & { receivedAt: number };

export type PastDecision = { request: PermissionRequest; decision: Decision; at: number };

// Mirrors src-tauri/src/settings.rs. Rust clamps everything on save; these ranges are for the UI.
export type Settings = {
  display: {
    /** Monitor name from `list_monitors`; null follows the primary monitor. */
    monitor: string | null;
    topOffset: number;
    scale: number;
    /** Also open when the pointer rests on the notch (not just when pushed to the top edge). */
    openOnHover: boolean;
    /** With nothing playing and no card showing, shrink to a thin bar. */
    compactIdle: boolean;
    /** Hide while a fullscreen app covers the notch's monitor. */
    hideInFullscreen: boolean;
    /** Reserve a strip at the top so maximised windows start below the notch (Windows only). */
    reserveSpace: boolean;
    /** Show the pill on every virtual desktop / workspace, not just the one it was opened on. */
    allWorkspaces: boolean;
  };
  notifications: {
    enabled: boolean;
    durationSecs: number;
    blockedApps: string[];
  };
  media: {
    enabled: boolean;
    ignoredPlayers: string[];
  };
  calendar: {
    /** Show events on Home and drop reminders while Google Calendar is connected. */
    enabled: boolean;
    /** Minutes before an event to drop a reminder; 0 turns reminders off. */
    reminderMinutes: number;
    /** The user's own Google OAuth client ("Desktop app"). */
    clientId: string;
    clientSecret: string;
  };
  chatgpt: {
    /** Model slug; null uses the first one the account offers. */
    model: string | null;
  };
};

/** Mirrors the platform-aware default in src-tauri/src/settings.rs (`Display::default`). */
const IS_LINUX = typeof navigator !== "undefined" && navigator.userAgent.includes("Linux");

export const DEFAULT_SETTINGS: Settings = {
  display: {
    monitor: null,
    topOffset: 0,
    scale: 1,
    openOnHover: false,
    compactIdle: true,
    hideInFullscreen: true,
    reserveSpace: false,
    allWorkspaces: IS_LINUX,
  },
  notifications: { enabled: true, durationSecs: 4.5, blockedApps: [] },
  media: { enabled: true, ignoredPlayers: [] },
  calendar: { enabled: true, reminderMinutes: 5, clientId: "", clientSecret: "" },
  chatgpt: { model: null },
};

export const RANGES = {
  topOffset: { min: 0, max: 48, step: 1 },
  scale: { min: 0.8, max: 1.4, step: 0.05 },
  durationSecs: { min: 2, max: 15, step: 0.5 },
  reminderMinutes: { min: 0, max: 60, step: 1 },
} as const;

export type MonitorInfo = { name: string; label: string; primary: boolean };
export type PlayerInfo = { id: string; label: string };
