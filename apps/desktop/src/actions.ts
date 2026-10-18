import {
  type MoveResult,
  archiveMessages,
  deleteForever,
  moveMessages,
  setFlags,
  trashMessages,
} from "./accounts";
import { type Draft, forwardDraft, replyDraft } from "./compose";
import type { ConfirmRequest } from "./Confirm";
import type { IconName } from "./icons";
import type { Anchor } from "./Menu";
import type { Shortcuts } from "./shortcuts";

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
}

/** What actions need from the app around them. */
export interface ActionHost {
  /** Open the composer on a draft being started. */
  compose: (start: Promise<Draft>) => void;
  /** The messages left their folder; `result.undo` reverses it when it can be. */
  moved: (ids: number[], result: MoveResult | null) => void;
  failed: (error: unknown) => void;
  confirm: (request: ConfirmRequest) => Promise<boolean>;
  /** Ask which of the account's folders to move to (other than `current`): its id, or null if cancelled. */
  pickFolder: (accountId: string, current: number | null, anchor: Anchor | null) => Promise<number | null>;
}

export type ActionId = "reply" | "replyAll" | "forward" | "unread" | "read" | "flag" | "archive" | "move" | "delete";

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

/**
 * Everything that can be done to some messages. The reading pane's toolbar,
 * the list's hover buttons, the right-click menu and the keyboard shortcuts
 * are all made from this one list.
 */
export function messageActions(target: Target, host: ActionHost): Action[] {
  const { ids } = target;
  const flags = (change: { seen?: boolean; flagged?: boolean }) => {
    setFlags(ids, change).catch(host.failed);
  };
  const move = (start: () => Promise<MoveResult>) => {
    start()
      .then((result) => host.moved(ids, result))
      .catch(host.failed);
  };

  const actions: Action[] = [];
  if (target.single !== null) {
    const id = target.single;
    actions.push(
      { id: "reply", label: "Reply", icon: "reply", keys: ["r"], shown: true, run: () => host.compose(replyDraft(id, false)) },
      { id: "replyAll", label: "Reply all", icon: "replyAll", keys: ["a"], shown: true, run: () => host.compose(replyDraft(id, true)) },
      { id: "forward", label: "Forward", icon: "forward", keys: ["f"], shown: true, run: () => host.compose(forwardDraft(id)) },
    );
  }
  actions.push(
    { id: "unread", label: "Mark unread", icon: "mail", keys: ["Shift+U"], shown: target.seen, run: () => flags({ seen: false }) },
    { id: "read", label: "Mark read", icon: "mailOpen", keys: ["Shift+I"], shown: !target.seen, run: () => flags({ seen: true }) },
    {
      id: "flag",
      label: target.flagged ? "Unflag" : "Flag",
      icon: target.flagged ? "flagFilled" : "flag",
      keys: ["s"],
      shown: true,
      run: () => flags({ flagged: !target.flagged }),
    },
    { id: "archive", label: "Archive", icon: "archive", keys: ["e"], shown: true, run: () => move(() => archiveMessages(ids)) },
  );
  if (target.accountId !== null) {
    const account = target.accountId;
    actions.push({
      id: "move",
      label: "Move to…",
      icon: "move",
      keys: ["v"],
      shown: true,
      run: (anchor) => {
        void host.pickFolder(account, target.mailboxId, anchor ?? null).then((folder) => {
          if (folder !== null) move(() => moveMessages(ids, folder));
        });
      },
    });
  }
  actions.push({
    id: "delete",
    label: target.disposable ? "Delete forever" : "Delete",
    icon: "trash",
    keys: ["#", "Delete"],
    shown: true,
    danger: target.disposable,
    run: () => {
      if (!target.disposable) {
        move(() => trashMessages(ids));
        return;
      }
      void host
        .confirm({
          title: ids.length === 1 ? "Delete this message for good?" : `Delete ${ids.length} messages for good?`,
          body: `${ids.length === 1 ? "It is" : "They are"} removed from the server. This cannot be undone.`,
          confirm: "Delete forever",
          danger: true,
        })
        .then((ok) => {
          if (!ok) return;
          deleteForever(ids)
            .then(() => host.moved(ids, null))
            .catch(host.failed);
        });
    },
  });
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
