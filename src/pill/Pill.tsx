import { useLayoutEffect, useRef, type CSSProperties } from "react";
import { AnimatePresence, motion, type Transition } from "motion/react";
import { inTauri, mediaCommand, openSettings, resolvePermission, setHitRect } from "../lib/bridge";
import { usePillState } from "../lib/usePillState";
import { useLiveSettings } from "../lib/useSettings";
import { useArtworkColor } from "../lib/useArtworkColor";
import type { View } from "../lib/types";
import { ExpandedView, IdleView, MediaCompactView, NotificationView, PermissionView } from "./views";

/**
 * Target size per view, in CSS px. The window is fixed (see notch.rs); only the notch morphs.
 * `radius` is the bottom corners; attached to the top edge the top corners are square.
 */
const SIZES: Record<View, { width: number; height: number; radius: number }> = {
  idle: { width: 190, height: 30, radius: 12 },
  media: { width: 300, height: 32, radius: 14 },
  notification: { width: 420, height: 78, radius: 24 },
  permission: { width: 520, height: 204, radius: 26 },
  expanded: { width: 680, height: 300, radius: 30 },
};

// Slightly under-damped so the notch overshoots a touch as it drops, then settles.
const SPRING: Transition = { type: "spring", stiffness: 420, damping: 32, mass: 0.9 };

export function Pill() {
  const settings = useLiveSettings();
  const { scale, topOffset } = settings.display;
  const attached = topOffset === 0;
  const durationMs = settings.notifications.durationSecs * 1000;
  const { state, view, dispatch, setOpen } = usePillState(durationMs);
  const accent = useArtworkColor(state.media?.artwork);
  const ref = useRef<HTMLDivElement>(null);
  const size = SIZES[view];
  // Closed (and a passing notification) never takes clicks, so browser tabs underneath stay usable.
  const interactive = view === "expanded" || view === "permission";

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
          style={accent ? ({ "--accent": accent } as CSSProperties) : undefined}
          initial={false}
          animate={{
            width: size.width,
            height: size.height,
            borderBottomLeftRadius: bottom,
            borderBottomRightRadius: bottom,
            borderTopLeftRadius: top,
            borderTopRightRadius: top,
          }}
          transition={SPRING}
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
                {view === "media" && state.media && <MediaCompactView media={state.media} />}
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
                    onSettings={openSettings}
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
