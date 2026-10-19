import { invoke } from "@tauri-apps/api/core";

/** Mirrors `DaemonStatus` in `src-tauri/src/main.rs`. */
export interface DaemonStatus {
  running: boolean;
  endpoint: string;
  version: string | null;
  buildId: string | null;
  pid: number | null;
  uptimeSecs: number | null;
  /** Whether this app holds desktop access (the `ui` principal). */
  desktopAccess: boolean;
  /** Whether this check started the service. */
  started: boolean;
  /** Whether a service from another build, or started elsewhere, was replaced. */
  replaced: boolean;
  error: string | null;
}

/** Connects to the background service, starting it if needed. */
export function fetchDaemonStatus(): Promise<DaemonStatus> {
  return invoke<DaemonStatus>("daemon_status");
}
