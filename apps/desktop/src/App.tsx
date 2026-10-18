import { useCallback, useEffect, useState } from "react";
import { AccountsPanel } from "./AccountsPanel";
import { useChanges } from "./changes";
import { MessageList, type OpenMailbox } from "./MessageList";
import { ReadingPane } from "./ReadingPane";
import { ShortcutHelp } from "./ShortcutHelp";
import { Composer } from "./Composer";
import { OutboxPanel } from "./OutboxPanel";
import { SendToast } from "./SendToast";
import { UndoToast } from "./UndoToast";
import { useShortcuts } from "./shortcuts";
import type { UndoInfo } from "./accounts";
import { type Draft, type Queued, newDraft } from "./compose";
import { type DaemonStatus, fetchDaemonStatus } from "./daemon";

type Check =
  | { state: "checking" }
  | { state: "done"; status: DaemonStatus }
  | { state: "failed"; message: string };

export function App() {
  const [check, setCheck] = useState<Check>({ state: "checking" });
  const [open, setOpen] = useState<OpenMailbox | null>(null);
  const [reading, setReading] = useState<number | null>(null);
  const [toast, setToast] = useState<UndoInfo | null>(null);
  const closeToast = useCallback(() => setToast(null), []);
  const [help, setHelp] = useState(false);
  const [composing, setComposing] = useState<number | null>(null);
  const [queued, setQueued] = useState<Queued | null>(null);
  const closeQueued = useCallback(() => setQueued(null), []);
  const [composeError, setComposeError] = useState<string | null>(null);

  /** Open the composer on a draft the daemon is starting (new, reply, forward). */
  const compose = useCallback((start: Promise<Draft>) => {
    setComposeError(null);
    start.then((draft) => setComposing(draft.id)).catch((error: unknown) => setComposeError(String(error)));
  }, []);
  const writeNew = () => {
    if (open) compose(newDraft(open.accountId));
  };
  useShortcuts({ "?": () => setHelp(true), c: writeNew });

  // Without "Checking…" in between, so change events do not make the card flicker.
  const update = useCallback(() => {
    fetchDaemonStatus()
      .then((status) => setCheck({ state: "done", status }))
      .catch((error: unknown) => setCheck({ state: "failed", message: String(error) }));
  }, []);

  const refresh = useCallback(() => {
    setCheck({ state: "checking" });
    update();
  }, [update]);

  useEffect(() => {
    refresh();
    window.addEventListener("focus", update);
    return () => window.removeEventListener("focus", update);
  }, [refresh, update]);
  useChanges(update);

  return (
    <main className="shell">
      <header>
        <h1>Regards</h1>
        <p className="subtitle">Pre-alpha</p>
      </header>

      <section className="card" aria-live="polite">
        <h2>Background service</h2>
        <ServiceStatus check={check} />
        <button type="button" onClick={refresh} disabled={check.state === "checking"}>
          Check again
        </button>
      </section>

      <div className="toolbar">
        <button
          type="button"
          onClick={writeNew}
          disabled={!open}
          title={open ? `New message from ${open.account} (c)` : "Open a folder of the account to write from"}
        >
          New message
        </button>
        {composeError && <span className="error small">{composeError}</span>}
      </div>

      <div className="workspace">
        <div className="side">
          <AccountsPanel
            selected={open?.id ?? null}
            onOpen={(mailbox) => {
              setOpen(mailbox);
              setReading(null);
            }}
          />
          <OutboxPanel onEdit={setComposing} />
        </div>
        <div className="main">
          {open ? (
            <MessageList mailbox={open} selected={reading} onOpen={setReading} />
          ) : (
            <section className="card">
              <p className="muted">Choose a folder to see its messages.</p>
            </section>
          )}
          {reading !== null && open && (
            <ReadingPane
              messageId={reading}
              accountId={open.accountId}
              onOpenMessage={setReading}
              onCompose={compose}
              onMoved={(undo) => {
                setReading(null);
                setToast(undo);
              }}
              onClose={() => setReading(null)}
            />
          )}
        </div>
      </div>
      <div className="toasts">
        {toast && <UndoToast undo={toast} onDone={closeToast} />}
        {queued && (
          <SendToast
            key={queued.outbox_id}
            queued={queued}
            onDone={closeQueued}
            onUndone={(draftId) => {
              setQueued(null);
              if (draftId !== null) setComposing(draftId);
            }}
          />
        )}
      </div>
      {composing !== null && (
        <Composer
          key={composing}
          draftId={composing}
          onClose={() => setComposing(null)}
          onQueued={(result) => {
            setComposing(null);
            setQueued(result);
          }}
        />
      )}
      {help && <ShortcutHelp onClose={() => setHelp(false)} />}
      <p className="muted small hint">
        Press <kbd>?</kbd> for keyboard shortcuts.
      </p>
    </main>
  );
}

function ServiceStatus({ check }: { check: Check }) {
  if (check.state === "checking") {
    return <p className="muted">Checking…</p>;
  }
  if (check.state === "failed") {
    return <p className="error">Could not check the service: {check.message}</p>;
  }
  const { status } = check;
  if (status.error || !status.running) {
    return (
      <p className="error">
        <span className="dot off" aria-hidden="true" /> The service could not be started.{" "}
        {status.error}
      </p>
    );
  }
  return (
    <>
      <p>
        <span className="dot on" aria-hidden="true" /> Running · version {status.version} · pid{" "}
        {status.pid} · up {status.uptimeSecs ?? 0}s
      </p>
      <p className="muted">
        {status.desktopAccess ? "This app has desktop access." : "This app has agent access only."}
        {status.started && " Started by this app just now."}
        {status.replaced && " An older or separately started service was replaced."}
      </p>
    </>
  );
}
