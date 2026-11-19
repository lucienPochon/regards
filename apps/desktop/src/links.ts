import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * A link opened from outside. `regards://` ones (agents give them, such as
 * `regards://message/42`) open a view. A `mailto:` one, once Regards is the
 * mail app, asks for a new message: start a draft with its fields (from the
 * default account) and open it. A link never acts: nothing is sent.
 */
export type OpenLink =
  | { kind: "message" | "draft" | "hold"; id: number }
  /** A link copied in Regards (`regards://message-id/…`): whichever copy of the message is stored. */
  | { kind: "message_id"; message_id: string }
  | {
      kind: "compose";
      to: string[];
      cc: string[];
      bcc: string[];
      subject: string;
      body: string;
    };

/**
 * Whether a `mailto:` link opens Regards (on Windows, as Windows itself
 * decides: only once the person chose it in Default apps); on macOS it
 * cannot be asked from Regards (Mail's settings choose).
 */
export interface MailAppStatus {
  registered: boolean;
  can_register: boolean;
}

export const mailAppStatus = () => invoke<MailAppStatus>("mail_app_status");

/**
 * The person asked: make Regards the default mail app. On Windows, Default
 * apps then opens on Regards' page to choose it; on macOS this rejects with
 * where to choose it.
 */
export const makeMailApp = () => invoke<void>("make_mail_app");

/** The link Regards was started with, if any; once. Call when the UI is ready. */
export const takeLink = () => invoke<OpenLink | null>("link_take");

/** Links opened while Regards runs (it shows its window first). */
export const onOpenLink = (open: (link: OpenLink) => void): Promise<UnlistenFn> =>
  listen<OpenLink>("link_open", (event) => open(event.payload));
