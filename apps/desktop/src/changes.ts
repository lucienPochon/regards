import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";

/**
 * Run `onChange` whenever the daemon reports a change (ADR-0007). Bursts are
 * coalesced, so a large sync triggers one refresh, not hundreds.
 */
export function useChanges(onChange: () => void): void {
  const latest = useRef(onChange);
  latest.current = onChange;

  useEffect(() => {
    let timer: number | undefined;
    const stop = listen("changes", () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => latest.current(), 100);
    });
    return () => {
      window.clearTimeout(timer);
      void stop.then((unlisten) => unlisten());
    };
  }, []);
}
