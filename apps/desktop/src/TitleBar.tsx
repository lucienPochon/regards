import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import { Icon } from "./icons";
import { type Anchor, anchorOf } from "./Menu";

/** Which window frame the app draws around itself (see tauri.windows.conf.json and tauri.macos.conf.json). */
export const PLATFORM: "windows" | "mac" | "linux" = navigator.userAgent.includes("Windows")
  ? "windows"
  : /Mac|iPhone|iPad/.test(navigator.platform)
    ? "mac"
    : "linux";

function currentWindow() {
  try {
    return getCurrentWindow();
  } catch {
    // The browser preview has no window to drive.
    return null;
  }
}

/**
 * On Windows the app draws its own caption buttons, over the top right of the
 * window: the app menu, minimise, maximise or restore, and close (which keeps
 * Regards in the tray when background mode is on). The band around them drags
 * the window.
 */
export function TitleBar({ onAppMenu }: { onAppMenu: (anchor: Anchor) => void }) {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    const window = currentWindow();
    if (!window) return;
    const update = () => {
      window.isMaximized().then(setMaximized, () => {});
    };
    update();
    const stop = window.onResized(update);
    return () => {
      void stop.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []);
  if (PLATFORM !== "windows") return null;
  return (
    <div className="caption" role="toolbar" aria-label="Window">
      <button type="button" className="caption-btn" title="Menu" aria-label="Menu" aria-haspopup="menu" onClick={(event) => onAppMenu(anchorOf(event.currentTarget, "end"))}>
        <Icon name="chevronDown" size={13} />
      </button>
      <button type="button" className="caption-btn" title="Minimise" aria-label="Minimise" onClick={() => void currentWindow()?.minimize()}>
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M0 5.5h10" stroke="currentColor" />
        </svg>
      </button>
      <button
        type="button"
        className="caption-btn"
        title={maximized ? "Restore" : "Maximise"}
        aria-label={maximized ? "Restore" : "Maximise"}
        onClick={() => void currentWindow()?.toggleMaximize()}
      >
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true" fill="none" stroke="currentColor">
          {maximized ? (
            <>
              <rect x="0.5" y="2.5" width="7" height="7" />
              <path d="M2.5 2.5V.5h7v7h-2" />
            </>
          ) : (
            <rect x="0.5" y="0.5" width="9" height="9" />
          )}
        </svg>
      </button>
      <button type="button" className="caption-btn close" title="Close" aria-label="Close" onClick={() => void currentWindow()?.close()}>
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M0 0l10 10M10 0 0 10" stroke="currentColor" />
        </svg>
      </button>
    </div>
  );
}
