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
| { kind: "message" | "draft" | "hold"; id: number }
  | {
      kind: "compose";
      to: string[];
      cc: string[];
      bcc: string[];
      subject: string;
      body: string;
    };

/** Whether Regards opens `mailto:` links; on macOS it cannot register itself (Mail's settings choose). */
export interface MailAppStatus {
  registered: boolean;
  can_register: boolean;
}

export const mailAppStatus = () => invoke<MailAppStatus>("mail_app_status");

/**
 * The person asked: open `mailto:` links in Regards. On Windows, Default apps
 * then opens to confirm it; on macOS this rejects with where to choose it.
 */
export const makeMailApp = () => invoke<void>("make_mail_app");

/** The link Regards was started with, if any; once. Call when the UI is ready. */
export const takeLink = () => invoke<OpenLink | null>("link_take");

/** Links opened while Regards runs (it shows its window first). */
export const onOpenLink = (open: (link: OpenLink) => void): Promise<UnlistenFn> =>
  listen<OpenLink>("link_open", (event) => open(event.payload));
