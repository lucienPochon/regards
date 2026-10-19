// The preview's Tour panel: every screen and state, one click each. Preview
// only; the app never loads it.

import { useEffect, useMemo, useRef, useState } from "react";
import { STOPS, type Stop, TOUR } from "./stops";
import "./tour.css";

const PREFS_KEY = "regards.prefs";
const OPEN_KEY = "regards.tour.open";

/** Defaults for every stop: no update question, the default layout. */
const DEFAULTS: Record<string, string> = { updates: "off", fresh: "1" };

function go(stop: Stop, dark: boolean) {
  const params: Record<string, string | null> = { ...DEFAULTS, ...stop.params };
  if (params.layout || params.sidebar) params.fresh = null;
  if (dark) params.theme = "dark";
  const query = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) if (value !== null) query.set(key, value);
  if (stop.steps?.length) query.set("do", stop.steps.join("|"));
  query.set("tour", stop.id);
  try {
    if (stop.prefs) window.localStorage.setItem(PREFS_KEY, JSON.stringify(stop.prefs));
    else window.localStorage.removeItem(PREFS_KEY);
    window.localStorage.setItem(OPEN_KEY, "1");
  } catch {
    // Storage blocked: the stop still loads, with the usual settings.
  }
  window.location.search = query.toString();
}

export function Tour() {
  const params = new URLSearchParams(window.location.search);
  const currentId = params.get("tour");
  const index = STOPS.findIndex((stop) => stop.id === currentId);
  const current = index >= 0 ? STOPS[index] : null;
  const [open, setOpen] = useState(() => {
    try {
      return window.localStorage.getItem(OPEN_KEY) === "1";
    } catch {
      return false;
    }
  });
  const [filter, setFilter] = useState("");
  const dark = params.get("theme") === "dark";
  const list = useRef<HTMLDivElement>(null);

  useEffect(() => {
    try {
      window.localStorage.setItem(OPEN_KEY, open ? "1" : "0");
    } catch {
      // Not remembered.
    }
    if (open) list.current?.querySelector(".tour-stop.current")?.scrollIntoView({ block: "center" });
  }, [open]);

  const groups = useMemo(() => {
    const words = filter.trim().toLowerCase();
    if (!words) return TOUR;
    return TOUR.map((group) => ({
      ...group,
      stops: group.stops.filter((stop) => `${group.title} ${stop.label} ${stop.note}`.toLowerCase().includes(words)),
    })).filter((group) => group.stops.length > 0);
  }, [filter]);

  const previous = index > 0 ? STOPS[index - 1] : null;
  const next = index < STOPS.length - 1 ? STOPS[index + 1] : null;

  if (!open) {
    return (
      <button type="button" className="tour-launch" onClick={() => setOpen(true)}>
        Tour{current ? ` · ${index + 1}/${STOPS.length}` : ` · ${STOPS.length} screens`}
      </button>
    );
  }

  return (
    <aside className="tour" aria-label="Preview tour">
      <header className="tour-head">
        <strong>Preview tour</strong>
        <span className="tour-count">{STOPS.length} screens</span>
        <button
          type="button"
          className="tour-theme"
          aria-pressed={dark}
          onClick={() => {
            const query = new URLSearchParams(window.location.search);
            if (dark) query.delete("theme");
            else query.set("theme", "dark");
            window.location.search = query.toString();
          }}
        >
          {dark ? "Light" : "Dark"}
        </button>
        <button type="button" className="tour-close" aria-label="Hide the tour" onClick={() => setOpen(false)}>
          ×
        </button>
      </header>
      <div className="tour-now">
        {current ? (
          <>
            <span className="tour-step">
              {index + 1} of {STOPS.length}
            </span>
            <strong>{current.label}</strong>
            <p>{current.note}</p>
          </>
        ) : (
          <p>Choose a screen below, or start at the first and go through them with Next.</p>
        )}
        <div className="tour-nav">
          <button type="button" disabled={!previous} onClick={() => previous && go(previous, dark)}>
            ‹ Previous
          </button>
          {current && (
            <button type="button" onClick={() => go(current, dark)}>
              Again
            </button>
          )}
          <button type="button" className="tour-next" onClick={() => go(next ?? STOPS[0], dark)}>
            {current ? (next ? "Next ›" : "From the start") : "Start ›"}
          </button>
        </div>
      </div>
      <input
        className="tour-filter"
        type="search"
        placeholder="Find a screen"
        value={filter}
        onChange={(event) => setFilter(event.target.value)}
      />
      <div className="tour-list" ref={list}>
        {groups.map((group) => (
          <section key={group.title}>
            <h4>{group.title}</h4>
            {group.stops.map((stop) => (
              <button
                key={stop.id}
                type="button"
                className={stop.id === currentId ? "tour-stop current" : "tour-stop"}
                title={stop.note}
                onClick={() => go(stop, dark)}
              >
                {stop.label}
              </button>
            ))}
          </section>
        ))}
      </div>
    </aside>
  );
}
