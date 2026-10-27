import type { MailboxSummary, MessageView, Role } from "./accounts";
import type { IconName } from "./icons";
import { t, tc } from "./i18n";
import type { Section } from "./smart";

/**
 * What the message column shows. A `role` view is that folder in every
 * account at once (all inboxes, all sent mail …); a `mailbox` view is one
 * folder of one account. Drafts and the Outbox are kept by the daemon, not on
 * the server. A `marked` view is what is pinned, snoozed or set aside, kept on
 * this device, across accounts (or the invitations to answer, from the
 * inbox). A `section` view is one Smart Inbox section
 * (Priority, Newsletters …) of every inbox, or of the one it was opened from.
 * `activity` is what agents did, as Settings › Agents also shows it.
 */
export type View =
  | { kind: "role"; role: Role }
  | { kind: "mailbox"; mailboxId: number; accountId: string }
  | { kind: "marked"; mark: MessageView }
  | { kind: "section"; section: Section; inbox?: { mailboxId: number; accountId: string } }
  | { kind: "drafts" }
  | { kind: "outbox" }
  | { kind: "activity" };

export const MARKED_LABELS: Record<MessageView, string> = {
  pinned: t("Pinned"),
  snoozed: t("Snoozed"),
  reminders: t("Reminders"),
  aside: t("Set aside"),
  invitations: t("Invitations"),
  invitation_replies: t("Invitation replies"),
};

export const MARKED_ICONS: Record<MessageView, IconName> = {
  pinned: "pin",
  snoozed: "clock",
  reminders: "bell",
  aside: "aside",
  invitations: "calendar",
  invitation_replies: "calendar",
};

/** The sidebar order, under the inbox. */
export const MARKED_VIEWS: MessageView[] = ["pinned", "snoozed", "reminders", "aside"];

export const INBOX: View = { kind: "role", role: "inbox" };

export const ROLE_LABELS: Record<Role, string> = {
  inbox: t("Inbox"),
  drafts: t("Drafts"),
  sent: t("Sent"),
  archive: tc("folder", "Archive"),
  all: t("All mail"),
  flagged: t("Flagged"),
  junk: t("Junk"),
  trash: t("Trash"),
};

export const ROLE_ICONS: Record<Role, IconName> = {
  inbox: "inbox",
  drafts: "draft",
  sent: "send",
  archive: "archive",
  all: "mail",
  flagged: "flag",
  junk: "junk",
  trash: "trash",
};

/**
 * The sidebar's main list, top to bottom. Drafts and the Outbox sit after the
 * inbox; the roles here are shown across accounts.
 */
export const MAIN_ROLES: Role[] = ["sent", "archive", "junk", "trash"];

/** Order of a single account's folders: its system folders first, as in the main list. */
const ROLE_ORDER: Role[] = ["inbox", "drafts", "sent", "archive", "all", "flagged", "junk", "trash"];

export function viewKey(view: View): string {
  switch (view.kind) {
    case "role":
      return `role:${view.role}`;
    case "mailbox":
      return `mailbox:${view.mailboxId}`;
    case "marked":
      return `marked:${view.mark}`;
    case "section":
      return `section:${view.section}${view.inbox ? `:${view.inbox.mailboxId}` : ""}`;
    default:
      return view.kind;
  }
}

export function sameView(a: View, b: View): boolean {
  return viewKey(a) === viewKey(b);
}

/** A folder's own name: the last part of its path ("Projects/2026" → "2026"). */
export function leafName(mailbox: MailboxSummary): string {
  if (mailbox.role) return ROLE_LABELS[mailbox.role];
  if (!mailbox.delimiter) return mailbox.name;
  const parts = mailbox.name.split(mailbox.delimiter);
  return parts[parts.length - 1] || mailbox.name;
}

/** How deep a folder sits in its account's tree, from 0. */
export function depth(mailbox: MailboxSummary): number {
  if (mailbox.role || !mailbox.delimiter) return 0;
  return Math.min(4, mailbox.name.split(mailbox.delimiter).length - 1);
}

/** System folders in their usual order, then the others in the server's order. */
export function sortFolders(mailboxes: MailboxSummary[]): MailboxSummary[] {
  const system = mailboxes
    .filter((mailbox) => mailbox.role !== null)
    .sort((a, b) => ROLE_ORDER.indexOf(a.role as Role) - ROLE_ORDER.indexOf(b.role as Role));
  const others = mailboxes.filter((mailbox) => mailbox.role === null);
  return [...system, ...others];
}
