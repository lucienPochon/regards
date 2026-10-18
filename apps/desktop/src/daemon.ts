import { invoke } from "@tauri-apps/api/core";

/** Mirrors `DaemonStatus` in `src-tauri/src/main.rs`. */
export interface DaemonStatus {
  running: boolean;
  endpoint: string;
  version: string | null;
  buildId: string | null;
  sameBuild: boolean | null;
  uptimeSecs: number | null;
  error: string | null;
}

export function fetchDaemonStatus(): Promise<DaemonStatus> {
  return invoke<DaemonStatus>("daemon_status");
}
