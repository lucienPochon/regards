import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

/** Open a message's conversation in a window of its own, or bring that window to the front. */
export const openThreadWindow = (messageId: number, title: string) => invoke("thread_window", { messageId, title });

/** The message a conversation's own window was opened for (its label is `thread-<id>`); `null` elsewhere. */
export function threadWindowMessage(): number | null {
  let label = "";
  try {
    label = getCurrentWebviewWindow().label;
  } catch {
    // The preview: `?thread=<id>` stands in.
    label = `thread-${new URLSearchParams(window.location.search).get("thread") ?? ""}`;
  }
  const id = Number(label.match(/^thread-(\d+)$/)?.[1]);
  return Number.isInteger(id) && id > 0 ? id : null;
}

/** What a conversation's own window tells the main one: an undo to show there. */
export const UNDO_ELSEWHERE = "undo_elsewhere";
