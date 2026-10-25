import { emitTo } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useRef, useState } from "react";
import { Composer } from "./Composer";
import { type SentElsewhere, getDraft } from "./compose";
import { type Anchor, Menu, type MenuEntry, type MenuRequest } from "./Menu";
import { getPrefs } from "./prefs";
import { reminderPresets, sendPresets, timeHint } from "./snooze";
import { TYPED_TIME_HINT, typedTimeEntries } from "./typedTime";
import { type Template, type Written, listTemplates, saveWritten } from "./templates";
import { TimePicker } from "./TimePicker";
import { PLATFORM, TitleBar } from "./TitleBar";
import { t } from "./i18n";

/**
 * A message written in a window of its own (Spark's "open in a new
 * window"): the composer alone, full size, with the window's own buttons and
 * its own menus. Sending closes it and the main window shows the undo;
 * closing it keeps the draft, as in the main window.
 */
export function ComposerWindow({ draftId }: { draftId: number }) {
  const [menu, setMenu] = useState<MenuRequest | null>(null);
  const [picking, setPicking] = useState<{
    title: string;
    confirm: string;
    initial: number;
    resolve: (time: number | null) => void;
  } | null>(null);
  const close = useCallback(() => {
    try {
      void getCurrentWindow().close();
    } catch {
      // The browser preview: a tab of its own.
      window.close();
    }
  }, []);
  // Closed by its frame (or Alt+F4), it saves what was just written first.
  const save = useRef<(() => Promise<boolean>) | null>(null);
  useEffect(() => {
    let frame: ReturnType<typeof getCurrentWindow> | null = null;
    try {
      frame = getCurrentWindow();
    } catch {
      return;
    }
    const stop = frame.onCloseRequested(async () => {
      await save.current?.().catch(() => false);
    });
    return () => void stop.then((unlisten) => unlisten()).catch(() => {});
  }, []);

  /** A palette page of times, with a way to pick any date (as the main window asks). */
  const pickTime = useCallback(
    (title: string, confirm: string, choices: { label: string; until: number | "someday" }[]) =>
      new Promise<number | null>((resolve) => {
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
              onSelect: () => resolve(typeof choice.until === "number" ? choice.until : null),
            })),
            "separator" as const,
            {
              label: t("Pick a date and time…"),
              icon: "plus" as const,
              onSelect: () => setPicking({ title, confirm, initial: Date.now() + 24 * 3600_000, resolve }),
            },
          ],
          onDismiss: () => resolve(null),
        });
      }),
    [],
  );
  const pickSendTime = useCallback(
    () => pickTime(t("Send at"), t("Schedule"), sendPresets(new Date(), getPrefs().sendPresets)),
    [pickTime],
  );
  const pickReminder = useCallback(
    () => pickTime(t("Remind me if no reply by"), t("Remind me"), reminderPresets(new Date(), getPrefs().reminderPresets)),
    [pickTime],
  );
  /** The templates to start from, and keeping what is written as one; editing them is in the main window's Settings. */
  const pickTemplate = useCallback(
    (_anchor: Anchor, written: Written) =>
      listTemplates().then(
        (templates) =>
          new Promise<Template | null>((resolve) => {
            const hasText = written.html.replace(/<[^>]*>/g, "").trim() !== "";
            const name = written.subject.trim() || "Untitled";
            const keep: MenuEntry[] = hasText
              ? [
                  {
                    label: t("Save this message as a template"),
                    icon: "plus",
                    detail: name,
                    onSelect: () => {
                      resolve(null);
                      void saveWritten(name, written);
                    },
                  },
                ]
              : [];
            setMenu({
              anchor: { x: 0, y: 0 },
              variant: "palette",
              title: t("Templates"),
              label: t("Insert a template"),
              filter: t("Type a template's name"),
              entries: [
                ...templates.map(
                  (template): MenuEntry => ({
                    label: template.name,
                    icon: "text",
                    detail: template.subject || undefined,
                    onSelect: () => resolve(template),
                  }),
                ),
                ...(templates.length > 0 && keep.length > 0 ? ["separator" as const] : []),
                ...keep,
              ],
              onDismiss: () => resolve(null),
            });
          }),
      ),
    [],
  );

  // As the main window: on Windows and macOS the page draws the top band, which drags the window.
  const framed = PLATFORM !== "linux";
  return (
    <div className={framed ? "composer-own framed" : "composer-own"}>
      {framed && <div className="drag-band" data-tauri-drag-region aria-hidden="true" />}
      <TitleBar />
      <Composer
        draftId={draftId}
        mode="full"
        onClose={close}
        onQueued={(queued) => {
          // The main window shows the undo, files a reply's conversation, and remembers the account.
          const tell = (sent: SentElsewhere) => emitTo("main", "composer_queued", sent).catch(() => undefined);
          void getDraft(draftId)
            .then(
              (draft) => tell({ queued, accountId: draft.account_id, replyingTo: draft.replying_to }),
              () => tell({ queued }),
            )
            .finally(close);
        }}
        pickSendTime={pickSendTime}
        pickReminder={pickReminder}
        pickTemplate={pickTemplate}
        saveRef={save}
      />
      {menu && <Menu request={menu} onClose={() => setMenu(null)} />}
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
