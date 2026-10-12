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
};

export type MediaAction = "play_pause" | "next" | "previous";

export type View = "idle" | "media" | "expanded" | "notification" | "permission";
