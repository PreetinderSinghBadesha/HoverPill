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
} as const;

export type IconName = keyof typeof PATHS;

/** Transport icons read better filled; everything else is a stroke. */
const FILLED = new Set<IconName>(["prev", "next", "play", "pause"]);

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
