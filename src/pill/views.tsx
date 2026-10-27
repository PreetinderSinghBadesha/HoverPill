import { AnimatePresence, motion } from "motion/react";
import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { connectChatGpt, notchTyping, openChatGptUsage, openLink } from "../lib/bridge";
import type { Chat } from "../lib/useAccounts";
import type {
  AccountStatus,
  CalendarEvent,
  Decision,
  MediaAction,
  MediaState,
  NotificationItem,
  PastDecision,
  PermissionRequest,
  ReceivedNotification,
  Tab,
} from "../lib/types";
import { Icon, type IconName } from "./icons";
import { formatCountdown, formatElapsed, MAX_DURATION, stopwatchElapsed, timerLeft, type Timers } from "../lib/useTimers";

// ---------- closed ----------

export function IdleView() {
  return <div className="idle" />;
}

/**
 * Closed with something playing: the cover on the left, its colour bleeding into the black.
 * A running timer or stopwatch takes the visualizer's place on the right.
 */
export function MediaCompactView({ media, clock }: { media: MediaState; clock: string | null }) {
  return (
    <div className="compact">
      <Artwork media={media} className="compact-art" />
      {clock ? <span className="compact-clock">{clock}</span> : <Visualizer playing={media.playing} />}
    </div>
  );
}

/**
 * Closed as a bubble against a screen edge: a running timer or stopwatch, else the cover of
 * what's playing, else a small pill shape.
 */
export function BubbleView({ media, clock }: { media: MediaState | null; clock: string | null }) {
  return (
    <div className="bubble">
      {clock ? (
        <span className={`bubble-clock${clock.length > 5 ? " is-long" : ""}`}>{clock}</span>
      ) : media ? (
        <Artwork media={media} className="bubble-art" />
      ) : (
        <span className="bubble-mark" />
      )}
    </div>
  );
}

/** Closed, nothing playing, a timer or stopwatch running. */
export function TimerCompactView({ clock }: { clock: string }) {
  return (
    <div className="compact">
      <Icon name="timer" className="compact-timer-icon" />
      <span className="compact-clock">{clock}</span>
    </div>
  );
}

/** The timer ran out. Stays until you deal with it. */
export function AlarmView({ durationMs, onSnooze, onDone }: { durationMs: number; onSnooze: () => void; onDone: () => void }) {
  return (
    <div className="card-alarm">
      <span className="alarm-icon" aria-hidden>
        <Icon name="timer" />
      </span>
      <div className="card-text">
        <div className="card-title">Time's up</div>
        <div className="card-body">{describeDuration(durationMs)} timer</div>
      </div>
      <button type="button" className="btn btn-quiet alarm-button" onClick={onSnooze}>
        +1 min
      </button>
      <button type="button" className="btn btn-solid alarm-button" onClick={onDone}>
        Done
      </button>
    </div>
  );
}

// ---------- cards that drop down by themselves ----------

export function NotificationView({ item, durationMs }: { item: NotificationItem; durationMs: number }) {
  return (
    <div className="card-notification">
      <AppIcon app={item.app} icon={item.icon} className="card-icon" />
      <div className="card-text">
        <div className="card-app">{item.app}</div>
        <div className="card-title">{item.title}</div>
        {item.body && <div className="card-body">{item.body}</div>}
      </div>
      <motion.div
        className="card-timer"
        initial={{ scaleX: 1 }}
        animate={{ scaleX: 0 }}
        transition={{ duration: durationMs / 1000, ease: "linear" }}
      />
    </div>
  );
}

export function PermissionView({ request, queued, onDecide }: { request: PermissionRequest; queued: number; onDecide: (d: Decision) => void }) {
  // "Always allow" only makes sense when Claude offered a rule to remember.
  const canAlwaysAllow = request.permission_suggestions?.some((s) => s.behavior === "allow") ?? false;
  return (
    <div className="card-permission">
      <div className="perm-header">
        <Icon name="spark" className="perm-mark" />
        <span>Claude Code wants to use</span>
        <span className="tool-chip">{request.tool}</span>
        {queued > 0 && <span className="perm-queued">{queued} more waiting</span>}
      </div>
      <pre className="perm-command">{request.command}</pre>
      <div className="perm-footer">
        <span className="perm-cwd">{request.cwd}</span>
        <button type="button" className="text-button" onClick={() => onDecide("ask")}>
          <Icon name="terminal" />
          Answer in terminal
        </button>
      </div>
      <div className="perm-actions">
        {canAlwaysAllow && (
          <button type="button" className="btn btn-quiet" onClick={() => onDecide("always_allow")}>
            Always allow
          </button>
        )}
        <button type="button" className="btn btn-quiet" onClick={() => onDecide("deny")}>
          Deny
        </button>
        <button type="button" className="btn btn-solid" onClick={() => onDecide("allow")}>
          Approve
        </button>
      </div>
    </div>
  );
}

// ---------- open: tab bar + tabs ----------

const TABS: { id: Tab; label: string; icon: IconName }[] = [
  { id: "home", label: "Home", icon: "home" },
  { id: "media", label: "Media", icon: "music" },
  { id: "timer", label: "Timer", icon: "timer" },
  { id: "notifications", label: "Notifications", icon: "bell" },
  { id: "claude", label: "Claude", icon: "spark" },
  { id: "chatgpt", label: "ChatGPT", icon: "chat" },
];

export type ExpandedProps = {
  tab: Tab;
  onTab: (t: Tab) => void;
  media: MediaState | null;
  onMedia: (a: MediaAction) => void;
  history: ReceivedNotification[];
  unseen: number;
  onClearHistory: () => void;
  decisions: PastDecision[];
  timers: Timers;
  onSettings: () => void;
  calendar: CalendarEvent[];
  chat: Chat;
  chatgpt: AccountStatus;
};

export function ExpandedView(props: ExpandedProps) {
  const now = useNow(15_000);
  return (
    <div className="expanded">
      <div className="topbar">
        <div className="tabs" role="tablist" aria-label="Notch sections">
          {TABS.map((t) => {
            const active = t.id === props.tab;
            const badge = t.id === "notifications" && props.unseen > 0 ? props.unseen : 0;
            const ticking = t.id === "timer" && (props.timers.timer.status === "running" || props.timers.stopwatch.startedAt !== null);
            return (
              <button
                key={t.id}
                type="button"
                role="tab"
                aria-selected={active}
                aria-label={t.label}
                className={`tab${active ? " is-active" : ""}`}
                onClick={() => props.onTab(t.id)}
              >
                <Icon name={t.icon} />
                {active && <span className="tab-label">{t.label}</span>}
                {badge > 0 && <span className="tab-badge">{badge > 9 ? "9+" : badge}</span>}
                {ticking && !active && <span className="tab-dot" aria-label="running" />}
              </button>
            );
          })}
        </div>
        <div className="topbar-end">
          <span className="clock">{now.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}</span>
          <button type="button" className="round-button" aria-label="Open settings" title="Settings" onClick={props.onSettings}>
            <Icon name="gear" />
          </button>
        </div>
      </div>

      <AnimatePresence mode="wait" initial={false}>
        <motion.div
          key={props.tab}
          className="tab-panel"
          role="tabpanel"
          initial={{ opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0, transition: { duration: 0.16 } }}
          exit={{ opacity: 0, transition: { duration: 0.08 } }}
        >
          {props.tab === "home" && <HomeTab {...props} now={now} />}
          {props.tab === "media" && <MediaTab media={props.media} onMedia={props.onMedia} />}
          {props.tab === "timer" && <TimerTab timers={props.timers} />}
          {props.tab === "notifications" && <NotificationsTab history={props.history} onClear={props.onClearHistory} now={now} />}
          {props.tab === "claude" && <ClaudeTab decisions={props.decisions} now={now} />}
          {props.tab === "chatgpt" && <ChatGptTab chat={props.chat} account={props.chatgpt} onSettings={props.onSettings} />}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}

function HomeTab({ media, onMedia, history, now, onTab, calendar }: ExpandedProps & { now: Date }) {
  const latest = history[0];
  const next = nextEvent(calendar, now);
  return (
    <div className="home">
      <div className="home-main">
        {media ? (
          <div className="home-player">
            <Artwork media={media} className="home-art" />
            <div className="home-meta">
              <div className="track-title">{media.title}</div>
              <div className="track-artist">{media.artist}</div>
              <Transport media={media} onMedia={onMedia} size="small" />
            </div>
          </div>
        ) : (
          <EmptyState icon="music" title="Nothing playing">
            Play something in Spotify, a browser or any media app.
          </EmptyState>
        )}
        {next ? (
          <UpNext event={next} now={now} />
        ) : latest && (
          <button type="button" className="home-latest" onClick={() => onTab("notifications")}>
            <AppIcon app={latest.app} icon={latest.icon} className="latest-icon" />
            <span className="latest-text">
              <span className="latest-title">{latest.title}</span>
              <span className="latest-body">{latest.body || latest.app}</span>
            </span>
            <span className="latest-time">{ago(latest.receivedAt, now)}</span>
          </button>
        )}
      </div>
      <Calendar today={now} events={calendar} />
    </div>
  );
}

// ---------- Google Calendar on Home ----------

/** All-day events come as a plain date; read it as a local date, not UTC midnight. */
function eventDate(value: string, allDay: boolean) {
  if (allDay) {
    const [y, m, d] = value.split("-").map(Number);
    return new Date(y, m - 1, d);
  }
  return new Date(value);
}

/** The first event that hasn't finished yet. */
function nextEvent(events: CalendarEvent[], now: Date) {
  return events.find((e) => eventDate(e.end, e.allDay) > now);
}

function dayLabel(d: Date, now: Date) {
  const days = Math.round((new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime() - new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()) / 86_400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Tomorrow";
  return d.toLocaleDateString([], { weekday: "long" });
}

function whenText(e: CalendarEvent, now: Date) {
  const start = eventDate(e.start, e.allDay);
  if (e.allDay) return `${dayLabel(start, now)}, all day`;
  const end = eventDate(e.end, false);
  const time = (d: Date) => d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  if (start <= now) return `Now, until ${time(end)}`;
  const mins = Math.round((start.getTime() - now.getTime()) / 60_000);
  if (mins < 60) return `In ${mins} min, at ${time(start)}`;
  return `${dayLabel(start, now)}, ${time(start)}`;
}

const CALL_HOSTS = /(^|\.)(meet\.google\.com|zoom\.us|teams\.microsoft\.com|teams\.live\.com|webex\.com)$/;

function isCall(link: string) {
  try {
    return CALL_HOSTS.test(new URL(link).hostname);
  } catch {
    return false;
  }
}

function UpNext({ event, now }: { event: CalendarEvent; now: Date }) {
  const call = event.link && isCall(event.link);
  return (
    <div className="up-next">
      <span className="up-next-mark" style={event.color ? { background: event.color } : undefined} aria-hidden />
      <span className="up-next-text">
        <span className="up-next-title">{event.title}</span>
        <span className="up-next-when">
          {whenText(event, now)}
          {event.location ? `, ${event.location}` : ""}
        </span>
      </span>
      {event.link && (
        <button type="button" className={`btn ${call ? "btn-solid" : "btn-quiet"} up-next-button`} onClick={() => openLink(event.link!)}>
          {call && <Icon name="video" />}
          {call ? "Join" : "Open"}
        </button>
      )}
    </div>
  );
}

function MediaTab({ media, onMedia }: { media: MediaState | null; onMedia: (a: MediaAction) => void }) {
  if (!media)
    return (
      <EmptyState icon="music" title="Nothing playing">
        Play something in Spotify, a browser or any media app and it shows up here.
      </EmptyState>
    );
  return (
    <div className="player">
      <div className="player-art-wrap">
        <Artwork media={media} className="player-art" />
      </div>
      <div className="player-meta">
        {media.app && <div className="player-app">Playing in {media.app}</div>}
        <div className="player-title">{media.title}</div>
        <div className="player-artist">{media.artist}</div>
        <div className="player-row">
          <Transport media={media} onMedia={onMedia} size="large" />
          <Visualizer playing={media.playing} />
        </div>
      </div>
    </div>
  );
}

function NotificationsTab({ history, onClear, now }: { history: ReceivedNotification[]; onClear: () => void; now: Date }) {
  if (!history.length)
    return (
      <EmptyState icon="bell" title="No notifications yet">
        Notifications from your apps collect here as they arrive.
      </EmptyState>
    );
  return (
    <div className="list-wrap">
      <div className="list-head">
        <span>
          {history.length} {history.length === 1 ? "notification" : "notifications"}
        </span>
        <button type="button" className="text-button" onClick={onClear}>
          Clear all
        </button>
      </div>
      <ul className="list">
        {history.map((n) => (
          <li key={n.id} className="list-item">
            <AppIcon app={n.app} icon={n.icon} className="list-icon" />
            <div className="list-text">
              <div className="list-line">
                <span className="list-app">{n.app}</span>
                <span className="list-time">{ago(n.receivedAt, now)}</span>
              </div>
              <div className="list-title">{n.title}</div>
              {n.body && <div className="list-body">{n.body}</div>}
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}

const DECISION_TEXT: Record<Decision, string> = { allow: "Approved", deny: "Denied", ask: "Sent to terminal", always_allow: "Always allowed" };

function ClaudeTab({ decisions, now }: { decisions: PastDecision[]; now: Date }) {
  if (!decisions.length)
    return (
      <EmptyState icon="spark" title="No requests yet">
        When Claude Code asks for permission, the request drops down from the notch. To connect it, choose Install Claude Code hook in
        the tray menu.
      </EmptyState>
    );
  return (
    <div className="list-wrap">
      <div className="list-head">
        <span>Recent requests</span>
      </div>
      <ul className="list">
        {decisions.map((d) => (
          <li key={d.request.id + d.at} className="list-item decision">
            <span className={`decision-mark is-${d.decision}`} aria-hidden />
            <div className="list-text">
              <div className="list-line">
                <span className="tool-chip">{d.request.tool}</span>
                <span className={`decision-text is-${d.decision}`}>{DECISION_TEXT[d.decision]}</span>
                <span className="list-time">{ago(d.at, now)}</span>
              </div>
              <code className="decision-command">{d.request.command}</code>
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}

// ---------- ChatGPT tab ----------

function ChatGptTab({ chat, account, onSettings }: { chat: Chat; account: AccountStatus; onSettings: () => void }) {
  if (!account.connected)
    return (
      <EmptyState icon="chat" title="Ask ChatGPT from the notch">
        Sign in with your ChatGPT account and questions use your Plus or Pro plan, no API key needed.{" "}
        <button type="button" className="text-button inline" disabled={account.busy} onClick={() => connectChatGpt().catch(console.error)}>
          {account.busy ? "Waiting for the browser…" : "Sign in with ChatGPT"}
        </button>
      </EmptyState>
    );
  if (!account.planUsage)
    return (
      <EmptyState icon="chat" title="Plan usage is off">
        HoverPill can only answer when it may use your ChatGPT plan.{" "}
        <button type="button" className="text-button inline" disabled={account.busy} onClick={() => connectChatGpt(true).catch(console.error)}>
          Turn it on
        </button>
      </EmptyState>
    );
  return <ChatPanel chat={chat} onSettings={onSettings} />;
}

function ChatPanel({ chat, onSettings }: { chat: Chat; onSettings: () => void }) {
  const [draft, setDraft] = useState("");
  const input = useRef<HTMLTextAreaElement>(null);
  const log = useRef<HTMLDivElement>(null);
  const { turns, pending, busy } = chat;

  // Keep the newest words in view as the answer streams in.
  useLayoutEffect(() => {
    log.current?.scrollTo({ top: log.current.scrollHeight });
  }, [turns.length, pending?.text, pending?.error]);

  // Leaving the tab (or the notch folding up) gives the keyboard back.
  useEffect(() => () => notchTyping(false), []);

  const send = () => {
    if (!draft.trim() || busy) return;
    chat.ask(draft);
    setDraft("");
  };
  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
    if (e.key === "Escape") input.current?.blur();
  };

  return (
    <div className="chat">
      <div className="chat-log" ref={log}>
        {turns.length === 0 && !pending && <p className="chat-hint">Ask anything. Answers use your ChatGPT plan and aren't saved.</p>}
        {turns.map((t, i) => (
          <p key={i} className={`chat-turn is-${t.role}`}>
            {t.text}
          </p>
        ))}
        {pending && (
          <p className="chat-turn is-assistant">
            {pending.text}
            {!pending.error && <span className="chat-caret" aria-hidden />}
          </p>
        )}
        {pending?.error && (
          <p className="chat-error" role="alert">
            {pending.error}{" "}
            {pending.usage ? (
              <button type="button" className="text-button inline" onClick={openChatGptUsage}>
                See usage
              </button>
            ) : (
              <button type="button" className="text-button inline" onClick={onSettings}>
                Settings
              </button>
            )}
          </p>
        )}
      </div>
      <div className="chat-input">
        <textarea
          ref={input}
          rows={1}
          value={draft}
          placeholder="Ask ChatGPT"
          aria-label="Ask ChatGPT"
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={onKey}
          onFocus={() => notchTyping(true)}
          onBlur={() => notchTyping(false)}
        />
        {busy ? (
          <button type="button" className="round-button" aria-label="Stop the answer" title="Stop" onClick={chat.stop}>
            <Icon name="stop" />
          </button>
        ) : (
          <button type="button" className="round-button is-send" aria-label="Send" title="Send" disabled={!draft.trim()} onClick={send}>
            <Icon name="send" />
          </button>
        )}
        <button type="button" className="round-button" aria-label="Clear the conversation" title="Clear" disabled={!turns.length && !pending} onClick={chat.clear}>
          <Icon name="clear" />
        </button>
      </div>
    </div>
  );
}

// ---------- Timer tab ----------

const PRESETS_MIN = [1, 3, 5, 10, 15, 25, 45, 60];

function TimerTab({ timers }: { timers: Timers }) {
  // Opens on whichever one is in use; otherwise the countdown.
  const [mode, setMode] = useState<"timer" | "stopwatch">(() =>
    timers.stopwatch.startedAt !== null && timers.timer.status !== "running" ? "stopwatch" : "timer",
  );
  return (
    <div className="timer-tab">
      <div className="segmented" role="tablist" aria-label="Timer or stopwatch">
        {(["timer", "stopwatch"] as const).map((m) => (
          <button key={m} type="button" role="tab" aria-selected={mode === m} className={`segment${mode === m ? " is-active" : ""}`} onClick={() => setMode(m)}>
            {m === "timer" ? "Timer" : "Stopwatch"}
          </button>
        ))}
      </div>
      {mode === "timer" ? <CountdownPane timers={timers} /> : <StopwatchPane timers={timers} />}
    </div>
  );
}

function CountdownPane({ timers }: { timers: Timers }) {
  const { timer, now, timerActions: act } = timers;
  const left = timerLeft(timer, now);
  const idle = timer.status === "idle" || timer.status === "done";
  const progress = idle ? 0 : 1 - left / timer.durationMs;
  return (
    <div className="timer-pane">
      <div className="timer-main">
        <div className="big-clock" aria-live="off">
          {formatCountdown(left)}
        </div>
        <div className="timer-progress" aria-hidden>
          <span style={{ transform: `scaleX(${progress})` }} />
        </div>
        <div className="timer-caption">
          {timer.status === "running" && `${describeDuration(timer.durationMs)} timer, ends at ${new Date(timer.endsAt ?? now).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}`}
          {timer.status === "paused" && `Paused, ${describeDuration(timer.durationMs)} timer`}
          {idle && "Pick a length, then start"}
        </div>
        <div className="timer-controls">
          {idle ? (
            <>
              <button type="button" className="round-button is-medium" aria-label="One minute less" onClick={() => act.setDuration(timer.durationMs - 60_000)} disabled={timer.durationMs <= 60_000}>
                <Icon name="minus" />
              </button>
              <button type="button" className="pill-button is-solid" onClick={act.start}>
                <Icon name="play" />
                Start
              </button>
              <button type="button" className="round-button is-medium" aria-label="One minute more" onClick={() => act.setDuration(timer.durationMs + 60_000)} disabled={timer.durationMs >= MAX_DURATION}>
                <Icon name="plus" />
              </button>
            </>
          ) : (
            <>
              <button type="button" className="pill-button" onClick={act.reset}>
                Reset
              </button>
              {timer.status === "running" ? (
                <button type="button" className="pill-button is-solid" onClick={act.pause}>
                  <Icon name="pause" />
                  Pause
                </button>
              ) : (
                <button type="button" className="pill-button is-solid" onClick={act.start}>
                  <Icon name="play" />
                  Resume
                </button>
              )}
            </>
          )}
        </div>
      </div>
      <div className="presets" role="group" aria-label="Timer length">
        {PRESETS_MIN.map((min) => (
          <button
            key={min}
            type="button"
            className={`preset${timer.durationMs === min * 60_000 ? " is-active" : ""}`}
            disabled={!idle}
            onClick={() => act.setDuration(min * 60_000)}
          >
            {min < 60 ? `${min} min` : "1 hour"}
          </button>
        ))}
      </div>
    </div>
  );
}

function StopwatchPane({ timers }: { timers: Timers }) {
  const { stopwatch, now, stopwatchActions: act } = timers;
  const elapsed = stopwatchElapsed(stopwatch, now);
  const running = stopwatch.startedAt !== null;
  const started = running || elapsed > 0;
  // Newest first, each with its own length and the running total.
  const laps = stopwatch.laps.map((at, i) => ({ n: i + 1, at, split: at - (stopwatch.laps[i - 1] ?? 0) })).reverse();
  return (
    <div className="timer-pane">
      <div className="timer-main">
        <div className="big-clock">{formatElapsed(elapsed, true)}</div>
        <div className="timer-caption">{running ? "Running" : started ? "Paused" : "Start, then add laps as you go"}</div>
        <div className="timer-controls">
          {running ? (
            <button type="button" className="pill-button" onClick={act.lap}>
              Lap
            </button>
          ) : (
            started && (
              <button type="button" className="pill-button" onClick={act.reset}>
                Reset
              </button>
            )
          )}
          {running ? (
            <button type="button" className="pill-button is-solid" onClick={act.pause}>
              <Icon name="pause" />
              Pause
            </button>
          ) : (
            <button type="button" className="pill-button is-solid" onClick={act.start}>
              <Icon name="play" />
              {started ? "Resume" : "Start"}
            </button>
          )}
        </div>
      </div>
      {laps.length ? (
        <ol className="laps" aria-label="Laps">
          {laps.map((l) => (
            <li key={l.n} className="lap">
              <span className="lap-n">Lap {l.n}</span>
              <span className="lap-split">{formatElapsed(l.split, true)}</span>
              <span className="lap-at">{formatElapsed(l.at, true)}</span>
            </li>
          ))}
        </ol>
      ) : (
        <div className="laps-empty">Laps show up here.</div>
      )}
    </div>
  );
}

/** "5-minute", "1-hour", "1 h 30 min" (for captions and the "Time's up" card). */
function describeDuration(ms: number) {
  const min = Math.round(ms / 60_000);
  if (min < 60) return `${min}-minute`;
  if (min % 60 === 0) return `${min / 60}-hour`;
  return `${Math.floor(min / 60)} h ${min % 60} min`;
}

// ---------- pieces ----------

function Transport({ media, onMedia, size }: { media: MediaState; onMedia: (a: MediaAction) => void; size: "small" | "large" }) {
  return (
    <div className={`transport is-${size}`}>
      <button type="button" className="transport-button" aria-label="Previous" onClick={() => onMedia("previous")}>
        <Icon name="prev" />
      </button>
      <button type="button" className="transport-button is-main" aria-label={media.playing ? "Pause" : "Play"} onClick={() => onMedia("play_pause")}>
        <Icon name={media.playing ? "pause" : "play"} />
      </button>
      <button type="button" className="transport-button" aria-label="Next" onClick={() => onMedia("next")}>
        <Icon name="next" />
      </button>
    </div>
  );
}

function Calendar({ today, events }: { today: Date; events: CalendarEvent[] }) {
  // Days with an event get a dot (multi-day all-day events mark their first day).
  const busy = new Set(events.map((e) => eventDate(e.start, e.allDay).toDateString()));
  const [offset, setOffset] = useState(0);
  const first = new Date(today.getFullYear(), today.getMonth() + offset, 1);
  const daysInMonth = new Date(first.getFullYear(), first.getMonth() + 1, 0).getDate();
  const lead = first.getDay();
  const cells = Array.from({ length: 42 }, (_, i) => new Date(first.getFullYear(), first.getMonth(), i - lead + 1));
  const rows = Math.ceil((lead + daysInMonth) / 7);
  const weekdays = Array.from({ length: 7 }, (_, i) =>
    new Date(2026, 1, 1 + i).toLocaleDateString([], { weekday: "narrow" }),
  );
  const isToday = (d: Date) => d.toDateString() === today.toDateString();

  return (
    <div className="calendar">
      <div className="calendar-head">
        <span className="calendar-month">{first.toLocaleDateString([], { month: "long", year: "numeric" })}</span>
        <div className="calendar-nav">
          {offset !== 0 && (
            <button type="button" className="text-button" onClick={() => setOffset(0)}>
              Today
            </button>
          )}
          <button type="button" className="round-button is-small" aria-label="Previous month" onClick={() => setOffset((o) => o - 1)}>
            <Icon name="chevronLeft" />
          </button>
          <button type="button" className="round-button is-small" aria-label="Next month" onClick={() => setOffset((o) => o + 1)}>
            <Icon name="chevronRight" />
          </button>
        </div>
      </div>
      <div className="calendar-grid" role="grid">
        {weekdays.map((w, i) => (
          <span key={`w${i}`} className="calendar-weekday">
            {w}
          </span>
        ))}
        {cells.slice(0, rows * 7).map((d) => (
          <span
            key={d.toISOString()}
            className={`calendar-day${d.getMonth() !== first.getMonth() ? " is-outside" : ""}${isToday(d) ? " is-today" : ""}${busy.has(d.toDateString()) ? " has-event" : ""}`}
          >
            {d.getDate()}
          </span>
        ))}
      </div>
    </div>
  );
}

function EmptyState({ icon, title, children }: { icon: IconName; title: string; children: ReactNode }) {
  return (
    <div className="empty">
      <Icon name={icon} className="empty-icon" />
      <div className="empty-title">{title}</div>
      <div className="empty-text">{children}</div>
    </div>
  );
}

function Artwork({ media, className }: { media: MediaState; className: string }) {
  return media.artwork ? <img className={`artwork ${className}`} src={media.artwork} alt="" /> : <div className={`artwork is-blank ${className}`} />;
}

function AppIcon({ app, icon, className }: { app: string; icon?: string; className: string }) {
  if (icon) return <img className={`app-icon ${className}`} src={icon} alt="" />;
  const hue = [...app].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 0);
  return (
    <span className={`app-icon is-letter ${className}`} style={{ background: `hsl(${hue} 55% 46%)` }} aria-hidden>
      {app.charAt(0).toUpperCase()}
    </span>
  );
}

/** Decorative bars in the cover's colour. Real levels would need loopback audio capture. */
function Visualizer({ playing }: { playing: boolean }) {
  return (
    <div className={`visualizer${playing ? " is-playing" : ""}`} aria-hidden>
      {[0, 1, 2, 3, 4].map((i) => (
        <span key={i} style={{ animationDelay: `${i * -0.19}s` }} />
      ))}
    </div>
  );
}

function useNow(everyMs: number) {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const t = window.setInterval(() => setNow(new Date()), everyMs);
    return () => window.clearInterval(t);
  }, [everyMs]);
  return now;
}

function ago(at: number, now: Date) {
  const s = Math.max(0, Math.round((now.getTime() - at) / 1000));
  if (s < 60) return "now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.round(m / 60);
  return h < 24 ? `${h}h` : `${Math.round(h / 24)}d`;
}
