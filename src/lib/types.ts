// Shapes shared with the Rust side (src-tauri/src/events.rs). Keep in sync.

export type Rect = { x: number; y: number; width: number; height: number };

export type PermissionRequest = {
  id: string;
  tool: string;
  /** Human-readable summary of what Claude wants to do (shell command, file path, ...). */
  command: string;
  cwd?: string;
};

/** "ask" hands the question back to Claude Code's own terminal prompt. */
export type Decision = "allow" | "deny" | "ask";

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
export type Tab = "home" | "media" | "timer" | "notifications" | "claude";

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
};

export const DEFAULT_SETTINGS: Settings = {
  display: { monitor: null, topOffset: 0, scale: 1, openOnHover: false, compactIdle: true, hideInFullscreen: true, reserveSpace: false },
  notifications: { enabled: true, durationSecs: 4.5, blockedApps: [] },
  media: { enabled: true, ignoredPlayers: [] },
};

export const RANGES = {
  topOffset: { min: 0, max: 48, step: 1 },
  scale: { min: 0.8, max: 1.4, step: 0.05 },
  durationSecs: { min: 2, max: 15, step: 0.5 },
} as const;

export type MonitorInfo = { name: string; label: string; primary: boolean };
export type PlayerInfo = { id: string; label: string };
