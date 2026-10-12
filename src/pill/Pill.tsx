import { useLayoutEffect, useRef } from "react";
import { AnimatePresence, motion, type Transition } from "motion/react";
import { inTauri, mediaCommand, resolvePermission, setHitRect } from "../lib/bridge";
import { usePillState } from "../lib/usePillState";
import type { View } from "../lib/types";
import { ExpandedView, IdleView, MediaCompactView, NotificationView, PermissionView } from "./views";

/** Target size per view, in CSS px. The window itself is fixed (see tauri.conf.json); only the pill morphs. */
const SIZES: Record<View, { width: number; height: number; radius: number }> = {
  idle: { width: 168, height: 32, radius: 16 },
  media: { width: 264, height: 32, radius: 16 },
  expanded: { width: 400, height: 124, radius: 34 },
  notification: { width: 380, height: 76, radius: 30 },
  permission: { width: 480, height: 192, radius: 32 },
};

// Slightly under-damped so the pill overshoots a touch, Dynamic Island style.
const SPRING: Transition = { type: "spring", stiffness: 420, damping: 30, mass: 0.9 };

export function Pill() {
  const { state, view, dispatch, setHover } = usePillState();
  const ref = useRef<HTMLDivElement>(null);
  const size = SIZES[view];

  // Report the pill's on-screen box to Rust every time it changes size (that includes every
  // frame of the spring), so click-through always matches what's visibly drawn.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const report = () => {
      const r = el.getBoundingClientRect();
      setHitRect({ x: r.x, y: r.y, width: r.width, height: r.height });
    };
    report();
    const ro = new ResizeObserver(report);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const permission = state.permissions[0];
  const notification = state.notifications[0];

  return (
    <div className="stage">
      <motion.div
        ref={ref}
        className="pill"
        initial={false}
        animate={{ width: size.width, height: size.height, borderRadius: size.radius }}
        transition={SPRING}
        // In Tauri, hover comes from the Rust cursor poller; these only drive the browser preview.
        onMouseEnter={inTauri ? undefined : () => setHover(true)}
        onMouseLeave={inTauri ? undefined : () => setHover(false)}
      >
        <AnimatePresence initial={false}>
          <motion.div
            key={view === "permission" ? `p-${permission?.id}` : view === "notification" ? `n-${notification?.id}` : view}
            className="content"
            style={{ width: size.width, height: size.height }}
            initial={{ opacity: 0, scale: 0.94, filter: "blur(6px)" }}
            animate={{ opacity: 1, scale: 1, filter: "blur(0px)", transition: { duration: 0.22, delay: 0.06 } }}
            exit={{ opacity: 0, scale: 0.97, filter: "blur(4px)", transition: { duration: 0.12 } }}
          >
            {view === "idle" && <IdleView />}
            {view === "media" && state.media && <MediaCompactView media={state.media} />}
            {view === "expanded" && (
              <ExpandedView
                media={state.media}
                onCommand={(action) => {
                  mediaCommand(action);
                  // Optimistic: the real state arrives back via `pill:media` once the OS confirms.
                  if (action === "play_pause" && state.media)
                    dispatch({ type: "media", value: { ...state.media, playing: !state.media.playing } });
                }}
              />
            )}
            {view === "notification" && notification && (
              <NotificationView
                item={notification}
                paused={state.hovered}
                onDismiss={() => dispatch({ type: "dismissNotification", id: notification.id })}
              />
            )}
            {view === "permission" && permission && (
              <PermissionView
                request={permission}
                queued={state.permissions.length - 1}
                onDecide={(decision) => {
                  resolvePermission(permission.id, decision);
                  dispatch({ type: "resolvePermission", id: permission.id });
                }}
              />
            )}
          </motion.div>
        </AnimatePresence>
      </motion.div>
    </div>
  );
}
