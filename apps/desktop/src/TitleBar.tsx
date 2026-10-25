import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useRef, useState } from "react";
import { Icon } from "./icons";
import { type Anchor, anchorOf } from "./Menu";
import { t } from "./i18n";

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

/** How long the maximise button is hovered before Windows 11 offers its Snap Layouts. */
const SNAP_AFTER = 450;

/**
 * On Windows the app draws its own caption buttons, over the top right of the
 * window: the app menu, minimise, maximise or restore, and close (which keeps
 * Regards in the tray when background mode is on). The band around them drags
 * the window. A message's own window has no app menu.
 */
export function TitleBar({ onAppMenu }: { onAppMenu?: (anchor: Anchor) => void }) {
  const [maximized, setMaximized] = useState(false);
  // Hovered a moment, the maximise button offers Windows' Snap Layouts (snap.rs).
  const snapTimer = useRef<number | null>(null);
  const stopSnap = () => {
    if (snapTimer.current !== null) window.clearTimeout(snapTimer.current);
    snapTimer.current = null;
  };
  const startSnap = () => {
    stopSnap();
    snapTimer.current = window.setTimeout(() => {
      snapTimer.current = null;
      invoke("snap_layouts").catch(() => {});
    }, SNAP_AFTER);
  };
  useEffect(() => stopSnap, []);
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
    <div className="caption" role="toolbar" aria-label={t("Window")}>
      {onAppMenu && (
        <button type="button" className="caption-btn" title={t("Menu")} aria-label={t("Menu")} aria-haspopup="menu" onClick={(event) => onAppMenu(anchorOf(event.currentTarget, "end"))}>
          <Icon name="chevronDown" size={13} />
        </button>
      )}
      <button type="button" className="caption-btn" title={t("Minimise")} aria-label={t("Minimise")} onClick={() => void currentWindow()?.minimize()}>
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M0 5.5h10" stroke="currentColor" />
        </svg>
      </button>
      <button
        type="button"
        className="caption-btn"
        title={maximized ? t("Restore") : t("Maximise")}
        aria-label={maximized ? t("Restore") : t("Maximise")}
        onClick={() => void currentWindow()?.toggleMaximize()}
        onPointerEnter={startSnap}
        onPointerLeave={stopSnap}
        onPointerDown={stopSnap}
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
      <button type="button" className="caption-btn close" title={t("Close")} aria-label={t("Close")} onClick={() => void currentWindow()?.close()}>
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M0 0l10 10M10 0 0 10" stroke="currentColor" />
        </svg>
      </button>
    </div>
  );
}
