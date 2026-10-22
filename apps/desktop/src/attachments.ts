import { invoke } from "@tauri-apps/api/core";

// Received attachments. Each is named by its message and its place in
// `MessageDetail.attachments`, inline ones included. Paths stay on the Rust
// side: the system's dialogs run there and only file names come back.

/** Mirrors `AttachmentPreview` in `regards-ipc`. */
export type AttachmentPreview =
  /** Show with an `<img>` only (see `imageSource`): an SVG then runs no script and loads nothing. */
  | { kind: "image"; /** From the image's own bytes. */ mime: string; /** Base64. */ data: string }
  /** Show as text, never as HTML. Only the first megabyte when `truncated`. */
  | { kind: "text"; text: string; truncated: boolean };

/**
 * What the app can show of an attachment whose `preview` is set. Fails for
 * an image over 10 MB ("too large to show here").
 */
export function previewAttachment(messageId: number, index: number): Promise<AttachmentPreview> {
  return invoke<AttachmentPreview>("attachment_preview", { messageId, index });
}

/** The `src` for an image preview. */
export function imageSource(preview: Extract<AttachmentPreview, { kind: "image" }>): string {
  return `data:${preview.mime};base64,${preview.data}`;
}

/**
 * Save an attachment in the system's save dialog, which starts in Downloads
 * with `name` (made safe). The name it was saved under, or null if the
 * person cancelled. The file is marked as from the Internet, as browsers
 * mark downloads.
 */
export function saveAttachment(messageId: number, index: number, name: string): Promise<string | null> {
  return invoke<string | null>("attachment_save", { messageId, index, name });
}

/**
 * Save the files attached (not the images shown inside the message) in a
 * folder the person picks, never replacing a file: "report.pdf" becomes
 * "report (2).pdf". The names they were saved under, or null if cancelled.
 */
export function saveAllAttachments(messageId: number): Promise<string[] | null> {
  return invoke<string[] | null>("attachment_save_all", { messageId });
}

/**
 * Open an attachment with its program, from a read-only copy (removed when
 * the daemon next starts). Ask first when its risk is "warn"; one that is
 * "save_only" is refused.
 */
export function openAttachment(messageId: number, index: number): Promise<void> {
  return invoke<void>("attachment_open", { messageId, index });
}
