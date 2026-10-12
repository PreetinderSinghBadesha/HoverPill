import { motion } from "motion/react";
import { useEffect, useState } from "react";
import type { Decision, MediaAction, MediaState, NotificationItem, PermissionRequest } from "../lib/types";

export function IdleView() {
  return <div className="idle" />;
}

export function MediaCompactView({ media }: { media: MediaState }) {
  return (
    <div className="row compact">
      <Artwork media={media} size={20} />
      <div className="marquee">
        <span>
          {media.title} · {media.artist}
        </span>
      </div>
      <Visualizer playing={media.playing} />
    </div>
  );
}

export function ExpandedView({ media, onCommand }: { media: MediaState | null; onCommand: (a: MediaAction) => void }) {
  if (!media) return <Clock />;
  return (
    <div className="row expanded">
      <Artwork media={media} size={64} />
      <div className="meta">
        <div className="title">{media.title}</div>
        <div className="subtitle">{media.artist}</div>
        <div className="controls">
          <IconButton label="Previous" onClick={() => onCommand("previous")} d="M6 6h2v12H6zM9.5 12 18 6v12z" />
          <IconButton
            label={media.playing ? "Pause" : "Play"}
            big
            onClick={() => onCommand("play_pause")}
            d={media.playing ? "M7 5h3.5v14H7zM13.5 5H17v14h-3.5z" : "M8 5v14l11-7z"}
          />
          <IconButton label="Next" onClick={() => onCommand("next")} d="M16 6h2v12h-2zM6 18l8.5-6L6 6z" />
        </div>
      </div>
      <Visualizer playing={media.playing} />
    </div>
  );
}

export function NotificationView({ item, paused, onDismiss }: { item: NotificationItem; paused: boolean; onDismiss: () => void }) {
  return (
    <div className="row notification" onClick={onDismiss}>
      <AppIcon item={item} />
      <div className="meta">
        <div className="eyebrow">{item.app}</div>
        <div className="title">{item.title}</div>
        <div className="subtitle ellipsis">{item.body}</div>
      </div>
      {/* Keyed on `paused` so the bar restarts when hover releases, matching the dismiss timer. */}
      <motion.div
        key={String(paused)}
        className="timer"
        initial={{ scaleX: 1 }}
        animate={{ scaleX: paused ? 1 : 0 }}
        transition={{ duration: paused ? 0 : 4.5, ease: "linear" }}
      />
    </div>
  );
}

export function PermissionView({ request, queued, onDecide }: { request: PermissionRequest; queued: number; onDecide: (d: Decision) => void }) {
  return (
    <div className="permission">
      <div className="perm-header">
        <span className="claude-mark">✻</span>
        <span>Claude Code wants to use</span>
        <span className="chip">{request.tool}</span>
        {queued > 0 && <span className="queued">+{queued} more</span>}
      </div>
      <pre className="command">{request.command}</pre>
      <div className="perm-footer">
        <span className="cwd ellipsis">{request.cwd}</span>
        <button className="link" onClick={() => onDecide("ask")}>
          Answer in terminal
        </button>
      </div>
      <div className="actions">
        <button className="btn deny" onClick={() => onDecide("deny")}>
          Deny
        </button>
        <button className="btn approve" onClick={() => onDecide("allow")}>
          Approve
        </button>
      </div>
    </div>
  );
}

function Artwork({ media, size }: { media: MediaState; size: number }) {
  return media.artwork ? (
    <img className="artwork" src={media.artwork} width={size} height={size} alt="" />
  ) : (
    <div className="artwork placeholder" style={{ width: size, height: size }} />
  );
}

function AppIcon({ item }: { item: NotificationItem }) {
  if (item.icon) return <img className="app-icon" src={item.icon} alt="" />;
  const hue = [...item.app].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 0);
  return (
    <div className="app-icon" style={{ background: `hsl(${hue} 65% 52%)` }}>
      {item.app.charAt(0).toUpperCase()}
    </div>
  );
}

/** Decorative bars. Real levels would need loopback audio capture; see docs/ARCHITECTURE.md. */
function Visualizer({ playing }: { playing: boolean }) {
  return (
    <div className={`visualizer${playing ? " playing" : ""}`} aria-hidden>
      {[0, 1, 2, 3].map((i) => (
        <span key={i} style={{ animationDelay: `${i * -0.23}s` }} />
      ))}
    </div>
  );
}

function IconButton({ label, d, onClick, big }: { label: string; d: string; onClick: () => void; big?: boolean }) {
  return (
    <button className={`icon-btn${big ? " big" : ""}`} aria-label={label} title={label} onClick={onClick}>
      <svg viewBox="0 0 24 24" fill="currentColor">
        <path d={d} />
      </svg>
    </button>
  );
}

function Clock() {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const t = window.setInterval(() => setNow(new Date()), 1000);
    return () => window.clearInterval(t);
  }, []);
  return (
    <div className="clock">
      <div className="time">{now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</div>
      <div className="subtitle">{now.toLocaleDateString([], { weekday: "long", month: "short", day: "numeric" })} · Nothing playing</div>
    </div>
  );
}
