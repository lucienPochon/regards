import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * A `regards://` link opened from outside: agents give them
 * (`regards://message/42`) so the person can see what an agent found or did.
 * A link only ever opens a view; it never acts.
 */
export interface OpenLink {
  kind: "message" | "draft" | "hold";
  id: number;
}

/** The link Regards was started with, if any; once. Call when the UI is ready. */
export const takeLink = () => invoke<OpenLink | null>("link_take");

/** Links opened while Regards runs (it shows its window first). */
export const onOpenLink = (open: (link: OpenLink) => void): Promise<UnlistenFn> =>
  listen<OpenLink>("link_open", (event) => open(event.payload));
