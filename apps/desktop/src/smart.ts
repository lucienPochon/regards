import { invoke } from "@tauri-apps/api/core";
import type { MoveResult } from "./accounts";

/**
 * The Smart Inbox (plan §5.4): the inbox by section, why a message is where
 * it is, and the person's choices for senders. The daemon sorts every message
 * by deterministic rules; nothing leaves the device.
 *
 * Wherever an address goes, a whole domain can go as `@example.org`; a choice
 * for an address wins over its domain's.
 */

/** In the order they are shown. */
export type Section = "priority" | "people" | "new_senders" | "notifications" | "newsletters";
export const SECTIONS: Section[] = ["priority", "people", "new_senders", "notifications", "newsletters"];
export const SECTION_LABELS: Record<Section, string> = {
  priority: "Priority",
  people: "People",
  new_senders: "New senders",
  notifications: "Notifications",
  newsletters: "Newsletters",
};

/** Where the person said a sender's mail goes. */
export type Policy = "people" | "notifications" | "newsletters";

/** The rule that placed a message: see `reasonText`. */
export type Reason =
  | "your_choice"
  | "priority_sender"
  | "from_you"
  | "delivery_report"
  | "sent_automatically"
  | "no_reply_address"
  | "mailing_list"
  | "bulk_mail"
  | "bulk_sender"
  | "you_wrote_to_them"
  | "new_sender";

/** How new senders' mail is shown: with People, in its own section, or held out of the inbox until accepted. */
export type Screening = "off" | "section" | "hold";

export interface SectionCount {
  section: Section;
  total: number;
  unread: number;
}

/** What is known about a sender, for one account. */
export interface Sender {
  address: string;
  /** The section chosen for them (or their domain), if any. */
  policy: Policy | null;
  priority: boolean;
  /** The account has written to them. */
  written_to: boolean;
  /** Blocked, by address or domain: their mail goes to Junk. */
  blocked: boolean;
}

export interface Explanation {
  section: Section;
  reason: Reason;
  /** None for a message without a sender's address. */
  sender: Sender | null;
}

/**
 * The inbox's sections that hold mail, with counts, in order. Without
 * screening, new senders count with People. For each section's messages, use
 * `listMessages(inbox, before, limit, section)`.
 */
export const smartOverview = (accountId: string) =>
  invoke<{ sections: SectionCount[] }>("smart_overview", { accountId }).then((result) => result.sections);

/** "Why is this here?" */
export const explain = (messageId: number) => invoke<Explanation>("message_explain", { id: messageId });

/**
 * The person's choice for a sender: a section (or `null` to let the rules
 * decide) and whether they are a priority. Their mail, old and new, follows.
 * Accepting a new sender is `{ policy: "people" }`.
 */
export const setSender = (accountId: string, address: string, choice: { policy: Policy | null; priority: boolean }) =>
  invoke<{ changed: number }>("sender_set", {
    sender: { account_id: accountId, address, policy: choice.policy, priority: choice.priority },
  });

/**
 * Block a sender or `@domain`: their inbox mail goes to Junk, and what they
 * send later too. Undoable like a move (it also unblocks). `blocked: false`
 * unblocks and moves nothing back.
 */
export const blockSender = (accountId: string, address: string, blocked = true) =>
  invoke<MoveResult>("sender_block", { sender: { account_id: accountId, address, blocked } });

export const screening = () => invoke<{ mode: Screening }>("smart_screening").then((result) => result.mode);
export const setScreening = (mode: Screening) => invoke<{ mode: Screening }>("smart_set_screening", { mode });

/** The answer to "why is this here?", in a sentence. */
export function reasonText(explanation: Explanation): string {
  const who = explanation.sender?.address ?? "the sender";
  switch (explanation.reason) {
    case "your_choice":
      return `You chose ${SECTION_LABELS[explanation.section]} for ${who}.`;
    case "priority_sender":
      return `${who} is one of your priority senders.`;
    case "from_you":
      return "You sent it.";
    case "delivery_report":
      return "It's a delivery report from a mail server.";
    case "sent_automatically":
      return "A program sent it, not a person.";
    case "no_reply_address":
      return `It comes from ${who}, an address that takes no replies.`;
    case "mailing_list":
      return "It came through a mailing list, or has an unsubscribe link.";
    case "bulk_mail":
      return "It's marked as bulk mail.";
    case "bulk_sender":
      return "It was sent through a bulk-mail service.";
    case "you_wrote_to_them":
      return `You have written to ${who}.`;
    case "new_sender":
      return `You haven't written to ${who} yet.`;
  }
}
