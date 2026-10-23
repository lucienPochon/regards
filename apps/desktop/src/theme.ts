import { useEffect, useState } from "react";

export type Theme = "system" | "light" | "dark";

const KEY = "regards.theme";

export function savedTheme(): Theme {
  try {
    const value = window.localStorage.getItem(KEY);
    return value === "light" || value === "dark" ? value : "system";
  } catch {
    return "system";
  }
}

/** Light, dark, or as the system is; remembered on this computer. */
export function applyTheme(theme: Theme, save = false): void {
  if (theme === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
  if (!save) return;
  try {
    if (theme === "system") window.localStorage.removeItem(KEY);
    else window.localStorage.setItem(KEY, theme);
  } catch {
    // Not remembered; it still applies now.
  }
}

/** Whether the app is dark now: chosen so, or the system's colours while it follows the system. */
export function isDark(): boolean {
  const chosen = document.documentElement.dataset.theme;
  return chosen ? chosen === "dark" : window.matchMedia("(prefers-color-scheme: dark)").matches;
}

/** `isDark`, kept current as the theme is chosen or the system's colours change. */
export function useDark(): boolean {
  const [dark, setDark] = useState(isDark);
  useEffect(() => {
    const update = () => setDark(isDark());
    const system = window.matchMedia("(prefers-color-scheme: dark)");
    system.addEventListener("change", update);
    const chosen = new MutationObserver(update);
    chosen.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => {
      system.removeEventListener("change", update);
      chosen.disconnect();
    };
  }, []);
  return dark;
}
