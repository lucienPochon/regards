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
