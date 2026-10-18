import { useLayoutEffect, useRef, type CSSProperties } from "react";
import { AnimatePresence, motion, type Transition } from "motion/react";
import { inTauri, mediaCommand, openSettings, resolvePermission, setHitRect } from "../lib/bridge";
import { pickView, usePillState } from "../lib/usePillState";
import { formatCountdown, formatElapsed, stopwatchElapsed, timerLeft, useTimers } from "../lib/useTimers";
import { useLiveSettings } from "../lib/useSettings";
import { useArtworkColor } from "../lib/useArtworkColor";
import { useAccounts, useChat } from "../lib/useAccounts";
import type { View } from "../lib/types";
import { AlarmView, ExpandedView, IdleView, MediaCompactView, NotificationView, PermissionView, TimerCompactView } from "./views";

/**
 * Target size per view, in CSS px. The window is fixed (see notch.rs); only the notch morphs.
 * `radius` is the bottom corners; attached to the top edge the top corners are square.
 */
type Size = { width: number; height: number; radius: number };

const SIZES: Record<View, Size> = {
  idle: { width: 190, height: 30, radius: 12 },
  media: { width: 300, height: 32, radius: 14 },
  timer: { width: 210, height: 32, radius: 14 },
  alarm: { width: 440, height: 78, radius: 24 },
  notification: { width: 420, height: 78, radius: 24 },
  permission: { width: 520, height: 204, radius: 26 },
  expanded: { width: 680, height: 300, radius: 30 },
};

/** Idle with "shrink when idle" on: a thin bar that barely overlaps anything. */
const IDLE_BAR: Size = { width: 120, height: 6, radius: 3 };

/** Pointed at while closed: faint enough to read the browser tab underneath, still findable. */
const PEEK_OPACITY = 0.12;

// Slightly under-damped so the notch overshoots a touch as it drops, then settles.
const SPRING: Transition = { type: "spring", stiffness: 420, damping: 32, mass: 0.9 };

export function Pill() {
  const settings = useLiveSettings();
  const { scale, topOffset } = settings.display;
  const attached = topOffset === 0;
  const durationMs = settings.notifications.durationSecs * 1000;
  const { state, dispatch, setOpen } = usePillState(durationMs);
  const timers = useTimers();
  const accounts = useAccounts();
  const chat = useChat();
  const { timer, stopwatch, now } = timers;
  const ringing = timer.status === "done";
  // What the closed notch shows ticking: the countdown wins over the stopwatch.
  const clock =
    timer.status === "running" || timer.status === "paused"
      ? formatCountdown(timerLeft(timer, now))
      : stopwatch.startedAt !== null
        ? formatElapsed(stopwatchElapsed(stopwatch, now))
        : null;
  const view = pickView(state, { ringing, ticking: clock !== null });
  const accent = useArtworkColor(state.media?.artwork);
  const ref = useRef<HTMLDivElement>(null);
  const size = view === "idle" && settings.display.compactIdle ? IDLE_BAR : SIZES[view];
  // Behind a fullscreen app only what's waiting on you (a Claude request, a finished timer) shows.
  const hidden = state.fullscreen && view !== "permission" && view !== "alarm";
  // Closed (and a passing notification) never takes clicks, so browser tabs underneath stay usable.
  const interactive = !hidden && (view === "expanded" || view === "permission" || view === "alarm");
  // Pointing at the closed notch fades it, unless resting is how you open it.
  const faded = state.peek && !interactive && !settings.display.openOnHover;

  // Report the notch's on-screen box to Rust every time it changes size (every frame of the
  // spring), so click-through always matches what's drawn. Scale, top gap and window resizes
  // move it without resizing it, so those re-report too.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const report = () => {
      const r = el.getBoundingClientRect();
      setHitRect({ x: r.x, y: r.y, width: r.width, height: r.height }, interactive);
    };
    report();
    const ro = new ResizeObserver(report);
    ro.observe(el);
    window.addEventListener("resize", report);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", report);
    };
  }, [scale, topOffset, interactive]);

  const permission = state.permissions[0];
  const notification = state.queue[0];
  const bottom = Math.min(size.radius, size.height / 2);
  const top = attached ? 0 : bottom;

  return (
    <div className="stage" style={{ paddingTop: topOffset }}>
      {/* Scaled as a whole so the spring keeps animating unscaled sizes; the hit rect is measured after scaling. */}
      <div className="scaler" style={{ transform: `scale(${scale})` }}>
        <motion.div
          ref={ref}
          className={`notch ${attached ? "is-attached" : "is-floating"}${accent ? " has-accent" : ""}`}
          style={{ ...(accent ? { "--accent": accent } : {}), "--ear": `${Math.min(12, size.height)}px` } as CSSProperties}
          initial={false}
          animate={{
            width: size.width,
            height: size.height,
            borderBottomLeftRadius: bottom,
            borderBottomRightRadius: bottom,
            borderTopLeftRadius: top,
            borderTopRightRadius: top,
            opacity: hidden ? 0 : faded ? PEEK_OPACITY : 1,
          }}
          transition={{ ...SPRING, opacity: { duration: 0.15, ease: "easeOut" } }}
          // In Tauri, opening is decided by the Rust cursor poller; these only drive the browser preview.
          onMouseEnter={inTauri ? undefined : () => setOpen(true)}
          onMouseLeave={inTauri ? undefined : () => setOpen(false)}
        >
          {attached && (
            <>
              <span className="ear is-left" aria-hidden />
              <span className="ear is-right" aria-hidden />
            </>
          )}
          <div className="notch-clip">
            <AnimatePresence initial={false}>
              <motion.div
                key={view === "permission" ? `p-${permission?.id}` : view === "notification" ? `n-${notification?.id}` : view}
                className="content"
                style={{ width: size.width, height: size.height }}
                initial={{ opacity: 0, filter: "blur(6px)" }}
                animate={{ opacity: 1, filter: "blur(0px)", transition: { duration: 0.2, delay: 0.07 } }}
                exit={{ opacity: 0, filter: "blur(4px)", transition: { duration: 0.1 } }}
              >
                {view === "idle" && <IdleView />}
                {view === "media" && state.media && <MediaCompactView media={state.media} clock={clock} />}
                {view === "timer" && clock && <TimerCompactView clock={clock} />}
                {view === "alarm" && <AlarmView durationMs={timer.durationMs} onSnooze={timers.timerActions.snooze} onDone={timers.timerActions.reset} />}
                {view === "notification" && notification && <NotificationView item={notification} durationMs={durationMs} />}
                {view === "permission" && permission && (
                  <PermissionView
                    request={permission}
                    queued={state.permissions.length - 1}
                    onDecide={(decision) => {
                      resolvePermission(permission.id, decision);
                      dispatch({ type: "decide", id: permission.id, decision });
                    }}
                  />
                )}
                {view === "expanded" && (
                  <ExpandedView
                    tab={state.tab}
                    onTab={(value) => dispatch({ type: "tab", value })}
                    media={state.media}
                    onMedia={(action) => {
                      mediaCommand(action);
                      // Optimistic: the real state arrives back via `pill:media` once the OS confirms.
                      if (action === "play_pause" && state.media)
                        dispatch({ type: "media", value: { ...state.media, playing: !state.media.playing } });
                    }}
                    history={state.history}
                    unseen={state.unseen}
                    onClearHistory={() => dispatch({ type: "clearHistory" })}
                    decisions={state.decisions}
                    timers={timers}
                    onSettings={openSettings}
                    calendar={state.calendar}
                    chat={chat}
                    chatgpt={accounts.chatgpt}
                  />
                )}
              </motion.div>
            </AnimatePresence>
          </div>
        </motion.div>
      </div>
    </div>
  );
}
