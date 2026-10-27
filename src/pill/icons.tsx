// Stroke icons drawn on a 24px grid; sized and coloured by CSS.

const PATHS = {
  home: "M4 11.5 12 5l8 6.5V19a1 1 0 0 1-1 1h-4.5v-5h-5v5H5a1 1 0 0 1-1-1z",
  music: "M9 17.5V6.5l10-2v11M9 17.5a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0Zm10-2a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0Z",
  bell: "M17.5 15.5v-4.5a5.5 5.5 0 1 0-11 0v4.5L5 17.5h14zM10 20a2 2 0 0 0 4 0",
  // Claude's spark: four strokes through the centre.
  spark: "M12 4v16M4 12h16M6.3 6.3l11.4 11.4M17.7 6.3 6.3 17.7",
  gear: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Zm7.2-3.9-.1-1 1.8-1.4-1.8-3.1-2.1.8a7 7 0 0 0-1.7-1L15 3.3h-3.6l-.3 2.1a7 7 0 0 0-1.7 1l-2.1-.8-1.8 3.1 1.8 1.4a7 7 0 0 0 0 2l-1.8 1.4 1.8 3.1 2.1-.8a7 7 0 0 0 1.7 1l.3 2.1H15l.3-2.1a7 7 0 0 0 1.7-1l2.1.8 1.8-3.1-1.8-1.4z",
  prev: "M6 6h2.4v12H6zM18.5 6.5v11L9.5 12z",
  next: "M15.6 6H18v12h-2.4zM5.5 6.5v11l9-5.5z",
  play: "M8 5.5v13l10.5-6.5z",
  pause: "M8 5.5h2.6v13H8zM13.4 5.5H16v13h-2.6z",
  chevronLeft: "M14.5 6 8.5 12l6 6",
  chevronRight: "M9.5 6l6 6-6 6",
  terminal: "M5 7l4 4-4 4M11 16h7",
  // A stopwatch: face, crown, and a hand.
  timer: "M12 21a7.5 7.5 0 1 0 0-15 7.5 7.5 0 0 0 0 15ZM12 9.5v4l2.5 1.6M10 2.75h4M18.2 6.3l1.3-1.3",
  plus: "M12 6v12M6 12h12",
  minus: "M6 12h12",
  // A speech bubble, for ChatGPT.
  chat: "M5 5h14a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1h-8.5L6 19.5V16H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1Z",
  send: "M5 12h13M12.5 6.5 18 12l-5.5 5.5",
  stop: "M7.5 7.5h9v9h-9z",
  clear: "M5 7h14M10 7V5h4v2M7 7l1 12h8l1-12",
  video: "M4 7.5h10.5v9H4zM14.5 10.5l5-3v9l-5-3",
  calendar: "M5 6h14v14H5zM5 10h14M9 3.5v4M15 3.5v4",
  copy: "M9 9h10v10H9zM15 9V5H5v10h4",
  screen: "M4 5h16v11H4zM9 20h6M12 16v4",
  close: "M7 7l10 10M17 7 7 17",
  lock: "M8 11V8a4 4 0 0 1 8 0v3M6 11h12v9H6z",
  unlock: "M8 11V8a4 4 0 0 1 7.7-1.5M6 11h12v9H6z",
  check: "M5 12.5l4.5 4.5L19 7.5",
} as const;

export type IconName = keyof typeof PATHS;

/** Transport icons read better filled; everything else is a stroke. */
const FILLED = new Set<IconName>(["prev", "next", "play", "pause", "stop"]);

export function Icon({ name, className }: { name: IconName; className?: string }) {
  const filled = FILLED.has(name);
  return (
    <svg
      viewBox="0 0 24 24"
      className={`icon${className ? ` ${className}` : ""}`}
      fill={filled ? "currentColor" : "none"}
      stroke={filled ? "none" : "currentColor"}
      aria-hidden
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
