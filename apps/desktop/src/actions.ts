import {
  type MarkResult,
  type MoveResult,
  archiveMessages,
  conversationIds,
  deleteForever,
  moveMessages,
  pinMessages,
  remindMessages,
  setAside,
  setFlags,
  trashMessages,
} from "./accounts";
import { type KeySet, getPrefs } from "./prefs";
import type { Person } from "./accounts";
import { type Draft, forwardDraft, replyDraft } from "./compose";
import type { ConfirmRequest } from "./Confirm";
import type { IconName } from "./icons";
import type { Anchor } from "./Menu";
import { type Shortcuts, keyText } from "./shortcuts";
import { snoozeMessages } from "./snooze";

/** The messages an action applies to, and what they have in common. */
export interface Target {
  ids: number[];
  /** The message, when there is exactly one: replying and forwarding need it. */
  single: number | null;
  /** The account they all belong to, if they do: moving needs it. */
  accountId: string | null;
  /** The folder they are all in, if they are: not offered as a place to move to. */
  mailboxId: number | null;
  /** All read. */
  seen: boolean;
  /** All flagged. */
  flagged: boolean;
  /** All in Trash or Junk, where deleting is for good. */
  disposable: boolean;
  /** All pinned. */
  pinned: boolean;
  /** All set aside. */
  aside: boolean;
  /** All snoozed. */
  snoozed: boolean;
  /** All waiting for a reply, with a reminder set. */
  reminding: boolean;
  /** Some are reminders that came due, shown in the inbox. */
  reminded: boolean;
  /** One newsletter or notification: Unsubscribe is offered, naming who sent it. */
  unsubscribe?: { from: Person | null } | null;
  /** All Done mail shown in the inbox (Show Done): Done takes them back to it. */
  done?: boolean;
  /**
   * Rows of the list, or the message open in the reader: Done, Delete, Move,
   * Junk and Mark read cover the rest of each one's conversation in its
   * folder too. Not for one message of a conversation (a card's ⋯).
   */
  conversation?: boolean;
}

/** What actions need from the app around them. */
export interface ActionHost {
  /** Open the composer on a draft being started. */
  compose: (start: Promise<Draft>) => void;
  /** The messages left their folder; `result.undo` reverses it when it can be. */
  moved: (ids: number[], result: MoveResult | null) => void;
  /** A mark changed (pin) and the messages stay where they are; `result.undo` reverses it. */
  marked: (result: MarkResult) => void;
  failed: (error: unknown) => void;
  confirm: (request: ConfirmRequest) => Promise<boolean>;
  /** Ask which of the account's folders to move to (other than `current`): its id, or null if cancelled. */
  pickFolder: (accountId: string, current: number | null, anchor: Anchor | null) => Promise<number | null>;
  /** Ask until when to snooze: a time (Unix ms), "someday", or null if cancelled. */
  pickSnooze: (anchor: Anchor | null) => Promise<number | "someday" | null>;
  /** Ask when to be reminded if nobody replies: a time (Unix ms), or null if cancelled. */
  pickReminder: (anchor: Anchor | null) => Promise<number | null>;
  /** The account's Junk folder, if it has one (for Mark as junk). */
  junkFolder?: (accountId: string) => number | null;
  /** Whether the account has labels (Gmail). */
  hasLabels?: (accountId: string) => boolean;
  /** Add or remove the account's labels on these messages. */
  pickLabels?: (ids: number[], accountId: string, anchor: Anchor | null) => void;
  /** Leave the mailing list a message came from, after asking. */
  unsubscribe?: (id: number, from: Person | null) => void;
  /** The account's inbox (for Back to inbox, on Done mail). */
  inboxFolder?: (accountId: string) => number | null;
}

export type ActionId =
  | "reply"
  | "replyAll"
  | "forward"
  | "unread"
  | "read"
  | "flag"
  | "pin"
  | "snooze"
  | "remind"
  | "aside"
  | "archive"
  | "move"
  | "junk"
  | "delete"
  | "unsubscribe"
  | "label";

export interface Action {
  id: ActionId;
  label: string;
  icon: IconName;
  /** The keyboard shortcuts that run it. */
  keys: string[];
  /** False for the half of a toggle that does not apply (Mark read on a read message). */
  shown: boolean;
  danger?: boolean;
  run: (anchor?: Anchor | null) => void;
}

/** Each action's keys in each key set (Settings › Shortcuts). */
const ACTION_KEYS: Record<KeySet, Record<ActionId, string[]>> = {
  standard: {
    reply: ["Shift+R"],
    replyAll: ["r"],
    forward: ["f"],
    unread: ["u"],
    read: ["u"],
    flag: ["i"],
    pin: ["d"],
    snooze: ["s"],
    remind: [],
    aside: ["g"],
    archive: ["e"],
    move: ["Mod+Shift+M"],
    junk: ["Shift+1"],
    delete: ["Delete", "Backspace"],
    unsubscribe: [],
    label: ["l"],
  },
  classic: {
    reply: ["r"],
    replyAll: ["a"],
    forward: ["f"],
    unread: ["Shift+U"],
    read: ["Shift+I"],
    flag: ["s"],
    pin: ["p"],
    snooze: ["b"],
    remind: ["Shift+R"],
    aside: ["Shift+A"],
    archive: ["e"],
    move: ["v"],
    junk: ["!"],
    delete: ["#", "Delete"],
    unsubscribe: [],
    label: ["l"],
  },
};

/** An action's name with its first key, for a tooltip. */
export const actionTitle = (action: Action) =>
  action.keys.length > 0 ? `${action.label} (${keyText(action.keys[0])})` : action.label;

/**
 * Everything that can be done to some messages. The reading pane's toolbar,
 * the list's hover buttons, the right-click menu and the keyboard shortcuts
 * are all made from this one list.
 */
export function messageActions(target: Target, host: ActionHost): Action[] {
  const { ids } = target;
  // What leaves the folder or is marked read: with the rest of each conversation
  // there, so a Done leaves nothing of it behind. `host.moved` still gets the rows.
  const all = (): Promise<number[]> => (target.conversation ? conversationIds(ids) : Promise.resolve(ids));
  const flags = (change: { seen?: boolean; flagged?: boolean }) => {
    (change.seen === undefined ? Promise.resolve(ids) : all()).then((list) => setFlags(list, change)).catch(host.failed);
  };
  const move = (start: () => Promise<MoveResult>) => {
    start()
      .then((result) => host.moved(ids, result))
      .catch(host.failed);
  };

  const keys = ACTION_KEYS[getPrefs().keySet];
  // A key that both marks read and unread belongs to the half that shows.
  const half = (id: "unread" | "read", shown: boolean) =>
    shown ? keys[id] : keys[id].filter((key) => !keys[id === "read" ? "unread" : "read"].includes(key));

  const actions: Action[] = [];
  if (target.single !== null) {
    const id = target.single;
    actions.push(
      { id: "reply", label: "Reply", icon: "reply", keys: keys.reply, shown: true, run: () => host.compose(replyDraft(id, false)) },
      { id: "replyAll", label: "Reply all", icon: "replyAll", keys: keys.replyAll, shown: true, run: () => host.compose(replyDraft(id, true)) },
      { id: "forward", label: "Forward", icon: "forward", keys: keys.forward, shown: true, run: () => host.compose(forwardDraft(id)) },
    );
  }
  actions.push(
    // Drawn as the row's dot: a ring on read mail, filled on unread mail.
    { id: "unread", label: "Mark unread", icon: "circle", keys: half("unread", target.seen), shown: target.seen, run: () => flags({ seen: false }) },
    { id: "read", label: "Mark read", icon: "circleFilled", keys: half("read", !target.seen), shown: !target.seen, run: () => flags({ seen: true }) },
    {
      id: "flag",
      label: target.flagged ? "Unflag" : "Flag",
      icon: target.flagged ? "flagFilled" : "flag",
      keys: keys.flag,
      shown: true,
      run: () => flags({ flagged: !target.flagged }),
    },
    {
      id: "pin",
      label: target.pinned ? "Unpin" : "Pin",
      icon: target.pinned ? "pinFilled" : "pin",
      keys: keys.pin,
      shown: true,
      run: () => {
        pinMessages(ids, !target.pinned).then(host.marked).catch(host.failed);
      },
    },
    {
      id: "snooze",
      label: target.snoozed ? "Unsnooze" : "Snooze…",
      icon: "clock",
      keys: keys.snooze,
      shown: true,
      run: (anchor) => {
        // Snoozed or set aside, a message leaves the inbox: as for a move, the next one opens.
        const left = (result: MarkResult) => host.moved(ids, { moved: result.changed, undo: result.undo });
        if (target.snoozed) {
          snoozeMessages(ids, null).then(host.marked).catch(host.failed);
          return;
        }
        void host.pickSnooze(anchor ?? null).then((until) => {
          if (until !== null) snoozeMessages(ids, until).then(left).catch(host.failed);
        });
      },
    },
    {
      id: "remind",
      label: target.reminding ? "No reminder" : "Remind me…",
      icon: "bell",
      keys: keys.remind,
      shown: true,
      run: (anchor) => {
        if (target.reminding) {
          remindMessages(ids, null).then(host.marked).catch(host.failed);
          return;
        }
        // A reminder that came due leaves the inbox again until the new time.
        const after = (result: MarkResult) =>
          target.reminded ? host.moved(ids, { moved: result.changed, undo: result.undo }) : host.marked(result);
        void host.pickReminder(anchor ?? null).then((at) => {
          if (at !== null) remindMessages(ids, at).then(after).catch(host.failed);
        });
      },
    },
    {
      id: "aside",
      label: target.aside ? "Back to inbox" : "Set aside",
      icon: "aside",
      keys: keys.aside,
      shown: true,
      run: () => {
        setAside(ids, !target.aside)
          .then((result) =>
            target.aside ? host.marked(result) : host.moved(ids, { moved: result.changed, undo: result.undo }),
          )
          .catch(host.failed);
      },
    },
    {
      id: "archive",
      label: "Archive",
      icon: "archive",
      keys: keys.archive,
      shown: true,
      // Settings › General: archiving a pinned message can also unpin it.
      run: () =>
        move(() =>
          target.pinned && getPrefs().pinnedArchive === "unpin"
            ? pinMessages(ids, false).then(all).then(archiveMessages)
            : all().then(archiveMessages),
        ),
    },
  );
  if (target.accountId !== null) {
    const account = target.accountId;
    actions.push({
      id: "move",
      label: "Move to…",
      icon: "move",
      keys: keys.move,
      shown: true,
      run: (anchor) => {
        void host.pickFolder(account, target.mailboxId, anchor ?? null).then((folder) => {
          if (folder !== null) move(() => all().then((list) => moveMessages(list, folder)));
        });
      },
    });
    // Junk is a move to the account's Junk folder, when it has one and they are not there already.
    const junk = host.junkFolder?.(account) ?? null;
    if (junk !== null && junk !== target.mailboxId) {
      actions.push({
        id: "junk",
        label: "Mark as junk",
        icon: "junk",
        keys: keys.junk,
        shown: true,
        run: () => move(() => all().then((list) => moveMessages(list, junk))),
      });
    }
  }
  // Gmail: labels, each a copy in the label's folder (ADR-0009 §6).
  if (target.accountId !== null && host.hasLabels?.(target.accountId) && host.pickLabels) {
    const [account, pick] = [target.accountId, host.pickLabels];
    actions.push({ id: "label", label: "Label…", icon: "tag", keys: keys.label, shown: true, run: (anchor) => pick(ids, account, anchor ?? null) });
  }
  if (target.single !== null && target.unsubscribe && host.unsubscribe) {
    const [id, from, leave] = [target.single, target.unsubscribe.from, host.unsubscribe];
    actions.push({ id: "unsubscribe", label: "Unsubscribe…", icon: "unsubscribe", keys: keys.unsubscribe, shown: true, run: () => leave(id, from) });
  }
  actions.push({
    id: "delete",
    label: target.disposable ? "Delete forever" : "Delete",
    icon: "trash",
    keys: keys.delete,
    shown: true,
    danger: target.disposable,
    run: () => {
      if (!target.disposable) {
        move(() => all().then(trashMessages));
        return;
      }
      // Counted with the rest of their conversations, so the question says how many go.
      void all()
        .then(async (list) => {
          const ok = await host.confirm({
            title: list.length === 1 ? "Delete this message for good?" : `Delete ${list.length} messages for good?`,
            body: `${list.length === 1 ? "It is" : "They are"} removed from the server. This cannot be undone.`,
            confirm: "Delete forever",
            danger: true,
          });
          if (!ok) return;
          await deleteForever(list);
          host.moved(ids, null);
        })
        .catch(host.failed);
    },
  });
  // Show Done: on Done mail, Done (E) takes it back to the inbox.
  const inbox = target.done && target.accountId !== null ? (host.inboxFolder?.(target.accountId) ?? null) : null;
  const archive = actions.findIndex((action) => action.id === "archive");
  if (inbox !== null && archive !== -1) {
    actions[archive] = { ...actions[archive], label: "Back to inbox", icon: "inbox", run: () => move(() => moveMessages(ids, inbox)) };
  }
  return actions;
}

/** The shown actions with these ids, in this order. */
export function choose(actions: Action[], ...ids: ActionId[]): Action[] {
  return ids.flatMap((id) => actions.filter((action) => action.id === id && action.shown));
}

/** The actions' keyboard shortcuts, for `useShortcuts`. */
export function actionKeys(actions: Action[]): Shortcuts {
  const keys: Shortcuts = {};
  for (const action of actions) {
    for (const key of action.keys) keys[key] = () => action.run(null);
  }
  return keys;
}
