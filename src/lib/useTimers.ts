import { useCallback, useEffect, useRef, useState } from "react";

/**
 * A countdown timer and a stopwatch that keep running while the notch is closed. Both are
 * anchored to wall-clock times (Date.now), never to counted ticks, so a closed, rarely
 * repainted notch stays exact.
 */

export type TimerStatus = "idle" | "running" | "paused" | "done";

export type TimerState = {
  /** The length you set. */
  durationMs: number;
  status: TimerStatus;
  /** While running: when it reaches zero. */
  endsAt: number | null;
  /** While paused: what was left. */
  leftMs: number;
};

export type StopwatchState = {
  /** While running: when the current run started. */
  startedAt: number | null;
  /** Time banked from earlier runs. */
  bankedMs: number;
  /** Elapsed time at each lap, oldest first. */
  laps: number[];
};

const DURATION_KEY = "hoverpill.timer.duration";
const DEFAULT_DURATION = 5 * 60_000;
export const MAX_DURATION = 10 * 3_600_000;

function savedDuration() {
  try {
    const v = Number(localStorage.getItem(DURATION_KEY));
    if (v >= 60_000 && v <= MAX_DURATION) return v;
  } catch {
    // Storage can be unavailable; the default is fine.
  }
  return DEFAULT_DURATION;
}

export function timerLeft(t: TimerState, now: number) {
  if (t.status === "running" && t.endsAt) return Math.max(0, t.endsAt - now);
  if (t.status === "paused") return t.leftMs;
  if (t.status === "done") return 0;
  return t.durationMs;
}

export function stopwatchElapsed(s: StopwatchState, now: number) {
  return s.bankedMs + (s.startedAt ? now - s.startedAt : 0);
}

/** Time left on a countdown: rounds up, so it reads 5:00 until a whole second has gone. */
export function formatCountdown(ms: number) {
  return clock(Math.ceil(ms / 1000) * 10, false);
}

/** Time elapsed on the stopwatch: rounds down; `tenths` adds 4:05.3. */
export function formatElapsed(ms: number, tenths = false) {
  return clock(Math.floor(ms / 100), tenths);
}

/** 4:05, 12:00, 1:02:03 from a count of tenths of a second. */
function clock(totalTenths: number, tenths: boolean) {
  const h = Math.floor(totalTenths / 36_000);
  const m = Math.floor((totalTenths % 36_000) / 600);
  const s = Math.floor((totalTenths % 600) / 10);
  const pad = (n: number) => String(n).padStart(2, "0");
  const base = h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
  return tenths ? `${base}.${totalTenths % 10}` : base;
}

/** A soft three-note chime, made with Web Audio so there's no sound file to ship. */
function chime(ctx: AudioContext | null) {
  if (!ctx) return;
  const notes = [880, 1108.73, 1318.51]; // A5, C#6, E6
  notes.forEach((freq, i) => {
    const start = ctx.currentTime + i * 0.18;
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = "sine";
    osc.frequency.value = freq;
    gain.gain.setValueAtTime(0.0001, start);
    gain.gain.exponentialRampToValueAtTime(0.18, start + 0.02);
    gain.gain.exponentialRampToValueAtTime(0.0001, start + 0.9);
    osc.connect(gain).connect(ctx.destination);
    osc.start(start);
    osc.stop(start + 1);
  });
}

export function useTimers() {
  const [timer, setTimer] = useState<TimerState>(() => ({ durationMs: savedDuration(), status: "idle", endsAt: null, leftMs: 0 }));
  const [stopwatch, setStopwatch] = useState<StopwatchState>({ startedAt: null, bankedMs: 0, laps: [] });
  const [now, setNow] = useState(() => Date.now());
  // Browsers only allow sound after a click on the page; Start is that click, so the audio
  // context is created there and reused when the timer ends.
  const audio = useRef<AudioContext | null>(null);

  const running = timer.status === "running" || stopwatch.startedAt !== null;
  useEffect(() => {
    if (!running) return;
    const t = window.setInterval(() => setNow(Date.now()), 100);
    return () => window.clearInterval(t);
  }, [running]);

  // Ring exactly when the timer ends, not on the next tick.
  useEffect(() => {
    if (timer.status !== "running" || !timer.endsAt) return;
    const t = window.setTimeout(() => {
      setTimer((s) => (s.status === "running" ? { ...s, status: "done", endsAt: null } : s));
      chime(audio.current);
    }, Math.max(0, timer.endsAt - Date.now()));
    return () => window.clearTimeout(t);
  }, [timer.status, timer.endsAt]);

  const unlockAudio = () => {
    try {
      audio.current ??= new AudioContext();
      void audio.current.resume();
    } catch {
      // No audio device: the card still drops down.
    }
  };

  const setDuration = useCallback((ms: number) => {
    const durationMs = Math.min(MAX_DURATION, Math.max(60_000, ms));
    try {
      localStorage.setItem(DURATION_KEY, String(durationMs));
    } catch {
      // Not remembering the length is harmless.
    }
    setTimer((s) => (s.status === "idle" ? { ...s, durationMs } : s));
  }, []);

  const timerActions = {
    setDuration,
    start: () => {
      unlockAudio();
      setTimer((s) => {
        const left = s.status === "paused" ? s.leftMs : s.durationMs;
        return { ...s, status: "running", endsAt: Date.now() + left, leftMs: 0 };
      });
      setNow(Date.now());
    },
    pause: () => setTimer((s) => (s.status === "running" && s.endsAt ? { ...s, status: "paused", leftMs: Math.max(0, s.endsAt - Date.now()), endsAt: null } : s)),
    reset: () => setTimer((s) => ({ ...s, status: "idle", endsAt: null, leftMs: 0 })),
    /** From the "Time's up" card: one more minute, running straight away. */
    snooze: () => {
      setTimer((s) => ({ ...s, status: "running", endsAt: Date.now() + 60_000, leftMs: 0 }));
      setNow(Date.now());
    },
  };

  const stopwatchActions = {
    start: () => setStopwatch((s) => (s.startedAt ? s : { ...s, startedAt: Date.now() })),
    pause: () => setStopwatch((s) => (s.startedAt ? { ...s, bankedMs: s.bankedMs + Date.now() - s.startedAt, startedAt: null } : s)),
    lap: () => setStopwatch((s) => (s.startedAt ? { ...s, laps: [...s.laps, stopwatchElapsed(s, Date.now())] } : s)),
    reset: () => setStopwatch({ startedAt: null, bankedMs: 0, laps: [] }),
  };

  return { timer, stopwatch, now, timerActions, stopwatchActions };
}

export type Timers = ReturnType<typeof useTimers>;
