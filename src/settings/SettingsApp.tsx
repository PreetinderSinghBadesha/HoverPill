import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { getAutostart, inTauri, listMonitors, mediaPlayers, recentNotificationApps, setAutostart } from "../lib/bridge";
import { useEditableSettings } from "../lib/useSettings";
import { DEFAULT_SETTINGS, RANGES, type MonitorInfo, type PlayerInfo, type Settings } from "../lib/types";
import { Section, SelectRow, SliderRow, SwitchRow, ToggleList } from "./controls";

type Update = (change: (s: Settings) => Settings, debounceMs?: number) => void;

const TABS = [
  { id: "general", label: "General", icon: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Zm7.4-3a7.4 7.4 0 0 0-.1-1.2l2-1.6-2-3.4-2.4 1a7.6 7.6 0 0 0-2-1.2L14.5 3h-5l-.4 2.6a7.6 7.6 0 0 0-2 1.2l-2.4-1-2 3.4 2 1.6a7.4 7.4 0 0 0 0 2.4l-2 1.6 2 3.4 2.4-1a7.6 7.6 0 0 0 2 1.2l.4 2.6h5l.4-2.6a7.6 7.6 0 0 0 2-1.2l2.4 1 2-3.4-2-1.6c.1-.4.1-.8.1-1.2Z" },
  { id: "display", label: "Display", icon: "M3 5h18v11H3zM8 20h8M12 16v4" },
  { id: "notifications", label: "Notifications", icon: "M18 16V11a6 6 0 1 0-12 0v5l-2 2h16l-2-2ZM10 20a2 2 0 0 0 4 0" },
  { id: "media", label: "Media", icon: "M9 18V6l11-2v12M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0Zm11-2a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z" },
] as const;
type TabId = (typeof TABS)[number]["id"];

const TAB_KEY = "hoverpill.settings.tab";

/** The top-strip reservation uses a Windows shell API; there's no equivalent wired up on Linux yet. */
const IS_WINDOWS = navigator.userAgent.includes("Windows");

function initialTab(): TabId {
  try {
    const saved = localStorage.getItem(TAB_KEY);
    if (TABS.some((t) => t.id === saved)) return saved as TabId;
  } catch {
    // Storage can be unavailable; the first tab is fine.
  }
  return "general";
}

export function SettingsApp() {
  const { settings, update, error } = useEditableSettings();
  const [tab, setTab] = useState<TabId>(initialTab);
  const tabRefs = useRef<Record<string, HTMLButtonElement | null>>({});

  const select = (id: TabId, focus = false) => {
    setTab(id);
    if (focus) tabRefs.current[id]?.focus();
    try {
      localStorage.setItem(TAB_KEY, id);
    } catch {
      // Not remembering the tab is harmless.
    }
  };

  // Arrow keys move between tabs (WAI-ARIA tabs pattern, automatic activation).
  const onTabKey = (e: KeyboardEvent) => {
    const i = TABS.findIndex((t) => t.id === tab);
    const to = { ArrowRight: i + 1, ArrowLeft: i - 1, Home: 0, End: TABS.length - 1 }[e.key];
    if (to === undefined) return;
    e.preventDefault();
    select(TABS[(to + TABS.length) % TABS.length].id, true);
  };

  return (
    <div className="app">
      <header className="header">
        <div className="brand">
          <span className="brand-pill" aria-hidden />
          HoverPill
        </div>
        <div className="tabs" role="tablist" aria-label="Settings sections" onKeyDown={onTabKey}>
          {TABS.map((t) => (
            <button
              key={t.id}
              ref={(el) => {
                tabRefs.current[t.id] = el;
              }}
              id={`tab-${t.id}`}
              role="tab"
              type="button"
              className="tab"
              aria-selected={tab === t.id}
              aria-controls={`panel-${t.id}`}
              tabIndex={tab === t.id ? 0 : -1}
              onClick={() => select(t.id)}
            >
              <svg viewBox="0 0 24 24" aria-hidden>
                <path d={t.icon} />
              </svg>
              {t.label}
            </button>
          ))}
        </div>
      </header>

      <main className="panel" role="tabpanel" id={`panel-${tab}`} aria-labelledby={`tab-${tab}`} tabIndex={0}>
        {error && (
          <div className="banner" role="alert">
            Couldn't save: {error}
          </div>
        )}
        {!settings ? (
          <div className="empty">Loading…</div>
        ) : (
          <>
            {tab === "general" && <GeneralTab />}
            {tab === "display" && <DisplayTab settings={settings} update={update} />}
            {tab === "notifications" && <NotificationsTab settings={settings} update={update} />}
            {tab === "media" && <MediaTab settings={settings} update={update} />}
          </>
        )}
      </main>
    </div>
  );
}

/** Re-runs `load` every `ms` while the tab is open, so lists pick up apps as they appear. */
function usePolled<T>(load: () => Promise<T>, initial: T, ms = 3000): T {
  const [value, setValue] = useState<T>(initial);
  useEffect(() => {
    let alive = true;
    const run = () => load().then((v) => alive && setValue(v), console.error);
    run();
    const t = window.setInterval(run, ms);
    return () => {
      alive = false;
      window.clearInterval(t);
    };
  }, [load, ms]);
  return value;
}

function GeneralTab() {
  const [autostart, setAutostartState] = useState<boolean | null>(null);
  const [autostartError, setAutostartError] = useState<string | null>(null);
  const [version, setVersion] = useState("");

  useEffect(() => {
    getAutostart().then(setAutostartState, console.error);
    if (inTauri) getVersion().then(setVersion, console.error);
  }, []);

  const toggle = (on: boolean) => {
    setAutostartError(null);
    setAutostart(on)
      .then(setAutostartState)
      .catch((e) => {
        setAutostartError(String(e));
        getAutostart().then(setAutostartState, console.error);
      });
  };

  return (
    <>
      <Section title="Startup">
        <SwitchRow
          title="Start at login"
          description={autostartError ?? "Open HoverPill automatically when you sign in."}
          checked={autostart ?? false}
          disabled={autostart === null}
          onChange={toggle}
        />
      </Section>
      <Section title="About">
        <div className="about">
          <p>
            HoverPill {version && <span className="muted">v{version}</span>}
          </p>
          <p className="muted">The tray icon has quick actions, including Quit. Opening HoverPill again while it's running brings you here.</p>
        </div>
      </Section>
    </>
  );
}

function DisplayTab({ settings, update }: { settings: Settings; update: Update }) {
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);
  useEffect(() => {
    listMonitors().then(setMonitors, console.error);
  }, []);

  const d = settings.display;
  const PRIMARY = "__primary__";
  const chosenMissing = d.monitor !== null && !monitors.some((m) => m.name === d.monitor);
  const options = [
    { value: PRIMARY, label: "Primary monitor" },
    ...monitors.map((m) => ({ value: m.name, label: m.label })),
    ...(chosenMissing && d.monitor ? [{ value: d.monitor, label: "Disconnected monitor (using primary)" }] : []),
  ];
  const setDisplay = (patch: Partial<Settings["display"]>, debounceMs = 0) =>
    update((s) => ({ ...s, display: { ...s.display, ...patch } }), debounceMs);

  return (
    <>
      <Section title="Position">
        <SelectRow
          title="Monitor"
          description="The pill sits at the top centre of this screen."
          value={d.monitor ?? PRIMARY}
          options={options}
          onChange={(v) => setDisplay({ monitor: v === PRIMARY ? null : v })}
        />
        <SliderRow
          title="Distance from top"
          description="At 0 the notch is attached to the top edge. Any gap turns it into a floating pill."
          value={d.topOffset}
          {...RANGES.topOffset}
          format={(v) => `${v} px`}
          onChange={(v) => setDisplay({ topOffset: v }, 150)}
        />
      </Section>
      <Section title="Opening">
        <SwitchRow
          title="Open when the pointer rests on the notch"
          description="Push the pointer against the top edge above the notch to open it. With this off, pointing at the closed notch fades it out so you can see what's underneath. Clicks on the closed notch always go through to it."
          checked={d.openOnHover}
          onChange={(openOnHover) => setDisplay({ openOnHover })}
        />
      </Section>
      <Section title="Staying out of the way">
        <SwitchRow
          title="Shrink when nothing is happening"
          description="With nothing playing and no notification, the notch becomes a thin bar."
          checked={d.compactIdle}
          onChange={(compactIdle) => setDisplay({ compactIdle })}
        />
        <SwitchRow
          title="Hide during fullscreen apps"
          description="Videos, games and presentations get the whole screen. Claude Code requests still appear."
          checked={d.hideInFullscreen}
          onChange={(hideInFullscreen) => setDisplay({ hideInFullscreen })}
        />
        <SwitchRow
          title="Keep windows below the notch"
          description={
            IS_WINDOWS
              ? "Reserves a strip across the top of the screen, like a taskbar, so maximised windows start below the notch. You lose that strip's height across the whole width."
              : "Only available on Windows for now."
          }
          checked={d.reserveSpace}
          disabled={!IS_WINDOWS}
          onChange={(reserveSpace) => setDisplay({ reserveSpace })}
        />
      </Section>
      <Section title="Size">
        <SliderRow
          title="Pill size"
          description="Scales the pill and everything in it."
          value={d.scale}
          {...RANGES.scale}
          format={(v) => `${Math.round(v * 100)}%`}
          onChange={(v) => setDisplay({ scale: v }, 150)}
        />
      </Section>
      <div className="actions">
        <button type="button" className="button" onClick={() => setDisplay(DEFAULT_SETTINGS.display)}>
          Reset display settings
        </button>
      </div>
    </>
  );
}

function NotificationsTab({ settings, update }: { settings: Settings; update: Update }) {
  const recent = usePolled(recentNotificationApps, [] as string[]);
  const n = settings.notifications;
  const blocked = (app: string) => n.blockedApps.some((b) => b.toLowerCase() === app.toLowerCase());
  const apps = [...recent, ...n.blockedApps.filter((b) => !recent.some((r) => r.toLowerCase() === b.toLowerCase()))];

  const setNotifications = (patch: Partial<Settings["notifications"]>, debounceMs = 0) =>
    update((s) => ({ ...s, notifications: { ...s.notifications, ...patch } }), debounceMs);

  return (
    <>
      <Section title="Notifications">
        <SwitchRow
          title="Show notifications in the pill"
          description="Mirror notifications from other apps."
          checked={n.enabled}
          onChange={(enabled) => setNotifications({ enabled })}
        />
        <SliderRow
          title="Show each notification for"
          description="Hovering the pill keeps a notification open."
          value={n.durationSecs}
          {...RANGES.durationSecs}
          format={(v) => `${v.toFixed(1)} s`}
          onChange={(durationSecs) => setNotifications({ durationSecs }, 150)}
        />
      </Section>
      <Section title="Apps">
        <ToggleList
          items={apps.map((app) => ({ key: app, label: app, on: !blocked(app), note: recent.includes(app) ? undefined : "not seen recently" }))}
          empty={<>Apps show up here after they send a notification. Turn one off to stop it from reaching the pill.</>}
          onToggle={(app, on) =>
            setNotifications({
              blockedApps: on ? n.blockedApps.filter((b) => b.toLowerCase() !== app.toLowerCase()) : [...n.blockedApps, app],
            })
          }
        />
      </Section>
      {!n.enabled && <Note>Notifications are off, so these app choices have no effect right now.</Note>}
    </>
  );
}

function MediaTab({ settings, update }: { settings: Settings; update: Update }) {
  const players = usePolled(mediaPlayers, [] as PlayerInfo[]);
  const m = settings.media;
  const setMedia = (patch: Partial<Settings["media"]>) => update((s) => ({ ...s, media: { ...s.media, ...patch } }));

  return (
    <>
      <Section title="Media">
        <SwitchRow
          title="Show media in the pill"
          description="Track, artwork and playback controls for whatever is playing."
          checked={m.enabled}
          onChange={(enabled) => setMedia({ enabled })}
        />
      </Section>
      <Section title="Players">
        <ToggleList
          items={players.map((p) => ({ key: p.id, label: p.label, on: !m.ignoredPlayers.includes(p.id) }))}
          empty={<>Players show up here while they're open. Turn one off (a browser, say) and the pill shows the next one instead.</>}
          onToggle={(id, on) =>
            setMedia({ ignoredPlayers: on ? m.ignoredPlayers.filter((p) => p !== id) : [...m.ignoredPlayers, id] })
          }
        />
      </Section>
      {!m.enabled && <Note>Media is off, so these player choices have no effect right now.</Note>}
    </>
  );
}

function Note({ children }: { children: ReactNode }) {
  return <p className="note">{children}</p>;
}
