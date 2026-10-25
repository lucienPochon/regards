import { emitTo } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useMemo, useState } from "react";
import { type MailboxSummary, type UndoInfo, getMessage } from "./accounts";
import { type ActionHost, type Target, choose, messageActions } from "./actions";
import { openComposerWindow } from "./compose";
import { Confirm, type ConfirmRequest } from "./Confirm";
import type { ListItem } from "./mail";
import { useMail } from "./mail";
import { Menu, type MenuEntry, type MenuRequest } from "./Menu";
import { getPrefs } from "./prefs";
import { ReadingPane } from "./ReadingPane";
import { recentFolders, rememberFolder } from "./recent-folders";
import { presets, reminderPresets, timeHint } from "./snooze";
import { UNDO_ELSEWHERE } from "./thread-window";
import { TimePicker } from "./TimePicker";
import { PLATFORM, TitleBar } from "./TitleBar";
import { TYPED_TIME_HINT, typedTimeEntries } from "./typedTime";
import { ROLE_ICONS, depth, leafName, sortFolders } from "./views";

/** The palette's message actions in a conversation's own window, in order. */
const ACTIONS = ["archive", "snooze", "pin", "aside", "flag", "read", "unread", "move", "copy", "junk", "delete", "reply", "replyAll", "forward", "remind", "mute"] as const;

type Picking = { title: string; confirm: string; initial: number; resolve: (time: number | null) => void };

/**
 * A conversation read in a window of its own (Spark's "open in a new
 * window"): the reader alone, with the window's own buttons and menus.
 * Replying opens the composer in a window too; what takes the message
 * away (Done, Delete, Move) closes this one, and the main window shows the
 * undo.
 */
export function ThreadWindow({ messageId }: { messageId: number }) {
  const mail = useMail();
  const accounts = useMemo(() => (mail.state === "ready" ? mail.accounts : []), [mail]);
  const [item, setItem] = useState<ListItem | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [menu, setMenu] = useState<MenuRequest | null>(null);
  const [asking, setAsking] = useState<{ request: ConfirmRequest; resolve: (ok: boolean) => void } | null>(null);
  const [picking, setPicking] = useState<Picking | null>(null);

  const close = useCallback(() => {
    try {
      void getCurrentWindow().close();
    } catch {
      // The browser preview: a tab of its own.
      window.close();
    }
  }, []);

  // The message, and where it is: once the accounts and their folders are known.
  useEffect(() => {
    if (accounts.length === 0) return;
    getMessage(messageId).then(
      (detail) => {
        const owner = accounts.find((entry) => entry.mailboxes.some((m) => m.id === detail.mailbox_id));
        const folder = owner?.mailboxes.find((m) => m.id === detail.mailbox_id);
        if (!owner || !folder) {
          setProblem("That message is no longer on this computer.");
          return;
        }
        setItem({ ...detail.message, mailboxId: folder.id, accountId: owner.account.id, role: folder.role });
        try {
          void getCurrentWindow().setTitle(detail.message.subject || "(no subject)");
        } catch {
          document.title = detail.message.subject || "(no subject)";
        }
      },
      () => setProblem("That message is no longer on this computer."),
    );
  }, [messageId, accounts]);

  const folders: MailboxSummary[] = useMemo(
    () => accounts.find((entry) => entry.account.id === item?.accountId)?.mailboxes ?? [],
    [accounts, item?.accountId],
  );
  const tellUndo = (undo: UndoInfo | null | undefined) => {
    if (undo) void emitTo("main", UNDO_ELSEWHERE, undo).catch(() => {});
  };

  /** A palette page of times, with a way to pick any date (as the main window asks). */
  const pickTime = (title: string, confirm: string, choices: { label: string; until: number | "someday" }[]) =>
    new Promise<number | "someday" | null>((resolve) => {
      setMenu({
        anchor: { x: 0, y: 0 },
        variant: "palette",
        title,
        label: title,
        filter: TYPED_TIME_HINT,
        typed: (query) => typedTimeEntries(query, resolve),
        entries: [
          ...choices.map((choice) => ({
            label: choice.label,
            icon: "clock" as const,
            hint: timeHint(choice.until),
            onSelect: () => resolve(choice.until),
          })),
          "separator" as const,
          {
            label: "Pick a date and time…",
            icon: "plus" as const,
            onSelect: () => setPicking({ title, confirm, initial: Date.now() + 24 * 3600_000, resolve }),
          },
        ],
        onDismiss: () => resolve(null),
      });
    });

  const host: ActionHost = {
    compose: (start) => {
      start.then((draft) => openComposerWindow(draft.id, draft.subject)).catch((error: unknown) => setProblem(String(error)));
    },
    moved: (ids, result) => {
      tellUndo(result?.undo);
      if (item && ids.includes(item.id)) close();
    },
    marked: (result) => tellUndo(result.undo),
    failed: (error) => setProblem(String(error)),
    confirm: (request) => new Promise<boolean>((resolve) => setAsking({ request, resolve })),
    pickFolder: (accountId, current, _anchor, purpose = "move") =>
      new Promise<number | null>((resolve) => {
        const choices = folders.filter((folder) => folder.selectable && folder.id !== current);
        const pick = (id: number) => {
          rememberFolder(accountId, id);
          resolve(id);
        };
        const icon = (folder: MailboxSummary) => (folder.role ? ROLE_ICONS[folder.role] : "folder");
        const recent = recentFolders(accountId).flatMap((id) => choices.filter((folder) => folder.id === id));
        setMenu({
          anchor: { x: 0, y: 0 },
          variant: "palette",
          title: purpose === "copy" ? "Copy to" : "Move to",
          label: purpose === "copy" ? "Copy to folder" : "Move to folder",
          filter: "Type a folder name",
          entries: [
            ...recent.map((folder) => ({ label: leafName(folder), detail: "Recent", icon: icon(folder), quick: true, onSelect: () => pick(folder.id) })),
            ...(recent.length > 0 ? ["separator" as const] : []),
            ...sortFolders(choices).map((folder) => ({ label: leafName(folder), icon: icon(folder), depth: depth(folder), onSelect: () => pick(folder.id) })),
          ],
          onDismiss: () => resolve(null),
        });
      }),
    copied: () => setProblem(null),
    pickSnooze: () => pickTime("Snooze until", "Snooze", presets(new Date(), getPrefs().snoozePresets)),
    pickReminder: () =>
      pickTime("Remind me if no reply by", "Remind me", reminderPresets(new Date(), getPrefs().reminderPresets)).then((time) =>
        typeof time === "number" ? time : null,
      ),
    junkFolder: () => folders.find((folder) => folder.role === "junk")?.id ?? null,
    inboxFolder: () => folders.find((folder) => folder.role === "inbox")?.id ?? null,
  };
  const actionsFor = (target: Target) => messageActions(target, host);
  const palette = (actions: ReturnType<typeof actionsFor>, title: string, extra: MenuEntry[] = []) =>
    setMenu({
      anchor: { x: 0, y: 0 },
      variant: "palette",
      title,
      label: title,
      filter: "Type an action",
      entries: [
        ...choose(actions, ...ACTIONS).map(
          (action): MenuEntry => ({
            label: action.label,
            icon: action.icon,
            keys: action.keys[0],
            danger: action.danger,
            onSelect: () => action.run(null),
          }),
        ),
        ...(extra.length > 0 ? ["separator" as const, ...extra] : []),
      ],
    });

  // As the main window: on Windows and macOS the page draws the top band, which drags the window.
  const framed = PLATFORM !== "linux";
  return (
    <div className={framed ? "thread-own framed" : "thread-own"}>
      {framed && <div className="drag-band" data-tauri-drag-region aria-hidden="true" />}
      <TitleBar />
      {problem && (
        <p className="list-error thread-own-problem" role="alert">
          {problem}
        </p>
      )}
      <div className="reader-column">
        {item ? (
          <ReadingPane
            key={item.id}
            item={item}
            folders={folders}
            mode="split"
            actionsFor={actionsFor}
            onPalette={palette}
            onClose={close}
            onDraft={(id) => void openComposerWindow(id, "")}
          />
        ) : (
          !problem && <div className="empty" aria-busy="true" />
        )}
      </div>
      {menu && <Menu request={menu} onClose={() => setMenu(null)} />}
      {asking && (
        <Confirm
          request={asking.request}
          onAnswer={(ok) => {
            asking.resolve(ok);
            setAsking(null);
          }}
        />
      )}
      {picking && (
        <TimePicker
          title={picking.title}
          confirm={picking.confirm}
          initial={picking.initial}
          onAnswer={(time) => {
            picking.resolve(time);
            setPicking(null);
          }}
        />
      )}
    </div>
  );
}
