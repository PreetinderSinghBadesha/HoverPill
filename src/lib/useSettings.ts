import { useCallback, useEffect, useRef, useState } from "react";
import { getSettings, on, saveSettings } from "./bridge";
import { DEFAULT_SETTINGS, type Settings } from "./types";

/** Read-only, always current: for the pill. */
export function useLiveSettings(): Settings {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  useEffect(() => {
    getSettings().then(setSettings).catch(console.error);
    return on("settings:changed", setSettings).off;
  }, []);
  return settings;
}

/**
 * Editable copy for the Settings window. Changes show immediately and are saved after
 * `debounceMs`; while a save is pending, echoes from Rust are ignored so a dragged slider
 * doesn't jump back to an older value.
 */
export function useEditableSettings() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const latest = useRef<Settings | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const queued = useRef(false);
  const inFlight = useRef(0);

  useEffect(() => {
    getSettings()
      .then((s) => {
        latest.current = s;
        setSettings(s);
      })
      .catch((e) => setError(String(e)));
    return on("settings:changed", (s) => {
      if (queued.current || inFlight.current > 0) return;
      latest.current = s;
      setSettings(s);
    }).off;
  }, []);

  const update = useCallback((change: (s: Settings) => Settings, debounceMs = 0) => {
    if (!latest.current) return;
    const next = change(latest.current);
    latest.current = next;
    setSettings(next);
    window.clearTimeout(timer.current);
    queued.current = true;
    timer.current = window.setTimeout(() => {
      queued.current = false;
      inFlight.current += 1;
      saveSettings(next)
        .then((stored) => {
          setError(null);
          // Only snap to the stored values if nothing newer was queued meanwhile.
          if (latest.current === next) {
            latest.current = stored;
            setSettings(stored);
          }
        })
        .catch((e) => setError(String(e)))
        .finally(() => (inFlight.current -= 1));
    }, debounceMs);
  }, []);

  return { settings, update, error };
}
